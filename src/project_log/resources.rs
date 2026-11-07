//! 单工程作者实例的有界日志邮箱和终端阅读状态；关闭浮窗不影响生产者生命周期。

use super::{stream::ProjectLogSink, types::*};
use super::wrapping::LogWrapLayout;
use bevy::prelude::*;
use feathrise_editor_ui::localization::EditorLocale;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc},
};

pub(super) const CHANNEL_LINES: usize = 128;
pub(super) const CACHE_LINES: usize = 1024;
pub(super) const CACHE_BYTES: usize = 256 * 1024;
pub(super) const LINE_BYTES: usize = 1024;
pub(super) const FRAME_LINES: usize = 32;
pub(super) const VIEW_LINES: usize = 64;
pub(super) const MAX_STYLE_SPANS: usize = 64;
pub(super) const TAIL_SLOTS: usize = LogTaskKind::COUNT * 3;
pub(super) const LINE_HEIGHT: f32 = 20.0;

pub(super) fn tail_index(task: LogTask, source: LogSource) -> usize {
    task.kind.index() * 3 + match source { LogSource::Build => 0, LogSource::Stdout => 1, LogSource::Stderr => 2 }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum LogPosition {
    Committed(u64),
    Tail { task: LogTask, source: LogSource, byte: usize },
}

#[derive(Clone, Copy, Debug)]
pub(super) struct LogSelection {
    pub anchor: LogPosition,
    pub focus: LogPosition,
}

#[derive(Resource, Clone)]
pub(crate) struct ProjectLogChannel {
    english: Arc<AtomicBool>,
    pub(super) owner: LogOwner,
    pub(super) sender: mpsc::SyncSender<LogEvent>,
    pub(super) generation: Arc<AtomicU64>,
    pub(super) next_task: Arc<AtomicU64>,
    pub(super) tasks: Arc<[AtomicU64; LogTaskKind::COUNT]>,
    pub(super) sequence: Arc<AtomicU64>,
    pub(super) dropped: Arc<AtomicU64>,
    // 未换行进度只有每任务/来源一份最新事实，不能与完成行争抢128个FIFO槽。
    pub(super) pending_tails: Arc<[Mutex<Option<LogEvent>>; TAIL_SLOTS]>,
}

impl ProjectLogChannel {
    pub(super) fn new(project_id: &str) -> (Self, ProjectLogInbox) {
        let (sender, receiver) = mpsc::sync_channel(CHANNEL_LINES);
        let pending_tails = Arc::new(std::array::from_fn(|_| Mutex::new(None)));
        (Self {
            english: Arc::default(),
            owner: LogOwner {
                project_id: Arc::from(project_id),
                instance: bevy::asset::uuid::Uuid::new_v4().as_u128(),
            },
            sender,
            generation: Arc::new(AtomicU64::new(0)),
            next_task: Arc::new(AtomicU64::new(0)),
            tasks: Arc::new(std::array::from_fn(|_| AtomicU64::new(0))),
            sequence: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
            pending_tails: pending_tails.clone(),
        }, ProjectLogInbox(ProjectLogReceiver { receiver, pending_tails }))
    }

    /// 通用采集入口保留给自有验收夹具；生产任务必须显式标明所属目标。
    #[cfg(test)]
    pub(crate) fn begin(&self) -> ProjectLogSink {
        self.begin_task(LogTaskKind::General)
    }

    pub(crate) fn begin_task(&self, kind: LogTaskKind) -> ProjectLogSink {
        // generation只标记工程阅读会话，任务之间不再清空历史或屏蔽另一类Job。
        let generation = self.generation.fetch_max(1, Ordering::AcqRel).max(1);
        let number = self.next_task.fetch_add(1, Ordering::AcqRel) + 1;
        self.tasks[kind.index()].store(number, Ordering::Release);
        let sink = ProjectLogSink { channel: self.clone(), generation, task: LogTask { kind, number } };
        sink.task_message("开始准备工程");
        sink
    }

    pub(super) fn set_locale(&self, locale: EditorLocale) {
        self.english.store(locale == EditorLocale::English, Ordering::Relaxed);
    }

    pub(super) fn locale(&self) -> EditorLocale {
        if self.english.load(Ordering::Relaxed) { EditorLocale::English } else { EditorLocale::Chinese }
    }
}

pub(super) struct ProjectLogInbox(pub ProjectLogReceiver);

pub(super) struct ProjectLogReceiver {
    receiver: mpsc::Receiver<LogEvent>,
    pending_tails: Arc<[Mutex<Option<LogEvent>>; TAIL_SLOTS]>,
}

impl ProjectLogReceiver {
    pub(super) fn try_iter(&self) -> impl Iterator<Item = LogEvent> + '_ {
        // 完成行优先；调用者take(FRAME_LINES)仍限制总采用数。UI绝不等待生产者的锁。
        self.receiver.try_iter().chain(self.pending_tails.iter().filter_map(|slot| slot.try_lock().ok()?.take()))
    }

    #[cfg(test)]
    pub(super) fn try_recv(&self) -> Result<LogEvent, mpsc::TryRecvError> {
        self.try_iter().next().ok_or(mpsc::TryRecvError::Empty)
    }
}

#[derive(Resource)]
pub(crate) struct ProjectLogState {
    pub(super) owner: LogOwner,
    tasks: Arc<[AtomicU64; LogTaskKind::COUNT]>,
    /// 当前阅读快照中的任务代次；后台原子换代不能在一次排版中途改掉正文来源。
    current_tasks: [u64; LogTaskKind::COUNT],
    pub(super) generation: u64,
    pub(super) after_sequence: u64,
    pub(super) lines: VecDeque<LogLine>,
    pub(super) bytes: usize,
    pub(super) trimmed: u64,
    pub(super) dropped: u64,
    pub(super) open: bool,
    pub(super) captured: bool,
    pub(super) follow: bool,
    /// 默认按视口软换行；复制与选区身份始终使用未折行的原始UTF-8正文。
    pub(super) wrap: bool,
    pub(super) wrap_layout: LogWrapLayout,
    pub(super) content_revision: u64,
    pub(super) clear_revision: u64,
    pub(super) scroll_back: usize,
    pub(super) visible_rows: usize,
    pub(super) dirty: bool,
    pub(super) tails: [Option<LogEvent>; TAIL_SLOTS],
    last_line_sequences: [u64; TAIL_SLOTS],
    pub(super) next_offset: u64,
    pub(super) selection: Option<LogSelection>,
    pub(super) select_all: bool,
    pub(super) copy_selection: bool,
    pub(super) copy_all: bool,
    pub(super) notice: String,
    pub(super) last_scroll_y: f32,
    pub(super) content_width: f32,
}

impl ProjectLogState {
    pub(super) fn new(channel: &ProjectLogChannel, _project_name: &str) -> Self {
        Self {
            owner: channel.owner.clone(), generation: 0, after_sequence: 0,
            tasks: channel.tasks.clone(),
            current_tasks: std::array::from_fn(|index| channel.tasks[index].load(Ordering::Acquire)),
            lines: VecDeque::new(), bytes: 0, trimmed: 0, dropped: 0, open: false,
            captured: false, follow: true, wrap: true, wrap_layout: LogWrapLayout::default(),
            content_revision: 0, clear_revision: 0, scroll_back: 0, visible_rows: 20, dirty: true,
            tails: std::array::from_fn(|_| None), last_line_sequences: [0; TAIL_SLOTS],
            next_offset: 0, selection: None, select_all: false,
            copy_selection: false, copy_all: false, notice: String::new(),
            last_scroll_y: 0., content_width: 0.,
        }
    }

    pub(crate) fn captures_input(&self) -> bool { self.open || self.captured }

    /// 只读本轮任务已经进入工程日志的命令，避免用菜单选择代替真实构建证据。
    #[cfg(all(windows, feature = "validation"))]
    pub(crate) fn validation_build_command(&self, kind: LogTaskKind) -> Option<&str> {
        let task = self.current_tasks[kind.index()];
        self.lines.iter().rev().find(|line| line.task == (LogTask { kind, number: task })
            && line.source == LogSource::Build && line.text.starts_with("& "))
            .map(|line| line.text.as_str())
    }

    /// 显式验收失败时摘取当前工程的真实输出；不改变终端阅读状态，也不复制整个缓存。
    #[cfg(feature = "validation")]
    pub(crate) fn validation_tail(&self) -> serde_json::Value {
        let mut remaining = 16 * 1024;
        let mut lines = Vec::new();
        for line in self.lines.iter().rev().take(64) {
            let mut end = line.text.len().min(remaining);
            while !line.text.is_char_boundary(end) { end -= 1; }
            if end == 0 && !line.text.is_empty() { break; }
            lines.push(serde_json::json!({"source":line.source.label(),"text":&line.text[..end],
                "task":line.task.number,"target":line.task.kind.label()}));
            remaining -= end;
        }
        lines.reverse();
        serde_json::json!({"project":self.owner.project_id.as_ref(),
            "instance":format!("{:032x}",self.owner.instance),"generation":self.generation,
            "trimmed":self.trimmed,"dropped":self.dropped,"lines":lines,
            "maximum_lines":64,"maximum_text_bytes":16 * 1024})
    }

    pub(super) fn clear(&mut self) {
        self.clear_revision = self.clear_revision.wrapping_add(1);
        self.content_revision = self.content_revision.wrapping_add(1);
        self.wrap_layout = LogWrapLayout::default();
        self.lines.clear();
        self.bytes = 0;
        self.trimmed = 0;
        self.scroll_back = 0;
        self.follow = true;
        self.tails = std::array::from_fn(|_| None);
        self.last_line_sequences = [0; TAIL_SLOTS];
        self.next_offset = 0;
        self.selection = None;
        self.select_all = false;
        self.notice.clear();
        self.content_width = 0.;
        self.dirty = true;
    }

    pub(super) fn adopt(&mut self, event: LogEvent) {
        self.refresh_tasks();
        if event.owner != self.owner || event.generation != self.generation
            || event.sequence <= self.after_sequence || !self.current_task(event.task) { return; }
        let source = tail_index(event.task, event.source);
        // latest尾行与FIFO独立轮询；跨线程迟到的旧尾行不能复活已提交内容。
        if event.kind == LogEventKind::Tail && (event.sequence <= self.last_line_sequences[source]
            || self.tails[source].as_ref().is_some_and(|tail|
                tail.task == event.task && tail.sequence >= event.sequence)) { return; }
        self.content_revision = self.content_revision.wrapping_add(1);
        let previous_rows = self.row_count();
        let newer_tail = self.tails[source].as_ref().is_some_and(|tail|
            tail.task == event.task && tail.sequence > event.sequence);
        if event.kind == LogEventKind::Line {
            self.last_line_sequences[source] = self.last_line_sequences[source].max(event.sequence);
            if !newer_tail && let Some(selection) = &mut self.selection {
                for endpoint in [&mut selection.anchor, &mut selection.focus] {
                    if let LogPosition::Tail { task, source, byte } = *endpoint && task == event.task && source == event.source {
                        let mut byte = byte.min(event.text.len());
                        while !event.text.is_char_boundary(byte) { byte -= 1; }
                        *endpoint = LogPosition::Committed(self.next_offset + byte as u64);
                    }
                }
            }
        } else if event.text.is_empty() && self.selection.is_some_and(|selection| {
            [selection.anchor, selection.focus].iter().any(|endpoint|
                matches!(endpoint, LogPosition::Tail { task, source, .. } if *task == event.task && *source == event.source))
        }) {
            // 临时行被终端擦除后撤回选区，不能把同一字节位置误套到另一来源。
            self.selection = None;
        }
        // 尚未消费完的旧完成行仍应进入历史，但不能清除更新尾行或挪用其选区。
        if !newer_tail { self.tails[source] = None; }
        if event.kind == LogEventKind::Tail {
            if !event.text.is_empty() { self.tails[source] = Some(event); }
            if !self.follow && (!self.wrap || self.wrap_layout.rows.is_empty()) { self.scroll_back += self.row_count().saturating_sub(previous_rows); }
            self.clamp_scroll();
            self.dirty = true;
            return;
        }
        // 样式与正文共享缓存预算，频繁切色不能绕过256KiB上限。
        self.bytes += event.text.len() + event.spans.len() * std::mem::size_of::<LogSpan>() + std::mem::size_of::<LogTask>();
        let start = self.next_offset;
        self.next_offset += event.text.len() as u64 + 1;
        self.lines.push_back(LogLine {
            #[cfg(feature = "validation")]
            task: event.task,
            #[cfg(feature = "validation")]
            source: event.source,
            text: event.text, spans: event.spans, start,
        });
        if !self.follow && (!self.wrap || self.wrap_layout.rows.is_empty()) { self.scroll_back += self.row_count().saturating_sub(previous_rows); }
        while self.lines.len() > CACHE_LINES || self.bytes > CACHE_BYTES {
            if let Some(line) = self.lines.pop_front() {
                self.bytes -= line.text.len() + line.spans.len() * std::mem::size_of::<LogSpan>() + std::mem::size_of::<LogTask>();
                self.trimmed += 1;
            }
        }
        self.clamp_scroll();
        self.dirty = true;
    }

    pub(super) fn clamp_scroll(&mut self) {
        self.scroll_back = self.scroll_back.min(self.display_row_count().saturating_sub(self.visible_rows));
    }

    pub(super) fn row_count(&self) -> usize {
        self.lines.len() + self.current_tails().count()
    }

    pub(super) fn display_row_count(&self) -> usize {
        if self.wrap && !self.wrap_layout.rows.is_empty() { self.wrap_layout.rows.len() } else { self.row_count() }
    }

    fn current_task(&self, task: LogTask) -> bool {
        task.number == 0 || self.tasks[task.kind.index()].load(Ordering::Acquire) == task.number
    }

    pub(super) fn refresh_tasks(&mut self) {
        let current = std::array::from_fn(|index| self.tasks[index].load(Ordering::Acquire));
        if self.current_tasks != current {
            self.current_tasks = current;
            self.content_revision = self.content_revision.wrapping_add(1);
            self.dirty = true;
        }
    }

    pub(super) fn current_tails(&self) -> impl Iterator<Item = &LogEvent> {
        self.tails.iter().flatten().filter(|tail| tail.task.number == 0
            || self.current_tasks[tail.task.kind.index()] == tail.task.number)
    }

    pub(super) fn first_row(&self) -> usize {
        self.display_row_count().saturating_sub(self.scroll_back + self.visible_rows.min(VIEW_LINES))
    }

    /// 临时尾行固定按任务类别和来源排列；已完成行保持单调字节身份，不移动选区。
    pub(super) fn rows(&self) -> impl Iterator<Item = (u64, &str)> {
        let mut offset = self.next_offset;
        self.lines.iter().map(|line| (line.start, line.text.as_str())).chain(
            self.current_tails().map(move |tail| {
                let start = offset;
                offset += tail.text.len() as u64 + 1;
                (start, tail.text.as_str())
            }),
        )
    }

    pub(super) fn visible_text(&self) -> (u64, String) {
        if self.wrap && self.wrap_layout.revision == Some(self.content_revision) && !self.wrap_layout.rows.is_empty() {
            let (origin, text, _) = self.visible_styled_text();
            return (origin, text);
        }
        let first = self.row_count().saturating_sub(self.scroll_back + self.visible_rows.min(VIEW_LINES));
        let mut rows = self.rows().skip(first).take(self.visible_rows.min(VIEW_LINES));
        let Some((origin, first)) = rows.next() else { return (0, self.text()); };
        let mut text = first.to_owned();
        for (_, row) in rows { text.push('\n'); text.push_str(row); }
        (origin, text)
    }

    /// 富色排版只拿当前最多64行，区间相对可见正文，不复制整份历史缓存。
    pub(super) fn visible_styled_text(&self) -> (u64, String, Vec<LogSpan>) {
        if self.wrap && self.wrap_layout.revision == Some(self.content_revision) && !self.wrap_layout.rows.is_empty() {
            let first = self.first_row();
            let rows = &self.wrap_layout.rows;
            let origin = rows[first].start;
            let end = rows[(first + self.visible_rows.min(VIEW_LINES)).min(rows.len()).saturating_sub(1)].end;
            return self.styled_range(origin, end);
        }
        let mut offset = self.next_offset;
        let rows = self.lines.iter().map(|line| (line.start, line.text.as_str(), line.spans.as_slice())).chain(
            self.current_tails().map(move |tail| {
                let start = offset;
                offset += tail.text.len() as u64 + 1;
                (start, tail.text.as_str(), tail.spans.as_slice())
            }),
        );
        let first = self.row_count().saturating_sub(self.scroll_back + self.visible_rows.min(VIEW_LINES));
        let mut rows = rows.skip(first).take(self.visible_rows.min(VIEW_LINES));
        let Some((origin, first, first_spans)) = rows.next() else { return (0, self.text(), Vec::new()); };
        let mut text = first.to_owned();
        let mut spans = first_spans.to_vec();
        for (_, row, row_spans) in rows {
            text.push('\n');
            let start = text.len();
            text.push_str(row);
            spans.extend(row_spans.iter().map(|span| LogSpan {
                start: start + span.start, end: start + span.end, style: span.style,
            }));
        }
        (origin, text, spans)
    }

    /// 视觉行只提供原文字节边界，不插入软换行字符；原生拖选、复制仍共享全局偏移。
    fn styled_range(&self, origin: u64, end: u64) -> (u64, String, Vec<LogSpan>) {
        let mut offset = self.next_offset;
        let rows = self.lines.iter().map(|line| (line.start, line.text.as_str(), line.spans.as_slice())).chain(
            self.current_tails().map(move |tail| {
                let start = offset;
                offset += tail.text.len() as u64 + 1;
                (start, tail.text.as_str(), tail.spans.as_slice())
            }),
        );
        let mut text = String::new();
        let mut spans = Vec::new();
        let mut first = true;
        for (start, row, styles) in rows {
            if start + (row.len() as u64) < origin { continue; }
            if start > end { break; }
            let from = origin.saturating_sub(start).min(row.len() as u64) as usize;
            let to = end.saturating_sub(start).min(row.len() as u64) as usize;
            if !first { text.push('\n'); }
            first = false;
            let base = text.len();
            text.push_str(&row[from..to]);
            for style in styles {
                let left = style.start.max(from);
                let right = style.end.min(to);
                if left < right { spans.push(LogSpan { start: base + left - from, end: base + right - from, style: style.style }); }
            }
        }
        (origin, text, spans)
    }

    pub(super) fn position_at(&self, offset: u64) -> LogPosition {
        let mut start = self.next_offset;
        for tail in self.current_tails() {
            let end = start + tail.text.len() as u64;
            if (start..=end).contains(&offset) {
                return LogPosition::Tail { task: tail.task, source: tail.source, byte: (offset - start) as usize };
            }
            start = end + 1;
        }
        LogPosition::Committed(offset)
    }

    pub(super) fn offset_of(&self, position: LogPosition) -> Option<u64> {
        let LogPosition::Tail { task, source, byte } = position else {
            let LogPosition::Committed(offset) = position else { unreachable!() };
            return Some(offset);
        };
        let mut offset = self.next_offset;
        for tail in self.current_tails() {
            if tail.task == task && tail.source == source {
                let mut at = byte.min(tail.text.len());
                while !tail.text.is_char_boundary(at) { at -= 1; }
                return Some(offset + at as u64);
            }
            offset += tail.text.len() as u64 + 1;
        }
        None
    }

    /// 复制只在明确操作时遍历有界缓存；不把全量日志交给每帧字体排版。
    pub(super) fn copy_text(&self, all: bool) -> String {
        let bounds = if all || self.select_all { None } else {
            let Some(selection) = self.selection else { return String::new(); };
            let (Some(anchor), Some(focus)) = (self.offset_of(selection.anchor), self.offset_of(selection.focus)) else { return String::new(); };
            Some(anchor.min(focus)..anchor.max(focus))
        };
        let mut text = String::new();
        for (start, row) in self.rows() {
            for (at, character) in row.char_indices().chain(std::iter::once((row.len(), '\n'))) {
                let position = start + at as u64;
                if bounds.as_ref().is_none_or(|range| range.contains(&position)) { text.push(character); }
            }
        }
        if bounds.is_none() { text.pop(); }
        text
    }

    pub(super) fn text(&self) -> String {
        if self.row_count() == 0 {
            return if self.generation == 0 {
                "当前工程还没有构建或运行输出。\n启动或预览工程后，真实的编译日志和运行输出会显示在这里。".into()
            } else {
                "等待本次构建或运行输出…".into()
            };
        }
        // 只排版当前可见逻辑行，滚轮移动阅读窗口；高频日志不会迫使UI重排全部缓存。
        self.visible_text().1
    }
}
