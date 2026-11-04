//! 实时终端尾行回归：用真实解码与读取旁路验证进度覆盖、结束提交及有界采集。

use super::{resources::*, stream::*, types::*};
use std::{io::Read, sync::atomic::Ordering};

fn drain(inbox: &ProjectLogInbox) -> Vec<(LogEventKind, String)> {
    inbox.0.try_iter().map(|event| (event.kind, event.text)).collect()
}

#[test]
fn reader_exposes_unterminated_output_before_eof_and_commits_it_only_once() {
    let (channel, inbox) = ProjectLogChannel::new("实时尾行");
    let bytes = b"    Building [======> ] 12/20: game";
    let mut reader = LogReader::new(bytes.as_slice(), Some(channel.begin()), LogSource::Build);
    let mut buffer = [0_u8; 128];
    let count = reader.read(&mut buffer).unwrap();
    assert_eq!(&buffer[..count], bytes, "实时观察不能改写原管道字节");
    assert_eq!(drain(&inbox), [(LogEventKind::Tail, String::from_utf8(bytes.to_vec()).unwrap())]);
    assert_eq!(reader.read(&mut buffer).unwrap(), 0);
    assert_eq!(drain(&inbox), [(LogEventKind::Line, String::from_utf8(bytes.to_vec()).unwrap())]);
    drop(reader);
    assert!(drain(&inbox).is_empty(), "EOF后Drop不能重复提交末行");
}

#[test]
fn carriage_return_refreshes_one_tail_without_committing_progress_history() {
    let (channel, inbox) = ProjectLogChannel::new("进度覆盖");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Build);
    // 超过邮箱和单帧预算的连续刷新仍只改变一条尾行，不生成数百条已完成行。
    for index in 0..400 {
        let progress = format!("Building {index}/400");
        decoder.write(format!("\r{progress}\r").as_bytes());
        assert_eq!(drain(&inbox), [(LogEventKind::Tail, progress)]);
    }
    decoder.write(b"\n");
    assert_eq!(drain(&inbox), [(LogEventKind::Line, "Building 399/400".into())]);
    decoder.finish();
    assert!(drain(&inbox).is_empty());
    assert_eq!(channel.dropped.load(Ordering::Relaxed), 0);
}

#[test]
fn one_write_coalesces_progress_and_terminal_only_refreshes_do_not_repeat_it() {
    let (channel, inbox) = ProjectLogChannel::new("合并尾行");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Build);
    decoder.write(b"old\rmiddle\rBuilding final");
    assert_eq!(drain(&inbox), [(LogEventKind::Tail, "Building final".into())]);
    decoder.write(b"\x1b[32m\x1b[0m\r");
    decoder.write(b"\rBuilding final\r");
    assert!(drain(&inbox).is_empty(), "正文未变化时不重复占用邮箱");
    decoder.finish();
    assert_eq!(drain(&inbox), [(LogEventKind::Line, "Building final".into())]);
}

#[test]
fn split_and_same_chunk_crlf_commit_each_complete_line_once() {
    let (channel, inbox) = ProjectLogChannel::new("CRLF边界");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stdout);
    decoder.write(b"first\r");
    assert_eq!(drain(&inbox), [(LogEventKind::Tail, "first".into())]);
    decoder.write(b"\nsecond\r\n");
    assert_eq!(drain(&inbox), [
        (LogEventKind::Line, "first".into()),
        (LogEventKind::Line, "second".into()),
    ]);
    decoder.finish();
    assert!(drain(&inbox).is_empty());
}

#[test]
fn cargo_carriage_return_and_erase_line_withdraw_the_visible_progress() {
    let (channel, inbox) = ProjectLogChannel::new("Cargo擦除进度");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Build);
    decoder.write(b"\r\x1b[1m\x1b[32m    Building\x1b[0m [==> ] 4/10: game\r");
    assert_eq!(drain(&inbox), [(LogEventKind::Tail, "    Building [==> ] 4/10: game".into())]);
    decoder.write(b"\r\x1b[");
    assert!(drain(&inbox).is_empty());
    decoder.write(b"K");
    assert_eq!(drain(&inbox), [(LogEventKind::Tail, String::new())]);
    decoder.finish();
    assert!(drain(&inbox).is_empty(), "已擦除进度不能在EOF被重新提交");

    decoder.write(b"retained\x1b[K");
    assert_eq!(drain(&inbox), [(LogEventKind::Tail, "retained".into())], "行尾向后擦除不能删掉已有正文");
    decoder.write(b"\x1b[2K");
    assert_eq!(drain(&inbox), [(LogEventKind::Tail, String::new())]);
}

#[test]
fn split_utf8_and_ansi_publish_only_visible_complete_characters() {
    let (channel, inbox) = ProjectLogChannel::new("中文实时日志");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stderr);
    let bytes = "\u{1b}[31m正在编译\u{1b}[0m\u{1b}]0;隐藏标题\u{7}…\n".as_bytes();
    let mut events = Vec::new();
    for byte in bytes {
        decoder.write(&[*byte]);
        events.extend(inbox.0.try_iter());
    }
    assert!(events.iter().all(|event| event.source == LogSource::Stderr));
    let tails: Vec<_> = events.iter().filter(|event| event.kind == LogEventKind::Tail)
        .map(|event| event.text.as_str()).collect();
    assert_eq!(tails, ["正", "正在", "正在编", "正在编译", "正在编译…"]);
    let lines: Vec<_> = events.iter().filter(|event| event.kind == LogEventKind::Line)
        .map(|event| event.text.as_str()).collect();
    assert_eq!(lines, ["正在编译…"]);
}

#[test]
fn finish_replaces_incomplete_utf8_and_is_idempotent() {
    let (channel, inbox) = ProjectLogChannel::new("结束边界");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stdout);
    decoder.write(b"old\r");
    assert_eq!(drain(&inbox), [(LogEventKind::Tail, "old".into())]);
    decoder.write(&[0xe4, 0xb8]);
    assert!(drain(&inbox).is_empty(), "半个UTF-8字符不能提前覆盖可见尾行");
    decoder.finish();
    assert_eq!(drain(&inbox), [(LogEventKind::Line, "�".into())]);
    decoder.finish();
    assert!(drain(&inbox).is_empty());
}

#[test]
fn long_utf8_output_splits_within_one_kib_without_losing_the_tail() {
    let (channel, inbox) = ProjectLogChannel::new("有界长行");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stdout);
    let text = "界".repeat(400);
    decoder.write(text.as_bytes());
    let mut events: Vec<_> = inbox.0.try_iter().collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind, LogEventKind::Line);
    assert_eq!(events[1].kind, LogEventKind::Tail);
    assert_eq!(events[0].text.len(), 1023, "分段不能切断三字节字符");
    decoder.finish();
    events.extend(inbox.0.try_iter());
    assert!(events.iter().all(|event| event.text.len() <= LINE_BYTES));
    let complete: String = events.into_iter().filter(|event| event.kind == LogEventKind::Line)
        .map(|event| event.text).collect();
    assert_eq!(complete, text);
}

#[test]
fn pending_tail_does_not_consume_or_overflow_the_completed_line_mailbox() {
    let (channel, inbox) = ProjectLogChannel::new("邮箱背压");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Build);
    for _ in 0..CHANNEL_LINES { decoder.write(b"complete\n"); }
    decoder.write(b"Building pending");
    assert_eq!(channel.dropped.load(Ordering::Relaxed), 0);
    let events = drain(&inbox);
    assert_eq!(events.iter().filter(|(kind, _)| *kind == LogEventKind::Line).count(), CHANNEL_LINES);
    assert_eq!(events.last(), Some(&(LogEventKind::Tail, "Building pending".into())));
    decoder.write(b"\x1b[0m");
    assert!(drain(&inbox).is_empty(), "已发布的同一尾行不重复发送");
    decoder.finish();
    assert_eq!(drain(&inbox), [(LogEventKind::Line, "Building pending".into())]);
}

#[test]
fn full_mailbox_does_not_change_or_block_original_reader_bytes() {
    let (channel, inbox) = ProjectLogChannel::new("真实读取限额");
    let bytes = format!("{}\rBuilding latest", "line\n".repeat(CHANNEL_LINES + 40)).into_bytes();
    let mut reader = LogReader::new(bytes.as_slice(), Some(channel.begin()), LogSource::Build);
    let mut original = Vec::new();
    reader.read_to_end(&mut original).unwrap();
    assert_eq!(original, bytes);
    let events: Vec<_> = inbox.0.try_iter().collect();
    assert_eq!(events.iter().filter(|event| event.kind == LogEventKind::Line).count(), CHANNEL_LINES);
    assert!(events.iter().all(|event| event.text.len() <= LINE_BYTES));
    assert!(channel.dropped.load(Ordering::Relaxed) > 0);
}

#[test]
fn a_thousand_fragmented_progress_updates_leave_room_for_the_final_error() {
    let (channel, inbox) = ProjectLogChannel::new("碎片进度不能挤掉错误");
    let sink = channel.begin();
    let mut progress = LogDecoder::new(Some(sink.clone()), LogSource::Build);
    let mut errors = LogDecoder::new(Some(sink), LogSource::Stderr);
    // 整段不消费收件箱，覆盖真实最坏情况：大量CR与逐字节read只合并到同一固定槽。
    for index in 0..1000 {
        for byte in format!("\rBuilding {index}/1000\r").bytes() { progress.write(&[byte]); }
    }
    errors.write(b"error[E0308]: retained final error\n");
    errors.finish();
    let events: Vec<_> = inbox.0.try_iter().collect();
    assert_eq!(channel.dropped.load(Ordering::Acquire), 0);
    assert_eq!(events.iter().filter(|event| event.kind == LogEventKind::Line).count(), 1);
    assert!(events.iter().any(|event| event.kind == LogEventKind::Line && event.text == "error[E0308]: retained final error"));
    assert_eq!(events.iter().filter(|event| event.kind == LogEventKind::Tail).count(), 1);
    assert!(events.iter().any(|event| event.kind == LogEventKind::Tail && event.text == "Building 999/1000"));
    progress.finish();
    assert_eq!(drain(&inbox), [(LogEventKind::Line, "Building 999/1000".into())]);
}

#[test]
fn collecting_never_waits_for_a_busy_tail_slot_and_keeps_the_frame_event_budget() {
    use bevy::prelude::*;
    let (channel, inbox) = ProjectLogChannel::new("非阻塞尾行采用");
    let sink = channel.begin();
    let mut decoder = LogDecoder::new(Some(sink.clone()), LogSource::Build);
    decoder.write(b"Building pending");
    let mut state = ProjectLogState::new(&channel, "非阻塞尾行采用");
    state.generation = sink.generation;
    let mut app = App::new();
    app.insert_resource(channel.clone()).insert_resource(state).insert_non_send_resource(inbox)
        .add_systems(Update, super::systems::collect);
    let held = channel.pending_tails[tail_index(sink.task, LogSource::Build)].lock().unwrap();
    app.update(); // try_lock必须直接跳过；若主线程改成lock，此处会发生同线程死锁。
    assert_eq!(app.world().resource::<ProjectLogState>().row_count(), 0);
    drop(held);
    for _ in 0..FRAME_LINES + 2 { decoder.write(b"complete\n"); }
    decoder.write(b"latest progress");
    app.update();
    let state = app.world().resource::<ProjectLogState>();
    assert_eq!(state.lines.len(), FRAME_LINES);
    assert_eq!(state.current_tails().count(), 0, "尾行也受每帧32条总采用预算约束");
    app.update();
    let state = app.world().resource::<ProjectLogState>();
    assert_eq!(state.lines.len(), FRAME_LINES + 2);
    assert!(state.current_tails().any(|tail| tail.text == "latest progress"));
    assert_eq!(channel.dropped.load(Ordering::Acquire), 0);
}

#[test]
fn late_completed_lines_do_not_clear_a_newer_tail_or_redirect_its_selection() {
    let (channel, _) = ProjectLogChannel::new("尾行顺序与选区");
    let mut state = ProjectLogState::new(&channel, "尾行顺序与选区");
    let event = |sequence, kind, text: &str| LogEvent { owner: channel.owner.clone(), generation: 0,
        sequence, task: LogTask::default(), source: LogSource::Build, kind, text: text.into(), spans: Vec::new() };
    state.adopt(event(2, LogEventKind::Tail, "new progress"));
    state.selection = Some(LogSelection { anchor: state.position_at(0), focus: state.position_at(3) });
    state.adopt(event(1, LogEventKind::Line, "older"));
    assert_eq!(state.copy_text(false), "new");
    assert_eq!(state.current_tails().count(), 1);
    state.adopt(event(3, LogEventKind::Line, "new progress"));
    state.adopt(event(2, LogEventKind::Tail, "new progress"));
    assert_eq!(state.current_tails().count(), 0, "已提交尾行不能被迟到槽读回复活");
    assert_eq!(state.copy_text(false), "new");
}

#[test]
fn split_sgr_preserves_unicode_ranges_and_sixteen_indexed_and_rgb_foregrounds() {
    let (channel, inbox) = ProjectLogChannel::new("终端颜色");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stdout);
    let bytes = "\u{1b}[1;31m红\u{1b}[22;38;5;46m绿\u{1b}[38;2;12;34;56;48;2;1;2;3m蓝\u{1b}[39m常规\n".as_bytes();
    let mut events = Vec::new();
    for byte in bytes {
        decoder.write(&[*byte]);
        events.extend(inbox.0.try_iter());
    }
    let line = events.iter().find(|event| event.kind == LogEventKind::Line).unwrap();
    assert_eq!(line.text, "红绿蓝常规");
    assert_eq!(line.spans, [
        LogSpan { start: 0, end: 3, style: LogStyle { fg: Some([205, 49, 49]), bold: true } },
        LogSpan { start: 3, end: 6, style: LogStyle { fg: Some([0, 255, 0]), bold: false } },
        LogSpan { start: 6, end: 9, style: LogStyle { fg: Some([12, 34, 56]), bold: false } },
    ]);
    assert!(events.iter().all(|event| event.spans.iter().all(|span|
        event.text.is_char_boundary(span.start) && event.text.is_char_boundary(span.end))));
}

#[test]
fn background_parameters_do_not_reset_foreground_and_equal_styles_coalesce() {
    let (channel, inbox) = ProjectLogChannel::new("背景参数隔离");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stderr);
    decoder.write(b"\x1b[1;31mA\x1b[48;2;0;22;0mB\x1b[22mC\x1b[0mD\n");
    let line = inbox.0.try_recv().unwrap();
    assert_eq!(line.text, "ABCD");
    assert_eq!(line.spans, [
        LogSpan { start: 0, end: 2, style: LogStyle { fg: Some([205, 49, 49]), bold: true } },
        LogSpan { start: 2, end: 3, style: LogStyle { fg: Some([205, 49, 49]), bold: false } },
    ]);
}

#[test]
fn bright_and_indexed_palette_endpoints_decode_without_overflow() {
    let (channel, inbox) = ProjectLogChannel::new("终端调色板边界");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stdout);
    decoder.write(b"\x1b[90mA\x1b[97mB\x1b[38;5;16mC\x1b[38;5;231mD\x1b[38;5;232mE\x1b[38;5;255mF\n");
    let line = inbox.0.try_recv().unwrap();
    assert_eq!(line.spans.iter().map(|span| span.style.fg.unwrap()).collect::<Vec<_>>(), [
        [102, 102, 102], [255, 255, 255], [0, 0, 0], [255, 255, 255], [8, 8, 8], [238, 238, 238],
    ]);
}

#[test]
fn same_text_progress_republishes_when_repainted_with_a_different_color() {
    let (channel, inbox) = ProjectLogChannel::new("同字换色");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Build);
    decoder.write(b"\x1b[31mprogress");
    let first = inbox.0.try_recv().unwrap();
    decoder.write(b"\r\x1b[32mprogress");
    let second = inbox.0.try_recv().unwrap();
    assert_eq!(first.text, second.text);
    assert_eq!(second.kind, LogEventKind::Tail);
    assert_ne!(first.spans, second.spans, "正文相同也必须更新可见样式");
    decoder.write(b"\x1b[0m");
    assert!(inbox.0.try_recv().is_err(), "行尾reset不追溯修改已经输出的字形");
    decoder.finish();
    let committed = inbox.0.try_recv().unwrap();
    assert_eq!(committed.kind, LogEventKind::Line);
    assert_eq!(committed.spans, second.spans);
}

#[test]
fn plain_cargo_status_fallback_colors_only_the_prefix_without_treating_stderr_as_error() {
    let (channel, inbox) = ProjectLogChannel::new("无ANSI的Cargo");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stderr);
    decoder.write(b"   Compiling game error.rs\nordinary stderr message\nwarning: detail\nerror[E123]: detail\nerrorless\nCompilingAgain\nBuilding 1/2\n");
    let events: Vec<_> = inbox.0.try_iter().collect();
    assert_eq!(events.len(), 7);
    assert_eq!(&events[0].text[events[0].spans[0].start..events[0].spans[0].end], "Compiling");
    assert_eq!(events[0].spans.len(), 1);
    assert!(events[1].spans.is_empty(), "stderr也承载编译进度与普通诊断");
    assert_eq!(events[2].spans[0].style.fg, Some([245, 245, 67]));
    assert_eq!(events[3].spans[0].style.fg, Some([241, 76, 76]));
    assert!(events[4].spans.is_empty() && events[5].spans.is_empty());
    assert_eq!(events[6].spans[0].style.fg, Some([41, 184, 219]));
}

#[test]
fn frequent_color_changes_are_bounded_and_line_splits_keep_valid_utf8_ranges() {
    let (channel, inbox) = ProjectLogChannel::new("恶意切色");
    let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stdout);
    let bytes: String = (0..500).map(|index| format!("\u{1b}[38;5;{}m界", index % 256)).collect();
    decoder.write(bytes.as_bytes());
    decoder.finish();
    let events: Vec<_> = inbox.0.try_iter().filter(|event| event.kind == LogEventKind::Line).collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events.iter().map(|event| event.text.as_str()).collect::<String>(), "界".repeat(500));
    assert!(events.iter().all(|event| event.text.len() <= LINE_BYTES && event.spans.len() == MAX_STYLE_SPANS));
    assert!(events.iter().all(|event| event.spans.iter().all(|span| span.start < span.end
        && span.end <= event.text.len() && event.text.is_char_boundary(span.start) && event.text.is_char_boundary(span.end))));
}

#[test]
fn style_metadata_uses_the_same_cache_byte_budget_as_plain_text() {
    let (channel, inbox) = ProjectLogChannel::new("颜色缓存预算");
    let sink = channel.begin();
    let mut state = ProjectLogState::new(&channel, "颜色缓存预算");
    state.generation = sink.generation;
    let mut decoder = LogDecoder::new(Some(sink), LogSource::Build);
    let line: String = (0..MAX_STYLE_SPANS).map(|index| format!("\u{1b}[{}m界", 31 + index % 2)).collect();
    for _ in 0..300 {
        decoder.write(line.as_bytes());
        decoder.write(b"\n");
        for event in inbox.0.try_iter() { state.adopt(event); }
    }
    assert!(state.trimmed > 0 && state.lines.len() < 300);
    assert!(state.bytes <= CACHE_BYTES);
    assert_eq!(state.bytes, state.lines.iter().map(|line|
        line.text.len() + line.spans.len() * std::mem::size_of::<LogSpan>() + std::mem::size_of::<LogTask>()).sum::<usize>());
    assert!(state.lines.iter().all(|line| line.spans.len() == MAX_STYLE_SPANS));
}

#[test]
fn visible_styles_are_rebased_to_the_unicode_page_while_copy_keeps_plain_history() {
    let (channel, inbox) = ProjectLogChannel::new("颜色虚拟页");
    let sink = channel.begin();
    let mut state = ProjectLogState::new(&channel, "颜色虚拟页");
    state.generation = sink.generation;
    state.visible_rows = 3;
    let mut output = LogDecoder::new(Some(sink.clone()), LogSource::Stdout);
    let mut tail = LogDecoder::new(Some(sink), LogSource::Stderr);
    output.write("first\n\u{1b}[31m中文\u{1b}[0m\nlast\n".as_bytes());
    tail.write("\u{1b}[34m尾巴".as_bytes());
    for event in inbox.0.try_iter() { state.adopt(event); }
    let (origin, text, spans) = state.visible_styled_text();
    assert_eq!((origin, text.as_str()), (6, "中文\nlast\n尾巴"));
    assert_eq!(state.visible_text(), (origin, text.clone()));
    assert_eq!(spans.iter().map(|span| (span.start, span.end)).collect::<Vec<_>>(), [(0, 6), (12, 18)]);
    assert_eq!(state.copy_text(true), "first\n中文\nlast\n尾巴");
    state.scroll_back = 1;
    let (origin, text, spans) = state.visible_styled_text();
    assert_eq!((origin, text.as_str()), (0, "first\n中文\nlast"));
    assert_eq!(spans.iter().map(|span| (span.start, span.end)).collect::<Vec<_>>(), [(6, 12)]);
    assert_eq!(state.copy_text(true), "first\n中文\nlast\n尾巴", "换页和颜色不得改变复制正文");
}

#[test]
fn parallel_project_and_author_jobs_keep_their_own_live_output_in_one_reading_session() {
    let (channel, inbox) = ProjectLogChannel::new("并行目标日志");
    let project = channel.begin_task(LogTaskKind::Project3d);
    let author = channel.begin_task(LogTaskKind::AuthorPreview);
    assert_ne!(project.task.number, author.task.number);
    assert_eq!(project.generation, author.generation);
    let mut state = ProjectLogState::new(&channel, "并行目标日志");
    state.generation = project.generation;
    let mut project = LogDecoder::new(Some(project), LogSource::Build);
    let mut author = LogDecoder::new(Some(author), LogSource::Build);
    project.write(b"Building project");
    author.write(b"Building author");
    for event in inbox.0.try_iter() { state.adopt(event); }
    assert_eq!(state.tails.iter().flatten().count(), 2);
    assert!(state.text().contains("工程3D #1") && state.text().contains("作者预览 #2"));
    assert!(state.rows().any(|(_, text)| text == "Building project"));
    assert!(state.rows().any(|(_, text)| text == "Building author"));
    project.write(b"\rproject finished\n");
    author.write(b"\rauthor still building");
    for event in inbox.0.try_iter() { state.adopt(event); }
    assert!(state.rows().any(|(_, text)| text == "project finished"));
    assert!(state.rows().any(|(_, text)| text == "author still building"));
    assert_eq!(channel.generation.load(Ordering::Acquire), 1, "另一类任务不能清空工程阅读会话");
}

#[test]
fn newer_job_rejects_only_the_same_targets_queued_and_future_output() {
    let (channel, inbox) = ProjectLogChannel::new("同类任务过期");
    let old = channel.begin_task(LogTaskKind::Project3d);
    let author = channel.begin_task(LogTaskKind::AuthorPreview);
    let mut state = ProjectLogState::new(&channel, "同类任务过期");
    state.generation = old.generation;
    for event in inbox.0.try_iter() { state.adopt(event); }
    let mut old = LogDecoder::new(Some(old), LogSource::Stderr);
    let mut author = LogDecoder::new(Some(author), LogSource::Stderr);
    old.write(b"queued old failure\n");
    let current = channel.begin_task(LogTaskKind::Project3d);
    let mut current = LogDecoder::new(Some(current), LogSource::Stderr);
    old.write(b"late old failure\n");
    author.write(b"author alive\n");
    current.write(b"current project alive\n");
    for event in inbox.0.try_iter() { state.adopt(event); }
    let text = state.copy_text(true);
    assert!(!text.contains("old failure"));
    assert!(text.contains("author alive") && text.contains("current project alive"));
    assert!(text.contains("工程3D #1") && text.contains("工程3D #3"), "已读旧历史保留明确归属");
    assert_eq!(channel.dropped.load(Ordering::Acquire), 0, "身份拒绝不是邮箱丢行");
}

#[test]
fn tail_selection_uses_task_and_stream_and_cannot_attach_to_a_replacement_job() {
    let (channel, inbox) = ProjectLogChannel::new("同流跨任务选区");
    let project = channel.begin_task(LogTaskKind::Project3d);
    let author = channel.begin_task(LogTaskKind::AuthorPreview);
    let mut state = ProjectLogState::new(&channel, "同流跨任务选区");
    state.generation = project.generation;
    let mut project = LogDecoder::new(Some(project), LogSource::Stderr);
    let mut author = LogDecoder::new(Some(author), LogSource::Stderr);
    project.write(b"project");
    author.write("作者进度".as_bytes());
    for event in inbox.0.try_iter() { state.adopt(event); }
    let start = state.rows().find(|(_, text)| *text == "作者进度").unwrap().0;
    state.selection = Some(LogSelection { anchor: state.position_at(start), focus: state.position_at(start + "作者进度".len() as u64) });
    project.write(b"\rmuch longer project progress\n");
    for event in inbox.0.try_iter() { state.adopt(event); }
    assert_eq!(state.copy_text(false), "作者进度");
    let current = channel.begin_task(LogTaskKind::AuthorPreview);
    let mut current = LogDecoder::new(Some(current), LogSource::Stderr);
    current.write("另一作者任务".as_bytes());
    for event in inbox.0.try_iter() { state.adopt(event); }
    assert!(state.copy_text(false).is_empty(), "旧任务选区不能按相同stderr字节位置套到新任务");
    assert!(!state.text().contains("作者进度"));
}
