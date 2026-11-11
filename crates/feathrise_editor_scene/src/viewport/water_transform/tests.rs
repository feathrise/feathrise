//! 通过真实移动输入、公共鼠标门与作者事务验证静水Y轴拖动。

use super::*;
use crate::{
    authoring::{EditorCommand, WaterNetworkEditState, water_asset_with_layer_for_tests},
    ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
    viewport::{modes::SelectionFilter, placement_move::interaction_tests::{camera, step}},
};
use bevy::{ecs::system::RunSystemOnce, input::mouse::{MouseMotion, MouseWheel}};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorSnapSettings;
use feathrise_editor_core::EditorSnapMode;
use std::sync::Arc;

#[path = "lifecycle.rs"]
mod lifecycle;

const START_LEVEL: f32 = 4.234_567;

fn app(snap: EditorSnapSettings) -> (App, Entity, Vec3) {
    let (mut app, _, node) = crate::viewport::placement_move::interaction_tests::app();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut water = water_asset_with_layer_for_tests(&draft.scene_id, "scenes/test");
        water.layers[0].resolution = [2, 2];
        water.layers[0].water_level = START_LEVEL;
        draft.water = Some(water);
        draft.water_asset_path = "scenes/test/water.scene_water.json".into();
        draft.water_masks.insert("lake".into(), Arc::new(vec![0, 255, 100, 0]));
    }
    let origin = Vec3::new(5.0, START_LEVEL, 5.0);
    let mut selected = SelectedWaterViewportState::default();
    selected.set_still_water_center_for_tests(app.world().resource::<EditorAuthoringSession>(), "lake", origin);
    app.init_resource::<WaterMoveState>()
        .init_resource::<TerrainObjectsEditorState>()
        .init_resource::<WaterNetworkEditState>()
        .insert_resource(selected)
        .insert_resource(EditorSettings { editor_snapping: snap, ..default() })
        .add_message::<MouseMotion>()
        .add_message::<MouseWheel>();
    confirm_selection(&mut app);
    // 真实顺序包含旧资产输入，验证静水正式选择不会串改残留placement，鼠标门也确实隔离相机。
    app.edit_schedule(Update, |schedule| {
        *schedule = Schedule::new(Update);
        schedule.add_systems((input, continue_selection, super::super::placement_move::handle_placement_move,
            update_scene_editor_numeric_scrub_mouse_guard, super::super::control_editor_viewport).chain());
    });
    (app, node, origin)
}

fn confirm_selection(app: &mut App) {
    app.world_mut().resource_scope(|world, mut candidates: Mut<LayerCandidateState>| {
        candidates.confirm_tree_author(LayerTarget::Water(WaterAuthoringFeatureKind::StillWater, "lake".into()),
            Some(0), world.resource::<EditorAuthoringSession>(), 0, SelectionFilter::default());
    });
}

fn pointer(app: &App, origin: Vec3, delta: f32) -> Vec2 {
    let camera = camera(app);
    camera.project(origin + Vec3::Y * (camera.handle_length(origin).unwrap() * 0.65 + delta)).unwrap()
}

fn begin(app: &mut App, node: Entity, origin: Vec3) {
    let cursor = pointer(app, origin, 0.0);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(app, node, cursor);
    assert!(app.world().resource::<WaterMoveState>().drag.is_some());
    assert!(app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
}

fn level(app: &App) -> f32 {
    app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().water.as_ref().unwrap().layers[0].water_level
}

fn release(app: &mut App, node: Entity, cursor: Vec2) {
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    step(app, node, cursor);
    assert!(app.world().resource::<WaterMoveState>().drag.is_none());
}

#[test]
fn water_move_hover_highlights_only_y_without_opening_a_scrub_or_owning_mouse() {
    let (mut app, node, origin) = app(EditorSnapSettings::default());
    app.init_resource::<AuthorToolFrame>();
    let active_axis = |app: &mut App| {
        *app.world_mut().resource_mut::<AuthorToolFrame>() = default();
        app.world_mut().run_system_once(draw).unwrap();
        let frame = app.world().resource::<AuthorToolFrame>();
        let [AuthorTool::Transform(TransformPreview::Move { axes, active_axes, color_active, .. })] = frame.preview.items.as_slice() else {
            panic!("正式静水选择必须只投影一个可写Y轴手柄");
        };
        assert_eq!(*axes, 0b010);
        assert_eq!(*color_active, Color::srgb(0.92, 0.72, 0.40).to_linear().to_f32_array());
        *active_axes
    };
    let cursor = pointer(&app, origin, 0.0);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<WaterMoveState>().hover);
    assert_eq!(active_axis(&mut app), 0b010);
    assert!(app.world().resource::<WaterMoveState>().drag.is_none());
    assert!(!app.world().resource::<PlacementMoveState>().owns_mouse());
    assert!(!app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, 7);
    assert!(session.undo.is_empty());
    assert_eq!(level(&app), START_LEVEL);
    assert!(!app.world().resource::<SceneEditorState>().dirty);

    // 原UI门覆盖时立即恢复轴色；旧鼠标位置不能继续给出可拖动暗示。
    app.world_mut().resource_mut::<LayerCandidateState>().pointer_over_ui = true;
    step(&mut app, node, cursor);
    assert!(!app.world().resource::<WaterMoveState>().hover);
    assert_eq!(active_axis(&mut app), 0);
    app.world_mut().resource_mut::<LayerCandidateState>().pointer_over_ui = false;
    begin(&mut app, node, origin);
    assert_eq!(active_axis(&mut app), 0b010, "按住后沿原水位事务保持手柄激活");
}

#[test]
fn water_move_previews_each_frame_and_commits_one_undoable_height_without_changing_r8() {
    let (mut app, node, origin) = app(EditorSnapSettings::default());
    let mask = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().water_masks["lake"].clone();
    let ticket = app.world().resource::<LayerCandidateState>().selection_revision;
    begin(&mut app, node, origin);
    for delta in [0.8, 1.4, 2.2] {
        let cursor = pointer(&app, origin, delta);
        step(&mut app, node, cursor);
        assert!((level(&app) - START_LEVEL - delta).abs() < 0.002);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.undo.is_empty());
        let candidates = app.world().resource::<LayerCandidateState>();
        assert_eq!(candidates.selection_revision, ticket);
        assert!(candidates.selected_matches_session(session));
        assert_eq!(session.terrain_revision, 2);
        assert!(Arc::ptr_eq(&mask, &session.draft.as_ref().unwrap().water_masks["lake"]));
    }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 10);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().water_revision, 3);
    let after = level(&app);
    let cursor = pointer(&app, origin, 2.2);
    release(&mut app, node, cursor);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(matches!(&session.undo[0], EditorCommand::WaterLevelChanged { layer_id, before, after: end }
        if layer_id == "lake" && *before == START_LEVEL && *end == after));
    assert_eq!(session.draft.as_ref().unwrap().placements.placements[0],
        crate::viewport::placement_move::interaction_tests::record());
    assert!(app.world().resource::<SceneEditorState>().dirty);
    crate::viewport::placement_move::interaction_tests::batch_undo_resources(&mut app);
    for undo in [true, false] {
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.undo_requested = undo;
            editor.redo_requested = !undo;
        }
        app.world_mut().run_system_once(crate::authoring::apply_undo_redo).unwrap();
        assert_eq!(level(&app), if undo { START_LEVEL } else { after });
    }
}

#[test]
fn water_move_freezes_snap_step_and_uses_world_or_relative_height_phase() {
    for (enabled, control, mode, expected) in [
        (false, false, EditorSnapMode::WorldGrid, START_LEVEL + 1.37),
        (false, true, EditorSnapMode::WorldGrid, 6.0),
        (true, false, EditorSnapMode::WorldGrid, 6.0),
        (true, true, EditorSnapMode::WorldGrid, START_LEVEL + 1.37),
        (true, false, EditorSnapMode::Relative, START_LEVEL + 1.0),
    ] {
        let (mut app, node, origin) = app(EditorSnapSettings { enabled, mode, ..default() });
        if control { app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft); }
        begin(&mut app, node, origin);
        app.world_mut().resource_mut::<EditorSettings>().editor_snapping.translation_meters = 9.0;
        let cursor = pointer(&app, origin, 1.37);
        step(&mut app, node, cursor);
        assert!((level(&app) - expected).abs() < 0.002, "常开={enabled} Ctrl={control} 模式={mode:?}");
    }
}

#[test]
fn water_move_click_or_return_to_origin_preserves_raw_level_dirty_and_redo() {
    for moved in [false, true] {
        let (mut app, node, origin) = app(EditorSnapSettings { enabled: true, ..default() });
        app.world_mut().resource_mut::<EditorAuthoringSession>().redo.push(EditorCommand::WaterLevelChanged {
            layer_id: "lake".into(), before: 0.0, after: START_LEVEL,
        });
        begin(&mut app, node, origin);
        if moved {
            let away = pointer(&app, origin, 1.37);
            step(&mut app, node, away);
            assert_ne!(level(&app), START_LEVEL);
        }
        let start = pointer(&app, origin, 0.0) + Vec2::Y / camera(&app).size;
        release(&mut app, node, start);
        assert_eq!(level(&app), START_LEVEL);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(session.undo.is_empty());
        assert_eq!(session.redo.len(), 1);
        assert!(!app.world().resource::<SceneEditorState>().dirty);
        if !moved { assert_eq!(session.revision, 7); assert_eq!(session.water_revision, 0); }
    }
}

#[test]
fn water_move_only_accepts_y_handle_in_move_mode_and_formal_current_selection() {
    for reject in 0..7 {
        let (mut app, node, origin) = app(EditorSnapSettings::default());
        let mut cursor = pointer(&app, origin, 0.0);
        match reject {
            0 => {
                let camera = camera(&app);
                cursor = camera.project(origin + Vec3::X * camera.handle_length(origin).unwrap() * 0.65).unwrap();
            }
            1 => app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Rotate,
            2 => app.world_mut().resource_mut::<LayerCandidateState>().selected = None,
            3 => app.world_mut().resource_mut::<LayerCandidateState>().selected = Some(LayerTarget::Placement("move-test".into())),
            4 => app.world_mut().resource_mut::<LayerCandidateState>().hover = Some(LayerTarget::Water(WaterAuthoringFeatureKind::StillWater, "only-hover".into())),
            5 => app.world_mut().resource_mut::<SceneEditorState>().selected_water_layer = 3,
            _ => app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1,
        }
        if reject == 4 {
            app.world_mut().resource_mut::<LayerCandidateState>().selected = None;
        }
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, cursor);
        assert!(app.world().resource::<WaterMoveState>().drag.is_none(), "拒绝场景{reject}");
        assert_eq!(level(&app), START_LEVEL);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}

#[test]
fn water_move_is_registered_by_real_editor_plugin_and_runs_in_its_production_order() {
    use bevy::ecs::{schedule::{ApplyDeferred, ScheduleCleanupPolicy}, system::IntoSystem};
    let (mut app, node, origin) = app(EditorSnapSettings::default());
    app.edit_schedule(Update, |schedule| *schedule = Schedule::new(Update));
    app.add_plugins(crate::SceneEditorPlugin); // 注册正式入口，遗漏安装时不能靠测试手动补水位系统。
    let keep = [
        IntoSystem::into_system(input).system_type(),
        IntoSystem::into_system(continue_selection).system_type(),
        IntoSystem::into_system(super::super::placement_move::handle_placement_move).system_type(),
        IntoSystem::into_system(update_scene_editor_numeric_scrub_mouse_guard).system_type(),
        IntoSystem::into_system(super::super::control_editor_viewport).system_type(),
    ];
    let mut schedule = app.world_mut().resource_mut::<bevy::ecs::schedule::Schedules>().remove(Update).unwrap();
    schedule.initialize(app.world_mut()).expect("正式编辑器的静水输入顺序必须无环");
    let mut seen = std::collections::HashSet::new();
    let mut remove = Vec::new();
    for (_, system) in schedule.systems().unwrap() {
        let kind = system.system_type();
        if keep.contains(&kind) {
            seen.insert(kind);
        } else if kind != std::any::TypeId::of::<ApplyDeferred>() {
            remove.extend(system.default_system_sets());
        }
    }
    assert_eq!(seen.len(), keep.len(), "正式插件必须安装水位手柄与选择续接，不能只声明模块");
    for set in remove.into_iter().collect::<std::collections::HashSet<_>>() {
        schedule.remove_systems_in_set(set, app.world_mut(), ScheduleCleanupPolicy::RemoveSystemsOnly).unwrap();
    }
    schedule.initialize(app.world_mut()).unwrap();
    let start = pointer(&app, origin, 0.0);
    app.world_mut().get_mut::<RelativeCursorPosition>(node).unwrap().normalized = Some(start);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    schedule.run(app.world_mut()); // 只运行生产图中保留下来的系统，不触发Startup或场景文件任务。
    assert!(app.world().resource::<WaterMoveState>().drag.is_some());
    assert!(app.world().resource::<SceneEditorNumericScrubMouseGuard>().blocked());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    for (delta, expected) in [(1.4, 6.0), (2.4, 7.0)] {
        let moved = pointer(&app, origin, delta);
        app.world_mut().get_mut::<RelativeCursorPosition>(node).unwrap().normalized = Some(moved);
        schedule.run(app.world_mut());
        assert!((level(&app) - expected).abs() < 0.002);
        assert!(app.world().resource::<ButtonInput<KeyCode>>().pressed(KeyCode::ControlLeft));
        assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::ControlLeft));
    } // 正式placement输入清除快捷键边沿，持续Ctrl电平仍跨帧控制水位吸附。
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    schedule.run(app.world_mut());
    assert!(app.world().resource::<WaterMoveState>().drag.is_none());
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}
