//! 植被路径字段与F12资产浏览器双向联动：定位、受检赋值及显式子资产选择。
//!
//! 【功能设计】定位绝不提交字符或修改草稿；赋值冻结标签/字段/原值，
//! GLTF候选有界枚举且多候选必须选择。成功只改一条引用，不改作者源或其他LOD参数。

mod enumeration; // GLTF节点、网格和材质在统一64项预算中枚举。
mod helpers; // 字段类型、冻结身份和唯一写入边界。
mod interaction; // 路径按钮及带票据选择操作。
mod loading; // 根/子资产加载与固定预算枚举。
mod scroll; // 请求开始露出中央选择区，结束后回到原字段滚动位置。
mod widgets; // 沿用铜边风格的同行按钮及内嵌选择面板。

pub(super) use interaction::handle_reference_buttons; // 安装于作者输入阶段。
pub(super) use loading::{invalidate_changed_assets, poll_reference}; // 失效先于非阻塞完成采用。
pub(super) use scroll::sync_picker_scroll; // 选择区域和原表单之间的一次性滚动交接。
pub(super) use widgets::{spawn_field_actions, spawn_picker}; // 详细表单复用的统一资产控件。

#[cfg(test)]
mod tests; // 身份隔离、类型验证、预算及UI回归。
