//! 资产目录的单一后台槽、原子采用快照和一次性反向定位意图。

use bevy::prelude::Resource; // 目录数据仅属于当前 F12 会话。
use bevy::tasks::Task; // IO 任务直到真正完成才释放槽位。

use super::types::{AssetRevealRequest, AssetScanResult}; // 完整请求和完成值均为可跨线程纯数据。
use super::{filter::{AnimationClipInfo, AssetTypeFilter, DirectoryAnimations, DirectorySnapshot}, watch::DirectoryWorker};
use crate::types::EditorAssetEntry; // UI 固定行池读取当前完整目录。

/// 当前一层资产目录和最多一个在途扫描或退役任务。
#[derive(Resource, Default)]
pub struct EditorAssetCatalog {
    /// 独立宿主提供显式工程代次和根；一体入口不设置，保持原来源规则。
    pub(super) hosted_source: Option<(u64, Option<std::path::PathBuf>)>,
    pub(super) last_click: Option<super::types::TimedAssetFileClick>,
    /// 相对 assets 根的当前已显示目录；定位完成前不会提前改写。
    pub(crate) current_folder: String,
    /// 最后成功采用的目录身份；普通点击/上级会提前改导航目录，不能据此判断是否换目录。
    pub(super) adopted_folder: Option<String>,
    /// 已应用的完整目录条目，不设置目录容量上限。
    pub(crate) entries: Vec<EditorAssetEntry>,
    pub(crate) filename_filter: String,
    pub(super) type_filter: AssetTypeFilter,
    pub(crate) adopted_type_filter: AssetTypeFilter,
    pub(super) animations: DirectoryAnimations,
    pub(super) worker: DirectoryWorker,
    /// 当前页，从零开始，定位采用时直接设置为目标所在页。
    pub(crate) page: usize,
    /// 连续滚动窗口只发布可见条目身份；完整目录仍在entries中，不能以窗口长度截断数据。
    pub(crate) visible_start: usize,
    pub(crate) visible_count: usize,
    pub(crate) viewport_revision: u64,
    /// 只由真实用户滚动推进；布局、反馈换行和池扩容不得取消合法定位。
    pub(crate) scroll_intent_revision: u64,
    pub(crate) scroll_anchor: Option<super::types::AssetScrollAnchor>,
    /// 最新请求修订；现有上级与刷新按钮直接推进该值。
    pub(crate) requested_revision: u64,
    /// 已完成或已取消的请求修订，防止失败后逐帧自动重试。
    pub(crate) applied_revision: u64,
    /// 独立目录反馈保留具体失败；其他工具更新底栏不能抹去该完成值。
    pub(crate) feedback: Option<super::types::AssetCatalogFeedback>,
    /// 最近一次成功定位的修订和条目索引；UI只消费一次，把高亮行滚入可见区域。
    pub(crate) revealed_entry: Option<(u64, usize)>,
    /// 文件或目录定位实际采用后的意图修订；普通初始扫描不能冒充显式定位。
    pub(super) revealed_revision: Option<u64>,
    /// 同一时间最多一个 IO 任务；None 完成值表示只释放旧目录。
    pub(super) task: Option<Task<Option<AssetScanResult>>>,
    /// 等待中的定位目标；普通浏览、选择和关闭可以使它失效。
    pub(super) reveal: Option<AssetRevealRequest>,
    /// 上一目录或迟到结果 O(1) 转移到后台释放，避免大目录析构占用 UI 帧。
    pub(super) retired_entries: Option<DirectorySnapshot>,
    /// 换工程时立即隐藏旧目录；与在途旧扫描结果各占一份退役预算，均由下一IO槽释放。
    pub(super) retired_source: Option<DirectorySnapshot>,
}

impl EditorAssetCatalog {
    pub(crate) fn set_type_filter(&mut self, filter: AssetTypeFilter) {
        if self.type_filter != filter {
            self.cancel_pending_reveal();
            self.type_filter = filter;
            self.feedback = None;
            self.requested_revision = self.requested_revision.wrapping_add(1);
        }
    }

    pub(crate) fn set_filename_filter(&mut self, filter: &str) {
        if self.filename_filter != filter {
            self.cancel_pending_reveal();
            self.filename_filter = filter.to_owned();
            self.feedback = None;
            self.requested_revision = self.requested_revision.wrapping_add(1);
        }
    }

    pub(crate) fn is_loading(&self) -> bool {
        self.hosted_source.as_ref().is_none_or(|(_, root)| root.is_some())
            && self.requested_revision != self.applied_revision
    }

    pub(crate) fn animation_clips(&self, relative_path: &str) -> &[AnimationClipInfo] {
        self.animations.get(relative_path).map_or(&[], |clips| clips.as_ref())
    }

    pub(super) fn source_context(&self) -> u64 {
        self.hosted_source.as_ref().map_or(0, |source| source.0)
    }

    /// 资源引用发起的定位是一次显式浏览意图，工程浏览器据此切回资产视图。
    pub(crate) fn pending_reveal_revision(&self) -> Option<u64> {
        self.reveal.as_ref().map(|request| request.revision)
    }

    /// 用户主动切到工程文件后，旧资产定位不得迟到并抢回视图或选择。
    pub(crate) fn cancel_pending_reveal(&mut self) {
        super::reveal::cancel_asset_reveal(self);
    }

    /// 换根只移动所有权和失效票据，不在UI帧扫描或销毁整张目录。
    pub(crate) fn bind_project_source(&mut self, context: u64, root: Option<std::path::PathBuf>) -> bool {
        if self.hosted_source.as_ref().is_some_and(|source| source.0 == context && source.1 == root) {
            return false;
        }
        self.hosted_source = Some((context, root));
        self.requested_revision = self.requested_revision.wrapping_add(1);
        // 即使两个工程包含相同路径，也不能保留行绑定、双击或反向定位意图。
        self.current_folder.clear();
        self.filename_filter.clear();
        self.adopted_folder = None;
        self.adopted_type_filter = AssetTypeFilter::All;
        self.page = 0;
        self.visible_start = 0;
        self.visible_count = 0;
        self.viewport_revision = self.viewport_revision.wrapping_add(1);
        self.scroll_intent_revision = self.scroll_intent_revision.wrapping_add(1);
        self.scroll_anchor = None;
        self.last_click = None;
        self.reveal = None;
        self.revealed_entry = None;
        self.revealed_revision = None;
        self.feedback = None;
        if !self.entries.is_empty() {
            debug_assert!(self.retired_source.is_none());
            self.retired_source = Some(DirectorySnapshot {
                entries: std::mem::take(&mut self.entries), animations: std::mem::take(&mut self.animations),
            });
        }
        true
    }

    pub(crate) fn revealed_revision(&self) -> Option<u64> { self.revealed_revision }

    pub(crate) fn request_scroll_anchor(&mut self, index: usize) {
        let ticket = self.scroll_anchor.map_or(1, |anchor| anchor.ticket.wrapping_add(1));
        self.scroll_anchor = Some(super::types::AssetScrollAnchor { ticket, revision: self.applied_revision, index, require_selection: false });
    }
}
