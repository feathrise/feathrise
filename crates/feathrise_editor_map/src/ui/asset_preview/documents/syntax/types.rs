//! 分词的UTF-8字节范围和可见字形映射；身份独立于选区与光标闪烁。

use super::super::types::{Document, SourceKey};
use bevy::{prelude::*, text::EditableTextGeneration};
use std::{ops::Range, path::Path};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Language {
    Json,
    Rust,
    Wgsl,
}

impl Language {
    pub(super) fn from_path(path: &str) -> Option<Self> {
        let ext = Path::new(path).extension()?.to_str()?;
        if ext.eq_ignore_ascii_case("json") || ext.eq_ignore_ascii_case("jsonl") {
            Some(Self::Json)
        } else if ext.eq_ignore_ascii_case("rs") {
            Some(Self::Rust)
        } else if ext.eq_ignore_ascii_case("wgsl") {
            Some(Self::Wgsl)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Key,
    Keyword,
    String,
    Number,
    Comment,
    Type,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Token {
    pub start: u32,
    pub end: u32,
    pub kind: Kind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SyntaxKey {
    pub document: SourceKey,
    pub revision: u64,
    pub language: Option<Language>,
}

impl SyntaxKey {
    pub(super) fn new(doc: &Document) -> Self {
        Self {
            document: doc.source_key(),
            revision: doc.revision,
            language: Language::from_path(&doc.path),
        }
    }

    pub(super) fn current(&self, doc: &Document) -> bool {
        self.document.path == doc.path
            && self.document.generation == doc.generation
            && self.revision == doc.revision
            && self.language == Language::from_path(&doc.path)
    }
}

pub(super) struct SyntaxResult {
    pub key: SyntaxKey,
    pub tokens: Vec<Token>,
}

#[derive(Clone, PartialEq)]
pub(super) struct GlyphKey {
    pub source: Entity,
    pub syntax: SyntaxKey,
    pub generation: Option<EditableTextGeneration>,
    pub preview_tick: u32,
    pub lines: Range<usize>,
    pub extent: Vec2,
    pub width: f32,
    pub scale: f32,
    pub glyphs: usize,
}

/// 每项固定16字节；字节范围用于让原生选区颜色优先，索引用于直接修改已提取字形。
pub(super) struct GlyphColor {
    pub glyph: u32,
    pub start: u32,
    pub end: u32,
    pub kind: Kind,
}

pub(super) struct VisibleColors {
    pub key: GlyphKey,
    pub colors: Vec<GlyphColor>,
}
