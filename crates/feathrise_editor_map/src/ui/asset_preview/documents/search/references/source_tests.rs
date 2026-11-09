//! Rust路径、真实Unicode位置、项目范围和外部定位回归；不启动用户IDE、不改生产资产。

use feathrise_editor_ui::UiInteraction;
use super::super::super::resources::DocumentState;
use super::super::{
    components::SearchAction,
    resources::AssetSearchState,
    tests::{Fixture, control_app, search},
    types::*,
};
use super::source_scanner::match_rust;
use crate::{
    resources::MapEditorState,
    ui::external_files::{ExternalEditor, ExternalEditors, ExternalFileAction, OpenExternalFile},
};
use bevy::prelude::*;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

#[test]
fn references_rust_literals_decode_escapes_raw_strings_and_exclude_comments() {
    let source = r###"
// "gltf/巨剑.glb"
/* "gltf/巨剑.glb" /* "gltf/巨剑.glb" */ */
/// gltf/巨剑.glb
//! gltf/巨剑.glb
#[doc = "gltf/巨剑.glb"]
fn 加载() {
    let 普通 = "gltf/巨剑.glb";
    let 原文 = r##"gltf/巨剑.glb#Mesh0/Primitive0"##;
    let 转义 = "gltf/\u{5de8}\u{5251}.glb";
    let 不完整 = "巨剑.glb";
    let 后缀 = "gltf/巨剑.glb.bak";
    let 备注 = "备注：gltf/巨剑.glb";
    let 字节 = b"gltf/\xe5\xb7\xa8\xe5\x89\x91.glb";
}
"###;
    let result = match_rust(source, "gltf/巨剑.glb", 1024, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.len(), 3);
    assert_eq!(
        result.iter().map(|hit| hit.line).collect::<Vec<_>>(),
        [8, 9, 10]
    );
    for hit in result {
        let value: syn::LitStr = syn::parse_str(&source[hit.range.clone()]).unwrap();
        assert_eq!(value.value().split('#').next(), Some("gltf/巨剑.glb"));
        assert_eq!(
            hit.column,
            source[..hit.range.start]
                .rsplit('\n')
                .next()
                .unwrap()
                .chars()
                .count()
                + 1
        );
        assert!(hit.snippet.contains('‹'));
    }
    assert!(
        match_rust(source, "GLTF/巨剑.glb", 1024, &AtomicBool::new(false))
            .unwrap()
            .is_empty()
    );
    // 重复解析会重置线程SourceMap，第二次原文位置必须仍然相同。
    assert_eq!(
        match_rust(source, "gltf/巨剑.glb", 1024, &AtomicBool::new(false)).unwrap()[0].line,
        8
    );
}

#[test]
fn references_rust_lexical_failure_cancel_depth_and_hit_budgets_are_atomic() {
    let cancel = AtomicBool::new(false);
    assert!(match_rust("let x = \"a.glb\"; let bad = \"", "a.glb", 1, &cancel).is_err());
    assert!(match_rust("let x = \"a.glb\";", "a.glb", 1, &AtomicBool::new(true)).is_err());
    let deep = format!("{}0{}", "(".repeat(129), ")".repeat(129));
    assert!(match_rust(&deep, "a.glb", 1, &cancel).is_err());
    assert_eq!(
        match_rust("let x = [\"a.glb\", \"a.glb\"];", "a.glb", 1, &cancel)
            .unwrap()
            .len(),
        1
    );
    assert!(match_rust(&" ".repeat(256 * 1024 + 1), "a.glb", 1, &cancel).is_err());
}

#[test]
fn references_worker_combines_json_and_current_project_sources_without_entering_build_assets() {
    let fixture = Fixture::new();
    fixture.write("Cargo.toml", "[workspace]\n");
    fixture.write(
        "assets/config/player.json",
        r#"{"model_path":"gltf/蓝衣.glb"}"#,
    );
    let source = "\u{feff}// 主角模板\r\nconst 身体: &str = \"gltf/蓝衣.glb\";\r\n";
    fixture.write("crates/game/src/player.rs", source);
    fixture.write(
        "build.rs",
        "const BODY: &str = r\"gltf/蓝衣.glb#Scene0\";\n",
    );
    fixture.write("apps/game/src/comments.rs", "// \"gltf/蓝衣.glb\"\n");
    fixture.write("apps/game/src/invalid.rs", "let invalid = \"");
    for path in [
        "assets/source.rs",
        "crates/game/target/source.rs",
        "crates/game/assets/source.rs",
        "tools/source.rs",
    ] {
        fixture.write(path, "const BODY: &str = \"gltf/蓝衣.glb\";\n");
    }
    let mut request = fixture.request("gltf/蓝衣.glb");
    request.root = fixture.0.join("assets");
    request.references = true;
    let result = search(request);
    assert_eq!(
        (
            result.hits,
            result.files.len(),
            result.scanned,
            result.skipped
        ),
        (3, 3, 5, 1)
    );
    let source_file = result
        .files
        .iter()
        .find(|file| file.path.ends_with("player.rs"))
        .unwrap();
    assert!(matches!(
        source_file.identity,
        ContentIdentity::ProjectSource(_)
    ));
    assert_eq!(source_file.hits[0].line, 2);
    let normalized = source.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    assert_eq!(
        &normalized[source_file.hits[0].range.clone()],
        "\"gltf/蓝衣.glb\""
    );
    assert_eq!(
        std::fs::read(fixture.0.join("crates/game/src/player.rs")).unwrap(),
        source.as_bytes()
    );
}

fn source_result(root: PathBuf) -> SearchResult {
    SearchResult {
        ticket: 1,
        references: true,
        root,
        hits: 1,
        files: vec![FileHits {
            path: "crates/game/src/player.rs".into(),
            identity: ContentIdentity::ProjectSource([7; 32]),
            dirty: false,
            hits: vec![Hit {
                range: 10..22,
                line: 28,
                column: 34,
                snippet: "model_path".into(),
            }],
        }],
        rows: vec![ResultRow::File(0), ResultRow::Hit(0, 0)],
        ..default()
    }
}

#[test]
fn references_source_editor_choice_preserves_results_and_uses_each_installed_editor() {
    use crate::ui::sidebars::{EditorNavigatorTab, MapEditorSidebarState};
    let fixture = Fixture::new();
    let (mut app, _) = control_app(&fixture);
    app.world_mut().resource_mut::<MapEditorSidebarState>().navigator = EditorNavigatorTab::Assets;
    app.insert_resource(ExternalEditors::for_tests(&ExternalEditor::ALL));
    let result = Arc::new(source_result(fixture.0.clone()));
    {
        let mut state = app.world_mut().resource_mut::<AssetSearchState>();
        state.ticket = 1; state.open = true;
        state.reference_target = Some("gltf/蓝衣.glb".into());
        state.result = Some(result.clone());
    }
    let row = app.world_mut().spawn((SearchAction::Row { ticket: 1, index: 1 }, UiInteraction::None)).id();
    for ide in ExternalEditor::ALL {
        let choice = app.world_mut().spawn((SearchAction::ChooseEditor(ide), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed)).id();
        app.update();
        assert!(app.world().resource::<Messages<OpenExternalFile>>().is_empty());
        let state = app.world().resource::<AssetSearchState>();
        assert_eq!(state.preferred_editor, Some(ide));
        assert_eq!(state.ticket, 1);
        assert!(Arc::ptr_eq(state.result.as_ref().unwrap(), &result));
        assert!(state.pending.is_none() && state.task.is_none());
        *app.world_mut().get_mut::<UiInteraction>(choice).unwrap() = UiInteraction::None;
        *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::Pressed;
        app.world_mut().entity_mut(row).insert(feathrise_editor_ui::EditorButtonClick::triggered());
        app.update();
        let requests: Vec<_> = app.world_mut().resource_mut::<Messages<OpenExternalFile>>().drain().collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].action, ExternalFileAction::ProjectSource { editor: ide, line: 28, column: 34, digest: [7; 32], in_project: true });
        assert!(!requests[0].has_draft);
        *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::None;
        app.update();
        for in_project in [false, true] {
            let button = app.world_mut().spawn((SearchAction::External { ticket: 1, index: 1, editor: ide, in_project }, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed)).id();
            app.update();
            let requests: Vec<_> = app.world_mut().resource_mut::<Messages<OpenExternalFile>>().drain().collect();
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].path, "crates/game/src/player.rs");
            assert_eq!(requests[0].action, ExternalFileAction::ProjectSource { editor: ide, line: 28, column: 34, digest: [7; 32], in_project });
            assert_eq!(requests[0].action.in_project(), in_project);
            assert!(!requests[0].has_draft);
            app.world_mut().despawn(button);
            app.update();
        }
    }
    app.world_mut().resource_mut::<AssetSearchState>().text_search();
    assert_eq!(app.world().resource::<AssetSearchState>().preferred_editor, Some(ExternalEditor::RustRover));
    app.insert_resource(ExternalEditors::for_tests(&[ExternalEditor::Zed]));
    assert_eq!(app.world().resource::<AssetSearchState>().selected_editor(app.world().resource::<ExternalEditors>()), Some(ExternalEditor::Zed));
    app.world_mut().spawn((SearchAction::ChooseEditor(ExternalEditor::VsCode), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    app.update();
    assert_eq!(app.world().resource::<AssetSearchState>().preferred_editor, Some(ExternalEditor::RustRover));
    app.insert_resource(ExternalEditors::default());
    assert_eq!(app.world().resource::<AssetSearchState>().selected_editor(app.world().resource::<ExternalEditors>()), None);
    assert!(app.world().resource::<Messages<OpenExternalFile>>().is_empty());
    assert!(app.world().resource::<DocumentState>().documents.is_empty());
    assert!(app.world().resource::<MapEditorState>().selected_asset.is_none());
}

#[test]
fn references_source_rows_use_available_ide_and_never_create_an_asset_draft() {
    for (installed, expected) in [
        (vec![], None),
        (
            vec![ExternalEditor::Zed, ExternalEditor::RustRover],
            Some(ExternalEditor::Zed),
        ),
        (ExternalEditor::ALL.to_vec(), Some(ExternalEditor::VsCode)),
    ] {
        let fixture = Fixture::new();
        let (mut app, _) = control_app(&fixture);
        app.insert_resource(ExternalEditors::for_tests(&installed));
        {
            let mut state = app.world_mut().resource_mut::<AssetSearchState>();
            state.ticket = 1;
            state.open = true;
            state.reference_target = Some("gltf/蓝衣.glb".into());
            state.result = Some(Arc::new(source_result(fixture.0.clone())));
        }
        let button = app
            .world_mut()
            .spawn((
                SearchAction::Row {
                    ticket: 1,
                    index: 1,
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
        if let Some(ide) = expected {
            assert_eq!(requests.len(), 1);
            assert_eq!(requests[0].path, "crates/game/src/player.rs");
            assert_eq!(
                requests[0].action,
                ExternalFileAction::ProjectSource {
                    editor: ide,
                    line: 28,
                    column: 34,
                    digest: [7; 32],
                    in_project: true,
                }
            );
            assert!(!requests[0].has_draft);
            *app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::None;
            app.update();
            app.world_mut().spawn((
                SearchAction::External {
                    ticket: 1,
                    index: 1,
                    editor: ide,
                    in_project: true,
                },
                feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed,
            ));
            app.update();
            let request = app
                .world_mut()
                .resource_mut::<Messages<OpenExternalFile>>()
                .drain()
                .next()
                .unwrap();
            assert_eq!(request.action, requests[0].action);
        } else {
            assert!(requests.is_empty());
            assert!(
                app.world()
                    .resource::<AssetSearchState>()
                    .hint
                    .contains("未检测到")
            );
        }
        assert!(app.world().resource::<DocumentState>().documents.is_empty());
        assert!(app.world().resource::<DocumentState>().expanded.is_none());
        assert!(app.world().resource::<AssetSearchState>().jump.is_none());
        assert!(
            app.world()
                .resource::<MapEditorState>()
                .selected_asset
                .is_none()
        );
    }
}
