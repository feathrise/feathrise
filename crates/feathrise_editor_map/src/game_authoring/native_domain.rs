//! 原生地图作者的宿主内存适配；DLL仅接收字节合同，地图历史与跨文件事务仍使用原端口。
use super::map_interaction::*;
use crate::authoring::domain_documents::{
    DomainSaveGuard, DomainValue, MapDomainDocument, MapDomainDocumentOps,
};
use bevy::prelude::*;
use feathrise_map_content::{
    MapCatalogEntry, MapId, MapTerrainBinding,
    authoring_io::{bytes_hash, compare_content_hash, file_hash, safe_content_path},
    authoring_transaction::PreparedWrite,
};
use feathrise_plugin_api::map_domain::{self as wire, *};
use std::{
    any::Any,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock, RwLock},
};

/// 调用仅允许发生在地图IO/作者后台任务；闭包内部排入DLL唯一串行队列，直接回包而不等待主帧。
pub struct NativeDomainPort {
    pub domain: &'static str,
    pub project_root: PathBuf,
    pub asset_root: PathBuf,
    pub call: Arc<dyn Fn(MapDomainRequest) -> Result<MapDomainSnapshot, String> + Send + Sync>,
}
fn ports() -> &'static RwLock<HashMap<PathBuf, Arc<NativeDomainPort>>> {
    static PORTS: OnceLock<RwLock<HashMap<PathBuf, Arc<NativeDomainPort>>>> = OnceLock::new();
    PORTS.get_or_init(Default::default)
}
pub fn register(port: Arc<NativeDomainPort>) -> Result<(), String> {
    let mut ports = ports().write().map_err(|_| "地图作者端口锁已失效")?;
    if ports
        .get(&port.asset_root)
        .is_some_and(|old| old.domain != port.domain)
    {
        return Err("当前地图只支持一个外部作者正文，不能同时安装两个地图领域".into());
    }
    ports.insert(port.asset_root.clone(), port);
    Ok(())
}
pub fn registered(root: &Path) -> Option<Arc<NativeDomainPort>> {
    ports().read().ok()?.get(root).cloned()
}
pub fn unregister(root: &Path, owner: &Arc<NativeDomainPort>) {
    if let Ok(mut ports) = ports().write()
        && ports.get(root).is_some_and(|port| Arc::ptr_eq(port, owner))
    {
        ports.remove(root);
    }
}

/// 完整正文和场景索引都在后台创建；主线程采用时只交换Arc，不解析领域JSON。
#[derive(Clone)]
pub struct NativeDocument {
    pub identity: MapDomainIdentity,
    pub descriptor: wire::MapDomainDocument,
    pub port: Arc<NativeDomainPort>,
    pub body: Arc<Vec<u8>>,
    pub scene: Arc<SceneIndex>,
}
pub struct SceneIndex {
    pub scene: DomainScene,
    layers: HashMap<String, usize>,
    objects: HashMap<String, usize>,
    layer_bounds: HashMap<String, (Vec3, Vec3)>,
}
impl SceneIndex {
    fn new(scene: DomainScene) -> Result<Self, String> {
        let mut index = Self {
            scene,
            layers: HashMap::new(),
            objects: HashMap::new(),
            layer_bounds: HashMap::new(),
        };
        for (i, row) in index.scene.layers.iter().enumerate() {
            validate_row(row)?;
            if row.layer.is_some() || !index.layers.insert(row.id.clone(), i).is_none() {
                return Err("地图领域图层身份重复或包含非法父层".into());
            }
        }
        for (i, row) in index.scene.objects.iter().enumerate() {
            validate_row(row)?;
            if index.objects.insert(row.id.clone(), i).is_some()
                || index.layers.contains_key(&row.id)
                || row
                    .layer
                    .as_ref()
                    .is_some_and(|layer| !index.layers.contains_key(layer))
            {
                return Err("地图领域对象身份重复或引用缺失图层".into());
            }
            if let (Some(layer), Some([min, max])) = (&row.layer, row.bounds) {
                let min = Vec3::from_array(min);
                let max = Vec3::from_array(max);
                index
                    .layer_bounds
                    .entry(layer.clone())
                    .and_modify(|bounds| {
                        bounds.0 = bounds.0.min(min);
                        bounds.1 = bounds.1.max(max);
                    })
                    .or_insert((min, max));
            }
        }
        Ok(index)
    }
    fn row(&self, kind: DomainItemKind, id: &str) -> Option<&DomainSceneRow> {
        match kind {
            DomainItemKind::Layer => self.scene.layers.get(*self.layers.get(id)?),
            DomainItemKind::Object => self.scene.objects.get(*self.objects.get(id)?),
        }
    }
    fn bounds(&self, kind: DomainItemKind, id: &str) -> Option<(Vec3, Vec3)> {
        if kind == DomainItemKind::Layer {
            return self.layer_bounds.get(id).copied();
        }
        let [min, max] = self.row(kind, id)?.bounds?;
        Some((Vec3::from_array(min), Vec3::from_array(max)))
    }
}
fn validate_row(row: &DomainSceneRow) -> Result<(), String> {
    if row.id.is_empty()
        || [&row.id, &row.name, &row.label]
            .iter()
            .any(|s| s.len() > 1024 || s.contains('\0'))
        || row.bounds.is_some_and(|[min, max]| {
            min.iter().chain(max.iter()).any(|v| !v.is_finite()) || (0..3).any(|i| min[i] > max[i])
        })
    {
        return Err("地图领域场景行或包围盒无效".into());
    }
    if let Some(binding) = &row.binding {
        serde_json::from_value::<MapTerrainBinding>(binding.clone())
            .map_err(|e| format!("地图领域地形关联无效：{e}"))?;
    }
    Ok(())
}

// 插件与宿主使用同一不可变缓存实现，宿主采用与保存事务仍留在编辑器。
pub use feathrise_plugin_api::map_domain::blob_io::{read_blob, write_blob};

pub fn validate_snapshot(
    port: Arc<NativeDomainPort>,
    snapshot: &MapDomainSnapshot,
) -> Result<MapDomainDocument, String> {
    snapshot.validate()?;
    if snapshot.ground_patch_preparation != GROUND_PATCH_PREPARATION_PROTOCOL {
        return Err("地图作者插件不支持地形跟随纯准备协议，请更新插件".into());
    }
    let identity = snapshot
        .identity
        .clone()
        .ok_or("地图领域回执缺少地图身份")?;
    if identity.domain != port.domain {
        return Err("地图领域回执归属不匹配".into());
    }
    MapId::parse(&identity.map_id)?;
    let descriptor = snapshot
        .document
        .clone()
        .ok_or("地图领域没有返回完整文档")?;
    let body = Arc::new(read_blob(&port.project_root, &descriptor.body)?);
    let scene = serde_json::from_slice(&read_blob(&port.project_root, &descriptor.scene)?)
        .map_err(|e| format!("地图领域场景格式无效：{e}"))?;
    let value = NativeDocument {
        identity,
        descriptor,
        port,
        body,
        scene: Arc::new(SceneIndex::new(scene)?),
    };
    check_guard(&value.port.asset_root, &value)?;
    Ok(document(value))
}
pub fn document(value: NativeDocument) -> MapDomainDocument {
    MapDomainDocument {
        domain: value.port.domain,
        value: Arc::new(value),
        ops: &OPS,
    }
}
pub fn load(root: &Path, map: &MapId) -> Result<MapDomainDocument, String> {
    let port = registered(root).ok_or("地图原生作者端口尚未就绪")?;
    let identity = MapDomainIdentity {
        domain: port.domain.into(),
        map_id: map.to_string(),
        generation: next_generation(),
        revision: 0,
        terrain_revision: 0,
        environment_revision: 0,
        ground_content: None,
    };
    let snapshot = (port.call)(MapDomainRequest {
        identity,
        command: MapDomainCommand::Open,
    })?;
    validate_snapshot(port, &snapshot)
}
pub fn next_generation() -> u64 {
    static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
fn typed(value: &dyn Any) -> &NativeDocument {
    value.downcast_ref().expect("原生地图领域宿主类型必须匹配")
}
fn check_guard(root: &Path, value: &NativeDocument) -> Result<DomainSaveGuard, String> {
    let descriptor = &value.descriptor;
    descriptor.validate()?;
    // 领域只可写当前地图私有目录；宿主不允许插件将字符槽改向全局配置或另一张地图。
    let catalog_path = safe_content_path(root, feathrise_map_content::MAP_CATALOG_ASSET_PATH)?;
    compare_content_hash(&descriptor.catalog_hash, &file_hash(&catalog_path)?)?;
    let catalog: feathrise_map_content::MapCatalogAsset =
        serde_json::from_slice(&fs::read(&catalog_path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let entry = catalog
        .maps
        .iter()
        .find(|entry| entry.id.as_str() == value.identity.map_id)
        .ok_or("地图领域目标地图未登记")?;
    if !descriptor
        .asset_path
        .to_lowercase()
        .starts_with(&format!("{}/", entry.folder.trim_end_matches('/')).to_lowercase())
        || entry
            .characters_path
            .as_ref()
            .is_some_and(|path| path != &descriptor.asset_path)
    {
        return Err("地图领域保存路径不属于当前地图，或目录入口已被外部改向".into());
    }
    let path = safe_content_path(root, &descriptor.asset_path)?;
    compare_content_hash(&descriptor.source_hash, &file_hash(&path)?)?;
    let mut files = vec![(path, descriptor.source_hash.clone())];
    for dependency in &descriptor.dependencies {
        let path = safe_content_path(root, &dependency.path)?;
        compare_content_hash(&dependency.hash, &file_hash(&path)?)?;
        files.push((path, dependency.hash.clone()));
    }
    Ok(DomainSaveGuard {
        files,
        catalog_hash: Some(descriptor.catalog_hash.clone()),
    })
}
static OPS: MapDomainDocumentOps = MapDomainDocumentOps {
    dirty: |value| typed(value).descriptor.dirty,
    hash: |value| &typed(value).descriptor.hash,
    asset_path: |value| &typed(value).descriptor.asset_path,
    registration_required: |value| typed(value).descriptor.registration_required,
    guard: |root, value| check_guard(root, typed(value)),
    prepare: |root, value| {
        let value = typed(value);
        if !value.descriptor.dirty {
            return Ok(None);
        }
        check_guard(root, value)?;
        Ok(Some((
            PreparedWrite::new(
                safe_content_path(root, &value.descriptor.asset_path)?,
                (*value.body).clone(),
            ),
            bytes_hash(&value.body),
        )))
    },
    register: |value, entry: &mut MapCatalogEntry| {
        let value = typed(value);
        if entry.id.as_str() != value.identity.map_id
            || entry
                .characters_path
                .as_ref()
                .is_some_and(|path| path != &value.descriptor.asset_path)
        {
            return Err("地图领域目录入口已变化".into());
        }
        entry.characters_path = Some(value.descriptor.asset_path.clone());
        Ok(())
    },
    saved: |value, hash| {
        let mut value = typed(value).clone();
        value.descriptor.source_hash = hash.into();
        value.descriptor.dirty = false;
        value.descriptor.registration_required = false;
        Arc::new(value)
    },
    follow_terrain: |value, ground| {
        let value = typed(value);
        let patches: Result<Vec<_>, String> = value
            .scene
            .scene
            .objects
            .iter()
            .filter_map(|row| row.binding.as_ref().map(|binding| (row, binding)))
            .map(|(row, binding)| {
                let binding: MapTerrainBinding =
                    serde_json::from_value(binding.clone()).map_err(|e| e.to_string())?;
                let pose = ground.bindings.resolve(&binding)?;
                Ok(DomainGroundPatch {
                    id: row.id.clone(),
                    position: pose.translation,
                    yaw_degrees: pose.yaw_degrees,
                    binding: None, // 既有跟随仅更新世界姿态，保留原源局部关联。
                })
            })
            .collect();
        let patches = patches?;
        if patches.is_empty() {
            return Ok(Arc::new(value.clone()) as DomainValue);
        }
        let blob = write_blob(
            &value.port.project_root,
            value.port.domain,
            &serde_json::to_vec(&patches).map_err(|e| e.to_string())?,
        )?;
        let mut identity = value.identity.clone();
        identity.terrain_revision = ground.revision;
        let response = (value.port.call)(MapDomainRequest {
            identity,
            // 后台跟随可能晚于新作者修订完成；只能准备文件，不能把DLL活动身份或正文退回旧版本。
            // 原ContentTerrainFollowState完整身份采用后，地图桥再通过唯一Adopt写侧同步当前版本。
            command: MapDomainCommand::PrepareGroundPatch {
                document: value.descriptor.clone(),
                patches: blob,
            },
        })?;
        Ok(validate_snapshot(value.port.clone(), &response)?.value)
    },
};

pub fn interaction(value: &NativeDocument, _generation: u64) -> Arc<MapInteractionSnapshot> {
    let rows = value.scene.clone();
    let selection = value.scene.clone();
    let bounds = value.scene.clone();
    let pick = value.scene.clone();
    Arc::new(MapInteractionSnapshot {
        map: MapId::parse(&value.identity.map_id).expect("后台已验证地图ID"),
        document_domain: value.port.domain,
        domain: value.port.domain,
        label: "地图作者",
        hash: value.descriptor.hash.clone(),
        layer_label: "作者层",
        object_label: "作者对象",
        layer_count: value.scene.scene.layers.len(),
        object_count: value.scene.scene.objects.len(),
        row: Arc::new(move |kind, index| {
            let row = match kind {
                DomainItemKind::Layer => rows.scene.layers.get(index),
                DomainItemKind::Object => rows.scene.objects.get(index),
            }?;
            Some(DomainTreeRow {
                id: row.id.clone(),
                name: row.name.clone(),
                label: row.label.clone(),
                hidden: row.hidden,
                locked: row.locked,
                actions: if kind == DomainItemKind::Layer {
                    0b11_1111
                } else {
                    0b00_1111
                },
                copy_locked: kind == DomainItemKind::Layer,
            })
        }),
        selection: Arc::new(move |kind, id| {
            let row = selection.row(kind, id)?;
            let layer = if kind == DomainItemKind::Layer {
                Some(row.id.clone())
            } else {
                row.layer.clone()
            };
            Some(DomainSelectionInfo {
                layer_index: layer
                    .as_ref()
                    .and_then(|id| selection.layers.get(id).copied()),
                layer_id: layer,
                visible: !row.hidden,
                status: format!("{}{}", row.label, if row.locked { "（锁定）" } else { "" }),
            })
        }),
        bounds: Arc::new(move |kind, id| bounds.bounds(kind, id)),
        pick: Arc::new(move |ray, ground, radius| {
            // 原统一拾取系统在后台调用；候选从同一不可变快照取得，不在主帧遍历地图。
            let mut hits: Vec<(String, String, f32)> = Vec::with_capacity(64);
            for row in &pick.scene.objects {
                if row.hidden {
                    continue;
                }
                let Some([min, max]) = row.bounds else {
                    continue;
                };
                let center = (Vec3::from_array(min) + Vec3::from_array(max)) * 0.5;
                let t = (center - ray.origin).dot(*ray.direction).max(0.);
                let distance = (ray.origin + *ray.direction * t).distance(center);
                let extent = Vec3::from_array(max).distance(Vec3::from_array(min)) * 0.5 + radius;
                if distance <= extent
                    || ground.is_some_and(|point| point.distance(center) <= extent)
                {
                    let index = hits.partition_point(|hit| hit.2.total_cmp(&t).is_le());
                    if index < 64 {
                        if hits.len() == 64 {
                            hits.pop();
                        }
                        hits.insert(index, (row.id.clone(), row.label.clone(), t));
                    }
                }
            }
            hits
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scene_index_rejects_missing_layer_and_duplicate_stable_identity() {
        let row = DomainSceneRow {
            id: "source".into(),
            name: "source".into(),
            label: "来源".into(),
            layer: Some("missing".into()),
            hidden: false,
            locked: false,
            bounds: Some([[0.; 3], [0.; 3]]),
            binding: None,
        };
        assert!(
            SceneIndex::new(DomainScene {
                layers: vec![],
                objects: vec![row.clone()]
            })
            .is_err()
        );
        let mut row = row;
        row.layer = None;
        assert!(
            SceneIndex::new(DomainScene {
                layers: vec![],
                objects: vec![row.clone(), row]
            })
            .is_err()
        );
    }
}
