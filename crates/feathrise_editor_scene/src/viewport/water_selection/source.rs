//! 当前作者画布与活动实例的水面来源门；只定点查询已应用根，不扫描场景或复制R8。

use super::*;
use bevy::ecs::system::SystemParam;
use feathrise_scene_content::SceneTerrainObject;
use feathrise_scene::{SubsceneActive, SubsceneInstance, TerrainObjectAuthoringTarget, TerrainObjectInstance};
use feathrise_scene_core::residency::{SceneInstanceOwner, SceneSourceFrame};
use std::sync::{Arc, Weak};

/// 查找键只保留画布和文件来源；弱画布身份防止已回收来源的地址被新画布复用。
#[derive(Clone, Debug)]
pub(super) struct WaterLookupSource {
    scene_id: SceneId,
    terrain: Option<Weak<SceneTerrainObject>>,
    path: String,
    hash: Option<[u8; 32]>,
}
fn same_weak<T>(a: &Option<Weak<T>>, b: &Option<Weak<T>>) -> bool {
    match (a, b) { (Some(a), Some(b)) => Weak::ptr_eq(a, b), (None, None) => true, _ => false }
}
impl PartialEq for WaterLookupSource {
    fn eq(&self, other: &Self) -> bool {
        self.scene_id == other.scene_id && same_weak(&self.terrain, &other.terrain)
            && self.path == other.path && self.hash == other.hash
    }
}
impl WaterLookupSource {
    pub(super) fn capture(session: &EditorAuthoringSession) -> Option<Self> {
        let draft = session.draft.as_ref()?;
        if draft.terrain_documents.loading || draft.water.as_ref()?.scene_id != draft.scene_id { return None; }
        Some(Self {
            scene_id: draft.scene_id.clone(), terrain: draft.terrain_documents.active.as_ref().map(|active| Arc::downgrade(&active.definition)),
            path: draft.water_asset_path.clone(), hash: draft.terrain_source_sha256,
        })
    }
    pub(super) fn matches(&self, session: &EditorAuthoringSession) -> bool {
        let Some(draft) = session.draft.as_ref() else { return false; };
        let terrain = draft.terrain_documents.active.as_ref().map(|active| Arc::downgrade(&active.definition));
        !draft.terrain_documents.loading && self.scene_id == draft.scene_id
            && draft.water.as_ref().is_some_and(|water| water.scene_id == self.scene_id)
            && same_weak(&self.terrain, &terrain)
            && self.path == draft.water_asset_path && self.hash == draft.terrain_source_sha256
    }
}

/// 实体查找只按结构来源失效，R8和数值修订仅决定中心能否授权，不能逐帧触发全水面查找。
#[derive(Clone, Debug)]
pub(super) struct WaterSourceIdentity {
    pub(super) source: WaterLookupSource,
    mask: Option<Weak<Vec<u8>>>,
    water_revision: u64,
    terrain_revision: u64,
}
impl WaterSourceIdentity {
    pub(super) fn capture(session: &EditorAuthoringSession, id: &str) -> Option<Self> {
        Some(Self { source: WaterLookupSource::capture(session)?,
            mask: session.draft.as_ref()?.water_masks.get(id).map(Arc::downgrade),
            water_revision: session.water_revision, terrain_revision: session.terrain_revision })
    }
    pub(super) fn matches(&self, session: &EditorAuthoringSession, id: &str) -> bool {
        let mask = session.draft.as_ref().and_then(|draft| draft.water_masks.get(id)).map(Arc::downgrade);
        self.water_revision == session.water_revision && self.terrain_revision == session.terrain_revision
            && same_weak(&self.mask, &mask) && self.source.matches(session)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum WaterSurfaceScope {
    Root,
    Instance(Entity, Mat4),
    Unavailable,
}
impl WaterSurfaceScope {
    pub(super) fn accepts(self, owner: Option<&SceneInstanceOwner>) -> bool {
        match self {
            // 旧单地形位于来源图首个根节点，由父调度直接加载；正式水面和GLB作者预览均不带实例归属。
            Self::Root => owner.is_none(),
            Self::Instance(entity, _) => owner.is_some_and(|owner| owner.0 == entity),
            Self::Unavailable => false,
        }
    }
    pub(super) fn matrix(self) -> Mat4 {
        match self { Self::Instance(_, frame) => frame, _ => Mat4::IDENTITY }
    }
}

#[derive(SystemParam)]
pub(crate) struct WaterSourceContext<'w, 's> {
    target: Option<Res<'w, TerrainObjectAuthoringTarget>>,
    instances: Query<'w, 's, (&'static SubsceneInstance, &'static TerrainObjectInstance, &'static SceneSourceFrame)>,
    active: Query<'w, 's, (), With<SubsceneActive>>,
}
impl WaterSourceContext<'_, '_> {
    pub(super) fn scope(&self, session: &EditorAuthoringSession) -> WaterSurfaceScope {
        let Some(draft) = session.draft.as_ref() else { return WaterSurfaceScope::Unavailable; };
        let Some(canvas) = draft.terrain_documents.active.as_ref() else {
            return WaterSurfaceScope::Root; // 主场景根水不带实例归属；子场景引用的水仍由owner门排除。
        };
        let Some(target) = self.target.as_ref().filter(|target|
            target.scene_id.as_ref() == Some(&draft.scene_id) && target.terrain.as_ref() == Some(&canvas.definition.id)) else {
            return WaterSurfaceScope::Unavailable;
        };
        let Some(entity) = target.instance() else { return WaterSurfaceScope::Unavailable; };
        let Ok((instance, object, frame)) = self.instances.get(entity) else { return WaterSurfaceScope::Unavailable; };
        applied_instance_scope(&draft.scene_id, &canvas.definition, entity, instance,
            &object.definition, frame, self.active.contains(instance.batch))
    }
}

/// 只校验一次定点查询返回的小型元组；真实target地址门由调用方保留，测试无需扩张场景公开接口。
pub(super) fn applied_instance_scope(map: &SceneId, current: &SceneTerrainObject, entity: Entity,
    instance: &SubsceneInstance, source: &SceneTerrainObject, frame: &SceneSourceFrame, active: bool) -> WaterSurfaceScope {
        // target只固定已采用地址；另核对源字段，避免同ID换后端/高度文件时借用旧根。
        // 不比较完整definition：运行时会派生resolved_stitches，且LOD等展示参数不属于水源身份。
        if &instance.source_scene_id != map || &instance.path.root_scene_id != map
            || !instance.path.instances.is_empty() || instance.path.terrain_id.as_ref() != Some(&current.id)
            || source.id != current.id || !source.enabled || !current.enabled || source.water_path != current.water_path
            || source.transform != current.transform || source.world_min != current.world_min || source.world_max != current.world_max
            || source.terrain.kind != current.terrain.kind || source.terrain.glb_path != current.terrain.glb_path
            || source.terrain.height_path != current.terrain.height_path || source.terrain.height_texture_path != current.terrain.height_texture_path
            || source.terrain.resolution != current.terrain.resolution || source.terrain.base_height != current.terrain.base_height
            || source.terrain.height_scale != current.terrain.height_scale || !active
            || frame.source_to_world != current.transform.as_transform().to_matrix() {
            return WaterSurfaceScope::Unavailable;
        }
        WaterSurfaceScope::Instance(entity, frame.source_to_world)
}
