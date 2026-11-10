//! 运行正式数值消息、可视根同步及撤销入口，验证立方体参数与对象身份共同提交。

use bevy::ecs::system::RunSystemOnce;
use feathrise_scene_content::ScenePlacementAssetTarget;

use super::*;

#[path = "numeric_event_tests.rs"]
mod event_tests;

fn app(field: PlacementTransformField) -> (App, Entity, Entity) {
    let (mut app, root, _) = crate::viewport::placement_move::interaction_tests::app();
    app.init_resource::<EditorSettings>()
        .init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .add_message::<NumericInputChanged>()
        .add_message::<NumericInputCommitted>()
        .add_message::<NumericInputScrubSensitivityChanged>()
        .add_message::<NumericInputScrubSensitivityCommitted>()
        .add_message::<NumericInputSetValue>()
        .add_systems(Update, handle_placement_transform_numeric_input.after(feathrise_editor_ui::EditorUiSystemSet::NumericInput));
    let (map, record) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.placement_water_surface_count = 0;
        let record = &mut draft.placements.placements[0];
        record.asset_path.clear();
        record.asset_target = ScenePlacementAssetTarget::DEFAULT_CUBOID;
        record.water_surfaces.clear();
        (draft.scene_id.clone(), record.clone())
    };
    app.world_mut()
        .entity_mut(root)
        .insert(ScenePlacementInstance::new(&map, &record));
    let input = app.world_mut().spawn(field).id();
    (app, input, root)
}

fn record(app: &App) -> &ScenePlacementRecord {
    &app.world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements[0]
}

fn commit(
    app: &mut App,
    entity: Entity,
    value: f32,
    source: NumericInputCommitSource,
    changed: bool,
) {
    app.world_mut().write_message(NumericInputCommitted {
        entity,
        value: NumericValue::F32(value),
        outcome: NumericInputCommitOutcome::Accepted,
        source,
        changed,
        edited: true,
    });
    app.update();
}

#[test]
fn terrain_patch_numeric_cutout_rejects_source_overflow_without_committing() {
    // 容器范围较大不能授权把洞口写出实际关联源；走真实字符及左拖消息验证采用门。
    for scrub in [false, true] {
        let (mut app, input, root) = app(PlacementTransformField::CutoutMinX);
        let (map, before) = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            draft.manifest.world_min = [-20.0; 2];
            draft.manifest.world_max = [20.0; 2];
            let mut source = draft.manifest.terrain_objects_for_edit()[0].clone();
            source.id = feathrise_scene_content::TerrainId::parse("numeric-patch-parent").unwrap();
            source.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
            source.world_min = [-2.0; 2];
            source.world_max = [2.0; 2];
            let terrain_id = source.id.clone();
            draft.manifest.terrains = Some(std::sync::Arc::from([source]));
            draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
            let record = &mut draft.placements.placements[0];
            record.asset_path = "gltf/test.glb".into();
            record.asset_target = Default::default();
            record.translation = [0.0; 3];
            record.yaw_degrees = 0.0;
            record.scale = [1.0; 3];
            record.terrain_patch = Some(feathrise_scene_content::ScenePlacementTerrainPatch {
                terrain_id: Some(terrain_id),
                cutout: Some(feathrise_scene_content::SceneTerrainCutout { min: [-1.0; 2], max: [1.0; 2] }),
                ..Default::default()
            });
            (draft.scene_id.clone(), record.clone())
        };
        app.world_mut().entity_mut(root).insert(ScenePlacementInstance::new(&map, &before));
        let previews_before = app.world().resource::<EditorAuthoringSession>().placement_previews.clone();
        let visibility_before = *app.world().get::<Visibility>(root).unwrap();
        if scrub {
            app.world_mut().write_message(NumericInputChanged { entity: input, value: NumericValue::F32(-3.0) });
            app.update();
        } else {
            commit(&mut app, input, -3.0, NumericInputCommitSource::Text, true);
        }
        assert_eq!(record(&app), &before, "source bounds must reject before adopting the numeric candidate");
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.revision, 7);
        assert!(session.undo.is_empty());
        assert_eq!(session.placement_previews, previews_before);
        assert_eq!(app.world().get::<Visibility>(root), Some(&visibility_before));
    }
}

#[test]
fn cuboid_size_scrub_updates_real_mesh_and_commits_one_undo_redo() {
    let (mut app, input, root) = app(PlacementTransformField::CuboidSizeY);
    let before = record(&app).clone();
    for value in [2.25, 2.75, 3.125] {
        app.world_mut().write_message(NumericInputChanged {
            entity: input,
            value: NumericValue::F32(value),
        });
        app.update();
        assert_eq!(
            PlacementTransformField::CuboidSizeY.value(record(&app)),
            value
        );
        assert_eq!(record(&app).scale, before.scale);
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
        let children = app
            .world()
            .get::<Children>(root)
            .expect("尺寸拖动应安装实际可视子节点");
        assert_eq!(
            children
                .iter()
                .filter(|entity| app.world().get::<Mesh3d>(*entity).is_some())
                .count(),
            1
        );
    }
    commit(
        &mut app,
        input,
        3.125,
        NumericInputCommitSource::Scrub,
        true,
    );
    let after = record(&app).clone();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(
        matches!(&session.undo[0], EditorCommand::PlacementChanged { before: old, after: new, .. } if old == &before && new == &after)
    );
    crate::viewport::placement_move::interaction_tests::batch_undo_resources(&mut app);
    for undo in [true, false] {
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .undo_requested = undo;
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .redo_requested = !undo;
        app.world_mut()
            .run_system_once(crate::authoring::apply_undo_redo)
            .unwrap();
        assert_eq!(record(&app), if undo { &before } else { &after });
        assert_eq!(
            app.world()
                .get::<ScenePlacementInstance>(root)
                .unwrap()
                .asset_target(),
            record(&app).asset_target
        );
    }
}

#[test]
fn cuboid_material_exact_input_updates_real_pbr_and_serializes_complete_record() {
    let (mut app, input, root) = app(PlacementTransformField::CuboidRoughness);
    let before = record(&app).clone();
    commit(&mut app, input, 0.375, NumericInputCommitSource::Text, true);
    assert_eq!(
        PlacementTransformField::CuboidRoughness.value(record(&app)),
        0.375
    );
    assert_eq!(record(&app).scale, before.scale);
    let child = app
        .world()
        .get::<Children>(root)
        .unwrap()
        .iter()
        .find(|entity| {
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(*entity)
                .is_some()
        })
        .unwrap();
    let material = &app
        .world()
        .get::<MeshMaterial3d<StandardMaterial>>(child)
        .unwrap()
        .0;
    assert_eq!(
        app.world()
            .resource::<Assets<StandardMaterial>>()
            .get(material)
            .unwrap()
            .perceptual_roughness,
        0.375
    );
    let restored: ScenePlacementRecord =
        serde_json::from_str(&serde_json::to_string(record(&app)).unwrap()).unwrap();
    assert_eq!(&restored, record(&app));
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn cuboid_scrub_release_after_selection_change_restores_without_committing_old_history() {
    let (mut app, input, _) = app(PlacementTransformField::CuboidSizeX);
    let before = record(&app).clone();
    app.world_mut().write_message(NumericInputChanged {
        entity: input,
        value: NumericValue::F32(4.0),
    });
    app.update();
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .selected_placement_id = None;
    commit(&mut app, input, 4.0, NumericInputCommitSource::Scrub, true);
    assert_eq!(record(&app), &before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn cuboid_precise_input_selection_change_cancels_original_character_buffer() {
    let (mut app, input, _) = app(PlacementTransformField::CuboidColorR);
    let mut second = record(&app).clone();
    second.id = "second-cube".into();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements
        .push(second);
    app.world_mut().resource_mut::<ActiveNumericInput>().entity = Some(input);
    app.update(); // 正式业务桥先为第一个对象冻结字符焦点。
    let mut messages = app
        .world()
        .resource::<Messages<NumericInputSetValue>>()
        .get_cursor_current();
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .select_indexed_test_placement("second-cube", 1, 3);
    app.update();
    let events = app.world().resource::<Messages<NumericInputSetValue>>();
    assert!(
        messages
            .read(events)
            .any(|event| event.entity == input && event.cancel_edit)
    );
    assert!(
        app.world()
            .resource::<PlacementTransformInputState>()
            .focus
            .is_none()
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn cuboid_collision_toggle_changes_actual_box_and_undo_restores_it() {
    let (mut app, _, root) = app(PlacementTransformField::CuboidSizeX);
    let before = record(&app).clone();
    app.world_mut().spawn((
        feathrise_editor_ui::EditorButtonClick::triggered(),
        super::super::SceneEditorPlacementTransformActionButton(
            super::super::PlacementTransformAction::ToggleCuboidCollision,
        ),
    ));
    app.world_mut()
        .run_system_once(super::super::handle_placement_transform_actions)
        .unwrap();
    assert!(matches!(
        record(&app).asset_target,
        ScenePlacementAssetTarget::Cuboid {
            collision: false,
            ..
        }
    ));
    let children = app.world().get::<Children>(root).unwrap();
    assert!(children.iter().all(|child| {
        app.world()
            .get::<avian3d::prelude::Collider>(child)
            .is_none()
    }));
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
    crate::viewport::placement_move::interaction_tests::batch_undo_resources(&mut app);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .undo_requested = true;
    app.world_mut()
        .run_system_once(crate::authoring::apply_undo_redo)
        .unwrap();
    assert_eq!(record(&app), &before);
    let children = app.world().get::<Children>(root).unwrap();
    assert!(children.iter().any(|child| {
        app.world()
            .get::<avian3d::prelude::Collider>(child)
            .is_some()
    }));
}
