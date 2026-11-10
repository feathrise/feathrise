//! 日志生产边界回归：实际流旁路、恶意输出上限、过期会话隔离与浮窗输入生命周期。

use super::{resources::*, stream::*, types::*};
use bevy::{input_focus::InputFocus, prelude::*};
use std::{io::Read, sync::atomic::Ordering};

#[test]
fn build_command_log_keeps_actual_arguments_and_long_paths_without_environment_dump() {
    let (channel, inbox) = ProjectLogChannel::new("构建命令夹具");
    let sink = channel.begin();
    let mut state = ProjectLogState::new(&channel, "构建命令夹具");
    state.generation = channel.generation.load(Ordering::Acquire);
    let mut command = std::process::Command::new("C:/工具目录/dx.exe");
    command.current_dir("D:/我的工程").args(["build", "--fat-binary", "--profile", "render-dev", "--features", "raytracing"])
        .arg("a'b $value").arg("目录".repeat(220)).arg("last-argument")
        .env("BUILD_COMMAND_TEST_SECRET", "must-not-appear");
    crate::build_process::log_build_command(&command, &crate::build_process::BuildControl::with_log(sink));
    for event in inbox.0.try_iter() { state.adopt(event); }
    let text = state.text();
    assert!(text.contains("工作目录：D:/我的工程"));
    assert!(text.contains("'C:/工具目录/dx.exe' build --fat-binary --profile render-dev --features raytracing"));
    assert!(text.contains(if cfg!(windows) { "'a''b $value'" } else { "'a'\"'\"'b $value'" }));
    assert!(text.ends_with("last-argument"), "超过1KiB的实际命令不能静默丢失末尾参数");
    assert!(!text.contains("must-not-appear"));
    assert!(!text.contains("BUILD_COMMAND_TEST_SECRET"));
    assert_eq!(command.get_args().last().unwrap(), "last-argument", "显示不能改写实际构建参数");
}

#[test]
fn stream_keeps_original_bytes_while_sanitizing_split_utf8_and_terminal_escapes() {
    let (channel, inbox) = ProjectLogChannel::new("日志工程");
    let sink = channel.begin();
    let bytes = "\u{1b}[31m编译错误\u{1b}[0m\n\u{1b}]0;不能改窗口标题\u{7}真实输出\n".as_bytes();
    let mut reader = LogReader::new(bytes, Some(sink), LogSource::Stderr);
    let mut original = Vec::new();
    let mut byte = [0_u8; 1];
    while reader.read(&mut byte).unwrap() != 0 { original.push(byte[0]); }
    assert_eq!(original, bytes, "旁路不能改写原诊断解析器看到的字节");
    let events: Vec<_> = inbox.0.try_iter().filter(|event| event.kind == LogEventKind::Line).collect();
    assert_eq!(events.iter().map(|event| event.text.as_str()).collect::<Vec<_>>(), ["编译错误", "真实输出"]);
    assert!(events.iter().all(|event| event.source == LogSource::Stderr));
}

#[test]
fn endless_line_and_full_mailbox_stay_bounded_without_blocking_the_original_reader() {
    let (channel, inbox) = ProjectLogChannel::new("大量输出工程");
    let sink = channel.begin();
    let payload = vec![b'x'; LINE_BYTES * (CHANNEL_LINES + 7)];
    let mut reader = LogReader::new(payload.as_slice(), Some(sink), LogSource::Stdout);
    let mut original = Vec::new();
    reader.read_to_end(&mut original).unwrap();
    assert_eq!(original, payload);
    let events: Vec<_> = inbox.0.try_iter().collect();
    assert_eq!(events.iter().filter(|event| event.kind == LogEventKind::Line).count(), CHANNEL_LINES);
    assert!(events.iter().all(|event| event.text.len() <= LINE_BYTES));
    assert!(channel.dropped.load(Ordering::Relaxed) >= 7);
}

#[test]
fn cache_filters_wrong_owner_and_previous_job_and_preserves_a_paused_read_position() {
    let (channel, _) = ProjectLogChannel::new("当前工程");
    let mut state = ProjectLogState::new(&channel, "当前工程");
    let _sink = channel.begin();
    state.generation = channel.generation.load(Ordering::Acquire);
    let event = |owner, generation, text: String| LogEvent { owner, generation, sequence: 1, task: LogTask::default(), source: LogSource::Stdout, kind: LogEventKind::Line, text, spans: Vec::new() };
    let mut wrong_owner = channel.owner.clone();
    wrong_owner.instance ^= 1;
    state.adopt(event(wrong_owner, state.generation, "另一个作者实例".into()));
    state.adopt(event(channel.owner.clone(), state.generation - 1, "旧Job".into()));
    assert!(state.lines.is_empty());
    for index in 0..CACHE_LINES + 20 {
        state.adopt(event(channel.owner.clone(), state.generation, format!("{index}:{}", "x".repeat(LINE_BYTES / 2))));
    }
    assert!(state.lines.len() <= CACHE_LINES);
    assert!(state.bytes <= CACHE_BYTES);
    assert!(state.trimmed > 0);
    state.scroll_back = 30;
    state.follow = false;
    let before = state.text();
    state.adopt(event(channel.owner.clone(), state.generation, "继续输出".into()));
    assert_eq!(state.text(), before, "暂停跟随时新输出不能移动正在阅读的历史");
    state.follow = true;
    state.scroll_back = 0;
    assert!(state.text().ends_with("继续输出"));
}

#[test]
fn clear_drops_already_queued_output_without_stopping_or_restarting_the_log_session() {
    let (channel, inbox) = ProjectLogChannel::new("清空工程");
    let sink = channel.begin();
    let mut state = ProjectLogState::new(&channel, "清空工程");
    state.generation = channel.generation.load(Ordering::Acquire);
    let mut log = LogDecoder::new(Some(sink), LogSource::Stdout);
    log.write(b"queued before clear\n");
    state.clear();
    state.after_sequence = channel.sequence.load(Ordering::Acquire);
    for event in inbox.0.try_iter() { state.adopt(event); }
    assert!(state.lines.is_empty(), "清空后不能由邮箱积压重放旧行");
    log.write(b"new output\n");
    for event in inbox.0.try_iter() { state.adopt(event); }
    assert_eq!(state.text(), "new output");
    assert_eq!(channel.generation.load(Ordering::Acquire), 1);
}

#[test]
fn escape_only_closes_the_log_when_it_owns_keyboard_input() {
    let (channel, _) = ProjectLogChannel::new("只读浮窗");
    let mut app = App::new();
    app.insert_resource(ProjectLogState::new(&channel, "只读浮窗"))
        .init_resource::<super::search::LogSearch>()
        .init_resource::<InputFocus>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_message::<feathrise_editor_map::standalone::RunSavedProject>()
        .add_message::<AppExit>()
        .add_systems(Update, super::systems::control);
    app.world_mut().resource_mut::<ProjectLogState>().open = true;
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    app.update();
    assert!(app.world().resource::<ProjectLogState>().open);
    assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape));

    let body = app.world_mut().spawn(super::components::LogBody::default()).id();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    app.world_mut().resource_mut::<InputFocus>().set(body, bevy::input_focus::FocusCause::Pressed);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
    app.update();
    assert_eq!(app.world().resource::<InputFocus>().get(), None, "视口右键也会释放日志正文焦点");
    assert!(app.world().resource::<ProjectLogState>().open);

    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
    app.world_mut().resource_mut::<InputFocus>().set(body, bevy::input_focus::FocusCause::Pressed);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<ProjectLogState>().open);
    assert_eq!(app.world().resource::<InputFocus>().get(), None);
    assert!(!app.world().resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape));
    assert!(app.world().resource::<Messages<AppExit>>().is_empty());
    assert!(app.world().resource::<Messages<feathrise_editor_map::standalone::RunSavedProject>>().is_empty());
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
    app.update();
    assert!(!app.world().resource::<ProjectLogState>().owns_keyboard());
    assert_eq!(channel.generation.load(Ordering::Acquire), 0, "窗口开关不能创建构建或运行日志会话");
}

#[cfg(feature = "validation")]
#[test]
fn validation_tail_keeps_recent_utf8_within_both_budgets_without_moving_the_reader() {
    let (channel, _) = ProjectLogChannel::new("验收日志工程");
    let mut state = ProjectLogState::new(&channel, "验收日志工程");
    state.generation = 7;
    let mut sequence = 0;
    // 通过正式采用路径构建有来源、字节偏移和缓存预算的行，不绕开主线的新阅读身份。
    let mut push = |state: &mut ProjectLogState, text: String| {
        sequence += 1;
        state.adopt(LogEvent {
            owner: state.owner.clone(), generation: state.generation, sequence, task: LogTask::default(),
            source: LogSource::Stderr, kind: LogEventKind::Line, text, spans: Vec::new(),
        });
    };
    for index in 0..80 {
        push(&mut state, format!("第{index}行"));
    }
    state.follow = false;
    state.scroll_back = 21;
    state.dirty = false;
    let tail = state.validation_tail();
    let lines = tail["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 64);
    assert_eq!(lines.first().unwrap()["text"], "第16行");
    assert_eq!(lines.last().unwrap()["text"], "第79行");
    assert_eq!(tail["project"], "验收日志工程");
    assert_eq!(tail["generation"], 7);

    // 使用真实单行预算以内的多字节输出，迫使总字节预算落在汉字/emoji中间。
    state.clear();
    for index in 0..80 {
        push(&mut state, format!("{index:02}-{}", "中🀄".repeat(145)));
    }
    state.follow = false;
    state.scroll_back = 21;
    state.dirty = false;
    let before_offset = state.next_offset;
    let latest = state.lines.back().unwrap().text.clone();
    let tail = state.validation_tail();
    let lines = tail["lines"].as_array().unwrap();
    assert!(lines.len() <= 64);
    assert!(lines.iter().map(|line| line["text"].as_str().unwrap().len()).sum::<usize>() <= 16 * 1024);
    assert_eq!(lines.last().unwrap()["text"], latest);
    assert!(lines.iter().all(|line| line["source"] == "stderr" && !line["text"].as_str().unwrap().contains('\u{fffd}')));
    assert!(lines.first().unwrap()["text"].as_str().unwrap().len() < LINE_BYTES - 6);
    assert!(!state.follow);
    assert_eq!(state.scroll_back, 21);
    assert!(!state.dirty);
    assert_eq!(state.next_offset, before_offset, "只读诊断不得改变日志阅读字节身份");

    state.clear();
    push(&mut state, "ERROR shader真实错误".into());
    push(&mut state, String::new());
    assert_eq!(state.validation_tail()["lines"].as_array().unwrap().len(), 2, "尾部空行不能吞掉真实错误");
}
