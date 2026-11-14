//! 已采用地形的只读宿主契约；不规定实体类型、贴地策略或玩法行为。

use avian3d::prelude::Collider;
use bevy::{asset::AssetId, ecs::system::SystemParam, prelude::*};
use feathrise_scene_content::{ActiveDynamicScene, SceneId, sample_heightfield_world};
use feathrise_scene::{DynamicSceneTerrainPrimitive, ResidentScenes};
use feathrise_scene_core::residency::SceneResidencyActivity;
use feathrise_scene_core::state::ScenePhase;

use super::super::{EditorAuthoringSession, EditorSceneDraft, TerrainLiveApplyState};
use super::helpers::{collider_contains_trimesh, colliders_share_shape};
use crate::components::EditorLiveTerrainSource;

/// 宿主只在实际采用后响应；F12关闭门必须等待本帧响应，不能提前恢复世界。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TerrainLiveSet {
    Adopt,
    React,
}

/// 借用当前完整草稿的采用回执；大数组不复制，也不增持会妨碍作者写入的Arc。
pub struct TerrainLiveReceipt<'a> {
    pub scene_id: &'a SceneId,
    pub revision: u64,
    pub candidate_epoch: u64,
    pub mesh: AssetId<Mesh>,
    draft: &'a EditorSceneDraft,
}

impl TerrainLiveReceipt<'_> {
    /// 按正式派生高度采样单个世界坐标；返回地表高度，不包含任何角色根偏移。
    pub fn sample_ground_y(&self, world_xz: [f32; 2]) -> Option<f32> {
        sample_heightfield_world(
            self.draft.heights.as_ref()?.as_slice(),
            self.draft.manifest.terrain.resolution,
            world_xz,
            self.draft.manifest.world_min,
            self.draft.manifest.world_max,
            self.draft.manifest.terrain.base_height,
            self.draft.manifest.terrain.height_scale,
        )
    }
}

/// 只读查询真实采用资格；不得把等待候选、旧场景或保存交接当成新地形已落地。
#[derive(SystemParam)]
pub struct TerrainLiveAdoption<'w, 's> {
    live: Res<'w, TerrainLiveApplyState>,
    session: Res<'w, EditorAuthoringSession>,
    scene: Option<Res<'w, State<ScenePhase>>>,
    active: Option<Res<'w, ActiveDynamicScene>>,
    scenes: Option<Res<'w, ResidentScenes>>,
    activity: Option<Res<'w, SceneResidencyActivity>>,
    sources: Query<
        'w,
        's,
        (
            &'static Mesh3d,
            Option<&'static Collider>,
            Option<&'static EditorLiveTerrainSource>,
        ),
        With<DynamicSceneTerrainPrimitive>,
    >,
}

impl TerrainLiveAdoption<'_, '_> {
    pub fn current(&self) -> Option<TerrainLiveReceipt<'_>> {
        let draft = self.session.draft.as_ref()?;
        let scene_id = self.live.active_scene_id.as_ref()?;
        #[cfg(any(test, feature = "runtime_test_support"))]
        if self.live.file_handoff.is_some() {
            return None; // 只有旧有限网格夹具继续验证GLB文件接管门。
        }
        if *self.scene.as_ref()?.get() != ScenePhase::Active
            || self.active.as_ref()?.scene_id.as_ref() != Some(scene_id)
            || &draft.scene_id != scene_id
            || !draft.terrain_dirty
            || self.session.terrain_revision != self.live.active_revision
            || !self.live.ready_for(scene_id, self.session.terrain_revision)
            || self
                .activity
                .as_ref()
                .is_some_and(|activity| activity.switching)
            || self
                .scenes
                .as_ref()
                .is_some_and(|scenes| scenes.contains(scene_id) && !scenes.active_is(scene_id))
        {
            return None;
        }
        let expected_mesh = self.live.active_mesh.as_ref()?;
        let expected_collider = self.live.active_collider.as_ref()?;
        // 与维护系统查询同一正式源集合；新源缺少碰撞/标记也必须参与唯一性检查。
        let (mesh, collider, source) = self.sources.single().ok()?;
        let collider = collider?;
        let source = source?;
        // 回执借用时再次做常数成本实体核对，文件失败或同帧换源不能借用上帧ready。
        if &source.scene_id != scene_id
            || source.revision != self.live.active_revision
            || mesh.0.id() != expected_mesh.id()
            || !collider_contains_trimesh(collider)
            || !colliders_share_shape(collider, expected_collider)
        {
            return None;
        }
        Some(TerrainLiveReceipt {
            scene_id,
            revision: self.live.active_revision,
            candidate_epoch: self.live.candidate_epoch,
            mesh: expected_mesh.id(),
            draft,
        })
    }
}
