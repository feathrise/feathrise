//! 外部作者文档的不可变版本与原地图事务端口；这里只拥有类型擦除值，不定义游戏格式。

use bevy::prelude::*;
use feathrise_map_content::{MapCatalogEntry, MapId, authoring_transaction::PreparedWrite};
use std::{
    any::Any,
    path::{Path, PathBuf},
    sync::Arc,
};

pub type DomainValue = Arc<dyn Any + Send + Sync>;

/// 游戏拥有正文、校验和编码实现；普通地图只调度唯一保存事务和来源冲突守卫。
#[derive(Clone)]
pub struct MapDomainDocument {
    pub domain: &'static str,
    pub value: DomainValue,
    pub ops: &'static MapDomainDocumentOps,
}

pub struct MapDomainDocumentOps {
    pub dirty: fn(&dyn Any) -> bool,
    pub hash: fn(&dyn Any) -> &str,
    pub asset_path: fn(&dyn Any) -> &str,
    pub registration_required: fn(&dyn Any) -> bool,
    pub guard: fn(&Path, &dyn Any) -> Result<DomainSaveGuard, String>,
    pub prepare:
        for<'a> fn(&Path, &'a dyn Any) -> Result<Option<(PreparedWrite<'a>, String)>, String>,
    pub register: fn(&dyn Any, &mut MapCatalogEntry) -> Result<(), String>,
    pub saved: fn(&dyn Any, &str) -> DomainValue,
    pub follow_terrain:
        fn(&dyn Any, &super::terrain_ground::TerrainGroundSnapshot) -> Result<DomainValue, String>,
}

#[derive(Default)]
pub struct DomainSaveGuard {
    pub files: Vec<(PathBuf, String)>,
    pub catalog_hash: Option<String>,
}

impl MapDomainDocument {
    pub fn dirty(&self) -> bool {
        (self.ops.dirty)(&*self.value)
    }
    pub fn hash(&self) -> &str {
        (self.ops.hash)(&*self.value)
    }
    pub fn asset_path(&self) -> &str {
        (self.ops.asset_path)(&*self.value)
    }
    pub fn registration_required(&self) -> bool {
        self.dirty() && (self.ops.registration_required)(&*self.value)
    }
    pub fn get<T: Any>(&self) -> Option<&T> {
        self.value.downcast_ref()
    }
    pub fn get_mut<T: Any + Clone + Send + Sync>(&mut self) -> Option<&mut T> {
        // 【功能设计】只有写侧需要唯一所有权；克隆的是外部类型自己的浅版本，不复制整个场景。
        if Arc::get_mut(&mut self.value).is_none() {
            self.value = Arc::new(self.get::<T>()?.clone());
        }
        Arc::get_mut(&mut self.value)?.downcast_mut()
    }
}

/// 世界动作由原系统在Commands采用边界调用，游戏仍拥有自己的多文档状态与算法。
#[derive(Resource, Clone, Copy)]
pub struct MapDomainAdapter {
    pub load: fn(&Path, &MapId) -> Result<MapDomainDocument, String>,
    pub park: fn(&mut World, MapId),
    pub restore: fn(&mut World, MapId, bool),
    pub close: fn(&mut World, MapId),
    pub recover: fn(&mut World) -> bool,
    pub history: fn(&mut World, bool) -> Result<(), String>,
}

impl super::EditorMapDraft {
    pub fn adopted_domain_hash(&self) -> &str {
        self.domain.as_ref().map_or("", MapDomainDocument::hash)
    }
    pub fn domain_dirty(&self) -> bool {
        self.domain.as_ref().is_some_and(MapDomainDocument::dirty)
    }
}

impl super::EditorAuthoringSession {
    /// 外部领域在完整替换历史后重置同一画布归属游标，不直接写私有文档结构。
    pub fn reset_history_scope(&mut self) {
        if let Some(draft) = self.draft.as_mut() {
            draft.terrain_documents.scoped_through = 0;
        }
    }
}
