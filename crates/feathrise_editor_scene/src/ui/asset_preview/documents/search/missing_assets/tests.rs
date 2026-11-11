//! 真实临时资产检查缺失、路径边界、原文位置、草稿优先与取消身份，不写源资产。

use feathrise_editor_ui::UiInteraction;
use super::super::{
    components::*,
    constants::MAX_HITS,
    resources::AssetSearchState,
    tests::{Fixture, control_app, search},
    types::*,
};
use super::*;
use crate::{
    resources::*,
    ui::{
        asset_preview::documents::resources::DocumentState,
        sidebars::{EditorNavigatorTab, SceneEditorSidebarState},
    },
};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, block_on},
    text::{EditableText, TextEdit},
};

fn request(fixture: &Fixture) -> SearchRequest {
    let mut request = fixture.request("");
    request.references = true;
    request.missing_assets = true;
    request
}

fn settle(app: &mut App) {
    for _ in 0..3000 {
        app.update();
        let state = app.world().resource::<AssetSearchState>();
        if !state.reference_requested && state.task.is_none() && state.pending.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("缺失检查未完成");
}

#[test]
fn missing_assets_fields_arrays_and_subassets_keep_precise_unicode_locations() {
    let fixture = Fixture::new();
    fixture.write("gltf/可用.glb", "物理文件存在，不解析模型内部结构");
    let body = "{\n  \"model_path\": \"gltf/雪.glb#Scene0\",\n  \"texture_paths\": [\"gltf/可用.glb#Mesh0\", \"textures/雪.png\", \"\"],\n  \"说明\": \"gltf/不存在.glb\", \"height_path\": null\n}\n";
    fixture.write(
        "config/雪.json",
        format!("\u{feff}{}", body.replace('\n', "\r\n")),
    );
    fixture.write("notes/a.json", r#"{"path":"不在范围.glb"}"#);
    let before = std::fs::read(fixture.0.join("config/雪.json")).unwrap();
    let result = search(request(&fixture));
    assert!(result.references && result.missing_assets);
    assert_eq!(
        (result.scanned, result.checked_paths, result.hits),
        (1, 3, 2)
    );
    assert_eq!(result.files[0].path, "config/雪.json");
    let hit = &result.files[0].hits[0];
    assert_eq!((hit.line, hit.column), (2, 17));
    assert_eq!(&body[hit.range.clone()], "\"gltf/雪.glb#Scene0\"");
    assert!(
        hit.snippet
            .contains("文件缺失 · gltf/雪.glb#Scene0 · /model_path")
    );
    assert_eq!(
        std::fs::read(fixture.0.join("config/雪.json")).unwrap(),
        before
    );
    assert!(result.error.is_empty() && !result.limited && result.skipped == 0);
}

#[test]
fn missing_assets_json_escapes_use_decoded_path_and_original_string_range() {
    let fixture = Fixture::new();
    fixture.write("textures/雪.png", "存在");
    let body = r#"{"路径_path":"gltf/\u96ea.glb","图_paths":["textures/\u96ea.png"]}"#;
    fixture.write("scenes/雪.json", body);
    let result = search(request(&fixture));
    assert_eq!((result.checked_paths, result.hits), (2, 1));
    let hit = &result.files[0].hits[0];
    assert_eq!(&body[hit.range.clone()], r#""gltf/\u96ea.glb""#);
    assert!(hit.snippet.contains("gltf/雪.glb"));
    assert_eq!(hit.column, body[..hit.range.start].chars().count() + 1);
}

#[test]
fn missing_assets_invalid_json_depth_or_oversized_path_never_publish_partial_diagnostics() {
    let fixture = Fixture::new();
    fixture.write("config/bad.json", r#"{"path":"missing.glb", broken}"#);
    fixture.write(
        "config/deep.json",
        format!("{}0{}", "[".repeat(140), "]".repeat(140)),
    );
    fixture.write(
        "config/long.json",
        serde_json::json!({"path": "a".repeat(MAX_PATH_BYTES + 1)}).to_string(),
    );
    let result = search(request(&fixture));
    assert_eq!(
        (
            result.scanned,
            result.skipped,
            result.checked_paths,
            result.hits
        ),
        (3, 3, 0, 0)
    );
    assert!(result.files.is_empty());
}

#[test]
fn missing_assets_distinguish_missing_invalid_and_non_file_targets() {
    let fixture = Fixture::new();
    fixture.write("folder/exists.png", "存在");
    fixture.write("plain.txt", "文件不是目录");
    fixture.write(
        "config/paths.json",
        serde_json::json!({"asset_paths": [
            "../outside.png", "C:/outside.png", "gltf\\wrong.glb", "#Scene0",
            "folder", "plain.txt/child.png", "folder/gone.png"
        ]})
        .to_string(),
    );
    let result = search(request(&fixture));
    assert_eq!((result.checked_paths, result.hits), (7, 7));
    let hits = &result.files[0].hits;
    assert!(
        hits[..4]
            .iter()
            .all(|hit| hit.snippet.starts_with("路径无效"))
    );
    assert!(hits[4].snippet.starts_with("目标不是文件"));
    assert!(hits[5].snippet.starts_with("父路径不是目录"));
    assert!(hits[6].snippet.starts_with("文件缺失"));
    assert!(hits.iter().all(|hit| hit.snippet.chars().count() <= 202));
}

#[test]
fn missing_assets_confirmed_draft_wins_without_saving_or_changing_history() {
    let fixture = Fixture::new();
    fixture.write("ok.glb", "存在");
    fixture.write("config/draft.json", r#"{"path":"ok.glb"}"#);
    let mut doc = fixture.doc("config/draft.json");
    doc.edit(r#"{"path":"draft-only.glb"}"#);
    let (mut app, field) = control_app(&fixture);
    let source = doc.source.clone();
    app.world_mut()
        .resource_mut::<DocumentState>()
        .documents
        .push(doc);
    app.world_mut()
        .resource_mut::<AssetSearchState>()
        .request_missing_assets(fixture.0.clone());
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = true;
    app.update();
    assert!(
        app.world()
            .resource::<AssetSearchState>()
            .reference_requested
    );
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = false;
    app.world_mut()
        .get_mut::<EditableText>(field)
        .unwrap()
        .queue_edit(TextEdit::Insert("尚未确认".into()));
    app.update();
    assert!(
        app.world()
            .resource::<AssetSearchState>()
            .reference_requested
    );
    app.world_mut()
        .get_mut::<EditableText>(field)
        .unwrap()
        .pending_edits
        .clear();
    app.update();
    assert!(Arc::ptr_eq(
        &app.world()
            .resource::<AssetSearchState>()
            .running
            .as_ref()
            .unwrap()
            .drafts[0]
            .text,
        &source
    ));
    settle(&mut app);
    let state = app.world().resource::<AssetSearchState>();
    assert_eq!(state.current().unwrap().hits, 1);
    assert!(state.current().unwrap().files[0].dirty);
    assert!(state.hint.contains("路径问题") && !state.hint.contains("替换"));
    let docs = app.world().resource::<DocumentState>();
    assert!(Arc::ptr_eq(&docs.documents[0].source, &source) && docs.documents[0].dirty());
    assert!(docs.expanded.is_none());
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("config/draft.json")).unwrap(),
        r#"{"path":"ok.glb"}"#
    );
}

#[test]
fn missing_assets_result_caps_remain_locatable_and_cancellation_is_explicit() {
    let fixture = Fixture::new();
    fixture.write(
        "config/many.json",
        serde_json::json!({"asset_paths": vec!["missing.glb"; MAX_HITS + 1]}).to_string(),
    );
    let result = search(request(&fixture));
    assert!(result.limited);
    assert_eq!((result.hits, result.checked_paths), (MAX_HITS, MAX_HITS));
    assert_eq!(result.rows.len(), MAX_HITS + 1);
    let request = request(&fixture);
    request.cancel.store(true, Ordering::Relaxed);
    assert!(search(request).cancelled);
}

#[test]
fn missing_assets_path_collection_and_metadata_budgets_cannot_claim_complete_success() {
    let fixture = Fixture::new();
    fixture.write("ok.glb", "存在");
    fixture.write(
        "config/many.json",
        serde_json::json!({"asset_paths": vec!["ok.glb"; MAX_PATHS_PER_FILE + 1]}).to_string(),
    );
    let result = search(request(&fixture));
    assert!(result.limited && result.hits == 0);
    assert_eq!(result.checked_paths, MAX_PATHS_PER_FILE);
    fixture.write("folder/ok.glb", "存在");
    let root = fixture.0.clone();
    let result = block_on(IoTaskPool::get().spawn(async move {
        let (mut checked, mut metadata) = (0, MAX_METADATA_CHECKS - 1);
        let result = check_file(
            Arc::from(r#"{"path":"folder/ok.glb"}"#),
            &root,
            MAX_HITS,
            &mut checked,
            &mut metadata,
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap();
        (result, checked, metadata)
    }));
    assert!(result.0.0.is_empty() && result.0.1);
    assert_eq!((result.1, result.2), (0, MAX_METADATA_CHECKS));
}

#[test]
fn missing_assets_queued_close_and_root_change_require_an_explicit_restart() {
    for changed_root in [false, true] {
        let fixture = Fixture::new();
        let (mut app, _) = control_app(&fixture);
        app.world_mut()
            .resource_mut::<AssetSearchState>()
            .request_missing_assets(fixture.0.clone());
        if changed_root {
            app.world_mut().resource_mut::<DocumentState>().root = fixture.0.join("other");
        } else {
            app.world_mut().resource_mut::<SceneEditorState>().open = false;
        }
        app.update();
        let state = app.world().resource::<AssetSearchState>();
        assert!(!state.reference_requested && state.pending.is_none() && state.task.is_none());
        app.world_mut().resource_mut::<SceneEditorState>().open = true;
        app.update();
        assert!(
            app.world()
                .resource::<AssetSearchState>()
                .current()
                .is_none()
        );
    }
}

#[test]
fn missing_assets_latest_request_waits_for_cancelled_slot_and_draft_changes_invalidate() {
    let fixture = Fixture::new();
    fixture.write("config/a.json", r#"{"path":"gone.glb"}"#);
    let (mut app, _) = control_app(&fixture);
    let (release, receiver) = std::sync::mpsc::channel();
    let cancel = {
        let mut state = app.world_mut().resource_mut::<AssetSearchState>();
        state.ticket = 1;
        let old = fixture.request("旧文本搜索");
        let cancel = old.cancel.clone();
        state.running = Some(old);
        state.task = Some(IoTaskPool::get().spawn(async move {
            receiver.recv().unwrap();
            SearchResult {
                ticket: 1,
                ..default()
            }
        }));
        state.request_missing_assets(fixture.0.clone());
        cancel
    };
    app.update();
    assert!(cancel.load(Ordering::Relaxed));
    assert!(
        app.world().resource::<AssetSearchState>().task.is_some()
            && app.world().resource::<AssetSearchState>().pending.is_some()
    );
    release.send(()).unwrap();
    settle(&mut app);
    let ticket = app.world().resource::<AssetSearchState>().ticket;
    assert_eq!(
        app.world()
            .resource::<AssetSearchState>()
            .current()
            .unwrap()
            .hits,
        1
    );
    app.world_mut()
        .resource_mut::<EditorAssetCatalog>()
        .current_folder = "unrelated".into();
    app.update();
    assert_eq!(app.world().resource::<AssetSearchState>().ticket, ticket);
    let mut doc = fixture.doc("config/a.json");
    doc.edit(r#"{"path":"changed.glb"}"#);
    app.world_mut()
        .resource_mut::<DocumentState>()
        .documents
        .push(doc);
    app.update();
    assert!(
        app.world()
            .resource::<AssetSearchState>()
            .current()
            .is_none()
    );
}

#[test]
fn missing_assets_header_opens_from_collapsed_and_text_shortcut_restores_preferences() {
    let fixture = Fixture::new();
    fixture.write("config/a.json", r#"{"path":"gone.glb"}"#);
    let (mut app, field) = control_app(&fixture);
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .navigator = EditorNavigatorTab::Assets;
    let header = app.world_mut()
        .spawn((SearchAction::MissingAssets, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
        .id();
    app.update();
    assert!(
        app.world().resource::<AssetSearchState>().open
            && app.world().resource::<AssetSearchState>().missing_assets
    );
    settle(&mut app);
    assert_eq!(
        app.world()
            .resource::<AssetSearchState>()
            .current()
            .unwrap()
            .hits,
        1
    );
    app.world_mut().entity_mut(header).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    let state = app.world().resource::<AssetSearchState>();
    assert!(!state.open && state.current().is_none());
    app.world_mut().entity_mut(header).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    settle(&mut app);
    assert_eq!(app.world().resource::<AssetSearchState>().current().unwrap().hits, 1);
    {
        let mut state = app.world_mut().resource_mut::<AssetSearchState>();
        state.filter = "json".into();
        state.case_sensitive = false;
        state.whole_word = true;
    }
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::KeyF);
    }
    app.update();
    let state = app.world().resource::<AssetSearchState>();
    assert!(!state.path_query() && !state.missing_assets && state.reference_target.is_none());
    assert_eq!(state.filter, "json");
    assert!(!state.case_sensitive && state.whole_word);
    assert_eq!(
        app.world()
            .resource::<bevy::input_focus::InputFocus>()
            .get(),
        Some(field)
    );
}
