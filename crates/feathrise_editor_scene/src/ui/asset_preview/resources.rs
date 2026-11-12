//! 资产预览的临时导航状态；不写入场景、预设、设置或撤销记录。

pub use feathrise_editor_animation_model::media::AssetPreviewWorkspace;
pub(super) use feathrise_editor_animation_model::media::MAX_MEDIA_TABS;

pub(crate) use feathrise_editor_animation_model::media::AssetPreviewNavigation;

pub(crate) use feathrise_editor_core::AssetPreviewFocus; // 浏览意图和技能库使用同一资源，不复制焦点。
