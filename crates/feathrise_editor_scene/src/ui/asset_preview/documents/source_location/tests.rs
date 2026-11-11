//! 诊断行列必须与原生 UTF-8 字节选择一致，且过期工程身份不能进入原文档会话。

use super::systems::{current, position};
use crate::{asset_catalog::project_files::EditorProjectFileCatalog, standalone::OpenProjectSourceLocation};
use std::path::PathBuf;

use super::{resources::SourceLocationState, systems};
use super::super::{components::DocumentSource, resources::DocumentState, types::SourceKey};
use crate::{resources::SceneEditorState, ui::vegetation_preset_editor::VegetationPresetEditorState, workbench::WorkbenchModel};
use bevy::{input_focus::{FocusCause, InputFocus}, prelude::*, text::{EditableText, TextEdit}};

#[test]
fn unicode_columns_select_whole_character_without_changing_line_endings() {
    let text = "fn main() {\n    let 中文 = 1;\n}\n";
    let (range, clamped) = position(text, 2, 9);
    assert_eq!(&text[range], "中");
    assert!(!clamped);
    let (range, clamped) = position(text, 4, 1);
    assert_eq!(range, text.len()..text.len());
    assert!(!clamped);
}

#[test]
fn shorter_draft_clamps_location_and_reports_that_saved_position_moved() {
    let text = "一行\n尾";
    let (range, clamped) = position(text, 999, 999);
    assert_eq!(range, text.len()..text.len());
    assert!(clamped);
    let (range, clamped) = position("", 1, 1);
    assert_eq!(range, 0..0);
    assert!(!clamped);
}

#[test]
fn diagnostic_navigation_requires_current_root_and_source_context() {
    let root = PathBuf::from(if cfg!(windows) { "D:/diagnostic-fixture" } else { "/diagnostic-fixture" });
    let mut catalog = EditorProjectFileCatalog::default();
    catalog.bind_project_source(0, Some(root.clone()));
    let mut request = OpenProjectSourceLocation { context: 0, project_root: root, path: "src/main.rs".into(), line: 1, column: 1 };
    assert!(current(&request, &catalog));
    request.context = 1;
    assert!(!current(&request, &catalog));
    request.context = 0;
    request.project_root.push("another");
    assert!(!current(&request, &catalog));
}

fn receiver() -> (App, OpenProjectSourceLocation) {
    let root = std::env::temp_dir().join("feathrise-source-location-receive-owned-fixture");
    let mut catalog = EditorProjectFileCatalog::default();
    catalog.bind_project_source(0, Some(root.clone()));
    let mut documents = DocumentState::default();
    documents.root = root.join("assets");
    documents.project_root = Some(root.clone());
    let mut app = App::new();
    app.insert_resource(catalog).insert_resource(documents)
        .insert_resource(SceneEditorState { open: true, ..default() })
        .insert_resource(WorkbenchModel { context: 0, project_root: Some(root.clone()), asset_root: Some(root.join("assets")), ..default() })
        .init_resource::<VegetationPresetEditorState>().init_resource::<SourceLocationState>().init_resource::<InputFocus>()
        .add_message::<OpenProjectSourceLocation>().add_systems(Update, systems::receive);
    (app, OpenProjectSourceLocation { context: 0, project_root: root, path: "src/author_notes.rs".into(), line: 1, column: 1 })
}

#[test]
fn unrelated_hidden_field_rebinding_does_not_drop_explicit_document_open() {
    let (mut app, request) = receiver();
    let mut input = EditableText::default();
    input.queue_edit(TextEdit::TextEnd(false));
    let hidden = app.world_mut().spawn((input, Node { display: Display::None, ..default() })).id();
    app.world_mut().write_message(request);
    app.update();
    assert_eq!(app.world().resource::<DocumentState>().expanded.as_ref().unwrap().path, "project://src/author_notes.rs");
    assert!(app.world().resource::<SourceLocationState>().pending.is_some());
    assert_eq!(app.world().get::<EditableText>(hidden).unwrap().pending_edits.len(), 1,
        "打开文档不得靠消费或清空另一个领域的隐藏回绑定来伪造成功");
}

#[test]
fn focused_field_and_bound_document_pending_edits_still_protect_original_input() {
    for document in [false, true] {
        let (mut app, request) = receiver();
        let mut input = EditableText::default();
        input.queue_edit(TextEdit::Insert("尚未采用的作者输入".into()));
        let entity = app.world_mut().spawn(input).id();
        if document {
            app.world_mut().entity_mut(entity).insert(DocumentSource { key: Some(SourceKey { path: "project://src/current.rs".into(), generation: 1 }) });
        } else {
            app.world_mut().resource_mut::<InputFocus>().set(entity, FocusCause::Navigated);
        }
        app.world_mut().write_message(request);
        app.update();
        assert!(app.world().resource::<DocumentState>().expanded.is_none());
        assert!(app.world().resource::<SourceLocationState>().pending.is_none());
        assert_eq!(app.world().get::<EditableText>(entity).unwrap().pending_edits.len(), 1);
        assert!(app.world().resource::<SceneEditorState>().status.contains("确认中文输入"));
    }
}

#[test]
fn bound_verbatim_windows_root_is_retained_as_exact_source_identity() {
    if !cfg!(windows) { return; }
    let root = PathBuf::from(r"\\?\D:\source-location-owned-fixture");
    let paths = feathrise_editor_core::EditorProjectPaths::new(&root, root.join("assets")).unwrap();
    let mut catalog = EditorProjectFileCatalog::default();
    catalog.bind_project_source(0, Some(paths.project_root().to_path_buf()));
    let request = OpenProjectSourceLocation { context: 0, project_root: root, path: "src/main.rs".into(), line: 1, column: 1 };
    assert!(current(&request, &catalog));
}
