//! 独立窗口退出的只读保护：复用所有原作者会话脏位与任务槽，不另建保存或丢弃算法。

use crate::{authoring, resources, ui, viewport};
use bevy::prelude::*;

fn any<T: Resource>(world: &World, check: impl FnOnce(&T) -> bool) -> bool {
    world.get_resource::<T>().is_some_and(check)
}

/// 仅窗口关闭请求调用；不在稳定帧扫描作者标签，不读取文件、地图实体或完整撤销正文。
///
/// 拒绝退出时由宿主把原因回显到原状态栏；作者继续用原保存及原标签关闭确认收口。
pub fn exit_blocker(world: &World) -> Option<String> {
    let pending = "作者事务尚未完成，已保留编辑器，请等待完成或先结束当前编辑再关闭窗口";
    if any::<resources::MapEditorState>(world, |state| {
        state.save_requested || state.map_transition_in_progress
    }) || any::<authoring::EditorAuthoringSession>(world, |state| {
        state.saving || state.vegetation_cross_busy
    }) || any::<authoring::DraftSaveTask>(world, |state| state.task.is_some())
        || any::<authoring::GlobalEnvironmentSaveTask>(world, |state| state.task.is_some())
        || any::<resources::MapCreationTask>(world, |state| state.task.is_some())
        || any::<resources::MapDeletionTask>(world, |state| state.busy())
        || any::<resources::MapFolderPersistence>(world, |state| state.busy())
        || any::<ui::terrain_objects::TerrainObjectsEditorState>(world, |state| state.busy())
        || any::<ui::scene_objects::SceneObjectState>(world, |state| state.busy())
        || any::<authoring::placement_batch::PlacementBatchState>(world, |state| state.busy())
        || authoring::buildings::exit_pending(world)
        || any::<authoring::courtyards::CourtyardAuthoringState>(world, |state| {
            state.save_pending()
        })
        || any::<authoring::SurfaceLayerEditTask>(world, |state| state.is_pending())
        || any::<authoring::PuddleAuthoringState>(world, |state| state.busy())
        || any::<authoring::vegetation_paint_scope::VegetationPaintState>(world, |state| {
            state.busy()
        })
        || any::<authoring::TerrainBrushStroke>(world, |state| state.active)
        || any::<authoring::SurfaceBrushStroke>(world, |state| state.active)
        || any::<authoring::VegetationBrushStroke>(world, |state| state.active)
        || any::<authoring::WaterBrushStroke>(world, |state| state.active)
        || any::<authoring::BoundaryEditState>(world, |state| state.dragging())
        || any::<authoring::WaterNetworkEditState>(world, |state| state.dragging())
        || any::<viewport::placement_move::PlacementMoveState>(world, |state| state.exit_pending())
        || any::<authoring::mesh_edit::MeshEditState>(world, |state| state.moving())
    {
        return Some(pending.into());
    }
    let active = world
        .get_resource::<resources::MapEditorState>()
        .and_then(|state| state.selected_map_id.as_ref());
    // 原作者明确允许无编辑的加载兼容补齐下次重新生成；退出继续使用原受检身份规则。
    let unedited_migration = active.is_some_and(|map| {
        world
            .get_resource::<authoring::EditorAuthoringSession>()
            .is_some_and(|session| session.is_unedited_load_migration_for(map))
    });
    if (!unedited_migration
        && (any::<resources::MapEditorState>(world, |state| state.dirty)
            || any::<authoring::EditorAuthoringSession>(world, |state| {
                state.has_map_data_changes()
            })))
        || any::<authoring::GlobalEnvironmentAuthoringState>(world, |state| state.dirty)
    {
        return Some(
            "当前地图或全局环境仍有未保存内容，请先使用原保存操作；编辑器与撤销历史已保留".into(),
        );
    }
    if let Some(reason) = world
        .get_resource::<ui::map_documents::MapDocuments>()
        .and_then(|state| state.exit_blocker(active, unedited_migration))
    {
        return Some(reason.into());
    }
    if let Some(reason) = world
        .get_resource::<ui::asset_preview::DocumentState>()
        .and_then(|state| state.exit_blocker())
    {
        return Some(reason.into());
    }
    if any::<ui::asset_preview::DocumentBatchSaveState>(world, |state| state.exit_pending())
        || any::<ui::asset_preview::AssetReplaceState>(world, |state| state.exit_pending())
    {
        return Some("文件批量保存或替换仍在处理中，请等待原事务完成；编辑器已保留".into());
    }
    if let Some(reason) = world
        .get_resource::<ui::asset_preview::AnimationEditorState>()
        .and_then(|state| state.exit_blocker())
    {
        return Some(reason.into());
    }
    if let Some(reason) = world
        .get_resource::<ui::vegetation_preset_editor::VegetationPresetEditorState>()
        .and_then(|state| state.exit_blocker())
    {
        return Some(reason.into());
    }
    if let Some(reason) = world
        .get_resource::<ui::water_preset_editor::WaterPresetEditorState>()
        .and_then(|state| state.exit_blocker())
    {
        return Some(reason.into());
    }
    if let Some(reason) = world
        .get_resource::<ui::building_presets::BuildingPresetEditorState>()
        .and_then(|state| state.exit_blocker())
    {
        return Some(reason.into());
    }
    if let Some(state) = world.get_resource::<feathrise_editor_surface_model::SurfacePresetEditorState>()
    {
        if state.busy() {
            return Some("地表预设写入或热采用尚未完成，请等待原事务完成".into());
        }
        if state.active_field.is_some() {
            return Some("地表预设字段尚未完成，请先Enter提交或Esc取消".into());
        }
        if state.tabs.iter().any(|tab| {
            tab.draft.dirty()
                || matches!(
                    tab.draft.mode,
                    feathrise_editor_surface_model::types::SurfaceDraftMode::Create
                )
        }) {
            return Some("存在未保存的地表预设，请先保存或使用原标签关闭确认丢弃".into());
        }
    }
    if let Some(reason)=super::project_close::authors::exit_blocker(world) { return Some(reason); }
    // 只读取官方当前焦点实体，不能为退出去遍历整棵UI或提前提交输入法候选。
    if world
        .get_resource::<bevy::input_focus::InputFocus>()
        .and_then(|focus| focus.get())
        .and_then(|entity| world.get::<bevy::text::EditableText>(entity))
        .is_some_and(|text| {
            text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty()
        })
        || any::<resources::EditorTextInputGuard>(world, |guard| guard.block_shortcuts)
    {
        return Some("文字或输入法候选尚未收口，请先完成或取消输入再关闭窗口".into());
    }
    if world
        .get_resource::<feathrise_editor_ui::ActiveNumericInput>()
        .and_then(|active| active.entity)
        .and_then(|entity| world.get::<feathrise_editor_ui::NumericInputEditState>(entity))
        .is_some_and(|state| state.editing)
        || any::<resources::MapEditorNumericScrubMouseGuard>(world, |guard| guard.blocked())
    {
        return Some("数值输入或拖动尚未收口，请先Enter提交、Esc取消或松开鼠标再关闭窗口".into());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_author_state_can_exit_but_current_dirty_and_parked_map_cannot() {
        let mut world = World::new();
        world.init_resource::<resources::MapEditorState>();
        world.init_resource::<authoring::EditorAuthoringSession>();
        world.init_resource::<ui::map_documents::MapDocuments>();
        assert!(exit_blocker(&world).is_none());
        world.resource_mut::<resources::MapEditorState>().dirty = true;
        assert!(exit_blocker(&world).unwrap().contains("未保存"));
        world.resource_mut::<resources::MapEditorState>().dirty = false;
        world
            .resource_mut::<ui::map_documents::MapDocuments>()
            .upsert(
                feathrise_map_content::MapId::parse("MAP-parked-dirty").unwrap(),
                "后台地图".into(),
                true,
            );
        assert!(exit_blocker(&world).unwrap().contains("地图文档"));
    }

    #[test]
    fn in_progress_stroke_and_official_pending_text_reject_exit_without_mutating_them() {
        let mut world = World::new();
        world.insert_resource(authoring::TerrainBrushStroke {
            active: true,
            ..default()
        });
        assert!(exit_blocker(&world).is_some());
        assert!(world.resource::<authoring::TerrainBrushStroke>().active);
        world.resource_mut::<authoring::TerrainBrushStroke>().active = false;
        let input = world.spawn(bevy::text::EditableText::default()).id();
        world
            .entity_mut(input)
            .get_mut::<bevy::text::EditableText>()
            .unwrap()
            .queue_edit(bevy::text::TextEdit::Insert("尚未提交".into()));
        world.insert_resource(bevy::input_focus::InputFocus::from_entity(input));
        assert!(exit_blocker(&world).unwrap().contains("文字"));
        assert!(
            !world
                .entity(input)
                .get::<bevy::text::EditableText>()
                .unwrap()
                .pending_edits
                .is_empty()
        );
    }

}
