//! F12当前场景地表作者层列表、显式选择和可撤销删除接线。

mod components; // 固定行、容器和局部动作按钮的ECS身份。
mod constants; // 沿用植被六槽分页与20像素行高。
mod helpers; // 当前选择、分页和稳定目标校验。
mod input; // 场景局部选择与后台删除入口。
mod scene; // 地形页作者层列表的静态BSN外壳。
mod systems; // 固定行池增量回显与鼠标反馈。
mod types; // 不持有像素的短UI身份与显示快照。

#[cfg(test)] // 覆盖删除选择与列表绑定边界。
mod tests; // 使用实际ECS系统验证查询与分页行为。

use bevy::prelude::*; // 插件按现有编辑器阶段安装系统。

pub(crate) use helpers::{layer_display_label, selected_index}; // 状态卡与作者列表共享真实名称和高亮选择。
pub(crate) use scene::surface_authoring_panel; // 侧栏通过独立入口构造作者列表。

/// 后台任务在保存之前采用，UI输入在统一撤销之前完成。
pub(crate) fn install(app: &mut App) {
    app.init_resource::<crate::authoring::SurfaceLayerEditTask>() // 场景删除与共享像素写侧物化只有一个后台槽。
        .add_systems(
            Update,
            input::handle_input
                .after(crate::ui::actions::handle_editor_action_buttons) // 先完成关闭和保存等通用按钮请求。
                .before(crate::authoring::apply_undo_redo) // 同帧历史输入观察最终作者操作。
                .run_if(crate::scene_browser::scene_browser_general_input_available) // 文件夹模态和拖拽保持鼠标独占。
                .in_set(crate::SceneEditorSystemSet::Input),
        ) // 沿用正式输入阶段。
        .add_systems(
            Update,
            crate::authoring::poll_surface_layer_edit
                .after(crate::authoring::poll_draft_load) // 切换场景后先更新草稿，再复验旧任务身份。
                .before(crate::authoring::start_global_environment_save) // 删除完成前不能让全局保存提前消费场景保存请求。
                .before(crate::authoring::start_draft_save) // 保存只读取删除后完整定义和像素。
                .in_set(crate::SceneEditorSystemSet::Tasks),
        ) // 只做非阻塞就绪轮询。
        .add_systems(
            Update,
            (
                systems::spawn_rows,
                systems::sync_rows,
                systems::sync_interactions,
            )
                .chain() // 首次挂载、内容刷新和悬停反馈按顺序收口。
                .after(crate::authoring::apply_surface_brush) // 本帧笔刷修订与显式新建层一起回显。
                .before(crate::ui::layer_status::sync_editor_layer_status) // 页面状态读取最终选择与真实层表。
                .in_set(crate::SceneEditorSystemSet::Presentation),
        ); // 与其他作者列表同阶段显示。
} // 不增加运行时渲染视图或场景数据格式。

/// 离屏夹具复用真实六行池与状态，不安装输入或地表像素删除/涂刷任务。
#[cfg(test)]
pub(crate) fn install_snapshot_presentation(app: &mut App) {
    app.init_resource::<crate::authoring::SurfaceLayerEditTask>()
        .add_systems(Update, (systems::spawn_rows, systems::sync_rows, systems::sync_interactions)
            .chain().after(crate::ui::scene_inspector::sync_selected_tool_properties)
            .before(crate::ui::layer_status::sync_editor_layer_status)
            .in_set(crate::SceneEditorSystemSet::Presentation));
}
