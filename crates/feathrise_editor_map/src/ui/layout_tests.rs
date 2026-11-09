//! 编辑器布局的专注与单侧显隐、窄窗恢复、真实 ECS 查询和鼠标所有权回归，不启动渲染。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::*;
use crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard;
use feathrise_editor_ui::ActiveNumericInputScrub;

fn layout_app(width: u32, scale: f32) -> App {
    let mut app = App::new();
    app.insert_resource(MapEditorState {
        open: true,
        ..default()
    })
    .init_resource::<MapEditorLayoutState>()
    .init_resource::<EditorSettings>()
    .add_systems(Startup, restore_editor_layout_preferences)
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .init_resource::<ActiveNumericInputScrub>()
    .init_resource::<ButtonInput<MouseButton>>()
    .init_resource::<VegetationPresetEditorState>()
    .insert_resource(UiScale(scale))
    .add_systems(
        Update,
        (
            handle_editor_layout_input,
            remember_editor_layout_preferences,
            update_map_editor_numeric_scrub_mouse_guard,
            sync_editor_layout,
        )
            .chain(),
    );
    app.world_mut().spawn((
        Window {
            resolution: bevy::window::WindowResolution::new(width, 1080),
            ..default()
        },
        PrimaryWindow,
    ));
    app
}

fn cursor(app: &mut App, position: f32) {
    let world = app.world_mut();
    let mut window = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
    window
        .single_mut(world)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(position, 100.0)));
}

#[test]
fn editor_layout_leaving_focus_opens_both_sides_and_keeps_preferred_widths() {
    for before in [(false, false), (false, true), (true, false), (true, true)] {
        let mut app = layout_app(1400, 1.0);
        let original = feathrise_editor_core::EditorLayoutPreferences {
            content_visible: before.0, inspector_visible: before.1,
            content_width: 312.0, inspector_width: 440.0,
        };
        app.world_mut().resource_mut::<EditorSettings>().editor_layout = original;
        app.update();
        app.world_mut().resource_mut::<MapEditorLayoutState>().toggle(EditorLayoutAction::Focus);
        app.update();
        let state = app.world().resource::<MapEditorLayoutState>();
        assert!(!state.content_visible && !state.inspector_visible);
        assert_eq!(state.focus_restore, Some(before));
        assert_eq!(app.world().resource::<EditorSettings>().editor_layout, original);
        app.world_mut().resource_mut::<MapEditorLayoutState>().toggle(EditorLayoutAction::Focus);
        app.update();
        let state = app.world().resource::<MapEditorLayoutState>();
        assert!(state.content_visible && state.inspector_visible, "{before:?}");
        assert!(state.focus_restore.is_none());
        assert_eq!((state.content_width, state.inspector_width), (312.0, 440.0));
        let shown = feathrise_editor_core::EditorLayoutPreferences {
            content_visible: true, inspector_visible: true, ..original
        };
        assert_eq!(state.preferences(), shown);
        assert_eq!(app.world().resource::<EditorSettings>().editor_layout, shown);
    }
}

#[test]
fn editor_layout_leaving_focus_opens_only_the_requested_side_for_every_previous_layout() {
    // 专注前的四种布局都不能影响单侧显式操作；快照只在专注期间保护首选布局。
    for before in [(false, false), (false, true), (true, false), (true, true)] {
        for (side, other, expected) in [
            (EditorLayoutAction::Content, EditorLayoutAction::Inspector, (true, false)),
            (EditorLayoutAction::Inspector, EditorLayoutAction::Content, (false, true)),
        ] {
            let mut state = MapEditorLayoutState {
                content_visible: before.0, inspector_visible: before.1,
                content_width: 312.0, inspector_width: 440.0, ..default()
            };
            let original = state.preferences();
            state.toggle(EditorLayoutAction::Focus);
            assert_eq!(state.focus_restore, Some(before));
            assert_eq!(state.preferences(), original, "临时专注不能覆盖首选布局");
            state.toggle(side);
            assert_eq!((state.content_visible, state.inspector_visible), expected, "{before:?}/{side:?}");
            assert!(state.focus_restore.is_none(), "显式单侧操作必须丢弃旧快照");
            let explicit = state.preferences();
            assert_eq!((explicit.content_visible, explicit.inspector_visible), expected);
            assert_eq!((explicit.content_width, explicit.inspector_width), (312.0, 440.0));
            state.toggle(side);
            assert!(!state.content_visible && !state.inspector_visible, "再次点击同侧只关闭该侧");
            state.toggle(other);
            let other_only = (expected.1, expected.0);
            assert_eq!((state.content_visible, state.inspector_visible), other_only);
            state.toggle(EditorLayoutAction::Focus);
            state.toggle(EditorLayoutAction::Focus);
            assert!(state.content_visible && state.inspector_visible, "再次退出专注应全开，不复活旧单侧布局");
            assert!(state.focus_restore.is_none());
        }
    }
}

#[test]
fn editor_layout_focus_side_input_keeps_panes_selected_colors_and_preferences_in_sync() {
    fn click(app: &mut App, button: Entity) {
        *app.world_mut().get_mut::<feathrise_editor_ui::EditorButtonClick>(button).unwrap() =
            feathrise_editor_ui::EditorButtonClick::triggered();
        app.update(); // 只给原输入系统动作脉冲，不直接写布局或选中颜色。
        *app.world_mut().get_mut::<feathrise_editor_ui::EditorButtonClick>(button).unwrap() = default();
        app.update();
        app.update(); // 经过原释放帧鼠标隔离，下一次独立点击才可进入。
    }

    for (side_index, expected) in [(0, [true, false, false]), (1, [false, true, false])] {
        let mut app = layout_app(1400, 1.0);
        let content = app.world_mut().spawn((Node::default(), MapEditorPane(EditorPane::Content))).id();
        let inspector = app.world_mut().spawn((Node::default(), MapEditorPane(EditorPane::Inspector))).id();
        let buttons = [EditorLayoutAction::Content, EditorLayoutAction::Inspector, EditorLayoutAction::Focus]
            .map(|action| app.world_mut().spawn((
                MapEditorLayoutButton(action), feathrise_editor_ui::EditorButtonClick::default(),
                UiInteraction::None, BackgroundColor::default(), BorderColor::default(),
                feathrise_editor_ui::ButtonColors::new(colors::BUTTON_DEV_SMALL_NORMAL,
                    colors::BUTTON_DEV_SMALL_HOVERED, colors::SELECT_BG_PRESSED),
            )).id());
        app.update();
        let original = app.world().resource::<EditorSettings>().editor_layout;
        click(&mut app, buttons[2]);
        assert_eq!(app.world().get::<Node>(content).unwrap().display, Display::None);
        assert_eq!(app.world().get::<Node>(inspector).unwrap().display, Display::None);
        assert_eq!(app.world().resource::<EditorSettings>().editor_layout, original);
        click(&mut app, buttons[side_index]);
        let state = app.world().resource::<MapEditorLayoutState>();
        assert!(state.focus_restore.is_none());
        assert_eq!((state.content_shown(), state.inspector_shown()), (expected[0], expected[1]));
        for (entity, visible) in [(content, expected[0]), (inspector, expected[1])] {
            assert_eq!(app.world().get::<Node>(entity).unwrap().display,
                if visible { Display::Flex } else { Display::None });
        }
        for (button, selected) in buttons.into_iter().zip(expected) {
            let expected_color = if selected { colors::SELECT_BG } else { colors::BUTTON_DEV_SMALL_NORMAL };
            assert_eq!(app.world().get::<BackgroundColor>(button).unwrap().0, expected_color);
            assert_eq!(app.world().get::<feathrise_editor_ui::ButtonColors>(button).unwrap().normal, expected_color);
        }
        let saved = app.world().resource::<EditorSettings>().editor_layout;
        assert_eq!((saved.content_visible, saved.inspector_visible), (expected[0], expected[1]));
        assert_eq!((saved.content_width, saved.inspector_width), (original.content_width, original.inspector_width));
        click(&mut app, buttons[side_index]);
        let saved = app.world().resource::<EditorSettings>().editor_layout;
        assert!(!saved.content_visible && !saved.inspector_visible);
        click(&mut app, buttons[2]);
        click(&mut app, buttons[2]);
        assert_eq!(app.world().get::<Node>(content).unwrap().display, Display::Flex);
        assert_eq!(app.world().get::<Node>(inspector).unwrap().display, Display::Flex);
        for (button, selected) in buttons.into_iter().zip([true, true, false]) {
            let expected_color = if selected { colors::SELECT_BG } else { colors::BUTTON_DEV_SMALL_NORMAL };
            assert_eq!(app.world().get::<BackgroundColor>(button).unwrap().0, expected_color);
            assert_eq!(app.world().get::<feathrise_editor_ui::ButtonColors>(button).unwrap().normal, expected_color);
        }
        let saved = app.world().resource::<EditorSettings>().editor_layout;
        assert!(saved.content_visible && saved.inspector_visible);
    }
}

#[test]
fn editor_layout_narrow_window_preserves_workspace_and_restores_preferences() {
    let state = MapEditorLayoutState {
        content_width: 310.0,
        inspector_width: 450.0,
        ..default()
    };
    let wide = pane_layout(&state, 1400.0);
    assert_eq!((wide.content, wide.inspector), (Some(310.0), Some(450.0)));
    let narrow = pane_layout(&state, 910.0);
    assert!(narrow.content.unwrap() >= EDITOR_CONTENT_MIN_WIDTH);
    assert!(narrow.inspector.unwrap() >= EDITOR_INSPECTOR_MIN_WIDTH);
    assert!(
        narrow.content.unwrap() + narrow.inspector.unwrap() + EDITOR_SPLITTER_WIDTH * 2.0
            <= 910.0 - EDITOR_WORKSPACE_MIN_WIDTH
    );
    let smaller = pane_layout(&state, 820.0);
    assert!(smaller.content.is_none());
    assert!(smaller.inspector.is_some());
    let smallest = pane_layout(&state, 600.0);
    assert!(smallest.content.is_none() && smallest.inspector.is_none());
    let restored = pane_layout(&state, 1400.0);
    assert_eq!(
        (restored.content, restored.inspector),
        (wide.content, wide.inspector)
    );
    assert!(state.content_visible && state.inspector_visible);
}

#[test]
fn editor_layout_sync_queries_are_disjoint_and_skill_hides_only_map_context() {
    let mut app = layout_app(1400, 1.0);
    let content = app
        .world_mut()
        .spawn((Node::default(), MapEditorPane(EditorPane::Content)))
        .id();
    let inspector = app
        .world_mut()
        .spawn((Node::default(), MapEditorPane(EditorPane::Inspector)))
        .id();
    let context = app
        .world_mut()
        .spawn((Node::default(), MapEditorSceneContext))
        .id();
    let hint = app.world_mut().spawn((Node::default(), MapEditorSceneContext,
        crate::components::MapEditorSceneContextHint)).id();
    let splitter = app
        .world_mut()
        .spawn((
            Node::default(),
            MapEditorPaneSplitter(EditorPane::Inspector),
            UiInteraction::None,
            BackgroundColor::default(),
        ))
        .id();
    app.world_mut().spawn((
        MapEditorLayoutButton(EditorLayoutAction::Inspector),
        UiInteraction::None,
        BackgroundColor::default(),
        BorderColor::default(),
    ));
    let label = app
        .world_mut()
        .spawn((
            Text::new("属性"),
            MapEditorLayoutButtonText(EditorLayoutAction::Inspector),
        ))
        .id();
    let scene_label = app
        .world_mut()
        .spawn((Text::new("场景"), MapEditorSidebarTabText))
        .id();
    app.world_mut()
        .spawn(MapEditorSidebarTabButton(EditorSidebarTab::Right(
            EditorRightSidebarTab::Scene,
        )))
        .add_child(scene_label);
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Domain("test.author", 17);
    app.update();
    assert_eq!(
        app.world().get::<Node>(content).unwrap().width,
        Val::Px(feathrise_editor_core::EditorLayoutPreferences::DEFAULT_CONTENT_WIDTH)
    );
    assert_eq!(
        app.world().get::<Node>(inspector).unwrap().width,
        Val::Px(feathrise_editor_core::EditorLayoutPreferences::DEFAULT_INSPECTOR_WIDTH)
    );
    assert_eq!(
        app.world().get::<Node>(context).unwrap().display,
        Display::None
    );
    assert_eq!(app.world().get::<Text>(label).unwrap().as_str(), "工作区");
    assert_eq!(app.world().get::<Node>(hint).unwrap().display, Display::Flex);
    assert_eq!(
        app.world().get::<Text>(scene_label).unwrap().as_str(),
        "预览"
    );
    for tab in [
        PresetWorkspaceTab::Domain("test.author", 17),
        PresetWorkspaceTab::Water(17),
    ] {
        app.world_mut()
            .resource_mut::<VegetationPresetEditorState>()
            .active_tab = tab;
        app.update();
        assert_eq!(
            app.world().get::<Node>(context).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Text>(scene_label).unwrap().as_str(),
            "预览"
        );
    } // 应用与静水独立预览都隐藏整份地图上下文，不增加相机或重复属性树。
    app.world_mut()
        .resource_mut::<MapEditorLayoutState>()
        .toggle(EditorLayoutAction::Focus);
    app.update();
    assert_eq!(
        app.world().get::<Node>(content).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world().get::<Node>(inspector).unwrap().display,
        Display::None
    );
    assert_eq!(
        app.world().get::<Node>(splitter).unwrap().display,
        Display::None
    );
    assert!(
        !app.world()
            .resource::<MapEditorLayoutState>()
            .inspector_shown()
    );
    app.world_mut()
        .resource_mut::<MapEditorLayoutState>()
        .toggle(EditorLayoutAction::Focus);
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Map;
    app.update();
    assert_eq!(
        app.world().get::<Node>(inspector).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Node>(context).unwrap().display,
        Display::Flex
    );
    assert_eq!(
        app.world().get::<Text>(scene_label).unwrap().as_str(),
        "属性"
    );
    assert_eq!(app.world().get::<Node>(hint).unwrap().display, Display::None);
}

#[test]
fn editor_layout_splitter_respects_ui_scale_and_chord_release_gate() {
    for pane in [EditorPane::Content, EditorPane::Inspector] {
        let mut app = layout_app(2400, 2.0);
        app.world_mut()
            .spawn((MapEditorPaneSplitter(pane), UiInteraction::Hovered));
        cursor(&mut app, 400.0);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(
            app.world()
                .resource::<MapEditorLayoutState>()
                .drag
                .is_some()
        );
        assert!(
            app.world()
                .resource::<MapEditorNumericScrubMouseGuard>()
                .blocked()
        );
        cursor(&mut app, 480.0);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right);
        app.update();
        let state = app.world().resource::<MapEditorLayoutState>();
        match pane {
            EditorPane::Content => assert_eq!(state.content_width, feathrise_editor_core::EditorLayoutPreferences::DEFAULT_CONTENT_WIDTH + 40.0),
            EditorPane::Inspector => assert_eq!(state.inspector_width,
                (feathrise_editor_core::EditorLayoutPreferences::DEFAULT_INSPECTOR_WIDTH - 40.0).max(feathrise_editor_core::EditorLayoutPreferences::INSPECTOR_MIN_WIDTH)),
        }
        // 每帧移动仍只修改会话宽度；松手一次才交给设置自动保存。
        assert_eq!(app.world().resource::<EditorSettings>().editor_layout, default());
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        assert!(
            app.world()
                .resource::<MapEditorLayoutState>()
                .drag
                .is_none()
        );
        let saved = app.world().resource::<EditorSettings>().editor_layout;
        match pane {
            EditorPane::Content => assert_eq!(saved.content_width, feathrise_editor_core::EditorLayoutPreferences::DEFAULT_CONTENT_WIDTH + 40.0),
            EditorPane::Inspector => assert_eq!(saved.inspector_width,
                (feathrise_editor_core::EditorLayoutPreferences::DEFAULT_INSPECTOR_WIDTH - 40.0).max(feathrise_editor_core::EditorLayoutPreferences::INSPECTOR_MIN_WIDTH)),
        }
        assert!(
            app.world()
                .resource::<MapEditorNumericScrubMouseGuard>()
                .blocked()
        );
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Right);
        app.update();
        assert!(
            app.world()
                .resource::<MapEditorNumericScrubMouseGuard>()
                .blocked()
        );
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.update();
        assert!(
            !app.world()
                .resource::<MapEditorNumericScrubMouseGuard>()
                .blocked()
        );
    }
}

#[test]
fn editor_layout_buttons_acquire_mouse_without_changing_editor_or_workspace() {
    let mut app = layout_app(1400, 1.0);
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = PresetWorkspaceTab::Domain("test.author", 17);
    app.world_mut().spawn((
        MapEditorLayoutButton(EditorLayoutAction::Focus),
        feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,
    ));
    app.update();
    assert!(
        app.world()
            .resource::<MapEditorLayoutState>()
            .focus_restore
            .is_some()
    );
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    assert!(app.world().resource::<MapEditorState>().open);
    assert!(!app.world().resource::<MapEditorState>().close_requested);
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Domain("test.author", 17)
    );
}

#[test]
fn editor_layout_does_not_start_splitter_during_a_modal_or_another_mouse_owner() {
    for modal in [false, true] {
        let mut app = layout_app(1400, 1.0);
        cursor(&mut app, 400.0);
        app.world_mut().spawn((
            MapEditorPaneSplitter(EditorPane::Content),
            UiInteraction::Pressed,
        ));
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        if modal {
            app.world_mut()
                .resource_mut::<MapEditorState>()
                .new_map_dialog_open = true;
        } else {
            app.world_mut()
                .resource_mut::<MapEditorNumericScrubMouseGuard>()
                .advance(true, true, false);
        }
        app.update();
        let state = app.world().resource::<MapEditorLayoutState>();
        assert!(state.drag.is_none());
        assert_eq!(state.content_width, feathrise_editor_core::EditorLayoutPreferences::DEFAULT_CONTENT_WIDTH);
    }
}

#[test]
fn editor_layout_restores_before_the_first_input_and_survives_restart() {
    let mut app = layout_app(1400, 1.0);
    let saved = feathrise_editor_core::EditorLayoutPreferences {
        content_width: 312.0, inspector_width: 440.0,
        content_visible: false, inspector_visible: true,
    };
    app.world_mut().resource_mut::<EditorSettings>().editor_layout = saved;
    // 第一帧就点击展开左栏；启动恢复必须先于这次显式输入。
    app.world_mut().spawn((MapEditorLayoutButton(EditorLayoutAction::Content), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    app.update();
    let expected = feathrise_editor_core::EditorLayoutPreferences { content_visible: true, ..saved };
    assert_eq!(app.world().resource::<MapEditorLayoutState>().preferences(), expected);
    assert_eq!(app.world().resource::<EditorSettings>().editor_layout, expected);
    assert!(!app.world().resource::<MapEditorState>().save_requested);

    let mut restarted = layout_app(1400, 1.0);
    restarted.world_mut().resource_mut::<EditorSettings>().editor_layout = expected;
    restarted.update();
    assert_eq!(restarted.world().resource::<MapEditorLayoutState>().preferences(), expected);
    assert!(restarted.world().resource::<MapEditorLayoutState>().focus_restore.is_none());
}

#[test]
fn editor_layout_focus_only_persists_explicit_exit_and_not_window_fit() {
    let mut app = layout_app(1400, 1.0);
    let saved = feathrise_editor_core::EditorLayoutPreferences {
        content_width: 312.0, inspector_width: 440.0, ..default()
    };
    app.world_mut().resource_mut::<EditorSettings>().editor_layout = saved;
    app.update();
    app.world_mut().resource_mut::<MapEditorLayoutState>().toggle(EditorLayoutAction::Content);
    app.update();
    let explicit = feathrise_editor_core::EditorLayoutPreferences { content_visible: false, ..saved };
    assert_eq!(app.world().resource::<EditorSettings>().editor_layout, explicit);
    let changed = app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed();
    app.world_mut().resource_mut::<MapEditorLayoutState>().toggle(EditorLayoutAction::Focus);
    app.update();
    assert_eq!(app.world().resource::<EditorSettings>().editor_layout, explicit);
    assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), changed);
    app.world_mut().resource_mut::<MapEditorLayoutState>().toggle(EditorLayoutAction::Focus);
    app.update();
    assert_eq!(app.world().resource::<EditorSettings>().editor_layout, saved);
    let changed = app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed();
    for (width, scale) in [(600, 1.0), (1400, 2.0), (1400, 1.0)] {
        let world = app.world_mut();
        let mut windows = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
        windows.single_mut(world).unwrap().resolution.set(width as f32, 1080.0);
        world.resource_mut::<UiScale>().0 = scale;
        app.update();
        assert_eq!(app.world().resource::<EditorSettings>().editor_layout, saved);
        assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), changed);
    }
    let state = app.world().resource::<MapEditorLayoutState>();
    assert!(state.content_visible);
    assert!(state.inspector_shown());
    assert_eq!(state.inspector_width, 440.0);
    // 专注期间退出程序也应保存进入前的混合显隐，下次启动不进入专注模式。
    app.world_mut().resource_mut::<MapEditorLayoutState>().toggle(EditorLayoutAction::Content);
    app.update();
    app.world_mut().resource_mut::<MapEditorLayoutState>().toggle(EditorLayoutAction::Focus);
    app.update();
    let mut restarted = layout_app(1400, 1.0);
    restarted.world_mut().resource_mut::<EditorSettings>().editor_layout = app.world().resource::<EditorSettings>().editor_layout;
    restarted.update();
    let state = restarted.world().resource::<MapEditorLayoutState>();
    assert!(!state.content_visible && state.inspector_visible);
    assert!(state.focus_restore.is_none());
}

#[test]
fn editor_layout_stable_frames_and_editor_reopening_do_not_request_settings_saves() {
    let mut app = layout_app(1400, 1.0);
    app.update();
    let changed = app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed();
    for open in [true, false, true, true] {
        app.world_mut().resource_mut::<MapEditorState>().open = open;
        app.update();
        assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), changed);
    }
}

#[test]
fn editor_layout_clicking_a_compressed_splitter_does_not_replace_preferred_width() {
    for pane in [EditorPane::Content, EditorPane::Inspector] {
        let mut app = layout_app(910, 1.0);
        let preferred = feathrise_editor_core::EditorLayoutPreferences {
            content_width: 312.0, inspector_width: 440.0, ..default()
        };
        app.world_mut().resource_mut::<EditorSettings>().editor_layout = preferred;
        app.update();
        let changed = app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed();
        app.world_mut().spawn((MapEditorPaneSplitter(pane), UiInteraction::Hovered));
        cursor(&mut app, 400.0);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
        app.update(); // 按住但不移动，不能把200/338px临时宽度当成作者选择。
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<MapEditorLayoutState>().drag.is_none());
        assert_eq!(app.world().resource::<MapEditorLayoutState>().preferences(), preferred);
        assert_eq!(app.world().resource::<EditorSettings>().editor_layout, preferred);
        assert_eq!(app.world().get_resource_ref::<EditorSettings>().unwrap().last_changed(), changed);
    }
}

#[test]
fn merged_header_keeps_title_commands_modes_and_launch_on_one_line_at_real_scales() {
    use bevy::camera::RenderTargetInfo;
    use crate::components::{MapEditorActionButton, MapEditorSettingsButton};
    use crate::types::{EditorAction, EditorSettingsAction};
    // 复用正式中文字体与Bevy布局夹具；只新增顶栏，不运行创建、保存、切图或文件事务。
    let (mut app, window) = crate::ui::terrain_objects::page_tests::page();
    app.init_resource::<MapEditorLayoutState>()
        .init_resource::<VegetationPresetEditorState>()
        .add_systems(Update, sync_editor_layout);
    let font = {
        let world = app.world_mut();
        world.query::<&TextFont>().iter(world).next().unwrap().font.clone()
    };
    let header = app.world_mut().spawn_scene(crate::ui::scene::header()).unwrap().id();
    {
        let mut node = app.world_mut().get_mut::<Node>(header).unwrap();
        node.position_type = PositionType::Absolute;
        node.left = px(0);
        node.top = px(0);
    }
    let bounds = |world: &World, entity| Rect::from_center_size(
        world.get::<UiGlobalTransform>(entity).unwrap().translation,
        world.get::<ComputedNode>(entity).unwrap().size());
    for (width, scale, dpi) in [(1280, 1.0, 1.0), (1280, 1.5, 1.0),
        (1600, 1.0, 1.0), (1920, 1.25, 1.0), (1280, 1.5, 2.0)] {
        app.world_mut().get_mut::<Window>(window).unwrap().resolution.set(width as f32, 720.0);
        app.world_mut().resource_mut::<UiScale>().0 = scale;
        let physical_size = UVec2::new((width as f32 * dpi) as u32, (720.0 * dpi) as u32);
        {
            let world = app.world_mut();
            for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                camera.viewport.as_mut().unwrap().physical_size = physical_size;
                camera.computed.target_info = Some(RenderTargetInfo { physical_size, scale_factor: dpi });
            }
            for mut text_font in world.query::<&mut TextFont>().iter_mut(world) { text_font.font = font.clone(); }
        }
        for focused in [false, true] {
            app.world_mut().resource_mut::<MapEditorLayoutState>().focus_restore = focused.then_some((true, true));
            for _ in 0..5 { app.update(); }
            let world = app.world_mut();
            let header_bounds = bounds(world, header);
            assert!((header_bounds.height() - 28.0 * scale * dpi).abs() <= 1.1);
            let children = world.get::<Children>(header).unwrap().to_vec();
            // 正式顶栏槽位包装原按钮；按原子节点顺序深度优先查找，不把包装层误判为功能缺失。
            let mut descendants = Vec::new();
            let mut pending: Vec<_> = children.iter().rev().copied().collect();
            while let Some(entity) = pending.pop() {
                descendants.push(entity);
                if let Some(children) = world.get::<Children>(entity) { pending.extend(children.iter().rev()); }
            }
            let mode_bar = *descendants.iter().find(|entity| world.get::<crate::viewport::modes::ViewportModeMenuRoot>(**entity).is_some())
                .expect("筛选菜单入口必须保留在单行顶栏");
            let mode_bounds = bounds(world, mode_bar);
            assert!((mode_bounds.center().y - header_bounds.center().y).abs() <= 1.1);
            assert_eq!(world.get::<Children>(mode_bar).unwrap().len(), 1); // 五种模式迁到三维视口左侧，顶栏仅保留菜单。
            for entity in &children {
                let rect = bounds(world, *entity);
                assert!(rect.min.x >= header_bounds.min.x - 1.1 && rect.max.x <= header_bounds.max.x + 1.1,
                    "顶栏不得横向溢出 {width}/{scale}/{dpi}: {rect:?}/{header_bounds:?}");
            }
            let actions: Vec<_> = descendants.iter().filter_map(|entity| world.get::<MapEditorActionButton>(*entity).map(|button| button.0)).collect();
            assert_eq!(actions, vec![EditorAction::Save, EditorAction::Undo, EditorAction::Redo, EditorAction::Close]);
            assert!(descendants.iter().any(|entity| world.get::<MapEditorSettingsButton>(*entity)
                .is_some_and(|button| button.0 == EditorSettingsAction::Open)));
            for entity in descendants {
                if let Some(text) = world.get::<Text>(entity) {
                    let rect = bounds(world, entity);
                    assert!(rect.width() > 0.0 && rect.height() > 0.0, "实际字形布局不能为空：{}", text.as_str());
                    assert!(rect.min.x >= header_bounds.min.x - 1.1 && rect.max.x <= header_bounds.max.x + 1.1,
                        "顶栏文字不得挤出窗口：{} / {width}/{scale}/{dpi}", text.as_str());
                    let parent = world.get::<ChildOf>(entity).unwrap().parent();
                    if world.get::<InteractionButton>(parent).is_some() {
                        let button = bounds(world, parent);
                        assert!(rect.min.x >= button.min.x - 1.1 && rect.max.x <= button.max.x + 1.1,
                            "短标签仍需完整显示：{} / {width}/{scale}/{dpi}", text.as_str());
                        assert!((button.height() - 26.0 * scale * dpi).abs() <= 1.1);
                    }
                }
            }
        }
    }
    assert!(!app.world().resource::<MapEditorState>().dirty);
}
