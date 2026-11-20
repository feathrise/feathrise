//! 将工程 World 的只读对象快照绑定到当前视口会话和已采用的场景。

use super::{
    resources::{EmbeddedStatus, Event, Outgoing, Runtime},
    systems::send,
};
use bevy::prelude::*;
use feathrise_editor_protocol::{Envelope, Message, PreviewKind, SessionIdentity};
use feathrise_editor_scene::standalone::{
    embedded_scene_tree::{self, ProjectSceneTreeRow},
    embedded_snapshot::EmbeddedSnapshotState,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

// 可选实时预览格式由工程 SDK 实现，沿用公开协议库已有的 Preview 信封。
pub(super) const CAPABILITY: &str = "scene-tree-v1";
const REQUEST: &str = "feathrise-scene-tree-request-v1";
pub(super) const RESPONSE: &str = "feathrise-scene-tree-v1";
const EMPTY_SCENE: &str = "__empty_world__";

#[derive(Debug, Default)]
pub(super) struct SceneTreeHost {
    pub supported: bool,
    scope: Option<SceneTreeScope>,
    next_id: u64,
    flight: Option<(u64, u64)>,
    version: Option<u64>,
    refresh_after: Option<Instant>,
}

#[derive(Debug)]
struct SceneTreeScope {
    identity: SessionIdentity,
    world_id: u64,
    scene_id: Option<String>,
}

pub(super) struct Response {
    identity: SessionIdentity,
    scene_id: String,
    request_id: u64,
    reply: Option<TreeReply>,
}

impl Response {
    pub(super) fn retire(self, world: &mut World) {
        if let Some(rows) = self.reply.and_then(|reply| reply.rows) {
            embedded_scene_tree::retire_rows(world, rows);
        }
    }
}

struct TreeReply {
    world_id: u64,
    revision: u64,
    version: Option<u64>,
    rows: Option<Arc<Vec<ProjectSceneTreeRow>>>,
}

#[derive(serde::Deserialize)]
struct WireReply {
    world_id: u64,
    revision: u64,
    version: Option<u64>,
    rows: Option<Vec<WireRow>>,
}

#[derive(serde::Deserialize)]
struct WireRow {
    depth: u32,
    index: u32,
    name: Option<String>,
    kind: Kind,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    MainCamera,
    Camera,
    Light,
    Scene,
    Mesh,
    Node,
    OffscreenCamera,
}

/// 在通信 worker 解码和核对完整层级；作者帧只采用共享行表。
#[cfg(windows)]
pub(super) fn decode(frame: Envelope) -> Event {
    let Message::Preview {
        kind: PreviewKind::AuthorDomain,
        sequence,
        scene_id,
        ref format,
    } = frame.message
    else {
        return Event::Protocol(frame);
    };
    debug_assert_eq!(format, RESPONSE);
    let reply = frame.payload.and_then(|bytes| {
        let reply: WireReply = serde_json::from_slice(&bytes).ok()?;
        let rows = if let Some(nodes) = reply.rows {
            reply.version?;
            let mut next_depth = 0;
            let mut rows = Vec::with_capacity(nodes.len());
            for node in nodes {
                if node.depth > next_depth {
                    return None;
                }
                next_depth = node.depth.saturating_add(1);
                let category = match node.kind {
                    Kind::MainCamera => ["主相机", "Main camera"],
                    Kind::Camera => ["相机", "Camera"],
                    Kind::OffscreenCamera => ["离屏相机", "Offscreen camera"],
                    Kind::Light => ["灯光", "Light"],
                    Kind::Scene => ["场景实例", "Scene instance"],
                    Kind::Mesh => ["渲染对象", "Renderable"],
                    Kind::Node => ["节点", "Node"],
                };
                rows.push(ProjectSceneTreeRow {
                    depth: node.depth,
                    category,
                    name: node.name,
                    index: node.index,
                });
            }
            Some(Arc::new(rows))
        } else {
            None
        };
        Some(TreeReply {
            world_id: reply.world_id,
            revision: reply.revision,
            version: reply.version,
            rows,
        })
    });
    Event::SceneTree(Response {
        identity: frame.identity,
        scene_id,
        request_id: sequence,
        reply,
    })
}

pub(super) fn receive(
    world: &mut World,
    runtime: &mut Runtime,
    status: &EmbeddedStatus,
    response: Response,
) {
    let tree = &mut runtime.tree;
    let Some((id, revision)) = tree.flight else {
        response.retire(world);
        return;
    };
    if !tree.supported
        || runtime.identity.as_ref() != Some(&response.identity)
        || runtime.scene_id.as_deref().unwrap_or(EMPTY_SCENE) != response.scene_id
        || response.request_id != id
    {
        response.retire(world);
        return;
    }
    // 匹配的信封已结清本次查询。正文身份错误时保留旧树，允许下一轮刷新。
    tree.flight = None;
    if revision != status.applied_revision || runtime.snapshot_in_flight.is_some() || status.playing
    {
        response.retire(world);
        return;
    }
    tree.refresh_after = Some(Instant::now() + Duration::from_secs(1));
    let current = response.reply.as_ref().is_some_and(|reply| {
        reply.world_id == status.world_id && reply.revision == revision
    });
    match response.reply {
        Some(TreeReply {
            version: Some(version),
            rows: Some(rows),
            ..
        }) if current => {
            tree.version = Some(version);
            embedded_scene_tree::publish(world, rows);
        }
        Some(TreeReply {
            version: Some(version),
            rows: None,
            ..
        }) if current && Some(version) == tree.version => (),
        // 可选诊断失败时保留上一份完整层级，后续更新可自然恢复。
        reply => {
            if let Some(rows) = reply.and_then(|reply| reply.rows) {
                embedded_scene_tree::retire_rows(world, rows);
            }
            embedded_scene_tree::unavailable(world);
        }
    }
}

pub(super) fn sync(
    world: &mut World,
    runtime: &mut Runtime,
    status: &mut EmbeddedStatus,
    active: bool,
    visible: bool,
) {
    let author = world.resource::<EmbeddedSnapshotState>();
    let scope = runtime
        .identity
        .as_ref()
        .filter(|_| {
            active
                && runtime.tree.supported
                && status.ready
                && status.error.is_none()
                && !status.playing
                && author.scene_id == runtime.scene_id
        })
        .map(|identity| (identity, status.world_id, runtime.scene_id.as_deref()));
    let previous = runtime.tree.scope.as_ref().map(|scope| {
        (&scope.identity, scope.world_id, scope.scene_id.as_deref())
    });
    // 空工程首次采用场景时可以沿用当前代次，仍须撤回旧场景的查询与行表。
    if scope != previous {
        runtime.tree.scope = scope.map(|(identity, world_id, scene_id)| SceneTreeScope {
            identity: identity.clone(),
            world_id,
            scene_id: scene_id.map(str::to_owned),
        });
        runtime.tree.flight = None;
        runtime.tree.version = None;
        runtime.tree.refresh_after = None;
        embedded_scene_tree::clear(world);
    }
    if scope.is_none()
        || !visible
        || runtime.tree.flight.is_some()
        || runtime.waiting_mode.is_some()
        || runtime.snapshot_in_flight.is_some()
        || runtime
            .tree
            .refresh_after
            .is_some_and(|due| Instant::now() < due)
        || !embedded_scene_tree::visible(world)
        || !embedded_scene_tree::ready_to_request(world)
    {
        return;
    }
    let request_id = runtime.tree.next_id.wrapping_add(1).max(1);
    let revision = status.applied_revision;
    let payload = serde_json::to_vec(&serde_json::json!({
        "revision": revision, "known": runtime.tree.version,
    }))
    .expect("固定场景树查询可序列化");
    if send(
        runtime,
        Outgoing {
            generation: 0,
            message: Message::Preview {
                kind: PreviewKind::AuthorDomain,
                sequence: request_id,
                format: REQUEST.into(),
                scene_id: runtime.scene_id.as_deref().unwrap_or(EMPTY_SCENE).into(),
            },
            payload: Some(Arc::new(payload)),
        },
        status,
    ) {
        runtime.tree.next_id = request_id;
        runtime.tree.flight = Some((request_id, revision));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_scene_clears_empty_world_tree_at_the_same_generation() {
        let mut app = App::new();
        embedded_scene_tree::install(&mut app);
        let world = app.world_mut();
        world.init_resource::<EmbeddedSnapshotState>();
        let mut runtime = Runtime::default();
        runtime.tree.supported = true;
        runtime.identity = Some(SessionIdentity {
            project_id: "fixture".into(), session: 8, generation: 1,
        });
        let mut status = EmbeddedStatus { ready: true, world_id: 4, ..default() };
        sync(world, &mut runtime, &mut status, true, false);
        embedded_scene_tree::publish(world, Arc::new(Vec::new()));
        runtime.tree.flight = Some((1, 0));
        runtime.tree.version = Some(7);

        // 首个 Snapshot 进入队列后，作者与工程已指向同一场景，且代次未变。
        runtime.scene_id = Some("first-scene".into());
        runtime.snapshot_in_flight = Some(1);
        world.resource_mut::<EmbeddedSnapshotState>().scene_id = runtime.scene_id.clone();
        sync(world, &mut runtime, &mut status, true, false);

        assert_eq!(runtime.tree.flight, None);
        assert_eq!(runtime.tree.version, None);
        assert!(!embedded_scene_tree::ready_to_request(world));
    }

    #[test]
    fn mismatched_body_releases_matching_query_and_retires_rows() {
        let mut app = App::new();
        embedded_scene_tree::install(&mut app);
        let world = app.world_mut();
        let identity = SessionIdentity { project_id: "fixture".into(), session: 8, generation: 2 };
        let mut runtime = Runtime::default();
        runtime.identity = Some(identity.clone());
        runtime.scene_id = Some("fixture".into());
        runtime.tree.supported = true;
        runtime.tree.flight = Some((2, 1));
        runtime.tree.version = Some(7);
        let status = EmbeddedStatus { world_id: 4, applied_revision: 1, ..default() };
        let response = Response {
            identity,
            scene_id: "fixture".into(),
            request_id: 2,
            reply: Some(TreeReply {
                world_id: 5, revision: 1, version: Some(8), rows: Some(Arc::new(Vec::new())),
            }),
        };
        receive(world, &mut runtime, &status, response);

        assert_eq!(runtime.tree.flight, None);
        assert_eq!(runtime.tree.version, Some(7));
        assert!(runtime.tree.refresh_after.is_some());
        assert!(!embedded_scene_tree::ready_to_request(world));
    }

    #[test]
    fn stale_scene_tree_reply_retires_rows_and_backpressures_new_queries() {
        let mut app = App::new();
        embedded_scene_tree::install(&mut app);
        let rows = Arc::new(Vec::new());
        let retired = Arc::downgrade(&rows);
        let response = Response {
            identity: SessionIdentity { project_id: "fixture".into(), session: 8, generation: 2 },
            scene_id: "fixture".into(),
            request_id: 1,
            reply: Some(TreeReply { world_id: 4, revision: 1, version: Some(1), rows: Some(rows) }),
        };
        receive(app.world_mut(), &mut Runtime::default(), &EmbeddedStatus::default(), response);
        assert!(retired.upgrade().is_some());
        assert!(!embedded_scene_tree::ready_to_request(app.world()));
    }
}
