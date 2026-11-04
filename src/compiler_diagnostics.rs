//! Cargo 结构化诊断的有界纯数据桥；路径核验只在后台构建线程执行，UI 不读取源码或猜测终端文本。

use serde::Serialize;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub(crate) const DIAGNOSTIC_LIMIT: usize = 256;
const MESSAGE_BYTES: usize = 8 * 1024;
const CODE_BYTES: usize = 128;
const PATH_BYTES: usize = 4096;
const SPAN_LIMIT: usize = 64;
const EXPANSION_LIMIT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(crate) enum DiagnosticLevel {
    Error,
    Warning,
    Note,
    Help,
    FailureNote,
    Other,
}

impl DiagnosticLevel {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Error => "错误",
            Self::Warning => "警告",
            Self::Note => "说明",
            Self::Help => "建议",
            Self::FailureNote => "失败说明",
            Self::Other => "诊断",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct SourceLocation {
    /// 只保存当前工程根内的真实相对路径，不把依赖库、系统源码或生成缓存送入作者文档。
    pub(crate) path: PathBuf,
    pub(crate) line: u32,
    /// rustc 的列是从 1 开始的 Unicode 字符列，不能当 UTF-8 字节索引使用。
    pub(crate) column: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CompilerDiagnostic {
    pub(crate) level: DiagnosticLevel,
    pub(crate) message: String,
    pub(crate) code: Option<String>,
    pub(crate) location: Option<SourceLocation>,
}

/// 接收 Cargo 的完整 JSON 行；只解释 compiler-message，原 rendered 正文仍交给既有终端解码器。
pub(crate) fn from_cargo(value: &Value, project_root: &Path) -> Option<CompilerDiagnostic> {
    if value["reason"] != "compiler-message" { return None; }
    let message = &value["message"];
    let text = message["message"].as_str()?;
    let level = match message["level"].as_str().unwrap_or_default() {
        "error" | "error: internal compiler error" => DiagnosticLevel::Error,
        "warning" => DiagnosticLevel::Warning,
        "note" => DiagnosticLevel::Note,
        "help" => DiagnosticLevel::Help,
        "failure-note" => DiagnosticLevel::FailureNote,
        _ => DiagnosticLevel::Other,
    };
    let root = project_root.canonicalize().ok();
    let manifest_parent = value["manifest_path"].as_str().and_then(|path| Path::new(path).parent());
    let location = root.as_deref().and_then(|root| {
        let spans = message["spans"].as_array()?;
        // 多主 span 取第一个可在本工程打开的位置；外部宏优先沿有限 expansion 链找作者调用点。
        spans.iter().take(SPAN_LIMIT).filter(|span| span["is_primary"] == true)
            .chain(spans.iter().take(SPAN_LIMIT).filter(|span| span["is_primary"] != true))
            .find_map(|span| span_location(span, root, manifest_parent))
    });
    Some(CompilerDiagnostic {
        level,
        message: bounded(text, MESSAGE_BYTES),
        code: message["code"]["code"].as_str().map(|code| bounded(code, CODE_BYTES)),
        location,
    })
}

fn span_location(span: &Value, root: &Path, manifest_parent: Option<&Path>) -> Option<SourceLocation> {
    let mut span = span;
    for _ in 0..=EXPANSION_LIMIT {
        if let Some(location) = direct_location(span, root, manifest_parent) { return Some(location); }
        span = span.get("expansion")?.get("span")?;
    }
    None
}

fn direct_location(span: &Value, root: &Path, manifest_parent: Option<&Path>) -> Option<SourceLocation> {
    let raw = span["file_name"].as_str()?;
    if raw.is_empty() || raw.len() > PATH_BYTES || raw.chars().any(char::is_control) { return None; }
    let line = u32::try_from(span["line_start"].as_u64()?).ok().filter(|line| *line > 0)?;
    let column = u32::try_from(span["column_start"].as_u64()?).ok().filter(|column| *column > 0)?;
    let raw = Path::new(raw);
    // Cargo 可同时返回工作区相对地址和包相对地址；所有候选最终都必须经过真实根包含检查。
    let candidates = if raw.is_absolute() {
        [Some(raw.to_path_buf()), None]
    } else {
        [Some(root.join(raw)), manifest_parent.map(|parent| parent.join(raw))]
    };
    let path = candidates.into_iter().flatten().find_map(|candidate| {
        let path = candidate.canonicalize().ok()?;
        let relative = path.strip_prefix(root).ok()?;
        if !path.is_file() || !valid_source_path(relative) { return None; }
        Some(relative.to_path_buf())
    })?;
    Some(SourceLocation { path, line, column })
}

fn valid_source_path(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
        && path.to_str().is_some_and(|path| path.len() <= PATH_BYTES)
        && path.components().all(|component| match component {
            Component::Normal(name) => name.to_str().is_some_and(|name| {
                !["target", ".git", ".trash"].iter().any(|reserved| name.eq_ignore_ascii_case(reserved))
            }),
            _ => false,
        })
}

fn bounded(text: &str, maximum: usize) -> String {
    let mut end = text.len().min(maximum);
    while !text.is_char_boundary(end) { end -= 1; }
    text[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("feathrise-compiler-diagnostics-{}", bevy::asset::uuid::Uuid::new_v4()));
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(root.join("src/main.rs"), "fn main() { let 中文 = missing; }\n").unwrap();
            Self(root.canonicalize().unwrap())
        }
        fn envelope(&self, spans: Value) -> Value {
            json!({"reason":"compiler-message", "manifest_path":self.0.join("Cargo.toml"),
                "message":{"level":"error","message":"cannot find value `missing`", "code":{"code":"E0425"}, "spans":spans}})
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); }
    }
    fn span(path: impl Into<Value>) -> Value {
        let path: Value = path.into();
        json!({"file_name":path,"line_start":1,"column_start":23,"is_primary":true})
    }

    #[test]
    fn cargo_primary_span_retains_unicode_column_and_relative_project_identity() {
        let fixture = Fixture::new();
        let result = from_cargo(&fixture.envelope(json!([span("src/main.rs")])), &fixture.0).unwrap();
        assert_eq!(result.level, DiagnosticLevel::Error);
        assert_eq!(result.code.as_deref(), Some("E0425"));
        assert_eq!(result.location, Some(SourceLocation { path: "src/main.rs".into(), line: 1, column: 23 }));
        assert!(from_cargo(&json!({"reason":"compiler-artifact"}), &fixture.0).is_none());
    }

    #[test]
    fn diagnostics_without_author_source_keep_message_without_openable_location() {
        let fixture = Fixture::new();
        fs::create_dir_all(fixture.0.join("target")).unwrap();
        fs::write(fixture.0.join("target/generated.rs"), "generated").unwrap();
        for path in ["../outside.rs", "target/generated.rs", "<proc-macro source code>", "missing.rs"] {
            let diagnostic = from_cargo(&fixture.envelope(json!([span(path)])), &fixture.0).unwrap();
            assert!(diagnostic.location.is_none(), "{path}");
            assert!(!diagnostic.message.is_empty());
        }
        let result = from_cargo(&fixture.envelope(json!([])), &fixture.0).unwrap();
        assert!(result.location.is_none());
    }

    #[test]
    fn external_macro_expansion_finds_checked_author_callsite() {
        let fixture = Fixture::new();
        let mut generated = span("<macro expansion>");
        generated["expansion"] = json!({"span":span(fixture.0.join("src/main.rs").to_string_lossy().as_ref())});
        let result = from_cargo(&fixture.envelope(json!([generated])), &fixture.0).unwrap();
        assert_eq!(result.location.unwrap().path, PathBuf::from("src/main.rs"));
    }

    #[test]
    fn unknown_levels_and_oversized_text_are_forward_compatible_and_bounded() {
        let fixture = Fixture::new();
        let mut envelope = fixture.envelope(json!([]));
        envelope["message"]["level"] = "future-note".into();
        envelope["message"]["message"] = "中文".repeat(MESSAGE_BYTES).into();
        envelope["message"]["code"]["code"] = "x".repeat(CODE_BYTES + 1).into();
        let result = from_cargo(&envelope, &fixture.0).unwrap();
        assert_eq!(result.level, DiagnosticLevel::Other);
        assert!(result.message.len() <= MESSAGE_BYTES);
        assert_eq!(result.code.unwrap().len(), CODE_BYTES);
    }
}
