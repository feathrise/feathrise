//! 文本、源码与配置资产的连续滚动预览和完整编辑；Markdown另用CommonMark排版。
//! 选择文件不写盘；草稿在本进程保留，只有单文件或批量显式保存提交完整文档。

use crate::MapEditorSystemSet;
use bevy::prelude::*;

mod components;
mod batch_save; // 本批/全部草稿共享唯一IO槽，逐文件校验并保留失败项。
mod constants;
mod external; // 返回聚焦窗口或重新打开F12时核对磁盘，保留未保存草稿。
mod history; // 文件独立的有界历史与保存点，保留原生单一输入。
mod interaction;
mod io;
mod json; // JSON诊断、格式化与保存前校验共享后台槽，不改变其他文本格式。
mod minimap; // 中央源码的单槽轮廓纹理与原生滚动导航。
mod navigation; // 全文查找替换、冻结选区与原生跳行，保持单一输入。
mod native; // 多标签各自保留原生状态，活动文件仍只使用一个ECS输入。
#[cfg(test)]
mod parser;
pub(crate) mod project_close;
mod presentation;
mod resources;
mod source; // 工程源码与原资产文本共用会话，内部身份不参与AssetServer加载。
mod project_files;
mod source_location; // 编译诊断沿原中央文档和后台定位槽跳转，不创建第二份可写源码。
#[cfg(feature = "validation")]
pub(crate) use source_location::validation as source_location_validation;
mod search; // assets跨文件搜索与只读中央定位，复用单一文档输入。
mod render; // 完整预览按固定预算构造、原子展示与退役，没有正文翻页入口。
mod scene;
mod session;
mod syntax; // 后台分词与原生可见字形着色，共用于连续预览和编辑。
#[cfg(test)]
mod tests;
mod types;
mod workspace; // 在中央与侧栏之间原位挂载同一棵文档UI。
#[cfg(any(test, feature = "validation"))]
pub(crate) mod validation; // 仅显式有限验收访问固定文本会话，不另建保存管线。

pub(crate) use resources::{DocumentState, DocumentMinimapState};
pub(crate) use project_files::OpenProjectDocument;
pub(crate) use source::is_project as is_project_document;
pub(crate) use search::AssetSearchState;
pub(crate) use search::AssetReplaceState;
pub(crate) use batch_save::{DocumentBatchSaveState, SaveAllDocuments};
pub(crate) use search::panel as asset_search_panel;
pub(super) use components::{DocumentTabRoot, DocumentSource}; // 文件菜单仅辨认命中与原生输入，不读取草稿内容。
pub(super) use scene::panel;
pub(super) use scene::sidebar_launch;
pub(super) use scene::open_selected_button;
pub(crate) use scene::center_host;
#[cfg(test)]
pub(crate) use workspace::{install_snapshot_document_tabs, set_snapshot_document_tabs_active};

pub(super) fn install(app: &mut App) {
    project_files::install(app);
    source_location::install(app);
    history::install(app);
    json::install(app);
    minimap::install(app);
    navigation::install(app);
    search::install(app);
    syntax::install(app);
    batch_save::install(app);
    app.init_resource::<DocumentState>()
        .init_resource::<resources::DocumentInputs>()
        .init_resource::<resources::PreviewRenderState>()
        .add_systems(
            Update,
            session::capture_edits
                .after(bevy::text::apply_text_edits)
                .before(MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (interaction::actions, interaction::scroll)
                .chain()
                .after(crate::map_browser::handle_map_browser_input)
                .after(crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard)
                .before(crate::ui::actions::handle_editor_shortcuts)
                .before(crate::ui::systems::scroll_editor_side_panels)
                .in_set(MapEditorSystemSet::Input),
        )
        .add_systems(
            Update,
            (session::sync_selection, external::request_check, session::drive_tasks)
                .chain()
                .after(super::loading::sync_asset_preview_selection)
                .in_set(MapEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            (workspace::sync, native::bind, presentation::sync, render::sync)
                .chain()
                .in_set(MapEditorSystemSet::Presentation),
        )
        .add_systems(
            PostUpdate,
            native::capture_preview.after(bevy::ui::UiSystems::Layout),
        )
        .add_systems(
            PostUpdate,
            (presentation::capture_source_scroll, presentation::restore_source_scroll)
                .chain()
                // 滚动采用位于官方文字排版之后、候选框定位之前，两端均按实际系统排序。
                .after(bevy::ui::widget::update_editable_text_layout)
                .before(bevy::ui_widgets::ImeSystems::UpdatePosition)
                .before(bevy::input_focus::InputFocusSystems::FocusChangeEvents)
                .in_set(bevy::ui::UiSystems::PostLayout),
        ); // 静止排版不覆盖手动滚动，真实输入继续使用官方光标跟随。
}
