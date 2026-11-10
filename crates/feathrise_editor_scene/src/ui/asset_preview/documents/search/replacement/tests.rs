//! 真实文件的跨文件保真替换、原子拒收、容量与历史；同步等待只在测试线程中使用。

use feathrise_editor_ui::UiInteraction;
use super::super::super::{
    constants::MAX_FILE_BYTES,
    history::Selection,
    resources::{DocumentInputs, DocumentState},
    types::ExpandedDocument,
};
use super::super::{
    resources::AssetSearchState,
    tests::{Fixture, search},
    types::{Draft, DraftStamp},
};
use super::{components::*, control, resources::AssetReplaceState, tasks, types::*, worker};
use crate::{
    resources::*,
    ui::{
        sidebars::{EditorNavigatorTab, SceneEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::{
    ecs::system::RunSystemOnce,
    input_focus::InputFocus,
    prelude::*,
    tasks::{IoTaskPool, block_on},
    text::{EditableText, TextEdit},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

fn work(fixture: &Fixture, documents: &DocumentState, replacement: &str) -> Request {
    let mut request = fixture.request("长剑");
    request.drafts = documents
        .documents
        .iter()
        .map(|doc| Draft {
            stamp: DraftStamp {
                key: doc.source_key(),
                revision: doc.revision,
            },
            text: doc.source.clone(),
            dirty: doc.dirty(),
        })
        .collect();
    let result = Arc::new(search(request));
    Request {
        ticket: 1,
        cancel: Arc::new(AtomicBool::new(false)),
        work: Work::Preview {
            checked: (0..result.files.len()).collect(),
            search: result,
            query: "长剑".into(),
            replacement: replacement.into(),
            case_sensitive: true,
            whole_word: false,
            drafts: documents
                .documents
                .iter()
                .map(|doc| DraftInput {
                    stamp: DraftStamp {
                        key: doc.source_key(),
                        revision: doc.revision,
                    },
                    disk: doc.disk.clone(),
                    source: doc.source.clone(),
                    pages: doc.pages.clone(),
                    selection: Selection {
                        anchor: 6,
                        focus: 0,
                    },
                })
                .collect(),
        },
    }
}
fn run(request: Request) -> Outcome {
    block_on(IoTaskPool::get().spawn(worker::run(request, None, None, None))).outcome
}
fn preview(request: Request) -> Arc<ReplacePlan> {
    let Outcome::Preview(result) = run(request) else {
        panic!("预览回复")
    };
    result.unwrap()
}
fn apply(plan: Arc<ReplacePlan>, target: Target) -> Result<Vec<AppliedFile>, String> {
    let Outcome::Apply { content, .. } = run(Request {
        ticket: 2,
        cancel: Arc::new(AtomicBool::new(false)),
        work: Work::Apply { plan, target },
    }) else {
        panic!("应用回复")
    };
    content
}
fn documents(fixture: &Fixture, paths: &[&str]) -> DocumentState {
    DocumentState {
        root: fixture.0.clone(),
        documents: paths.iter().map(|path| fixture.doc(path)).collect(),
        ..default()
    }
}
fn basic() -> Fixture {
    let fixture = Fixture::new();
    fixture.write("a.txt", "长剑\r\n第二把长剑\n尾部\r\n");
    fixture.write("b.json", "{\"武器\":\"长剑\"}");
    fixture
}

#[test]
fn references_read_only_guard_rejects_replace_worker_and_stale_ui_actions() {
    let fixture = basic();
    let docs = documents(&fixture, &["a.txt"]);
    let mut request = work(&fixture, &docs, "短剑");
    let Work::Preview { search, .. } = &mut request.work else { unreachable!() };
    Arc::get_mut(search).unwrap().references = true;
    let Outcome::Preview(Err(error)) = run(request) else { panic!("引用结果必须拒绝替换") };
    assert!(error.contains("只读"));
    let mut app = app(&fixture);
    app.world_mut().resource_mut::<AssetSearchState>().request_references("gltf/剑.glb".into(), fixture.0.clone());
    {
        let mut state = app.world_mut().resource_mut::<AssetReplaceState>();
        state.tab_open = true; state.active = true;
    }
    app.world_mut().spawn((ReplaceAction::Open, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    app.world_mut().spawn((ReplaceAction::ApplyChecked, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.press(KeyCode::ControlLeft); keys.press(KeyCode::ShiftLeft); keys.press(KeyCode::KeyH);
    app.world_mut().run_system_once(control::input).unwrap();
    app.world_mut().run_system_once(tasks::drive).unwrap();
    let state = app.world().resource::<AssetReplaceState>();
    assert!(!state.active && state.pending.is_none() && state.task.is_none());
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::KeyH));
    assert!(app.world().resource::<DocumentState>().documents.is_empty());
    assert_eq!(std::fs::read_to_string(fixture.0.join("a.txt")).unwrap(), "长剑\r\n第二把长剑\n尾部\r\n");
}

#[test]
fn missing_assets_read_only_guard_rejects_replace_worker_shortcut_and_stale_buttons() {
    let fixture = basic();
    let docs = documents(&fixture, &["a.txt"]);
    let mut request = work(&fixture, &docs, "短剑");
    let Work::Preview { search, .. } = &mut request.work else { unreachable!() };
    let result = Arc::get_mut(search).unwrap();
    result.missing_assets = true; // 即便只读标记异常遗漏，后台仍不能把缺失结果当文字替换。
    let Outcome::Preview(Err(error)) = run(request) else { panic!("缺失检查必须拒绝替换") };
    assert!(error.contains("只读"));
    let mut app = app(&fixture);
    app.world_mut().resource_mut::<AssetSearchState>().request_missing_assets(fixture.0.clone());
    {
        let mut state = app.world_mut().resource_mut::<AssetReplaceState>();
        state.tab_open = true; state.active = true;
    }
    app.world_mut().spawn((ReplaceAction::Open, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    app.world_mut().spawn((ReplaceAction::ApplyChecked, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    keys.press(KeyCode::ControlLeft); keys.press(KeyCode::ShiftLeft); keys.press(KeyCode::KeyH);
    app.world_mut().run_system_once(control::input).unwrap();
    app.world_mut().run_system_once(tasks::drive).unwrap();
    let state = app.world().resource::<AssetReplaceState>();
    assert!(!state.active && state.pending.is_none() && state.task.is_none());
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::KeyH));
    assert!(app.world().resource::<DocumentState>().documents.is_empty());
}

#[test]
fn cross_replacement_previews_exact_unicode_and_preserves_bom_mixed_line_endings_without_writing() {
    let fixture = basic();
    fixture.write("a.txt", "\u{feff}长剑\r\n第二把长剑\n尾部\r\n");
    let docs = documents(&fixture, &["a.txt"]);
    let plan = preview(work(&fixture, &docs, "$1\\短剑"));
    assert_eq!(plan.files.len(), 2);
    let content = plan.files[0].content.as_ref().unwrap();
    assert_eq!(
        content.pages.concat(),
        "$1\\短剑\r\n第二把$1\\短剑\n尾部\r\n"
    );
    assert!(plan.files[0].disk.bom);
    assert_eq!(plan.search.files[0].hits[1].line, 2);
    assert_eq!(plan.search.files[0].hits[1].column, 4);
    assert!(plan.files[0].after[0].contains("‹$1\\短剑›"));
    assert_eq!(
        std::fs::read_to_string(fixture.0.join("a.txt")).unwrap(),
        "\u{feff}长剑\r\n第二把长剑\n尾部\r\n"
    );
    assert!(!docs.documents[0].dirty());
}

#[test]
fn cross_replacement_hit_file_and_checked_targets_are_distinct_and_each_is_one_history_step() {
    for (target, a, b) in [
        (
            Target::Hit(0, 1),
            "长剑\n第二把短剑\n尾部\n",
            "{\"武器\":\"长剑\"}",
        ),
        (
            Target::File(0),
            "短剑\n第二把短剑\n尾部\n",
            "{\"武器\":\"长剑\"}",
        ),
        (
            Target::Checked,
            "短剑\n第二把短剑\n尾部\n",
            "{\"武器\":\"短剑\"}",
        ),
    ] {
        let fixture = basic();
        let mut docs = documents(&fixture, &["a.txt", "b.json"]);
        let plan = preview(work(&fixture, &docs, "短剑"));
        let content = apply(plan.clone(), target).unwrap();
        let mut batch = tasks::adopt(&plan, content, &mut docs)
            .ok()
            .unwrap()
            .unwrap();
        assert_eq!(docs.documents[0].source_text(), a);
        assert_eq!(docs.documents[1].source_text(), b);
        tasks::travel(&mut batch, &mut docs, false).unwrap();
        assert_eq!(docs.documents[0].source_text(), "长剑\n第二把长剑\n尾部\n");
        assert!(docs.documents.iter().all(|doc| !doc.dirty()));
        assert_eq!(
            docs.documents[0].pages.concat(),
            "长剑\r\n第二把长剑\n尾部\r\n"
        );
        tasks::travel(&mut batch, &mut docs, true).unwrap();
        assert_eq!(docs.documents[0].source_text(), a);
        assert_eq!(
            std::fs::read_to_string(fixture.0.join("b.json")).unwrap(),
            "{\"武器\":\"长剑\"}"
        );
    }
}

#[test]
fn cross_replacement_uses_unsaved_drafts_keeps_original_history_and_can_import_new_documents() {
    let fixture = basic();
    let mut docs = documents(&fixture, &["a.txt"]);
    docs.documents[0].edit("草稿长剑\n长剑");
    let plan = preview(work(&fixture, &docs, "短剑"));
    assert!(plan.search.files[0].dirty);
    let content = apply(plan.clone(), Target::Checked).unwrap();
    let mut batch = tasks::adopt(&plan, content, &mut docs)
        .ok()
        .unwrap()
        .unwrap();
    assert_eq!(docs.documents.len(), 2);
    assert_eq!(docs.documents[0].source_text(), "草稿短剑\n短剑");
    tasks::travel(&mut batch, &mut docs, false).unwrap();
    assert_eq!(docs.documents[0].source_text(), "草稿长剑\n长剑");
    assert!(docs.documents[0].dirty());
    let selection = docs.documents[0].travel(false).unwrap();
    assert_eq!(selection, Selection::default());
    assert!(!docs.documents[0].dirty());
    assert!(tasks::travel(&mut batch, &mut docs, true).is_err()); // 后续单文件历史不能被旧批次覆盖。
}

#[test]
fn cross_replacement_external_change_in_last_file_rejects_whole_batch_before_any_draft_write() {
    let fixture = basic();
    let docs = documents(&fixture, &["a.txt", "b.json"]);
    let plan = preview(work(&fixture, &docs, "短剑"));
    fixture.write("b.json", "{\"外部\":\"新长剑\"}");
    assert!(
        apply(plan, Target::Checked)
            .err()
            .unwrap()
            .contains("已变化")
    );
    assert!(docs.documents.iter().all(|doc| !doc.dirty()));
    assert_eq!(docs.documents[0].source_text(), "长剑\n第二把长剑\n尾部\n");
}

#[test]
fn cross_replacement_rejects_late_draft_revision_atomically_and_keeps_new_edits() {
    let fixture = basic();
    let mut docs = documents(&fixture, &["a.txt", "b.json"]);
    let plan = preview(work(&fixture, &docs, "短剑"));
    let content = apply(plan.clone(), Target::Checked).unwrap();
    docs.documents[1].edit("新草稿长剑");
    assert!(tasks::adopt(&plan, content, &mut docs).is_err());
    assert!(!docs.documents[0].dirty());
    assert_eq!(docs.documents[1].source_text(), "新草稿长剑");
}

#[test]
fn cross_replacement_capacity_includes_unloaded_tabs_and_never_partially_imports_or_evicts() {
    let fixture = basic();
    let mut docs = documents(&fixture, &[]);
    let plan = preview(work(&fixture, &docs, "短剑"));
    for i in 0..7 {
        docs.parked.push(ExpandedDocument {
            id: i + 1,
            path: format!("reserved-{i}.txt"),
            return_tab: default(),
        });
    }
    let content = apply(plan.clone(), Target::Checked).unwrap();
    assert!(tasks::adopt(&plan, content, &mut docs).is_err());
    assert!(docs.documents.is_empty());
    assert_eq!(docs.parked.len(), 7);
}

#[test]
fn cross_replacement_same_text_is_no_op_and_empty_text_deletes_only_real_matches() {
    let fixture = basic();
    let mut docs = documents(&fixture, &[]);
    let same = preview(work(&fixture, &docs, "长剑"));
    assert!(same.files.iter().all(|file| file.content.is_none()));
    assert!(
        tasks::adopt(
            &same,
            apply(same.clone(), Target::Checked).unwrap(),
            &mut docs
        )
        .ok()
        .unwrap()
        .is_none()
    );
    assert!(docs.documents.is_empty());
    let delete = preview(work(&fixture, &docs, ""));
    assert_eq!(
        delete.files[0].content.as_ref().unwrap().pages.concat(),
        "\r\n第二把\n尾部\r\n"
    );
}

#[test]
fn cross_replacement_limited_search_keeps_actions_and_replaces_only_listed_matches() {
    use super::super::constants::MAX_HITS;
    for target in [
        Target::Hit(0, MAX_HITS - 1),
        Target::File(0),
        Target::Checked,
    ] {
        let fixture = Fixture::new();
        let original = "长剑\n".repeat(MAX_HITS + 5);
        fixture.write("a.txt", &original);
        fixture.write("b.txt", "未列出的长剑");
        // 已开草稿先搜索，确定在a文件中达到上限；未列出的b文件不能隐式加入替换。
        let mut docs = documents(&fixture, &["a.txt", "b.txt"]);
        let request = work(&fixture, &docs, "短剑");
        let Work::Preview { search, .. } = &request.work else {
            unreachable!()
        };
        assert!(search.limited);
        assert_eq!(search.files.len(), 1);
        assert_eq!(search.files[0].hits.len(), MAX_HITS);
        let plan = preview(request);
        assert_eq!(plan.files[0].after.len(), MAX_HITS);
        let content = apply(plan.clone(), target).unwrap();
        let mut batch = tasks::adopt(&plan, content, &mut docs)
            .ok()
            .unwrap()
            .unwrap();
        let expected = if matches!(target, Target::Hit(_, _)) {
            format!(
                "{}短剑\n{}",
                "长剑\n".repeat(MAX_HITS - 1),
                "长剑\n".repeat(5)
            )
        } else {
            format!("{}{}", "短剑\n".repeat(MAX_HITS), "长剑\n".repeat(5))
        };
        assert_eq!(docs.documents[0].source_text(), expected);
        assert_eq!(docs.documents[1].source_text(), "未列出的长剑");
        assert!(!docs.documents[1].dirty());
        assert_eq!(
            std::fs::read_to_string(fixture.0.join("a.txt")).unwrap(),
            original
        );
        tasks::travel(&mut batch, &mut docs, false).unwrap();
        assert_eq!(docs.documents[0].source_text(), original);
        tasks::travel(&mut batch, &mut docs, true).unwrap();
        assert_eq!(docs.documents[0].source_text(), expected);
    }
}

#[test]
fn cross_replacement_limited_search_still_rejects_changed_listed_match_ranges() {
    let fixture = Fixture::new();
    fixture.write(
        "a.txt",
        "长剑\n".repeat(super::super::constants::MAX_HITS + 5),
    );
    let docs = documents(&fixture, &["a.txt"]);
    let mut request = work(&fixture, &docs, "短剑");
    let Work::Preview { search, .. } = &mut request.work else {
        unreachable!()
    };
    let search = Arc::get_mut(search).unwrap();
    assert!(search.limited);
    search.files[0].hits[0].range = search.files[0].hits[1].range.clone();
    let Outcome::Preview(result) = run(request) else {
        unreachable!()
    };
    assert!(result.err().unwrap().contains("匹配集合已变化"));
    assert!(docs.documents.iter().all(|doc| !doc.dirty()));
}

#[test]
fn cross_replacement_overflow_failed_result_invalid_ranges_and_cancellation_refuse() {
    let fixture = basic();
    fixture.write(
        "b.json",
        format!("長{}长剑", "x".repeat(MAX_FILE_BYTES - 12)),
    );
    let docs = documents(&fixture, &[]);
    let Outcome::Preview(result) = run(work(&fixture, &docs, &"剑".repeat(512))) else {
        unreachable!()
    };
    assert!(result.err().unwrap().contains("256 KiB"));
    let mut request = work(&fixture, &docs, "短剑");
    if let Work::Preview { search, .. } = &mut request.work {
        Arc::get_mut(search).unwrap().error = "无法读取搜索目录".into();
    }
    let Outcome::Preview(result) = run(request) else {
        unreachable!()
    };
    assert!(result.err().unwrap().contains("搜索失败"));
    let request = work(&fixture, &docs, "短剑");
    request.cancel.store(true, Ordering::Relaxed);
    let Outcome::Preview(result) = run(request) else {
        unreachable!()
    };
    assert!(result.err().unwrap().contains("取消"));
    assert!(
        super::super::super::navigation::prepare_ranges(
            "长剑",
            &[Arc::from("长剑")],
            false,
            false,
            &[1..3],
            "短剑",
            Selection::default(),
            None,
            false
        )
        .is_err()
    );
}

fn app(fixture: &Fixture) -> App {
    let mut app = App::new();
    app.insert_resource(feathrise_editor_documents::content_operations());
    app.add_plugins(MinimalPlugins)
        .init_resource::<AssetReplaceState>()
        .init_resource::<AssetSearchState>()
        .init_resource::<crate::ui::external_files::ExternalEditors>() // 复用搜索输入门的安装缓存，夹具不启动系统检测。
        .init_resource::<DocumentInputs>()
        .init_resource::<crate::ui::external_files::ExternalEditors>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<SceneEditorSidebarState>()
        .insert_resource(SceneEditorState {
            open: true,
            ..default()
        })
        .insert_resource(documents(fixture, &[]));
    app.world_mut()
        .resource_mut::<SceneEditorSidebarState>()
        .navigator = EditorNavigatorTab::Assets;
    app
}

#[test]
fn cross_replacement_ctrl_shift_h_and_enter_only_preview_and_pending_ime_input_cannot_apply() {
    let fixture = basic();
    let mut app = app(&fixture);
    let request = work(&fixture, app.world().resource::<DocumentState>(), "短剑");
    let Work::Preview { search, .. } = request.work else {
        unreachable!()
    };
    {
        let mut state = app.world_mut().resource_mut::<AssetSearchState>();
        state.ticket = 1;
        state.open = true;
        state.result = Some(search);
        state.query = "长剑".into();
    }
    let mut field = EditableText::default();
    field.editor.set_text("短剑");
    let field = app.world_mut().spawn((ReplaceField, field)).id();
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::ControlLeft);
        keys.press(KeyCode::ShiftLeft);
        keys.press(KeyCode::KeyH);
    }
    app.world_mut().run_system_once(control::input).unwrap();
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::KeyH)
    );
    assert!(app.world().resource::<AssetReplaceState>().active);
    assert!(matches!(
        &app.world()
            .resource::<AssetReplaceState>()
            .pending
            .as_ref()
            .unwrap()
            .work,
        Work::Preview { .. }
    ));
    assert!(app.world().resource::<DocumentState>().documents.is_empty());
    app.world_mut()
        .get_mut::<EditableText>(field)
        .unwrap()
        .queue_edit(TextEdit::Insert("候选未采用".into()));
    app.world_mut()
        .spawn((ReplaceAction::ApplyChecked, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed));
    app.world_mut().run_system_once(control::input).unwrap();
    assert!(matches!(
        &app.world()
            .resource::<AssetReplaceState>()
            .pending
            .as_ref()
            .unwrap()
            .work,
        Work::Preview { .. }
    ));
}

#[test]
fn cross_replacement_cancel_keeps_running_slot_and_only_latest_request_can_publish() {
    let fixture = basic();
    let mut app = app(&fixture);
    let old = work(&fixture, app.world().resource::<DocumentState>(), "旧剑");
    let mut next = work(&fixture, app.world().resource::<DocumentState>(), "短剑");
    next.ticket = 2;
    {
        let Work::Preview { search, .. } = &next.work else {
            unreachable!()
        };
        let mut state = app.world_mut().resource_mut::<AssetSearchState>();
        state.ticket = 1;
        state.result = Some(search.clone());
    }
    let (send, receive) = std::sync::mpsc::channel();
    {
        let mut state = app.world_mut().resource_mut::<AssetReplaceState>();
        state.tab_open = true;
        state.active = true;
        state.ticket = 1;
        state.running = Some(old);
        state.task = Some(IoTaskPool::get().spawn(async move {
            receive.recv().unwrap();
            Reply {
                ticket: 1,
                outcome: Outcome::Preview(Err("迟到".into())),
            }
        }));
        state.cancel("取消");
        state.pending = Some(next);
    }
    app.world_mut().run_system_once(tasks::drive).unwrap();
    assert!(app.world().resource::<AssetReplaceState>().task.is_some());
    assert!(
        app.world()
            .resource::<AssetReplaceState>()
            .pending
            .is_some()
    );
    send.send(()).unwrap();
    for _ in 0..2000 {
        app.world_mut().run_system_once(tasks::drive).unwrap();
        if app
            .world()
            .resource::<AssetReplaceState>()
            .current()
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let state = app.world().resource::<AssetReplaceState>();
    assert_eq!(state.current().unwrap().ticket, 2);
    assert_eq!(state.current().unwrap().replacement, "短剑");
    assert!(state.task.is_none());
}

#[test]
fn cross_replacement_close_and_same_path_document_activation_cancel_without_adoption() {
    let fixture = basic();
    let mut app = app(&fixture);
    {
        let mut state = app.world_mut().resource_mut::<AssetReplaceState>();
        state.tab_open = true;
        state.active = true;
        state.center_activation = 0;
    }
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .active_tab = crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Document;
    app.world_mut()
        .resource_mut::<DocumentState>()
        .center_activation = 1;
    app.world_mut().run_system_once(control::input).unwrap();
    assert!(!app.world().resource::<AssetReplaceState>().active);
    let request = work(&fixture, app.world().resource::<DocumentState>(), "短剑");
    {
        let mut state = app.world_mut().resource_mut::<AssetReplaceState>();
        state.active = true;
        state.pending = Some(request);
    }
    app.world_mut().resource_mut::<SceneEditorState>().open = false;
    app.world_mut().run_system_once(tasks::drive).unwrap();
    assert!(
        app.world()
            .resource::<AssetReplaceState>()
            .pending
            .is_none()
    );
    assert!(app.world().resource::<DocumentState>().documents.is_empty());
}

#[test]
fn cross_replacement_disk_change_after_search_refuses_even_when_loaded_draft_revision_is_equal() {
    let fixture = basic();
    let docs = documents(&fixture, &["a.txt"]);
    let request = work(&fixture, &docs, "短剑");
    fixture.write("a.txt", "外部修改后的长剑");
    let Outcome::Preview(result) = run(request) else {
        unreachable!()
    };
    assert!(result.err().unwrap().contains("已变化"));
    assert!(!docs.documents[0].dirty());
}

#[test]
fn cross_replacement_more_than_eight_files_requires_explicit_subset_and_never_truncates_all() {
    let fixture = Fixture::new();
    for i in 0..9 {
        fixture.write(&format!("{i}.txt"), "长剑");
    }
    let mut app = app(&fixture);
    let Work::Preview { search, .. } =
        work(&fixture, app.world().resource::<DocumentState>(), "短剑").work
    else {
        unreachable!()
    };
    {
        let mut state = app.world_mut().resource_mut::<AssetSearchState>();
        state.ticket = 1;
        state.result = Some(search);
    }
    let button = app
        .world_mut()
        .spawn((ReplaceAction::Open, feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
        .id();
    app.world_mut().run_system_once(control::input).unwrap();
    assert!(
        app.world()
            .resource::<AssetReplaceState>()
            .checked
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<AssetReplaceState>()
            .pending
            .is_none()
    );
    *app.world_mut().get_mut::<ReplaceAction>(button).unwrap() = ReplaceAction::SelectAll;
    app.world_mut().run_system_once(control::input).unwrap();
    assert!(
        app.world()
            .resource::<AssetReplaceState>()
            .checked
            .is_empty()
    );
    for index in 0..9 {
        let ticket = app.world().resource::<AssetReplaceState>().preview_ticket;
        *app.world_mut().get_mut::<ReplaceAction>(button).unwrap() =
            ReplaceAction::Check { ticket, index };
        app.world_mut().run_system_once(control::input).unwrap();
    }
    assert_eq!(
        app.world().resource::<AssetReplaceState>().checked,
        (0..8).collect::<Vec<_>>()
    );
    assert!(
        app.world()
            .resource::<AssetReplaceState>()
            .hint
            .contains("最多勾选8")
    );
    assert!(app.world().resource::<DocumentState>().documents.is_empty());
}
