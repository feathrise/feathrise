//! F12全局植被预设管理：Asset检查、内嵌编辑、引用保护删除与原子热采用。
//!
//! 【功能设计】预设数量不由UI实体或Rust枚举限定；固定按钮池只负责浏览，
//! 本模块直接维护开发者全局目录。地图继续只保存稳定预设ID，编辑不会改写该身份或后端类型。

mod asset_reference; // 路径定位、单字段赋值和显式GLTF子资产选择，不重写其他LOD参数。
mod components; // 文档层级与控件视觉身份，不进入正式预设数据。
mod defaults; // 从真实GLTF事实生成可通过公共校验的Blade/Mesh默认规格。
mod fields; // 中央详细字段与正式强类型描述之间的唯一读写映射。
mod inspection; // 根Asset异步加载与节点/primitive固定预算分帧检查。
mod interaction; // 局部按钮、IME名称和ASCII数值字段输入。
mod mutations; // 枚举切换与Mesh LOD/部件结构修改。
mod persistence; // 乐观并发、全地图引用保护、原子写入与热采用门。
pub(crate) mod project_close;
mod presentation; // 固定BSN壳的文字、显隐、焦点和按钮反馈。
mod scene; // 植被页内嵌静态面板，不遮挡右侧Asset浏览器。
mod types; // 草稿、任务、检查队列和UI marker普通类型。
mod workspace; // 中央标签条与活动分区的动态、有限实体构造。
#[cfg(test)]
pub(crate) use workspace::{install_workspace_tab_fixture, set_workspace_tab_fixture_context};
#[cfg(test)]
pub(crate) use types::PresetWorkspaceTabBar;
#[cfg(test)]
pub(crate) fn install_sidebar_presentation_for_tests(app: &mut bevy::prelude::App) {
    app.add_systems(
        bevy::prelude::Update,
        presentation::sync_panel.in_set(crate::MapEditorSystemSet::Presentation),
    ); // 侧栏夹具运行原摘要和诊断同步，不开放私有编辑字段或复制预设状态。
}

#[cfg(test)]
pub(crate) fn install_workspace_presentation_for_tests(app: &mut bevy::prelude::App) {
    // 实际中央文档夹具复用正式页面显隐，私有SystemParam不成为功能域接口。
    app.add_systems(
        bevy::prelude::Update,
        workspace::sync_workspace.in_set(crate::MapEditorSystemSet::Presentation),
    );
}

use bevy::prelude::*; // App、Update与系统注册。

/// 共用原中央标签及文档构造；这里只安装表现所需会话，不启动任何目录读写或作者预览。
pub(crate) fn install_workbench_presentation(app: &mut App) {
    app.init_resource::<VegetationPresetEditorState>()
        .init_resource::<super::surface_preset_editor::SurfacePresetEditorState>()
        .init_resource::<super::water_preset_editor::WaterPresetEditorState>()
        .init_resource::<super::building_presets::BuildingPresetEditorState>()
        .init_resource::<feathrise_editor_core::WorkspaceDomainState>()
        .add_systems(Update, workspace::sync_workspace.in_set(crate::MapEditorSystemSet::Presentation));
}

pub(crate) use scene::{
    vegetation_preset_editor_panel, vegetation_preset_feedback,
    vegetation_workspace_editor_page, vegetation_workspace_tab_bar,
}; // 左栏全局库、唯一诊断与中央静态挂载点由同一功能域提供。
pub use types::{VegetationPresetEditorState, PresetWorkspaceTabHost, PresetWorkspaceEditorHost};
pub use feathrise_editor_core::PresetWorkspaceDynamicRoot;
pub(crate) use types::{
    PresetEditorAction,
    PresetEditorButton,
    PresetNotesScroll, // 备注局部滚动复用外层统一滚轮路由。
    PresetWorkspaceMapPage,
    PresetWorkspaceEditorPage, // 独立宿主沿用原中央页面，不复制地图/预设布局。
    PresetWorkspaceTab,

}; // 顶层场景、地表共享标签和快捷键隔离使用同一工作区身份。

use crate::MapEditorSystemSet; // 接入编辑器既有输入、任务和Presentation顺序。

/// 共享正文换代先提交延迟命令，局部检查器只能读取本帧仍存活的实体。
pub(crate) use feathrise_editor_core::PresetWorkspaceRebuildSet;

/// 安装预设管理资源与三个阶段系统。
pub(crate) fn install(app: &mut App) {
    app.init_resource::<VegetationPresetEditorState>()
        .init_resource::<feathrise_editor_core::WorkspaceDomainState>()
        .add_systems(
            Update,
            (
                interaction::handle_text_input, // 已聚焦字段先收口同帧键盘与IME。
                asset_reference::handle_reference_buttons, // 定位不提交输入，显式赋值冻结所属标签/字段。
                interaction::handle_buttons, // 按钮随后读取最终缓冲并登记资产或目录任务。
                interaction::sync_text_focus, // 明确按钮业务目标后聚焦官方字段。
            )
                .chain()
                .in_set(MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (
                inspection::poll_asset_inspection, // 根加载与大GLTF检查均非阻塞推进。
                asset_reference::invalidate_changed_assets, // 模型热更后不能选择旧编号候选。
                asset_reference::poll_reference,   // 有界枚举及真实类型验证完成后才改一条引用。
                persistence::poll_catalog_write,   // 文件结果采用后显式触发全局目录热加载。
            )
                .chain()
                .in_set(MapEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (
                asset_reference::sync_picker_scroll, // 长表单下方发起选择也能看到候选，结束后回到原字段。
                workspace::sync_workspace.in_set(PresetWorkspaceRebuildSet), // 正文换代是局部回显的延迟命令屏障。
                presentation::sync_panel, // 正文命令提交后同时同步侧栏和文档诊断。
                presentation::sync_document_badge,
                presentation::sync_button_styles, // 局部按钮不扩大全局EditorAction查询。
            )
                .chain()
                .in_set(MapEditorSystemSet::Presentation),
        ); // 备注真实光标滚动由官方TextScroll负责。
}

// 共享会话保持唯一嵌入状态；只在真实修改后标记Changed，避免稳定帧整树回显。
pub fn edit_shared_workspace(
    workspace: &mut ResMut<VegetationPresetEditorState>,
    edit: impl FnOnce(&mut feathrise_editor_core::PresetWorkspaceState),
) {
    let before = (workspace.active_tab, workspace.ui_revision);
    edit(&mut workspace.bypass_change_detection().workspace);
    if before != (workspace.active_tab, workspace.ui_revision) {
        workspace.set_changed();
    }
}
