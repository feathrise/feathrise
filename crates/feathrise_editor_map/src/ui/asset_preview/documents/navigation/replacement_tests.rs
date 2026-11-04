//! 字面替换的Unicode边界、原字节、冻结选区、一次历史及异步取消回归。

use super::super::{components::DocumentSource, history::Selection, resources::DocumentState};
use super::{
    components::*,
    control, index, replace,
    resources::DocumentNavigation,
    tests::{document, key},
    types::*,
};
use bevy::{
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    text::{EditableText, FontCx, LayoutCx},
};
use std::{
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

fn search(
    source: &str,
    query: &str,
    options: SearchOptions,
    scope: Option<Range<usize>>,
) -> TextIndex {
    let mut key = key(query);
    key.options = options;
    key.scope = scope;
    index::build(key, source)
}

fn make_request(index: TextIndex, replacement: &str) -> ReplacementRequest {
    ReplacementRequest {
        index: Arc::new(index),
        ticket: 1,
        source: Entity::PLACEHOLDER,
        focus: None,
        selection: default(),
        replacement: replacement.into(),
        target: None,
    }
}

#[test]
fn insensitive_matching_reports_original_utf8_offsets_including_kelvin_and_accents() {
    let options = SearchOptions {
        case_sensitive: false,
        whole_word: false,
    };
    let source = "K K k CAFÉ café";
    let found = search(source, "k", options, None);
    assert_eq!(found.matches, [0..1, 2..5, 6..7]);
    let found = search(source, "café", options, None);
    assert_eq!(
        found
            .matches
            .iter()
            .map(|range| &source[range.clone()])
            .collect::<Vec<_>>(),
        ["CAFÉ", "café"]
    );
    assert_eq!(search(source, "k", default(), None).matches, [6..7]);
}

#[test]
fn whole_word_respects_unicode_marks_digits_underscore_and_punctuation_queries() {
    let options = SearchOptions {
        case_sensitive: true,
        whole_word: true,
    };
    let source = "e e\u{301} _e e_ e9 é e-e [e]";
    assert_eq!(search(source, "e", options, None).matches.len(), 4);
    let source = "角色 角色名 _角色 角色_ （角色）";
    assert_eq!(search(source, "角色", options, None).matches.len(), 2);
    assert_eq!(search("a.*b$1 .* a.*", ".*", options, None).matches, [7..9]);
    assert_eq!(
        search(
            "assets/a.json assets/a.jsonx",
            "assets/a.json",
            options,
            None
        )
        .matches,
        [0..13]
    );
}

#[test]
fn selection_edges_do_not_invent_word_boundaries_or_split_unicode_matches() {
    let options = SearchOptions {
        case_sensitive: true,
        whole_word: true,
    };
    assert!(
        search("foobar foo", "foo", options, Some(0..3))
            .matches
            .is_empty()
    );
    assert_eq!(
        search("foobar foo", "foo", options, Some(3..10)).matches,
        [7..10]
    );
    assert!(
        search("中文中文", "中文", default(), Some(1..5))
            .matches
            .is_empty()
    );
    assert!(search("中文", "", default(), None).matches.is_empty());
    assert_eq!(
        search("aaa aa", "aa", default(), Some(1..3)).matches,
        [1..3]
    );
}

#[test]
fn option_and_scope_changes_are_part_of_complete_index_identity() {
    let doc = document("word word");
    let mut state = DocumentNavigation {
        query: "word".into(),
        ..default()
    };
    let initial = index::key_for(&doc, &state);
    assert!(index::current(&initial, &doc, &state));
    state.options.case_sensitive = false;
    assert!(!index::current(&initial, &doc, &state));
    state.options = default();
    state.in_selection = true;
    state.scope = Some(SelectionScope {
        document: doc.source_key(),
        revision: doc.revision,
        range: 5..9,
    });
    assert!(!index::current(&initial, &doc, &state));
    let scoped = index::key_for(&doc, &state);
    state.scope.as_mut().unwrap().range = 0..4;
    assert!(!index::current(&scoped, &doc, &state));
}

#[test]
fn literal_replacement_preserves_mixed_raw_newlines_and_dollar_backslash_text() {
    let raw = "旧\r\n保留\n旧\r\n";
    let source = raw.replace("\r\n", "\n");
    let request = make_request(index::build(key("旧"), &source), "$1\\新");
    let out = replace::prepare(&source, &[Arc::from(raw)], true, true, &request)
        .unwrap()
        .unwrap();
    assert_eq!(out.source.as_ref(), "$1\\新\n保留\n$1\\新\n");
    assert_eq!(out.pages[0].as_ref(), "$1\\新\r\n保留\n$1\\新\r\n");
    assert_eq!(out.count, 2);
}

#[test]
fn reverse_selection_and_frozen_scope_follow_changed_unicode_lengths() {
    let source = "外 旧 旧 外";
    let mut request = make_request(
        search(source, "旧", default(), Some(4..source.len() - 4)),
        "新名字",
    );
    request.selection = Selection {
        anchor: source.len(),
        focus: 0,
    };
    let out = replace::prepare(source, &[Arc::from(source)], false, false, &request)
        .unwrap()
        .unwrap();
    assert_eq!(out.source.as_ref(), "外 新名字 新名字 外");
    assert_eq!(
        out.selection,
        Selection {
            anchor: out.source.len(),
            focus: 0
        }
    );
    assert_eq!(out.scope, Some(4..out.source.len() - 4));
    assert!(out.source.is_char_boundary(out.selection.anchor));
}

#[test]
fn one_target_and_empty_replacement_do_not_replace_other_occurrences() {
    let source = "旧\n旧\n旧";
    let mut request = make_request(index::build(key("旧"), source), "");
    request.target = Some(4..7);
    let out = replace::prepare(source, &[Arc::from("旧\r\n旧\n旧")], true, false, &request)
        .unwrap()
        .unwrap();
    assert_eq!(out.source.as_ref(), "旧\n\n旧");
    assert_eq!(out.pages[0].as_ref(), "旧\r\n\n旧");
    assert_eq!(
        out.selection,
        Selection {
            anchor: 4,
            focus: 4
        }
    );
    assert_eq!(out.count, 1);
}

#[test]
fn output_growth_bom_and_actual_crlf_bytes_are_checked_without_truncation() {
    use super::super::constants::MAX_FILE_BYTES;
    let source = "a".repeat(MAX_FILE_BYTES / 2 + 1);
    let request = make_request(index::build(key("a"), &source), "aa");
    assert!(
        replace::prepare(
            &source,
            &[Arc::from(source.as_str())],
            false,
            false,
            &request
        )
        .is_err()
    );
    let source = "a\n".repeat(MAX_FILE_BYTES / 3);
    let request = make_request(index::build(key("a"), &source), "b");
    let raw = source.replace('\n', "\r\n");
    assert!(replace::prepare(&source, &[Arc::from(raw.as_str())], true, true, &request).is_err());
    assert!(replace::prepare(&source, &[Arc::from(raw)], true, false, &request).is_ok());
}

fn setup(source: &str, query: &str, replacement: &str) -> (App, Entity) {
    fn confirmed(value: &str) -> EditableText {
        let mut text = EditableText::default();
        text.editor.set_text(value);
        text // 无窗口夹具没有apply_text_edits，不能遗留new()排入的TextEnd初始化队列。
    }
    let doc = document(source);
    let document_key = doc.source_key();
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .insert_resource(crate::resources::MapEditorState {
            open: true,
            ..default()
        })
        .init_resource::<crate::resources::EditorTextInputGuard>()
        .init_resource::<crate::resources::MapEditorNumericScrubMouseGuard>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<InputFocus>()
        .init_resource::<FontCx>()
        .init_resource::<LayoutCx>()
        .insert_resource(DocumentState {
            active: Some(doc.path.clone()),
            visible: true,
            documents: vec![doc],
            ..default()
        })
        .insert_resource(DocumentNavigation {
            document: Some(document_key.clone()),
            mode: NavigationMode::Replace,
            query: query.into(),
            replacement: replacement.into(),
            ..default()
        })
        .add_systems(
            Update,
            (
                control::input,
                index::drive,
                control::navigate,
                replace::apply,
            )
                .chain(),
        );
    let bytes = feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec();
    let mut font = app.world_mut().resource_mut::<FontCx>();
    font.context
        .collection
        .register_fonts(Font::from_bytes(bytes).data, None);
    font.set_sans_serif_family("Noto Sans CJK SC").unwrap();
    let source = app
        .world_mut()
        .spawn((
            DocumentSource {
                key: Some(document_key),
                ..default()
            },
            confirmed(source),
        ))
        .id();
    for (kind, value) in [
        (NavigationField::Query, query),
        (NavigationField::Replacement, replacement),
        (NavigationField::Line, ""),
    ] {
        app.world_mut().spawn((
            kind,
            confirmed(value),
            crate::ui::text_input::EditorTextInput,
        ));
    }
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(source, FocusCause::Navigated);
    wait(&mut app, |app| {
        app.world().resource::<DocumentNavigation>().index.is_some()
    });
    (app, source)
}

fn wait(app: &mut App, predicate: impl Fn(&App) -> bool) {
    for _ in 0..2000 {
        app.update();
        if predicate(app) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("后台查找/替换未完成");
}

fn press(app: &mut App, action: NavigationAction) {
    let button = app.world_mut().spawn((action, feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed)).id();
    app.update();
    app.world_mut().despawn(button);
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
}

fn settle(app: &mut App) {
    wait(app, |app| {
        let state = app.world().resource::<DocumentNavigation>();
        state.replace.is_none()
            && state.ready.is_none()
            && state.task.is_none()
            && state.index.is_some()
    });
}

#[test]
fn all_replacements_form_one_history_step_and_same_text_is_a_clean_noop() {
    let (mut app, source) = setup("旧 旧", "旧", "长剑");
    select(&mut app, source, 7, 0);
    press(&mut app, NavigationAction::ReplaceAll);
    settle(&mut app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "长剑 长剑"
    );
    let mut documents = app.world_mut().resource_mut::<DocumentState>();
    let doc = documents.current_mut().unwrap();
    assert!(doc.dirty());
    assert_eq!(
        doc.travel(false),
        Some(Selection {
            anchor: 7,
            focus: 0
        })
    );
    assert_eq!(doc.source_text(), "旧 旧");
    assert!(!doc.dirty());
    assert!(!doc.history.can_undo());
    assert!(doc.travel(true).is_some());
    assert_eq!(doc.source_text(), "长剑 长剑");

    let (mut app, _) = setup("旧 旧", "旧", "旧");
    press(&mut app, NavigationAction::ReplaceAll);
    settle(&mut app);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert_eq!(doc.revision, 0);
    assert!(!doc.dirty() && !doc.history.can_undo());
}

#[test]
fn replace_current_uses_exact_selection_and_moves_to_next_surviving_match() {
    let (mut app, source) = setup("旧 旧 旧", "旧", "新");
    select(&mut app, source, 4, 7);
    press(&mut app, NavigationAction::ReplaceCurrent);
    settle(&mut app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "旧 新 旧"
    );
    assert_eq!(
        app.world()
            .get::<EditableText>(source)
            .unwrap()
            .editor
            .raw_selection()
            .text_range(),
        8..11
    );
    press(&mut app, NavigationAction::ReplaceCurrent);
    settle(&mut app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "旧 新 新"
    );
}

#[test]
fn selection_scope_stays_frozen_through_navigation_then_maps_after_replacement() {
    let (mut app, source) = setup("旧 旧 旧 外", "旧", "新名字");
    select(&mut app, source, 0, 7);
    press(&mut app, NavigationAction::InSelection);
    settle(&mut app);
    assert_eq!(
        app.world()
            .resource::<DocumentNavigation>()
            .index
            .as_ref()
            .unwrap()
            .matches
            .len(),
        2
    );
    press(&mut app, NavigationAction::Next);
    assert_eq!(
        app.world()
            .resource::<DocumentNavigation>()
            .scope
            .as_ref()
            .unwrap()
            .range,
        0..7
    );
    press(&mut app, NavigationAction::ReplaceAll);
    settle(&mut app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "新名字 新名字 旧 外"
    );
    assert_eq!(
        app.world()
            .resource::<DocumentNavigation>()
            .scope
            .as_ref()
            .unwrap()
            .range,
        0..19
    );
    assert!(
        app.world()
            .resource::<DocumentNavigation>()
            .index
            .as_ref()
            .unwrap()
            .matches
            .is_empty()
    );
}

#[test]
fn manual_edit_invalidates_scope_without_broadening_to_the_whole_document() {
    let (mut app, source) = setup("旧 旧", "旧", "新");
    select(&mut app, source, 0, 3);
    press(&mut app, NavigationAction::InSelection);
    settle(&mut app);
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .edit("旧 旧 外");
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .editor
        .set_text("旧 旧 外");
    settle(&mut app);
    let state = app.world().resource::<DocumentNavigation>();
    assert!(state.in_selection && state.scope.is_none());
    assert!(state.index.as_ref().unwrap().matches.is_empty());
    press(&mut app, NavigationAction::ReplaceAll);
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source_text(),
        "旧 旧 外"
    );
}

#[test]
fn no_native_selection_cannot_enable_selection_only_mode() {
    let (mut app, source) = setup("旧 旧", "旧", "新");
    select(&mut app, source, 0, 0);
    press(&mut app, NavigationAction::InSelection);
    assert!(!app.world().resource::<DocumentNavigation>().in_selection);
    assert!(
        app.world()
            .resource::<DocumentNavigation>()
            .hint
            .contains("请先")
    );
}

#[test]
fn ctrl_h_opens_replace_but_other_fields_and_ime_keep_their_shortcuts() {
    let (mut app, source) = setup("旧", "旧", "新");
    app.world_mut().resource_mut::<DocumentNavigation>().mode = NavigationMode::Closed;
    let unrelated = app
        .world_mut()
        .spawn((
            crate::ui::text_input::EditorTextInput,
            EditableText::new("预设"),
        ))
        .id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(unrelated, FocusCause::Navigated);
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyH);
    }
    app.update();
    assert_eq!(
        app.world().resource::<DocumentNavigation>().mode,
        NavigationMode::Closed
    );
    app.world_mut().despawn(unrelated);
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(source, FocusCause::Navigated);
    app.world_mut()
        .resource_mut::<crate::resources::EditorTextInputGuard>()
        .block_shortcuts = true;
    app.update();
    assert_eq!(
        app.world().resource::<DocumentNavigation>().mode,
        NavigationMode::Closed
    );
    app.world_mut()
        .resource_mut::<crate::resources::EditorTextInputGuard>()
        .block_shortcuts = false;
    app.update();
    assert_eq!(
        app.world().resource::<DocumentNavigation>().mode,
        NavigationMode::Replace
    );
}

#[test]
fn obsolete_replacement_keeps_its_worker_slot_and_cannot_apply_after_modal_or_selection_change() {
    for cancel in 0..5 {
        let (mut app, source) = setup("旧 旧", "旧", "新");
        press(&mut app, NavigationAction::ReplaceAll);
        // 将受检请求放入真实可控worker；已启动原任务如提前完成也保持同一事务身份。
        let request = app.world().resource::<DocumentNavigation>().replace.clone();
        if request.is_none() {
            panic!("替换在测试干预前意外采用");
        }
        let request = request.unwrap();
        let release = Arc::new(AtomicBool::new(false));
        let worker_release = release.clone();
        // 先等待原worker真正结束，不能在测试中借丢弃句柄掩盖并发问题。
        if let Some(mut task) = app
            .world_mut()
            .resource_mut::<DocumentNavigation>()
            .task
            .take()
        {
            while bevy::tasks::futures::check_ready(&mut task).is_none() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        app.world_mut().resource_mut::<DocumentNavigation>().task =
            Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                while !worker_release.load(Ordering::Acquire) {
                    bevy::tasks::futures_lite::future::yield_now().await;
                }
                let content =
                    replace::prepare("旧 旧", &[Arc::from("旧 旧")], false, false, &request);
                NavigationWork::Replace(ReplacementResult { request, content })
            }));
        match cancel {
            0 => {
                app.world_mut()
                    .resource_mut::<crate::resources::MapEditorState>()
                    .new_map_dialog_open = true
            }
            1 => select(&mut app, source, 3, 3),
            2 => app
                .world_mut()
                .resource_mut::<InputFocus>()
                .set(source, FocusCause::Navigated),
            3 => app
                .world_mut()
                .get_mut::<EditableText>(source)
                .unwrap()
                .queue_edit(bevy::text::TextEdit::Insert("未采用".into())),
            _ => {
                app.world_mut()
                    .resource_mut::<DocumentState>()
                    .current_mut()
                    .unwrap()
                    .edit("新的草稿");
            }
        }
        app.update();
        assert!(app.world().resource::<DocumentNavigation>().task.is_some());
        release.store(true, Ordering::Release);
        wait(&mut app, |app| {
            let state = app.world().resource::<DocumentNavigation>();
            state.replace.is_none() && state.ready.is_none() && state.task.is_none()
        });
        assert_ne!(
            app.world()
                .resource::<DocumentState>()
                .current()
                .unwrap()
                .source_text(),
            "新 新"
        );
    }
}
