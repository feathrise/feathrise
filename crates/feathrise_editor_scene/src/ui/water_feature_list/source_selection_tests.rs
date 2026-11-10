//! 根水与同ID私有水的真实选择回归；点击只恢复对应作者槽，不能串用像素或撤销来源。
use feathrise_editor_ui::UiInteraction;
use super::helpers::*;
use crate::authoring::{EditorAuthoringSession, EditorCommand,
    terrain_documents::{TerrainCanvas, TerrainDocumentIdentity, TerrainDocumentTask, root_water}};
use crate::ui::terrain_objects::TerrainObjectsEditorState;
use crate::{resources::SceneEditorState, types::{EditorTool, WaterAuthoringFeatureKind},
    viewport::SelectedWaterViewportState};
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use feathrise_scene_content::{SceneTerrainDescriptor, TerrainId};
use std::{collections::BTreeMap, sync::Arc};

/// 两个文档故意共享层ID；只靠层名、类型或索引隔离的选择桥必须在这里失败。
pub(crate) fn same_id_sources() -> (EditorAuthoringSession, TerrainId, String) {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("private-same-water-id").unwrap();
    let folder = object.private_folder(&draft.scene_id);
    object.water_path = Some(format!("{folder}/map.scene_water.json"));
    object.surface_path = None;
    object.vegetation_path = None;
    draft.manifest.terrain = SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_documents = Default::default();
    let mut root = crate::authoring::water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder);
    root.layers[0].id = "same-water-layer".into();
    root.layers[0].resolution = [4; 2];
    root.layers[0].water_level = 7.0;
    root.layers[0].label = "场景湖".into();
    let id = root.layers[0].id.clone();
    let mut private = root.clone();
    private.layers[0].water_level = 900.0;
    private.layers[0].label = "网格湖".into();
    draft.water = Some(root);
    draft.water_asset_path = format!("{}/root.scene_water.json", draft.folder);
    draft.water_masks = BTreeMap::from([(id.clone(), Arc::new(vec![17; 16]))]);
    draft.water_dirty_layers.clear();
    draft.terrain_documents.values.insert(object.id.clone(), Arc::new(TerrainCanvas {
        identity: Some(TerrainDocumentIdentity { definition: Arc::new(object.clone()),
            manifest: object.source_manifest(&draft.manifest), folder }),
        water: Some(private), water_asset_path: object.water_path.clone().unwrap(),
        water_masks: BTreeMap::from([(id.clone(), Arc::new(vec![211; 16]))]), ..default()
    }));
    (EditorAuthoringSession { draft: Some(draft), revision: 9, water_revision: 19, ..default() }, object.id, id)
}

#[test]
fn source_switch_waits_for_all_bounded_history_before_parking_same_id_water() {
    let (mut session, private, id) = same_id_sources();
    let mut editor = SceneEditorState::default();
    for _ in 0..130 {
        session.undo.push(EditorCommand::WaterLevelChanged { layer_id: id.clone(), before: 3.0, after: 7.0 });
    }
    for expected in [64, 128] {
        let error = focus_water_source(&mut editor, &mut session, Some(&private), false, None, None).unwrap_err();
        assert!(error.contains("撤销记录"));
        let draft = session.draft.as_ref().unwrap();
        assert!(draft.terrain_documents.active.is_none(), "有未归属历史时不能先切到目标私有槽");
        assert_eq!(draft.terrain_documents.scoped_through, expected);
        assert_eq!(session.revision, 9);
        assert_eq!(draft.water.as_ref().unwrap().layers[0].water_level, 7.0);
        assert!(session.undo[..expected].iter().all(|command| matches!(command, EditorCommand::RootWater(_))));
        assert!(session.undo[expected..].iter().all(|command| matches!(command, EditorCommand::WaterLevelChanged { .. })));
    }
    focus_water_source(&mut editor, &mut session, Some(&private), false, None, None).unwrap();
    assert!(session.undo.iter().all(|command| matches!(command, EditorCommand::RootWater(_))));
    assert_eq!(session.draft.as_ref().unwrap().water.as_ref().unwrap().layers[0].water_level, 900.0);
    for _ in 0..65 {
        session.undo.push(EditorCommand::WaterLevelChanged { layer_id: id.clone(), before: 899.0, after: 900.0 });
    }
    assert!(focus_water_source(&mut editor, &mut session, None, false, None, None).is_err());
    assert_eq!(session.draft.as_ref().unwrap().terrain_documents.active.as_ref().unwrap().definition.id, private);
    focus_water_source(&mut editor, &mut session, None, false, None, None).unwrap();
    assert!(session.undo[..130].iter().all(|command| matches!(command, EditorCommand::RootWater(_))));
    assert!(session.undo[130..].iter().all(|command| matches!(command, EditorCommand::TerrainCanvas { id, command }
        if id == &private && matches!(command.as_ref(), EditorCommand::WaterLevelChanged { .. }))));
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(draft.water.as_ref().unwrap().layers[0].water_level, 7.0);
    assert_eq!(draft.terrain_documents.values[&private].water.as_ref().unwrap().layers[0].water_level, 900.0);
    assert_eq!(session.undo.len(), 195);
    assert!(session.redo.is_empty() && !editor.dirty);
}

#[test]
fn rejected_source_selection_does_not_scope_park_or_clear_a_pending_canvas_request() {
    for reason in ["busy", "sparse-dirty", "definition-changed", "missing-id", "saving", "canvas-loading"] {
        let (mut session, private, id) = same_id_sources();
        session.undo.push(EditorCommand::WaterLevelChanged { layer_id: id.clone(), before: 3.0, after: 7.0 });
        let pixels = session.draft.as_ref().unwrap().water_masks[&id].clone();
        let mut editor = SceneEditorState { tool: EditorTool::Raise,
            cursor_world: Some(Vec3::new(1.0, 2.0, 3.0)), ..default() };
        let mut selection = TerrainObjectsEditorState::default();
        selection.selected = Some(private.clone());
        selection.selected_index = Some(0);
        selection.documents_loading = reason == "canvas-loading";
        let mut documents = TerrainDocumentTask::default();
        documents.requested = Some(private.clone());
        if reason == "sparse-dirty" { session.sparse_terrain_dirty = true; }
        if reason == "saving" { session.saving = true; }
        if reason == "definition-changed" {
            Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap())[0].transform.translation[0] += 1.0;
        }
        let requested_id = if reason == "missing-id" { "no-longer-present" } else { &id };
        assert!(activate_listed_water_feature(&mut editor, &mut session, Some(&private), reason == "busy",
            WaterAuthoringFeatureKind::StillWater, requested_id, Some(&mut selection), Some(&mut documents)).is_err(), "{reason}");
        let draft = session.draft.as_ref().unwrap();
        assert!(draft.terrain_documents.active.is_none(), "{reason}");
        assert_eq!(draft.terrain_documents.scoped_through, 0, "失败资格检查不能先改历史：{reason}");
        assert!(matches!(&session.undo[0], EditorCommand::WaterLevelChanged { .. }));
        assert!(Arc::ptr_eq(&draft.water_masks[&id], &pixels));
        assert!(draft.terrain_documents.values.contains_key(&private));
        assert_eq!(session.revision, 9);
        assert_eq!(session.water_revision, 19);
        assert_eq!(selection.selected.as_ref(), Some(&private));
        assert_eq!(selection.selected_index, Some(0));
        assert_eq!(selection.documents_loading, reason == "canvas-loading");
        assert_eq!(documents.requested.as_ref(), Some(&private));
        assert_eq!(editor.tool, EditorTool::Raise);
        assert_eq!(editor.cursor_world, Some(Vec3::new(1.0, 2.0, 3.0)));
        assert!(editor.terrain_canvas_target.is_none() && !editor.dirty);
    }
}

#[test]
fn same_active_private_source_rejects_loading_or_changed_definition_before_clearing_gates() {
    for reason in ["draft-loading", "selection-loading", "definition-changed"] {
        let (mut session, private, id) = same_id_sources();
        let mut editor = SceneEditorState::default();
        focus_water_source(&mut editor, &mut session, Some(&private), false, None, None).unwrap();
        let revision = session.revision;
        let mut selection = TerrainObjectsEditorState::default();
        selection.selected = Some(private.clone());
        selection.selected_index = Some(0);
        selection.documents_loading = reason == "selection-loading";
        let mut documents = TerrainDocumentTask::default();
        documents.requested = Some(private.clone());
        let draft = session.draft.as_mut().unwrap();
        draft.terrain_documents.loading = reason == "draft-loading";
        if reason == "definition-changed" {
            Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0].terrain.glb_path = "scenes/new/terrain.glb".into();
        }
        assert!(activate_listed_water_feature(&mut editor, &mut session, Some(&private), false,
            WaterAuthoringFeatureKind::StillWater, &id, Some(&mut selection), Some(&mut documents)).is_err(), "{reason}");
        let draft = session.draft.as_ref().unwrap();
        assert_eq!(draft.terrain_documents.loading, reason == "draft-loading");
        assert_eq!(selection.documents_loading, reason == "selection-loading");
        assert_eq!(documents.requested.as_ref(), Some(&private));
        assert_eq!(session.revision, revision);
        assert_eq!(draft.water.as_ref().unwrap().layers[0].water_level, 900.0);
        assert_eq!(root_water::source(draft).water.unwrap().layers[0].water_level, 7.0);
    }
}

#[test]
fn same_id_list_rows_highlight_their_source_and_cross_source_clicks_do_not_form_a_double_click() {
    use crate::{authoring::{WaterBrushStroke, WaterNetworkEditState}, components::*, resources::SceneEditorNumericScrubMouseGuard};
    use feathrise_editor_ui::EditorButtonClick;
    let (mut session, private, id) = same_id_sources();
    let mut editor = SceneEditorState { open: true, ..default() };
    activate_listed_water_feature(&mut editor, &mut session, Some(&private), false,
        WaterAuthoringFeatureKind::StillWater, &id, None, None).unwrap();
    assert_eq!(root_water::listed_count(session.draft.as_ref().unwrap()), 2);
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).insert_resource(editor).insert_resource(session)
        .init_resource::<SelectedWaterViewportState>().init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<WaterBrushStroke>().init_resource::<WaterNetworkEditState>();
    let rows = [0, 1].map(|slot| {
        let text = app.world_mut().spawn((SceneEditorWaterFeatureRowText, Text::new(""), TextColor::default())).id();
        app.world_mut().spawn((SceneEditorWaterFeatureRow { slot,
            kind: WaterAuthoringFeatureKind::StillWater, feature_id: None, terrain: None },
            Node::default(), BackgroundColor::default(), UiInteraction::None, EditorButtonClick::default())).add_child(text).id()
    });
    app.world_mut().run_system_once(super::sync_water_feature_rows).unwrap();
    app.world_mut().run_system_once(super::sync_water_feature_row_interactions).unwrap();
    assert!(app.world().get::<SceneEditorWaterFeatureRow>(rows[0]).unwrap().terrain.is_none());
    assert_eq!(app.world().get::<SceneEditorWaterFeatureRow>(rows[1]).unwrap().terrain.as_ref(), Some(&private));
    assert_eq!(app.world().get::<BackgroundColor>(rows[0]).unwrap().0, crate::ui::theme::SLOT_BG);
    assert_eq!(app.world().get::<BackgroundColor>(rows[1]).unwrap().0, crate::ui::theme::SELECT_BG);
    // 不推进真实时钟：同ID私有行→根行不能组成双击；根行再点一次仍保留真正双击。
    for (row, expects_focus) in [(rows[1], false), (rows[0], false), (rows[0], true)] {
        app.world_mut().entity_mut(row).insert(EditorButtonClick::triggered());
        app.world_mut().run_system_once(super::handle_water_feature_row_clicks).unwrap();
        assert_eq!(app.world().resource::<SceneEditorState>().status.contains("正在定位"), expects_focus);
        app.world_mut().entity_mut(row).insert(EditorButtonClick::default());
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert!(draft.terrain_documents.active.is_none());
    assert_eq!(draft.water.as_ref().unwrap().layers[0].water_level, 7.0);
    assert_eq!(draft.terrain_documents.values[&private].water.as_ref().unwrap().layers[0].water_level, 900.0);
    assert!(session.undo.is_empty() && session.redo.is_empty());
}
