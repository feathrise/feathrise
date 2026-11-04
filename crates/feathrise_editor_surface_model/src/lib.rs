//! 地表预设的稳定编译边界；唯一草稿和 ECS 类型只在此定义，业务由独立插件注册。
pub mod components;
pub mod extensions;
pub mod host;
pub mod resources;
pub mod scene;
pub mod system_params;
mod tabs;
pub mod types;
pub use extensions::SurfaceEditorExtension;
pub use host::SurfaceEditorHost;
pub use resources::SurfacePresetEditorState;
pub use scene::{surface_preset_feedback, surface_preset_panel};
pub use tabs::spawn_surface_tabs;
