//! 一个在途CPU槽和一个合并请求；取消不丢句柄，完成值不携带整图副本。
use super::types::MeshSnapKey;
use crate::viewport::vegetation_ground::GroundCursorTarget;
use bevy::{prelude::*, tasks::Task};
use feathrise_scene::ClipmapGroundHit;

#[derive(Resource, Default)]
pub(crate) struct MeshSnapCursor {
    pub(super) task: Option<Task<(MeshSnapKey, ClipmapGroundHit)>>,
    pub(super) pending: Option<MeshSnapKey>,
    pub(super) accepted: Option<(MeshSnapKey, ClipmapGroundHit)>,
    pub(super) completed: Option<(MeshSnapKey, ClipmapGroundHit)>,
    pub(super) last_started: Option<MeshSnapKey>,
    pub(super) requires_query: bool,
    pub(super) awaiting_object_hit: bool,
    pub(crate) preview: Option<Vec3>,
}
impl MeshSnapCursor {
    pub(crate) fn requires_query(&self) -> bool {
        self.requires_query || self.pending.is_some() || self.completed.is_some()
    }
    /// 调用方在一次消费前冻结绑定地形，完成帧的悬停对象不能改写本次作者意图。
    pub(crate) fn frozen_terrain(&self) -> Option<&feathrise_map_content::TerrainId> {
        self.completed.as_ref()?.0.object.as_ref().map(|object| &object.id)
    }
    pub(crate) fn take_click(
        &mut self,
        map: &feathrise_map_content::MapId,
        target: &GroundCursorTarget,
        author: u64,
        terrain: u64,
    ) -> Option<ClipmapGroundHit> {
        let (key, hit) = self.completed.take()?;
        (key.map == *map && key.target == *target && key.author == author && key.terrain == terrain)
            .then_some(hit)
    }
    pub(super) fn cancel(&mut self) {
        self.pending = None;
        self.accepted = None;
        self.completed = None;
        self.last_started = None;
        self.awaiting_object_hit = false;
        self.requires_query = false; // task保持占槽，绝不把丢弃句柄当抢占取消。
        self.preview = None;
    }
}
