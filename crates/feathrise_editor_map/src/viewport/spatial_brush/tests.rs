//! 空间笔刷的真实输入、冻结请求与取消门验证。
use super::{
    components::*, request::build_request, resources::SpatialBrushState, systems, types::*, ui,
};
use crate::{
    authoring::{
        EditorAuthoringSession, TerrainExternalChangeState, test_heightfield_draft,
        volume_sculpt::VolumeAuthoringState,
    },
    components::MapEditorViewport,
    resources::*,
    types::{EditorTool, EditorView},
    ui::{
        building_presets::BuildingPlacementControls, courtyard::CourtyardUiState,
        gltf_browser::EditorGltfBrowserState,
        numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
        placement_transform::PlacementTransformInputState, placement_water::PlacementInstanceIndex,
        text_input::EditorTextInputGuard, vegetation_preset_editor::VegetationPresetEditorState,
    },
    viewport::{
        LayerCandidateState,
        camera_math::orbit_angles_from_view,
        modes::{ViewportMode, ViewportModeState},
        placement_move::MoveCamera,
    },
};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::*,
    ui::{InteractionDisabled, RelativeCursorPosition},
};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputChanged, NumericInputCommitted,
    NumericValue,
};
use feathrise_scene_core::input::{EditorInputState, InputGateState};
use feathrise_map_content::MapTerrainKind;
use std::sync::Arc;

fn publish_cursor(app: &mut App) -> bool {
    app.world_mut()
        .run_system_once(crate::ui::systems::sync_map_editor_custom_cursor)
        .unwrap();
    app.world()
        .resource::<EditorInputState>()
        .map_editor_custom_cursor_visible
}

#[test]
fn spatial_fixed_brush_preserves_pointer_with_a_valid_terrain_hit_and_restores_mouse_mode() {
    let (mut app, _) = app();
    app.init_resource::<EditorInputState>().init_resource::<InputGateState>();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .brush_cursor_world = Some(Vec3::ZERO);
    // 固定轮廓不能替代鼠标：即使地形拾取继续成功，也必须归还系统指针。
    assert!(!publish_cursor(&mut app));
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .brush_cursor_world = Some(Vec3::X);
    assert!(!publish_cursor(&mut app));

    app.world_mut().resource_mut::<SpatialBrushState>().enabled = false;
    assert!(publish_cursor(&mut app), "随鼠标模式仍使用真实笔刷光标");
    app.world_mut().resource_mut::<SpatialBrushState>().enabled = true;
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.tool = EditorTool::Surface;
        editor.view = EditorView::Top;
    }
    assert!(publish_cursor(&mut app), "空间会话不能影响普通地表刷");
}

#[test]
fn spatial_cursor_returns_on_the_placement_click_before_ground_resolution() {
    let (mut app, _) = app();
    app.init_resource::<EditorInputState>().init_resource::<InputGateState>();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .brush_cursor_world = Some(Vec3::ZERO);
    {
        let mut state = app.world_mut().resource_mut::<SpatialBrushState>();
        state.center = None;
        state.armed = true;
    }
    assert!(publish_cursor(&mut app), "放置前的跟随轮廓仍能定位鼠标");
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    tick(&mut app); // 真实Input阶段先冻结点击，定位结果在Cursor之后才采用。
    assert!(app.world().resource::<SpatialBrushState>().click_pending);
    assert!(!publish_cursor(&mut app), "定位等待不能留下一帧无光标");
}

#[test]
fn spatial_cursor_remains_visible_during_underground_handle_drag_and_cancel() {
    let (mut app, node) = app();
    app.init_resource::<EditorInputState>().init_resource::<InputGateState>();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .brush_cursor_world = Some(Vec3::ZERO);
    let grabbed = begin_drag(&mut app, node);
    assert!(!publish_cursor(&mut app));
    pointer(&mut app, node, grabbed + Vec3::X * 3.);
    tick(&mut app);
    assert!(!publish_cursor(&mut app));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    tick(&mut app);
    assert!(app.world().resource::<SpatialBrushState>().drag.is_none());
    assert!(!publish_cursor(&mut app), "取消移动后仍需定位可见鼠标");
}

#[test]
fn spatial_inactive_or_idle_controller_preserves_escape_for_other_editor_and_game_handlers() {
    for closed in [false, true] {
        let (mut app, _) = app();
        if closed {
            app.world_mut().resource_mut::<MapEditorState>().open = false;
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Escape);
        app.update();
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .just_pressed(KeyCode::Escape),
            "没有活动定位或拖动时不能吞掉全局Esc"
        );
    }
}

pub(super) fn app() -> (App, Entity) {
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.terrain.kind = MapTerrainKind::SparseClipmap;
    draft.manifest.world_min = [-256.; 2];
    draft.manifest.world_max = [256.; 2];
    let map = draft.map_id.clone();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(MapEditorState {
            open: true,
            selected_map_id: Some(map.clone()),
            tool: EditorTool::VolumeDig,
            view: EditorView::Front,
            brush_radius: 1.,
            volume_brush_half_height: 1.,
            brush_strength: 1.,
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 7,
            terrain_revision: 2,
            placement_structure_revision: 3,
            ..default()
        })
        .init_resource::<SpatialBrushState>()
        .init_resource::<VolumeAuthoringState>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<EditorGltfBrowserState>()
        .init_resource::<LayerCandidateState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<ViewportModeState>()
        .init_resource::<PlacementTransformInputState>()
        .init_resource::<CourtyardUiState>()
        .init_resource::<MapEditorSettingsState>()
        .init_resource::<MapBrowserState>()
        .init_resource::<MapBrowserDragState>()
        .init_resource::<TerrainExternalChangeState>()
        .init_resource::<MapDeletionTask>()
        .init_resource::<MapEditorLayoutState>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<ActiveNumericInputScrub>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<BuildingPlacementControls>()
        .init_resource::<PlacementInstanceIndex>()
        .add_message::<NumericInputChanged>()
        .add_message::<NumericInputCommitted>()
        .add_systems(
            Update,
            (
                systems::handles,
                update_map_editor_numeric_scrub_mouse_guard,
                ui::input,
            )
                .chain(),
        );
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
    let (yaw, pitch) = orbit_angles_from_view(EditorView::Front);
    app.insert_resource(MapEditorViewportState {
        _image: default(),
        camera: Entity::PLACEHOLDER,
        center: Vec3::new(0., -2., 0.),
        scene_center: Vec3::ZERO,
        scene_half_extents: Vec3::splat(256.),
        extent: 40.,
        yaw,
        pitch,
        framed_map_id: Some(map),
        render_size: UVec2::new(800, 600),
    });
    let node = app
        .world_mut()
        .spawn((
            MapEditorViewport,
            ComputedNode::default(),
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::ZERO),
            },
        ))
        .id();
    app.update();
    let mut state = app.world_mut().resource_mut::<SpatialBrushState>();
    state.enabled = true;
    state.center = Some(Vec3::new(0., -2., 0.));
    (app, node)
}

fn tick(app: &mut App) {
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
}
fn camera(app: &App) -> MoveCamera {
    MoveCamera::new(
        app.world().resource::<MapEditorState>(),
        app.world().resource::<MapEditorViewportState>(),
        1.,
    )
    .unwrap()
}
fn pointer(app: &mut App, node: Entity, world: Vec3) {
    let p = camera(app).project(world).unwrap();
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(node)
        .unwrap()
        .normalized = Some(p);
}
fn begin_drag(app: &mut App, node: Entity) -> Vec3 {
    let origin = app.world().resource::<SpatialBrushState>().center.unwrap();
    let length = camera(app).handle_length(origin).unwrap();
    pointer(app, node, origin + Vec3::X * length * 0.7);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    tick(app);
    assert!(app.world().resource::<SpatialBrushState>().drag.is_some());
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    origin + Vec3::X * length * 0.7
}

#[test]
fn spatial_handle_moves_underground_without_modifying_map_or_history() {
    let (mut app, node) = app();
    let grabbed = begin_drag(&mut app, node);
    pointer(&mut app, node, grabbed + Vec3::X * 3.);
    tick(&mut app);
    let center = app.world().resource::<SpatialBrushState>().center.unwrap();
    assert!((center - Vec3::new(3., -2., 0.)).length() < 0.001);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 7);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert!(!app.world().resource::<VolumeAuthoringState>().busy());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    tick(&mut app);
    assert!(app.world().resource::<SpatialBrushState>().owns_mouse());
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    assert_eq!(
        app.world().resource::<SpatialBrushState>().center,
        Some(center)
    );
    tick(&mut app);
    tick(&mut app);
    assert!(!app.world().resource::<SpatialBrushState>().owns_mouse());
    assert!(
        !app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

#[test]
fn spatial_escape_and_camera_changes_restore_origin() {
    for camera_changed in [false, true] {
        let (mut app, node) = app();
        let origin = app.world().resource::<SpatialBrushState>().center;
        let grabbed = begin_drag(&mut app, node);
        pointer(&mut app, node, grabbed + Vec3::X * 3.);
        tick(&mut app);
        if camera_changed {
            app.world_mut()
                .resource_mut::<MapEditorViewportState>()
                .extent += 1.;
        } else {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
        }
        tick(&mut app);
        let state = app.world().resource::<SpatialBrushState>();
        assert!(state.drag.is_none());
        assert_eq!(state.center, origin);
        assert!(state.owns_mouse());
        assert!(
            app.world()
                .resource::<EditorAuthoringSession>()
                .undo
                .is_empty()
        );
    }
}

#[test]
fn spatial_coordinate_updates_do_not_start_a_sculpt_and_execute_freezes_entire_line() {
    let (mut app, _) = app();
    let field = app.world_mut().spawn(Coordinate(1)).id();
    app.world_mut().write_message(NumericInputChanged {
        entity: field,
        value: NumericValue::F32(-6.),
    });
    tick(&mut app);
    assert_eq!(
        app.world()
            .resource::<SpatialBrushState>()
            .center
            .unwrap()
            .y,
        -6.
    );
    assert!(!app.world().resource::<VolumeAuthoringState>().busy());
    let start_button = app
        .world_mut()
        .spawn((ActionButton(Action::Start), Interaction::None))
        .id();
    let end_button = app
        .world_mut()
        .spawn((ActionButton(Action::End), Interaction::None))
        .id();
    let execute = app
        .world_mut()
        .spawn((ActionButton(Action::Execute), Interaction::None))
        .id();
    app.world_mut().resource_mut::<SpatialBrushState>().mode = ApplyMode::Line;
    app.world_mut()
        .entity_mut(start_button)
        .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    tick(&mut app);
    app.world_mut().resource_mut::<SpatialBrushState>().center = Some(Vec3::new(10., -8., 0.));
    app.world_mut()
        .entity_mut(end_button)
        .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    tick(&mut app);
    app.world_mut()
        .entity_mut(execute)
        .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    tick(&mut app);
    {
        let mut state = app.world_mut().resource_mut::<SpatialBrushState>();
        let brush = state.pending.as_ref().unwrap().brush;
        assert_eq!(brush.from, [0., -6., 0.]);
        assert_eq!(brush.to, [10., -8., 0.]);
        state.center = Some(Vec3::new(20., -4., 0.));
    }
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .brush_radius = 2.;
    app.world_mut()
        .resource_scope(|world, mut state: Mut<SpatialBrushState>| {
            let request = state
                .take_request(
                    world.resource::<MapEditorState>(),
                    world.resource::<EditorAuthoringSession>(),
                )
                .unwrap()
                .unwrap();
            assert_eq!(request.brush.to, [10., -8., 0.]);
            assert_eq!(request.brush.radius, 1.);
            assert!(
                state
                    .take_request(
                        world.resource::<MapEditorState>(),
                        world.resource::<EditorAuthoringSession>()
                    )
                    .unwrap()
                    .is_none()
            );
        });
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn spatial_next_start_reuses_fixed_brush_after_execution_without_placing_again() {
    for mode in [ApplyMode::Point, ApplyMode::Line] {
        let (mut app, _) = app();
        app.add_message::<feathrise_editor_ui::NumericInputSetValue>();
        let center = app.world().resource::<SpatialBrushState>().center.unwrap();
        {
            let mut state = app.world_mut().resource_mut::<SpatialBrushState>();
            state.mode = mode;
            state.start = Some(center - Vec3::X);
            state.end = Some(center);
        }
        let mut button = |action| {
            app.world_mut()
                .spawn((
                    ActionButton(action),
                    Interaction::None,
                    BackgroundColor::default(),
                    BorderColor::default(),
                ))
                .id()
        };
        let start = button(Action::Start);
        let end = button(Action::End);
        let execute = button(Action::Execute);
        app.world_mut().run_system_once(ui::sync).unwrap();
        app.world_mut()
            .entity_mut(execute)
            .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        tick(&mut app);
        app.world_mut()
            .resource_scope(|world, mut state: Mut<SpatialBrushState>| {
                assert!(
                    state
                        .take_request(
                            world.resource::<MapEditorState>(),
                            world.resource::<EditorAuthoringSession>(),
                        )
                        .unwrap()
                        .is_some()
                );
            });
        // 联合采用推进作者/结构修订；工具会话里的固定刷心必须仍能作为下一段起点。
        app.world_mut()
            .resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
                session
                    .mark_placement_structure_changed(&mut world.resource_mut::<MapEditorState>());
            });
        tick(&mut app);
        app.world_mut().run_system_once(ui::sync).unwrap();
        assert_eq!(
            app.world().resource::<SpatialBrushState>().center,
            Some(center)
        );
        assert!(!app.world().resource::<SpatialBrushState>().armed);
        assert!(
            !app.world().entity(start).contains::<InteractionDisabled>(),
            "单次或直线执行后，现有定位笔刷都能直接设为下一段起点"
        );

        app.world_mut()
            .entity_mut(start)
            .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        tick(&mut app);
        let state = app.world().resource::<SpatialBrushState>();
        assert_eq!(state.mode, ApplyMode::Line);
        assert_eq!(state.start, Some(center));
        assert!(state.end.is_none(), "新起点不能沿用上一段的终点");
        assert_eq!(state.center, Some(center));
        assert!(!state.armed);
        assert!(state.pending.is_none());
        app.world_mut().run_system_once(ui::sync).unwrap();
        assert!(
            app.world()
                .entity(execute)
                .contains::<InteractionDisabled>(),
            "新终点明确设置前不能执行旧路径"
        );

        let field = app.world_mut().spawn(Coordinate(0)).id();
        app.world_mut().write_message(NumericInputChanged {
            entity: field,
            value: NumericValue::F32(center.x + 4.),
        });
        tick(&mut app);
        app.world_mut().entity_mut(end).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        tick(&mut app);
        app.world_mut().run_system_once(ui::sync).unwrap();
        assert!(
            !app.world()
                .entity(execute)
                .contains::<InteractionDisabled>()
        );
        app.world_mut()
            .entity_mut(execute)
            .insert(Interaction::None);
        tick(&mut app);
        app.world_mut()
            .entity_mut(execute)
            .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        tick(&mut app);
        let request = app
            .world()
            .resource::<SpatialBrushState>()
            .pending
            .as_ref()
            .unwrap();
        assert_eq!(request.brush.from, center.to_array());
        assert_eq!(request.brush.to, (center + Vec3::X * 4.).to_array());
        assert_eq!(
            request.identity.revision,
            app.world().resource::<EditorAuthoringSession>().revision
        );
        assert!(request.matches(
            app.world().resource::<MapEditorState>(),
            app.world().resource::<EditorAuthoringSession>()
        ));
    }
}

#[test]
fn spatial_line_accepts_long_paths_and_rejects_missing_endpoint_or_invalid_coordinates() {
    let (app, _) = app();
    let editor = app.world().resource::<MapEditorState>();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(
        build_request(
            editor,
            session,
            None,
            ApplyMode::Line,
            Some(Vec3::ZERO),
            None
        )
        .is_err()
    );
    for end in [Vec3::X * 80., Vec3::new(200., -120., 200.)] {
        let request = build_request(
            editor,
            session,
            None,
            ApplyMode::Line,
            Some(Vec3::ZERO),
            Some(end),
        )
        .expect("内部刷印和局部块预算不能限制完整直线路径");
        assert_eq!(request.brush.from, Vec3::ZERO.to_array());
        assert_eq!(request.brush.to, end.to_array());
    }
    assert!(
        build_request(
            editor,
            session,
            Some(Vec3::splat(f32::NAN)),
            ApplyMode::Point,
            None,
            None
        )
        .is_err()
    );
    assert!(
        build_request(
            editor,
            session,
            Some(Vec3::X * 256.),
            ApplyMode::Point,
            None,
            None
        )
        .is_err()
    );
}

#[test]
fn spatial_frozen_requests_reject_author_height_structure_tool_and_folder_changes() {
    for kind in 0..5 {
        let (mut app, _) = app();
        let request = build_request(
            app.world().resource::<MapEditorState>(),
            app.world().resource::<EditorAuthoringSession>(),
            Some(Vec3::ZERO),
            ApplyMode::Point,
            None,
            None,
        )
        .unwrap();
        app.world_mut().resource_mut::<SpatialBrushState>().pending = Some(request);
        match kind {
            0 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .revision += 1
            }
            1 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .terrain_revision += 1
            }
            2 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .placement_structure_revision += 1
            }
            3 => app.world_mut().resource_mut::<MapEditorState>().tool = EditorTool::VolumeFill,
            _ => app
                .world_mut()
                .resource_mut::<EditorAuthoringSession>()
                .draft
                .as_mut()
                .unwrap()
                .folder
                .push_str("-other"),
        }
        app.world_mut()
            .resource_scope(|world, mut state: Mut<SpatialBrushState>| {
                assert!(
                    state
                        .take_request(
                            world.resource::<MapEditorState>(),
                            world.resource::<EditorAuthoringSession>()
                        )
                        .is_err()
                );
                assert!(state.pending.is_none());
            });
    }
}

#[test]
fn spatial_map_close_clears_coordinates_and_does_not_replay_pressed_execute() {
    let (mut app, _) = app();
    let execute = app
        .world_mut()
        .spawn((ActionButton(Action::Execute), feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed))
        .id();
    app.world_mut().resource_mut::<MapEditorState>().open = false;
    tick(&mut app);
    let state = app.world().resource::<SpatialBrushState>();
    assert!(!state.enabled);
    assert!(state.center.is_none());
    assert!(state.pending.is_none());
    app.world_mut().resource_mut::<MapEditorState>().open = true;
    tick(&mut app);
    assert!(app.world().get::<Interaction>(execute).is_some());
    assert!(
        app.world()
            .resource::<SpatialBrushState>()
            .pending
            .is_none()
    );
}

#[test]
fn spatial_modal_press_is_consumed_and_cannot_execute_after_modal_closes() {
    let (mut app, _) = app();
    app.world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .open = true;
    app.world_mut()
        .spawn((ActionButton(Action::Execute), feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed));
    tick(&mut app);
    assert!(
        app.world()
            .resource::<SpatialBrushState>()
            .pending
            .is_none()
    );
    app.world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .open = false;
    tick(&mut app);
    tick(&mut app);
    assert!(
        app.world()
            .resource::<SpatialBrushState>()
            .pending
            .is_none()
    );
}
