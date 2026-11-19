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
    scope: Option<(u64, u64, u64)>,
    next_id: u64,
    flight: Option<(u64, u64)>,
    version: Option<u64>,
    refresh_after: Option<Instant>,
}

pub(super) struct Response {
    identity: SessionIdentity,
    scene_id: String,
    request_id: u64,
    reply: Option<TreeReply>,
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
        return;
    };
    if !tree.supported
        || runtime.identity.as_ref() != Some(&response.identity)
        || runtime.scene_id.as_deref().unwrap_or(EMPTY_SCENE) != response.scene_id
        || response.request_id != id
        || response
            .reply
            .as_ref()
            .is_some_and(|reply| reply.world_id != status.world_id || reply.revision != revision)
    {
        return;
    }
    tree.flight = None;
    if revision != status.applied_revision || runtime.snapshot_in_flight.is_some() || status.playing
    {
        return;
    }
    tree.refresh_after = Some(Instant::now() + Duration::from_secs(1));
    match response.reply {
        Some(TreeReply {
            version: Some(version),
            rows: Some(rows),
            ..
        }) => {
            tree.version = Some(version);
            embedded_scene_tree::publish(world, rows);
        }
        Some(TreeReply {
            version: Some(version),
            rows: None,
            ..
        }) if Some(version) == tree.version => (),
        // 可选诊断失败时保留上一份完整层级，后续更新可自然恢复。
        _ => embedded_scene_tree::unavailable(world),
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
        .map(|identity| (identity.session, identity.generation, status.world_id));
    if scope != runtime.tree.scope {
        runtime.tree.scope = scope;
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
