//! 编辑器跨功能契约；标签、输入和预览意图只有一个类型与状态来源。

pub mod constants; // 作者预览通道由编辑器拥有，运行时无须携带这些工具参数。

mod status;
pub use status::{EditorStatusKind, EditorStatusOperation, EditorStatusTarget};

mod external_editor;
pub use external_editor::ExternalEditor;

mod project_paths;
mod fonts;
pub use fonts::{editor_font, editor_font_template};
pub use project_paths::{
    EditorProjectPaths, EditorResourcePaths, bind_editor_project_paths, bind_editor_resource_paths,
    editor_asset_root, editor_project_paths, editor_resource_root,
    sdk_asset_root, sdk_workspace_root, workspace_asset_root,
};

mod components;
pub mod view_projection;
mod resources;
mod system_sets;
mod types;
pub use system_sets::*;

pub use components::*;
pub use resources::*;
pub use types::*;

/// 原一体作者和自定义资产读取的兼容路径；独立宿主通过统一字体入口使用 SDK 内置默认字体。
pub const FONT_PATH: &str = "fonts/open_source/noto_sans/cjk/NotoSansCJKsc-VF.otf";

#[cfg(feature = "runtime_test_support")]
static RUNTIME_TEST_ASSET_ROOT: std::sync::RwLock<Option<std::path::PathBuf>> = std::sync::RwLock::new(None);

/// 独立进程的GPU回归使用隔离作者目录；异步IO与AssetServer必须指向同一份夹具。
#[cfg(feature = "runtime_test_support")]
pub fn set_runtime_test_asset_root(root: Option<std::path::PathBuf>) {
    *RUNTIME_TEST_ASSET_ROOT.write().unwrap() = root;
}

mod settings; // 作者偏好及其持久格式只属于编辑器。
pub use settings::*;
