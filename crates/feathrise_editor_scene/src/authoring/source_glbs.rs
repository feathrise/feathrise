//! 保存场景所引用 GLB 的源文件草稿；跨文件编辑沿用场景的单个写盘事务。

use bevy::prelude::{Mat4, Transform};
use feathrise_editor_animation_model::data::{GlbParent, GlbStructure};
use feathrise_scene_content::authoring_io::{bytes_hash, file_hash};
use feathrise_scene_content::authoring_transaction::PreparedWrite;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

mod mesh_properties;
pub(crate) use mesh_properties::{SourceMeshHeight, SourceMeshProperties};

#[derive(Clone)]
pub(crate) struct SourceGlbNode {
    pub(crate) index: usize,
    pub(crate) path: Vec<String>,
    pub(crate) mesh: Option<u32>,
    pub(crate) mesh_name: String,
    pub(crate) folder: bool,
}

#[derive(Clone)]
pub(crate) struct SourceGlbVersion {
    pub(crate) original_hash: String,
    content_hash: String,
    pub(crate) bytes: Arc<Vec<u8>>,
    pub(crate) nodes: Arc<[SourceGlbNode]>,
    node_slots: BTreeMap<usize, usize>,
    original_node_poses: Arc<BTreeMap<usize, Transform>>,
}

impl SourceGlbVersion {
    pub(crate) fn node(&self, index: usize) -> Option<&SourceGlbNode> {
        self.nodes.get(*self.node_slots.get(&index)?)
    }

    pub(crate) fn original_node_pose(&self, index: usize) -> Option<Transform> {
        self.original_node_poses.get(&index).copied()
    }
}

/// 场景切换、撤销与保存只共享完整版本，编码和任何文件访问均在后台完成。
#[derive(Clone, Default)]
pub struct SourceGlbEdits(pub(crate) Arc<BTreeMap<String, SourceGlbVersion>>);

impl SourceGlbEdits {
    pub(crate) fn verify_current(&self, other: &Self) -> Result<(), String> {
        if Arc::ptr_eq(&self.0, &other.0) {
            return Ok(());
        }
        Err("源 GLB 草稿已变化".into())
    }

    pub(crate) fn version(&self, path: &str) -> Option<&SourceGlbVersion> {
        self.0.get(path)
    }
}

pub(crate) struct SourceGlbSave<'a> {
    pub(crate) paths: Vec<String>,
    pub(crate) writes: Vec<PreparedWrite<'a>>,
    hashes: BTreeMap<PathBuf, &'a str>,
}

impl<'a> SourceGlbSave<'a> {
    pub(crate) fn prepare(root: &Path, edits: &'a SourceGlbEdits) -> Result<Self, String> {
        let mut batch = Self {
            paths: Vec::new(),
            writes: Vec::new(),
            hashes: BTreeMap::new(),
        };
        for (relative, version) in edits.0.iter() {
            let path = source_path(root, relative)?;
            verify_source_hash(&path, &version.original_hash)?;
            batch
                .hashes
                .insert(path.clone(), version.original_hash.as_str());
            batch.paths.push(relative.clone());
            batch
                .writes
                .push(PreparedWrite::shared(path, version.bytes.clone()));
        }
        Ok(batch)
    }

    pub(crate) fn check_source(&self, path: &Path) -> Result<(), String> {
        self.hashes
            .get(path)
            .map_or(Ok(()), |expected| verify_source_hash(path, expected))
    }
}

pub(crate) fn source_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = super::files::safe_asset_path(root, relative)?;
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
    {
        return Err("场景树源编辑需要项目内的 GLB 文件".into());
    }
    // 受摆放协议管理的主场景 GLB 由场景保存器生成，直接写文件会丢失摆放身份。
    if path.file_name().is_some_and(|name| {
        name.eq_ignore_ascii_case(feathrise_scene_content::SCENE_PLACEMENTS_GLB_FILE_NAME)
    }) && path.parent().is_some_and(|parent| {
        parent
            .join(feathrise_scene_content::SCENE_MANIFEST_FILE_NAME)
            .is_file()
    }) {
        return Err("主场景 GLB 由场景草稿保存，请使用主场景节点的编辑入口".into());
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("无法解析资产目录：{error}"))?;
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("无法打开源 GLB {relative}：{error}"))?;
    let actual = canonical
        .strip_prefix(&canonical_root)
        .ok()
        .map(|part| part.to_string_lossy().replace('\\', "/"));
    if actual.as_deref() != Some(relative) {
        return Err(format!("源 GLB 路径必须直达工程资产：{relative}"));
    }
    Ok(path)
}

#[derive(Clone)]
pub(crate) struct SourceNodeAddress {
    pub(crate) index: usize,
    pub(crate) path: Vec<String>,
    pub(crate) mesh: Option<u32>,
}

impl SourceNodeAddress {
    fn resolve(&self, document: &GlbStructure) -> Result<usize, String> {
        let node = document.node(self.index)?;
        if document.node_path_in_scene(0, self.index)? != self.path
            || self
                .mesh
                .is_some_and(|mesh| node.mesh != Some(mesh as usize))
        {
            return Err("源 GLB 节点已变化，请重新选择".into());
        }
        Ok(self.index)
    }
}

#[derive(Clone)]
pub(crate) struct SourceGlbTarget {
    pub(crate) path: String,
    pub(crate) node: Option<SourceNodeAddress>,
    pub(crate) world: Mat4,
}

impl SourceGlbTarget {
    fn parent(&self, document: &GlbStructure) -> Result<GlbParent, String> {
        if let Some(node) = &self.node {
            return Ok(GlbParent::Node(node.resolve(document)?));
        }
        // 场景中的完整 GLB 摆放使用 Scene0，和文件预览的默认选择分别处理。
        document.children(GlbParent::Scene(0))?;
        Ok(GlbParent::Scene(0))
    }

    fn node(&self, document: &GlbStructure) -> Result<usize, String> {
        self.node
            .as_ref()
            .ok_or("请选中 GLB 内的节点")?
            .resolve(document)
    }
}

pub(crate) fn read_node(
    root: &Path,
    target: &SourceGlbTarget,
    draft: &SourceGlbEdits,
) -> Result<(Transform, SourceMeshProperties, String), String> {
    let path = source_path(root, &target.path)?;
    let (document, content_hash) = if let Some(staged) = draft.version(&target.path) {
        (GlbStructure::from_bytes(&staged.bytes)?, staged.content_hash.clone())
    } else {
        let bytes = std::fs::read(&path).map_err(|error| format!("读取源 GLB 失败：{error}"))?;
        (GlbStructure::from_bytes(&bytes)?, bytes_hash(&bytes))
    };
    let node = target.node(&document)?;
    Ok((document.node_transform(node)?, SourceMeshProperties::read(&document, node)?, content_hash))
}

pub(crate) enum SourceGlbOperation {
    Rename(String),
    Folder(String),
    Move(SourceGlbTarget),
    NodeEdit {
        expected_hash: String,
        pose: Option<Transform>,
        properties: Option<SourceMeshProperties>,
    },
}

pub(crate) fn edit(
    root: &Path,
    before: &SourceGlbEdits,
    source: SourceGlbTarget,
    operation: SourceGlbOperation,
) -> Result<Option<(SourceGlbEdits, usize)>, String> {
    let load = |relative: &str| {
        let path = source_path(root, relative)?;
        if let Some(previous) = before.version(relative) {
            verify_source_hash(&path, &previous.original_hash)?;
            return Ok((
                GlbStructure::from_bytes(&previous.bytes)?,
                previous.original_hash.clone(),
                previous.content_hash.clone(),
                previous.bytes.clone(),
            ));
        }
        let bytes =
            std::fs::read(&path).map_err(|error| format!("读取源 GLB {relative} 失败：{error}"))?;
        let hash = bytes_hash(&bytes);
        Ok::<_, String>((GlbStructure::from_bytes(&bytes)?, hash.clone(), hash, Arc::new(bytes)))
    };
    let (mut document, source_hash, content_hash, source_bytes) = load(&source.path)?;
    let original_poses = |path: &str| before.version(path)
        .map_or_else(Arc::default, |version| version.original_node_poses.clone());
    let mut source_poses = original_poses(&source.path);
    let mut next = before.0.as_ref().clone();
    let mut changed_bytes = 0usize;
    let mut store = |path: String,
                     document: GlbStructure,
                     original_hash: String,
                     base: Arc<Vec<u8>>,
                     original_node_poses: Arc<BTreeMap<usize, Transform>>|
     -> Result<(), String> {
        let bytes = document.encode()?;
        if bytes != *base {
            changed_bytes = changed_bytes.saturating_add(bytes.capacity());
            let nodes = outline(&document)?;
            let node_slots = nodes
                .iter()
                .enumerate()
                .map(|(slot, node)| (node.index, slot))
                .collect();
            next.insert(
                path,
                SourceGlbVersion {
                    original_hash,
                    content_hash: bytes_hash(&bytes),
                    bytes: Arc::new(bytes),
                    nodes,
                    node_slots,
                    original_node_poses,
                },
            );
        }
        Ok(())
    };
    match operation {
        SourceGlbOperation::Rename(name) => {
            let node = source.node(&document)?;
            if document.node(node)?.name == name {
                return Ok(None);
            }
            document.rename_node(node, &name)?;
        }
        SourceGlbOperation::Folder(name) => {
            document.create_folder(source.parent(&document)?, &name)?;
        }
        SourceGlbOperation::NodeEdit { expected_hash, pose, properties } => {
            // 数值与模式来自完整节点快照。版本冲突时保留外部或较新的草稿，等面板重读后重试。
            if expected_hash != content_hash {
                return Err("源 GLB 在读取属性后已变化，请使用更新后的属性重新编辑".into());
            }
            let node = source.node(&document)?;
            let mut changed = false;
            if let Some(pose) = pose && let previous = document.node_transform(node)? && previous != pose {
                document.set_node_transform(node, pose)?;
                // 只为真正写过 TRS 的节点保留源姿势，视口用这个差量叠加已经算出的贴地姿态。
                let poses = Arc::make_mut(&mut source_poses);
                if *poses.entry(node).or_insert(previous) == pose {
                    poses.remove(&node);
                }
                changed = true;
            }
            if let Some(properties) = properties {
                if document.node(node)?.mesh.is_none() {
                    return Err("请选择源 GLB 内的网格".into());
                }
                if SourceMeshProperties::read(&document, node)? != properties {
                    properties.write(&mut document, node)?;
                    changed = true;
                }
            }
            if !changed {
                return Ok(None);
            }
        }
        SourceGlbOperation::Move(destination) => {
            let node = source.node(&document)?;
            if source.path == destination.path {
                let parent = destination.parent(&document)?;
                if document
                    .children(parent)?
                    .iter()
                    .any(|child| child.index == node)
                {
                    return Ok(None);
                }
                document.move_node(node, parent)?;
            } else {
                let (mut target, hash, _, bytes) = load(&destination.path)?;
                let parent = destination.parent(&target)?;
                let basis = destination.world.inverse() * source.world;
                if !basis.is_finite() {
                    return Err("目标 GLB 的场景变换不可逆".into());
                }
                document.transfer_node_to_in_space(node, &mut target, parent, basis)?;
                let target_poses = original_poses(&destination.path);
                store(destination.path, target, hash, bytes, target_poses)?;
            }
        }
    }
    store(source.path, document, source_hash, source_bytes, source_poses)?;
    Ok((changed_bytes > 0).then(|| (SourceGlbEdits(Arc::new(next)), changed_bytes)))
}

fn outline(document: &GlbStructure) -> Result<Arc<[SourceGlbNode]>, String> {
    let mut result = Vec::new();
    let mut pending = document
        .children(GlbParent::Scene(0))?
        .into_iter()
        .rev()
        .map(|node| (node, Vec::new()))
        .collect::<Vec<_>>();
    let mut seen = std::collections::HashSet::new();
    while let Some((node, mut path)) = pending.pop() {
        if !seen.insert(node.index) {
            return Err("源 GLB 树包含重复或循环节点".into());
        }
        path.push(node.name);
        let mesh = node
            .mesh
            .map(u32::try_from)
            .transpose()
            .map_err(|_| "GLB 网格索引越界")?;
        if node.folder || mesh.is_some() {
            let mesh_name = node
                .mesh
                .map(|index| document.mesh_name(index))
                .transpose()?
                .flatten()
                .unwrap_or_default()
                .to_owned();
            result.push(SourceGlbNode {
                index: node.index,
                path: path.clone(),
                mesh,
                mesh_name,
                folder: node.folder,
            });
        }
        for child in node.children.into_iter().rev() {
            pending.push((document.node(child)?, path.clone()));
        }
    }
    Ok(result.into())
}

pub(crate) fn verify_source_hash(path: &Path, expected: &str) -> Result<(), String> {
    if file_hash(path)? != expected {
        return Err(format!(
            "源 GLB 已在外部修改，请重开后再编辑：{}",
            path.display()
        ));
    }
    Ok(())
}
