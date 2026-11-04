//! F12 全局太阳/月光基础值与当前地图逐属性覆盖的专业数值输入。
//!
//! 【功能设计】六组属性共享全局完整配置；地图颜色以 RGB 整组继承，启用覆盖时复制
//! 当前解析值保证画面不跳变。拖动只实时更新小型内存快照，松手后才生成一条撤销记录。

mod contract; // 字段、范围、继承和标记类型集中定义，避免 UI 与事务各自解释格式。
mod scene; // 固定 BSN 槽位首次挂载公共 NumericInput 子树。
mod sync; // 作者值、跟随禁用态、颜色预览与折叠状态同步。
mod systems; // 精确输入、拖动事务、继承切换和折叠交互。

pub(crate) use contract::{
    CelestialColorSwatch, CelestialNumericInputSlot, CelestialNumericInputState,
    CelestialNumericKey, CelestialNumericScope, CelestialOverrideProperty, CelestialOverrideToggle,
    CelestialOverrideToggleLabel, CelestialSectionBody, CelestialSectionState,
    CelestialSectionToggle, CelestialSectionToggleLabel, CelestialValueField,
};
pub(crate) use scene::setup_celestial_numeric_inputs;
pub(crate) use sync::sync_celestial_controls;
pub(crate) use systems::{
    handle_celestial_numeric_inputs, handle_celestial_override_toggles,
    handle_celestial_section_toggles,
};
