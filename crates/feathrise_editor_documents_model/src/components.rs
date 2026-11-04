//! 文档固定工具栏、完整输入与连续预览的独立UI标记。

use super::types::SourceKey;
use bevy::prelude::*;

#[derive(Component, Default, Clone)]
pub struct DocumentPanel;
#[derive(Component, Default, Clone)]
pub struct DocumentCenterHost;
#[derive(Component, Default, Clone)]
pub struct DocumentTabRoot;
#[derive(Component, Default, Clone)]
pub struct DocumentSidebarLaunch;
#[derive(Component, Default, Clone)]
pub struct DocumentExpandLabel;
#[derive(Component, Default, Clone)]
pub struct DocumentPathText;
#[derive(Component, Default, Clone)]
pub struct DocumentPreviewScroll;
#[derive(Component, Default, Clone)]
pub struct DocumentPreviewBody;
#[derive(Component, Default, Clone)]
pub struct DocumentEditArea;
#[derive(Component, Default, Clone)]
pub struct DocumentMinimapRoot;
#[derive(Component, Default, Clone)]
pub struct DocumentMinimapRail;
#[derive(Component, Default, Clone)]
pub struct DocumentMinimapImage;
#[derive(Component, Default, Clone)]
pub struct DocumentMinimapThumb;
#[derive(Component, Default, Clone)]
pub struct DocumentMinimapLabel;
/// 只记录原生编辑代次、焦点和本帧滚动值，不复制全文或接管光标/选区。
#[derive(Component, Default, Clone)]
pub struct DocumentScrollState {
    pub generation: Option<bevy::text::EditableTextGeneration>,
    pub focused: bool,
    pub preserved: Option<Vec2>,
    pub restore: Option<Vec2>, // 切回文件的首帧在原生光标跟随后恢复该文件自己的手动视区。
}
#[derive(Component, Default, Clone)]
#[require(DocumentScrollState)]
pub struct DocumentSource {
    pub key: Option<SourceKey>, // 跨域菜单只辨认输入实体，不暴露文档绑定内部身份。
}
#[derive(Component, Default, Clone)]
pub struct DocumentStatus;
#[derive(Component, Default, Clone)]
pub struct DocumentExternalNotice;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DocumentAction {
    #[default]
    Preview,
    Edit,
    Save,
    SaveAll,
    Reload,
    KeepDraft, // 只收起当前外部修订提示，不修改保存基线。
    ToggleExpand,
    OpenSelected,
    RestoreSidebar,
    ActivateTab(u64), // 明确标签ID，返回既有文档时保留其草稿与视区。
    CloseTab(u64),
}

#[derive(Component, Default, Clone)]
pub struct DocumentReloadLabel;
