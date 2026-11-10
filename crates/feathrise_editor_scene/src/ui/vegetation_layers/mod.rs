//! F12植被作者图层列表、结构操作和参数操作。

mod allocation; // 大画布在后台single-flight清零，身份匹配后才原子加入作者结构。
mod layer_creation; // 新建、复制与唯一私有作者源身份。
mod numeric_input; // 五个连续作者参数共享专业数值框与拖动事务。
mod operations; // 纯作者值变换、Arc浅快照与统一撤销命令。
pub(crate) use operations::vegetation_canvas_unavailable; // 预设选择、底栏反馈和建层入口共用当前画布身份门。
mod systems; // 静态按钮、固定行池和正式profile选择接线。
mod scene_menu;
pub(crate) use scene_menu::input as handle_scene_menu;

pub(crate) use allocation::{
    VegetationBlankLayerAllocation, poll_vegetation_blank_layer_allocation,
};
pub(super) use numeric_input::vegetation_numeric_slots; // BSN面板只声明稳定槽位，子树由公共控件一次生成。
pub(crate) use numeric_input::{
    VegetationNumericInputState, apply_vegetation_numeric_scrub, handle_vegetation_numeric_input,
    setup_vegetation_numeric_inputs, sync_vegetation_numeric_inputs,
};
pub(crate) use systems::VEGETATION_LAYER_ROW_COUNT;
pub(crate) use systems::{
    handle_vegetation_authoring_buttons, handle_vegetation_layer_row_clicks,
    spawn_vegetation_layer_rows, sync_vegetation_layer_row_interactions,
    sync_vegetation_layer_rows,
}; // 插件按Input事务与Presentation同步阶段分别注册公共数值框接线。
