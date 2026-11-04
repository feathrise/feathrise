//! 资产列表右键菜单：复制路径或打开冻结资产的外部编辑器与文件夹，不修改选择、预设或地图。

use bevy::prelude::*; // 菜单安装复用现有编辑器阶段和系统剪贴板。

use crate::MapEditorSystemSet; // 输入、后台采用和表现保持既有顺序。

mod components; // 固定遮罩、菜单和复制按钮的专用标记。
mod constants; // 短路径词法计算的固定预算。
pub(crate) mod helpers; // 路径身份、窗口边界及资产预览共用的复制结果逻辑。
mod messages; // 显式复制点击与系统剪贴板写入分离。
mod resources; // 仅持有一个临时菜单及关闭帧鼠标所有权。
mod scene; // 暗墨铜边菜单节点及原位布局、悬停回显。
mod systems; // 右键开关、失效取消和一次剪贴板写入。
mod types; // 冻结真实条目身份，避免固定行池换绑后复制错项。

pub(crate) use resources::AssetContextMenuState; // 统一鼠标门只读取菜单是否拥有本帧输入。
pub(crate) use messages::CopyAssetPath; // 中央文件菜单复用唯一剪贴板写入端。
pub(crate) use types::AssetPathCopyKind;

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AssetMenuInput; // 场景对象菜单在现有资产菜单仲裁后取得输入。

#[cfg(test)]
pub(crate) use components::AssetMenuReferencesButton;
#[cfg(test)]
pub(crate) use components::AssetMenuFavoriteButton;
#[cfg(test)]
pub(crate) fn control_menu_for_tests(world: &mut World) {
    use bevy::ecs::system::RunSystemOnce;
    world.run_system_once(systems::control_menu).unwrap();
}

/// 菜单输入先于普通编辑动作，剪贴板只在显式点击时调用。
pub(crate) fn install(app: &mut App) {
    crate::ui::external_files::register_requests(app);
    crate::ui::asset_shortcuts::register(app);
    app.init_resource::<AssetContextMenuState>() // 同一时刻只允许一个菜单。
        .init_resource::<Clipboard>() // DefaultPlugins已提供时不重复创建系统剪贴板。
        .add_message::<messages::CopyAssetPath>() // 测试可验证复制意图而不污染真实剪贴板。
        .add_systems(
            Update, // 所有输入按当前帧真实按键边沿消费。
            (systems::control_menu, systems::copy_requested_path)
                .chain() // 先验证点击行，再执行该次明确复制。
                .after(super::asset_preview::AssetTabMenuInputSet) // 文件菜单优先，已有遮罩不能被背景菜单取代。
                .after(crate::map_browser::handle_map_browser_input) // 模态和地图拖动先取得输入所有权。
                .before(
                    super::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
                ) // 普通按钮和笔刷同帧读到菜单隔离门。
                .in_set(AssetMenuInput)
                .in_set(MapEditorSystemSet::Input), // 目录采用前冻结当前显示条目。
        ) // 不使用run_if积压关闭帧或点击事件。
        .add_systems(Update, scene::setup_menu.in_set(MapEditorSystemSet::Tasks)) // 编辑器根出现时创建一次固定节点。
        .add_systems(
            Update,
            scene::sync_menu.after(super::rows::sync_asset_rows).in_set(MapEditorSystemSet::Presentation),
        ); // 刷新/翻页后同帧隐藏失效菜单。
}

#[cfg(test)] // 自动回归不访问操作系统剪贴板。
mod tests; // 行身份、输入隔离和窗口/全屏缩放边界。
