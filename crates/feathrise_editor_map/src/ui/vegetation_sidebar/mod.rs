//! 植被侧栏只读状态桥；不增加工具、修改作者数据或接管后台任务。

mod components;
mod systems;
mod types;

pub(crate) use components::{VegetationSidebarFeedbackText, VegetationSidebarRoot};
pub(crate) use types::VegetationSidebarFeedbackKind;

pub(crate) fn install(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.add_systems(
        Update,
        systems::sync_feedback
            .after(crate::authoring::start_vegetation_compile)
            .after(crate::authoring::sync_live_blade_density)
            .in_set(crate::MapEditorSystemSet::Presentation),
    ); // 读取同帧笔划、编译及密度发布后的回执，不靠诊断字符串推断任务完成。
}

#[cfg(test)]
mod tests;
