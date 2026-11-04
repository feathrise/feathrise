//! 整体对象只桥接原工具已发布的姿态；引用查找与编码留在原有单飞后台槽。
use super::resources::{EmbeddedAuthorPreviewState, Key, Version};
use crate::authoring::EditorAuthoringSession;
use bevy::prelude::*;
use feathrise_map_content::{
    MapId, MapInstanceAuthorPreview, MapInstanceId, MapInstancePlacement, MapTerrainObject,
    TerrainId, TerrainObjectTransform, embedded_author_preview::AuthorPreview,
};
use feathrise_scene::TerrainObjectTransformPreview;
use std::{collections::VecDeque, sync::Arc};

const MAX_EDGES: usize = 32;

#[derive(Clone)]
pub(super) enum FrozenPose {
    SubMap {
        id: MapInstanceId,
        source: Arc<[MapInstancePlacement]>,
        active: bool,
    },
    Terrain {
        id: TerrainId,
        source: Option<Arc<[MapTerrainObject]>>,
        preview: Option<TerrainObjectTransform>,
        active: bool,
    },
}
impl FrozenPose {
    fn active(&self) -> bool {
        match self {
            Self::SubMap { active, .. } | Self::Terrain { active, .. } => *active,
        }
    }
    fn same_target(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::SubMap { id: a, .. }, Self::SubMap { id: b, .. }) => a == b,
            (Self::Terrain { id: a, .. }, Self::Terrain { id: b, .. }) => a == b,
            _ => false,
        }
    }
    fn same_value(&self, other: &Self) -> bool {
        if !self.same_target(other) || self.active() != other.active() {
            return false;
        }
        match (self, other) {
            (Self::SubMap { source: a, .. }, Self::SubMap { source: b, .. }) => Arc::ptr_eq(a, b),
            (
                Self::Terrain {
                    preview: a,
                    source: sa,
                    ..
                },
                Self::Terrain {
                    preview: b,
                    source: sb,
                    ..
                },
            ) => {
                a == b
                    && (self.active()
                        || match (sa, sb) {
                            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                            (None, None) => true,
                            _ => false,
                        })
            }
            _ => false,
        }
    }
    fn authoritative(&self, session: &EditorAuthoringSession) -> Option<Self> {
        let draft = session.draft.as_ref()?;
        Some(match self {
            Self::SubMap { id, .. } => Self::SubMap {
                id: id.clone(),
                source: draft.manifest.sub_maps.clone(),
                active: false,
            },
            Self::Terrain { id, .. } => Self::Terrain {
                id: id.clone(),
                source: draft.manifest.terrains.clone(),
                preview: None,
                active: false,
            },
        })
    }
    pub(super) fn encode(self, map_id: MapId, revision: u64) -> AuthorPreview {
        match self {
            Self::SubMap { id, source, active } => {
                let pose = source
                    .iter()
                    .find(|value| value.id == id)
                    .map(|value| value.transform);
                AuthorPreview::SubMapPose {
                    map_id,
                    revision,
                    id,
                    pose,
                    active,
                }
            }
            Self::Terrain {
                id,
                source,
                preview,
                active,
            } => {
                let pose = preview.or_else(|| {
                    source
                        .as_ref()?
                        .iter()
                        .find(|value| value.id == id)
                        .map(|value| value.transform)
                });
                AuthorPreview::TerrainPose {
                    map_id,
                    revision,
                    id,
                    pose,
                    active,
                }
            }
        }
    }
}

#[derive(Default)]
pub(super) struct PoseMailbox {
    observed: Option<FrozenPose>,
    queue: VecDeque<(u64, FrozenPose)>,
    ticket: u64,
    retired: Vec<FrozenPose>,
}
impl PoseMailbox {
    pub(super) fn clear(&mut self) {
        // 序号不回退；完整来源接管撤销旧边沿，当前仍在拖动的源下一帧自然重放。
        self.retired.extend(self.observed.take());
        self.retired
            .extend(self.queue.drain(..).map(|(_, value)| value));
    }
    fn push(&mut self, value: FrozenPose) -> bool {
        if self
            .observed
            .as_ref()
            .is_some_and(|old| old.same_value(&value))
        {
            return true;
        }
        let merge = value.active()
            && self
                .queue
                .back()
                .is_some_and(|(_, last)| last.active() && last.same_target(&value));
        // 为当前笔的可靠结束保留席位；背压时不承认新 Begin，稍后重读原工具当前姿态。
        if !merge
            && self.queue.len()
                >= if value.active() {
                    MAX_EDGES - 1
                } else {
                    MAX_EDGES
                }
        {
            return false;
        }
        let Some(ticket) = self.ticket.checked_add(1) else {
            return false;
        };
        self.ticket = ticket;
        self.retired.extend(self.observed.replace(value.clone()));
        if merge {
            self.retired
                .extend(self.queue.pop_back().map(|(_, value)| value));
        }
        self.queue.push_back((ticket, value));
        true
    }
    fn observe(&mut self, current: Option<FrozenPose>, session: &EditorAuthoringSession) {
        if let Some(previous) = self.observed.as_ref().filter(|value| value.active())
            && current
                .as_ref()
                .is_none_or(|value| !previous.same_target(value))
            && let Some(end) = previous.authoritative(session)
            && !self.push(end)
        {
            return;
        }
        if let Some(current) = current {
            self.push(current);
        }
    }
    fn acknowledge(&mut self, revision: u64) {
        while self
            .queue
            .front()
            .is_some_and(|(ticket, _)| *ticket <= revision)
        {
            self.retired
                .extend(self.queue.pop_front().map(|(_, value)| value));
        }
    }
    fn capture(&self, key: Key) -> Option<(Key, Version, super::source::Frozen)> {
        let (revision, pose) = self.queue.front()?;
        Some((
            key,
            Version {
                revision: *revision,
                present: pose.active(),
            },
            super::source::Frozen::Pose(pose.clone()),
        ))
    }
}

pub(super) fn observe(world: &World, state: &mut EmbeddedAuthorPreviewState, map: &MapId) {
    let Some(session) = world.get_resource::<EditorAuthoringSession>() else {
        return;
    };
    let Some(draft) = session.draft.as_ref().filter(|draft| &draft.map_id == map) else {
        return;
    };
    let known = state.applied_pose_targets.as_ref().filter(|known| {
        known.session == state.session
            && known.generation == state.generation
            && &known.map_id == map
    });
    let sub_map = world
        .get_resource::<crate::viewport::sub_maps::SubMapTransformState>()
        .and_then(|source| source.embedded_target(session))
        .or_else(|| {
            world
                .get_resource::<crate::ui::sub_maps::SubMapEditorState>()
                .and_then(|source| source.embedded_target(session))
        });
    let sub_map = sub_map
        .filter(|id| known.is_some_and(|known| known.targets.sub_maps.contains(id)))
        .and_then(|id| {
            world
                .get_resource::<MapInstanceAuthorPreview>()
                .filter(|source| source.map_id.as_ref() == Some(map))
                .map(|source| FrozenPose::SubMap {
                    id,
                    source: source.instances.clone(),
                    active: true,
                })
        });
    let terrain = world
        .get_resource::<TerrainObjectTransformPreview>()
        .and_then(|source| source.pose.as_ref())
        .filter(|(owner, id, _)| {
            owner == map && known.is_some_and(|known| known.targets.terrains.contains(id))
        })
        .map(|(_, id, pose)| FrozenPose::Terrain {
            id: id.clone(),
            source: draft.manifest.terrains.clone(),
            preview: Some(*pose),
            active: true,
        });
    state.sub_map_pose.observe(sub_map, session);
    state.terrain_pose.observe(terrain, session);
}

pub(super) fn acknowledge(world: &mut World, state: &mut EmbeddedAuthorPreviewState) {
    for (key, mailbox) in [
        (Key::SubMapPose, &mut state.sub_map_pose),
        (Key::TerrainPose, &mut state.terrain_pose),
    ] {
        if let Some(version) = state.acknowledged.get(&key) {
            mailbox.acknowledge(version.revision);
        }
    }
    retire_stale(world, state);
}

pub(super) fn retire_stale(world: &mut World, state: &mut EmbeddedAuthorPreviewState) {
    let (enabled, session, generation) = (state.enabled, state.session, state.generation);
    let stale = |credit: &super::resources::PoseTargetCredit| {
        !enabled || credit.session != session || credit.generation != generation
    };
    let pending = if state.pending_pose_targets.as_ref().is_some_and(stale) {
        state.pending_pose_targets.take()
    } else {
        None
    };
    let applied = if state.applied_pose_targets.as_ref().is_some_and(stale) {
        state.applied_pose_targets.take()
    } else {
        None
    };
    if pending.is_none()
        && applied.is_none()
        && state.sub_map_pose.retired.is_empty()
        && state.terrain_pose.retired.is_empty()
    {
        return;
    }
    world
        .resource_mut::<crate::authoring::AuthoringRetirement>()
        .push((
            std::mem::take(&mut state.sub_map_pose.retired),
            std::mem::take(&mut state.terrain_pose.retired),
            pending,
            applied,
        ));
}

pub(super) fn capture(
    state: &EmbeddedAuthorPreviewState,
    terrain: bool,
) -> Option<(Key, Version, super::source::Frozen)> {
    if terrain {
        state.terrain_pose.capture(Key::TerrainPose)
    } else {
        state.sub_map_pose.capture(Key::SubMapPose)
    }
}

#[cfg(test)]
mod tests;
