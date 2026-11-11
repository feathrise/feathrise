//! 工程关闭复用地表原目录/纹理派生事务；写入和热采用完成前不得退出。

use bevy::prelude::*;
use feathrise_editor_core::PresetWorkspaceTab;
use feathrise_editor_surface_model::SurfacePresetEditorState;
use feathrise_scene_content::SurfacePresetRegistry;

pub fn request_project_save(world: &mut World, tab: u64) -> Result<(), String> {
    world.resource_scope(|world, mut state: Mut<SurfacePresetEditorState>| {
        if state.active_field.is_some() {
            return Err("请先用Enter提交或Esc取消地表预设字段，再保存全部".into());
        }
        let registry = world
            .get_resource::<SurfacePresetRegistry>()
            .ok_or("PBR地表预设目录尚未加载")?;
        let draft = state
            .active_draft(PresetWorkspaceTab::Surface(tab))
            .ok_or("地表预设标签已关闭")?;
        let mode = draft.mode.clone();
        let descriptor = draft.descriptor.clone();
        super::persistence::start_upsert(&mut state, registry, &mode, descriptor, tab)
    })
}
