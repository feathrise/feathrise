//! 通用插件控件只携带插件、文档与字段身份，不认识装备类型。
use bevy::prelude::*;
use feathrise_plugin_api::Request;

#[derive(Component, Clone)]
pub(super) struct ToolButton {
    pub index: usize,
    pub action: UiAction,
}
#[derive(Clone)]
pub(super) enum UiAction {
    OpenTool,
    Tab(u32),
    CloseLibrary,
    Request(Request),
    Close(u32),
    Choices(String),
    Adopt {
        document: u32,
        field: String,
        extensions: Vec<String>,
    },
    Locate {
        document: u32,
        field: String,
    },
}
#[derive(Component)]
pub(super) struct ToolHeader;
#[derive(Component)]
pub(super) struct ToolPreviewHost(pub usize);
#[derive(Component)]
pub(super) struct ToolPanel {
    pub index: usize,
    pub revision: u64,
}
#[derive(Component)]
pub(super) struct ToolBody(pub usize);
#[derive(Component)]
pub(super) struct ToolText {
    pub index: usize,
    pub document: u32,
    pub field: String,
    pub baseline: String,
    pub multiline: bool,
}
#[derive(Component)]
pub(super) struct ToolNumber {
    pub index: usize,
    pub document: u32,
    pub field: String,
}

/// 中央工作台与共享工作区同帧换代；右侧预览不挂在这个动态根上。
#[derive(Component)]
pub(super) struct ToolWorkspaceRoot;
#[derive(Component)]
pub(super) struct ToolCatalog {
    pub index: usize,
    pub view: feathrise_plugin_api::CatalogView,
}
