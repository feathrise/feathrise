//! DX stdout的展示适配：原始机器回执留给产物解析，真实诊断显示一次，后台邮箱不接收过程噪声。

use super::Diagnostics;
use crate::{compiler_diagnostics::{CompilerDiagnostic, DIAGNOSTIC_LIMIT}, project_log::{LogDecoder, LogSource, ProjectLogSink}};
use serde_json::Value;
use std::{borrow::Cow, path::Path};

pub(crate) struct OutputLog<'a> {
    root: &'a Path,
    decoder: LogDecoder,
    callback: Diagnostics,
    // 与原诊断上限共用固定预算；原记录与RustcOutput镜像不能各占一次展示/定位回执。
    seen: Vec<CompilerDiagnostic>,
    // DX非TTY的Compiled计数用LF结束，语义仍是当前进度，不能提交成历史正文。
    compiled_progress: bool,
}

impl<'a> OutputLog<'a> {
    pub(crate) fn new(root: &'a Path, sink: Option<ProjectLogSink>, callback: Diagnostics) -> Self {
        Self { root, decoder: LogDecoder::new(sink, LogSource::Build), callback, seen: Vec::new(), compiled_progress: false }
    }

    pub(crate) fn emit_line(&mut self, line: &[u8]) {
        // read_output已跨读取块拼完LF行；CR覆盖可把过程与真正错误放在同一物理行，须逐段分流。
        for part in line.split_inclusive(|byte| matches!(*byte, b'\r' | b'\n')) { self.emit_part(part); }
    }

    fn emit_part(&mut self, part: &[u8]) {
        if let Some(plain) = visible_text(part) {
            if let Ok(value) = serde_json::from_str::<Value>(plain.trim_ascii()) {
                let inner = value.get("json").and_then(Value::as_str)
                    .and_then(|text| serde_json::from_str::<Value>(text).ok());
                let record = inner.as_ref().unwrap_or(&value);
                if self.structured(record) { return; }
                // 只拆DX明确的tracing外壳；未知JSON字段与普通工程JSON原样保留。
                if let Some(message) = tracing_message(&value) {
                    self.human_record(message);
                    return;
                }
            }
            if cargo_process_text(&plain) { return; }
            if dx_compiled_progress(&plain) {
                let end = part.iter().rposition(|byte| !matches!(*byte, b'\r' | b'\n')).map_or(0, |at| at + 1);
                let mut redraw = b"\r\x1b[2K".to_vec();
                redraw.extend_from_slice(&part[..end]);
                self.decoder.write(&redraw);
                self.compiled_progress = true;
                return;
            }
            // CRLF在emit_line中被拆成两段，LF不能再次把刚识别的语义进度提交到历史。
            if self.compiled_progress && part == b"\n" { return; }
            self.clear_compiled_progress();
            if part.ends_with(b"\r") && diagnostic_text(&plain) {
                // 折叠后续过程行不能让前面的真实诊断只留在会被覆盖的临时尾槽。
                let mut committed = part[..part.len() - 1].to_vec();
                committed.push(b'\n');
                self.decoder.write(&committed);
                return;
            }
        }
        self.clear_compiled_progress();
        self.decoder.write(part);
    }

    fn structured(&mut self, record: &Value) -> bool {
        if record.get("BuildsFinished").is_some() || record.get("Hotpatch").is_some()
            || record.get("BuildFinished").is_some() || record.get("BuildUpdate").is_some() {
            return true;
        } // 完整client、跳表和进度机器回执仅保留在原raw tail，绝不塞进日志邮箱。
        if let Some(message) = record.get("Error").and_then(|error| error.get("message")).and_then(Value::as_str) {
            self.human_record(message);
            return true;
        }
        match record.get("reason").and_then(Value::as_str) {
            Some("compiler-artifact" | "build-script-executed" | "build-finished") => return true,
            Some("compiler-message") => return self.diagnostic(&record["message"]),
            _ => {}
        }
        match record.get("$message_type").and_then(Value::as_str) {
            Some("artifact") => return true,
            Some("diagnostic") => return self.diagnostic(record),
            _ => {}
        }
        if let Some(object) = record.as_object()
            && object.keys().all(|key| matches!(key.as_str(), "artifact" | "emit" | "$message_type"))
            && record.get("artifact").is_some_and(Value::is_string)
            && record.get("emit").is_some_and(Value::is_string) {
            return true;
        }
        if let Some(message) = record.get("RustcOutput").and_then(|output| output.get("message")) {
            return self.diagnostic(message);
        }
        false
    }

    fn diagnostic(&mut self, message: &Value) -> bool {
        let envelope = serde_json::json!({"reason":"compiler-message","message":message});
        let Some(diagnostic) = compiler_diagnostic(&envelope, self.root) else { return false; };
        if self.seen.contains(&diagnostic) { return true; }
        if self.seen.len() < DIAGNOSTIC_LIMIT {
            if let Some(callback) = &self.callback { callback(diagnostic.clone()); }
            self.seen.push(diagnostic.clone());
        }
        // 即使超过定位预算，真实诊断正文仍可见；去重缓存永远不随工具输出无限增长。
        let code = diagnostic.code.as_ref().map_or(String::new(), |code| format!("[{code}]"));
        let fallback = format!("{}{code}: {}", message["level"].as_str().unwrap_or("diagnostic"), diagnostic.message);
        let text = message.get("rendered").and_then(Value::as_str).filter(|text| !text.is_empty()).unwrap_or(&fallback);
        self.clear_compiled_progress();
        // rendered是已确认诊断正文，不再套过程过滤，避免正文中的代码/提示碰巧像Cargo状态词。
        if text.ends_with('\n') { self.decoder.write(text.as_bytes()); }
        else {
            let mut line = text.as_bytes().to_vec(); line.push(b'\n');
            self.decoder.write(&line);
        }
        true
    }

    fn human_record(&mut self, text: &str) {
        if text.ends_with('\n') { self.emit_line(text.as_bytes()); }
        else {
            let mut line = text.as_bytes().to_vec(); line.push(b'\n');
            self.emit_line(&line);
        }
    }

    fn clear_compiled_progress(&mut self) {
        if self.compiled_progress {
            self.decoder.write(b"\r\x1b[2K");
            self.compiled_progress = false;
        }
    }

    pub(crate) fn finish(&mut self) {
        self.clear_compiled_progress();
        self.decoder.finish();
    }
}

/// 只识别DX明确的计数／crate格式；total=0是实际工具输出的未知总量，不能误当非法日志。
fn dx_compiled_progress(text: &str) -> bool {
    let Some(text) = text.trim_ascii().strip_prefix("Compiled [") else { return false; };
    let Some((counts, name)) = text.split_once("]: ") else { return false; };
    let Some((current, total)) = counts.split_once('/') else { return false; };
    let count = |value: &str| {
        let value = value.trim_ascii();
        !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) && value.parse::<u64>().is_ok()
    };
    count(current) && count(total) && !name.is_empty() && name.len() <= 256
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn tracing_message(value: &Value) -> Option<&str> {
    let object = value.as_object()?;
    if !value.get("timestamp")?.is_string()
        || !matches!(value.get("level")?.as_str()?, "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR")
        || value.get("json").is_some()
        || object.keys().any(|key| !matches!(key.as_str(), "timestamp" | "level" | "message" | "json" | "dx_src" | "target" | "path" | "telemetry")) {
        return None;
    }
    value.get("message")?.as_str()
}

fn diagnostic_text(text: &str) -> bool {
    let text = text.trim_ascii_start();
    ["error:", "error[", "warning:", "warning["].iter().any(|prefix| text.starts_with(prefix))
}

/// 仅为识别去掉SGR和行首/行末擦行；中间擦除、未知控制或坏转义不猜测，交回正式终端。
fn visible_text(line: &[u8]) -> Option<Cow<'_, str>> {
    let text = std::str::from_utf8(line).ok()?;
    if !text.contains('\u{1b}') { return Some(Cow::Borrowed(text)); }
    let mut plain = String::new();
    let mut rest = text;
    let mut erased_after_text = false;
    while let Some(at) = rest.find('\u{1b}') {
        if erased_after_text && !rest[..at].trim_ascii().is_empty() { return None; }
        plain.push_str(&rest[..at]);
        let sgr = rest[at + 1..].strip_prefix('[')?;
        let end = sgr.find(|character: char| !character.is_ascii_digit() && character != ';' && character != ':')?;
        match sgr.as_bytes()[end] {
            b'm' => {},
            b'K' if matches!(&sgr[..end], "" | "0" | "1" | "2") => {
                erased_after_text |= !plain.trim_ascii().is_empty();
            }
            _ => return None,
        }
        rest = &sgr[end + 1..];
    }
    if erased_after_text && !rest.trim_ascii().is_empty() { return None; }
    plain.push_str(rest);
    Some(Cow::Owned(plain))
}

#[cfg(test)]
pub(super) fn cargo_verbose_process_line(line: &[u8]) -> bool {
    visible_text(line).is_some_and(|text| cargo_process_text(&text))
}

fn cargo_process_text(text: &str) -> bool {
    let text = text.trim_ascii();
    if text.bytes().any(|byte| byte.is_ascii_control() && byte != b'\t') { return false; }
    let mut words = text.split_ascii_whitespace();
    match words.next() {
        Some("Fresh" | "Dirty") => {
            let Some(package) = words.next() else { return false; };
            let Some(version) = words.next().and_then(|value| value.strip_prefix('v')) else { return false; };
            let mut version = version.split(['-', '+', ':']).next().unwrap().split('.');
            package.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                && (0..3).all(|_| version.next().is_some_and(|part|
                    !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())))
                && version.next().is_none()
        }
        Some("Running") => text.strip_prefix("Running").is_some_and(|command| {
            let command = command.trim_ascii();
            command.len() > 2 && command.starts_with('`') && command.ends_with('`')
        }),
        _ => false,
    }
}

pub(super) fn compiler_diagnostic(record: &Value, root: &Path) -> Option<CompilerDiagnostic> {
    if record["$message_type"] == "diagnostic" {
        crate::compiler_diagnostics::from_cargo(&serde_json::json!({"reason":"compiler-message","message":record}), root)
    } else { crate::compiler_diagnostics::from_cargo(record, root) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[test]
    fn dx_lf_crlf_and_tracing_compiled_updates_share_one_bounded_tail_and_disappear_at_eof() {
        let (sink, probe) = crate::project_log::fixture_log_probe("dx-compiled-progress");
        let root = std::env::current_dir().unwrap();
        let mut output = OutputLog::new(&root, Some(sink), None);
        for index in 1..=889 {
            let text = format!("Compiled [{index:3}/889]: fixture_{index}");
            match index % 3 {
                0 => output.emit_line(format!("{text}\n").as_bytes()),
                1 => output.emit_line(format!("\x1b[32m{text}\x1b[0m\r\n").as_bytes()),
                _ => output.emit_line(format!("{}\n", json!({"timestamp":"1s","level":"INFO","message":text})).as_bytes()),
            }
        }
        let events = probe.drain();
        assert_eq!(events.len(), 1, "889次进度不得消耗889个完成行FIFO槽");
        assert_eq!(events[0].kind, "tail");
        assert_eq!(events[0].text, "Compiled [889/889]: fixture_889");
        // 进度替换与 EOF 撤回必须仍归属同一真实任务，并以新序号防止旧尾行重放。
        assert_eq!((events[0].target, events[0].task), (crate::project_log::LogTaskKind::General, 1));
        let sequence = events[0].sequence;
        assert!(sequence > 0);
        assert_eq!(probe.dropped(), 0);
        output.finish();
        let events = probe.drain();
        assert_eq!(events.len(), 1);
        assert_eq!((events[0].kind, events[0].text.as_str()), ("tail", ""));
        assert_eq!((events[0].target, events[0].task), (crate::project_log::LogTaskKind::General, 1));
        assert!(events[0].sequence > sequence);
        output.finish();
        assert!(probe.drain().is_empty());
    }

    #[test]
    fn compiled_progress_never_joins_real_compiling_lines_or_structured_diagnostics() {
        let (sink, probe) = crate::project_log::fixture_log_probe("dx-mixed-compiled-progress");
        let root = std::env::current_dir().unwrap();
        let mut output = OutputLog::new(&root, Some(sink), None);
        for (index, package, krate) in [(417, "generational-box", "generational_box"), (418, "warnings", "warnings"),
            (419, "tracing-subscriber", "bevy_ecs_macros"), (420, "skrifa", "skrifa")]
        {
            output.emit_line(format!("   Compiling {package} v0.7.10\n").as_bytes());
            output.emit_line(format!("Compiled [{index}/889]: {krate}\n").as_bytes());
        }
        // total=0来自真实DX记录；已确认诊断正文中同形的源码行仍按正文保留。
        output.emit_line(b"Compiled [  2/0]: fixture_leaf\n");
        let diagnostic = json!({"$message_type":"diagnostic","message":"fixture error","level":"error","spans":[],
            "rendered":"error: fixture error\nCompiled [1/2]: source_example\n"});
        output.emit_line(format!("{diagnostic}\n").as_bytes());
        output.emit_line(b"Compiled [889/889]: fixture\n");
        output.emit_line(b"    Finished `render-dev` profile in 1s\n");
        output.finish();
        let events = probe.drain();
        let lines: Vec<_> = events.iter().filter(|event| event.kind == "line").map(|event| event.text.as_str()).collect();
        assert_eq!(lines, ["   Compiling generational-box v0.7.10", "   Compiling warnings v0.7.10",
            "   Compiling tracing-subscriber v0.7.10", "   Compiling skrifa v0.7.10", "error: fixture error",
            "Compiled [1/2]: source_example", "    Finished `render-dev` profile in 1s"]);
        assert_eq!(probe.dropped(), 0);
    }

    #[test]
    fn only_complete_dx_compiled_counter_records_are_progress() {
        for text in ["Compiled [417/889]: generational_box", "Compiled [  2/0]: fixture-leaf"] {
            assert!(dx_compiled_progress(text));
        }
        for text in ["Compiled project successfully", "Compiled [x/889]: game", "Compiled [1/2]: game failed",
            "Compiled [+1/2]: game", "Compiled [1/2/3]: game", "Compiled [1/2]: ",
            "warning: Compiled [1/2]: game", "Compiled [18446744073709551616/2]: game"]
        {
            assert!(!dx_compiled_progress(text), "未知正文必须保留：{text}");
        }
    }

    #[test]
    fn machine_bursts_leave_real_mailbox_space_for_human_output_and_unique_diagnostics() {
        let (sink, probe) = crate::project_log::fixture_log_probe("dx-wire-display");
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let received = diagnostics.clone();
        let callback: Arc<dyn Fn(CompilerDiagnostic) + Send + Sync> = Arc::new(move |diagnostic| {
            received.lock().unwrap().push(diagnostic);
        });
        let root = std::env::var_os("FEATHRISE_DX_REPLAY_PROJECT_ROOT")
            .map(std::path::PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap());
        assert!(root.is_absolute() && root.is_dir());
        let mut output = OutputLog::new(&root, Some(sink), Some(callback));
        // 与Cargo的实际wire字段同形；测试不使用空sink或直接写成功状态。
        let artifact = json!({"reason":"compiler-artifact","package_id":"registry+https://github.com/rust-lang/crates.io-index#cfg-if@1.0.5",
            "manifest_path":"C:/fixture/cfg-if/Cargo.toml","target":{"kind":["lib"],"crate_types":["lib"],"name":"cfg_if",
            "src_path":"C:/fixture/cfg-if/src/lib.rs","edition":"2018","doc":true,"doctest":true,"test":true},
            "profile":{"opt_level":"3","debuginfo":0,"debug_assertions":true,"overflow_checks":true,"test":false},
            "features":[],"filenames":["C:/fixture/target/libcfg_if.rlib"],"executable":null,"fresh":true});
        let wrapped_artifact = json!({"timestamp":"1s","level":"INFO","json":artifact.to_string()});
        let records = [artifact, wrapped_artifact, json!({"reason":"build-script-executed","package_id":"path+file:///fixture#build@0.1.0",
            "linked_libs":[],"linked_paths":[],"cfgs":[],"env":[],"out_dir":"C:/fixture/target/build/out"}),
            json!({"reason":"build-finished","success":true}),
            json!({"$message_type":"artifact","artifact":"C:/fixture/target/game.exe","emit":"link"}),
            json!({"BuildUpdate":{"stage":"Starting"}})];
        for index in 0..2048 {
            output.emit_line(format!("{}\n", records[index % records.len()]).as_bytes());
            output.emit_line(b"\r\x1b[K\x1b[32m       Fresh\x1b[0m cfg-if v1.0.5\n");
            output.emit_line(b"       Dirty avian_derive v0.2.3: the dependency `proc-macro-error3` was rebuilt\n");
            output.emit_line(b"\x1b[2K     Running `rustc --crate-name cfg_if src/lib.rs`\n");
        }
        assert!(probe.drain().is_empty(), "机器正文和纯过程不能进入真实128更新邮箱");
        assert_eq!(probe.dropped(), 0);
        output.emit_line(b"   Compiling game v0.1.0\n");
        output.emit_line(b"    Finished `render-dev` profile in 1s\n");
        let error = json!({"$message_type":"diagnostic","message":"mismatched types","level":"error",
            "code":{"code":"E0308"},"spans":[],"rendered":"error[E0308]: mismatched types\n  --> src/main.rs:2:1\n"});
        output.emit_line(format!("{error}\n").as_bytes());
        let mirror = json!({"timestamp":"1s","level":"INFO","json":json!({"RustcOutput":{"message":error}}).to_string()});
        output.emit_line(format!("{mirror}\n").as_bytes());
        let warning = json!({"reason":"compiler-message","message":{"message":"unused variable","level":"warning",
            "code":{"code":"unused_variables"},"spans":[],"rendered":"warning: unused variable\n"}});
        output.emit_line(format!("{warning}\n").as_bytes());
        output.emit_line(b"ordinary custom output\n");
        let tracing_process = json!({"timestamp":"1s","level":"INFO",
            "message":"       Fresh cfg-if v1.0.5\n     Running `rustc --crate-name cfg_if`\n"});
        output.emit_line(format!("{tracing_process}\n").as_bytes());
        let unknown = json!({"event":"custom","message":"keep JSON fields","detail":7});
        output.emit_line(format!("{unknown}\n").as_bytes());
        output.finish();
        let events = probe.drain();
        let lines: Vec<_> = events.iter().filter(|event| event.kind == "line").map(|event| event.text.as_str()).collect();
        let unknown_text = unknown.to_string();
        assert_eq!(lines, ["   Compiling game v0.1.0", "    Finished `render-dev` profile in 1s",
            "error[E0308]: mismatched types", "  --> src/main.rs:2:1", "warning: unused variable",
            "ordinary custom output", unknown_text.as_str()]);
        assert!(events.iter().all(|event| event.source == LogSource::Build));
        assert_eq!(diagnostics.lock().unwrap().len(), 2, "原记录与镜像只定位一次");
        assert_eq!(probe.dropped(), 0, "噪声之后仍能完整展示真实警告、错误与进度");
    }

    #[test]
    fn cr_erase_process_mixes_keep_true_errors_and_only_mirror_diagnostics_are_visible() {
        let (sink, probe) = crate::project_log::fixture_log_probe("dx-terminal-mix");
        let root = std::env::current_dir().unwrap();
        let mut output = OutputLog::new(&root, Some(sink), None);
        output.emit_line(b"\x1b[2K       Dirty game v0.1.0: source changed\r\x1b[Kerror[E0308]: retained error\n");
        output.emit_line(b"warning: retained before overwrite\r\x1b[K       Fresh game v0.1.0\n");
        let mirror = json!({"RustcOutput":{"message":{"message":"mirror only warning","level":"warning","spans":[],
            "rendered":"warning: mirror only warning\n"}}});
        output.emit_line(format!("{mirror}\n").as_bytes());
        output.emit_line(b"\x1b[K\x1b[32m   Compiling\x1b[0m game v0.1.0\n");
        output.finish();
        let events = probe.drain();
        let lines: Vec<_> = events.iter().filter(|event| event.kind == "line").map(|event| event.text.as_str()).collect();
        assert_eq!(lines, ["error[E0308]: retained error", "warning: retained before overwrite",
            "warning: mirror only warning", "   Compiling game v0.1.0"]);
        assert_eq!(probe.dropped(), 0);
    }

    /// 私有raw仅由本轮显式环境路径提供，普通测试不依赖用户工程、历史目录或外部构建。
    #[test]
    #[ignore = "设置FEATHRISE_DX_STDOUT_REPLAY为本轮实际DX stdout raw文件后显式运行"]
    fn private_dx_stdout_replay_uses_the_real_sink_and_reports_displayed_records() {
        use sha2::Digest;
        let path = std::path::PathBuf::from(std::env::var_os("FEATHRISE_DX_STDOUT_REPLAY")
            .expect("私有raw回放需要显式文件路径"));
        assert!(path.is_absolute() && path.is_file());
        let raw = std::fs::read(&path).unwrap();
        assert!(!raw.is_empty() && raw.len() <= super::super::MAX_COMPILER_OUTPUT_BYTES);
        let root = std::env::var_os("FEATHRISE_DX_REPLAY_PROJECT_ROOT")
            .map(std::path::PathBuf::from).unwrap_or_else(|| std::env::current_dir().unwrap());
        assert!(root.is_absolute() && root.is_dir());
        let (sink, probe) = crate::project_log::fixture_log_probe("private-dx-raw-replay");
        let diagnostics = Arc::new(Mutex::new(Vec::new()));
        let received = diagnostics.clone();
        let callback: Arc<dyn Fn(CompilerDiagnostic) + Send + Sync> = Arc::new(move |diagnostic| {
            received.lock().unwrap().push(diagnostic);
        });
        let mut output = OutputLog::new(&root, Some(sink), Some(callback));
        let mut events = Vec::new();
        let mut records = 0;
        for line in raw.split_inclusive(|byte| *byte == b'\n') {
            output.emit_line(line);
            records += 1;
            // 明确回放消费节奏，不把整文件瞬间突发等同真实Cargo数秒内的UI消费时序。
            if records % 16 == 0 { events.extend(probe.drain()); }
        }
        output.finish();
        events.extend(probe.drain());
        let lines: Vec<_> = events.iter().filter(|event| event.kind == "line").collect();
        assert!(!lines.is_empty(), "回放必须确有原始人类输出");
        assert!(lines.iter().all(|event| event.source == LogSource::Build));
        assert!(lines.iter().all(|event| !cargo_verbose_process_line(event.text.as_bytes())));
        assert!(lines.iter().all(|event| !event.text.contains("\"reason\":\"compiler-artifact\"")
            && !event.text.contains("\"reason\":\"build-script-executed\"")
            && !event.text.contains("\"$message_type\":\"artifact\"")));
        assert_eq!(probe.dropped(), 0);
        let hash: String = sha2::Sha256::digest(&raw).iter().map(|byte| format!("{byte:02x}")).collect();
        let report = json!({"raw":path,"root":root,"raw_bytes":raw.len(),"raw_sha256":hash,"wire_records":records,
            "displayed_lines":lines.len(),"diagnostics":diagnostics.lock().unwrap().len(),
            "dropped":probe.dropped(),"drain_every_wire_records":16,
            "committed_progress_lines":lines.iter().filter(|event|dx_compiled_progress(&event.text)).count(),
            "temporary_progress_updates":events.iter().filter(|event|event.kind=="tail" && dx_compiled_progress(&event.text)).count(),
            "visible_text":lines.iter().map(|event| event.text.as_str()).collect::<Vec<_>>()});
        // 显式私有回放才落盘实际展示正文；独占新文件，不覆盖原始输出和任何旧验收回执。
        if let Some(report_path) = std::env::var_os("FEATHRISE_DX_REPLAY_REPORT") {
            use std::io::Write;
            let report_path = std::path::PathBuf::from(report_path);
            assert!(report_path.is_absolute());
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(report_path).unwrap();
            file.write_all(&serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
        println!("{}", json!({"raw_bytes":raw.len(),"wire_records":records,
            "displayed_lines":lines.len(),"diagnostics":diagnostics.lock().unwrap().len(),"dropped":probe.dropped()}));
    }
}
