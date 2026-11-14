//! 中央资产文件标签的右键菜单；外部打开冻结文件，批量保存或关闭文件页，保留文档会话和作者预设。

use crate::SceneEditorSystemSet;
use bevy::prelude::*;

mod components;
mod messages;
mod resources;
mod scene;
mod system_sets;
mod systems;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(super) use tests::{gpu_click_close_all, gpu_click_close_unmodified, gpu_click_save_all, gpu_click_external, gpu_click_copy_path, gpu_assert_external_visibility, gpu_open_menu, gpu_pointer_input}; // 正式GPU文档夹具复用实际菜单鼠标命中。

pub(crate) use resources::AssetTabMenuState;
pub(crate) use system_sets::AssetTabMenuInputSet;

pub(super) fn install(app: &mut App) {
    crate::ui::external_files::register_requests(app);
    app.init_resource::<AssetTabMenuState>()
        .add_message::<crate::ui::asset_context_menu::CopyAssetPath>()
        .add_message::<super::documents::SaveAllDocuments>()
        .add_message::<messages::CloseAllAssetTabs>()
        .add_message::<messages::CloseUnmodifiedAssetTabs>()
        .add_systems(Update, (systems::control_menu, systems::close_all_tabs).chain()
            .after(crate::scene_browser::handle_scene_browser_input)
            .before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(AssetTabMenuInputSet)
            .in_set(SceneEditorSystemSet::Input))
        .add_systems(Update, scene::setup.in_set(SceneEditorSystemSet::Tasks))
        .add_systems(Update, scene::sync.in_set(SceneEditorSystemSet::Presentation));
}
