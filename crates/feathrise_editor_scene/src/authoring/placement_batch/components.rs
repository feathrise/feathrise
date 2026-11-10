//! 批量作者操作的按钮与临时模型身份；预览尚未成为正式作者记录。
use super::types::PlacementBatchAction;
use bevy::prelude::*;

#[derive(Component)]
pub(crate) struct PlacementBatchButton(pub(crate) PlacementBatchAction);

#[derive(Component)]
pub(super) struct PasteGhost;

/// 公共显隐父级随场景作者预览退休；最后一个孩子移走或删除时立即回收空容器。
#[derive(Component)]
pub(crate) struct PastePreviewGroup {
    pub(super) scene_id: feathrise_scene_content::SceneId,
    pub(super) key: String,
}

#[cfg(test)]
impl PastePreviewGroup {
    pub(crate) fn test_group(map: feathrise_scene_content::SceneId, key: String) -> Self {
        Self { scene_id: map, key }
    }
}
