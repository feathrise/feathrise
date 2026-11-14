//! 体素地形实尺寸的真数值输入与历史回归；高度地形不暴露固定高度域。
use super::*;
use crate::authoring;
use bevy::ecs::system::RunSystemOnce;
use std::sync::Arc;

fn history(app: &mut App, undoing: bool) {
    app.init_resource::<authoring::courtyards::CourtyardAuthoringState>()
        .init_resource::<authoring::GlobalEnvironmentAuthoringState>()
        .init_resource::<authoring::WaterMaskMaterializeTask>()
        .init_resource::<authoring::SurfaceLayerEditTask>()
        .init_resource::<authoring::SurfaceBrushStroke>()
        .init_resource::<authoring::WaterBrushStroke>()
        .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
        .init_resource::<authoring::WaterFileHandoff>();
    if !app.world().contains_resource::<authoring::EditorPreviewAssets>() {
        app.insert_resource(authoring::EditorPreviewAssets {
            portal_base_mesh: default(), portal_arrow_head_mesh: default(), portal_arrow_shaft_mesh: default(),
            portal_edge_beam_mesh: default(), portal_edge_ring_mesh: default(), portal_base_material: default(),
            portal_arrow_material: default(), portal_edge_beam_material: default(), portal_edge_ring_material: default(),
            portal_label_font: default(),
        });
    }
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.undo_requested = undoing;
        editor.redo_requested = !undoing;
    }
    app.world_mut().run_system_once(authoring::apply_undo_redo).unwrap();
}

fn numeric_text(app: &mut App, window: Entity, value: Field, text: &str) {
    let root = field(app, value);
    click(app, window, root, MouseButton::Middle);
    assert_eq!(app.world().resource::<ActiveNumericInput>().entity, Some(root));
    key(app, window, KeyCode::Digit4, Key::Character(text.into()), Some(text));
    key(app, window, KeyCode::Enter, Key::Enter, None);
}

#[test]
fn voxel_height_real_numeric_is_world_meters_and_history_keeps_all_author_sources() {
    let (mut app, window) = page();
    configure_native(&mut app, SceneTerrainKind::Voxel, 4);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0].transform.scale[1] = 1.75;
        draft.vegetation_author_densities.insert("retained-grass".into(), Arc::new(vec![173; 16]));
    }
    let before = current_object(&app);
    app.update();
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().set_section_open(MeshPropertySection::Type, false);
    app.update();
    let tab = action(&mut app, Action::TypeProperties);
    click(&mut app, window, tab, MouseButton::Left);
    assert_eq!(numeric_value(&mut app, Field::VoxelHeight), 28.0);
    numeric_text(&mut app, window, Field::VoxelHeight, "42");
    complete(&mut app);
    let after = current_object(&app);
    assert_eq!(after.transform.scale[1], 2.625);
    assert_eq!(after.voxel, before.voxel);
    assert_eq!(after.terrain, before.terrain);
    assert_eq!(after.surface_path, before.surface_path);
    assert_eq!(after.vegetation_path, before.vegetation_path);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    history(&mut app, true);
    app.update();
    assert_eq!(current_object(&app), before);
    history(&mut app, false);
    app.update();
    assert_eq!(current_object(&app), after);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().vegetation_author_densities["retained-grass"].as_slice(), &[173; 16]);
}

#[test]
fn height_domain_clipmap_properties_have_no_fixed_cap_control() {
    let (mut app, window) = native_page(SceneTerrainKind::SparseClipmap, 4);
    app.world_mut().resource_mut::<TerrainObjectsEditorState>().set_section_open(MeshPropertySection::Type, false);
    app.update();
    let tab = action(&mut app, Action::TypeProperties);
    click(&mut app, window, tab, MouseButton::Left);
    for field in [Field::VoxelHeight] {
        let entity = self::field(&mut app, field);
        assert!(app.world().entity(entity).contains::<NumericInputDisabled>());
        assert!(numeric::field_value(field, app.world().resource::<EditorAuthoringSession>(), app.world().resource::<TerrainObjectsEditorState>()).is_none());
    }
    let world = app.world_mut();
    let summary = world.query_filtered::<&Text, With<SourceSummary>>().single(world).unwrap();
    assert!(summary.0.contains("不受创建高度域限制"));
    assert!(!summary.0.contains("作者高度域："));
}
