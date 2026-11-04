//! 当前Scene0网格选择的固定预算解析与视口表现系统。

mod presentation; // 精确覆盖、身份徽标、三维范围框与双击定位。
mod resolve; // 活动根、World票据与完整primitive路径的有界解析。

pub(crate) use presentation::{
    draw_selected_placement_highlight_frame, sync_selected_placement_viewport,
}; // 父模块保持原系统路径和插件注册接口。
pub(crate) use resolve::resolve_selected_placement_mesh; // Cursor阶段先原子发布完整源primitive。
