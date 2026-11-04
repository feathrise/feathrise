//! 当前应用作者域与普通地图工作区的真实接入合同；不发现、安装或动态加载插件。
//! 游戏拥有草稿和执行类型，普通编辑器只消费来源身份、已有预览根与UI挂载点。

use bevy::prelude::*;

pub mod map_api;
pub mod native_domain; // 外部领域的不透明草稿沿用地图历史、保存与地形身份。
pub mod map_interaction; // 当前游戏扩展的选择、输入与树事实，不持有其草稿或执行类型。

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalPreviewIdentity {
    pub domain: &'static str,
    pub tab: u64,
    pub revision: u64,
    pub ticket: u64,
}

/// 应用在公共预览选择之前发布；根所有权交给唯一资产预览生命周期，不再新建第二个RTT。
#[derive(Resource, Default)]
pub struct ExternalAssetPreviewRequest {
    pub active: bool,
    pub identity: Option<ExternalPreviewIdentity>,
    pub model_path: Option<String>,
    pub root: Option<Entity>,
    /// true表示应用提交完整组合根；GLB目录枚举及热更由应用负责，普通GLB预览继续使用编辑器加载器。
    pub constructed: bool,
    pub title: Option<String>,
    pub host: Option<Entity>,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AssetPreviewPrepare;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AssetPreviewLoading;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExternalPreviewAnimate;

/// 普通地面和浮层遮挡已采用、系统光标尚未显示的应用落点接入阶段。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomainGroundCursor;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct BaseGroundCursorResolved;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EditorCursorPresentation;

/// 继续使用普通编辑器数值拖动、目录模态与地图驻留门，游戏只登记自己的原有系统。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomainViewportInput;
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomainPresentation;

/// 本帧作者输入结束后刷新外部忙碌事实，地图停放不能沿用上一帧“空闲”。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DomainInputSettled;

/// 应用追加既有工具入口、工具正文及属性/预览，不重建普通编辑器布局。
#[derive(Component, Clone, Default)]
pub struct DomainModuleHost;
#[derive(Component, Clone, Default)]
pub struct DomainToolsHost;
/// 动态工具的显示名与右栏归属；仅登记通用UI事实，不承载领域或视口操作。
#[derive(Resource, Default)]
pub struct DomainToolLabels(pub std::collections::BTreeMap<&'static str, String>);

/// 标记唯一预览画布位于某个右侧工具页，沿用同一相机与输入可见门。
#[derive(Component)]
pub struct DomainToolPreviewHost(pub &'static str);
#[derive(Component, Clone, Default)]
pub struct DomainInspectorHost;
#[derive(Component, Clone, Default)]
pub struct DomainPreviewHost;
