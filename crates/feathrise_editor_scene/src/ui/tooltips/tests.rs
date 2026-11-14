//! 悬停延迟、真实键位、原状态只读和禁用条件回归；不运行动作或写入作者文件。

use feathrise_editor_ui::UiInteraction;
use super::components::*;
use super::helpers::*;
use super::resources::EditorTooltipState;
use super::systems::sync_tooltip;
use super::types::*;
use crate::authoring::EditorAuthoringSession;
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorSettingsState, SceneEditorState};
use crate::types::EditorAction;
use crate::ui::action_availability::EditorActionAvailability;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorKeyBinding;
use feathrise_editor_core::EditorShortcutAction;
use feathrise_editor_ui::localization::EditorLocale;
use std::time::Duration;

fn app() -> (App, Entity, Entity) {
    let mut app = App::new();
    let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    app.init_resource::<Time<Real>>()
        .init_resource::<EditorTooltipState>()
        .init_resource::<EditorSettings>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<SceneEditorSettingsState>()
        // 中文文案断言使用明确的语言，与系统语言无关。
        .insert_resource(EditorLocale::Chinese)
        .insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active))
        .insert_resource(feathrise_scene_content::ActiveDynamicScene { scene_id: Some(draft.scene_id.clone()) })
        .insert_resource(SceneEditorState {
            open: true,
            selected_scene_id: Some(draft.scene_id.clone()),
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            ..default()
        })
        .add_systems(Update, sync_tooltip);
    let mut window = Window {
        focused: true,
        ..default()
    };
    window.set_physical_cursor_position(Some(Vec2::new(20.0, 20.0).as_dvec2()));
    app.world_mut().spawn((window, PrimaryWindow));
    let popup = app
        .world_mut()
        .spawn((
            TooltipPopup,
            Node {
                display: Display::None,
                ..default()
            },
            Visibility::Hidden,
        ))
        .id();
    for kind in [
        TooltipText::Title,
        TooltipText::Description,
        TooltipText::Reason,
    ] {
        app.world_mut()
            .spawn((kind, Text::default(), Node::default(), ChildOf(popup)));
    }
    let target = app
        .world_mut()
        .spawn((
            EditorTooltipTarget(TooltipAction::Editor(EditorAction::Undo)),
            UiInteraction::Hovered,
            Node::default(),
            ComputedNode {
                size: Vec2::new(80.0, 26.0),
                ..default()
            },
            InheritedVisibility::VISIBLE,
        ))
        .id();
    (app, popup, target)
}

fn advance(app: &mut App, millis: u64) {
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(millis));
    app.update();
}

fn text(app: &mut App, kind: TooltipText) -> String {
    let world = app.world_mut();
    world
        .query::<(&TooltipText, &Text)>()
        .iter(world)
        .find(|(found, _)| std::mem::discriminant(*found) == std::mem::discriminant(&kind))
        .unwrap()
        .1
        .as_str()
        .to_owned()
}

#[test]
fn tooltip_keys_follow_custom_binding_and_unbound_without_falling_back_to_defaults() {
    let mut settings = EditorSettings::default();
    let locale = EditorLocale::Chinese;
    let copy = tooltip_copy(TooltipAction::Editor(EditorAction::FocusSelected), false);
    assert_eq!(tooltip_title(&copy, &settings, locale), "聚焦所选 · F");
    let mut input = ButtonInput::default();
    input.press(KeyCode::ControlLeft);
    input.press(KeyCode::ShiftLeft);
    settings
        .editor_keybindings
        .assign(
            EditorShortcutAction::FocusSelected,
            Some(EditorKeyBinding::from_input(KeyCode::KeyG, &input)),
        )
        .unwrap();
    assert_eq!(
        tooltip_title(&copy, &settings, locale),
        "聚焦所选 · Ctrl+Shift+G"
    );
    settings
        .editor_keybindings
        .assign(EditorShortcutAction::FocusSelected, None)
        .unwrap();
    assert_eq!(tooltip_title(&copy, &settings, locale), "聚焦所选 · 未绑定");
    assert_eq!(
        tooltip_title(&copy, &settings, EditorLocale::English),
        "Frame Selected · Unassigned"
    );
    assert_eq!(
        tooltip_title(
            &tooltip_copy(TooltipAction::Editor(EditorAction::Close), false),
            &settings,
            locale,
        ),
        "启动"
    );
}

#[test]
fn viewport_mode_tooltips_use_real_bindings_and_never_invent_select_or_edit_shortcuts() {
    use crate::viewport::modes::ViewportMode;
    let mut settings = EditorSettings::default();
    let locale = EditorLocale::Chinese;
    for (mode, action, label, default_key) in [
        (ViewportMode::Move, EditorShortcutAction::MoveSelected, "移动", "W"),
        (ViewportMode::Rotate, EditorShortcutAction::RotateSelected, "旋转", "E"),
        (ViewportMode::Scale, EditorShortcutAction::ScaleSelected, "缩放", "R"),
    ] {
        let copy = tooltip_copy(TooltipAction::ViewportMode(mode), false);
        assert_eq!(
            tooltip_title(&copy, &settings, locale),
            format!("{label} · {default_key}")
        );
        let mut modifiers = ButtonInput::default();
        modifiers.press(KeyCode::ControlLeft);
        modifiers.press(KeyCode::ShiftLeft);
        settings.editor_keybindings.assign(
            action, Some(EditorKeyBinding::from_input(KeyCode::KeyG, &modifiers)),
        ).unwrap();
        assert_eq!(
            tooltip_title(&copy, &settings, locale),
            format!("{label} · Ctrl+Shift+G")
        );
        settings.editor_keybindings.assign(action, None).unwrap();
        assert_eq!(
            tooltip_title(&copy, &settings, locale),
            format!("{label} · 未绑定")
        );
        assert!(!copy.description.is_empty());
    } // 同一设置项被重新绑定或禁用后，图标说明必须立即采用实际按键。
    for (action, title) in [
        (TooltipAction::ViewportMode(ViewportMode::Select), "选择"),
        (TooltipAction::ViewportMode(ViewportMode::Edit), "编辑"),
        (TooltipAction::ViewportOptions, "筛选与操作"),
    ] {
        let copy = tooltip_copy(action, false);
        assert_eq!(tooltip_title(&copy, &settings, locale), title);
        assert!(copy.shortcut.is_none() && copy.fixed_key.is_none());
        assert!(!copy.description.is_empty());
    }
}

#[test]
fn mesh_edit_tooltips_describe_element_tools_and_their_own_shortcuts() {
    use crate::viewport::modes::ViewportMode;
    let settings = EditorSettings::default();
    let locale = EditorLocale::Chinese;
    for (mode, title) in [
        (ViewportMode::Select, "选择元素"),
        (ViewportMode::Move, "移动所选元素 · G"),
        (ViewportMode::Rotate, "旋转所选元素 · R"),
        (ViewportMode::Scale, "缩放所选元素 · S"),
    ] {
        let copy = tooltip_copy(TooltipAction::ViewportMode(mode), true);
        assert_eq!(tooltip_title(&copy, &settings, locale), title);
        assert!(copy.shortcut.is_none(), "元素工具不能回显对象变换的 W/E/R 配置");
        assert!(!copy.description.contains("所选对象"));
        if matches!(mode, ViewportMode::Rotate | ViewportMode::Scale) {
            assert!(copy.description.contains("所选元素中心"));
            assert!(copy.description.contains("单点"));
            assert!(copy.description.contains("XYZ 约束"));
            assert!(copy.description.contains("不改变位置"));
        }
    }
}

#[test]
fn scale_tooltip_changes_with_edit_mode_and_keeps_element_key_independent_of_object_binding() {
    use crate::authoring::mesh_edit::MeshEditState;
    use crate::viewport::modes::ViewportMode;
    let (mut app, _, target) = app();
    app.world_mut().get_mut::<EditorTooltipTarget>(target).unwrap().0 =
        TooltipAction::ViewportMode(ViewportMode::Scale);
    advance(&mut app, 0);
    advance(&mut app, 400);
    assert_eq!(text(&mut app, TooltipText::Title), "缩放 · R");

    let mut mesh_edit = MeshEditState::default();
    mesh_edit.activate_for_test();
    app.insert_resource(mesh_edit);
    advance(&mut app, 0);
    assert_eq!(text(&mut app, TooltipText::Title), "缩放所选元素 · S");
    assert!(text(&mut app, TooltipText::Description).contains("单点围绕自身中心缩放不改变位置"));

    // 同一悬停按钮无需移开鼠标；对象快捷键重绑也不能覆盖元素缩放的固定 S。
    let mut modifiers = ButtonInput::default();
    modifiers.press(KeyCode::ControlLeft);
    modifiers.press(KeyCode::ShiftLeft);
    app.world_mut().resource_mut::<EditorSettings>().editor_keybindings.assign(
        EditorShortcutAction::ScaleSelected,
        Some(EditorKeyBinding::from_input(KeyCode::KeyG, &modifiers)),
    ).unwrap();
    advance(&mut app, 0);
    assert_eq!(text(&mut app, TooltipText::Title), "缩放所选元素 · S");
    app.insert_resource(MeshEditState::default());
    advance(&mut app, 0);
    assert_eq!(text(&mut app, TooltipText::Title), "缩放 · Ctrl+Shift+G");
    assert!(text(&mut app, TooltipText::Description).contains("所选普通资产"));
}

#[test]
fn tooltip_reports_known_real_conditions_without_hiding_the_operation_description() {
    let undo = TooltipAction::Editor(EditorAction::Undo);
    let mut state = EditorActionAvailability::default();
    assert_eq!(unavailable_reason(undo, &state), Some("尚未进入就绪场景"));
    state.current_scene = true;
    state.undo_empty = true;
    assert_eq!(
        unavailable_reason(undo, &state),
        Some("当前场景没有可撤销操作")
    );
    state.saving = true;
    assert_eq!(unavailable_reason(undo, &state), Some("当前场景正在保存"));
    state.saving = false;
    let focus = TooltipAction::Editor(EditorAction::FocusSelected);
    assert_eq!(unavailable_reason(focus, &state), Some("场景视口尚未就绪"));
    state.viewport_ready = true;
    assert_eq!(
        unavailable_reason(focus, &state),
        Some("请先选择可聚焦的资产、水体或传送点")
    );
    state.focus_available = true;
    assert_eq!(unavailable_reason(focus, &state), None);
    let save = TooltipAction::Editor(EditorAction::Save);
    assert_eq!(unavailable_reason(save, &state), None);
    state.deletion_busy = true;
    assert_eq!(
        unavailable_reason(save, &state),
        Some("场景回收事务尚未完成")
    );
    assert_eq!(unavailable_reason(TooltipAction::Settings, &state), None);
    assert!(!tooltip_copy(focus, false).description.is_empty());
}

#[test]
fn tooltip_flips_above_and_keeps_actual_size_inside_narrow_scaled_roots() {
    for width in [213.0, 320.0, 430.0, 1280.0] {
        for height in [240.0, 360.0, 720.0] {
            let viewport = Vec2::new(width, height);
            let size = Vec2::new(280.0_f32.min(width * 0.95), 90.0);
            for anchor in [
                Rect::from_corners(Vec2::ZERO, Vec2::new(60.0, 26.0)),
                Rect::from_corners(viewport - Vec2::new(60.0, 26.0), viewport),
            ] {
                let position = tooltip_position(anchor, size, viewport);
                assert!(position.min_element() >= 0.0);
                assert!((position + size).cmple(viewport).all());
                if anchor.max.y == height {
                    assert!(position.y + size.y < anchor.min.y);
                }
            }
        }
    }
}

#[test]
fn tooltip_uses_real_time_delay_and_never_changes_settings_authoring_or_button_bounds() {
    let (mut app, popup, target) = app();
    let editor_tick = app
        .world()
        .get_resource_ref::<SceneEditorState>()
        .unwrap()
        .last_changed();
    let session_tick = app
        .world()
        .get_resource_ref::<EditorAuthoringSession>()
        .unwrap()
        .last_changed();
    let settings_tick = app
        .world()
        .get_resource_ref::<EditorSettings>()
        .unwrap()
        .last_changed();
    let original = app.world().get::<Node>(target).unwrap().clone();
    advance(&mut app, 0);
    advance(&mut app, 349);
    assert_eq!(
        app.world().get::<Node>(popup).unwrap().display,
        Display::None
    );
    advance(&mut app, 1);
    assert_eq!(
        app.world().get::<Node>(popup).unwrap().display,
        Display::Flex
    );
    assert_eq!(text(&mut app, TooltipText::Title), "撤销 · Ctrl+Z");
    assert!(text(&mut app, TooltipText::Reason).contains("没有可撤销"));
    advance(&mut app, 100);
    assert_eq!(app.world().get::<Node>(target).unwrap(), &original);
    assert_eq!(
        app.world()
            .get_resource_ref::<SceneEditorState>()
            .unwrap()
            .last_changed(),
        editor_tick
    );
    assert_eq!(
        app.world()
            .get_resource_ref::<EditorAuthoringSession>()
            .unwrap()
            .last_changed(),
        session_tick
    );
    assert_eq!(
        app.world()
            .get_resource_ref::<EditorSettings>()
            .unwrap()
            .last_changed(),
        settings_tick
    );
    let world = app.world_mut();
    assert_eq!(world.query::<&TooltipElement>().iter(world).count(), 4);
}

#[test]
fn tooltip_restarts_delay_on_target_change_and_updates_binding_while_visible() {
    let (mut app, popup, target) = app();
    advance(&mut app, 0);
    advance(&mut app, 400);
    let input = ButtonInput::default();
    app.world_mut()
        .resource_mut::<EditorSettings>()
        .editor_keybindings
        .assign(
            EditorShortcutAction::Undo,
            Some(EditorKeyBinding::from_input(KeyCode::KeyG, &input)),
        )
        .unwrap();
    advance(&mut app, 0);
    assert_eq!(text(&mut app, TooltipText::Title), "撤销 · G");
    app.world_mut()
        .get_mut::<UiInteraction>(target)
        .unwrap()
        .set_if_neq(UiInteraction::None);
    app.world_mut().spawn((
        EditorTooltipTarget(TooltipAction::Settings),
        UiInteraction::Hovered,
        Node::default(),
        ComputedNode {
            size: Vec2::splat(40.0),
            ..default()
        },
        InheritedVisibility::VISIBLE,
    ));
    advance(&mut app, 0);
    assert_eq!(
        app.world().get::<Node>(popup).unwrap().display,
        Display::None
    );
    advance(&mut app, 350);
    assert_eq!(text(&mut app, TooltipText::Title), "设置");
}

#[test]
fn tooltip_clears_on_mouse_chord_modal_focus_loss_hidden_target_and_scene_change() {
    for case in 0..8 {
        let (mut app, popup, target) = app();
        advance(&mut app, 0);
        advance(&mut app, 400);
        match case {
            0 => app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left),
            1 => app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Right),
            2 => {
                app.world_mut()
                    .resource_mut::<SceneEditorSettingsState>()
                    .open = true
            }
            3 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .new_scene_dialog_open = true
            }
            4 => app.world_mut().resource_mut::<SceneEditorState>().open = false,
            5 => {
                let world = app.world_mut();
                world
                    .query_filtered::<&mut Window, With<PrimaryWindow>>()
                    .single_mut(world)
                    .unwrap()
                    .focused = false;
            }
            6 => {
                app.world_mut()
                    .get_mut::<ComputedNode>(target)
                    .unwrap()
                    .size = Vec2::ZERO
            }
            7 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .selected_scene_id = None
            }
            _ => unreachable!(),
        }
        advance(&mut app, 0);
        assert_eq!(
            app.world().get::<Node>(popup).unwrap().display,
            Display::None,
            "case {case}"
        );
    }
}
