//! 场景检查器的对象属性与本对象诊断；全局操作反馈使用底栏，作者事务仍归原功能域。

mod components;
mod scene;
mod selected_tool_properties;
mod systems;
mod types;

pub use components::InspectorSection;
pub(crate) use components::{InspectorTree, InspectorBody};

pub(super) use scene::{
    scene_inspector_feedback, scene_inspector_overview, scene_inspector_properties,
};
pub(crate) use systems::sync_scene_inspector;
pub(crate) use components::SelectedToolProperties;
pub(crate) use types::SelectedToolPropertiesKind;
pub(crate) use selected_tool_properties::sync_selected_tool_properties;

#[cfg(test)]
pub(crate) use selected_tool_properties::{set_selected_tool_properties_fixture, install_selected_tool_properties_snapshot_presentation};

#[cfg(test)]
pub(crate) mod tests;
