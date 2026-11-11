//! 后台诊断与格式化的完整身份；偏移始终是当前已确认正文的UTF-8字节位置。

use super::super::{
    history::Selection,
    types::{Document, SourceKey},
};
use bevy::prelude::*;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct JsonKey {
    pub document: SourceKey,
    pub revision: u64,
}

impl JsonKey {
    pub fn new(doc: &Document) -> Self {
        Self {
            document: doc.source_key(),
            revision: doc.revision,
        }
    }
    pub fn current(&self, doc: &Document) -> bool {
        self.document.path == doc.path
            && self.document.generation == doc.generation
            && self.revision == doc.revision
    }
}

pub(super) use feathrise_editor_documents_model::json::types::{Diagnostic, Formatted};

pub(super) struct Validation {
    pub key: JsonKey,
    pub error: Option<Diagnostic>,
}

#[derive(Clone)]
pub(super) struct FormatRequest {
    pub key: JsonKey,
    pub ticket: u64,
    pub source: Entity,
    pub selection: Selection,
}

pub(super) struct WorkResult {
    pub validation: Validation,
    pub request: Option<FormatRequest>,
    pub formatted: Result<Option<Formatted>, String>,
}

pub(super) struct Reveal {
    pub key: JsonKey,
    pub source: Entity,
    pub selection: Range<usize>,
}

pub(super) use feathrise_editor_documents_model::json::supported;
