//! 文本文档的不可变后台结果、格式、身份与完整内容；存储分块不形成界面分页。

use crate::asset_catalog::DocumentFormat;
use std::{path::PathBuf, sync::Arc};

/// 每个路径拥有独立标签；数字只用于本进程激活/关闭，不作为磁盘身份。
pub(super) struct ExpandedDocument {
    pub id: u64,
    pub path: String,
    pub return_tab: crate::ui::vegetation_preset_editor::PresetWorkspaceTab,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DiskIdentity {
    pub path: PathBuf, // 保存前再次规范化并验证目标；禁止符号链接换成另一文件。
    pub digest: [u8; 32],
    pub bom: bool,
    pub crlf: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ViewMode {
    #[default]
    Preview,
    Edit,
}

pub(super) use feathrise_editor_documents_model::types::{
    BlockKind, Block, PreviewPage,
};
#[cfg(test)]
pub(super) use feathrise_editor_documents_model::types::{InlineStyle, ParsedPreview};

#[derive(Clone, Debug)]
pub(super) struct Document {
    pub path: String,
    pub format: DocumentFormat, // 格式与路径一起固定；切到源码时不能沿用Markdown解析规则。
    pub generation: u64,
    pub disk: DiskIdentity,
    pub pages: Vec<Arc<str>>, // 后台源分块；最小替换保留未修改字节，保存快照只共享Arc。
    pub source: Arc<str>, // 后台加载时准备的完整LF输入值，滚动和光标移动不重新拼接全文。
    pub revision: u64, // 每次确认编辑/撤销/重做都增长，用于后台派生身份，不回退到历史修订。
    pub saved_revision: u64, // 已保存的内容身份；撤销回同一快照时可清除星号。
    pub history: super::history::DocumentHistory,
    pub preview: Arc<Vec<PreviewPage>>,
    pub preview_revision: Option<u64>,
    pub preview_clipped: bool,
    pub mode: ViewMode,
    pub preview_scroll: bevy::prelude::Vec2,
    pub confirm_reload: bool,
    pub external: super::external::ExternalDocument,
    pub status: String,
}

pub(super) struct NativeDocumentInput {
    pub key: SourceKey,
    pub revision: u64, // 完整身份核验草稿，切标签不扫描全文比较内容。
    pub input: bevy::text::EditableText,
    pub scroll: bevy::prelude::Vec2,
}

pub(super) use feathrise_editor_documents_model::types::SourceKey;

/// 轮廓固定真实输入实体、文件代次、内容修订和物理排版范围；光标闪烁不换代。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct MinimapKey {
    pub source: bevy::prelude::Entity,
    pub document: SourceKey,
    pub revision: u64,
    pub extent: bevy::prelude::Vec2,
    pub view_width: f32,
    pub glyphs: usize,
}

/// 栅格化前已量化到固定纹理的字形轮廓；每项8字节，不拷贝字串或字体图集。
#[derive(Clone, Copy, Debug)]
pub(super) struct MinimapMark {
    pub min: [u16; 2],
    pub max: [u16; 2],
}

pub(super) struct MinimapCollection {
    pub key: MinimapKey,
    pub cursor: usize,
    pub marks: Vec<MinimapMark>,
}

pub(super) struct MinimapResult {
    pub key: MinimapKey,
    pub pixels: Vec<u8>,
}

pub(super) struct MinimapPicture {
    pub key: MinimapKey,
    pub image: bevy::prelude::Handle<bevy::prelude::Image>,
}

#[derive(Clone, Debug)]
pub(super) struct MinimapDrag {
    pub document: SourceKey,
    pub source: bevy::prelude::Entity,
    pub revision: u64,
    pub grab_ratio: f32,
}

pub(super) struct SourceContents {
    pub pages: Vec<Arc<str>>,
    pub text: Arc<str>,
}

#[derive(Clone, Debug)]
pub(super) struct LoadRequest {
    pub path: String,
    pub ticket: u64,
    pub expected_document: Option<(SourceKey, u64)>, // 丢弃确认只授权替换这一代、这一修订；首次加载要求仍无会话。
}

pub(super) enum IoResult {
    Checked(super::external::CheckReply),
    Loaded(LoadRequest, Result<(DiskIdentity, SourceContents), String>),
    SavedBatch(super::batch_save::SaveReply),
    Saved {
        path: String,
        generation: u64,
        revision: u64,
        result: Result<DiskIdentity, String>,
    },
}

pub(super) struct PreviewResult {
    pub path: String,
    pub generation: u64,
    pub revision: u64,
    pub pages: Arc<Vec<PreviewPage>>,
    pub clipped: bool,
}

/// 排版分块仅用于后台快照和固定预算构造；正文没有页码或换页身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PreviewIdentity {
    pub path: String,
    pub generation: u64,
    pub revision: u64,
}

pub(super) struct RenderedPreview {
    pub identity: PreviewIdentity,
    pub root: bevy::prelude::Entity,
}

pub(super) struct PreviewBuild {
    pub identity: PreviewIdentity,
    pub root: bevy::prelude::Entity,
    pub literal: Option<bevy::prelude::Entity>,
    pub pages: Arc<Vec<PreviewPage>>,
    pub page: usize,
    pub block: usize,
}
