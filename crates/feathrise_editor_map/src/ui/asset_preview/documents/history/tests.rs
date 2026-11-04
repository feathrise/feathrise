//! 撤销保存点、中文字节选区、分组边界、内存上限与官方输入/多文件缓存回归。

use super::super::{
    components::DocumentSource,
    io, native,
    resources::{DocumentInputs, DocumentState},
    session,
    types::*,
};
use super::{constants::*, control, resources::HistoryCapture, types::*};
use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, FontCx, LayoutCx, TextEdit},
    ui::widget::TextScroll,
};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

fn document(raw: &str) -> Document {
    Document {
        path: "history.json".into(),
        format: crate::asset_catalog::DocumentFormat::Config,
        generation: 1,
        disk: DiskIdentity {
            path: PathBuf::from("history.json"),
            digest: [0; 32],
            bom: false,
            crlf: raw.contains("\r\n"),
        },
        pages: io::source_pages(raw),
        source: Arc::from(raw.replace("\r\n", "\n")),
        revision: 0,
        saved_revision: 0,
        history: default(),
        preview: Arc::new(Vec::new()),
        preview_revision: None,
        preview_clipped: false,
        mode: ViewMode::Edit,
        preview_scroll: Vec2::ZERO,
        confirm_reload: false,
        external: default(),
        status: String::new(),
    }
}

fn typing(before: usize, after: usize, at: Instant) -> EditContext {
    EditContext {
        before: Selection {
            anchor: before,
            focus: before,
        },
        after: Selection {
            anchor: after,
            focus: after,
        },
        typing: true,
        at,
    }
}

#[test]
fn history_restores_exact_mixed_line_endings_content_selection_and_clean_save_point() {
    let raw = "中文\r\n第二行\n尾部🙂\r\n";
    let mut doc = document(raw);
    let original_source = doc.source.clone();
    let original_pages = doc.pages.clone();
    doc.edit_with(
        "修改\n第二行\n尾部🙂\n",
        EditContext {
            before: Selection {
                anchor: 6,
                focus: 0,
            },
            after: Selection {
                anchor: 6,
                focus: 6,
            },
            ..default()
        },
    );
    assert!(doc.dirty());
    assert_eq!(
        doc.travel(false),
        Some(Selection {
            anchor: 6,
            focus: 0
        })
    );
    assert!(Arc::ptr_eq(&doc.source, &original_source));
    assert_eq!(doc.pages, original_pages);
    assert!(!doc.dirty());
    assert_eq!(
        doc.revision, 2,
        "派生修订不能倒退，否则旧后台结果会重新有效"
    );
    assert_eq!(
        doc.travel(true),
        Some(Selection {
            anchor: 6,
            focus: 6
        })
    );
    assert!(doc.dirty());
    assert_eq!(doc.revision, 3);
}

#[test]
fn history_merges_contiguous_typing_but_timeout_motion_and_delete_are_separate() {
    let mut doc = document("");
    let at = Instant::now();
    doc.edit_with("中", typing(0, 3, at));
    doc.edit_with("中文", typing(3, 6, at + Duration::from_millis(100)));
    assert_eq!(doc.history.undo.len(), 1);
    doc.edit_with("中文a", typing(6, 7, at + Duration::from_secs(2)));
    assert_eq!(doc.history.undo.len(), 2);
    doc.edit_with("b中文a", typing(0, 1, at + Duration::from_millis(2100)));
    assert_eq!(doc.history.undo.len(), 3, "光标位置不同不能合并");
    doc.edit("b中文"); // 删除没有typing资格，即使很快仍是独立一步。
    assert_eq!(doc.history.undo.len(), 4);
    doc.travel(false);
    assert_eq!(doc.source_text(), "b中文a");
    doc.travel(false);
    assert_eq!(doc.source_text(), "中文a");
    doc.travel(false);
    assert_eq!(doc.source_text(), "中文");
    doc.travel(false);
    assert_eq!(doc.source_text(), "");
    assert!(!doc.dirty());
}

#[test]
fn history_new_edit_after_undo_discards_redo_and_cannot_reuse_a_content_identity() {
    let mut doc = document("a");
    doc.edit("b");
    doc.edit("c");
    doc.saved_revision = doc.content_revision();
    doc.travel(false);
    assert!(doc.dirty());
    let previous = doc.content_revision();
    doc.edit("d");
    assert!(!doc.history.can_redo());
    assert!(doc.content_revision() > previous);
    assert!(doc.dirty());
}

#[test]
fn history_save_receipt_marks_only_submitted_content_even_after_travel() {
    let mut doc = document("原文");
    doc.edit("提交的内容");
    let submitted = doc.content_revision();
    doc.edit("更新的内容");
    doc.travel(false);
    let revision = doc.revision;
    doc.saved_revision = submitted; // 异步回执来到时已经撤销回提交内容。
    assert!(!doc.dirty());
    assert_ne!(
        revision, doc.saved_revision,
        "内容保存点和任务修订必须分别判断"
    );
    doc.travel(true);
    assert!(doc.dirty());
    doc.travel(false);
    assert!(!doc.dirty());
}

#[test]
fn history_bounds_entry_count_and_shared_snapshot_bytes() {
    let mut doc = document("a");
    for index in 0..300 {
        doc.edit(&format!("修改{index}"));
    }
    assert_eq!(doc.history.undo.len(), MAX_ENTRIES);
    assert!(doc.history.bytes <= MAX_SNAPSHOT_BYTES);
    let large = "a\n".repeat(128 * 1024);
    let mut doc = document(&large);
    for index in 0..24 {
        doc.edit(&format!("{index}\n{large}"));
    }
    assert!(doc.history.undo.len() < MAX_ENTRIES);
    assert!(doc.history.bytes <= MAX_SNAPSHOT_BYTES);
    assert!(doc.history.can_undo());
    while doc.travel(false).is_some() {}
    assert!(doc.history.bytes <= MAX_SNAPSHOT_BYTES);
    doc.edit("分支");
    assert!(!doc.history.can_redo());
    assert!(doc.history.bytes <= MAX_SNAPSHOT_BYTES);
}

#[test]
fn history_snapshot_recording_shares_large_text_and_reports_only_its_own_cost() {
    let doc = document(&"a\n".repeat(128 * 1024));
    let snapshot = Snapshot {
        source: doc.source.clone(),
        pages: doc.pages.clone(),
        content_revision: 0,
        selection: Selection::default(),
    };
    let mut history = DocumentHistory::default();
    let mut times = Vec::new();
    // 测量只含共享快照克隆、登记和预算淘汰；不把它冒充原生全文重排或游戏总帧时。
    for index in 0..512 {
        let start = Instant::now();
        let before = Snapshot {
            content_revision: index,
            ..snapshot.clone()
        };
        let after = Snapshot {
            content_revision: index + 1,
            ..snapshot.clone()
        };
        history.record(before, after, EditContext::default());
        times.push(start.elapsed().as_secs_f64() * 1_000_000.0);
    }
    assert!(history.bytes <= MAX_SNAPSHOT_BYTES);
    assert!(Arc::ptr_eq(
        &history.undo.back().unwrap().after.source,
        &doc.source
    ));
    times.sort_by(f64::total_cmp);
    println!(
        "256 KiB文本、512次共享历史登记/淘汰：中位{:.3}微秒、最大{:.3}微秒（不含原生排版）",
        times[times.len() / 2],
        times[times.len() - 1]
    );
}

fn app(source: &str) -> (App, Entity) {
    let doc = document(source);
    let key = doc.source_key();
    let mut font = FontCx::default();
    let bytes = feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec();
    let asset = Font::from_bytes(bytes);
    font.context.collection.register_fonts(asset.data, None);
    font.set_sans_serif_family("Noto Sans CJK SC").unwrap();
    let mut layout = LayoutCx::default();
    let mut text = EditableText::default();
    text.allow_newlines = true;
    text.editor.set_text(doc.source_text());
    text.editor
        .driver(&mut font.context, &mut layout.0)
        .move_to_text_end();
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .insert_resource(font)
        .insert_resource(layout)
        .init_resource::<bevy::clipboard::Clipboard>()
        .init_resource::<HistoryCapture>()
        .init_resource::<DocumentInputs>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<crate::resources::EditorTextInputGuard>()
        .init_resource::<crate::resources::MapEditorNumericScrubMouseGuard>()
        .init_resource::<crate::resources::MapEditorSettingsState>()
        .insert_resource(crate::resources::MapEditorState {
            open: true,
            ..default()
        })
        .insert_resource(DocumentState {
            active: Some(doc.path.clone()),
            documents: vec![doc],
            visible: true,
            ..default()
        })
        .add_systems(
            Update,
            (
                control::prepare,
                bevy::text::apply_text_edits,
                session::capture_edits,
                control::input,
                native::bind,
            )
                .chain(),
        );
    let source = app
        .world_mut()
        .spawn((
            DocumentSource { key: Some(key) },
            text,
            TextScroll::default(),
            ComputedNode::default(),
        ))
        .id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(source, FocusCause::Navigated);
    (app, source)
}

fn chord(app: &mut App, key: KeyCode, shift: bool) {
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.press(KeyCode::ControlLeft);
    if shift {
        keys.press(KeyCode::ShiftLeft);
    }
    keys.press(key);
    app.update();
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.release_all();
    keys.clear();
}

fn insert(app: &mut App, source: Entity, value: &str) {
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::Insert(value.into()));
    app.update();
}

#[test]
fn history_official_input_groups_typing_and_supports_both_redo_shortcuts() {
    let (mut app, source) = app("原文");
    insert(&mut app, source, "a");
    insert(&mut app, source, "b");
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .history
            .undo
            .len(),
        1
    );
    chord(&mut app, KeyCode::KeyZ, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "原文"
    );
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    chord(&mut app, KeyCode::KeyY, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "原文ab"
    );
    chord(&mut app, KeyCode::KeyZ, false);
    chord(&mut app, KeyCode::KeyZ, true);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "原文ab"
    );
}

#[test]
fn history_native_paste_and_delete_are_independent_without_using_system_clipboard() {
    let (mut app, source) = app("前文");
    insert(&mut app, source, "a");
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .pending_paste = Some(bevy::clipboard::ClipboardRead::Ready(Ok(
        "粘贴\n两行".into()
    )));
    app.update(); // 用官方跨帧粘贴回执，不读写用户剪贴板。
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "前文a粘贴\n两行"
    );
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::Backspace);
    app.update();
    chord(&mut app, KeyCode::KeyZ, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "前文a粘贴\n两行"
    );
    chord(&mut app, KeyCode::KeyZ, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "前文a"
    );
    chord(&mut app, KeyCode::KeyZ, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "前文"
    );
}

#[test]
fn history_ime_candidates_never_enter_journal_and_commit_is_one_separate_step() {
    let (mut app, source) = app("中文");
    insert(&mut app, source, "a");
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::ImeSetCompose {
            value: "候选".into(),
            cursor: None,
        });
    app.update();
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_text(),
        "中文a"
    );
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .history
            .undo
            .len(),
        1
    );
    chord(&mut app, KeyCode::KeyZ, false);
    assert!(
        app.world()
            .get::<EditableText>(source)
            .unwrap()
            .is_composing()
    );
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::ImeCommit {
            value: "确认".into(),
        });
    app.update();
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "中文a确认"
    );
    chord(&mut app, KeyCode::KeyZ, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "中文a"
    );
    assert_eq!(
        Selection::read(app.world().get::<EditableText>(source).unwrap()),
        Selection {
            anchor: 7,
            focus: 7
        }
    );
}

#[test]
fn history_hidden_modal_and_other_input_focus_do_not_consume_document_undo() {
    for blocked in 0..4 {
        let (mut app, source) = app("原文");
        insert(&mut app, source, "a");
        match blocked {
            0 => app.world_mut().resource_mut::<DocumentState>().visible = false,
            1 => {
                app.world_mut()
                    .resource_mut::<crate::resources::MapEditorSettingsState>()
                    .open = true
            }
            2 => {
                app.world_mut()
                    .resource_mut::<crate::resources::EditorTextInputGuard>()
                    .block_shortcuts = true
            }
            _ => {
                let field = app.world_mut().spawn(EditableText::default()).id();
                app.world_mut()
                    .resource_mut::<InputFocus>()
                    .set(field, FocusCause::Navigated);
            }
        }
        chord(&mut app, KeyCode::KeyZ, false);
        assert_eq!(
            app.world()
                .resource::<DocumentState>()
                .current()
                .unwrap()
                .source_text(),
            "原文a"
        );
    }
}

#[test]
fn history_file_switch_restores_native_state_and_keeps_each_journal_independent() {
    let (mut app, source) = app("甲");
    insert(&mut app, source, "a");
    let first = app
        .world()
        .resource::<DocumentState>()
        .active
        .clone()
        .unwrap();
    let mut second = document("乙");
    second.path = "second.rs".into();
    {
        let mut docs = app.world_mut().resource_mut::<DocumentState>();
        docs.active = Some(second.path.clone());
        docs.documents.push(second);
    }
    app.update();
    app.update(); // 正式bind移动原生状态，再采用新文件TextStart队列。
    insert(&mut app, source, "b");
    chord(&mut app, KeyCode::KeyZ, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "乙"
    );
    app.world_mut().resource_mut::<DocumentState>().active = Some(first);
    app.update();
    app.update();
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "甲a"
    );
    chord(&mut app, KeyCode::KeyZ, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "甲"
    );
    let docs = app.world().resource::<DocumentState>();
    assert_eq!(docs.documents[1].source_text(), "乙");
    assert!(docs.documents[1].history.can_redo());
}

#[test]
fn history_noop_input_and_cursor_motion_do_not_record_content() {
    let (mut app, source) = app("中文\n正文");
    app.update();
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::TextStart(false));
    app.update();
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert!(!doc.history.can_undo());
    assert!(!doc.dirty());
    assert_eq!(doc.revision, 0);
}

#[test]
fn history_native_ime_replacement_restores_reverse_unicode_selection() {
    let (mut app, source) = app("中文你好");
    let mut font = app.world_mut().remove_resource::<FontCx>().unwrap();
    let mut layout = app.world_mut().remove_resource::<LayoutCx>().unwrap();
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .editor
        .driver(&mut font.context, &mut layout.0)
        .select_byte_range(6, 0);
    app.insert_resource(font).insert_resource(layout);
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::ImeSetCompose {
            value: "候选".into(),
            cursor: None,
        });
    app.update();
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::ImeCommit {
            value: "替换".into(),
        });
    app.update();
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "替换你好"
    );
    chord(&mut app, KeyCode::KeyZ, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "中文你好"
    );
    assert_eq!(
        Selection::read(app.world().get::<EditableText>(source).unwrap()),
        Selection {
            anchor: 6,
            focus: 0
        }
    );
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
}

/// 完整GPU夹具通过真实键盘消息验证控件路由；不直接调用travel伪造界面结果。
pub(in super::super) fn gpu_chord(app: &mut App, window: Entity, code: KeyCode, shift: bool) {
    use bevy::input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    };
    let mut chord = vec![(KeyCode::ControlLeft, Key::Control)];
    if shift {
        chord.push((KeyCode::ShiftLeft, Key::Shift));
    }
    chord.push((
        code,
        Key::Character(if code == KeyCode::KeyY { "y" } else { "z" }.into()),
    ));
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

pub(in super::super) fn gpu_assert_history(app: &mut App, window: Entity, source: Entity) {
    let before = "fn main() {\n    println!(\"第二文件\");\n}\n";
    let after = format!("{before}// 第二份草稿\n");
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        after.as_str()
    );
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(source, FocusCause::Navigated);
    let revision = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .revision;
    gpu_chord(app, window, KeyCode::KeyZ, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        before
    );
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .revision
            > revision
    );
    gpu_chord(app, window, KeyCode::KeyY, false);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        after.as_str()
    );
    gpu_chord(app, window, KeyCode::KeyZ, false);
    gpu_chord(app, window, KeyCode::KeyZ, true);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        after.as_str()
    );
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
}

pub(in super::super) fn gpu_click_history(
    app: &mut App,
    window: Entity,
    camera: Entity,
    redo: bool,
) {
    use super::components::HistoryAction;
    use bevy::ecs::system::RunSystemOnce;
    let action = if redo {
        HistoryAction::Redo
    } else {
        HistoryAction::Undo
    };
    let (entity, position) = {
        let world = app.world_mut();
        let mut buttons = world.query::<(Entity, &HistoryAction, &UiGlobalTransform)>();
        let (entity, _, transform) = buttons
            .iter(world)
            .find(|(_, candidate, _)| **candidate == action)
            .unwrap();
        (entity, transform.affine().translation)
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
        "真实鼠标必须命中撤销/重做按钮"
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
    app.update();
}
