//! 检索结果和可见行绘制的版本身份；字节位置始终对应已确认的UTF-8草稿。

use super::super::{history::Selection, types::SourceKey};
use bevy::prelude::*;
use std::{ops::Range, sync::Arc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum NavigationMode {
    #[default]
    Closed,
    Find,
    Replace,
    Go,
}

impl NavigationMode {
    pub(super) fn searching(self) -> bool {
        matches!(self, Self::Find | Self::Replace)
    }
}

pub(in super::super) use feathrise_editor_documents_model::navigation::types::{
    IndexKey, ReplacedContent, SearchOptions, TextIndex,
};

pub(super) struct SelectionScope {
    pub document: SourceKey,
    pub revision: u64,
    pub range: Range<usize>, // 开启时冻结，查找改变原生选区不会重定检索范围。
}

/// 检索与替换共用一个不可抢占的任务槽；索引用Arc转交，不复制全部匹配。
pub(super) enum NavigationWork {
    Index(TextIndex),
    Replace(ReplacementResult),
}

#[derive(Clone)]
pub(super) struct ReplacementRequest {
    pub index: Arc<TextIndex>,
    pub ticket: u64,
    pub source: Entity,
    pub focus: Option<Entity>,
    pub selection: Selection,
    pub replacement: String,
    pub target: Option<Range<usize>>, // None表示全部；Some只包含这次明确选择的一处匹配。
}

pub(super) struct ReplacementResult {
    pub request: ReplacementRequest,
    pub content: Result<Option<ReplacedContent>, String>,
}

#[derive(Clone, Copy)]
pub(super) enum NavigationRequest {
    Next(bool),
    Line(usize),
}

pub(super) struct RevealTarget {
    pub source: Entity,
    pub key: IndexKey,
    pub selection: Range<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PaintKey {
    pub source: Entity,
    pub index: IndexKey,
    pub extent: Vec2,
    pub width: f32,
    pub scale: f32,
    pub lines: usize,
}

/// 一行只保存固定1024个横向像素；分帧续算用原生run/cluster索引，不反复扫描行首。
pub(super) struct RowPaint {
    pub line: usize,
    pub run: usize,
    pub cluster: usize,
    pub x: f32,
    pub ready: bool,
    pub pixels: Vec<u8>,
}

pub(super) struct RowSlot {
    pub number: Entity,
    pub highlight: Entity,
    pub paint: Option<RowPaint>,
}

// 系统集只共享调度顺序，不把导航资源或业务系统公开给小场景。
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub(in super::super) enum NavigationSystemSet {
    Input,
    Navigate,
    Reveal,
}
