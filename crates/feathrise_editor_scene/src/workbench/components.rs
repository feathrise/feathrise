//! 原工作台向独立宿主开放的挂载点；不公开业务控件的内部marker。
use bevy::prelude::*;

/// 工程入口挂在原顶栏；独立宿主可整合窗口控制，不能复制另一套工作台。
#[derive(Component, Default, Clone)]
pub struct WorkbenchHeader;

/// 顶栏的呈现槽位；宿主只移动原控件实体，不复制保存、历史、布局或设置的业务入口。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WorkbenchHeaderItem {
    Save,
    Undo,
    Redo,
    Selection,
    Navigation,
    Inspector,
    Focus,
    Settings,
    #[default]
    Spacer,
}

/// 底栏日志入口由独立宿主接管；未安装真实进程日志源时保持隐藏。
#[derive(Component, Default, Clone)]
pub struct WorkbenchProjectLogButton;

/// 宿主非模态面板的实际边界，同时用于鼠标命中与原生工程视口留孔。
#[derive(Component)]
#[require(bevy::ui::RelativeCursorPosition)]
pub struct WorkbenchFloatingPanel;

/// 只记录设置访问门自己施加的禁用及原视觉；其他系统已禁用的控件不归本门恢复。
#[derive(Component)]
pub(super) struct SettingsAccessDisabled {
    pub background: Option<Color>,
    pub border: Option<BorderColor>,
    pub labels: Vec<(Entity, TextColor)>,
}
