//! 无GPU运行正式网格发布系统，验证退出门、固定世界锚点与真实手势吸附隔离。
use super::{
    EditorGridGizmos, EditorGridOutlineGizmos, MeshSnapCursor, components::GridView, draw_grid,
};
use crate::{
    authoring::{EditorAuthoringSession, placement_batch::PlacementBatchState},
    components::SceneEditorCamera,
    resources::{SceneEditorState, SceneEditorViewportState},
    types::EditorView,
    ui::{
        building_presets::BuildingPlacementControls, gltf_browser::EditorGltfBrowserState,
        placement_transform::PlacementTransformInputState,
        vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState},
    },
    viewport::{multi_select::PlacementSelectionSet, placement_move::PlacementMoveState},
};
use bevy::{asset::AssetPlugin, ecs::system::RunSystemOnce, gizmos::GizmoPlugin, prelude::*};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorSnapSettings;
use feathrise_editor_core::EditorSnapMode;

fn grid_resources(app: &mut App) {
    app.add_plugins(GizmoPlugin)
        .init_gizmo_group::<EditorGridGizmos>()
        .init_gizmo_group::<EditorGridOutlineGizmos>()
        .init_resource::<EditorSettings>()
        .init_resource::<EditorGltfBrowserState>()
        .init_resource::<PlacementSelectionSet>()
        .init_resource::<PlacementMoveState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<MeshSnapCursor>()
        .init_resource::<PlacementBatchState>()
        .init_resource::<PlacementTransformInputState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<BuildingPlacementControls>();
}

fn fixture() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()));
    grid_resources(&mut app);
    let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let map = draft.scene_id.clone();
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(map.clone()),
        ..default()
    })
    .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() });
    let author = app.world_mut().spawn((SceneEditorCamera, Camera3d::default())).id();
    let game = app.world_mut().spawn(Camera3d::default()).id();
    app.insert_resource(SceneEditorViewportState {
        _image: Handle::default(),
        camera: author,
        center: Vec3::new(2.8, 5.0, 8.9),
        scene_center: Vec3::ZERO,
        scene_half_extents: Vec3::splat(5.0),
        extent: 20.0,
        yaw: 0.0,
        pitch: 0.0,
        framed_scene_id: Some(map),
        render_size: UVec2::new(640, 480),
    });
    (app, author, game)
}

fn publish(app: &mut App) {
    app.world_mut().run_system_once(draw_grid).unwrap();
}

fn grid(app: &App, camera: Entity) -> GridView {
    *app.world().get::<GridView>(camera).expect("有效作者视图应安装网格参数")
}

#[test]
fn actual_grid_withdraws_at_every_exit_gate_and_restores_without_touching_game_camera() {
    for gate in 0..10 {
        let (mut app, author, game) = fixture();
        let map = app.world().resource::<SceneEditorState>().selected_scene_id.clone();
        publish(&mut app);
        let original = grid(&app, author);
        assert!(app.world().get::<GridView>(game).is_none());
        let mut draft = None;
        let mut viewport = None;
        match gate {
            0 => app.world_mut().resource_mut::<EditorSettings>().editor_snapping.show_grid = false,
            1 => app.world_mut().resource_mut::<SceneEditorState>().open = false,
            2 => app.world_mut().resource_mut::<SceneEditorState>().scene_transition_in_progress = true,
            3 => app.world_mut().resource_mut::<SceneEditorViewportState>().framed_scene_id = None,
            4 => app.world_mut().resource_mut::<SceneEditorState>().selected_scene_id = None,
            5 => app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Document,
            6 => draft = app.world_mut().resource_mut::<EditorAuthoringSession>().draft.take(),
            7 => app.world_mut().resource_mut::<SceneEditorViewportState>().center.x = f32::NAN,
            8 => app.world_mut().resource_mut::<SceneEditorViewportState>().extent = f32::INFINITY,
            _ => viewport = app.world_mut().remove_resource::<SceneEditorViewportState>(),
        }
        publish(&mut app);
        assert!(app.world().get::<GridView>(author).is_none(), "退出门{gate}不能保留上一帧网格");
        assert!(app.world().get::<GridView>(game).is_none());

        if let Some(viewport) = viewport { app.insert_resource(viewport); }
        if let Some(draft) = draft {
            app.world_mut().resource_mut::<EditorAuthoringSession>().draft = Some(draft);
        }
        app.world_mut().resource_mut::<EditorSettings>().editor_snapping.show_grid = true;
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.open = true;
            editor.scene_transition_in_progress = false;
            editor.selected_scene_id = map.clone();
        }
        app.world_mut().resource_mut::<VegetationPresetEditorState>().active_tab = PresetWorkspaceTab::Scene;
        {
            let mut viewport = app.world_mut().resource_mut::<SceneEditorViewportState>();
            viewport.framed_scene_id = map;
            viewport.center = Vec3::new(2.8, 5.0, 8.9);
            viewport.extent = 20.0;
        }
        publish(&mut app);
        assert!(grid(&app, author) == original, "退出门{gate}恢复后应重新发布同一平面与相位");
        assert!(app.world().get::<GridView>(game).is_none());
    }
}

#[test]
fn actual_grid_does_not_mark_unchanged_parameters_or_in_plane_panning_as_changed() {
    let (mut app, author, game) = fixture();
    publish(&mut app);
    let original = grid(&app, author);
    let changed = app.world().entity(author).get_ref::<GridView>().unwrap().last_changed();
    app.world_mut().clear_trackers();
    publish(&mut app);
    assert_eq!(app.world().entity(author).get_ref::<GridView>().unwrap().last_changed(), changed,
        "稳定输入不应每帧重插组件并重复提取uniform");
    app.world_mut().resource_mut::<SceneEditorViewportState>().center += Vec3::new(1000.0, 50.0, -1000.0);
    app.world_mut().resource_mut::<SceneEditorState>().cursor_world = Some(Vec3::new(2.3, -81.0, 3.7));
    app.world_mut().resource_mut::<MeshSnapCursor>().preview = Some(Vec3::new(-1.2, 49.0, 4.6));
    publish(&mut app);
    assert!(grid(&app, author) == original, "相机、观察高度及真实光标只改变各自状态，不搬动参考平面或原点");
    assert_eq!(app.world().entity(author).get_ref::<GridView>().unwrap().last_changed(), changed);
    assert!(app.world().get::<GridView>(game).is_none());
}

#[test]
fn actual_grid_uniform_uses_each_work_plane_and_preserves_base_step_during_zoom() {
    let (mut app, author, _) = fixture();
    app.world_mut().resource_mut::<EditorSettings>().editor_snapping.translation_meters = 0.5;
    app.world_mut().resource_mut::<SceneEditorViewportState>().extent = 500.0;
    for (view, normal, axis_u, axis_v, distance) in [
        (EditorView::Top, Vec3::Y, Vec3::X, Vec3::Z, 0.0),
        (EditorView::Perspective, Vec3::Y, Vec3::X, Vec3::Z, 0.0),
        (EditorView::Front, Vec3::Z, Vec3::X, Vec3::Y, 0.0),
        (EditorView::Side, Vec3::X, Vec3::Z, Vec3::Y, 0.0),
    ] {
        app.world_mut().resource_mut::<SceneEditorState>().view = view;
        publish(&mut app);
        let actual = grid(&app, author);
        assert_eq!(actual.origin_stride, Vec4::new(0.0, 0.0, 0.0, 0.5));
        assert_eq!(actual.plane.truncate(), normal);
        assert!((actual.plane.w - distance).abs() < 0.00001);
        assert_eq!(actual.axis_u, axis_u.extend(0.0));
        assert_eq!(actual.axis_v, axis_v.extend(0.0));
        for extent in [20.0, 49.0, 5000.0] {
            app.world_mut().resource_mut::<SceneEditorViewportState>().extent = extent;
            publish(&mut app);
            assert!(grid(&app, author) == actual,
                "连续缩放只改变GPU像素足迹，不能更换基础步长或在CPU整档跳格");
        }
    }
}

#[test]
fn actual_grid_stays_at_world_origin_while_real_drag_keeps_its_own_frozen_snap() {
    use crate::viewport::placement_move::interaction_tests::{app, camera, step};
    for building in [false, true] {
        let (mut app, root, node) = app();
        let origin = Vec3::new(5.3, 4.0, 5.0);
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
            record.translation = origin.to_array();
            record.building_preset_id = building.then(|| "grid-test-building".into());
        }
        app.world_mut().get_mut::<Transform>(root).unwrap().translation = origin;
        app.world_mut().resource_mut::<BuildingPlacementControls>().grid_step_meters = 2.0;
        app.insert_resource(EditorSettings {
            editor_snapping: EditorSnapSettings { enabled: !building, mode: EditorSnapMode::Relative, ..default() },
            ..default()
        });
        let camera = camera(&app);
        let grabbed = origin + Vec3::X * (camera.handle_length(origin).unwrap() * 0.65);
        if building { app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft); }
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, camera.project(grabbed).unwrap());
        step(&mut app, node, camera.project(grabbed + Vec3::X * 1.6).unwrap());
        let (_, _, frozen) = app.world().resource::<PlacementMoveState>().grid_reference(false).unwrap();

        // 先执行真实手柄输入再安装无GPU绘制资源，避免夹具增加与输入无关的渲染系统。
        grid_resources(&mut app);
        let author = app.world_mut().spawn((SceneEditorCamera, Camera3d::default())).id();
        app.world_mut().resource_mut::<SceneEditorViewportState>().camera = author;
        publish(&mut app);
        let original = grid(&app, author);
        assert_eq!(original.origin_stride, Vec4::new(0.0, 0.0, 0.0, 1.0));
        assert_eq!(original.plane.w, 0.0);
        assert_eq!(frozen.step(), if building { 2.0 } else { 1.0 });
        {
            let mut settings = app.world_mut().resource_mut::<EditorSettings>();
            settings.editor_snapping.translation_meters = 4.0;
            settings.editor_snapping.mode = if building { EditorSnapMode::Relative } else { EditorSnapMode::WorldGrid };
            settings.editor_snapping.enabled = building;
        }
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ControlLeft);
        publish(&mut app);
        assert_eq!(grid(&app, author).origin_stride, Vec4::new(0.0, 0.0, 0.0, 4.0),
            "显示间距可采用新配置，实际手势仍使用冻结吸附参数");
        assert_eq!(app.world().resource::<PlacementMoveState>().grid_reference(false).unwrap().2, frozen);
        for (view, normal) in [
            (EditorView::Top, Vec3::Y),
            (EditorView::Front, Vec3::Z),
            (EditorView::Side, Vec3::X),
        ] {
            app.world_mut().resource_mut::<SceneEditorState>().view = view;
            publish(&mut app);
            let actual = grid(&app, author);
            assert_eq!(actual.origin_stride, Vec4::new(0.0, 0.0, 0.0, 4.0));
            assert_eq!(actual.plane.truncate(), normal);
            assert_eq!(actual.plane.w, 0.0);
        }
    }
}

#[test]
fn actual_vertical_asset_drag_does_not_lift_the_horizontal_reference_grid_or_write_history() {
    use crate::viewport::placement_move::interaction_tests::{app, camera, step};
    let (mut app, root, node) = app();
    let origin = Vec3::new(5.3, 4.25, 5.7);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().placements.placements[0].translation = origin.to_array();
    }
    app.world_mut().get_mut::<Transform>(root).unwrap().translation = origin;
    app.insert_resource(EditorSettings {
        editor_snapping: EditorSnapSettings {
            enabled: true, mode: EditorSnapMode::Relative, translation_meters: 0.5, ..default()
        },
        ..default()
    });
    let camera = camera(&app);
    let grabbed = origin + Vec3::Y * (camera.handle_length(origin).unwrap() * 0.65);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut app, node, camera.project(grabbed).unwrap());
    step(&mut app, node, camera.project(grabbed + Vec3::Y * 1.3).unwrap());
    let actual = app.world().get::<Transform>(root).unwrap().translation;
    assert!((actual.y - (origin.y + 1.5)).abs() < 0.002,
        "真实Y手柄仍应按原相对起点执行半米吸附");
    let (_, focus, frozen) = app.world().resource::<PlacementMoveState>().grid_reference(false).unwrap();
    assert!(actual.distance(focus) < 0.002);
    assert_eq!(Vec3::from_array(frozen.origin()), origin);

    // 先跑实际手柄，再发布各观察平面的正式组件；发布网格不能修改候选、作者文档或撤销历史。
    grid_resources(&mut app);
    let author = app.world_mut().spawn((SceneEditorCamera, Camera3d::default())).id();
    let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements[0].clone();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    for view in [EditorView::Top, EditorView::Perspective, EditorView::Front, EditorView::Side] {
        app.world_mut().resource_mut::<SceneEditorState>().view = view;
        app.world_mut().resource_mut::<SceneEditorViewportState>().center = actual;
        app.world_mut().resource_mut::<MeshSnapCursor>().preview = Some(actual + Vec3::Y * 10.0);
        publish(&mut app);
        let grid = grid(&app, author);
        assert_eq!(grid.origin_stride, Vec4::new(0.0, 0.0, 0.0, 0.5));
        assert_eq!(grid.plane.w, 0.0);
        if matches!(view, EditorView::Top | EditorView::Perspective) {
            assert_eq!(grid.plane.truncate(), Vec3::Y);
        }
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements[0], before);
        assert_eq!(session.revision, revision);
        assert!(session.undo.is_empty());
        assert!(!app.world().resource::<SceneEditorState>().dirty);
        assert_eq!(app.world().get::<Transform>(root).unwrap().translation, actual);
    }
}
