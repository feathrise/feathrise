//! 动画作者文档的纯数据边界；读取、曲线计算和保真保存只供后台任务调用。

mod clips;
mod composition;
mod creation;
mod curves;
mod material_slots;
mod reader;
mod structure;
mod types;
mod writer;

pub use clips::new_scene_clip_id;
pub use composition::{ContentItem, ContentKind, ContentSpan, DirectClipMapping};
pub use material_slots::{MaterialChange, MaterialSlot};
pub use types::{
    AnimationDocument, Channel, Clip, ClipSnapshot, Keyframe, Node, PreviewJoint,
};
pub use reader::preview_joint_topology;
pub use structure::{GlbNodeInfo, GlbParent, GlbStructure, MeshGeometry};

#[cfg(any(test, feature = "runtime_test_support"))]
pub mod tests;
