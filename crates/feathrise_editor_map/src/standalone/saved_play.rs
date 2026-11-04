//! 保存后试玩的只读提交门；只检查原文件写入/发布槽，不因未保存草稿或只读检查阻止试玩。

use crate::{authoring, resources, ui};
use bevy::prelude::*;

fn any<T: Resource>(world: &World, check: impl FnOnce(&T) -> bool) -> bool {
    world.get_resource::<T>().is_some_and(check)
}

/// 宿主显式应用代码时复用原F12的已保存地图选择；不公开或复制作者文档所有权。
pub fn current_saved_play_request(
    world: &World,
    profile: feathrise_editor_core::ProjectBuildProfile,
) -> super::RunSavedProject {
    let editor = world.get_resource::<resources::MapEditorState>();
    super::RunSavedProject {
        profile,
        map_id: editor.and_then(|editor| editor.selected_map_id.as_ref()).map(|id| id.0.clone()),
        has_unsaved_changes: editor.is_some_and(|editor| editor.dirty),
        stop_only: false,
    }
}

/// 在创建游戏进程前使用；拒绝的是当前在途显式提交，作者完成保存后可重新按F12。
///
/// 所有检测均为固定资源的任务标记，不读取磁盘、不遍历文档正文、不触发第二套保存。
pub fn saved_play_blocker(world: &World) -> Option<String> {
    let pending = any::<resources::MapEditorState>(world, |state| state.save_requested)
        || any::<authoring::EditorAuthoringSession>(world, |state| state.saving)
        || any::<authoring::DraftSaveTask>(world, |state| state.task.is_some())
        || any::<authoring::GlobalEnvironmentSaveTask>(world, |state| state.task.is_some())
        || any::<resources::MapCreationTask>(world, |state| state.task.is_some())
        || any::<resources::MapDeletionTask>(world, |state| {
            state.task.is_some() || state.pending_start.is_some()
        })
        || any::<resources::MapFolderPersistence>(world, |state| state.busy())
        || any::<ui::vegetation_preset_editor::VegetationPresetEditorState>(world, |state| {
            state.saved_play_pending()
        })
        || any::<ui::water_preset_editor::WaterPresetEditorState>(world, |state| {
            state.saved_play_pending()
        })
        || any::<ui::building_presets::BuildingPresetEditorState>(world, |state| {
            state.saved_play_pending()
        })
        || any::<ui::asset_preview::DocumentState>(world, |state| state.saved_play_pending())
        || any::<ui::asset_preview::DocumentBatchSaveState>(world, |state| state.exit_pending())
        || any::<ui::asset_preview::AnimationEditorState>(world, |state| {
            state.saved_play_pending()
        })
        || any::<feathrise_editor_surface_model::SurfacePresetEditorState>(world, |state| {
            state.write_task.is_some() || state.adoption.is_some()
        });
    if pending {
        Some("保存尚未完成，完成后请重新启动工程。草稿和历史已保留".into())
    } else {
        super::project_close::authors::saved_play_blocker(world)
            .map(|reason| format!("{reason}，完成后请重新启动工程；草稿和历史已保留"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn code_apply_captures_the_same_saved_map_without_saving_the_draft() {
        let mut world = World::new();
        world.insert_resource(resources::MapEditorState {
            selected_map_id: Some(feathrise_map_content::MapId::parse("MAP-apply-saved").unwrap()),
            dirty: true,
            ..default()
        });
        let request = current_saved_play_request(&world, feathrise_editor_core::ProjectBuildProfile::Release);
        assert_eq!(request.map_id.as_deref(), Some("MAP-apply-saved"));
        assert_eq!(request.profile, feathrise_editor_core::ProjectBuildProfile::Release);
        assert!(request.has_unsaved_changes && !request.stop_only);
        let editor = world.resource::<resources::MapEditorState>();
        assert!(editor.dirty && !editor.save_requested);
    }

    #[test]
    fn dirty_drafts_allow_saved_play_but_in_flight_explicit_save_rejects_it() {
        let mut world = World::new();
        world.insert_resource(resources::MapEditorState {
            open: true,
            dirty: true,
            ..default()
        });
        assert!(
            saved_play_blocker(&world).is_none(),
            "脏草稿仍可运行磁盘版本"
        );
        world.insert_resource(authoring::EditorAuthoringSession {
            saving: true,
            ..default()
        });
        assert!(saved_play_blocker(&world).unwrap().contains("保存尚未完成"));
        assert!(world.resource::<resources::MapEditorState>().dirty);
        world
            .resource_mut::<authoring::EditorAuthoringSession>()
            .saving = false;
        assert!(saved_play_blocker(&world).is_none());
    }
}
