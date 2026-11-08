//! 跨作者功能共享的工作区、滚动和文本组件；重导出保留唯一 ECS 身份。

use bevy::prelude::*;

/// 宿主在原F12按钮旁安装运行配置，避免重建第二个启动入口。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ProjectLaunchButton;

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ProjectLaunchLabel;

/// 中央动态标签按钮和正文的共同根；修订变化时整体替换。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PresetWorkspaceDynamicRoot;

/// 中央植被预设详细页的独立滚动视口；地图标签活动时该页退出布局。
#[derive(Component, Clone, Copy, Default)]
pub struct MapEditorPresetWorkspaceScroll;

/// 中央植被预设标签条的横向滚动视口；预设标签不分页且不会因数量增加而失去入口。
#[derive(Component, Clone, Copy, Default)]
pub struct MapEditorPresetTabScroll;

/// 标记真实可见的官方文本输入框；关闭F12只释放这些字段的焦点。
#[derive(Component, Clone, Default)] // BSN静态控件需要可克隆默认标记。
#[require(bevy::ui_widgets::TextInput)]
pub struct EditorTextInput; // 主菜单姓名框不使用此编辑器专用身份。

/// 备注拥有独立有限滚动视区；鼠标在其中时先滚备注，再滚中央大表单。
#[derive(Component, Clone, Copy, Debug, Default)] // 标记只服务编辑器布局，不进入目录元数据。
pub struct PresetNotesScroll; // 外层滚轮入口用此明确区分备注与工作区视口。
