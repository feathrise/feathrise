//! 文档后台分词的 UTF-8 字节契约。

use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Json,
    Rust,
    Wgsl,
}

impl Language {
    pub fn from_path(path: &str) -> Option<Self> {
        let ext = Path::new(path).extension()?.to_str()?;
        if ext.eq_ignore_ascii_case("json") || ext.eq_ignore_ascii_case("jsonl") {
            Some(Self::Json)
        } else if ext.eq_ignore_ascii_case("rs") {
            Some(Self::Rust)
        } else if ext.eq_ignore_ascii_case("wgsl") || ext.eq_ignore_ascii_case("wesl") {
            Some(Self::Wgsl)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Key,
    Keyword,
    String,
    Number,
    Comment,
    Type,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub start: u32,
    pub end: u32,
    pub kind: Kind,
}
