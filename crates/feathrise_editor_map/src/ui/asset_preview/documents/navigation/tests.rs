//! 检索字节身份、中文与尾空行、循环导航、迟到单槽及原生选区回归。

use super::super::{
    resources::DocumentState,
    types::{DiskIdentity, Document, SourceKey, ViewMode},
};
use super::{index, paint, resources::DocumentNavigation, types::*};
use bevy::{
    prelude::*,
    text::{EditableText, FontCx, LayoutCx},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub(super) fn key(query: &str) -> IndexKey {
    IndexKey {
        document: SourceKey {
            path: "测试.json".into(),
            generation: 1,
        },
        revision: 0,
        query: query.into(),
        ticket: 0,
        options: default(),
        scope: None,
    }
}

pub(super) fn document(source: &str) -> Document {
    Document {
        preview_scroll: Vec2::ZERO,
        path: "测试.json".into(),
        format: crate::asset_catalog::DocumentFormat::Config,
        generation: 1,
        disk: DiskIdentity {
            path: "unused.json".into(),
            digest: [0; 32],
            bom: false,
            crlf: false,
        },
        pages: vec![Arc::from(source)],
        source: Arc::from(source),
        revision: 0,
        saved_revision: 0,
        history: default(),
        preview: Arc::new(Vec::new()),
        preview_revision: None,
        preview_clipped: false,
        mode: ViewMode::Edit,
        confirm_reload: false,
        external: default(),
        status: String::new(),
    }
}

#[test]
fn literal_unicode_query_keeps_utf8_positions_and_trailing_empty_lines() {
    let source = "中文配置\n\n{\"中文\": 1}\n";
    let result = index::build(key("中文"), source);
    assert_eq!(result.lines, vec![0, 13, 14, source.len()]);
    assert_eq!(
        result
            .matches
            .iter()
            .map(|range| &source[range.clone()])
            .collect::<Vec<_>>(),
        ["中文", "中文"]
    );
    let result = index::build(key(".*"), "a.*b.*");
    assert_eq!(result.matches, [1..3, 4..6]); // 普通字面查找，符号不具有正则含义。
}

#[test]
fn empty_query_and_empty_document_do_not_match_every_byte() {
    let result = index::build(key(""), "");
    assert_eq!(result.lines, [0]);
    assert!(result.matches.is_empty());
    assert!(index::build(key("找不到"), "abc").matches.is_empty());
}

#[test]
fn next_previous_wrap_and_start_near_the_current_caret() {
    let matches = [2..4, 9..11, 20..22];
    assert_eq!(index::next_match(&matches, None, 5, false), Some(1));
    assert_eq!(index::next_match(&matches, None, 5, true), Some(0));
    assert_eq!(index::next_match(&matches, Some(2), 20, false), Some(0));
    assert_eq!(index::next_match(&matches, Some(0), 2, true), Some(2));
    assert_eq!(index::next_match(&[], None, 0, false), None);
}

#[test]
fn revision_generation_query_and_ticket_all_reject_stale_results() {
    let doc = document("中文");
    let state = DocumentNavigation {
        query: "中文".into(),
        ..default()
    };
    assert!(index::current(&key("中文"), &doc, &state));
    for part in 0..5 {
        let mut key = key("中文");
        match part {
            0 => key.document.path = "另一文件.md".into(),
            1 => key.document.generation += 1,
            2 => key.revision += 1,
            3 => key.query = "新查询".into(),
            _ => key.ticket += 1,
        }
        assert!(!index::current(&key, &doc, &state));
    }
}

#[test]
fn match_intersection_handles_grapheme_ranges_and_soft_wrap_boundaries() {
    let matches = [3..9, 15..21];
    assert!(!paint::intersects(&matches, 0..3));
    assert!(paint::intersects(&matches, 2..6));
    assert!(!paint::intersects(&matches, 9..15));
    assert!(paint::intersects(&matches, 18..24));
    let mut row = vec![0; 16];
    paint::fill_row(&mut row, -3.0, 25.0, 16.0);
    assert!(row.iter().all(|pixel| *pixel == 1));
}

#[test]
fn visible_lines_use_binary_search_instead_of_scanning_the_whole_document() {
    let mut checks = 0;
    let line = paint::lower_line(100_000, |i| {
        checks += 1;
        i < 98_765
    });
    assert_eq!(line, 98_765);
    assert!(checks < 20);
}

#[test]
fn obsolete_task_keeps_the_slot_until_completion_then_only_latest_query_is_adopted() {
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .insert_resource(DocumentState {
            visible: true,
            active: Some("测试.json".into()),
            documents: vec![document("旧查询\n新查询")],
            ..default()
        })
        .init_resource::<DocumentNavigation>()
        .add_systems(Update, index::drive);
    let release = Arc::new(AtomicBool::new(false));
    let worker_release = release.clone();
    app.world_mut().resource_mut::<DocumentNavigation>().task =
        Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
            while !worker_release.load(Ordering::Acquire) {
                bevy::tasks::futures_lite::future::yield_now().await;
            }
            NavigationWork::Index(index::build(key("旧查询"), "旧查询\n新查询"))
        }));
    {
        let mut state = app.world_mut().resource_mut::<DocumentNavigation>();
        state.query = "新查询".into();
        state.ticket = 1;
    }
    app.update();
    assert!(app.world().resource::<DocumentNavigation>().task.is_some());
    assert!(app.world().resource::<DocumentNavigation>().index.is_none());
    release.store(true, Ordering::Release);
    for _ in 0..1000 {
        app.update();
        if app.world().resource::<DocumentNavigation>().index.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let state = app.world().resource::<DocumentNavigation>();
    assert_eq!(state.index.as_ref().unwrap().key.query, "新查询");
    assert_eq!(state.index.as_ref().unwrap().key.ticket, 1);
}

#[test]
fn native_byte_navigation_keeps_text_and_selects_unicode_without_a_second_input() {
    let source = "中文\n第二行\n";
    let mut text = EditableText::new(source);
    let mut font = FontCx::default();
    let mut layout = LayoutCx::default();
    let bytes = feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec();
    let asset = Font::from_bytes(bytes);
    font.context
        .collection
        .register_fonts(asset.data.clone(), None);
    font.set_sans_serif_family("Noto Sans CJK SC").unwrap();
    let mut driver = text.editor.driver(&mut font.context, &mut layout.0);
    driver.select_byte_range(0, 6);
    assert_eq!(driver.editor.raw_selection().text_range(), 0..6);
    driver.move_to_byte(7);
    assert_eq!(driver.editor.raw_selection().text_range(), 7..7);
    assert_eq!(text.value().to_string(), source);
}

#[test]
fn modal_cancels_pending_navigation_instead_of_replaying_it_after_close() {
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .insert_resource(crate::resources::MapEditorState {
            open: true,
            new_map_dialog_open: true,
            ..default()
        })
        .init_resource::<crate::resources::EditorTextInputGuard>()
        .init_resource::<crate::resources::MapEditorNumericScrubMouseGuard>()
        .init_resource::<bevy::input_focus::InputFocus>()
        .init_resource::<FontCx>()
        .init_resource::<LayoutCx>()
        .init_resource::<DocumentState>()
        .insert_resource(DocumentNavigation {
            pending: Some(NavigationRequest::Line(2)),
            ..default()
        })
        .add_systems(Update, super::control::navigate);
    app.update();
    assert!(
        app.world()
            .resource::<DocumentNavigation>()
            .pending
            .is_none()
    );
    app.world_mut()
        .resource_mut::<crate::resources::MapEditorState>()
        .new_map_dialog_open = false;
    app.update();
    assert!(
        app.world()
            .resource::<DocumentNavigation>()
            .pending
            .is_none()
    );
}

/// 真实GPU夹具复用正式键盘消息、原生中文IME和完整文档UI；不手写匹配结果或滚动值。
pub(in super::super) fn gpu_assert_navigation(
    app: &mut App,
    window: Entity,
    source: Entity,
    original: &str,
    target: Handle<Image>,
    width: u32,
    snapshot: fn(&mut App, Handle<Image>, &str, u32),
) {
    use super::{components::*, resources::NavigationPaint};
    use bevy::input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    };
    fn chord(app: &mut App, window: Entity, code: KeyCode, logical: Key, ctrl: bool, shift: bool) {
        let mut events = Vec::new();
        if ctrl {
            events.push((KeyCode::ControlLeft, Key::Control));
        }
        if shift {
            events.push((KeyCode::ShiftLeft, Key::Shift));
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
    fn frames(app: &mut App, count: usize) {
        for _ in 0..count {
            app.update();
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
    }
    fn field(app: &mut App, kind: NavigationField) -> Entity {
        let world = app.world_mut();
        let mut query = world.query::<(Entity, &NavigationField)>();
        query
            .iter(world)
            .find(|(_, field)| **field == kind)
            .unwrap()
            .0
    }
    let revision = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .revision;
    chord(
        app,
        window,
        KeyCode::KeyF,
        Key::Character("f".into()),
        true,
        false,
    );
    frames(app, 3);
    assert_eq!(
        app.world().resource::<DocumentNavigation>().mode,
        NavigationMode::Find
    );
    let query = field(app, NavigationField::Query);
    assert_eq!(
        app.world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(query)
    );
    // 预编辑跨帧保持候选；Enter不能改变正文选区，提交后才检索中文。
    let selection = app
        .world()
        .get::<EditableText>(source)
        .unwrap()
        .editor
        .raw_selection()
        .text_range();
    app.world_mut().write_message(bevy::window::Ime::Preedit {
        window,
        value: "中文".into(),
        cursor: Some((6, 6)),
    });
    frames(app, 2);
    assert!(
        app.world()
            .get::<EditableText>(query)
            .unwrap()
            .is_composing()
    );
    chord(app, window, KeyCode::Enter, Key::Enter, false, false);
    assert_eq!(
        app.world()
            .get::<EditableText>(source)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        selection
    );
    let needle = if original.contains("中文记录") {
        "中文记录"
    } else {
        "角色"
    };
    app.world_mut().write_message(bevy::window::Ime::Commit {
        window,
        value: needle.into(),
    });
    frames(app, 15);
    let expected = original.match_indices(needle).count();
    assert_eq!(
        app.world()
            .resource::<DocumentNavigation>()
            .index
            .as_ref()
            .unwrap()
            .matches
            .len(),
        expected
    );
    assert_eq!(app.world().resource::<DocumentNavigation>().query, needle);
    chord(app, window, KeyCode::Enter, Key::Enter, false, false);
    frames(app, 8);
    let found = app
        .world()
        .get::<EditableText>(source)
        .unwrap()
        .editor
        .raw_selection()
        .text_range();
    assert_eq!(&original[found.clone()], needle);
    assert_eq!(
        app.world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(query)
    );
    if width > 320 {
        assert!(app.world().resource::<NavigationPaint>().overview_ready);
    }
    let marks = &app.world().resource::<NavigationPaint>().atlas;
    assert!(
        app.world()
            .resource::<Assets<Image>>()
            .get(marks)
            .unwrap()
            .data
            .as_ref()
            .unwrap()
            .iter()
            .any(|byte| *byte != 0)
    );
    let selected = app
        .world()
        .resource::<DocumentNavigation>()
        .selected
        .unwrap();
    chord(app, window, KeyCode::Enter, Key::Enter, false, true);
    frames(app, 8);
    assert_eq!(
        app.world().resource::<DocumentNavigation>().selected,
        Some((selected + expected - 1) % expected)
    );
    assert_visible_selection(app, source);
    snapshot(
        app,
        target.clone(),
        if width > 320 {
            "asset-browser-document-find.png"
        } else {
            "asset-browser-document-find-sidebar.png"
        },
        width,
    );
    // 同一匹配再次定位没有新的原生光标代次，仍必须从手动滚开的视区返回。
    let unique = if original.contains("中文记录") {
        "文件末尾必须可编辑"
    } else if original.contains("skill_15") {
        "fn skill_15"
    } else {
        "最后一行"
    };
    {
        let mut text = app.world_mut().get_mut::<EditableText>(query).unwrap();
        text.queue_edit(bevy::text::TextEdit::SelectAll);
        text.queue_edit(bevy::text::TextEdit::Insert(unique.into()));
    }
    frames(app, 8);
    assert_eq!(
        app.world()
            .resource::<DocumentNavigation>()
            .index
            .as_ref()
            .unwrap()
            .matches
            .len(),
        1
    );
    chord(app, window, KeyCode::Enter, Key::Enter, false, false);
    frames(app, 4);
    let first_offset = app
        .world()
        .get::<bevy::ui::widget::TextScroll>(source)
        .unwrap()
        .0;
    assert!(first_offset.y > 0.0);
    app.world_mut()
        .get_mut::<bevy::ui::widget::TextScroll>(source)
        .unwrap()
        .0 = Vec2::ZERO;
    frames(app, 3);
    assert_eq!(
        app.world()
            .get::<bevy::ui::widget::TextScroll>(source)
            .unwrap()
            .0,
        Vec2::ZERO
    );
    chord(app, window, KeyCode::Enter, Key::Enter, false, false);
    frames(app, 3);
    assert_eq!(
        app.world()
            .get::<bevy::ui::widget::TextScroll>(source)
            .unwrap()
            .0,
        first_offset
    );
    assert_visible_selection(app, source);
    // 正文导航仍保留全文、草稿修订和唯一输入。软换行不能生成额外逻辑编号。
    assert_eq!(
        app.world()
            .get::<EditableText>(source)
            .unwrap()
            .value()
            .to_string(),
        original
    );
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .revision,
        revision
    );
    let world = app.world_mut();
    let mut sources =
        world.query_filtered::<Entity, With<super::super::components::DocumentSource>>();
    assert_eq!(sources.iter(world).count(), 1);
    let lines = original.bytes().filter(|byte| *byte == b'\n').count() + 1;
    assert_eq!(
        world
            .resource::<DocumentNavigation>()
            .index
            .as_ref()
            .unwrap()
            .lines
            .len(),
        lines
    );
    chord(
        app,
        window,
        KeyCode::KeyG,
        Key::Character("g".into()),
        true,
        false,
    );
    frames(app, 3);
    let line = field(app, NavigationField::Line);
    app.world_mut()
        .get_mut::<EditableText>(line)
        .unwrap()
        .queue_edit(bevy::text::TextEdit::Insert("0".into()));
    frames(app, 3);
    chord(app, window, KeyCode::Enter, Key::Enter, false, false);
    assert_eq!(
        app.world().resource::<DocumentNavigation>().mode,
        NavigationMode::Go
    );
    assert!(!app.world().resource::<DocumentNavigation>().hint.is_empty());
    {
        let mut text = app.world_mut().get_mut::<EditableText>(line).unwrap();
        text.queue_edit(bevy::text::TextEdit::SelectAll);
        text.queue_edit(bevy::text::TextEdit::Insert("1".into()));
    }
    frames(app, 3);
    chord(app, window, KeyCode::Enter, Key::Enter, false, false);
    frames(app, 3);
    assert_eq!(
        app.world()
            .get::<EditableText>(source)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        0..0
    );
    assert_eq!(
        app.world()
            .get::<bevy::ui::widget::TextScroll>(source)
            .unwrap()
            .0
            .y,
        0.0
    );
    assert_eq!(
        app.world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(source)
    );
    snapshot(
        app,
        target,
        if width > 320 {
            "asset-browser-document-line-numbers.png"
        } else {
            "asset-browser-document-line-numbers-sidebar.png"
        },
        width,
    );
}

fn assert_visible_selection(app: &App, source: Entity) {
    let layout = app
        .world()
        .get::<bevy::text::TextLayoutInfo>(source)
        .unwrap();
    let scroll = app
        .world()
        .get::<bevy::ui::widget::TextScroll>(source)
        .unwrap()
        .0;
    let height = app
        .world()
        .get::<ComputedNode>(source)
        .unwrap()
        .content_box()
        .height();
    for rect in &layout.selection_rects {
        assert!(
            rect.min.y >= scroll.y - 0.01 && rect.max.y <= scroll.y + height + 0.01,
            "匹配选区必须完整可见：{rect:?}, scroll={scroll:?}, view={height}"
        );
    }
}
