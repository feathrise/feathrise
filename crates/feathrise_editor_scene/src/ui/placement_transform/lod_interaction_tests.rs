//! 对象LOD的真实按钮、公共数值消息、预览元数据和完整记录撤销回归。
use super::*;
use feathrise_scene_content::{MeshLodMode, MeshLodPolicy};
use crate::ui::placement_transform::{PlacementTransformAction as Action, SceneEditorPlacementTransformActionButton};

fn click(app: &mut App, action: Action) {
    let button = app.world_mut().spawn((feathrise_editor_ui::EditorButtonClick::triggered(),
        SceneEditorPlacementTransformActionButton(action))).id();
    app.world_mut().run_system_once(crate::ui::placement_transform::handle_placement_transform_actions).unwrap();
    app.world_mut().despawn(button);
}

fn policy(app: &App) -> MeshLodPolicy { record(app).mesh_lod.unwrap_or_default() }

#[test]
fn mesh_lod_buttons_keep_channels_independent_and_update_preview_for_builtin_and_glb() {
    for builtin in [true, false] {
        let (mut app, _, root) = app(PlacementTransformField::LodScreenError);
        if !builtin {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
            record.asset_path = "gltf/test.glb".into(); record.asset_target = Default::default();
        }
        let before = record(&app).clone();
        let global = app.world().resource::<EditorSettings>().render.mesh_lod;
        click(&mut app, Action::DisplayLodInherit);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        click(&mut app, Action::DisplayLodCustom);
        assert!(matches!(policy(&app).display, MeshLodMode::Override(_)));
        assert_eq!(policy(&app).collision, MeshLodMode::Inherit);
        click(&mut app, Action::CollisionLodOriginal);
        assert_eq!(policy(&app).collision, MeshLodMode::Disabled);
        assert!(matches!(policy(&app).display, MeshLodMode::Override(_)));
        click(&mut app, Action::DisplayLodOriginal);
        assert_eq!(policy(&app).display, MeshLodMode::Disabled);
        assert_eq!(app.world().get::<ScenePlacementInstance>(root).unwrap().mesh_lod_policy(), policy(&app));
        let after = record(&app).clone();
        assert_eq!(after.asset_target, before.asset_target); // 关闭简化不能移除碰撞或改写作者几何。
        assert_eq!(after.translation, before.translation);
        assert_eq!(app.world().resource::<EditorSettings>().render.mesh_lod, global);
        assert_eq!(serde_json::from_str::<ScenePlacementRecord>(&serde_json::to_string(&after).unwrap()).unwrap(), after);
        crate::viewport::placement_move::interaction_tests::batch_undo_resources(&mut app);
        for undo in [true, false] {
            app.world_mut().resource_mut::<SceneEditorState>().undo_requested = undo;
            app.world_mut().resource_mut::<SceneEditorState>().redo_requested = !undo;
            app.world_mut().run_system_once(crate::authoring::apply_undo_redo).unwrap();
            assert_eq!(app.world().get::<ScenePlacementInstance>(root).unwrap().mesh_lod_policy(), policy(&app));
            if undo { assert!(matches!(policy(&app).display, MeshLodMode::Override(_))); }
            else { assert_eq!(record(&app), &after); }
        }
        click(&mut app, Action::DisplayLodInherit);
        assert_eq!(policy(&app).collision, MeshLodMode::Disabled);
        click(&mut app, Action::CollisionLodInherit);
        assert_eq!(record(&app).mesh_lod, None);
    }
}

#[test]
fn mesh_lod_building_properties_update_the_existing_root_without_creating_a_second_preview() {
    let (mut app, input, root) = app(PlacementTransformField::LodScreenError);
    app.add_observer(crate::ui::placement_water::register_placement_instance_root);
    let (map, before) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.placement_previews.clear(); // 建筑复用已登记活动根，而非普通网格的独立作者预览。
        let draft = session.draft.as_mut().unwrap();
        let record = &mut draft.placements.placements[0];
        record.asset_path = "gltf/test.glb".into(); record.asset_target = Default::default();
        record.building_preset_id = Some("lod-test-building".into());
        (draft.scene_id.clone(), record.clone())
    };
    app.world_mut().entity_mut(root).remove::<ScenePlacementInstance>().insert(ScenePlacementInstance::new(&map, &before));
    click(&mut app, Action::DisplayLodCustom);
    commit(&mut app, input, 2.5, NumericInputCommitSource::Text, true);
    assert_eq!(PlacementTransformField::LodScreenError.value(record(&app)), 2.5);
    assert_eq!(app.world().get::<ScenePlacementInstance>(root).unwrap().mesh_lod_policy(), policy(&app));
    assert!(app.world().resource::<EditorAuthoringSession>().placement_previews.is_empty());
    assert_eq!(record(&app).translation, before.translation);
    assert_eq!(record(&app).asset_target, before.asset_target);
}

#[test]
fn mesh_lod_numeric_fields_preserve_pose_and_scrub_commits_one_complete_history() {
    let (mut app, input, root) = app(PlacementTransformField::LodScreenError);
    click(&mut app, Action::DisplayLodCustom);
    click(&mut app, Action::CollisionLodCustom);
    let before = record(&app).clone();
    for (field, value) in [(PlacementTransformField::LodScreenError, 2.5),
        (PlacementTransformField::LodHysteresis, 0.2), (PlacementTransformField::LodSafeNear, 64.0),
        (PlacementTransformField::LodCollisionError, 0.0001)] {
        app.world_mut().entity_mut(input).insert(field);
        commit(&mut app, input, value, NumericInputCommitSource::Text, true);
        assert_eq!(field.value(record(&app)), value);
        assert_eq!(record(&app).translation, before.translation);
        assert_eq!(record(&app).terrain_binding, before.terrain_binding);
        assert_eq!(record(&app).asset_target, before.asset_target);
        assert_eq!(app.world().get::<ScenePlacementInstance>(root).unwrap().mesh_lod_policy(), policy(&app));
    }
    let before_scrub = record(&app).clone();
    let history = app.world().resource::<EditorAuthoringSession>().undo.len();
    for value in [0.001, 0.002, 0.003] {
        app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(value) });
        app.update();
    }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), history);
    commit(&mut app, input, 0.003, NumericInputCommitSource::Scrub, true);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), history + 1);
    crate::viewport::placement_move::interaction_tests::batch_undo_resources(&mut app);
    app.world_mut().resource_mut::<SceneEditorState>().undo_requested = true;
    app.world_mut().run_system_once(crate::authoring::apply_undo_redo).unwrap();
    assert_eq!(record(&app), &before_scrub);
    assert_eq!(app.world().get::<ScenePlacementInstance>(root).unwrap().mesh_lod_policy(), policy(&app));
}

#[test]
fn mesh_lod_hidden_and_stale_text_inputs_cannot_reenable_or_retarget_overrides() {
    let (mut app, input, _) = app(PlacementTransformField::LodScreenError);
    click(&mut app, Action::DisplayLodCustom);
    app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(input);
    app.update(); // 原公共业务桥冻结当前作者修订和对象身份。
    click(&mut app, Action::DisplayLodOriginal);
    let before = record(&app).clone();
    let history = app.world().resource::<EditorAuthoringSession>().undo.len();
    commit(&mut app, input, 4.0, NumericInputCommitSource::Text, true);
    assert_eq!(record(&app), &before);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), history);

    click(&mut app, Action::DisplayLodCustom);
    app.update();
    let original = record(&app).clone();
    let mut second = original.clone(); second.id = "lod-second".into();
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().placements.placements.push(second.clone());
    app.world_mut().resource_mut::<EditorGltfBrowserState>().select_indexed_test_placement("lod-second", 1, 3);
    commit(&mut app, input, 4.0, NumericInputCommitSource::Text, true);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, [original, second]);
}
