//! 引用字段、真实原文范围、子资源、JSON失败及有限后台扫描回归。

use feathrise_editor_ui::UiInteraction;
use super::super::super::resources::DocumentState;
use super::super::{
    components::SearchAction,
    resources::AssetSearchState,
    tests::{Fixture, control_app, search},
    types::*,
};
use super::*;
use crate::resources::{EditorTextInputGuard, SceneEditorState};
use bevy::{
    input_focus::InputFocus,
    prelude::*,
    tasks::IoTaskPool,
    text::{EditableText, TextEdit},
};
use std::sync::{Arc, atomic::AtomicBool};

fn hits(source: &str, path: &str) -> Vec<Hit> {
    match_json(source, path, 1024, &AtomicBool::new(false)).unwrap()
}

#[test]
fn references_exact_fields_subassets_and_json_pointer() {
    let source = r#"{"presets":[{"asset_path":"gltf/巨剑.glb","runtime":{"mesh_path":"gltf/巨剑.glb#Mesh0/Primitive0"},"label":"gltf/巨剑.glb","model_path":"gltf/巨剑.glb.bak","paths":["gltf/巨剑.glb"]}],"texture_paths":["gltf/巨剑.glb",null],"a/~_path":"gltf/巨剑.glb","number":3.14,"bool":false}"#;
    let result = hits(source, "gltf/巨剑.glb");
    assert_eq!(result.len(), 4);
    assert_eq!(result[0].snippet, "/presets/0/asset_path");
    assert_eq!(result[1].snippet, "/presets/0/runtime/mesh_path");
    assert_eq!(result[2].snippet, "/texture_paths/0");
    assert_eq!(result[3].snippet, "/a~1~0_path");
    for hit in result {
        let raw = &source[hit.range.clone()];
        let value: String = serde_json::from_str(raw).unwrap();
        assert_eq!(value.split('#').next(), Some("gltf/巨剑.glb"));
        assert_eq!(hit.line, 1);
        assert_eq!(hit.column, source[..hit.range.start].chars().count() + 1);
    }
}

#[test]
fn references_escapes_duplicates_unicode_and_real_lines() {
    let source = "{\n  \"中文\": 1, \"pa\\u0074h\": \"gltf\\/\\u5de8\\u5251.glb\",\n  \"path\" :\t\"gltf/巨剑.glb\", \"emoji_path\":\"gltf/\\ud83d\\ude00.glb\"\n}";
    let result = hits(source, "gltf/巨剑.glb");
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].line, 2);
    assert_eq!(result[0].column, 25);
    assert_eq!(
        &source[result[0].range.clone()],
        r#""gltf\/\u5de8\u5251.glb""#
    );
    assert_eq!((result[1].line, result[1].column), (3, 12));
    let emoji = hits(source, "gltf/😀.glb");
    assert_eq!(emoji.len(), 1);
    assert_eq!(emoji[0].snippet, "/emoji_path");
    assert!(hits(source, "gltf/巨剑.gl").is_empty());
    assert!(hits(source, "GLTF/巨剑.glb").is_empty());
}

#[test]
fn references_invalid_json_is_atomic_and_cancellable() {
    let cancel = AtomicBool::new(false);
    for source in [
        r#"{"path":"a.glb","bad":}"#,
        r#"{"path":"a.glb"} trailing"#,
        r#"{"path":"a.glb","path":"\ud800"}"#,
    ] {
        assert!(match_json(source, "a.glb", 1, &cancel).is_err());
    }
    let deep = format!("{}0{}", "[".repeat(150), "]".repeat(150));
    assert!(match_json(&deep, "a.glb", 1, &cancel).is_err());
    let long = format!("{{\"{}_path\":\"a.glb\"}}", "x".repeat(1024));
    assert!(match_json(&long, "a.glb", 1, &cancel).is_err());
    let source = r#"{"path":"a.glb","path":"a.glb"}"#;
    assert_eq!(match_json(source, "a.glb", 1, &cancel).unwrap().len(), 1);
    assert!(match_json(source, "a.glb", 1, &AtomicBool::new(true)).is_err());
    let large = format!("{}{{\"path\":\"a.glb\"}}", " ".repeat(256 * 1024));
    assert!(match_json(&large, "a.glb", 1, &cancel).is_err());
}

#[test]
fn references_worker_scans_config_scenes_only_and_drafts_win() {
    let fixture = Fixture::new();
    fixture.write(
        "config/equipment.json",
        "\u{feff}{\r\n  \"model_path\": \"gltf/巨剑.glb\"\r\n}",
    );
    fixture.write(
        "scenes/HN-01/map.manifest.json",
        r#"{"terrain":{"glb_path":"gltf/巨剑.glb#Scene0"}}"#,
    );
    fixture.write("config/draft.json", r#"{"asset_path":"other.glb"}"#);
    fixture.write("config/invalid.json", r#"{"path":"gltf/巨剑.glb",bad}"#);
    fixture.write("config/data.txt", r#"{"path":"gltf/巨剑.glb"}"#);
    fixture.write("config-other/data.json", r#"{"path":"gltf/巨剑.glb"}"#);
    fixture.write("gltf/data.json", r#"{"path":"gltf/巨剑.glb"}"#);
    let doc = fixture.doc("config/draft.json");
    let mut request = fixture.request("gltf/巨剑.glb");
    request.references = true;
    request.drafts.push(Draft {
        stamp: DraftStamp {
            key: doc.source_key(),
            revision: 3,
        },
        text: Arc::from(r#"{"asset_path":"gltf/巨剑.glb"}"#),
        dirty: true,
    });
    let result = search(request);
    assert!(result.references);
    assert_eq!((result.scanned, result.skipped, result.hits), (4, 1, 3));
    assert_eq!(result.files.len(), 3);
    assert!(result.files[0].dirty);
    assert_eq!(result.files[1].hits[0].line, 2);
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("config/draft.json")).unwrap(),
        r#"{"asset_path":"other.glb"}"#
    );
}

#[test]
fn references_targets_are_physical_asset_paths() {
    for path in ["gltf/中文.glb", "scenes/MAP-1/terrain/height.png"] {
        assert!(valid_target(path));
    }
    for path in [
        "",
        "../a.glb",
        "/a.glb",
        "C:/a.glb",
        "gltf\\a.glb",
        "gltf/a.glb#Scene0",
    ] {
        assert!(!valid_target(path));
    }
}

fn settle(app: &mut App) {
    for _ in 0..2000 {
        app.update();
        let state = app.world().resource::<AssetSearchState>();
        if !state.reference_requested && state.task.is_none() && state.pending.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("引用扫描未结束");
}

#[test]
fn references_request_waits_for_confirmed_input_and_keeps_document_and_disk() {
    let fixture = Fixture::new();
    fixture.write("config/草稿.json", r#"{"asset_path":"old.glb"}"#);
    let (mut app, field) = control_app(&fixture);
    let mut doc = fixture.doc("config/草稿.json");
    doc.edit(r#"{"asset_path":"gltf/剑.glb"}"#);
    let source = doc.source.clone();
    app.world_mut()
        .resource_mut::<DocumentState>()
        .documents
        .push(doc);
    app.world_mut()
        .resource_mut::<AssetSearchState>()
        .request_references("gltf/剑.glb".into(), fixture.0.clone());
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = true;
    app.update();
    assert!(
        app.world()
            .resource::<AssetSearchState>()
            .reference_requested
    );
    assert!(app.world().resource::<AssetSearchState>().task.is_none());
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = false;
    app.world_mut()
        .get_mut::<EditableText>(field)
        .unwrap()
        .queue_edit(TextEdit::Insert("候选尚待采用".into()));
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
    let state = app.world().resource::<AssetSearchState>();
    assert!(Arc::ptr_eq(
        &state.running.as_ref().unwrap().drafts[0].text,
        &source
    ));
    settle(&mut app);
    let state = app.world().resource::<AssetSearchState>();
    assert_eq!(state.current().unwrap().hits, 1);
    assert!(state.current().unwrap().files[0].dirty);
    assert!(state.hint.contains("直接引用") && !state.hint.contains("替换"));
    let docs = app.world().resource::<DocumentState>();
    assert!(docs.documents[0].dirty());
    assert!(Arc::ptr_eq(&docs.documents[0].source, &source));
    assert!(docs.expanded.is_none());
    assert!(!app.world().resource::<SceneEditorState>().save_requested);
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("config/草稿.json")).unwrap(),
        r#"{"asset_path":"old.glb"}"#
    );
}

#[test]
fn references_queued_close_or_changed_root_cancels_without_automatic_resume() {
    for changed_root in [false, true] {
        let fixture = Fixture::new();
        let (mut app, _) = control_app(&fixture);
        app.world_mut()
            .resource_mut::<AssetSearchState>()
            .request_references("a.glb".into(), fixture.0.clone());
        if changed_root {
            app.world_mut().resource_mut::<DocumentState>().root = fixture.0.join("other");
        } else {
            app.world_mut().resource_mut::<SceneEditorState>().open = false;
        }
        app.update();
        let state = app.world().resource::<AssetSearchState>();
        assert!(!state.reference_requested && state.task.is_none() && state.pending.is_none());
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
fn references_ctrl_shift_f_restores_text_search_and_preserves_user_options() {
    let fixture = Fixture::new();
    let (mut app, field) = control_app(&fixture);
    {
        let mut state = app.world_mut().resource_mut::<AssetSearchState>();
        state.query = "长剑".into();
        state.filter = "json,rs".into();
        state.current_folder = true;
        state.whole_word = true;
        state.case_sensitive = false;
        state.request_references("gltf/剑.glb".into(), fixture.0.clone());
    }
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::KeyF);
    }
    app.update();
    let state = app.world().resource::<AssetSearchState>();
    assert!(
        state.reference_target.is_none() && !state.reference_requested && state.pending.is_none()
    );
    assert_eq!(
        (&state.query, &state.filter),
        (&"长剑".to_string(), &"json,rs".to_string())
    );
    assert!(state.current_folder && state.whole_word && !state.case_sensitive);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
}

#[test]
fn references_latest_target_waits_for_the_cancelled_slot_and_folder_changes_do_not_invalidate() {
    let fixture = Fixture::new();
    fixture.write("config/data.json", r#"{"asset_path":"b.glb"}"#);
    let (mut app, _) = control_app(&fixture);
    let (release, receiver) = std::sync::mpsc::channel();
    let old = fixture.request("旧搜索");
    let cancel = old.cancel.clone();
    {
        let mut state = app.world_mut().resource_mut::<AssetSearchState>();
        state.open = true;
        state.ticket = 1;
        state.current_folder = true;
        state.running = Some(old);
        state.task = Some(IoTaskPool::get().spawn(async move {
            receiver.recv().unwrap();
            SearchResult {
                ticket: 1,
                ..default()
            }
        }));
        state.request_references("a.glb".into(), fixture.0.clone());
        state.request_references("b.glb".into(), fixture.0.clone());
    }
    app.update();
    assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
    let state = app.world().resource::<AssetSearchState>();
    assert!(state.task.is_some());
    assert_eq!(state.pending.as_ref().unwrap().query, "b.glb");
    release.send(()).unwrap();
    settle(&mut app);
    app.world_mut()
        .resource_mut::<crate::resources::EditorAssetCatalog>()
        .current_folder = "gltf".into();
    app.update();
    assert_eq!(
        app.world()
            .resource::<AssetSearchState>()
            .current()
            .unwrap()
            .hits,
        1
    );
}

#[test]
fn references_external_location_uses_available_ide_and_exact_position_without_internal_jump() {
    use crate::ui::external_files::{ExternalEditor, ExternalEditors, OpenExternalFile};
    let fixture = Fixture::new();
    fixture.write(
        "config/中文配置.json",
        "{\n  \"备注\":\"中文\",\n  \"model_path\": \"gltf/剑.glb\"\n}",
    );
    for (ide, in_project) in ExternalEditor::ALL.into_iter().flat_map(|ide| [false, true].map(|in_project| (ide, in_project))) {
        let mut request = fixture.request("gltf/剑.glb");
        request.references = true;
        let result = search(request);
        assert_eq!(
            (result.files[0].hits[0].line, result.files[0].hits[0].column),
            (3, 17)
        );
        let (mut app, _) = control_app(&fixture);
        app.insert_resource(ExternalEditors::for_tests(&[ide]));
        {
            let mut state = app.world_mut().resource_mut::<AssetSearchState>();
            state.open = true;
            state.ticket = 1;
            state.reference_target = Some("gltf/剑.glb".into());
            state.result = Some(Arc::new(result));
        }
        let button = app
            .world_mut()
            .spawn((
                SearchAction::External {
                    ticket: 1,
                    index: 1,
                    editor: ide,
                    in_project,
                },
                feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,
            ))
            .id();
        app.update();
        let requests: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<OpenExternalFile>>()
            .drain()
            .collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "config/中文配置.json");
        assert_eq!(requests[0].action.editor(), Some(ide));
        assert_eq!(requests[0].action.in_project(), in_project);
        assert_eq!(requests[0].action.location(), Some((3, 17)));
        assert!(app.world().resource::<DocumentState>().expanded.is_none());
        assert!(
            app.world()
                .resource::<SceneEditorState>()
                .selected_asset
                .is_none()
        );
        *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
        app.update();
        app.insert_resource(ExternalEditors::default());
        *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
        app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
        app.update();
        assert!(
            app.world()
                .resource::<Messages<OpenExternalFile>>()
                .is_empty()
        );
    }
}

#[test]
fn references_results_are_limited_locatable_and_invalidated_when_the_draft_changes() {
    let fixture = Fixture::new();
    fixture.write(
        "config/large.json",
        format!(
            "{{\"asset_paths\":[{}]}}",
            vec!["\"a.glb\""; 1030].join(",")
        ),
    );
    let (mut app, _) = control_app(&fixture);
    app.world_mut()
        .resource_mut::<DocumentState>()
        .documents
        .push(fixture.doc("config/large.json"));
    app.world_mut()
        .resource_mut::<AssetSearchState>()
        .request_references("a.glb".into(), fixture.0.clone());
    settle(&mut app);
    let state = app.world().resource::<AssetSearchState>();
    assert_eq!(state.current().unwrap().hits, 1024);
    assert!(state.current().unwrap().limited);
    assert!(state.hint.contains("仍可定位") && !state.hint.contains("替换"));
    app.world_mut().resource_mut::<DocumentState>().documents[0].edit(r#"{"asset_path":"b.glb"}"#);
    app.update();
    assert!(
        app.world()
            .resource::<AssetSearchState>()
            .current()
            .is_none()
    );
    assert!(app.world().resource::<DocumentState>().documents[0].dirty());
}
