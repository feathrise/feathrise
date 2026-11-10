//! JSON语法、Unicode诊断、词元保真、后台身份、保存事务与原生历史回归。

use feathrise_editor_ui::UiInteraction;
use super::super::{
    components::*, history, io, native, resources::DocumentState, session, types::*,
};
use super::{components::JsonAction, control, format, systems, types::*, validation};
use bevy::{
    ecs::system::RunSystemOnce,
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    tasks::AsyncComputeTaskPool,
    text::{EditableText, FontCx, LayoutCx, TextEdit},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

static COUNTER: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(raw: &str, path: &str) -> (Self, Document) {
        let root = std::env::temp_dir().join(format!(
            "rpg-json-tools-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(path), raw).unwrap();
        let (disk, contents) = io::load(&root, path).unwrap();
        let doc = Document {
            path: path.into(),
            format: crate::asset_catalog::DocumentFormat::Config,
            generation: 1,
            disk,
            pages: contents.pages,
            source: contents.text,
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
        };
        (Self { root }, doc)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let temp = std::env::temp_dir();
        assert!(self.root.parent() == Some(temp.as_path())); // 只回收本测试明确创建的临时子目录。
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn app(fixture: &Fixture, doc: Document) -> App {
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .insert_resource(DocumentState {
            root: fixture.root.clone(),
            active: Some(doc.path.clone()),
            documents: vec![doc],
            visible: true,
            ..default()
        })
        .add_systems(Update, (session::drive_tasks, systems::drive).chain());
    app
}

fn settle(app: &mut App) {
    for _ in 0..1000 {
        app.update();
        let state = app.world().resource::<DocumentState>();
        if state.io.is_none()
            && state.parser.is_none()
            && state.pending_load.is_none()
            && state.json.task.is_none()
            && state.json.save.is_none()
            && state.json.format.is_none()
            && state.json.ready.is_none()
            && state.json.locate.is_none()
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    panic!("JSON后台任务未完成");
}

#[test]
fn json_invalid_save_keeps_draft_and_disk_then_valid_save_preserves_bom_crlf() {
    let original = "\u{feff}{\"中文\":1}\r\n";
    let (fixture, doc) = Fixture::new(original, "设置.JSON");
    let mut app = app(&fixture, doc);
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state
            .current_mut()
            .unwrap()
            .edit("{\"中文\":2 \"其他\":3}\n");
        state.request_save();
    }
    settle(&mut app);
    let state = app.world().resource::<DocumentState>();
    assert!(state.current().unwrap().dirty());
    assert!(state.current().unwrap().status.contains("保存已停止"));
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("设置.JSON")).unwrap(),
        original
    );
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state
            .current_mut()
            .unwrap()
            .edit("{\"中文\":2,\"其他\":3}\n");
        state.request_save();
    }
    settle(&mut app);
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("设置.JSON")).unwrap(),
        "\u{feff}{\"中文\":2,\"其他\":3}\r\n"
    );
}

#[test]
fn json_save_waiting_for_validation_cancels_when_the_submitted_revision_changes() {
    let (fixture, doc) = Fixture::new("{\"值\":1}", "设置.json");
    let mut app = app(&fixture, doc);
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.current_mut().unwrap().edit("{\"值\":2}");
        state.request_save();
        assert!(state.json.save.is_some());
        state.current_mut().unwrap().edit("{\"值\":3}");
    }
    settle(&mut app);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("设置.json")).unwrap(),
        "{\"值\":1}"
    );
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert!(doc.dirty());
    assert!(doc.status.contains("保存已取消"));
}

#[test]
fn json_task_remains_single_flight_when_content_or_document_changes() {
    let (fixture, doc) = Fixture::new("{}", "设置.json");
    let mut app = app(&fixture, doc);
    app.world_mut().resource_mut::<DocumentState>().json.task =
        Some(AsyncComputeTaskPool::get().spawn(std::future::pending()));
    for value in ["[]", "true", "null"] {
        app.world_mut()
            .resource_mut::<DocumentState>()
            .current_mut()
            .unwrap()
            .edit(value);
        app.update();
        assert!(app.world().resource::<DocumentState>().json.task.is_some());
        assert!(
            app.world()
                .resource::<DocumentState>()
                .json
                .cache
                .is_empty()
        );
    }
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .generation += 1;
    app.update();
    assert!(app.world().resource::<DocumentState>().json.task.is_some());
    assert!(
        app.world()
            .resource::<DocumentState>()
            .json
            .cache
            .is_empty()
    );
}

#[test]
fn json_explicit_checked_save_survives_hidden_tab_and_preserves_external_edit_conflicts() {
    let (fixture, doc) = Fixture::new("{\"值\":1}", "设置.json");
    let mut app = app(&fixture, doc);
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.current_mut().unwrap().edit("{\"值\":2}");
        state.request_save();
        state.active = None;
        state.visible = false;
    }
    settle(&mut app);
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("设置.json")).unwrap(),
        "{\"值\":2}"
    );
    assert!(!app.world().resource::<DocumentState>().documents[0].dirty());
    std::fs::write(fixture.root.join("设置.json"), "{\"值\":3}").unwrap();
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.active = Some("设置.json".into());
        state.visible = true;
        state.current_mut().unwrap().edit("{\"值\":4}");
        state.request_save();
    }
    settle(&mut app);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert!(doc.status.contains("外部修改"));
    assert!(doc.dirty());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("设置.json")).unwrap(),
        "{\"值\":3}"
    );
}

fn native_app(fixture: &Fixture, doc: Document) -> (App, Entity) {
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
    let key = doc.source_key();
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .insert_resource(font)
        .insert_resource(layout)
        .init_resource::<bevy::clipboard::Clipboard>()
        .init_resource::<super::super::resources::DocumentInputs>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<crate::resources::EditorTextInputGuard>()
        .init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>()
        .insert_resource(crate::resources::SceneEditorState {
            open: true,
            ..default()
        })
        .insert_resource(DocumentState {
            root: fixture.root.clone(),
            active: Some(doc.path.clone()),
            documents: vec![doc],
            visible: true,
            ..default()
        })
        .configure_sets(
            Update,
            (
                crate::SceneEditorSystemSet::Input,
                crate::SceneEditorSystemSet::Tasks,
                crate::SceneEditorSystemSet::Presentation,
            )
                .chain(),
        )
        .add_systems(
            Update,
            (bevy::text::apply_text_edits, session::capture_edits)
                .chain()
                .before(crate::SceneEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            session::drive_tasks.in_set(crate::SceneEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            native::bind.in_set(crate::SceneEditorSystemSet::Presentation),
        );
    history::install(&mut app);
    super::install(&mut app);
    let source = app
        .world_mut()
        .spawn((
            DocumentSource { key: Some(key) },
            text,
            ComputedNode::default(),
            DocumentScrollState::default(),
        ))
        .id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(source, FocusCause::Navigated);
    (app, source)
}

fn press(app: &mut App, action: JsonAction) {
    let button = app.world_mut().spawn((action, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed)).id();
    app.update();
    app.world_mut().despawn(button);
}

#[test]
fn json_format_is_one_native_history_step_and_undo_restores_exact_raw_bytes() {
    let raw = "\u{feff}{\"名字\":\"中文🙂\",\"值\":1}\r\n";
    let (fixture, doc) = Fixture::new(raw, "设置.json");
    let old = doc.source.clone();
    let old_pages = doc.pages.clone();
    let (mut app, source) = native_app(&fixture, doc);
    press(&mut app, JsonAction::Format);
    settle(&mut app);
    let formatted = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .source
        .clone();
    assert_ne!(old, formatted);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        formatted.as_ref()
    );
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("设置.json")).unwrap(),
        raw,
        "格式化不隐式写盘"
    );
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::KeyZ);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert_eq!(doc.source, old);
    assert_eq!(doc.pages, old_pages);
    assert!(!doc.dirty());
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        old.as_ref()
    );
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.press(KeyCode::ControlLeft);
    keys.press(KeyCode::KeyY);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source,
        formatted
    );
    app.world_mut()
        .resource_mut::<DocumentState>()
        .request_save();
    settle(&mut app);
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
}

#[test]
fn json_preview_error_click_enters_source_and_reveals_the_same_diagnostic() {
    let (fixture, mut doc) = Fixture::new("{\n  \"中文\": 1 \"后续\": 2\n}", "设置.json");
    let error = validation::validate(doc.source_text()).unwrap();
    doc.mode = ViewMode::Preview;
    let (mut app, source) = native_app(&fixture, doc);
    settle(&mut app);
    app.world_mut()
        .resource_mut::<super::super::resources::DocumentInputs>()
        .sessions
        .clear();
    press(&mut app, JsonAction::Locate);
    settle(&mut app);
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
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
}

#[test]
fn json_format_shortcut_requires_source_focus_and_ime_or_pending_paste_blocks_actions() {
    let (fixture, doc) = Fixture::new("{}", "设置.json");
    let (mut app, source) = native_app(&fixture, doc);
    let other = app.world_mut().spawn_empty().id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(other, FocusCause::Navigated);
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::AltLeft);
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::KeyF);
    }
    app.update();
    assert!(
        app.world()
            .resource::<DocumentState>()
            .json
            .format
            .is_none()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(source, FocusCause::Navigated);
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::ImeSetCompose {
            value: "候选".into(),
            cursor: None,
        });
    app.update();
    press(&mut app, JsonAction::Format);
    assert!(
        app.world()
            .resource::<DocumentState>()
            .json
            .format
            .is_none()
    );
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source
            .as_ref(),
        "{}"
    );
    {
        let mut text = app.world_mut().get_mut::<EditableText>(source).unwrap();
        text.clear();
        text.editor.set_text("{}");
        text.pending_paste = Some(bevy::clipboard::ClipboardRead::Ready(Ok("粘贴".into())));
    }
    let button = app
        .world_mut()
        .spawn((JsonAction::Format, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
        .id();
    app.world_mut().run_system_once(control::input).unwrap();
    assert!(
        app.world()
            .resource::<DocumentState>()
            .json
            .format
            .is_none()
    );
    app.world_mut().despawn(button); // 模拟回执不读取操作系统剪贴板，忙碌状态不能创建格式化意图。
}

#[test]
fn json_windows_format_shortcut_uses_native_source_and_a_single_undo_step() {
    let (fixture, doc) = Fixture::new("{}", "设置.json");
    let (mut app, source) = native_app(&fixture, doc);
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::AltLeft);
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::KeyF);
    }
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release_all();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    settle(&mut app);
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "{}\n"
    );
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::KeyZ);
    }
    app.update();
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        "{}"
    );
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
}

#[test]
fn json_background_validation_and_format_report_bounded_local_cost() {
    let source = format!("[{}0]", "123.4500,".repeat(12_000));
    let mut check_times = Vec::new();
    let mut format_times = Vec::new();
    for _ in 0..16 {
        let start = std::time::Instant::now();
        assert!(validation::validate(&source).is_none());
        check_times.push(start.elapsed().as_secs_f64() * 1000.0);
        let start = std::time::Instant::now();
        let formatted = format::pretty(&source, true, true, default())
            .unwrap()
            .unwrap();
        assert!(formatted.pages[0].len() + 3 <= super::super::constants::MAX_FILE_BYTES);
        format_times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    check_times.sort_by(f64::total_cmp);
    format_times.sort_by(f64::total_cmp);
    println!(
        "JSON后台局部诊断：{}字节，校验中位/最大(ms)={:.3}/{:.3}，受检格式化中位/最大(ms)={:.3}/{:.3}；不含任务采用、原生全文重排或游戏帧时",
        source.len(),
        check_times[8],
        check_times[15],
        format_times[8],
        format_times[15]
    );
}

#[test]
fn json_invalid_format_and_late_result_keep_original_content_and_history() {
    let (fixture, doc) = Fixture::new("{\"a\":}", "设置.json");
    let (mut app, source) = native_app(&fixture, doc);
    press(&mut app, JsonAction::Format);
    settle(&mut app);
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
            .status
            .contains("无法格式化")
    );
    // 旧后台结果在新编辑后到达，不能把新草稿覆盖成格式化的旧文件。
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    let key = JsonKey::new(doc);
    let request = FormatRequest {
        key: key.clone(),
        ticket: 9,
        source,
        selection: default(),
    };
    app.world_mut()
        .get_mut::<EditableText>(source)
        .unwrap()
        .queue_edit(TextEdit::Insert(" ".into()));
    app.update();
    let before = app
        .world()
        .resource::<DocumentState>()
        .current()
        .unwrap()
        .source
        .clone();
    app.world_mut().resource_mut::<DocumentState>().json.ready = Some(WorkResult {
        validation: Validation { key, error: None },
        request: Some(request),
        formatted: Ok(Some(
            format::pretty("{}", false, false, default())
                .unwrap()
                .unwrap(),
        )),
    });
    app.update();
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .source,
        before
    );
    assert_eq!(
        app.world().get::<EditableText>(source).unwrap().value(),
        before.as_ref()
    );
}

#[test]
fn json_reload_pending_does_not_reopen_discarded_source_via_error_or_format_tools() {
    let (fixture, doc) = Fixture::new("{\"值\":}", "设置.json");
    let (mut app, _) = native_app(&fixture, doc);
    settle(&mut app);
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.current_mut().unwrap().mode = ViewMode::Preview;
        state.request_load("设置.json".into());
    }
    let button = app
        .world_mut()
        .spawn((JsonAction::Locate, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
        .id();
    app.world_mut().run_system_once(control::input).unwrap();
    assert_eq!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .mode,
        ViewMode::Preview
    );
    assert!(
        app.world()
            .resource::<DocumentState>()
            .json
            .locate
            .is_none()
    );
    app.world_mut().despawn(button);
    let button = app
        .world_mut()
        .spawn((JsonAction::Format, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
        .id();
    app.world_mut().run_system_once(control::input).unwrap();
    assert!(
        app.world()
            .resource::<DocumentState>()
            .json
            .format
            .is_none()
    );
    app.world_mut().despawn(button);
}
