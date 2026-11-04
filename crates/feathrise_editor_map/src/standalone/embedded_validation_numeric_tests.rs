//! 验收驱动必须走原 PointerInputPlugin，并且不能自行写入数值或冒充公共捕获。

use super::*;
use bevy::{
    input::InputPlugin,
    math::Affine2,
    picking::{PickingPlugin, input::PointerInputPlugin},
    window::{ExitCondition, WindowPlugin},
};
use feathrise_editor_ui::NumericValue;

fn fixture(scale: f32) -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        },
        InputPlugin,
        PickingPlugin,
        PointerInputPlugin,
    ));
    app.init_resource::<ActiveNumericInputScrub>();
    install(&mut app);
    let mut window = Window {
        resolution: (800, 600).into(),
        ..default()
    };
    window.resolution.set_scale_factor_override(Some(scale));
    let window = app.world_mut().spawn((window, PrimaryWindow)).id();
    let root = app
        .world_mut()
        .spawn((
            Node::default(),
            InheritedVisibility::VISIBLE,
            ComputedNode {
                size: Vec2::new(120., 26.),
                ..default()
            },
            UiGlobalTransform::from(Affine2::from_translation(Vec2::new(200., 140.))),
            NumericInputValue(NumericValue::F32(7.)),
            NumericInputScrubConfig::linear(0.1, 0.001, -100., 100., 1.),
        ))
        .id();
    app.update();
    (app, window, root)
}

#[test]
fn raw_numeric_driver_emits_dpi_correct_ordered_pointer_and_escape_without_writing_value() {
    for scale in [1., 2.] {
        let (mut app, window, root) = fixture(scale);
        change(app.world_mut(), root, 8.).unwrap();
        app.update(); // 悬停先行，业务和按钮均未按下。
        assert_eq!(
            app.world().get::<Window>(window).unwrap().cursor_position(),
            Some(Vec2::new(200., 140.) / scale)
        );
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        app.update(); // 原 InputPlugin 与 PointerInputPlugin 同时看到真正的按下消息。
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        assert_eq!(probe(app.world()).unwrap()["pointer_press"], 1);
        app.update();
        assert_eq!(
            app.world().get::<Window>(window).unwrap().cursor_position(),
            Some(Vec2::new(210., 140.) / scale)
        );
        assert_eq!(
            app.world().get::<NumericInputValue>(root).unwrap().0,
            NumericValue::F32(7.),
            "没有生产控件系统就不能出现目标值，验收助手只允许发原始消息"
        );
        assert_eq!(probe(app.world()).unwrap()["captured"], false);
        finish(app.world_mut(), root, true).unwrap();
        app.update();
        assert!(
            app.world()
                .resource::<ButtonInput<KeyCode>>()
                .pressed(KeyCode::Escape)
        );
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        app.update();
        assert!(idle(app.world()));
        let proof = probe(app.world()).unwrap();
        assert_eq!(proof["pointer_release"], 1);
        assert_eq!(proof["escape_press"], 1);
        assert_eq!(proof["escape_release"], 1);
        assert_eq!(proof["events"].as_array().unwrap().len(), 5);
        assert!(
            proof["events"]
                .as_array()
                .unwrap()
                .windows(2)
                .all(|events| events[0]["frame"].as_u64() < events[1]["frame"].as_u64())
        );
    }
}

#[test]
fn raw_numeric_driver_rejects_hidden_field_and_overlapping_action_batch() {
    let (mut app, _, root) = fixture(1.);
    app.world_mut().get_mut::<Node>(root).unwrap().display = Display::None;
    assert!(change(app.world_mut(), root, 8.).is_err());
    app.world_mut().get_mut::<Node>(root).unwrap().display = Display::Flex;
    change(app.world_mut(), root, 8.).unwrap();
    assert!(change(app.world_mut(), root, 9.).is_err());
    assert!(finish(app.world_mut(), root, false).is_err());
    assert_eq!(app.world().resource::<Driver>().actions.len(), 3);
    assert_eq!(
        app.world().get::<NumericInputValue>(root).unwrap().0,
        NumericValue::F32(7.)
    );
}

#[test]
fn raw_numeric_driver_properties_tab_reveals_original_hidden_page_before_drag() {
    use crate::{
        resources::{MapEditorNumericScrubMouseGuard, MapEditorState},
        ui::sidebars::*,
    };
    let (mut app, _, root) = fixture(1.);
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.left = EditorLeftSidebarTab::Vegetation;
    sidebar.right = EditorRightSidebarTab::Tools;
    app.insert_resource(MapEditorState {
        open: true,
        ..default()
    })
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .insert_resource(sidebar)
    .add_systems(
        Update,
        (
            handle_editor_sidebar_input,
            sync_editor_sidebar_presentation,
        )
            .chain(),
    );
    let page = app
        .world_mut()
        .spawn((
            Node::default(),
            InheritedVisibility::VISIBLE, // 该无渲染夹具没有VisibilityPlugin，页面继承可见性须与真实已布局UI一致。
            MapEditorSidebarPage(EditorSidebarTab::Right(EditorRightSidebarTab::Scene)),
        ))
        .id();
    app.world_mut().entity_mut(page).add_child(root);
    let tab = app
        .world_mut()
        .spawn((
            Node::default(),
            InheritedVisibility::VISIBLE,
            MapEditorSidebarTabButton(EditorSidebarTab::Right(EditorRightSidebarTab::Scene)),
            ComputedNode {
                size: Vec2::new(80., 24.),
                ..default()
            },
            UiGlobalTransform::from(Affine2::from_translation(Vec2::new(40., 20.))),
        ))
        .id();
    app.update();
    assert!(
        !field_ready(app.world(), root),
        "工具页可见不等于属性数字框可见"
    );
    assert!(change(app.world_mut(), root, 8.).is_err());
    assert_eq!(
        super::super::press_numeric_properties(app.world_mut()).unwrap(),
        tab
    );
    assert_eq!(
        app.world().resource::<MapEditorSidebarState>().right,
        EditorRightSidebarTab::Tools,
        "助手不能直接写切页成功，必须等待原按钮业务"
    );
    app.update();
    assert_eq!(
        app.world().resource::<MapEditorSidebarState>().right,
        EditorRightSidebarTab::Scene
    );
    assert!(
        field_ready(app.world(), root),
        "{}",
        field_diagnostic(app.world(), root)
    );
    change(app.world_mut(), root, 8.).unwrap();
    assert_eq!(
        app.world().get::<NumericInputValue>(root).unwrap().0,
        NumericValue::F32(7.)
    );
}

#[test]
fn raw_numeric_driver_allows_logic_parent_but_rejects_actual_ancestor_clip() {
    let (mut app, _, root) = fixture(1.);
    let logical = app.world_mut().spawn_empty().id();
    app.world_mut().entity_mut(logical).add_child(root);
    assert!(
        field_ready(app.world(), root),
        "无 Node 的逻辑祖先不是隐藏 UI"
    );
    let clipping = app
        .world_mut()
        .spawn((
            Node {
                overflow: Overflow::clip(),
                ..default()
            },
            ComputedNode {
                size: Vec2::splat(10.),
                ..default()
            },
            UiGlobalTransform::default(),
        ))
        .id();
    app.world_mut().entity_mut(clipping).add_child(logical);
    let diagnostic = field_diagnostic(app.world(), root);
    assert_eq!(diagnostic["ready"], false);
    assert!(diagnostic["reason"].as_str().unwrap().contains("裁剪区域"));
    assert!(change(app.world_mut(), root, 8.).is_err());
}

#[test]
fn raw_numeric_driver_waits_for_original_author_set_value_and_stable_layout_before_press() {
    use crate::{
        authoring::{EditorAuthoringSession, test_heightfield_draft},
        resources::MapEditorState,
        ui::{
            gltf_browser::EditorGltfBrowserState,
            placement_transform::{
                PlacementTransformField, PlacementTransformInputState,
                sync_placement_transform_panel,
            },
        },
        viewport::{LayerCandidateState, placement_move::interaction_tests as original},
    };
    use feathrise_editor_ui::{
        EditorUiPlugin, EditorUiSystemSet, NumericInputConfig, NumericInputDisplayText,
        NumericInputEditState, NumericInputRoot, NumericInputSpec,
    };
    let (mut app, window, root) = fixture(1.5);
    let mut draft = test_heightfield_draft(vec![0; 121].into());
    draft.manifest.world_min = [-100.; 2];
    draft.manifest.world_max = [100.; 2];
    let mut record = original::record();
    record.translation[0] = 7.;
    let id = record.id.clone();
    draft.placements.placements.push(record);
    let map = draft.map_id.clone();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .insert_resource(MapEditorState {
        open: true,
        selected_map_id: Some(map),
        ..default()
    })
    .init_resource::<EditorGltfBrowserState>()
    .init_resource::<PlacementTransformInputState>()
    .init_resource::<LayerCandidateState>()
    .init_resource::<feathrise_editor_core::EditorSettings>()
    .add_plugins(EditorUiPlugin)
    .add_systems(
        Update,
        sync_placement_transform_panel.after(EditorUiSystemSet::NumericInput),
    );
    let revision = app
        .world()
        .resource::<EditorAuthoringSession>()
        .placement_structure_revision;
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .select_indexed_test_placement(&id, 0, revision);
    app.world_mut().entity_mut(root).insert((
        PlacementTransformField::TranslationX,
        NumericInputRoot,
        NumericInputConfig(NumericInputSpec::f32(3)),
        NumericInputValue(NumericValue::F32(0.)),
        NumericInputEditState::new(NumericValue::F32(0.), false),
    ));
    let text = app
        .world_mut()
        .spawn((NumericInputDisplayText { root }, Text::new("0.000")))
        .id();
    select_author(app.world_mut(), &id);
    assert!(!author_ready(app.world_mut(), root).0);
    assert!(change(app.world_mut(), root, 8.).is_err());
    app.update(); // 原 Presentation 生产 SetValue，但公共控件本帧已经跑过，仍不能以可见代替同步。
    assert_eq!(
        app.world().get::<NumericInputValue>(root).unwrap().0,
        NumericValue::F32(0.)
    );
    assert!(!author_ready(app.world_mut(), root).0);
    assert!(app.world().resource::<Driver>().actions.is_empty());
    app.update(); // 真实公共外部赋值桥采用，不由验收助手写值或文字。
    assert_eq!(
        app.world().get::<NumericInputValue>(root).unwrap().0,
        NumericValue::F32(7.)
    );
    assert_eq!(app.world().get::<Text>(text).unwrap().as_str(), "7.000");
    assert!(!author_ready(app.world_mut(), root).0);
    app.update();
    assert!(author_ready(app.world_mut(), root).0);
    assert_eq!(ready::expected_value(app.world()), Some(7.));
    // 已经同步值但布局仍变化时同样重新等待，不在旧中心起笔。
    app.world_mut()
        .entity_mut(root)
        .insert(UiGlobalTransform::from(Affine2::from_translation(
            Vec2::new(200.5, 140.),
        )));
    assert!(!author_ready(app.world_mut(), root).0);
    app.update();
    assert!(author_ready(app.world_mut(), root).0);
    change(app.world_mut(), root, 8.).unwrap();
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(app.world().resource::<Driver>().gesture.unwrap().start, 7.);
    let point = app
        .world()
        .get::<Window>(window)
        .unwrap()
        .cursor_position()
        .unwrap()
        * 1.5;
    assert!(
        (point.x - 211.).abs() < 0.001,
        "8 只能由起点 7 的十个物理像素得到，不能从旧 0 计算八十像素"
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn raw_numeric_driver_half_pixel_center_at_150_percent_survives_integer_window_echo() {
    use bevy::{
        camera::{ComputedCameraValues, RenderTarget, RenderTargetInfo},
        ui::{RelativeCursorPosition, UiStack},
        window::WindowRef,
    };
    use feathrise_editor_ui::{
        EditorUiPlugin, NumericInputConfig, NumericInputEditState, NumericInputFieldButton,
        NumericInputRoot, NumericInputSpec,
    };
    let (mut app, window, root) = fixture(1.5);
    app.add_plugins(EditorUiPlugin);
    let camera = app
        .world_mut()
        .spawn((
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(800, 600),
                        scale_factor: 1.5,
                    }),
                    ..default()
                },
                ..default()
            },
            RenderTarget::Window(WindowRef::Entity(window)),
        ))
        .id();
    app.world_mut().entity_mut(root).insert((
        Button,
        NumericInputRoot,
        NumericInputFieldButton { root },
        NumericInputConfig(NumericInputSpec::f32(3)),
        NumericInputEditState::new(NumericValue::F32(7.), false),
        UiTargetCamera(camera),
        Interaction::None,
        RelativeCursorPosition::default(),
        UiGlobalTransform::from(Affine2::from_translation(Vec2::new(200.5, 140.5))),
    ));
    app.insert_resource(UiStack {
        partition: vec![0..1],
        uinodes: vec![root],
    });
    // 与 Winit 的 CursorMoved 分支相同：Win32 返回整数物理位置，再换算成逻辑消息。
    fn echo(app: &mut App, window: Entity) {
        let mut state = app.world_mut().get_mut::<Window>(window).unwrap();
        let physical = state.physical_cursor_position().unwrap().round();
        state.set_physical_cursor_position(Some(physical.as_dvec2()));
        let position = physical / state.scale_factor();
        let message = CursorMoved {
            window,
            position,
            delta: None,
        };
        app.world_mut().write_message(message.clone());
        app.world_mut()
            .write_message(WindowEvent::CursorMoved(message));
    }
    change(app.world_mut(), root, 8.).unwrap();
    app.update(); // 整物理像素起点，随后真实 OS 回声先于按下进入公共消息流。
    assert_eq!(
        app.world()
            .get::<Window>(window)
            .unwrap()
            .physical_cursor_position(),
        Some(Vec2::new(201., 141.))
    );
    echo(&mut app, window);
    app.update();
    app.update();
    assert_eq!(
        app.world().get::<NumericInputValue>(root).unwrap().0,
        NumericValue::F32(8.)
    );
    echo(&mut app, window);
    app.update();
    assert_eq!(
        app.world().get::<NumericInputValue>(root).unwrap().0,
        NumericValue::F32(8.),
        "窗口回声不能重现7.95"
    );
    change(app.world_mut(), root, 9.).unwrap();
    app.update();
    echo(&mut app, window);
    app.update();
    assert_eq!(
        app.world().get::<NumericInputValue>(root).unwrap().0,
        NumericValue::F32(9.)
    );
    finish(app.world_mut(), root, false).unwrap();
    app.update();
    assert!(idle(app.world()));
    change(app.world_mut(), root, 10.).unwrap();
    app.update();
    echo(&mut app, window);
    app.update();
    app.update();
    echo(&mut app, window);
    app.update();
    assert_eq!(
        app.world().get::<NumericInputValue>(root).unwrap().0,
        NumericValue::F32(10.)
    );
    finish(app.world_mut(), root, true).unwrap();
    app.update();
    app.update();
    assert_eq!(
        app.world().get::<NumericInputValue>(root).unwrap().0,
        NumericValue::F32(9.)
    );
    assert!(idle(app.world()));
    assert!(
        probe(app.world()).unwrap()["pointer_move"]
            .as_u64()
            .unwrap()
            > 5,
        "确实消费额外窗口回声，不能只测自发的两条Move"
    );
}
