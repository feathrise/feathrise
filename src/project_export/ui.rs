//! 工程导出使用共同对话框与原生文本输入；目录选择和打开目录均不阻塞作者帧。

mod resources;
mod scene;
mod systems;
#[cfg(test)]
mod tests;

pub(super) use scene::mount;
pub(super) use systems::{handle, present};
pub(crate) use resources::ExportOverlayState;
use bevy::prelude::*;

pub(super) fn install(app: &mut App) {
    app.init_resource::<resources::ExportOverlayState>()
        .init_non_send::<resources::ExportFolderPicker>();
}

/// 关闭释放帧也占有输入，不能把Esc、鼠标释放或滚轮交给背后的场景工具。
pub(crate) fn captures_input(world: &World) -> bool {
    world.get_resource::<resources::ExportOverlayState>().is_some_and(|state| state.captured)
        || world.get_resource::<super::resources::ExportState>().is_some_and(|state| state.open)
}

pub(crate) fn ready_to_exit(world: &World) -> bool {
    world.get_non_send::<resources::ExportFolderPicker>().is_none_or(|state| state.0.is_none())
        && world.get_resource::<resources::ExportOverlayState>().is_none_or(|state| state.open_directory.is_none())
}

/// 只在原关闭事务已明确授权后调用；原生对话框仍由自己的STA线程收口。
pub(crate) fn cancel_for_exit(world: &mut World) {
    if let Some(picker) = world.get_non_send::<resources::ExportFolderPicker>()
        && let Some(picker) = &picker.0
    {
        picker.cancel();
    }
    if let Some(mut state) = world.get_resource_mut::<super::resources::ExportState>() {
        state.open = false;
    }
}
