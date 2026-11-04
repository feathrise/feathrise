//! 资产的外部编辑器联动：结构化请求、受检路径与唯一后台启动槽。

use bevy::prelude::*;

mod components;
mod constants;
mod discovery;
mod helpers;
mod launcher;
mod messages;
mod resources;
mod systems;
mod types;
#[cfg(test)]
mod tests;
#[cfg(test)]
pub(crate) use tests::{ExternalRequestProbe, install_probe};

pub(crate) use components::ExternalFileTab;
pub(crate) use messages::OpenExternalFile;
pub(crate) use messages::OpenExternalProjectFile;
pub(crate) use types::ExternalFileAction;
pub(crate) use types::ExternalEditor;
pub(crate) use resources::ExternalEditors;
pub(crate) use constants::EXTERNAL_MENU_ITEMS;
pub(crate) use launcher::platform_path; // 路径复制与IDE启动共用Windows盘符/UNC前缀转换。
pub(crate) use launcher::{checked_project, is_project_source, read_project_source}; // 引用后台与IDE定位共用当前工作树边界。

pub(crate) fn install(app: &mut App) {
    register_requests(app);
    app.init_resource::<resources::ExternalFileState>()
        .add_systems(Update, systems::drive.in_set(crate::MapEditorSystemSet::Tasks));
}

/// 菜单和文档的独立测试夹具也注册同一消息，实际启动只由正式插件安装一次。
pub(crate) fn register_requests(app: &mut App) {
    app.add_message::<OpenExternalFile>().add_message::<OpenExternalProjectFile>().init_resource::<ExternalEditors>();
}
