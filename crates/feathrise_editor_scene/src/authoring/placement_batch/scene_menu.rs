//! 场景菜单冻结单个完整摆放，进入原复制/删除捕获槽及唯一后台事务。

use super::{
    resources::PlacementBatchState,
    systems::{BatchEnvironment, content},
    types::*,
};
use crate::{
    authoring::{EditorAuthoringSession, courtyards::CourtyardAuthoringState},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::scene_context_menu::{SceneContextMenuState, SceneMenuAction, SceneObjectKey},
    viewport::{
        multi_select::{PlacementSelectionSet, SelectedMember},
        placement_move::{MoveInputContext, PlacementMoveState},
    },
};
use bevy::prelude::*;
use std::sync::Arc;

pub(crate) fn input(
    menu: Option<Res<SceneContextMenuState>>,
    mut state: ResMut<PlacementBatchState>,
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    context: MoveInputContext,
    movement: Res<PlacementMoveState>,
    selection: Res<PlacementSelectionSet>,
    visuals: Res<CourtyardAuthoringState>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    env: BatchEnvironment,
) {
    let Some(request) = menu.as_ref().and_then(|menu| menu.ready.as_ref()) else {
        return;
    };
    let SceneObjectKey::Placement { index, id, .. } = &request.target.key else {
        return;
    };
    let action = match request.action {
        SceneMenuAction::Copy => PlacementBatchAction::Copy,
        SceneMenuAction::Delete => PlacementBatchAction::Delete,
        _ => return,
    };
    if !request.target.current(&editor, &session)
        || !request.target.allows(request.action)
        || state.busy()
        || session.model_vegetation_busy
        || !context.tree_batch_available(&editor)
        || movement.owns_mouse()
        || selection.owns_mouse()
        || guard.blocked()
        || visuals.operation_pending()
        || visuals.visual_sync_pending()
    {
        return;
    }
    let draft = session.draft.as_ref().unwrap();
    let record = &draft.placements.placements[*index];
    if !record.editor.selectable() {
        return;
    }
    let Some(bounds) = draft.manifest.world_bounds() else {
        return;
    };
    state.ticket = state.ticket.wrapping_add(1);
    state.capture = Some(Capture {
        model_vegetation: crate::authoring::model_vegetation::root_targets(draft),
        identity: BatchIdentity::from_session(&session, content(&env, &session)).unwrap(),
        action,
        records: Vec::with_capacity(1),
        members: Arc::new(vec![SelectedMember {
            id: id.clone(),
            index: *index,
            position: Vec3::from_array(record.translation),
        }]),
        bounds,
        source_objects: draft.manifest.terrain_objects_for_edit(),
        length: draft.placements.placements.len(),
        // 右键冻结的是单行。选中后 Cursor 阶段同步单项选集，不应使这个命令失效。
        selection_revision: None,
        selected_id: context.browser.selected_placement_id.clone(),
    });
    state.owns_mouse = true;
    editor.status = "正在准备右键对象的完整快照…".into();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ui::{
            gltf_browser::EditorGltfBrowserState,
            scene_context_menu::{SceneObjectRequest, SceneObjectTarget, select_target_for_tests},
        },
        viewport::{
            modes::{ViewportMode, ViewportModeState},
            multi_select::advance_multi_selection,
            placement_move::interaction_tests::app as base_app,
        },
    };
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn scene_context_menu_copy_and_delete_keep_the_complete_frozen_record_and_single_history() {
        for (action, cuboid) in [
            (SceneMenuAction::Copy, false),
            (SceneMenuAction::Delete, false),
            (SceneMenuAction::Delete, true),
        ] {
            let (mut app, _, _) = base_app();
            app.init_resource::<PlacementBatchState>()
                .init_resource::<PlacementSelectionSet>()
                .init_resource::<CourtyardAuthoringState>()
                .init_asset::<bevy::world_serialization::WorldAsset>()
                .add_systems(
                    Update,
                    (
                        super::super::handle_placement_batch,
                        super::super::sync_batch_selection,
                    )
                        .chain()
                        .before(crate::viewport::placement_move::handle_placement_move),
                );
            app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
            {
                let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
                editor.placement_transform_enabled = false;
                editor.tool = crate::types::EditorTool::Vegetation;
            }
            let target = {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                let draft = session.draft.as_mut().unwrap();
                let mut record = draft.placements.placements[0].clone();
                record.id = "right-clicked".into();
                if cuboid {
                    record.asset_path.clear();
                    record.asset_target = feathrise_scene_content::ScenePlacementAssetTarget::Cuboid {
                        size_mm: [1000; 3], color_srgb: [160; 3], roughness_milli: 600, collision: false,
                    };
                    record.water_surfaces.clear();
                }
                draft.placement_water_surface_count = 1 + record.water_surfaces.len();
                draft.placements.placements.push(record.clone());
                SceneObjectTarget {
                    scene_id: draft.scene_id.clone(),
                    revision: session.revision,
                    key: SceneObjectKey::Placement {
                        index: 1,
                        id: record.id,
                        mesh: None,
                    },
                    name: String::new(),
                    hidden: false,
                    locked: false,
                    copyable: true,
                }
            };
            let mut menu = SceneContextMenuState::default();
            menu.ready = Some(SceneObjectRequest {
                target,
                action,
                name: String::new(),
            });
            app.insert_resource(menu);
            {
                let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
                browser.select_indexed_test_placement("right-clicked", 1, 3);
                browser.selected_placement_id = Some("move-test".into());
            }
            app.world_mut().run_system_once(select_target_for_tests).unwrap();
            app.world_mut().run_system_once(input).unwrap();
            app.world_mut()
                .resource_mut::<SceneContextMenuState>()
                .ready = None;
            assert!(app.world().resource::<PlacementBatchState>().busy());
            // 正式 Cursor 同步把菜单选中的对象投影为选集，必须在后台捕获消费前实际换代。
            let revision = app.world().resource::<PlacementSelectionSet>().revision;
            app.world_mut().run_system_once(advance_multi_selection).unwrap();
            assert_ne!(app.world().resource::<PlacementSelectionSet>().revision, revision);
            let start = std::time::Instant::now();
            while app.world().resource::<PlacementBatchState>().busy() {
                app.update();
                assert!(start.elapsed().as_secs() < 5);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(app.world().resource::<SceneEditorState>().tool, crate::types::EditorTool::Vegetation);
            let records = &session.draft.as_ref().unwrap().placements.placements;
            assert_eq!(records[0].id, "move-test");
            if action == SceneMenuAction::Copy {
                let clipboard = app
                    .world()
                    .resource::<PlacementBatchState>()
                    .clipboard
                    .as_ref()
                    .unwrap();
                assert_eq!(clipboard.records.len(), 1);
                assert_eq!(clipboard.records[0].id, "right-clicked");
                assert_eq!(
                    clipboard.records[0].water_surfaces,
                    records[1].water_surfaces
                );
                assert_eq!(records.len(), 2);
                assert!(session.undo.is_empty());
            } else {
                assert_eq!(records.len(), 1);
                assert_eq!(session.undo.len(), 1);
                assert_eq!(
                    session
                        .draft
                        .as_ref()
                        .unwrap()
                        .placement_water_surface_count,
                    1
                );
            }
        }
    }
}
