//! 地表综合色与正式Mesh植被密度预览。

mod mesh; // 密度散布直接复用已登记多档原型、风材质和LOD节点。
mod surface; // 五层RGBA作者权重的有界综合色预览。

pub(crate) use mesh::{
    clear_vegetation_previews, poll_vegetation_preview_rebuild, start_vegetation_preview_rebuild,
    suppress_new_runtime_mesh_roots,
};
pub(crate) use surface::{
    install_surface_preview_render, poll_surface_preview_rebuild, start_surface_preview_rebuild,
    same_surface_canvas, surface_canvas_mesh,
}; // 未保存作者纹理与实际GPU管线共同就绪后才接管视口。
