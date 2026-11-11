//! 单份JSON扫描的临时游标、字段栈与结果；只在后台计算闭包内存活。

use super::super::types::Hit;
use std::{cell::Cell, sync::atomic::AtomicBool};

pub(super) struct Cursor<'a> {
    pub source: &'a [u8],
    pub position: &'a Cell<usize>,
    pub cancel: &'a AtomicBool,
}

pub(super) struct Scan<'a> {
    pub source: &'a str,
    pub target: Option<&'a str>, // None收集明确路径供缺失检查；Some保持原精确引用查询。
    pub position: &'a Cell<usize>,
    pub pointer: String,
    pub maximum: usize,
    pub hits: Vec<Hit>,
    pub paths: Vec<String>,
    pub limited: bool,
}

pub(in super::super) struct PathReference {
    pub hit: Hit,
    pub path: String,
}

pub(super) struct ValueSeed<'a, 's> {
    pub scan: &'a mut Scan<'s>,
    pub field: bool,
    pub start: usize,
}
