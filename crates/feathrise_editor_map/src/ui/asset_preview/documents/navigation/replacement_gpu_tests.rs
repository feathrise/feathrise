//! 正式文档工作区的离屏GPU回归；真实键鼠/IME验证替换、范围、撤销与受检保存。

use feathrise_editor_ui::UiInteraction;
use super::super::{components::DocumentSource, resources::DocumentState};
use super::{components::*, control, index, resources::DocumentNavigation, types::NavigationMode};
use bevy::{
    ecs::system::RunSystemOnce,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, FontCx, LayoutCx, TextEdit},
};
use std::{path::Path, time::Duration};

pub(in super::super) fn gpu_assert_replacement(
    app: &mut App,
    window: Entity,
    camera: Entity,
    source: Entity,
    root: &Path,
    path: &str,
    mut snapshot: impl FnMut(&mut App, &str),
) {
    let before = app
        .world()
        .get::<EditableText>(source)
        .unwrap()
        .value()
        .to_string();
    let disk_before = std::fs::read(root.join(path)).unwrap();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(source, FocusCause::Navigated);
    chord(app, window, KeyCode::KeyH, true);
    settle(app);
    assert_eq!(
        app.world().resource::<DocumentNavigation>().mode,
        NavigationMode::Replace
    );
    let query = field(app, NavigationField::Query);
    let replacement = field(app, NavigationField::Replacement);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(query));
    set_field(app, query, "weapon");
    assert_eq!(matches(app), 2);
    click(app, window, camera, NavigationAction::WholeWord);
    settle(app);
    assert_eq!(matches(app), 1);
    click(app, window, camera, NavigationAction::CaseSensitive);
    settle(app);
    assert_eq!(matches(app), 2);
    set_field(app, replacement, "item");
    assert_layout(app, window, replacement);
    snapshot(app, "options");
    click(app, window, camera, NavigationAction::ReplaceAll);
    settle(app);
    let changed = before
        .replace("\"weapon\"", "\"item\"")
        .replace("\"Weapon\"", "\"item\"");
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        changed.as_str()
    );
    assert!(changed.contains("weapon_extra"));
    assert_eq!(std::fs::read(root.join(path)).unwrap(), disk_before);
    chord(app, window, KeyCode::Escape, false);
    chord(app, window, KeyCode::KeyZ, true);
    settle(app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        before.as_str()
    );
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    chord(app, window, KeyCode::KeyY, true);
    settle(app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        changed.as_str()
    );
    chord(app, window, KeyCode::KeyZ, true);
    settle(app);

    chord(app, window, KeyCode::KeyH, true);
    frames(app, 3);
    // 真实候选消息及Enter：查询/替换输入尚未确认时，不能穿透为查找或写正文。
    app.world_mut().write_message(bevy::window::Ime::Preedit {
        window,
        value: "旧".into(),
        cursor: Some((3, 3)),
    });
    frames(app, 2);
    chord(app, window, KeyCode::Enter, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        before.as_str()
    );
    app.world_mut().write_message(bevy::window::Ime::Commit {
        window,
        value: "旧".into(),
    });
    settle(app);
    assert_eq!(matches(app), 3);
    let start = before.find("旧 旧 旧").unwrap();
    select(app, source, start + "旧 旧".len(), start);
    click(app, window, camera, NavigationAction::InSelection);
    settle(app);
    assert_eq!(matches(app), 2);
    let scope = app
        .world()
        .resource::<DocumentNavigation>()
        .scope
        .as_ref()
        .unwrap()
        .range
        .clone();
    click(app, window, camera, NavigationAction::Next);
    settle(app);
    assert_eq!(
        app.world()
            .resource::<DocumentNavigation>()
            .scope
            .as_ref()
            .unwrap()
            .range,
        scope
    );
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(replacement, FocusCause::Navigated);
    app.world_mut()
        .get_mut::<EditableText>(replacement)
        .unwrap()
        .queue_edit(TextEdit::SelectAll);
    frames(app, 2);
    app.world_mut().write_message(bevy::window::Ime::Preedit {
        window,
        value: "新名字".into(),
        cursor: Some((9, 9)),
    });
    frames(app, 2);
    chord(app, window, KeyCode::Enter, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        before.as_str()
    );
    app.world_mut().write_message(bevy::window::Ime::Commit {
        window,
        value: "新名字".into(),
    });
    settle(app);
    assert_eq!(
        app.world().resource::<DocumentNavigation>().replacement,
        "新名字"
    );
    snapshot(app, "selection");
    click(app, window, camera, NavigationAction::ReplaceCurrent);
    settle(app);
    let once = before.replacen("旧", "新名字", 1);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        once.as_str()
    );
    assert_eq!(matches(app), 1);
    click(app, window, camera, NavigationAction::ReplaceAll);
    settle(app);
    let saved = before.replacen("旧 旧 旧", "新名字 新名字 旧", 1);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        saved.as_str()
    );
    assert_eq!(std::fs::read(root.join(path)).unwrap(), disk_before);
    assert_eq!(matches(app), 0);
    assert!(app.world().resource::<DocumentNavigation>().in_selection);
    snapshot(app, "replaced");
    chord(app, window, KeyCode::Escape, false);
    chord(app, window, KeyCode::KeyZ, true);
    settle(app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        once.as_str()
    );
    chord(app, window, KeyCode::KeyZ, true);
    settle(app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        before.as_str()
    );
    chord(app, window, KeyCode::KeyY, true);
    chord(app, window, KeyCode::KeyY, true);
    settle(app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        saved.as_str()
    );
    chord(app, window, KeyCode::KeyS, true);
    settle(app);
    assert_eq!(std::fs::read_to_string(root.join(path)).unwrap(), saved);
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );

    // 替换可以产生临时不合法JSON；直到显式保存仍必须由原语法门保护磁盘。
    chord(app, window, KeyCode::KeyH, true);
    settle(app);
    click(app, window, camera, NavigationAction::InSelection);
    set_field(app, query, "长剑");
    set_field(app, replacement, "\"");
    assert_eq!(matches(app), 3);
    click(app, window, camera, NavigationAction::ReplaceAll);
    settle(app);
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    chord(app, window, KeyCode::Escape, false);
    chord(app, window, KeyCode::KeyS, true);
    settle(app);
    assert_eq!(std::fs::read_to_string(root.join(path)).unwrap(), saved);
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .status
            .contains("保存已停止")
    );
    snapshot(app, "invalid-save");
    chord(app, window, KeyCode::KeyZ, true);
    settle(app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        saved.as_str()
    );
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<DocumentSource>>()
            .iter(world)
            .count(),
        1
    );
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn settle(app: &mut App) {
    for _ in 0..150 {
        frames(app, 1);
        let state = app.world().resource::<DocumentNavigation>();
        let documents = app.world().resource::<DocumentState>();
        if state.task.is_none()
            && state.replace.is_none()
            && state.ready.is_none()
            && state.index.as_ref().is_some_and(|found| {
                documents
                    .current()
                    .is_some_and(|doc| index::current(&found.key, doc, state))
            })
            && super::super::json::gpu_tools_idle(documents)
        {
            frames(app, 3);
            return;
        }
    }
    panic!("GPU查找替换尚未完成");
}

fn field(app: &mut App, kind: NavigationField) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &NavigationField)>()
        .iter(world)
        .find(|(_, field)| **field == kind)
        .unwrap()
        .0
}

fn set_field(app: &mut App, entity: Entity, value: &str) {
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(entity, FocusCause::Navigated);
    let mut text = app.world_mut().get_mut::<EditableText>(entity).unwrap();
    text.queue_edit(TextEdit::SelectAll);
    text.queue_edit(TextEdit::Insert(value.into()));
    frames(app, 3);
    settle(app);
}

fn matches(app: &App) -> usize {
    app.world()
        .resource::<DocumentNavigation>()
        .index
        .as_ref()
        .unwrap()
        .matches
        .len()
}

fn select(app: &mut App, source: Entity, anchor: usize, focus: usize) {
    let mut font = app.world_mut().remove_resource::<FontCx>().unwrap();
    let mut layout = app.world_mut().remove_resource::<LayoutCx>().unwrap();
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .editor
        .driver(&mut font.context, &mut layout.0)
        .select_byte_range(anchor, focus);
    app.insert_resource(font).insert_resource(layout);
    frames(app, 2);
}

fn assert_layout(app: &mut App, window: Entity, replacement: Entity) {
    let width = app.world().get::<Window>(window).unwrap().width();
    assert!(
        app.world()
            .get::<ComputedNode>(replacement)
            .unwrap()
            .size()
            .x
            > 10.0
    );
    let world = app.world_mut();
    for (action, node, transform) in world
        .query::<(&NavigationAction, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
    {
        if matches!(
            action,
            NavigationAction::ReplaceCurrent
                | NavigationAction::ReplaceAll
                | NavigationAction::CaseSensitive
                | NavigationAction::WholeWord
                | NavigationAction::InSelection
        ) {
            let position = transform.affine().translation;
            assert!(node.size().min_element() > 1.0);
            assert!(
                position.x - node.size().x / 2.0 >= -0.1
                    && position.x + node.size().x / 2.0 <= width + 0.1,
                "替换工具必须完整位于视区：位置{position:?}，尺寸{:?}，宽度{width}",
                node.size()
            );
        }
    }
}

fn click(app: &mut App, window: Entity, camera: Entity, action: NavigationAction) {
    let (entity, position) = {
        let world = app.world_mut();
        world
            .query::<(Entity, &NavigationAction, &UiGlobalTransform)>()
            .iter(world)
            .find(|(_, candidate, _)| **candidate == action)
            .map(|(entity, _, transform)| (entity, transform.affine().translation))
            .unwrap()
    };
    let target = app
        .world()
        .get::<bevy::camera::RenderTarget>(camera)
        .unwrap()
        .clone();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(position.as_dvec2()));
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    assert_eq!(
        *app.world().get::<UiInteraction>(entity).unwrap(),
        UiInteraction::Pressed,
        "真实鼠标必须命中替换工具"
    );
    app.world_mut().run_system_once(control::input).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    app.world_mut().run_system_once(super::control::input).unwrap();
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = target;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    frames(app, 1);
}

fn chord(app: &mut App, window: Entity, code: KeyCode, ctrl: bool) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release_all();
    keys.clear(); // 通用夹具曾直接注入边沿；本段使用完整按下/松开消息。
    let logical = match code {
        KeyCode::Escape => Key::Escape,
        KeyCode::Enter => Key::Enter,
        KeyCode::KeyH => Key::Character("h".into()),
        KeyCode::KeyS => Key::Character("s".into()),
        KeyCode::KeyY => Key::Character("y".into()),
        _ => Key::Character("z".into()),
    };
    let mut events = Vec::new();
    if ctrl {
        events.push((KeyCode::ControlLeft, Key::Control));
    }
    events.push((code, logical));
    for (code, key) in &events {
        app.world_mut().write_message(KeyboardInput {
            key_code: *code,
            logical_key: key.clone(),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
    }
    app.update();
    for (code, key) in events {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: key,
            state: ButtonState::Released,
            text: None,
            repeat: false,
            window,
        });
    }
    app.update();
}
