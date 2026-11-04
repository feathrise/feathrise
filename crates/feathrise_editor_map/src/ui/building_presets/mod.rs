//! F12 建筑预设目录、中央表单、原子保存与专用摆放控制。
//!
//! 【功能设计】建筑模型仍由右侧资产浏览器选择，但地图实例只保存已经通过全局
//! 目录校验的规范预设 ID；目录写入失败时继续使用上一份有效热加载快照。

mod components; // 工作台节点与按钮视觉身份，不改变持久化数据。
mod fields; // 建筑语义字段的唯一显示、解析与范围校验。
mod interaction; // 列表选择、草稿字段、分类和摆放参数输入。
mod list; // 任意长度三列目录的固定预算创建、重绑和退役。
mod persistence; // 全局 JSON 乐观并发、引用保护和原子发布。
pub(crate) mod project_close;
mod presentation; // 页头状态与工作台按钮按真实修订和交互变化同步。
mod scene; // 左栏管理卡和连续列表静态壳。
mod types; // 草稿、动作、任务和 ECS 标记。
mod workspace; // 中央浏览器式标签和完整建筑语义表单。

use bevy::prelude::*; // App 与 Update 系统注册。

pub(crate) use scene::building_preset_panel; // 左栏建筑页组合唯一公开静态场景。
pub(crate) use types::{
    BuildingPlacementControlText, BuildingPlacementControls, BuildingPresetEditorState,
    BuildingPresetStatusText,
}; // 资源与唯一反馈标记由侧栏宿主复用。
pub(crate) use workspace::{spawn_active_building_editor, spawn_building_tabs}; // 公共中央工作区加入建筑标签。

use crate::MapEditorSystemSet; // 建筑输入、任务和回显遵循编辑器统一阶段。

/// 安装建筑预设作者功能；不启动游戏或同步磁盘扫描。
pub(crate) fn install(app: &mut App) {
    app.init_resource::<BuildingPresetEditorState>() // 未保存草稿与目录任务独立于地图草稿。
        .init_resource::<BuildingPlacementControls>() // 旋转和网格只属于当前 F12 会话。
        .add_systems(
            Update,
            (
                interaction::handle_text_input, // 键盘先提交当前字段。
                interaction::handle_buttons,    // 同帧按钮读取已提交草稿。
            )
                .chain()
                .in_set(MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            persistence::poll_catalog_write.in_set(MapEditorSystemSet::Tasks), // 非阻塞采用后台文件结果。
        )
        .add_systems(
            Update,
            (
                list::sync_building_preset_list, // 结构和标签按固定预算同步。
                list::sync_building_preset_button_styles, // 当前选择与悬停独立回显。
                interaction::sync_panel_text,    // 管理摘要和摆放参数只在值变化时写文字。
                interaction::sync_sidebar_button_styles, // 固定参数按钮与任意长度目录的样式预算隔离。
                presentation::sync_document_badge,
                presentation::sync_button_styles,
            )
                .chain()
                .after(crate::ui::vegetation_preset_editor::PresetWorkspaceRebuildSet) // 共享正文提交后再同步新页头和导航。
                .in_set(MapEditorSystemSet::Presentation),
        );
}

#[cfg(test)]
mod workbench_tests;
