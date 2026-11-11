//! 临时文件验证撤销保存点、迟到回执、重载换代与关闭未修改的草稿保护。

use super::super::history::{EditContext, Selection};
use super::*;

#[test]
fn history_explicit_save_splits_typing_and_undo_returns_to_the_saved_bytes() {
    let fixture = Fixture::new();
    fixture.write("原文\r\n");
    let mut app = app(&fixture);
    let at = std::time::Instant::now();
    let context = |before, after, millis| EditContext {
        before: Selection {
            anchor: before,
            focus: before,
        },
        after: Selection {
            anchor: after,
            focus: after,
        },
        typing: true,
        at: at + std::time::Duration::from_millis(millis),
    };
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .edit_with("原文\na", context(7, 8, 0));
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
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .edit_with("原文\nab", context(8, 9, 100));
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .travel(false);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert_eq!(doc.source_text(), "原文\na");
    assert!(!doc.dirty());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(),
        "原文\r\na"
    );
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .travel(false);
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty(),
        "撤销已保存编辑会成为新的未保存草稿"
    );
}

#[test]
fn history_saving_an_older_undo_snapshot_clears_only_its_content_identity() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut app = app(&fixture);
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        let doc = state.current_mut().unwrap();
        doc.edit("旧草稿");
        doc.edit("新草稿");
        doc.travel(false);
        assert!(doc.dirty());
        state.request_save();
    }
    settle(&mut app);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert!(!doc.dirty());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(),
        "旧草稿"
    );
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .travel(true);
    assert!(
        app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .travel(false);
    assert!(
        !app.world()
            .resource::<DocumentState>()
            .current()
            .unwrap()
            .dirty()
    );
}

#[test]
fn history_late_save_receipt_does_not_mark_new_branch_clean() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut app = app(&fixture);
    {
        let mut state = app.world_mut().resource_mut::<DocumentState>();
        state.current_mut().unwrap().edit("提交内容");
        state.request_save();
        let doc = state.current_mut().unwrap();
        doc.travel(false);
        doc.edit("另一个分支");
    }
    settle(&mut app);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert_eq!(doc.source_text(), "另一个分支");
    assert!(doc.dirty());
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(),
        "提交内容"
    );
}

#[test]
fn history_reload_replaces_generation_and_drops_old_journal_only_after_success() {
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut app = app(&fixture);
    app.world_mut()
        .resource_mut::<DocumentState>()
        .current_mut()
        .unwrap()
        .edit("草稿");
    app.world_mut().resource_mut::<DocumentState>().load_ticket = 1;
    app.world_mut()
        .resource_mut::<DocumentState>()
        .request_load("说明.md".into());
    settle(&mut app);
    let doc = app.world().resource::<DocumentState>().current().unwrap();
    assert_eq!(doc.generation, 2);
    assert!(!doc.history.can_undo());
    assert!(!doc.dirty());
    assert_eq!(doc.source_text(), "原文");
}

#[test]
fn close_unmodified_keeps_dirty_tabs_and_history_after_saved_or_undone_files_close() {
    use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut docs = DocumentState::default();
    let mut workspace = VegetationPresetEditorState::default();
    for (path, dirty) in [
        ("dirty.md", true),
        ("saved.md", false),
        ("undone.md", false),
    ] {
        let mut doc = fixture.document();
        doc.path = path.into();
        doc.edit("改动");
        if !dirty {
            doc.travel(false);
        }
        docs.documents.push(doc);
        docs.expand(path.into(), &mut workspace).unwrap();
    }
    docs.active = Some("undone.md".into());
    docs.central_active = true;
    let removed = docs.close_unmodified_tabs().unwrap();
    assert_eq!(removed.0, "undone.md");
    assert_eq!(docs.tab_count(), 1);
    assert_eq!(docs.expanded.as_ref().unwrap().path, "dirty.md");
    assert!(docs.central_active);
    assert_eq!(workspace.active_tab, PresetWorkspaceTab::Document);
    assert_eq!(docs.documents.len(), 3);
    let dirty = docs
        .documents
        .iter_mut()
        .find(|doc| doc.path == "dirty.md")
        .unwrap();
    assert!(dirty.dirty());
    assert!(dirty.history.can_undo());
    assert_eq!(docs.close_unmodified_tabs(), None, "活动草稿不能被关闭");
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("说明.md")).unwrap(),
        "原文"
    );
}

#[test]
fn close_unmodified_keeps_saving_draft_until_a_successful_receipt_and_never_cancels_io() {
    use crate::ui::vegetation_preset_editor::VegetationPresetEditorState;
    let fixture = Fixture::new();
    fixture.write("原文");
    let mut app = app(&fixture);
    let mut workspace = VegetationPresetEditorState::default();
    {
        let mut docs = app.world_mut().resource_mut::<DocumentState>();
        docs.expand("说明.md".into(), &mut workspace).unwrap();
        docs.current_mut().unwrap().edit("保存中");
        docs.request_save();
        assert_eq!(docs.close_unmodified_tabs(), None);
        assert!(docs.io.is_some());
        assert_eq!(docs.tab_count(), 1);
    }
    settle(&mut app);
    let mut docs = app.world_mut().resource_mut::<DocumentState>();
    assert!(!docs.current().unwrap().dirty());
    assert!(docs.close_unmodified_tabs().is_some());
    assert_eq!(docs.tab_count(), 0);
}
