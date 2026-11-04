//! 全局静态网格LOD的常规设置卡；专业数值控件与原唯一自动保存链共享。

mod components;
mod scene;
mod systems;
#[cfg(test)]
mod tests;

pub(crate) use components::MeshLodField;
pub(super) use scene::panel;
pub(crate) use systems::{handle_mesh_lod_inputs, setup_mesh_lod_inputs, sync_mesh_lod_inputs};
