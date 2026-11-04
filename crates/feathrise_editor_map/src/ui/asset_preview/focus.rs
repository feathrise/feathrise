//! 唯一预览视图的来源切换：资产点击优先浏览，显式预设预览或离开标签后归还工作区。

#[cfg(test)]
use super::resources::AssetPreviewFocus; // 来源与加载、导航状态独立，后台完成不能抢回面板。
#[cfg(test)]
use crate::ui::vegetation_preset_editor::PresetWorkspaceTab; // 稳定标签身份隔离不同作者草稿。

#[cfg(test)] // 无窗口运行真实点击、加载来源及面板挂载系统。
#[path = "focus_tests.rs"]
mod tests;
