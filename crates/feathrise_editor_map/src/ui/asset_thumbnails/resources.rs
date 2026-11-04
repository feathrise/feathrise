//! 单飞检查/烘焙、有限LRU快照以及分帧退役队列。

use super::{components::RenderReceipt, types::*};
use bevy::prelude::*;
use bevy::tasks::Task;
use bevy::world_serialization::WorldAsset;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub(super) struct ThumbnailJob {
    pub epoch: u64,
    pub path: String,
    pub stamp: u64,
    pub started: f64,
    pub phase: JobPhase,
    pub target: Handle<Image>,
    pub camera: Entity,
    pub await_reload: bool,
    pub refresh_after_load: bool, // 旧初载/热更尚在途时，先消费其发布回执，再派发新一轮reload。
    pub root: Option<Entity>,
    pub scene: Option<Handle<WorldAsset>>,
    pub image: Option<Handle<Image>>,
    pub texture_preview: Option<Handle<crate::ui::asset_preview::compatible_texture::CompatiblePreviewTexture>>, // 唯一图片加载请求；材质只引用其私有安全label。
    pub owned_mesh: Option<Handle<Mesh>>,
    pub owned_material: Option<Handle<StandardMaterial>>,
    pub stack: Vec<NodeCursor>,
    pub nodes: usize,
    pub minimum: Vec3,
    pub maximum: Vec3,
    pub probes: Vec<Arc<RenderReceipt>>,
}

pub(super) struct Retirement {
    pub stack: Vec<NodeCursor>,
    pub job: ThumbnailJob, // 整棵旧树退役前保持源句柄，不能中途再实例化缺失依赖。
}

#[derive(Resource, Default)]
pub(super) struct ThumbnailState {
    pub identity: Option<WindowKey>,
    pub last_catalog: Option<WindowKey>,
    pub epoch: u64,
    pub paths: Vec<String>, // 永远只含当前页，最多十二项。
    pub inspection: Option<Task<InspectionResult>>,
    pub inspect_after: f64,
    pub cache: HashMap<String, CacheEntry>,
    pub reload_paths: HashSet<String>,
    pub used: u64,
    pub visual_revision: u64,
    pub active: Option<ThumbnailJob>,
    pub retiring: Option<Retirement>,
    pub camera: Option<Entity>,
    pub light: Option<Entity>,
}

impl ThumbnailState {
    pub fn changed(&mut self) {
        self.visual_revision = self.visual_revision.wrapping_add(1);
    }

    /// 至多淘汰一个非窗口快照；超高窗口也不能以保护所有可见项为由突破64张预算。
    /// 无空位时保留原类型占位，后续滚动改变保护窗口后再按原需求加载，不截断目录条目。
    pub fn make_room(&mut self, path: &str) -> bool {
        if self.cache.contains_key(path) || self.cache.len() < super::constants::CACHE_CAPACITY {
            return true;
        }
        let oldest = self
            .cache
            .iter()
            .filter(|(key, _)| !self.paths.contains(key))
            .min_by_key(|(_, entry)| entry.used)
            .map(|(key, _)| key.clone());
        if let Some(oldest) = oldest {
            self.cache.remove(&oldest);
            self.reload_paths.remove(&oldest);
            return true;
        }
        false // 全部缓存都仍受当前窗口保护；调用方不得继续插入第65张。
    }

    pub fn job_current(&self, job: &ThumbnailJob) -> bool {
        self.identity.is_some()
            && self.epoch == job.epoch
            && self.paths.contains(&job.path)
            && self
                .cache
                .get(&job.path)
                .and_then(|entry| entry.source.as_ref().ok())
                .is_some_and(|source| source.stamp == job.stamp)
    }
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod budget_tests;
