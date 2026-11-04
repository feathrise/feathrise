//! F12连续河流样条、急流段与瀑布水幕作者工具。

mod actions; // 面板选择、结构、参数和连接动作的原子事务。
mod connections; // 稳定端点选择、显示与静水/河流/瀑布几何连续性派生。
mod edit; // 顶视/透视逐点绘制与控制点XZ拖动事务。
mod helpers; // 编辑期宽松校验、点选择与共享纯函数。
mod numeric_input; // 四项控制点参数复用公共专业数值框与单次撤销拖动事务。
mod object_click; // 多网格冻结原Collider点击，异步完成后仍核对原水体/作者/来源身份。
#[cfg(test)]
mod object_click_tests;
mod preview; // 三维中心线、宽度边、控制点与状态回显。
#[cfg(test)]
mod tests; // 跨模块纯逻辑回归与连接派生测试。
mod types; // 多网格延后单击只属于按下时的地图、画布、河流及输入代次。

pub(crate) use actions::apply_water_network_actions; // 输入阶段消费一帧面板动作。
pub(crate) use connections::sync_water_network_connections; // 水位、撤销和静水结构共享唯一连接派生入口。
pub(crate) use edit::edit_water_network_from_viewport; // Presentation阶段读取最终地形光标。
pub(crate) use helpers::validate_intermediate_water as validate_water_network_edit; // 静水结构与预览共享未完成水网校验。
pub(crate) use numeric_input::{
    WaterNetworkNumericInputSlot, WaterNetworkNumericInputState, WaterNetworkNumericKind,
    handle_water_network_numeric_inputs, setup_water_network_numeric_inputs,
    sync_water_network_numeric_inputs,
}; // UI只组合固定槽位，字段身份、事务和灵敏度由水网领域统一拥有。
pub(crate) use preview::{draw_water_network, sync_water_network_text}; // 只进入F12离屏相机与左栏固定文本。
