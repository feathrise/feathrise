//! 只在工程关闭请求和收尾阶段扫描轻量会话标记；稳定帧不枚举标签或复制正文。

use super::types::*;
use crate::{authoring, resources, ui, viewport};
use bevy::prelude::*;

fn any<T: Resource>(world: &World, check: impl FnOnce(&T) -> bool) -> bool {
    world.get_resource::<T>().is_some_and(check)
}

pub(super) fn collect(world: &World) -> (ProjectCloseSnapshot, Vec<SaveItem>) {
    let mut snapshot = ProjectCloseSnapshot::default();
    let mut items = Vec::new();
    scene_pending(world, &mut snapshot);
    input_blocker(world, &mut snapshot);

    // 文件/动画先写原源文件，外部作者按应用登记顺序交接，最后保存场景引用。
    ui::asset_preview::documents::project_close::collect(world, &mut snapshot, &mut items);
    ui::asset_preview::animation::project_close::collect(world, &mut snapshot, &mut items);
    ui::asset_preview::weights::project_close::collect(world, &mut snapshot, &mut items);
    if any::<ui::asset_preview::rigging::RiggingState>(world, |state| state.exit_pending()) {
        snapshot.blocked("自动绑骨任务仍在收尾，请等待完成或取消");
    }
    super::presets::collect(world, &mut snapshot, &mut items);
    ui::vegetation_preset_editor::project_close::collect(world, &mut snapshot, &mut items);
    ui::water_preset_editor::project_close::collect(world, &mut snapshot, &mut items);
    ui::building_presets::project_close::collect(world, &mut snapshot, &mut items);
    collect_scenes(world, &mut snapshot, &mut items);
    (snapshot, items)
}

fn collect_scenes(world: &World, snapshot: &mut ProjectCloseSnapshot, items: &mut Vec<SaveItem>) {
    let editor = world.get_resource::<resources::SceneEditorState>();
    let session = world.get_resource::<authoring::EditorAuthoringSession>();
    let mut included = std::collections::BTreeSet::new();
    if let Some(session) = session
        && let Some(draft) = session.draft.as_ref()
        && !session.is_unedited_load_migration_for(&draft.scene_id)
        && (session.has_scene_data_changes() || editor.is_some_and(|editor| editor.dirty))
    {
        included.insert(draft.scene_id.clone());
        snapshot.draft(
            items,
            SaveTarget::Scene(draft.scene_id.clone()),
            format!("场景：{} [{}]", draft.manifest.display_name, draft.scene_id),
        );
    }
    if let Some(documents) = world.get_resource::<ui::scene_documents::SceneDocuments>() {
        // 原标签顺序决定队列；parked是唯一草稿所有者，不从运行时场景重造快照。
        for tab in &documents.tabs {
            let parked = documents.parked.get(&tab.scene_id);
            let migration =
                parked.is_some_and(|doc| doc.session.is_unedited_load_migration_for(&tab.scene_id));
            let dirty = parked.map_or(tab.dirty, |doc| {
                doc.dirty || doc.session.has_scene_data_changes()
            });
            let current_migration =
                session.is_some_and(|session| session.is_unedited_load_migration_for(&tab.scene_id));
            if dirty && !migration && !current_migration && included.insert(tab.scene_id.clone()) {
                snapshot.draft(
                    items,
                    SaveTarget::Scene(tab.scene_id.clone()),
                    format!("场景：{} [{}]", tab.name, tab.scene_id),
                );
            }
        }
        // 防御历史交接中的草稿暂不在可见tab里；不让hidden所有者被“没有星号”绕过。
        for (map, doc) in &documents.parked {
            if !doc.session.is_unedited_load_migration_for(map)
                && (doc.dirty || doc.session.has_scene_data_changes())
                && included.insert(map.clone())
            {
                snapshot.draft(
                    items,
                    SaveTarget::Scene(map.clone()),
                    format!("后台场景：{map}"),
                );
            }
        }
    }
    if any::<authoring::GlobalEnvironmentAuthoringState>(world, |state| state.dirty) {
        snapshot.draft(items, SaveTarget::GlobalEnvironment, "全局环境/天体配置");
    }
    if editor.is_some_and(|editor| editor.dirty)
        && session.is_none_or(|session| session.draft.is_none())
        && !any::<authoring::GlobalEnvironmentAuthoringState>(world, |state| state.dirty)
    {
        snapshot.blocked("当前作者脏状态没有完整场景来源，请返回原编辑器确认加载诊断后重试");
    }
}

fn scene_pending(world: &World, snapshot: &mut ProjectCloseSnapshot) {
    if any::<feathrise_editor_project_browser_model::file_operations::FileOperationService>(world, |service| service.is_busy()) {
        snapshot.pending("文件整理任务仍在处理中，正在保留工程并等待文件回执");
    }
    if any::<resources::SceneEditorState>(world, |state| {
        state.save_requested
            || state.scene_transition_in_progress
            || state.pending_scene_transition.is_some()
    }) || any::<authoring::EditorAuthoringSession>(world, |state| state.saving)
        || any::<authoring::DraftSaveTask>(world, |state| state.task.is_some())
        || any::<authoring::GlobalEnvironmentSaveTask>(world, |state| state.task.is_some())
        || any::<resources::SceneCreationTask>(world, |state| state.task.is_some())
        || any::<resources::SceneDeletionTask>(world, |state| {
            state.task.is_some() || state.pending_start.is_some()
        })
        || any::<resources::SceneFolderPersistence>(world, |state| state.busy())
        || any::<authoring::DraftLoadTask>(world, |state| state.task.is_some())
        || any::<ui::scene_documents::SceneDocuments>(world, |state| {
            state.history_task.is_some()
                || state.pending_history.is_some()
                || state.preparing_target.is_some()
                || state.close_requested.is_some()
                || state.closing.is_some()
        })
        || any::<authoring::EditorAuthoringSession>(world, |state| state.vegetation_cross_busy)
        || any::<authoring::SparseTerrainAuthoringState>(world, |state| !state.document_can_park())
        || any::<authoring::VegetationCompileState>(world, |state| state.task.is_some())
        || any::<authoring::WaterMaskMaterializeTask>(world, |state| {
            state.task.is_some() || state.terrain_task.is_some()
        })
        || any::<ui::terrain_objects::TerrainObjectsEditorState>(world, |state| state.busy())
        || any::<ui::scene_objects::SceneObjectState>(world, |state| state.busy())
        || any::<authoring::placement_batch::PlacementBatchState>(world, |state| state.busy())
        || any::<authoring::SurfaceLayerEditTask>(world, |state| state.is_pending())
        || any::<authoring::PuddleAuthoringState>(world, |state| state.busy())
        || any::<ui::subscenes::SubsceneEditorState>(world, |state| state.busy())
        || any::<ui::vegetation_layers::VegetationBlankLayerAllocation>(world, |state| state.busy())
        || any::<authoring::courtyards::CourtyardAuthoringState>(world, |state| {
            state.save_pending() || state.visual_sync_pending()
        })
        || any::<authoring::vegetation_paint_scope::VegetationPaintState>(world, |state| {
            state.busy()
        })
        || authoring::buildings::exit_pending(world)
        || any::<ui::asset_preview::AssetReplaceState>(world, |state| state.exit_pending())
    {
        snapshot.pending("原场景、图层、对象或文件事务尚未完成，正在保留作者现场等待");
    }
    if let Some(session) = world.get_resource::<authoring::EditorAuthoringSession>()
        && let Some(draft) = session.draft.as_ref()
    {
        if let Some(handoff) = world.get_resource::<authoring::WaterFileHandoff>() {
            if handoff.unresolved_reason_for(&draft.scene_id).is_none()
                && handoff.pending_for(
                    &draft.scene_id,
                    session.water_revision,
                    session.terrain_revision,
                )
            {
                snapshot.pending("正在等待原水域/摆放正式保存交接");
            }
        }
    }
}

fn input_blocker(world: &World, snapshot: &mut ProjectCloseSnapshot) {
    // 宿主overlay会主动设置block_shortcuts；这里只检查真实作者输入，不读取该派生门。
    if any::<resources::SceneEditorState>(world, |state| {
        state.new_scene_dialog_open
            || state.scene_switch_confirmation.is_some()
            || state.portal_name_edit.active
    }) || any::<resources::SceneBrowserState>(world, |state| state.dialog.is_some())
    {
        snapshot.blocked("请先完成或取消原场景/目录/名称对话框，再关闭工程");
    }
    if any::<authoring::TerrainBrushStroke>(world, |stroke| stroke.active)
        || any::<authoring::SurfaceBrushStroke>(world, |stroke| stroke.active)
        || any::<authoring::VegetationBrushStroke>(world, |stroke| stroke.active)
        || any::<authoring::WaterBrushStroke>(world, |stroke| stroke.active)
        || any::<authoring::BoundaryEditState>(world, |state| state.dragging())
        || any::<authoring::WaterNetworkEditState>(world, |state| state.dragging())
        || any::<viewport::placement_move::PlacementMoveState>(world, |state| state.exit_pending())
        || any::<authoring::mesh_edit::MeshEditState>(world, |state| state.moving())
        || any::<viewport::subscenes::SubsceneTransformState>(world, |state| state.owns_mouse())
        || any::<authoring::BoundaryThicknessInputState>(world, |state| state.busy())
        || any::<feathrise_editor_ui::ActiveNumericInput>(world, |state| state.entity.is_some())
    {
        snapshot.blocked("请先松开当前拖动、用Enter提交或Esc取消数值/笔划，再关闭工程");
    }
    if world
        .get_resource::<bevy::input_focus::InputFocus>()
        .and_then(|focus| focus.get())
        .and_then(|entity| world.get::<bevy::text::EditableText>(entity))
        .is_some_and(|text| {
            text.is_composing() || text.pending_paste.is_some() || !text.pending_edits.is_empty()
        })
    {
        snapshot.blocked("文字或输入法候选尚未收口，请先完成或取消输入再关闭工程");
    }
}
