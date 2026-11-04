//! 正式文档工作区的真实GPU交互验证；鼠标命中诊断/格式化，原生快捷键保存和撤销。

use super::super::{components::*, history, resources::DocumentState, types::ViewMode};
use super::{components::JsonAction, control, validation};
use bevy::{
    ecs::system::RunSystemOnce,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, TextEdit},
    ui::widget::TextScroll,
};
use std::{path::Path, time::Duration};

pub(in super::super) fn gpu_assert_tools(
    app: &mut App,
    window: Entity,
    camera: Entity,
    source: Entity,
    root: &Path,
    path: &str,
    mut snapshot: impl FnMut(&mut App, &str),
) {
    let disk_before = std::fs::read(root.join(path)).unwrap();
    let rows: String = (0..240).map(|index| format!("    {index},\n")).collect();
    let bad = format!(
        "{{\n  \"中文\": \"保留\\u4e00🙂\",\n  \"数字\": 1.2300e+04,\n  \"重复\": 1,\n  \"重复\": 2,\n  \"rows\": [\n{rows}    999\n  ],\n  \"错误\": true \"最后\": false\n}}\n"
    );
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(source, FocusCause::Navigated);
    {
        let mut text = app.world_mut().get_mut::<EditableText>(source).unwrap();
        text.queue_edit(TextEdit::SelectAll);
        text.queue_edit(TextEdit::Insert(bad.clone().into()));
    }
    settle(app);
    let error = validation::validate(&bad).unwrap();
    assert!(error.line > 240);
    {
        let world = app.world_mut();
        let (label, node, feedback) = world
            .query_filtered::<(&Text, &ComputedNode, &feathrise_editor_ui::PanelStatusText), With<super::components::DiagnosticLabel>>()
            .single(world)
            .unwrap();
        assert!(
            label.contains(&format!("JSON第{}行", error.line)) && node.size() == Vec2::ZERO && feedback.0 == "JSON",
            "诊断全文进入底栏，正文不占高度：{label:?}, size={:?}",
            node.size()
        );
        let locate = world.query::<(&JsonAction, &ComputedNode)>().iter(world)
            .find(|(action, _)| **action == JsonAction::Locate).unwrap();
        assert!(locate.1.size().min_element() > 1.0, "真实错误定位入口仍须可点击");
    }
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        bad.as_str()
    );
    chord(app, window, KeyCode::KeyS, "s");
    settle(app);
    assert_eq!(
        std::fs::read(root.join(path)).unwrap(),
        disk_before,
        "有语法错误时Ctrl+S不得写盘"
    );
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .status
            .contains("保存已停止"),
        "保存反馈：{}",
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .status
    );
    // 先浏览首行，再从预览点击诊断：必须主动跳到屏幕之外的真实错误，而非刚好已在末尾。
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::TextStart(false));
    frames(app, 4);
    press_document(app, DocumentAction::Preview);
    frames(app, 5);
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .mode,
        ViewMode::Preview
    );
    click(app, window, camera, JsonAction::Locate);
    settle(app);
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .mode,
        ViewMode::Edit
    );
    assert_eq!(
        app.world()
            .get::<EditableText>(source)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        error.selection
    );
    let layout = app
        .world()
        .get::<bevy::text::TextLayoutInfo>(source)
        .unwrap();
    let node = app.world().get::<ComputedNode>(source).unwrap();
    let scroll = app.world().get::<TextScroll>(source).unwrap().0.y;
    let bounds = layout
        .selection_rects
        .iter()
        .copied()
        .reduce(|a, b| Rect {
            min: a.min.min(b.min),
            max: a.max.max(b.max),
        })
        .unwrap();
    assert!(
        scroll > 100.0
            && bounds.min.y >= scroll
            && bounds.max.y <= scroll + node.content_box().height(),
        "错误定位要露出完整选区：scroll={scroll}, bounds={bounds:?}"
    );
    snapshot(app, "error");
    click(app, window, camera, JsonAction::Format);
    settle(app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        bad.as_str()
    );
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .status
            .contains("无法格式化")
    );
    let mut corrected = bad.clone();
    corrected.insert(error.selection.start, ',');
    {
        let mut text = app.world_mut().get_mut::<EditableText>(source).unwrap();
        text.queue_edit(TextEdit::Left(false));
        text.queue_edit(TextEdit::Insert(",".into()));
    }
    settle(app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        corrected.as_str()
    );
    assert!(validation::validate(&corrected).is_none());
    click(app, window, camera, JsonAction::Format);
    settle(app);
    let formatted = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .source
        .clone();
    assert_ne!(formatted.as_ref(), corrected);
    assert!(formatted.contains("1.2300e+04"));
    assert_eq!(formatted.matches("\"重复\":").count(), 2);
    assert!(formatted.contains("保留\\u4e00🙂"));
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        formatted.as_ref()
    );
    assert_eq!(
        std::fs::read(root.join(path)).unwrap(),
        disk_before,
        "格式化本身不保存文件"
    );
    snapshot(app, "formatted");
    history::gpu_chord(app, window, KeyCode::KeyZ, false);
    settle(app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        corrected.as_str(),
        "一次撤销恢复格式化前全文"
    );
    history::gpu_chord(app, window, KeyCode::KeyY, false);
    settle(app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        formatted.as_ref()
    );
    let revision = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .revision;
    click(app, window, camera, JsonAction::Format);
    settle(app);
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .revision,
        revision,
        "已格式化文件不新增历史"
    );
    chord(app, window, KeyCode::KeyS, "s");
    settle(app);
    assert_eq!(
        std::fs::read_to_string(root.join(path)).unwrap(),
        formatted.as_ref()
    );
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<DocumentSource>>()
            .single(app.world())
            .unwrap(),
        source
    );
    snapshot(app, "saved");
}

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
        std::thread::sleep(Duration::from_millis(16));
    }
}

fn settle(app: &mut App) {
    for _ in 0..120 {
        frames(app, 1);
        let state = app.world().resource::<DocumentState>();
        if state.json.task.is_none()
            && state.json.ready.is_none()
            && state.json.format.is_none()
            && state.json.locate.is_none()
            && state.json.save.is_none()
            && state.io.is_none()
            && state
                .current()
                .is_some_and(|doc| state.json.checked(doc).is_some())
        {
            frames(app, 3); // 正式PostUpdate排版/滚动与渲染提取使用完成值。
            return;
        }
    }
    panic!("GPU JSON工具未完成");
}

fn press_document(app: &mut App, action: DocumentAction) {
    let entity = app
        .world_mut()
        .query::<(Entity, &DocumentAction)>()
        .iter(app.world())
        .find(|(_, candidate)| **candidate == action)
        .unwrap()
        .0;
    *app.world_mut().get_mut::<Interaction>(entity).unwrap() = Interaction::Pressed;
    app.world_mut().entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.world_mut()
        .run_system_once(super::super::interaction::actions)
        .unwrap();
    app.world_mut().get_mut::<feathrise_editor_ui::EditorButtonClick>(entity).unwrap().invalidate();
    *app.world_mut().get_mut::<Interaction>(entity).unwrap() = Interaction::None;
}

fn click(app: &mut App, window: Entity, camera: Entity, action: JsonAction) {
    let (entity, position) = {
        let world = app.world_mut();
        world
            .query::<(Entity, &JsonAction, &UiGlobalTransform)>()
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
        *app.world().get::<Interaction>(entity).unwrap(),
        Interaction::Pressed,
        "真实鼠标必须命中JSON按钮"
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
    app.world_mut().run_system_once(control::input).unwrap();
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = target;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    frames(app, 1);
}

fn chord(app: &mut App, window: Entity, code: KeyCode, letter: &str) {
    // 通用夹具此前直接注入Ctrl+S边沿，没有模拟松键；新键盘消息先恢复完整释放状态。
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release_all();
    keys.clear();
    let chord = [
        (KeyCode::ControlLeft, Key::Control),
        (code, Key::Character(letter.into())),
    ];
    for (code, key) in &chord {
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
    for (code, key) in chord {
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
