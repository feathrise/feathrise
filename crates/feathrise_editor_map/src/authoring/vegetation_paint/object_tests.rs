//! 多网格植被按压经过真实相机、单飞Collider命中及R8笔划，验证等待起笔和失效取消。
use super::*;
use crate::{
    authoring::{
        SparseTerrainAuthoringState,
        terrain_documents::TerrainDocumentIdentity,
        terrain_ground::{
            TerrainGroundCursor, TerrainGroundSnapshot, cursor_for_tests, cursor_running_for_tests,
            hold_cursor_result_for_tests,
        },
    },
    components::MapEditorCamera,
    resources::{MapEditorNumericScrubMouseGuard, MapEditorViewportState},
    types::EditorView,
    ui::{asset_preview::AssetPreviewState, placement_transform::PlacementTransformInputState},
    viewport::{VegetationGroundCursor, sync_editor_viewport_camera},
};
use bevy::{
    asset::AssetPlugin,
    camera::{ComputedCameraValues, RenderTargetInfo},
};
use feathrise_map_content::*;
use std::time::{Duration, Instant};

#[path = "object_mesh_tests.rs"]
mod object_mesh_tests;

pub(crate) fn fixture() -> App {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("grass-clipmap").unwrap();
    object.world_min = [-32.0; 2];
    object.world_max = [32.0; 2];
    object.terrain.kind = MapTerrainKind::SparseClipmap;
    object.terrain.resolution = [257; 2];
    object.terrain.collision_lods = default_collision_lods_for_resolution([257; 2]);
    let folder = format!("{}/terrains/{}", draft.folder, object.id.as_str());
    object.terrain.glb_path = format!("{folder}/terrain.glb");
    object.terrain.height_path = Some(format!("{folder}/height_tiles.map_height_manifest.json"));
    let source = object.source_manifest(&draft.manifest);
    draft.manifest.terrain = MapTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(object.clone()),
        manifest: source,
        folder: folder.clone(),
    });
    draft.terrain_ground = Some(TerrainGroundSnapshot::test_mesh(
        draft.map_id.clone(),
        object.id.clone(),
        object.transform,
        avian3d::prelude::Collider::trimesh(
            vec![
                Vec3::new(-32.0, 2.0, -32.0),
                Vec3::new(32.0, 2.0, -32.0),
                Vec3::new(32.0, 2.0, 32.0),
                Vec3::new(-32.0, 2.0, 32.0),
            ],
            vec![[0, 2, 1], [0, 3, 2]],
        ),
    ));
    let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/vegetation_presets.vegetation_presets.json"
    )))
    .unwrap();
    let catalog = ResolvedVegetationPresetCatalog::build(&catalog).unwrap();
    let mut output = catalog
        .resolve("hn01_ordinary_grass")
        .unwrap()
        .create_map_layer(&folder);
    output.resolution = [3; 2];
    let mut vegetation = MapVegetationAsset {
        schema_version: MAP_SCHEMA_VERSION,
        map_id: draft.map_id.clone(),
        layers: vec![output],
        authoring: None,
    };
    super::super::vegetation_defaults::ensure_vegetation_authoring(&mut vegetation, &folder);
    let authoring = vegetation.authoring.as_mut().unwrap();
    let mut layer = authoring.layers[0].clone();
    authoring.layers = ["grass-a", "grass-b"]
        .into_iter()
        .map(|id| {
            layer.id = id.into();
            layer.label = id.into();
            layer.density_source_path = format!("{folder}/vegetation/author/{id}.r8");
            layer.clone()
        })
        .collect();
    authoring
        .validate(&vegetation.layers)
        .expect("回归画布应使用有效的正式输出与作者组引用");
    draft.vegetation = Some(vegetation);
    for id in ["grass-a", "grass-b"] {
        draft
            .vegetation_author_densities
            .insert(id.into(), Arc::new(vec![0; 9]));
    }
    let map = draft.map_id.clone();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(MapEditorState {
            open: true,
            selected_map_id: Some(map),
            tool: EditorTool::Vegetation,
            terrain_canvas_target: Some(object.id),
            view: EditorView::Top,
            brush_radius: 2.0,
            brush_strength: 0.5,
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            ..default()
        })
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<VegetationBrushStroke>()
        .init_resource::<TerrainGroundCursor>()
        .init_resource::<VegetationGroundCursor>()
        .init_resource::<SparseTerrainAuthoringState>()
        .init_resource::<AssetPreviewState>()
        .init_resource::<PlacementTransformInputState>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .add_systems(
            Update,
            (
                sync_editor_viewport_camera,
                resolve_cursor.in_set(crate::MapEditorSystemSet::Cursor),
                apply_vegetation_brush.run_if(
                    crate::ui::numeric_scrub_mouse_guard::map_editor_numeric_scrub_mouse_available,
                ),
            )
                .chain(),
        );
    let size = UVec2::splat(128);
    let camera = app
        .world_mut()
        .spawn((
            MapEditorCamera,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: size,
                        scale_factor: 1.0,
                    }),
                    clip_from_view: Mat4::perspective_infinite_reverse_rh(
                        50_f32.to_radians(),
                        1.0,
                        0.1,
                    ),
                    ..default()
                },
                ..default()
            },
            Projection::default(),
            Transform::default(),
            feathrise_scene::DynamicClipmapAuthoringView {
                focus_xz: Vec2::ZERO,
                focus_y: 2.0,
            },
        ))
        .id();
    app.insert_resource(MapEditorViewportState {
        _image: Handle::default(),
        camera,
        center: Vec3::new(0.0, 2.0, 0.0),
        scene_center: Vec3::ZERO,
        scene_half_extents: Vec3::splat(32.0),
        extent: 16.0,
        yaw: 0.0,
        pitch: std::f32::consts::FRAC_PI_2,
        framed_map_id: None,
        render_size: size,
    });
    app.world_mut().spawn((
        MapEditorViewport,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::ZERO),
        },
    ));
    app
}
fn resolve_cursor(world: &mut World) {
    cursor_for_tests(world);
}
fn step(app: &mut App) {
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
}
fn until(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) {
        assert!(Instant::now() < deadline, "真实地形射线应有限完成");
        step(app);
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn begin(app: &mut App) -> std::sync::mpsc::Sender<()> {
    until(app, |app| {
        app.world()
            .resource::<MapEditorState>()
            .brush_cursor_world
            .is_some()
            && !cursor_running_for_tests(app.world())
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(app);
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .brush_cursor_world
            .is_none(),
        "按下换代会作废旧悬停，起笔必须等待新命中"
    );
    assert!(!app.world().resource::<VegetationBrushStroke>().active);
    hold_cursor_result_for_tests(app.world_mut())
}
fn pixels(app: &App, id: &str) -> Vec<u8> {
    app.world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .vegetation_author_densities[id]
        .as_ref()
        .clone()
}

#[test]
fn explicit_clipmap_grass_press_waits_for_real_hit_and_creates_one_stroke() {
    let mut app = fixture();
    let release = begin(&mut app);
    assert_eq!(pixels(&app, "grass-a"), vec![0; 9]);
    release.send(()).unwrap();
    until(&mut app, |app| {
        app.world()
            .resource::<MapEditorState>()
            .brush_cursor_world
            .is_some()
            && !cursor_running_for_tests(app.world())
    });
    assert!(
        pixels(&app, "grass-a")[4] > 0,
        "持续按住时真实命中到达应起笔，不能要求重新点击"
    );
    assert_eq!(pixels(&app, "grass-b"), vec![0; 9]);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    step(&mut app);
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
    for _ in 0..3 {
        step(&mut app);
    }
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn held_object_grass_publishes_continuously_and_retains_last_density_after_f12_close() {
    use crate::authoring::{
        live_density::{LiveBladeDensityState, sync_live_blade_density},
        vegetation_compile::{
            VegetationCompileState, poll_vegetation_compile, start_vegetation_compile,
        },
    };
    let mut app = fixture();
    let map = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .map_id
        .clone();
    let output = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .vegetation
        .as_ref()
        .unwrap()
        .layers[0]
        .id
        .clone();
    app.init_asset::<Image>()
        .init_resource::<VegetationCompileState>()
        .init_resource::<LiveBladeDensityState>()
        .init_resource::<feathrise_scene::DynamicBladeDraft>()
        .insert_resource(ActiveDynamicMap {
            map_id: Some(map.clone()),
        })
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f64(1.0 / 60.0),
        ))
        .add_systems(
            Update,
            (
                poll_vegetation_compile,
                start_vegetation_compile,
                sync_live_blade_density,
            )
                .chain()
                .after(apply_vegetation_brush),
        );
    begin(&mut app).send(()).unwrap();
    until(&mut app, |app| {
        app.world()
            .resource::<feathrise_scene::DynamicBladeDraft>()
            .for_map(&map)
            .is_some_and(|snapshot| snapshot.sources[&output][4] > 0)
    });
    let layout = app
        .world()
        .resource::<feathrise_scene::DynamicBladeDraft>()
        .layout_revision_for_map(&map);
    for _ in 0..3 {
        let before = app
            .world()
            .resource::<feathrise_scene::DynamicBladeDraft>()
            .for_map(&map)
            .unwrap()
            .sources[&output][4];
        until(&mut app, |app| {
            app.world()
                .resource::<feathrise_scene::DynamicBladeDraft>()
                .for_map(&map)
                .is_some_and(|snapshot| snapshot.sources[&output][4] > before)
        });
        assert!(
            app.world().resource::<VegetationBrushStroke>().active,
            "正式草地必须在松笔前连续刷新"
        );
        assert_eq!(
            app.world()
                .resource::<feathrise_scene::DynamicBladeDraft>()
                .layout_revision_for_map(&map),
            layout,
            "Blade纯刷印不能重建草窗布局"
        );
    }
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    step(&mut app);
    until(&mut app, |app| {
        let session = app.world().resource::<EditorAuthoringSession>();
        app.world()
            .resource::<feathrise_scene::DynamicBladeDraft>()
            .for_map(&map)
            .is_some_and(|snapshot| snapshot.source_revision == session.vegetation_revision)
    });
    let snapshot = app
        .world()
        .resource::<feathrise_scene::DynamicBladeDraft>()
        .for_map(&map)
        .unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        snapshot.sources[&output].as_ref(),
        session.draft.as_ref().unwrap().vegetation_densities[&output].as_slice()
    );
    assert!(snapshot.sources[&output][4] > 0);
    assert_eq!(session.undo.len(), 1);
}

#[test]
fn explicit_grass_waiting_press_cancels_after_release_layer_canvas_or_input_gate_change() {
    for case in 0..5 {
        let mut app = fixture();
        let release = begin(&mut app);
        let target = app
            .world()
            .resource::<MapEditorState>()
            .terrain_canvas_target
            .clone();
        match case {
            0 => app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Left),
            1 => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .selected_vegetation_layer = 1
            }
            2 => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .terrain_canvas_target = Some(TerrainId::parse("other").unwrap())
            }
            3 => app
                .world_mut()
                .resource_mut::<MapEditorNumericScrubMouseGuard>()
                .advance(true, true, false),
            _ => app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::ShiftLeft),
        }
        step(&mut app);
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.selected_vegetation_layer = 0;
            editor.terrain_canvas_target = target;
        }
        if case == 3 {
            *app.world_mut()
                .resource_mut::<MapEditorNumericScrubMouseGuard>() = default();
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::ShiftLeft);
        release.send(()).unwrap();
        until(&mut app, |app| !cursor_running_for_tests(app.world()));
        for _ in 0..4 {
            step(&mut app);
        }
        assert_eq!(pixels(&app, "grass-a"), vec![0; 9], "case {case}");
        assert_eq!(pixels(&app, "grass-b"), vec![0; 9], "case {case}");
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
    }
}

fn dense_grass(app: &mut App) {
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    for layer in &mut draft.vegetation.as_mut().unwrap().authoring.as_mut().unwrap().layers {
        layer.resolution = [129; 2];
    }
    for density in draft.vegetation_author_densities.values_mut() { *density = Arc::new(vec![0; 129 * 129]); }
    drop(session);
    app.world_mut().resource_mut::<MapEditorState>().brush_radius = 0.6;
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)));
}

fn move_pointer(app: &mut App, x: f32) {
    let world = app.world_mut();
    let mut viewport = world.query_filtered::<&mut RelativeCursorPosition, With<MapEditorViewport>>();
    viewport.single_mut(world).unwrap().normalized = Some(Vec2::new(x, 0.0));
}

#[test]
fn continuous_grass_drag_consumes_delayed_real_points_and_fills_the_segment_with_one_undo() {
    let mut app = fixture();
    dense_grass(&mut app);
    begin(&mut app).send(()).unwrap();
    until(&mut app, |app| app.world().resource::<VegetationBrushStroke>().active);
    let start = app.world().resource::<VegetationBrushStroke>().last_world.unwrap();
    move_pointer(&mut app, 0.25);
    step(&mut app);
    let release = hold_cursor_result_for_tests(app.world_mut());
    move_pointer(&mut app, 0.45);
    for _ in 0..5 { step(&mut app); }
    let stroke = app.world().resource::<VegetationBrushStroke>();
    assert_eq!(stroke.last_world, Some(start), "正常后台等待不能把涂抹轨迹清成圆印");
    assert!(stroke.waiting_exposure > 0.0, "在途真实射线不应丢掉按住时间");
    release.send(()).unwrap();
    until(&mut app, |app| app.world().resource::<VegetationBrushStroke>().last_world
        .is_some_and(|point| point.x > 1.0));
    assert!(app.world().resource::<MapEditorState>().brush_cursor_world.is_none(),
        "较早完成的画笔点不冒充最新悬停或选择位置");
    let density = pixels(&app, "grass-a");
    for x in 65..=70 {
        assert!(density[64 * 129 + x] > 0, "真实端点之间的第{x}列不得漏刷");
    }
    assert!(pixels(&app, "grass-b").iter().all(|pixel| *pixel == 0));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<VegetationBrushStroke>().active);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn continuous_grass_drag_cancels_late_points_and_cannot_resume_the_old_hold_after_focus_or_camera_change() {
    for case in 0..3 {
        let mut app = fixture();
        dense_grass(&mut app);
        begin(&mut app).send(()).unwrap();
        until(&mut app, |app| app.world().resource::<VegetationBrushStroke>().active);
        let before = pixels(&app, "grass-a");
        move_pointer(&mut app, 0.25);
        step(&mut app);
        let release = hold_cursor_result_for_tests(app.world_mut());
        match case {
            0 => app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left),
            1 => app.world_mut().resource_mut::<MapEditorViewportState>().extent = 20.0,
            _ => app.world_mut().resource_mut::<MapEditorNumericScrubMouseGuard>().advance(true, true, false),
        }
        step(&mut app);
        if case == 2 { *app.world_mut().resource_mut::<MapEditorNumericScrubMouseGuard>() = default(); }
        release.send(()).unwrap();
        until(&mut app, |app| !cursor_running_for_tests(app.world()));
        for _ in 0..5 { step(&mut app); }
        assert_eq!(pixels(&app, "grass-a"), before, "case {case}: 迟到命中不得复活原笔");
        assert!(!app.world().resource::<VegetationBrushStroke>().active);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    }
}

#[test]
fn continuous_grass_long_thin_stroke_advances_bounded_prefixes_without_spot_gaps() {
    let mut app = fixture();
    let mut editor = app.world_mut().remove_resource::<MapEditorState>().unwrap();
    let mut session = app.world_mut().remove_resource::<EditorAuthoringSession>().unwrap();
    let draft = session.draft.as_mut().unwrap();
    let layer = &mut draft.vegetation.as_mut().unwrap().authoring.as_mut().unwrap().layers[0];
    layer.resolution = [2049, 3];
    let layer = layer.clone();
    draft.vegetation_author_densities.insert(layer.id.clone(), Arc::new(vec![0; 2049 * 3]));
    let mut stroke = VegetationBrushStroke { active: true, layer_id: Some(layer.id.clone()), ..default() };
    let mut sample = Some(path::Sample { from: Vec3::new(-12.0, 2.0, 0.0), to: Vec3::new(12.0, 2.0, 0.0),
        radius: 0.05, erase: false, amount: 1.0, exposure: 0.0 });
    let mut frames = 0;
    while let Some(current) = sample {
        sample = path::apply(current, &layer, &mut session, &mut editor, &mut stroke);
        frames += 1;
        if frames == 1 { assert!(stroke.last_world.unwrap().x < -11.0, "一帧最多推进32枚相连圆刷"); }
        assert!(frames < 100, "有界前缀必须最终排空");
    }
    assert!(frames > 1);
    let draft = session.draft.as_ref().unwrap();
    for x in 641..1408 {
        assert!(draft.vegetation_author_densities[&layer.id][2049 + x] > 0, "5厘米刷子的第{x}列不能出现圆印断口");
    }
    finish_vegetation_stroke(&mut editor, &mut session, &mut stroke);
    assert_eq!(session.undo.len(), 1, "分帧前缀仍合并为一次笔划");
}

fn enable_real_save_shortcut(app: &mut App) {
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().map_id.clone();
    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
        .insert_resource(ActiveDynamicMap { map_id: Some(map.clone()) })
        .init_resource::<feathrise_editor_ui::ActiveNumericInput>()
        .init_resource::<crate::resources::MapDeletionTask>()
        .init_resource::<crate::authoring::WaterNetworkEditState>()
        .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>()
        .init_resource::<crate::ui::surface_preset_editor::SurfacePresetEditorState>()
        .init_resource::<crate::ui::water_preset_editor::WaterPresetEditorState>()
        .init_resource::<crate::ui::building_presets::BuildingPresetEditorState>()
        .init_resource::<crate::game_authoring::map_interaction::MapInteractionState>()
        .init_resource::<feathrise_editor_core::EditorSettings>();
    app.world_mut().resource_mut::<MapEditorViewportState>().framed_map_id = Some(map);
}

fn undo_grass_once(app: &mut App, before: &[u8], painted: &[u8]) {
    use crate::authoring::*;
    use bevy::ecs::system::RunSystemOnce;
    // 使用生产历史入口验证整个长段；门户资源只是该公共系统的固定参数，不加载模型。
    app.insert_resource(EditorPreviewAssets {
        portal_base_mesh: default(), portal_arrow_head_mesh: default(), portal_arrow_shaft_mesh: default(),
        portal_edge_beam_mesh: default(), portal_edge_ring_mesh: default(), portal_base_material: default(),
        portal_arrow_material: default(), portal_edge_beam_material: default(), portal_edge_ring_material: default(),
        portal_label_font: default(),
    })
        .init_resource::<courtyards::CourtyardAuthoringState>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<WaterMaskMaterializeTask>()
        .init_resource::<crate::authoring::surface_layer_edit::SurfaceLayerEditTask>()
        .init_resource::<SurfaceBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
        .init_resource::<WaterFileHandoff>();
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.save_requested = false; editor.close_requested = false; editor.undo_requested = true;
    }
    app.world_mut().run_system_once(apply_undo_redo).unwrap();
    assert_eq!(pixels(app, "grass-a"), before, "一次生产撤销应恢复起笔前的完整作者R8，包括排空后的尾部");
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().redo.len(), 1);
    app.world_mut().resource_mut::<MapEditorState>().redo_requested = true;
    app.world_mut().run_system_once(apply_undo_redo).unwrap();
    assert_eq!(pixels(app, "grass-a"), painted, "一次重做应恢复整条已受理轨迹");
}

#[test]
fn continuous_grass_accepted_long_drag_drains_on_save_close_and_ctrl_s_with_one_undo() {
    use bevy::ecs::system::RunSystemOnce;
    for ending in 0..3 {
        let mut app = fixture();
        dense_grass(&mut app);
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            for layer in &mut draft.vegetation.as_mut().unwrap().authoring.as_mut().unwrap().layers {
                layer.resolution = [2049, 3];
            }
            for pixels in draft.vegetation_author_densities.values_mut() { *pixels = Arc::new(vec![0; 2049 * 3]); }
        }
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.brush_radius = 0.05; editor.brush_strength = 1.0;
        }
        let before = pixels(&app, "grass-a");
        begin(&mut app).send(()).unwrap();
        until(&mut app, |app| app.world().resource::<VegetationBrushStroke>().active);
        move_pointer(&mut app, 0.45);
        until(&mut app, |app| {
            let stroke = app.world().resource::<VegetationBrushStroke>();
            app.world().resource::<TerrainGroundCursor>().paint_hit().zip(stroke.last_world)
                .is_some_and(|(hit, accepted)| accepted.x > 0.0 && hit.point.x - accepted.x > 0.5)
        });
        let target = app.world().resource::<TerrainGroundCursor>().paint_hit().unwrap().point;
        let prefix = app.world().resource::<VegetationBrushStroke>().last_world.unwrap();
        assert!(target.x - prefix.x > 0.5, "必须已经受理超出32刷印预算的真实长段，不能只测一个点");
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        match ending {
            0 => app.world_mut().resource_mut::<MapEditorState>().save_requested = true,
            1 => app.world_mut().resource_mut::<MapEditorState>().close_requested = true,
            _ => {
                enable_real_save_shortcut(&mut app);
                let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keyboard.press(KeyCode::ControlLeft); keyboard.press(KeyCode::KeyS);
                drop(keyboard);
                app.world_mut().run_system_once(crate::ui::actions::handle_editor_shortcuts).unwrap();
                assert!(app.world().resource::<MapEditorState>().save_requested,
                    "Ctrl+S应通过正式快捷键分派登记保存请求");
            }
        }
        step(&mut app); // 保存/关闭使真实光标取消权限，冻结刷段仍需正常排空。
        assert!(app.world().resource::<TerrainGroundCursor>().input_authority().is_none());
        assert!(app.world().resource::<VegetationBrushStroke>().active,
            "case {ending}: 第一次取消光标后仍有已受理尾段，不能提前结束");
        until(&mut app, |app| !app.world().resource::<VegetationBrushStroke>().active);
        let painted = pixels(&app, "grass-a");
        let target_column = ((target.x + 32.0) / 64.0 * 2048.0).round() as usize;
        assert!(target_column > 1024);
        for x in 1024..=target_column {
            assert!(painted[2049 + x] > 0, "case {ending}: 正常收尾不能丢弃已受理路径的第{x}列");
        }
        assert!(pixels(&app, "grass-b").iter().all(|pixel| *pixel == 0));
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
        for _ in 0..4 { step(&mut app); }
        assert_eq!(pixels(&app, "grass-a"), painted, "收尾后仍按住或Ctrl仍按住不能追加新点");
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
        undo_grass_once(&mut app, &before, &painted);
    }
}

fn paint_delayed_samples(samples: impl IntoIterator<Item = path::Sample>) -> Vec<u8> {
    let mut app = fixture();
    let mut editor = app.world_mut().remove_resource::<MapEditorState>().unwrap();
    let mut session = app.world_mut().remove_resource::<EditorAuthoringSession>().unwrap();
    let draft = session.draft.as_mut().unwrap();
    let layer = &mut draft.vegetation.as_mut().unwrap().authoring.as_mut().unwrap().layers[0];
    layer.resolution = [257; 2];
    let layer = layer.clone();
    draft.vegetation_author_densities.insert(layer.id.clone(), Arc::new(vec![0; 257 * 257]));
    let mut stroke = VegetationBrushStroke { active: true, layer_id: Some(layer.id.clone()), ..default() };
    for sample in samples {
        let mut pending = Some(sample);
        while let Some(current) = pending {
            pending = path::apply(current, &layer, &mut session, &mut editor, &mut stroke);
        }
    }
    session.draft.as_ref().unwrap().vegetation_author_densities[&layer.id].as_ref().clone()
}

#[test]
fn continuous_grass_delayed_bends_remain_real_segments_and_endpoint_hold_only_exposes_endpoint() {
    let sample = |from, to, exposure| path::Sample { from, to, radius: 0.25, erase: false, amount: 0.0, exposure };
    let (a, b, c) = (Vec3::new(-4.0, 2.0, -4.0), Vec3::new(4.0, 2.0, -4.0), Vec3::new(4.0, 2.0, 4.0));
    let mut path = path::VegetationPaintPath::default();
    path.queue(sample(a, b, 0.03));
    path.queue(sample(b, c, 0.03));
    assert_eq!(path.tail.len(), 2, "真实B拐点不能被合并成A→C斜线");
    assert_eq!(path.tail[0].to, b);
    assert_eq!(path.tail[1].from, b);
    let without_hold = paint_delayed_samples(path.tail.iter().copied());
    for _ in 0..10 { path.queue(sample(c, c, 0.1)); }
    assert_eq!(path.tail.len(), 3, "同点停留可以合并，但必须与前面的运动段分开");
    assert_eq!(path.tail[0].exposure, 0.03);
    assert_eq!(path.tail[1].exposure, 0.03);
    assert!((path.tail[2].exposure - 1.0).abs() < 1e-6);
    let held = paint_delayed_samples(path.tail.iter().copied());
    let at = |x: i32, z: i32| ((z + 32) * 4) as usize * 257 + ((x + 32) * 4) as usize;
    assert!(held[at(0, -4)] > 0 && held[at(4, 0)] > 0 && held[at(4, -4)] > 0,
        "两条真实折线及拐点都应完整绘制");
    assert_eq!(held[at(0, 0)], 0, "不能在未经过的A→C对角线上刷草");
    assert_eq!(held[at(0, -4)], without_hold[at(0, -4)], "C处停留曝光不能涂满早已受理的A→B段");
    assert_eq!(held[at(4, 0)], without_hold[at(4, 0)], "C处停留曝光不能均摊到B→C运动段");
    assert!(held[at(4, 4)] > without_hold[at(4, 4)], "只有终点附近应继续累积停留曝光");
}
