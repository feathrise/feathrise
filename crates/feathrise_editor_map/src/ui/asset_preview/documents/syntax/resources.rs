//! 单一后台分词槽、可见映射与渲染阶段的有限定位游标；不保留第二份全文或字形布局。

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
    pub text_start: usize,
    pub cached: Option<usize>,
    pub cursor: usize,
}
