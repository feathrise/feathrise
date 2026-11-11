//! 资产预览的临时导航状态；不写入地图、预设、设置或撤销记录。

use bevy::prelude::*; // 导航属于当前编辑器会话的单个资源。
use feathrise_editor_core::EditorSnapSettings;

use super::types::AssetPreviewDrag; // 冻结按下时的手势，不在拖动途中切换用途。
use super::view_math::AssetPreviewView; // 图片平移与模型轨道共享独立缩放倍率。

/// 图片/模型/音频按路径保留多个标签；只有活动文件使用唯一画布，后台只留下身份、视角或试听位置。
#[derive(Resource, Debug, Default)]
pub struct AssetPreviewWorkspace {
    pub(super) expanded: Option<super::types::ExpandedAssetPreview>,
    pub(super) parked: Vec<super::types::ExpandedAssetPreview>,
    pub(super) next_tab: u64,
    pub(super) central_active: bool,
    pub(super) restore_view: bool,
}

pub(super) const MAX_MEDIA_TABS: usize = 32; // 仅32份轻量身份/视角/位置，绝不保留32套模型、RTT或音频输出。

impl AssetPreviewWorkspace {
    pub(super) fn tabs(&self) -> impl Iterator<Item = &super::types::ExpandedAssetPreview> {
        self.expanded.iter().chain(self.parked.iter())
    }

    pub(crate) fn has_tabs(&self) -> bool {
        self.expanded.is_some() || !self.parked.is_empty()
    }

    /// 释放最多32份轻量文件/视角身份；唯一画布沿原加载链归还侧栏，不增加模型副本。
    pub(super) fn close_all_tabs(&mut self) -> Option<(String, super::super::vegetation_preset_editor::PresetWorkspaceTab)> {
        self.parked.clear();
        self.central_active = false;
        self.restore_view = false;
        self.expanded.take().map(|tab| (tab.path, tab.return_tab))
    }

    /// 动画媒体已经具有草稿；关闭未修改文件时保留这些标签，并延续唯一画布。
    pub(super) fn close_unmodified_tabs(&mut self, mut dirty: impl FnMut(&str) -> bool)
        -> Option<(String, super::super::vegetation_preset_editor::PresetWorkspaceTab)> {
        self.parked.retain(|tab| dirty(&tab.path));
        if self.expanded.as_ref().is_none_or(|tab| dirty(&tab.path)) { return None; }
        let removed = self.expanded.take().map(|tab| (tab.path, tab.return_tab));
        self.expanded = self.parked.pop();
        self.restore_view = self.expanded.is_some();
        if self.expanded.is_none() { self.central_active = false; }
        removed
    }

    pub fn active(&self, tab: super::super::vegetation_preset_editor::PresetWorkspaceTab) -> bool {
        self.expanded.is_some() && tab == super::super::vegetation_preset_editor::PresetWorkspaceTab::AssetPreview
    }
}

/// 与加载状态分离，鼠标移动不能触发资产重新加载或模型包围盒扫描。
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)] // 稳定输入帧不触发资源Changed。
pub(crate) struct AssetPreviewNavigation {
    pub(super) view: AssetPreviewView, // 保留缩放、模型观察方向和两种平移量。
    pub(super) drag: Option<AssetPreviewDrag>, // 捕获后允许移出画布继续拖动，直到原按键松开。
    pub(super) mouse_captured: bool,   // 极短按下/松开同帧也必须发布一次鼠标所有权。
    pub(super) wheel_captured: bool,   // 仅真实可交互画布悬停时阻止外层资产页滚动。
}

impl AssetPreviewNavigation {
    pub(crate) fn snapping(&self) -> EditorSnapSettings {
        self.view.snapping
    }

    pub(crate) fn grid_visible(&self) -> bool {
        self.view.snapping.show_grid
    }

    /// 同一个 GLB 选择其他子网格时恢复取景，文件标签的辅助线与吸附偏好仍有效。
    pub(super) fn reset_for_model_target(&mut self) {
        let snapping = self.view.snapping;
        *self = Self::default();
        self.view.snapping = snapping;
    }

    /// 共用编辑器鼠标门，使拖动越过画布也不会触发地图相机或笔刷。
    pub(crate) fn dragging(&self) -> bool {
        self.drag.is_some() || self.mouse_captured // release帧之后的和弦隔离交由既有统一门延续。
    }

    /// 外层滚动系统仍消费事件，但不能再应用已交给预览的滚轮。
    pub(crate) fn owns_wheel(&self) -> bool {
        self.wheel_captured || self.dragging() // 捕获拖动中也不能滚走当前预览画布。
    }
}

pub(crate) use feathrise_editor_core::AssetPreviewFocus; // 浏览意图和技能库使用同一资源，不复制焦点。
