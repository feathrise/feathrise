//! 后台标准流旁路：保留原诊断尾部与IPC，非阻塞发布完整日志行和可覆盖的实时尾行。

use super::{resources::*, types::*};
use std::{io::{self, Read}, sync::{atomic::Ordering, mpsc::TrySendError}};

#[derive(Clone)]
pub(crate) struct ProjectLogSink {
    pub(super) channel: ProjectLogChannel,
    pub(super) generation: u64,
    pub(super) task: LogTask,
}

impl std::fmt::Debug for ProjectLogSink {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("ProjectLogSink")
            .field("project", &self.channel.owner.project_id)
            .field("generation", &self.generation).finish()
    }
}

impl ProjectLogSink {
    fn current(&self) -> bool {
        self.channel.generation.load(Ordering::Acquire) == self.generation
            && self.channel.tasks[self.task.kind.index()].load(Ordering::Acquire) == self.task.number
    }

    fn event(&self, source: LogSource, kind: LogEventKind, text: String, spans: Vec<LogSpan>) -> bool {
        if !self.current() { return false; }
        let sequence = self.channel.sequence.fetch_add(1, Ordering::AcqRel) + 1;
        let event = LogEvent { owner: self.channel.owner.clone(), generation: self.generation, sequence, task: self.task, source, kind, text, spans };
        let slot = &self.channel.pending_tails[tail_index(self.task, source)];
        if kind == LogEventKind::Tail {
            let Ok(mut latest) = slot.try_lock() else { return false; };
            // 换任务与获得槽之间可能跨线程竞争；旧任务不能挤掉新任务的最新事实。
            if !self.current() || latest.as_ref().is_some_and(|pending|
                (pending.generation, pending.task.number, pending.sequence) > (self.generation, self.task.number, sequence)) { return false; }
            *latest = Some(event);
            return true;
        }
        match self.channel.sender.try_send(event) {
            Ok(()) => {
                if let Ok(mut latest) = slot.try_lock()
                    && latest.as_ref().is_some_and(|pending| pending.task == self.task && pending.sequence < sequence)
                { *latest = None; }
                true
            }
            Err(TrySendError::Full(_)) => {
                // 仅无法显示的完成行计溢出；进度覆盖不冒充历史丢行。原管道仍继续排空。
                self.channel.dropped.fetch_add(1, Ordering::Relaxed);
                false
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub(crate) fn task_label(&self) -> String { format!("{} #{}", self.task.kind.label(), self.task.number) }

    pub(crate) fn locale(&self) -> feathrise_editor_ui::localization::EditorLocale {
        self.channel.locale()
    }

    /// 归属只在短分隔行显示，原诊断及其复制正文不逐行插入前缀。
    pub(crate) fn task_message(&self, message: &str) {
        if self.task.kind == LogTaskKind::General { return; }
        // 只翻译编辑器写入的分隔行。外部标准流与已记录的正文、选区和字节坐标保持原样。
        let locale = self.locale();
        let title = locale.editor_label(self.task.kind.label());
        let message = locale.editor_notice(message.lines().next().unwrap_or_default());
        let mut text = format!("── {title} #{} · {message} ──", self.task.number);
        if text.len() > LINE_BYTES {
            let mut end = LINE_BYTES;
            while !text.is_char_boundary(end) { end -= 1; }
            text.truncate(end);
        }
        let _ = self.event(LogSource::Build, LogEventKind::Line, text, Vec::new());
    }
}

/// CSI参数也有固定预算，未闭合或恶意超长转义不能在后台无限积攒。
#[derive(Clone, Copy)]
struct CsiParameters {
    values: [u16; 16],
    len: usize,
    valid: bool,
}

impl CsiParameters {
    fn new() -> Self { Self { values: [0; 16], len: 1, valid: true } }

    fn push(&mut self, character: char) {
        if !self.valid { return; }
        if character == ';' {
            if self.len == self.values.len() { self.valid = false; } else { self.len += 1; }
        } else if let Some(digit) = character.to_digit(10) {
            let value = &mut self.values[self.len - 1];
            if let Some(next) = value.checked_mul(10).and_then(|value| value.checked_add(digit as u16)) {
                *value = next;
            } else { self.valid = false; }
        } else { self.valid = false; }
    }
}

#[derive(Clone, Copy, Default)]
enum Escape {
    #[default]
    Text,
    Start,
    Csi(CsiParameters),
    String,
    StringEnd,
}

pub(crate) struct LogDecoder {
    sink: Option<ProjectLogSink>,
    source: LogSource,
    escape: Escape,
    utf8: Vec<u8>,
    line: String,
    spans: Vec<LogSpan>,
    style: LogStyle,
    overwrite: bool,
    published_tail: Option<(String, Vec<LogSpan>)>,
}

impl LogDecoder {
    pub(crate) fn new(sink: Option<ProjectLogSink>, source: LogSource) -> Self {
        Self { sink, source, escape: Escape::Text, utf8: Vec::new(), line: String::new(),
            spans: Vec::new(), style: LogStyle::default(), overwrite: false, published_tail: None }
    }

    pub(crate) fn write(&mut self, bytes: &[u8]) {
        if self.sink.is_none() { return; }
        // 调用者即使一次交来完整编译诊断，解码暂存与临时字符串也固定在8KiB附近。
        for chunk in bytes.chunks(8192) { self.chunk(chunk); }
        self.publish_tail();
    }

    fn chunk(&mut self, bytes: &[u8]) {
        self.utf8.extend_from_slice(bytes);
        let mut at = 0;
        while at < self.utf8.len() {
            match std::str::from_utf8(&self.utf8[at..]) {
                Ok(text) => {
                    let text = text.to_owned();
                    self.characters(&text);
                    at = self.utf8.len();
                }
                Err(error) => {
                    let end = at + error.valid_up_to();
                    let text = std::str::from_utf8(&self.utf8[at..end]).expect("UTF-8已验证前缀").to_owned();
                    self.characters(&text);
                    if let Some(count) = error.error_len() {
                        self.characters("�");
                        at = end + count;
                    } else {
                        at = end;
                        break; // 最多保留3个跨read边界的UTF-8字节，下次继续解码。
                    }
                }
            }
        }
        self.utf8.drain(..at);
    }

    fn characters(&mut self, text: &str) {
        for character in text.chars() {
            self.escape = match self.escape {
                Escape::Start => match character {
                    '[' => Escape::Csi(CsiParameters::new()),
                    ']' | 'P' | '^' | '_' => Escape::String,
                    _ => Escape::Text,
                },
                Escape::Csi(mut parameters) => {
                    if ('\u{40}'..='\u{7e}').contains(&character) {
                        // Cargo用CR+擦除行撤回进度；仅剥序列会让旧Building永久留在尾部。
                        if parameters.valid {
                            let values = &parameters.values[..parameters.len];
                            if character == 'm' { self.sgr(values); }
                            if character == 'K' && values.len() == 1 && (values[0] == 2 || (values[0] == 0 && self.overwrite)) {
                                self.line.clear();
                                self.spans.clear();
                                self.overwrite = false;
                            }
                        }
                        Escape::Text
                    } else {
                        parameters.push(character);
                        Escape::Csi(parameters)
                    }
                },
                Escape::String => match character { '\u{7}' => Escape::Text, '\u{1b}' => Escape::StringEnd, _ => Escape::String },
                Escape::StringEnd => if character == '\\' { Escape::Text } else { Escape::String },
                Escape::Text => {
                    if character == '\u{1b}' { Escape::Start }
                    else {
                        match character {
                            '\n' => self.flush(),
                            // CR只是回到行首；等到正文才覆盖，CRLF仍只提交当前行一次。
                            '\r' => self.overwrite = true,
                            '\t' => self.character(' '),
                            character if !character.is_control() => self.character(character),
                            _ => {},
                        }
                        Escape::Text
                    }
                }
            };
        }
    }

    fn sgr(&mut self, parameters: &[u16]) {
        let mut at = 0;
        while at < parameters.len() {
            match parameters[at] {
                0 => self.style = LogStyle::default(),
                1 => self.style.bold = true,
                22 => self.style.bold = false,
                30..=37 => self.style.fg = Some(ansi_color(parameters[at] - 30)),
                90..=97 => self.style.fg = Some(ansi_color(parameters[at] - 90 + 8)),
                39 => self.style.fg = None,
                38 | 48 => {
                    let foreground = parameters[at] == 38;
                    let (color, count) = match parameters.get(at + 1) {
                        Some(5) if parameters.get(at + 2).is_some_and(|value| *value <= 255) =>
                            (ansi_color(parameters[at + 2]), 3),
                        Some(2) if parameters.get(at + 2..at + 5).is_some_and(|rgb| rgb.iter().all(|value| *value <= 255)) =>
                            ([parameters[at + 2] as u8, parameters[at + 3] as u8, parameters[at + 4] as u8], 5),
                        _ => break, // 不把不完整RGB的分量误认成reset或bold。
                    };
                    if foreground { self.style.fg = Some(color); }
                    // 暂不显示背景色，但必须消费完整参数，避免污染前景状态。
                    at += count;
                    continue;
                }
                _ => {},
            }
            at += 1;
        }
    }

    fn character(&mut self, character: char) {
        if self.overwrite {
            self.line.clear();
            self.spans.clear();
            self.overwrite = false;
        }
        if self.line.len() + character.len_utf8() > LINE_BYTES {
            self.flush(); // 恶意无换行输出也按固定字节分段，不能无限持有单行。
        }
        let start = self.line.len();
        self.line.push(character);
        if self.style == LogStyle::default() { return; }
        if let Some(last) = self.spans.last_mut() && last.end == start && last.style == self.style {
            last.end = self.line.len();
        } else if self.spans.len() < MAX_STYLE_SPANS {
            // 超预算仅放弃后续颜色，正文与原字节旁路仍完整保留。
            self.spans.push(LogSpan { start, end: self.line.len(), style: self.style });
        }
    }

    fn flush(&mut self) {
        self.overwrite = false;
        if self.line.is_empty() { return; }
        if let Some(sink) = &self.sink {
            let spans = self.line_spans();
            self.spans.clear();
            if sink.event(self.source, LogEventKind::Line, std::mem::take(&mut self.line), spans) {
                self.published_tail = None;
            }
        }
    }

    fn publish_tail(&mut self) {
        let spans = self.line_spans();
        if self.published_tail.as_ref().map_or(self.line.is_empty(), |(text, previous)| *text == self.line && *previous == spans) { return; }
        if let Some(sink) = &self.sink {
            // 同字的CR重绘也可能换色；纯控制刷新仍不重复挤占有界邮箱。
            if sink.event(self.source, LogEventKind::Tail, self.line.clone(), spans.clone()) {
                self.published_tail = if self.line.is_empty() { None } else { Some((self.line.clone(), spans)) };
            }
        }
    }

    fn line_spans(&self) -> Vec<LogSpan> {
        if !self.spans.is_empty() { return self.spans.clone(); }
        // Cargo经pipe默认关闭ANSI；只识别行首状态词，不把整个stderr误染成错误。
        let start = self.line.len() - self.line.trim_start_matches(' ').len();
        let text = &self.line[start..];
        let end = text.find(|character: char| !character.is_ascii_alphabetic()).unwrap_or(text.len());
        let color = match &text[..end] {
            "Compiling" | "Checking" | "Finished" | "Downloading" | "Downloaded" | "Updating" | "Locking" | "Adding" => 10,
            "Building" | "note" | "help" => 14,
            "warning" => 11,
            "error" => 9,
            _ => return Vec::new(),
        };
        if text[end..].chars().next().is_some_and(|character| !character.is_whitespace() && character != ':' && character != '[') {
            return Vec::new();
        }
        vec![LogSpan { start, end: start + end, style: LogStyle { fg: Some(ansi_color(color)), bold: true } }]
    }

    pub(crate) fn finish(&mut self) {
        if !self.utf8.is_empty() { self.utf8.clear(); self.characters("�"); }
        self.flush();
        self.publish_tail();
    }
}

fn ansi_color(index: u16) -> [u8; 3] {
    const BASIC: [[u8; 3]; 16] = [
        [0, 0, 0], [205, 49, 49], [13, 188, 121], [229, 229, 16],
        [36, 114, 200], [188, 63, 188], [17, 168, 205], [229, 229, 229],
        [102, 102, 102], [241, 76, 76], [35, 209, 139], [245, 245, 67],
        [59, 142, 234], [214, 112, 214], [41, 184, 219], [255, 255, 255],
    ];
    if index < 16 { return BASIC[index as usize]; }
    if index < 232 {
        let value = index - 16;
        let component = |value: u16| if value == 0 { 0 } else { (55 + value * 40) as u8 };
        return [component(value / 36), component(value / 6 % 6), component(value % 6)];
    }
    [8 + (index - 232) as u8 * 10; 3]
}

/// 在原读取线程里同时观察字节；不增加pipe/线程，也不改变原解析器的读取与退出合同。
pub(crate) struct LogReader<R> {
    reader: R,
    decoder: LogDecoder,
}

impl<R> LogReader<R> {
    pub(crate) fn new(reader: R, sink: Option<ProjectLogSink>, source: LogSource) -> Self {
        Self { reader, decoder: LogDecoder::new(sink, source) }
    }
}

impl<R: Read> Read for LogReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let count = self.reader.read(bytes)?;
        if count == 0 { self.decoder.finish(); } else { self.decoder.write(&bytes[..count]); }
        Ok(count)
    }
}

impl<R> Drop for LogReader<R> {
    fn drop(&mut self) { self.decoder.finish(); }
}
