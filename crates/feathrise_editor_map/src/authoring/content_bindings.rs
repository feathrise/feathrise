//! 传送点与外部作者文档的地形关联换代；完整后台结果原子采用，旧固定内容保持原样。
use super::{EditorAuthoringSession, terrain_ground::TerrainGroundSnapshot};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use feathrise_map_content::{MapId, MapPortalRecord, TerrainObjectTransform};
use std::sync::Arc;
#[derive(Clone, PartialEq)]
struct Identity {
    map: MapId,
    author: u64,
    terrain: u64,
    generation: u64,
    portals: u64,
    domain_hash: String,
}
struct Capture {
    identity: Identity,
    ground: Arc<TerrainGroundSnapshot>,
    domain: Option<super::domain_documents::MapDomainDocument>,
    records: Vec<MapPortalRecord>,
    cursor: usize,
}
struct Work {
    identity: Identity,
    ground: Arc<TerrainGroundSnapshot>,
    task: Task<Result<(Vec<MapPortalRecord>, Option<super::domain_documents::MapDomainDocument>), String>>,
}
#[derive(Resource, Default)]
pub(crate) struct ContentTerrainFollowState {
    capture: Option<Capture>,
    work: Option<Work>,
    accepted: Option<Identity>,
    waiting: Option<Arc<TerrainGroundSnapshot>>,
    preview_cursor: usize,
    pub(crate) failure: Option<String>,
}
impl ContentTerrainFollowState {
    pub(crate) fn ready(&self, session: &EditorAuthoringSession) -> bool {
        let Some(identity) = identity(session) else {
            return session
                .draft
                .as_ref()
                .is_none_or(|draft| draft.manifest.terrains.is_none());
        };
        self.accepted.as_ref() == Some(&identity)
            && self.failure.is_none()
            && session
                .draft
                .as_ref()
                .is_none_or(|draft| self.preview_cursor >= draft.portals.portals.len())
    }
    pub(crate) fn blocks_close(
        &self,
        editor: &mut crate::resources::MapEditorState,
        session: &EditorAuthoringSession,
    ) -> bool {
        if self.ready(session) {
            return false;
        }
        if let Some(error) = self.failure.as_ref() {
            editor
                .cancel_close_request(format!("关联内容无法恢复：{error}；请修正或解除关联后重试"));
        } else {
            editor.status = "正在后台完成作者文档和传送点的地形关联；完成后继续关闭编辑器".into();
        }
        true
    }
}
fn identity(session: &EditorAuthoringSession) -> Option<Identity> {
    let draft = session
        .draft
        .as_ref()
        .filter(|draft| draft.manifest.terrains.is_some())?;
    let ground = draft
        .terrain_ground
        .as_ref()
        .filter(|ground| ground.revision == session.terrain_revision)?;
    Some(Identity {
        map: draft.map_id.clone(),
        author: session.revision,
        terrain: session.terrain_revision,
        generation: ground.generation,
        portals: session.portal_revision,
        domain_hash: draft.adopted_domain_hash().to_owned(),
    })
}
pub(super) fn reconcile(
    mut commands: Commands,
    editor: Res<crate::resources::MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    preview: Option<Res<super::EditorPreviewAssets>>,
    mut state: ResMut<ContentTerrainFollowState>,
) {
    let Some(current) = identity(&session) else {
        if let Some(retired) = state.capture.take() {
            AsyncComputeTaskPool::get()
                .spawn(async move { drop(retired) })
                .detach();
        }
        state.failure = None;
        return;
    };
    if let Some(result) = state
        .work
        .as_mut()
        .and_then(|work| check_ready(&mut work.task))
    {
        let work = state.work.take().unwrap();
        if work.identity == current {
            match result {
                Ok((portals, domain)) => {
                    let draft = session.draft.as_mut().unwrap();
                    let retired_domain = std::mem::replace(&mut draft.domain, domain);
                    let retired_portals = std::mem::replace(&mut draft.portals.portals, portals);
                    AsyncComputeTaskPool::get()
                        .spawn(async move { drop((retired_domain, retired_portals)) })
                        .detach();
                    state.accepted = identity(&session);
                    state.preview_cursor = 0;
                    state.failure = None;
                    state.waiting = None;
                    return; // 后续按64根预算同步预览，不在完整数据采用帧遍历全部根。
                }
                Err(error) if error.contains("高度页尚未完整就绪") => {
                    state.waiting = Some(work.ground);
                    state.failure = None;
                }
                Err(error) => {
                    state.accepted = Some(current.clone());
                    state.failure = Some(error);
                    state.waiting = None;
                }
            }
        } else {
            AsyncComputeTaskPool::get()
                .spawn(async move { drop(result) })
                .detach();
        }
    }
    if state.accepted.as_ref() == Some(&current) {
        let draft = session.draft.as_ref().unwrap();
        let end = (state.preview_cursor + 64).min(draft.portals.portals.len());
        let records = draft.portals.portals[state.preview_cursor..end].to_vec();
        state.preview_cursor = end;
        if let Some(preview) = preview.as_deref() {
            for record in records {
                super::refresh_owned_portal_preview(&mut commands, preview, &mut session, &record);
            }
        }
        return;
    }
    if state.work.is_some() || !editor.open || session.saving {
        return;
    }
    let draft = session.draft.as_ref().unwrap();
    let ground = draft.terrain_ground.as_ref().unwrap().clone();
    if state
        .waiting
        .as_ref()
        .is_some_and(|waiting| Arc::ptr_eq(waiting, &ground))
    {
        return;
    }
    state.waiting = None;
    if state
        .capture
        .as_ref()
        .is_none_or(|capture| capture.identity != current)
    {
        if let Some(retired) = state.capture.take() {
            AsyncComputeTaskPool::get()
                .spawn(async move { drop(retired) })
                .detach();
        }
        state.failure = None;
        state.capture = Some(Capture {
            identity: current.clone(),
            ground,
            domain: draft.domain.clone(),
            records: Vec::new(),
            cursor: 0,
        });
    }
    let capture = state.capture.as_mut().unwrap();
    let end = (capture.cursor + 64).min(draft.portals.portals.len());
    capture
        .records
        .extend_from_slice(&draft.portals.portals[capture.cursor..end]);
    capture.cursor = end;
    if end != draft.portals.portals.len() {
        return;
    }
    let capture = state.capture.take().unwrap();
    let task_ground = capture.ground.clone();
    state.work = Some(Work {
        identity: capture.identity,
        ground: capture.ground,
        task: AsyncComputeTaskPool::get().spawn(async move {
            let mut records = capture.records;
            for record in &mut records {
                if let Some(binding) = record.terrain_binding.as_ref() {
                    let pose = task_ground.bindings.resolve(binding)?;
                    record.position = pose.translation;
                    record.arrow_degrees = pose.yaw_degrees;
                    if record.arrival_terrain_binding.is_none() {
                        let (id, y) = task_ground
                            .support_identity(
                                record.arrival[0],
                                record.arrival[2],
                                Some(record.arrival[1] + 0.5),
                            )?
                            .ok_or_else(|| {
                                format!(
                                    "传送点{}的安全到达位置没有真实承托面，请调整到达点",
                                    record.id
                                )
                            })?;
                        record.arrival[1] = y;
                        record.arrival_terrain_binding = Some(task_ground.bind(
                            &id,
                            TerrainObjectTransform {
                                translation: record.arrival,
                                yaw_degrees: 0.,
                                scale: [1.; 3],
                                ..Default::default()
                            },
                        )?);
                    }
                }
                if let Some(binding) = record.arrival_terrain_binding.as_ref() {
                    record.arrival = task_ground.bindings.resolve(binding)?.translation;
                }
            }
            let domain = capture.domain.map(|mut domain| {
                domain.value = (domain.ops.follow_terrain)(&*domain.value, &task_ground)?;
                Ok::<_, String>(domain)
            }).transpose()?;
            Ok((records, domain))
        }),
    });
}

#[cfg(test)]
mod tests;
