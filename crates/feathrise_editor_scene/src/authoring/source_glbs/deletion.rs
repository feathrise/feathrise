//! 红色删除绑定摆放及源字节。只在场景保存成功后永久删除实际 GLB。

use super::{source_path, verify_source_hash};
use crate::authoring::{EditorSceneDraft, files::{asset_path_collision_key, decode_scene_catalog, safe_asset_path}};
use feathrise_scene_content::{SCENE_CATALOG_ASSET_PATH, SceneId, ScenePlacementRecord, read_scene_placements};
use feathrise_scene_content::authoring_io::file_hash;
use std::{collections::{BTreeMap, BTreeSet}, path::{Path, PathBuf}, sync::Arc};

#[derive(Clone)]
pub(crate) struct GlbDeletion {
    pub(crate) placement_id: String,
    relative: String,
    expected_hash: String,
}

impl GlbDeletion {
    /// 捕获点只读取文件。异步结果由单行摆放事务再次核对后才进入草稿。
    pub(crate) fn capture(root: &Path, placement_id: String, relative: String) -> Result<Self, String> {
        let path = source_path(root, &relative)?;
        if !path.is_file() { return Err(format!("源 GLB 不是普通文件：{relative}")); }
        Ok(Self {
            placement_id,
            relative,
            expected_hash: file_hash(&path)?,
        })
    }

    pub(crate) fn relative(&self) -> &str { &self.relative }

    pub(crate) fn heap_bytes(&self) -> usize {
        self.placement_id.capacity() + self.relative.capacity() + self.expected_hash.capacity()
    }

    fn verify_path(&self, root: &Path) -> Result<PathBuf, String> {
        let path = source_path(root, &self.relative)?;
        verify_source_hash(&path, &self.expected_hash)?;
        Ok(path)
    }
}

pub(crate) type GlbDeletions = Arc<BTreeMap<String, GlbDeletion>>;

#[derive(Default)]
pub(crate) struct GlbDeletionReceipt {
    pub(crate) removed: Vec<String>,
    pub(crate) failures: Vec<String>,
}

/// 只在正常保存的已有后台事务中读取项目目录及摆放。失败发生在开始写入前。
pub(crate) fn preflight<'a>(root: &Path, draft: &'a EditorSceneDraft) -> Result<Vec<&'a GlbDeletion>, String> {
    if draft.glb_deletions.is_empty() { return Ok(Vec::new()); }
    let mut targets = BTreeMap::<_, &GlbDeletion>::new();
    for deletion in draft.glb_deletions.values() {
        let key = asset_path_collision_key(Path::new(deletion.relative()));
        let previous = targets.entry(key).or_insert(deletion);
        if previous.expected_hash != deletion.expected_hash {
            return Err(format!("同一源 GLB 的待删除版本不同：{}", deletion.relative()));
        }
    }
    for deletion in targets.values() { deletion.verify_path(root)?; }
    let key = |path: &str| asset_path_collision_key(Path::new(path));
    let shared = |scene: &SceneId, relative: &str| format!("源 GLB「{relative}」仍被场景 {scene} 使用；请移除引用或撤销文件删除");
    let verify_records = |scene: &SceneId, records: &[ScenePlacementRecord]| {
        for record in records {
            if let Some(target) = targets.get(&key(&record.asset_path)) {
                return Err(shared(scene, target.relative()));
            }
        }
        Ok(())
    };
    let verify_path = |scene: &SceneId, path: &str| {
        targets.get(&key(path)).map_or(Ok(()), |target| Err(shared(scene, target.relative())))
    };
    verify_records(&draft.scene_id, &draft.placements.placements)?;
    verify_path(&draft.scene_id, &draft.placements_path())?;
    for (path, _) in draft.source_glb_edits.0.iter() {
        verify_path(&draft.scene_id, path)?;
    }
    verify_path(&draft.scene_id, &draft.manifest.terrain.glb_path)?;
    for terrain in draft.manifest.terrains.as_deref().into_iter().flatten() {
        verify_path(&draft.scene_id, &terrain.terrain.glb_path)?;
    }
    let catalog_path = safe_asset_path(root, SCENE_CATALOG_ASSET_PATH)?;
    let catalog = decode_scene_catalog(&catalog_path, &std::fs::read(&catalog_path)
        .map_err(|error| format!("读取场景目录以核对 GLB 引用失败：{error}"))?)?;
    for entry in catalog.scenes.iter().filter(|entry| entry.id != draft.scene_id) {
        let relative = entry.placements_path();
        verify_path(&entry.id, &relative)?;
        let path = safe_asset_path(root, &relative)?;
        let bytes = std::fs::read(&path).map_err(|error| format!("读取场景 {} 摆放以核对 GLB 引用失败：{error}", entry.id))?;
        let placements = read_scene_placements(&bytes, &relative)?;
        if placements.scene_id != entry.id {
            return Err(format!("场景 {} 摆放身份不一致，无法核对 GLB 引用", entry.id));
        }
        verify_records(&entry.id, &placements.placements)?;
        let path = safe_asset_path(root, &entry.manifest_path())?;
        let manifest: feathrise_scene_content::SceneManifestAsset = serde_json::from_slice(&std::fs::read(&path)
            .map_err(|error| format!("读取场景 {} 清单以核对 GLB 引用失败：{error}", entry.id))?)
            .map_err(|error| format!("解析场景 {} 清单以核对 GLB 引用失败：{error}", entry.id))?;
        verify_path(&entry.id, &manifest.terrain.glb_path)?;
        for terrain in manifest.terrains.as_deref().into_iter().flatten() {
            verify_path(&entry.id, &terrain.terrain.glb_path)?;
        }
    }
    Ok(targets.into_values().collect())
}

/// 场景已持久化且作者锁仍在。文件失败时保留源，报告实际的部分完成结果。
pub(crate) fn finish(root: &Path, deletions: Vec<&GlbDeletion>) -> GlbDeletionReceipt {
    let mut receipt = GlbDeletionReceipt::default();
    for deletion in deletions {
        let result = deletion.verify_path(root).and_then(|path| {
            std::fs::remove_file(&path).map_err(|error| format!("永久删除源 GLB 失败：{error}"))
        });
        match result {
            Ok(()) => receipt.removed.push(deletion.relative.clone()),
            Err(error) => receipt.failures.push(format!("「{}」{error}", deletion.relative)),
        }
    }
    receipt
}

/// 停放标签的未保存引用还没写盘，按帧扫描借用记录，避免在保存输入帧拷贝或遍历全部场景。
#[derive(Default)]
pub(crate) struct ParkedGlbReferences {
    scope: Option<GlbDeletions>,
    documents: BTreeMap<SceneId, (u64, crate::authoring::placement_deletion::PlacementDeletionIdentity)>,
    paths: BTreeSet<String>,
    scene: Option<SceneId>,
    record: usize,
}

impl ParkedGlbReferences {
    pub(crate) fn advance(
        &mut self, pending: &GlbDeletions, documents: &crate::ui::scene_documents::SceneDocuments,
    ) -> Result<bool, String> {
        use std::ops::Bound::{Excluded, Unbounded};

        let unchanged = self.scope.as_ref().is_some_and(|source| Arc::ptr_eq(source, pending))
            && self.documents.len() == documents.parked.len()
            && documents.parked.iter().all(|(id, document)| self.documents.get(id).is_some_and(|(revision, identity)| {
                *revision == document.session.revision && identity.matches(&document.session.placement_deletions)
            }));
        if !unchanged {
            self.scope = Some(pending.clone());
            self.documents = documents.parked.iter().map(|(id, document)| {
                (id.clone(), (document.session.revision, document.session.placement_deletions.identity()))
            }).collect();
            self.paths = pending.values().map(|deletion| asset_path_collision_key(Path::new(deletion.relative()))).collect();
            self.scene = documents.parked.first_key_value().map(|(scene, _)| scene.clone());
            self.record = 0;
        }
        for _ in 0..16 { // 和同一保存入口的摆放采集一样跨帧继续，无场景总量限制。
            let Some(scene) = self.scene.as_ref() else { return Ok(true); };
            let draft = documents.parked.get(scene).and_then(|document| document.session.draft.as_ref());
            let record = draft.and_then(|draft| draft.placements.placements.get(self.record));
            if let Some(record) = record {
                if self.paths.contains(&asset_path_collision_key(Path::new(&record.asset_path))) {
                    return Err(format!("源 GLB「{}」仍被打开的场景 {scene} 使用", record.asset_path));
                }
                self.record += 1;
            } else {
                self.scene = documents.parked.range((Excluded(scene.clone()), Unbounded)).next().map(|(next, _)| next.clone());
                self.record = 0;
            }
        }
        Ok(self.scene.is_none())
    }
}
