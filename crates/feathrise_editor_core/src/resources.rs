//! 中央工作区修订、候选输入保护与唯一预览意图；均不写入场景或预设。

use crate::PresetWorkspaceTab;

/// 顶栏共享的运行选择；不写入工程/场景，实际启动必须在消息中冻结profile。
#[derive(bevy::prelude::Resource, Clone, Copy, Debug, Default)]
pub struct ProjectLaunchSettings {
    pub profile: crate::ProjectBuildProfile,
    pub preview: bool,
    /// 常驻Job正在更换构建配置；完成真实收口前不能再派发另一份试玩。
    pub switching_profile: bool,
}

impl ProjectLaunchSettings {
    pub fn action_label(&self) -> &'static str {
        if self.preview { "预览" } else { "启动" }
    }
}

/// 宿主只读投影的原启动按钮阶段；作者提示不需要依赖构建线程、进程或具体宿主。
#[derive(bevy::prelude::Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProjectLaunchInterruption {
    #[default]
    None,
    CanStopCompilation,
    StoppingCompilation,
}

impl ProjectLaunchInterruption {
    pub fn action_label(self) -> Option<&'static str> {
        match self { Self::None => None, Self::CanStopCompilation => Some("停止编译"), Self::StoppingCompilation => Some("正在停止") }
    }
}

/// 已安装作者参与者发布的轻量事实；普通文档恢复标签和输入仲裁不再读取具体游戏草稿。
#[derive(bevy::prelude::Resource, Default)]
pub struct WorkspaceDomainState {
    pub open_tabs: Vec<PresetWorkspaceTab>,
    pub input_blocked: bool,
    /// 普通工作区已实际重建的修订；应用追加自己的原表单时复用同一延迟命令屏障。
    pub presented_revision: u64,
}

impl WorkspaceDomainState {
    pub fn is_open(&self, tab: PresetWorkspaceTab) -> bool { self.open_tabs.contains(&tab) }
}
use bevy::prelude::*;

/// 应用作者适配显式追加的视图层；默认只使用普通编辑器自己的场景与环境通道。
/// 相机可见集合与全图取景集合分别声明，避免把特效、离屏样品或隐藏玩法内容算进场景范围。
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct EditorApplicationViewLayers {
    pub camera: bevy::camera::visibility::RenderLayers,
    pub framing: bevy::camera::visibility::RenderLayers,
}

impl Default for EditorApplicationViewLayers {
    fn default() -> Self {
        Self {
            camera: bevy::camera::visibility::RenderLayers::none(),
            framing: bevy::camera::visibility::RenderLayers::none(),
        }
    }
}

/// 单一中央标签与重建票据；具体预设会话只持有这份共享契约，不复制状态。
#[derive(Resource, Debug)]
pub struct PresetWorkspaceState {
    pub active_tab: PresetWorkspaceTab,
    pub ui_revision: u64,
}

impl Default for PresetWorkspaceState {
    fn default() -> Self {
        Self {
            active_tab: PresetWorkspaceTab::Scene,
            ui_revision: 1,
        }
    }
}

impl PresetWorkspaceState {
    /// 零值只代表尚未建立 UI；回绕也必须保持有效重建身份。
    pub fn touch_ui(&mut self) {
        self.ui_revision = self.ui_revision.wrapping_add(1).max(1);
    }
}

/// 当前帧官方候选过程是否要求屏蔽业务确认与取消。
#[derive(Resource, Default)] // 不维护第二份预编辑文字。
pub struct EditorTextInputGuard {
    // 所有文本域共享候选快捷键边界。
    pub block_shortcuts: bool, // 组合持续期间与组合消息末帧均为true。
} // 文本编辑本身仍完全由官方系统执行。

/// 独立宿主在原工作台上打开菜单时复用原鼠标静默释放门，不让浮层点击穿透作者视口。
#[derive(Resource, Default)]
pub struct EditorHostOverlay {
    pub active: bool,
}

/// 唯一预览面板的浏览意图；打开装备或角色草稿不应禁止作者查看候选资产。
#[derive(Resource, Debug, Default)] // 只记录轻量标签身份，不复制草稿、模型或目录。
pub struct AssetPreviewFocus {
    browser_for: Option<PresetWorkspaceTab>, // Some表示当前标签下由作者明确选择了文件浏览。
}

impl AssetPreviewFocus {
    pub fn browse(&mut self, tab: PresetWorkspaceTab) {
        self.browser_for = Some(tab); // 重复点击同一文件也属于明确浏览，不依赖路径Changed。
    }

    pub fn show_workspace(&mut self) {
        self.browser_for = None; // 显式预览恢复当前草稿，不改资产选择或未保存字段。
    }

    pub fn browsing(&self, tab: PresetWorkspaceTab) -> bool {
        self.browser_for == Some(tab) // 只在发起浏览的同一标签内覆盖默认预设视图。
    }

    pub fn sync_workspace(&mut self, open: bool, tab: PresetWorkspaceTab) {
        if self.browser_for.is_some() && (!open || !self.browsing(tab)) {
            self.show_workspace(); // 关闭F12或离开标签后，旧浏览意图不能在下次返回时复活。
        }
    }
}
