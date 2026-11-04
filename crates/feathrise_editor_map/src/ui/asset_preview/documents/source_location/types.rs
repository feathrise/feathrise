//! 编译位置、当前草稿身份和原生露出回执分离，迟到结果不能把新文件滚回旧位置。

use super::super::types::SourceKey;
use crate::standalone::OpenProjectSourceLocation;
use bevy::prelude::Entity;
use std::ops::Range;

pub(super) struct PendingLocation {
    pub request: OpenProjectSourceLocation,
    pub path: String,
    pub document: Option<(SourceKey, u64)>,
}

pub(super) struct PositionResult {
    pub ticket: u64,
    pub document: SourceKey,
    pub revision: u64,
    pub selection: Range<usize>,
    pub clamped: bool,
}

pub(super) struct Reveal {
    pub source: Entity,
    pub document: SourceKey,
    pub revision: u64,
    pub selection: Range<usize>,
}
