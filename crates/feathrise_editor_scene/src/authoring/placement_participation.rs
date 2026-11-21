//! 摆放参与状态由配套 SDK 持有。固定旧 SDK 仍能使用其原有的编辑器保护操作。

use bevy::prelude::{Commands, Entity, World};
use feathrise_scene::ScenePlacementInstance;
use feathrise_scene_content::ScenePlacementEditorMetadata;

// Rust 优先解析同名的 SDK 固有方法。发布锁更新前，旧 SDK 沿原有默认参与行为。
#[allow(dead_code)]
trait LegacyPlacementParticipation {
    fn supports_participation() -> bool { false }
    fn participates(&self) -> bool { true }
    fn set_participation(&mut self, _enabled: bool) {}
}

impl LegacyPlacementParticipation for ScenePlacementEditorMetadata {}

#[allow(dead_code)]
trait LegacyPlacementCollisionParticipation {
    fn set_editor_collision_participation(_world: &mut World, _root: Entity, _enabled: bool) {}
}

impl LegacyPlacementCollisionParticipation for ScenePlacementInstance {}

pub(crate) fn sync_gltf_collision(commands: &mut Commands, root: Entity, enabled: bool) {
    commands.queue(move |world: &mut World| {
        ScenePlacementInstance::set_editor_collision_participation(world, root, enabled);
    });
}

pub(crate) fn available() -> bool {
    ScenePlacementEditorMetadata::supports_participation()
}

pub(crate) fn participates(metadata: &ScenePlacementEditorMetadata) -> bool {
    metadata.participates()
}

pub(crate) fn set_participation(metadata: &mut ScenePlacementEditorMetadata, enabled: bool) -> Result<(), String> {
    if !available() {
        return Err("当前 SDK 不支持摆放参与状态".into());
    }
    metadata.set_participation(enabled);
    Ok(())
}
