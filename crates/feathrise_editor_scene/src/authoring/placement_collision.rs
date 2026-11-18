//! 摆放碰撞是 Feathrise 的场景属性，建模回填和属性编辑读取同一个值。

use feathrise_scene_content::ScenePlacementAssetTarget;
use std::sync::OnceLock;

fn scene0_collision_target() -> Option<&'static ScenePlacementAssetTarget> {
    static ENABLED: OnceLock<Option<ScenePlacementAssetTarget>> = OnceLock::new();
    // 通过现有场景格式识别 SDK 能力。旧公开锁仍能构建编辑器，其 SDK 会忽略这个可选字段。
    ENABLED.get_or_init(|| {
        serde_json::from_str(r#"{"kind":"scene0","collision":true}"#).ok()
            .filter(|target| *target != ScenePlacementAssetTarget::Scene0)
    }).as_ref()
}

pub(crate) fn mesh_collision_enabled(target: &ScenePlacementAssetTarget) -> Option<bool> {
    match target {
        ScenePlacementAssetTarget::Cuboid { collision, .. }
        | ScenePlacementAssetTarget::EditableCuboid { collision, .. }
        | ScenePlacementAssetTarget::EditableMesh { collision, .. } => Some(*collision),
        target if target.is_scene0() => scene0_collision_target().map(|enabled| enabled == target),
        _ => None,
    }
}

pub(crate) fn asset_target_with_mesh_collision(
    target: &ScenePlacementAssetTarget,
    enabled: bool,
) -> Option<ScenePlacementAssetTarget> {
    if target.is_scene0() {
        return if enabled { scene0_collision_target().cloned() } else { Some(ScenePlacementAssetTarget::Scene0) };
    }
    let mut target = target.clone();
    match &mut target {
        ScenePlacementAssetTarget::Cuboid { collision, .. }
        | ScenePlacementAssetTarget::EditableCuboid { collision, .. }
        | ScenePlacementAssetTarget::EditableMesh { collision, .. } => *collision = enabled,
        _ => return None,
    }
    Some(target)
}
