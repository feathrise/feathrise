//! 活动与停放作者文档的弱删除索引；不拥有作者整表。

use std::collections::HashMap;
use bevy::prelude::Resource;
use feathrise_scene_content::SceneId;
use super::types::DocumentIndex;

#[derive(Resource, Default)]
pub(crate) struct PlacementDeletionIndex {
    pub(super) documents: HashMap<SceneId, DocumentIndex>,
    pub(super) revision: u64,
}

impl PlacementDeletionIndex {
    pub(crate) fn revision(&self) -> u64 { self.revision }
}
