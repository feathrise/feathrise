//! 唯一浮层、原动态字段归还与真实鼠标门回归，不读取地图或启动文件任务。

use super::{components::*, resources::EditorContextHelpState, systems};
use crate::{resources::*, ui::sidebars::*};
use bevy::{
    ecs::system::RunSystemOnce,
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
    ui::RelativeCursorPosition,
};

struct Fixture {
    app: App,
    button: Entity,
    body: Entity,
    popup: Entity,
    scroll: Entity,
    close: Entity,
}

fn fixture() -> Fixture {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        })
        .init_resource::<MapEditorSidebarState>()
        .init_resource::<EditorContextHelpState>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<MouseWheel>()
        .add_systems(PreUpdate, systems::track_pointer)
        .add_systems(
            Update,
            (
                systems::handle_input,
                crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
            )
                .chain(),
        )
        .add_systems(PostUpdate, systems::sync_popup);
    let home = app
        .world_mut()
        .spawn((
            Node::default(),
            InheritedVisibility::VISIBLE,
            ComputedNode {
                size: Vec2::new(600.0, 480.0),
                ..default()
            },
        ))
        .id();
    let button = app
        .world_mut()
        .spawn((
            Button,
            InheritedVisibility::VISIBLE, // MinimalPlugins不运行可见性传播，显式模拟已呈现的真实问号。
            ContextHelpButton("动态对象说明".into()),
            BackgroundColor::default(),
            Node {
                position_type: PositionType::Absolute,
                width: px(18),
                height: px(18),
                ..default()
            },
            ComputedNode {
                size: Vec2::splat(18.0),
                ..default()
            },
            ChildOf(home),
        ))
        .id();
    let body = app
        .world_mut()
        .spawn((
            ContextHelpBody,
            Text::new("原动态字段"),
            Node {
                display: Display::None,
                ..default()
            },
            ChildOf(button),
        ))
        .id();
    let popup = app
        .world_mut()
        .spawn((
            ContextHelpPopup,
            RelativeCursorPosition::default(),
            InheritedVisibility::VISIBLE,
            Node {
                display: Display::None,
                ..default()
            },
            ComputedNode {
                size: Vec2::new(420.0, 240.0),
                ..default()
            },
        ))
        .id();
    let scroll = app
        .world_mut()
        .spawn((
            ContextHelpScroll,
            RelativeCursorPosition::default(),
            ScrollPosition::default(),
            Node::default(),
            ComputedNode {
                size: Vec2::new(400.0, 180.0),
                content_size: Vec2::new(400.0, 600.0),
                ..default()
            },
            ChildOf(popup),
        ))
        .id();
    app.world_mut()
        .spawn((ContextHelpTitle, Text::default(), ChildOf(popup)));
    let close = app
        .world_mut()
        .spawn((ContextHelpClose, Interaction::None, ChildOf(popup)))
        .id();
    app.update();
    Fixture {
        app,
        button,
        body,
        popup,
        scroll,
        close,
    }
}

fn press_help(test: &mut Fixture) {
    test.app.world_mut().entity_mut(test.button).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    test.app.update();
    *test
        .app
        .world_mut()
        .get_mut::<Interaction>(test.button)
        .unwrap() = Interaction::None;
    test.app.update();
}

fn open(test: &mut Fixture) {
    press_help(test);
    assert!(test.app.world().resource::<EditorContextHelpState>().active.is_some(),
        "公共打开夹具必须真实进入帮助，后续关闭/模态回归不能空通过");
    assert_eq!(test.app.world().get::<ChildOf>(test.body).unwrap().parent(), test.scroll);
}

/// 根偏移和物理倍率由布局夹具显式给出，验证鼠标坐标只转换一次，不借系统DPI碰巧通过。
fn positioned_fixture(viewport: Vec2, scale: f32, pointer: Vec2) -> (Fixture, Entity) {
    let mut test = fixture();
    let origin = Vec2::new(60.0, 32.0);
    let size = viewport * scale;
    let root = test.app.world_mut().spawn((
        crate::components::MapEditorRoot,
        Node::default(),
        InheritedVisibility::VISIBLE,
        ComputedNode { size, inverse_scale_factor: scale.recip(), ..default() },
        UiGlobalTransform::from(bevy::math::Affine2::from_translation(origin + size * 0.5)),
    )).id();
    let home = test.app.world().get::<ChildOf>(test.button).unwrap().parent();
    test.app.world_mut().entity_mut(home).insert(ChildOf(root));
    test.app.world_mut().entity_mut(test.popup).insert(ChildOf(root));
    test.app.world_mut().entity_mut(test.button).insert(
        UiGlobalTransform::from(bevy::math::Affine2::from_translation(origin + pointer * scale)),
    );
    let mut window = Window { focused: true, ..default() };
    window.resolution.set_physical_resolution((origin.x + size.x + 50.0) as u32, (origin.y + size.y + 50.0) as u32);
    window.set_physical_cursor_position(Some((origin + pointer * scale).as_dvec2()));
    let window = test.app.world_mut().spawn((window, bevy::window::PrimaryWindow)).id();
    test.app.add_systems(PostUpdate, systems::position_popup.after(systems::sync_popup));
    (test, window)
}

#[test]
fn help_opens_beside_click_across_root_offsets_and_scales_and_stays_put_while_reading() {
    for scale in [0.6, 1.0, 1.5, 2.0] {
        let point = Vec2::new(80.0, 100.0);
        let (mut test, window) = positioned_fixture(Vec2::new(900.0, 700.0), scale, point);
        open(&mut test);
        let before = test.app.world().get::<Node>(test.popup).unwrap().clone();
        let (Val::Px(left), Val::Px(top)) = (before.left, before.top) else { panic!("帮助需要鼠标旁的明确坐标") };
        assert!((Vec2::new(left, top) - point - Vec2::splat(12.0)).abs().max_element() < 0.001);
        assert_eq!((before.right, before.bottom), (Val::Auto, Val::Auto));
        test.app.world_mut().get_mut::<Window>(window).unwrap()
            .set_physical_cursor_position(Some(Vec2::new(650.0, 530.0).as_dvec2()));
        test.app.update();
        let after = test.app.world().get::<Node>(test.popup).unwrap();
        assert_eq!((after.left, after.top, after.bottom), (before.left, before.top, before.bottom),
            "帮助打开后移动鼠标不能让正文跟随或跳回固定角落");
    }
}

#[test]
fn help_flips_at_bottom_right_and_limits_long_content_inside_narrow_roots() {
    for viewport in [Vec2::new(900.0, 700.0), Vec2::new(256.0, 240.0)] {
        for scale in [1.0, 1.5, 2.0] {
            let point = viewport - Vec2::splat(10.0);
            let (mut test, _) = positioned_fixture(viewport, scale, point);
            open(&mut test);
            let node = test.app.world().get::<Node>(test.popup).unwrap();
            let (Val::Px(left), Val::Px(bottom), Val::Px(width), Val::Px(height)) =
                (node.left, node.bottom, node.width, node.max_height) else { panic!("帮助需要明确的有界位置与滚动高度") };
            assert_eq!(node.top, Val::Auto, "底部问号应向上展开，避免正文越过窗口");
            assert!((bottom - 22.0).abs() < 0.001);
            assert!(left >= 6.0 && left + width <= viewport.x - 6.0 + 0.001);
            assert!(height > 40.0 && height + bottom <= viewport.y - 6.0 + 0.001);
            assert!((width - 420.0_f32.min(viewport.x - 12.0)).abs() < 0.001);
        }
    }
}

#[test]
fn help_borrows_one_original_body_and_keeps_inline_height_zero() {
    let mut test = fixture();
    assert_eq!(
        test.app.world().get::<Node>(test.body).unwrap().display,
        Display::None
    );
    assert_eq!(
        test.app
            .world()
            .get::<Node>(test.button)
            .unwrap()
            .position_type,
        PositionType::Absolute
    );
    open(&mut test);
    assert_eq!(
        test.app.world().get::<ChildOf>(test.body).unwrap().parent(),
        test.scroll
    );
    assert_eq!(
        test.app.world().get::<Node>(test.popup).unwrap().display,
        Display::Flex
    );
    **test.app.world_mut().get_mut::<Text>(test.body).unwrap() = "本帧更新后的原动态字段".into();
    test.app.update();
    assert_eq!(
        test.app.world().get::<Text>(test.body).unwrap().as_str(),
        "本帧更新后的原动态字段"
    );
    let tick = test
        .app
        .world()
        .entity(test.body)
        .get_ref::<Node>()
        .unwrap()
        .last_changed();
    test.app.update();
    assert_eq!(
        test.app
            .world()
            .entity(test.body)
            .get_ref::<Node>()
            .unwrap()
            .last_changed(),
        tick,
        "稳定帧不得在隐藏与显露间反复写Node"
    );
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    test.app.update();
    assert_eq!(
        test.app.world().get::<ChildOf>(test.body).unwrap().parent(),
        test.button
    );
    assert_eq!(
        test.app.world().get::<Node>(test.body).unwrap().display,
        Display::None
    );
    assert_eq!(
        test.app.world().get::<Node>(test.popup).unwrap().display,
        Display::None
    );
    assert!(
        !test
            .app
            .world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Escape)
    );
    let world = test.app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<ContextHelpBody>>()
            .iter(world)
            .count(),
        1
    );
}

#[test]
fn legacy_dynamic_help_moves_all_original_fields_and_returns_them_on_page_change() {
    let mut test = fixture();
    let home = test.app.world_mut().spawn((Node::default(), ComputedNode {
        size: Vec2::new(600.0, 480.0), ..default()
    }, InheritedVisibility::VISIBLE)).id();
    let bodies: Vec<_> = (0..2)
        .map(|index| {
            test.app
                .world_mut()
                .spawn((
                    MapEditorSidebarHelpBody(EditorSidebarHelp::Placement),
                    Text::new(format!("原说明{index}")),
                    Node {
                        display: Display::None,
                        ..default()
                    },
                    ChildOf(home),
                ))
                .id()
        })
        .collect();
    test.app
        .world_mut()
        .resource_mut::<MapEditorSidebarState>()
        .toggle_help(EditorSidebarHelp::Placement);
    test.app.update();
    for body in &bodies {
        assert_eq!(
            test.app.world().get::<ChildOf>(*body).unwrap().parent(),
            test.scroll
        );
    }
    test.app
        .world_mut()
        .resource_mut::<MapEditorSidebarState>()
        .navigator = EditorNavigatorTab::Scene;
    test.app.update();
    for body in bodies {
        assert_eq!(
            test.app.world().get::<ChildOf>(body).unwrap().parent(),
            home
        );
        assert_eq!(
            test.app.world().get::<Node>(body).unwrap().display,
            Display::None
        );
    }
    assert!(
        test.app
            .world()
            .resource::<EditorContextHelpState>()
            .active
            .is_none()
    );
}

#[test]
fn help_scroll_close_and_outside_click_keep_mouse_until_release_and_quiet_frame() {
    let mut test = fixture();
    open(&mut test);
    test.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(test.popup)
        .unwrap()
        .cursor_over = true;
    test.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(test.scroll)
        .unwrap()
        .cursor_over = true;
    test.app.update();
    test.app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Line,
        x: 0.0,
        y: -2.0,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    test.app.update();
    assert!(
        test.app
            .world()
            .get::<ScrollPosition>(test.scroll)
            .unwrap()
            .y
            > 0.0
    );
    assert!(
        test.app
            .world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    test.app.world_mut().entity_mut(test.close).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    test.app.update();
    assert!(
        test.app
            .world()
            .resource::<EditorContextHelpState>()
            .active
            .is_none()
    );
    assert!(
        test.app
            .world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    *test
        .app
        .world_mut()
        .get_mut::<Interaction>(test.close)
        .unwrap() = Interaction::None;
    test.app
        .world_mut()
        .get_mut::<RelativeCursorPosition>(test.popup)
        .unwrap()
        .cursor_over = false;
    test.app.update();
    open(&mut test);
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    test.app.update();
    assert!(
        test.app
            .world()
            .resource::<EditorContextHelpState>()
            .active
            .is_none()
    );
    assert!(
        test.app
            .world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked(),
        "点外关闭不能同帧触发地图选择/笔刷"
    );
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    test.app.update();
    assert!(
        test.app
            .world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    test.app.update();
    assert!(
        !test
            .app
            .world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
}

#[test]
fn closing_editor_returns_help_without_losing_original_text_entity() {
    let mut test = fixture();
    open(&mut test);
    test.app.world_mut().resource_mut::<MapEditorState>().open = false;
    test.app.update();
    assert!(
        test.app
            .world()
            .resource::<EditorContextHelpState>()
            .active
            .is_none()
    );
    assert_eq!(
        test.app.world().get::<ChildOf>(test.body).unwrap().parent(),
        test.button
    );
    assert_eq!(
        test.app.world().get::<Text>(test.body).unwrap().as_str(),
        "原动态字段"
    );
}

#[test]
fn stale_help_press_cannot_open_from_hidden_ancestor_or_another_navigation_page() {
    for hidden_by_display in [true, false] {
        let mut test = fixture();
        let home = test
            .app
            .world()
            .get::<ChildOf>(test.button)
            .unwrap()
            .parent();
        if hidden_by_display {
            test.app.world_mut().get_mut::<Node>(home).unwrap().display = Display::None;
        } else {
            test.app
                .world_mut()
                .entity_mut(home)
                .insert(MapEditorSidebarPage(EditorSidebarTab::Navigator(
                    EditorNavigatorTab::Assets,
                )));
        }
        test.app.world_mut().entity_mut(test.button).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        test.app.update();
        assert!(
            test.app
                .world()
                .resource::<EditorContextHelpState>()
                .active
                .is_none()
        );
        assert_eq!(
            test.app.world().get::<ChildOf>(test.body).unwrap().parent(),
            test.button
        );
        assert_eq!(
            test.app.world().get::<Node>(test.body).unwrap().display,
            Display::None
        );
    }
}

#[test]
fn hiding_a_tool_subpage_returns_mounted_help_even_when_top_level_page_is_unchanged() {
    let mut test = fixture();
    let home = test.app.world().get::<ChildOf>(test.button).unwrap().parent();
    open(&mut test);
    assert_eq!(test.app.world().get::<ChildOf>(test.body).unwrap().parent(), test.scroll);
    let sidebar = test.app.world().resource::<MapEditorSidebarState>();
    let page_before = (sidebar.navigator, sidebar.left, sidebar.right);
    // 真实塑形子页/属性折叠只改原父级Display，不改变顶层枚举；移挂正文不能继续留在根。
    test.app.world_mut().get_mut::<Node>(home).unwrap().display = Display::None;
    test.app.update();
    let sidebar = test.app.world().resource::<MapEditorSidebarState>();
    assert_eq!((sidebar.navigator, sidebar.left, sidebar.right), page_before);
    assert!(test.app.world().resource::<EditorContextHelpState>().active.is_none());
    assert_eq!(test.app.world().get::<ChildOf>(test.body).unwrap().parent(), test.button);
    assert_eq!(test.app.world().get::<Node>(test.body).unwrap().display, Display::None);
    assert_eq!(test.app.world().get::<Node>(test.popup).unwrap().display, Display::None);
    test.app.world_mut().get_mut::<Node>(home).unwrap().display = Display::Flex;
    test.app.update();
    assert!(test.app.world().resource::<EditorContextHelpState>().active.is_none());
}

#[test]
fn collapsing_either_source_pane_returns_help_without_checking_its_borrowed_body_parent() {
    for content in [true, false] {
        let mut test = fixture();
        test.app.init_resource::<MapEditorLayoutState>();
        let home = test.app.world().get::<ChildOf>(test.button).unwrap().parent();
        let page = if content {
            EditorSidebarTab::Navigator(EditorNavigatorTab::Maps)
        } else {
            EditorSidebarTab::Right(EditorRightSidebarTab::Scene)
        };
        test.app.world_mut().entity_mut(home).insert(MapEditorSidebarPage(page));
        open(&mut test);
        assert_eq!(test.app.world().get::<ChildOf>(test.body).unwrap().parent(), test.scroll,
            "帮助借出后来源检查仍应允许正常持续显示");
        {
            let mut layout = test.app.world_mut().resource_mut::<MapEditorLayoutState>();
            if content { layout.content_visible = false; } else { layout.inspector_visible = false; }
        }
        test.app.update();
        assert!(test.app.world().resource::<EditorContextHelpState>().active.is_none());
        assert_eq!(test.app.world().get::<ChildOf>(test.body).unwrap().parent(), test.button);
        assert_eq!(test.app.world().get::<Node>(test.popup).unwrap().display, Display::None);
    }
}

#[test]
fn legacy_help_closes_when_its_original_navigation_or_inspector_pane_is_collapsed() {
    for content in [true, false] {
        let mut test = fixture();
        test.app.init_resource::<MapEditorLayoutState>();
        let help = if content { EditorSidebarHelp::MapBrowser } else { EditorSidebarHelp::Inspector };
        let page = if content {
            EditorSidebarTab::Navigator(EditorNavigatorTab::Maps)
        } else {
            EditorSidebarTab::Right(EditorRightSidebarTab::Scene)
        };
        let home = test.app.world_mut().spawn((Node::default(), ComputedNode {
            size: Vec2::new(280.0, 480.0), ..default()
        }, InheritedVisibility::VISIBLE, MapEditorSidebarPage(page))).id();
        let body = test.app.world_mut().spawn((
            MapEditorSidebarHelpBody(help), Text::new("原侧栏动态信息"),
            Node { display: Display::None, ..default() }, ChildOf(home),
        )).id();
        test.app.world_mut().resource_mut::<MapEditorSidebarState>().toggle_help(help);
        test.app.update();
        assert_eq!(test.app.world().get::<ChildOf>(body).unwrap().parent(), test.scroll);
        assert!(test.app.world().resource::<EditorContextHelpState>().active.is_some());
        {
            let mut layout = test.app.world_mut().resource_mut::<MapEditorLayoutState>();
            if content { layout.content_visible = false; } else { layout.inspector_visible = false; }
        }
        test.app.update();
        assert!(test.app.world().resource::<EditorContextHelpState>().active.is_none());
        assert!(test.app.world().resource::<MapEditorSidebarState>().active_help().is_none());
        assert_eq!(test.app.world().get::<ChildOf>(body).unwrap().parent(), home);
        assert_eq!(test.app.world().get::<Node>(body).unwrap().display, Display::None);
        assert_eq!(test.app.world().get::<Text>(body).unwrap().as_str(), "原侧栏动态信息");
        test.app.update();
        assert!(test.app.world().resource::<EditorContextHelpState>().active.is_none());
    }
}

fn set_management_modal(app: &mut App, mode: Option<usize>) {
    app.init_resource::<MapBrowserState>()
        .init_resource::<MapDeletionTask>()
        .init_resource::<crate::authoring::TerrainExternalChangeState>();
    app.world_mut().resource_mut::<MapBrowserState>().dialog = None;
    app.world_mut().resource_mut::<MapDeletionTask>().confirmation = None;
    app.world_mut().resource_mut::<crate::authoring::TerrainExternalChangeState>().conflict = None;
    match mode {
        Some(0) => {
            app.world_mut().resource_mut::<MapBrowserState>().dialog = Some(crate::types::MapFolderDialogState {
                mode: crate::types::MapFolderDialogMode::Create,
                target_folder_id: None,
                name: "中文目录".into(),
            });
        }
        Some(1) => {
            app.world_mut().resource_mut::<MapDeletionTask>().confirmation = Some(crate::types::MapDeletionRequest {
                map_id: feathrise_map_content::MapId::parse("MAP-context-help-delete").unwrap(),
                display_name: "待删除地图".into(),
                folder: "maps/MAP-context-help-delete".into(),
            });
        }
        Some(2) => {
            app.world_mut().resource_mut::<crate::authoring::TerrainExternalChangeState>()
                .register_save_conflict(feathrise_map_content::MapId::parse("MAP-context-help-conflict").unwrap(), [7; 32]);
        }
        None => {},
        Some(_) => unreachable!(),
    }
}

#[test]
fn management_modals_return_help_and_consume_hidden_old_presses() {
    for mode in 0..3 {
        let mut test = fixture();
        open(&mut test);
        set_management_modal(&mut test.app, Some(mode));
        test.app.update();
        assert!(test.app.world().resource::<EditorContextHelpState>().active.is_none());
        assert_eq!(test.app.world().get::<ChildOf>(test.body).unwrap().parent(), test.button);
        assert_eq!(test.app.world().get::<Node>(test.popup).unwrap().display, Display::None);
        test.app.world_mut().entity_mut(test.button).insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        test.app.update();
        assert!(test.app.world().resource::<EditorContextHelpState>().active.is_none());
        set_management_modal(&mut test.app, None);
        test.app.update();
        assert!(test.app.world().resource::<EditorContextHelpState>().active.is_none(),
            "模态关闭不得重放被遮挡的旧帮助按下");
        assert_eq!(test.app.world().get::<Text>(test.body).unwrap().as_str(), "原动态字段");
    }
}

#[test]
fn late_management_modal_before_presentation_also_closes_and_returns_help() {
    for mode in 0..3 {
        let mut test = fixture();
        open(&mut test);
        // 异步采用可能发生在本帧帮助输入之后；表现层也必须检查同一个正式模态条件。
        set_management_modal(&mut test.app, Some(mode));
        test.app.world_mut().run_system_once(systems::sync_popup).unwrap();
        assert!(test.app.world().resource::<EditorContextHelpState>().active.is_none());
        assert_eq!(test.app.world().get::<ChildOf>(test.body).unwrap().parent(), test.button);
        assert_eq!(test.app.world().get::<Node>(test.body).unwrap().display, Display::None);
        assert_eq!(test.app.world().get::<Node>(test.popup).unwrap().display, Display::None);
    }
}

fn settings_fixture() -> Fixture {
    let mut test = fixture();
    test.app.init_resource::<MapEditorSettingsState>()
        .init_resource::<MapEditorLayoutState>()
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .add_systems(Update, crate::ui::editor_settings::handle_settings_input
            .after(systems::handle_input)
            .before(crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard));
    test.app
        .world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .open = true;
    let home = test
        .app
        .world()
        .get::<ChildOf>(test.button)
        .unwrap()
        .parent();
    test.app
        .world_mut()
        .entity_mut(home)
        .insert(crate::components::MapEditorSettingsPanel);
    test.app.update();
    test
}

#[test]
fn settings_help_uses_the_same_popup_and_escape_keeps_underlying_settings_open() {
    let mut test = settings_fixture();
    assert!(
        test.app
            .world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    open(&mut test); // 设置自身的模态鼠标门不能挡住模态内的问号。
    assert!(
        test.app
            .world()
            .resource::<EditorContextHelpState>()
            .from_settings
    );
    assert_eq!(
        test.app.world().get::<ChildOf>(test.body).unwrap().parent(),
        test.scroll
    );
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    test.app.update();
    assert!(
        test.app
            .world()
            .resource::<EditorContextHelpState>()
            .active
            .is_none()
    );
    assert!(
        test.app.world().resource::<MapEditorSettingsState>().open,
        "Esc只关闭最上层帮助，不顺带关闭设置"
    );
    assert_eq!(
        test.app.world().get::<ChildOf>(test.body).unwrap().parent(),
        test.button
    );
    let world = test.app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<ContextHelpPopup>>()
            .iter(world)
            .count(),
        1
    );
}

#[test]
fn settings_help_stays_available_when_both_workbench_panes_are_collapsed() {
    let mut test = settings_fixture();
    {
        let mut layout = test.app.world_mut().resource_mut::<MapEditorLayoutState>();
        layout.content_visible = false;
        layout.inspector_visible = false;
    }
    open(&mut test);
    test.app.update();
    assert!(test.app.world().resource::<EditorContextHelpState>().from_settings);
    assert!(test.app.world().resource::<EditorContextHelpState>().active.is_some());
    assert_eq!(test.app.world().get::<ChildOf>(test.body).unwrap().parent(), test.scroll);
    assert_eq!(test.app.world().get::<Node>(test.popup).unwrap().display, Display::Flex);
}

#[test]
fn settings_only_allows_its_own_help_and_never_bypasses_numeric_hold_or_release() {
    let mut behind = fixture();
    behind.app.init_resource::<MapEditorSettingsState>();
    behind
        .app
        .world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .open = true;
    press_help(&mut behind);
    assert!(
        behind
            .app
            .world()
            .resource::<EditorContextHelpState>()
            .active
            .is_none(),
        "设置打开后底层页面的残留问号不能穿透模态"
    );

    let mut test = settings_fixture();
    test.app
        .world_mut()
        .resource_mut::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .entity = Some(test.button);
    press_help(&mut test);
    assert!(
        test.app
            .world()
            .resource::<EditorContextHelpState>()
            .active
            .is_none()
    );
    test.app
        .world_mut()
        .resource_mut::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .entity = None;
    {
        let mut mouse = test
            .app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>();
        mouse.press(MouseButton::Left);
        mouse.release(MouseButton::Left);
    }
    press_help(&mut test);
    assert!(
        test.app
            .world()
            .resource::<EditorContextHelpState>()
            .active
            .is_none(),
        "实际数值拖动结束的release帧不能被设置内问号绕过"
    );
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    test.app.update();
    open(&mut test);
    assert!(
        test.app
            .world()
            .resource::<EditorContextHelpState>()
            .from_settings
    );
}
