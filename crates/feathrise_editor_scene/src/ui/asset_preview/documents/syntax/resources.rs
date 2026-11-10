//! 单一后台分词槽、可见映射与渲染提取身份；不保留第二份全文或字形布局。

use super::types::*;
use bevy::{prelude::*, tasks::Task};
use std::ops::Range;

#[derive(Resource, Default)]
pub(super) struct SyntaxIndex {
    pub task: Option<Task<SyntaxResult>>,
    pub result: Option<SyntaxResult>,
}

#[derive(Resource, Default)]
pub(super) struct SyntaxPaint {
    pub visible: Option<VisibleColors>,
    pub selection: Range<usize>,
    pub selected_color: bool,
}

#[derive(Resource, Default)]
pub(super) struct SyntaxExtraction {
    pub source: Option<PaintIdentity>,
    pub text_start: usize,
}

#[derive(PartialEq)]
pub(super) struct PaintIdentity {
    pub key: GlyphKey,
    pub selection: Range<usize>,
    pub selected_color: bool,
    pub palette: &'static str,
}
