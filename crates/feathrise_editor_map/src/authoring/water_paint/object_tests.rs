//! 静水真实相机/水位平面/R8回归：无地形可绘制，水位独立且输入失权不会复活旧按压。
use super::*;
use crate::viewport::water_plane::{WaterPlaneCursor, sync_water_plane_cursor};
use std::time::{Duration, Instant};

fn fixture(root: bool) -> App {
    let mut app = crate::authoring::vegetation_paint::object_tests::fixture();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(Duration::from_millis(16)))
        .init_resource::<WaterBrushStroke>().init_resource::<WaterMaskMaterializeTask>()
        .init_resource::<WaterPlaneCursor>()
        .init_resource::<crate::resources::MapEditorSettingsState>()
        .add_systems(Update, (sync_water_plane_cursor, apply_water_brush).chain().after(crate::MapEditorSystemSet::Cursor));
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.tool = EditorTool::Water; editor.brush_radius = 0.8; editor.brush_strength = 1.0;
    }
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    draft.terrain_ground = None; draft.terrain_mesh = None; draft.terrain_editing_index = None;
    if root {
        draft.terrain_documents.active = None;
        draft.manifest.terrain = feathrise_map_content::MapTerrainDescriptor::empty();
        draft.manifest.terrains = Some(Arc::from([]));
        draft.manifest.world_min = [-0.5; 2]; draft.manifest.world_max = [0.5; 2];
    }
    let mut water = crate::authoring::water_asset_with_layer_for_tests(&draft.map_id, &draft.folder);
    water.layers[0].resolution = [129; 2]; water.layers[0].water_level = 3.0;
    water.layers[0].canvas_bounds = Some([[-32.0; 2], [32.0; 2]]);
    draft.water_masks.insert(water.layers[0].id.clone(), Arc::new(vec![0; 129 * 129]));
    draft.water = Some(water);
    drop(session);
    if root { app.world_mut().resource_mut::<MapEditorState>().terrain_canvas_target = None; }
    app
}
fn step(app: &mut App) {
    app.update();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    std::thread::sleep(Duration::from_millis(1));
}
fn until(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app) { assert!(Instant::now() < deadline, "真实静水笔刷应在有界等待内前进"); step(app); }
}
fn move_pointer(app: &mut App, x: f32) {
    let world = app.world_mut();
    let mut viewport = world.query_filtered::<&mut RelativeCursorPosition, With<MapEditorViewport>>();
    let mut node = viewport.single_mut(world).unwrap();
    node.cursor_over = true; node.normalized = Some(Vec2::new(x, 0.0));
}
fn mask(app: &App) -> &[u8] {
    let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
    &draft.water_masks[&draft.water.as_ref().unwrap().layers[0].id]
}
fn pixel(point: Vec3) -> usize {
    let x = ((point.x + 32.0) * 2.0).round() as usize;
    let z = ((point.z + 32.0) * 2.0).round() as usize;
    z * 129 + x
}

#[test]
fn water_without_any_terrain_uses_its_own_bounds_and_level_for_one_continuous_stroke() {
    let mut app = fixture(true);
    step(&mut app);
    assert!((app.world().resource::<WaterPlaneCursor>().point.unwrap().y - 3.0).abs() < 0.0001);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    until(&mut app, |app| app.world().resource::<EditorAuthoringSession>().water_revision > 0);
    let first = app.world().resource::<WaterBrushStroke>().last_world.unwrap();
    assert!(mask(&app)[pixel(first)] > 0);
    move_pointer(&mut app, 0.40);
    until(&mut app, |app| app.world().resource::<WaterBrushStroke>().last_world
        .is_some_and(|point| point.x > 2.0));
    let last = app.world().resource::<WaterBrushStroke>().last_world.unwrap();
    assert!(mask(&app)[pixel(last)] > 0, "地图根的±0.5占位范围不能裁掉独立水画布");
    assert!(mask(&app)[pixel(first.lerp(last, 0.5))] > 0, "同次移动按水位平面补齐连续刷段");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<WaterBrushStroke>().active);
    for _ in 0..4 { step(&mut app); }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1,
        "同一次连续按压只产生一个完整静水历史点");
    assert!(app.world().resource::<WaterBrushStroke>().height_before.is_empty());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().terrain_revision, 0);
}

#[test]
fn private_clipmap_water_does_not_require_a_ground_snapshot_or_author_mip_zero() {
    let mut app = fixture(false);
    step(&mut app);
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_ground.is_none());
    assert!(app.world().resource::<WaterPlaneCursor>().point.is_some());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    until(&mut app, |app| app.world().resource::<EditorAuthoringSession>().water_revision > 0);
    assert!((app.world().resource::<WaterBrushStroke>().last_world.unwrap().y - 3.0).abs() < 0.0001);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<WaterBrushStroke>().active);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn water_ui_ownership_loss_finishes_the_stroke_and_held_mouse_cannot_restart_it() {
    let mut app = fixture(true);
    step(&mut app);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    until(&mut app, |app| app.world().resource::<EditorAuthoringSession>().water_revision > 0);
    app.world_mut().resource_mut::<crate::resources::MapEditorSettingsState>().open = true;
    until(&mut app, |app| !app.world().resource::<WaterBrushStroke>().active);
    let revision = app.world().resource::<EditorAuthoringSession>().water_revision;
    app.world_mut().resource_mut::<crate::resources::MapEditorSettingsState>().open = false;
    move_pointer(&mut app, 0.40);
    for _ in 0..4 { step(&mut app); }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().water_revision, revision);
    assert!(!app.world().resource::<WaterBrushStroke>().active);
    assert_eq!(mask(&app)[pixel(app.world().resource::<WaterPlaneCursor>().point.unwrap())], 0);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn water_leaving_and_reentering_viewport_keeps_one_history_without_bridging_the_gap() {
    let mut app = fixture(true);
    move_pointer(&mut app, -0.40);
    step(&mut app);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    until(&mut app, |app| app.world().resource::<EditorAuthoringSession>().water_revision > 0);
    let first = app.world().resource::<WaterBrushStroke>().last_world.unwrap();
    {
        let world = app.world_mut();
        let mut viewport = world.query_filtered::<&mut RelativeCursorPosition, With<MapEditorViewport>>();
        let mut node = viewport.single_mut(world).unwrap();
        node.cursor_over = false;
        node.normalized = None;
    }
    step(&mut app);
    assert!(app.world().resource::<WaterPlaneCursor>().point.is_none());
    assert!(app.world().resource::<WaterBrushStroke>().active,
        "离开视口只暂停采样，同一次按压仍是一条历史");
    move_pointer(&mut app, 0.40);
    until(&mut app, |app| app.world().resource::<WaterBrushStroke>().last_world
        .is_some_and(|point| point.x > 2.0));
    let last = app.world().resource::<WaterBrushStroke>().last_world.unwrap();
    assert!(mask(&app)[pixel(first)] > 0);
    assert!(mask(&app)[pixel(last)] > 0);
    assert_eq!(mask(&app)[pixel(first.lerp(last, 0.5))], 0,
        "鼠标经过侧栏的空窗不能被补成横穿画布的水域");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<WaterBrushStroke>().active);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn water_same_frame_press_release_paints_shared_r8_once_after_materialization() {
    let mut app = fixture(true);
    step(&mut app);
    let point = app.world().resource::<WaterPlaneCursor>().point.unwrap();
    let shared_before = {
        let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
        draft.water_masks[&draft.water.as_ref().unwrap().layers[0].id].clone()
    };
    {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.press(MouseButton::Left);
        mouse.release(MouseButton::Left);
    }
    step(&mut app);
    assert!(app.world().resource::<WaterBrushStroke>().active);
    assert!(app.world().resource::<WaterBrushStroke>().pending_sample.is_some(),
        "同帧单击必须冻结落点，不能因共享R8尚未物化而丢笔");
    until(&mut app, |app| !app.world().resource::<WaterBrushStroke>().active);
    assert!(mask(&app)[pixel(point)] > 0);
    assert_eq!(shared_before[pixel(point)], 0, "后台写侧不能修改共享的起笔前版本");
    for _ in 0..4 { step(&mut app); }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().terrain_revision, 0);
}
