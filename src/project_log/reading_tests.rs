//! 阅读合同回归：跨虚拟页复制、UTF-8、尾行来源稳定身份与真正只读的输入队列。

use super::{components::LogBody, resources::*, types::*};
use bevy::{prelude::*, text::{EditableText, TextEdit}};

fn state() -> ProjectLogState {
    let (channel, _) = ProjectLogChannel::new("日志阅读夹具");
    ProjectLogState::new(&channel, "日志阅读夹具")
}

fn append(state: &mut ProjectLogState, source: LogSource, kind: LogEventKind, text: &str) {
    state.adopt(LogEvent { owner: state.owner.clone(), generation: state.generation,
        sequence: state.content_revision + 1, task: LogTask::default(), source, kind, text: text.into(), spans: Vec::new() });
}

#[test]
fn copy_all_and_unicode_selection_cover_the_cache_beyond_the_visible_page() {
    let mut state = state();
    state.visible_rows = 4;
    for index in 0..100 { append(&mut state, LogSource::Stderr, LogEventKind::Line, &format!("错误{index:03}")); }
    assert_eq!(state.text().lines().count(), 4);
    let all = state.copy_text(true);
    assert_eq!(all.lines().count(), 100);
    assert!(all.starts_with("错误000\n") && all.ends_with("错误099"));
    assert!(!all.contains("[stderr]"));
    let anchor = state.lines[1].start + "错".len() as u64;
    let focus = state.lines[80].start + "错误080".len() as u64;
    state.selection = Some(LogSelection { anchor: LogPosition::Committed(anchor), focus: LogPosition::Committed(focus) });
    let selected = state.copy_text(false);
    assert!(selected.starts_with("误001\n") && selected.ends_with("错误080"));
    state.scroll_back = 60;
    assert_eq!(state.copy_text(false), selected, "滚动只改变可见页，不能缩短全局选区");
    state.select_all = true;
    assert_eq!(state.copy_text(false), all, "Ctrl+A选择完整缓存");
}

#[test]
fn concurrent_live_tail_changes_and_commits_cannot_redirect_selection_to_another_source() {
    let mut state = state();
    append(&mut state, LogSource::Stdout, LogEventKind::Tail, "A");
    append(&mut state, LogSource::Stderr, LogEventKind::Tail, "中文进度");
    let start = 2;
    state.selection = Some(LogSelection { anchor: state.position_at(start), focus: state.position_at(start + "中文进度".len() as u64) });
    assert_eq!(state.copy_text(false), "中文进度");
    append(&mut state, LogSource::Stdout, LogEventKind::Tail, "much longer output");
    assert_eq!(state.copy_text(false), "中文进度");
    append(&mut state, LogSource::Stdout, LogEventKind::Line, "stdout完成");
    assert_eq!(state.copy_text(false), "中文进度");
    append(&mut state, LogSource::Stderr, LogEventKind::Line, "中文进度");
    assert_eq!(state.copy_text(false), "中文进度", "尾行正式提交后选区转为不可变行身份");
}

#[test]
fn live_progress_replaces_one_row_and_erasing_it_clears_its_selection() {
    let mut state = state();
    for index in 0..100 { append(&mut state, LogSource::Build, LogEventKind::Tail, &format!("Building {index}/100")); }
    assert_eq!(state.row_count(), 1);
    assert_eq!(state.lines.len(), 0);
    assert_eq!(state.text(), "Building 99/100");
    state.selection = Some(LogSelection { anchor: state.position_at(0), focus: state.position_at(8) });
    append(&mut state, LogSource::Build, LogEventKind::Tail, "");
    assert!(state.selection.is_none());
    assert!(state.copy_text(false).is_empty());
}

#[test]
fn readonly_blocks_insert_delete_cut_paste_and_ime_but_preserves_native_drag_selection() {
    let mut app = App::new();
    let mut state = state();
    state.open = true;
    app.insert_resource(state).add_systems(Update, super::reading::readonly);
    let entity = app.world_mut().spawn((LogBody::default(), EditableText::new("原始中文日志"))).id();
    {
        let mut text = app.world_mut().get_mut::<EditableText>(entity).unwrap();
        text.pending_edits = vec![TextEdit::Insert("改写".into()), TextEdit::Cut, TextEdit::Paste,
            TextEdit::Backspace, TextEdit::Delete, TextEdit::ImeCommit { value: "输入法".into() },
            TextEdit::MoveToPoint(Vec2::ZERO), TextEdit::ExtendSelectionToPoint(Vec2::ONE), TextEdit::Copy];
    }
    app.update();
    let text = app.world().get::<EditableText>(entity).unwrap();
    assert_eq!(text.value().to_string(), "原始中文日志");
    assert_eq!(text.pending_edits, [TextEdit::MoveToPoint(Vec2::ZERO), TextEdit::ExtendSelectionToPoint(Vec2::ONE)]);
    let state = app.world().resource::<ProjectLogState>();
    assert!(!state.follow && state.copy_selection);
}
