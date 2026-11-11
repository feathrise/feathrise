//! 在后台只读 GLB 首个 JSON 块，展开真实场景、节点和蒙皮关系。

use serde::{Deserialize, de::IgnoredAny};
use std::{
    fs::File,
    io::Read,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

pub(in super::super) const MAX_JSON: usize = 16 * 1024 * 1024;
const MAX_ROWS: usize = 65_536;
const MAX_DEPTH: usize = 128;

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Scene,
    Node,
    Joint,
    Mesh(usize),
    Skin(usize),
    LooseNodes,
    LooseAssets,
}

pub(super) struct Row {
    pub kind: Kind,
    pub index: usize,
    pub name: String,
    pub depth: usize,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Document {
    scenes: Vec<Scene>,
    nodes: Vec<Node>,
    meshes: Vec<Mesh>,
    skins: Vec<Skin>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Scene {
    name: String,
    nodes: Vec<usize>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Node {
    name: String,
    children: Vec<usize>,
    mesh: Option<usize>,
    skin: Option<usize>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Mesh {
    name: String,
    primitives: Vec<IgnoredAny>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Skin {
    name: String,
    joints: Vec<usize>,
}

fn active(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        return Err("已取消 GLB 结构读取".into());
    }
    Ok(())
}

pub(super) fn read_json(
    root: &Path,
    relative: &str,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    active(cancel)?;
    read_json_file(&mut open_asset(root, relative)?, cancel)
}

/// 树和右侧属性使用同一只读路径门，文件符号链接也须留在工程资产根内。
pub(in super::super) fn open_asset(root: &Path, relative: &str) -> Result<File, String> {
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let path = feathrise_editor_assets::files::safe_asset_path(&root, relative)?
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err("文件不属于当前工程资产目录".into());
    }
    File::open(path).map_err(|error| error.to_string())
}

pub(in super::super) fn read_json_file(file: &mut File, cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    active(cancel)?;
    let before = file.metadata().map_err(|error| error.to_string())?;
    let mut header = [0; 20];
    file.read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    let word = |at| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
    let length = word(12) as usize;
    if &header[..4] != b"glTF"
        || word(4) != 2
        || u64::from(word(8)) != before.len()
        || &header[16..] != b"JSON"
        || length == 0
        || length > MAX_JSON
        || 20 + length as u64 > before.len()
    {
        return Err("GLB 2.0 元数据无效或超过 16 MiB 读取预算".into());
    }
    // 只读目录块，网格、贴图和动画二进制仍由原资源加载器持有。
    let mut bytes = vec![0; length];
    for chunk in bytes.chunks_mut(64 * 1024) {
        active(cancel)?;
        file.read_exact(chunk).map_err(|error| error.to_string())?;
    }
    let after = file.metadata().map_err(|error| error.to_string())?;
    if after.len() != before.len() || after.modified().ok() != before.modified().ok() {
        return Err("GLB 已改变，等待预览重新加载".into());
    }
    Ok(bytes)
}

struct Build<'a> {
    document: Document,
    rows: Vec<Row>,
    parented: Vec<bool>,
    joints: Vec<bool>,
    visited: Vec<usize>,
    meshes: Vec<bool>,
    skins: Vec<bool>,
    cancel: &'a AtomicBool,
}

impl Build<'_> {
    fn push(
        rows: &mut Vec<Row>,
        kind: Kind,
        index: usize,
        name: &str,
        depth: usize,
    ) -> Result<(), String> {
        if rows.len() == MAX_ROWS || depth > MAX_DEPTH {
            return Err("GLB 结构超过 65536 行或 128 层的查看预算".into());
        }
        // 名称保持原有 Unicode。去掉控制符并限制显示长度，防止单行占满侧栏布局。
        let name = name
            .chars()
            .filter(|character| !character.is_control())
            .take(192)
            .collect();
        rows.push(Row {
            kind,
            index,
            name,
            depth,
        });
        Ok(())
    }

    fn walk(&mut self, roots: Vec<usize>, depth: usize, epoch: usize) -> Result<(), String> {
        let mut pending: Vec<_> = roots
            .into_iter()
            .rev()
            .map(|index| (index, depth))
            .collect();
        while let Some((index, depth)) = pending.pop() {
            active(self.cancel)?;
            let node = self
                .document
                .nodes
                .get(index)
                .ok_or("GLB 场景引用了无效节点")?;
            if self.visited[index] == epoch {
                return Err("GLB 场景包含循环或重复的节点引用".into());
            }
            self.visited[index] = epoch;
            let kind = if self.joints[index] {
                Kind::Joint
            } else {
                Kind::Node
            };
            Self::push(&mut self.rows, kind, index, &node.name, depth)?;
            if let Some(mesh) = node.mesh {
                let item = self
                    .document
                    .meshes
                    .get(mesh)
                    .ok_or("GLB 节点引用了无效网格")?;
                Self::push(
                    &mut self.rows,
                    Kind::Mesh(item.primitives.len()),
                    mesh,
                    &item.name,
                    depth + 1,
                )?;
                self.meshes[mesh] = true;
            }
            if let Some(skin) = node.skin {
                let item = self
                    .document
                    .skins
                    .get(skin)
                    .ok_or("GLB 节点引用了无效蒙皮")?;
                Self::push(
                    &mut self.rows,
                    Kind::Skin(item.joints.len()),
                    skin,
                    &item.name,
                    depth + 1,
                )?;
                self.skins[skin] = true;
            }
            pending.extend(node.children.iter().rev().map(|&child| (child, depth + 1)));
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<Row>, String> {
        for (index, node) in self.document.nodes.iter().enumerate() {
            active(self.cancel)?;
            for &child in &node.children {
                let flag = self
                    .parented
                    .get_mut(child)
                    .ok_or("GLB 包含无效子节点引用")?;
                if child == index || std::mem::replace(flag, true) {
                    return Err("GLB 节点的父级关系无效".into());
                }
            }
        }
        for skin in &self.document.skins {
            active(self.cancel)?;
            for &joint in &skin.joints {
                *self.joints.get_mut(joint).ok_or("GLB 蒙皮引用了无效关节")? = true;
            }
        }
        let scene_count = self.document.scenes.len();
        for index in 0..scene_count {
            active(self.cancel)?;
            let scene = &mut self.document.scenes[index];
            Self::push(&mut self.rows, Kind::Scene, index, &scene.name, 0)?;
            let roots = std::mem::take(&mut scene.nodes);
            self.walk(roots, 1, index + 1)?;
        }
        let loose: Vec<_> = self
            .parented
            .iter()
            .enumerate()
            .filter_map(|(index, &parented)| {
                (!parented && self.visited[index] == 0).then_some(index)
            })
            .collect();
        if !loose.is_empty() {
            Self::push(&mut self.rows, Kind::LooseNodes, 0, "", 0)?;
            self.walk(loose, 1, scene_count + 1)?;
        }
        if self.visited.contains(&0) {
            return Err("GLB 包含无法从父级访问的节点".into());
        }
        if self.meshes.contains(&false) || self.skins.contains(&false) {
            Self::push(&mut self.rows, Kind::LooseAssets, 0, "", 0)?;
            for (index, item) in self.document.meshes.iter().enumerate() {
                active(self.cancel)?;
                if !self.meshes[index] {
                    Self::push(
                        &mut self.rows,
                        Kind::Mesh(item.primitives.len()),
                        index,
                        &item.name,
                        1,
                    )?;
                }
            }
            for (index, item) in self.document.skins.iter().enumerate() {
                active(self.cancel)?;
                if !self.skins[index] {
                    Self::push(
                        &mut self.rows,
                        Kind::Skin(item.joints.len()),
                        index,
                        &item.name,
                        1,
                    )?;
                }
            }
        }
        Ok(self.rows)
    }
}

pub(super) fn decode(bytes: &[u8], cancel: &AtomicBool) -> Result<Vec<Row>, String> {
    active(cancel)?;
    let document: Document =
        serde_json::from_slice(bytes).map_err(|error| format!("GLB 结构解析失败：{error}"))?;
    let nodes = document.nodes.len();
    if [
        nodes,
        document.scenes.len(),
        document.meshes.len(),
        document.skins.len(),
    ]
    .into_iter()
    .any(|count| count > MAX_ROWS)
    {
        return Err("GLB 结构超过查看预算".into());
    }
    Build {
        rows: Vec::new(),
        parented: vec![false; nodes],
        joints: vec![false; nodes],
        visited: vec![0; nodes],
        meshes: vec![false; document.meshes.len()],
        skins: vec![false; document.skins.len()],
        document,
        cancel,
    }
    .finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_real_scene_nodes_meshes_and_unattached_skin() {
        let rows = decode(
            br#"{
            "scenes":[{"name":"Main","nodes":[1]}],
            "nodes":[{"name":"Joint"}, {"name":"Root","children":[0],"mesh":0}],
            "meshes":[{"name":"Body","primitives":[{"attributes":{"POSITION":0}}]}],
            "skins":[{"joints":[0]}]
        }"#,
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(rows.len(), 6);
        assert_eq!(
            rows.iter().map(|row| row.depth).collect::<Vec<_>>(),
            [0, 1, 2, 2, 0, 1]
        );
        assert!(matches!(rows[0].kind, Kind::Scene));
        assert_eq!(rows[0].name, "Main");
        assert_eq!(rows[1].name, "Root");
        assert!(matches!(rows[2].kind, Kind::Mesh(1)));
        assert!(matches!(rows[3].kind, Kind::Joint));
        assert!(matches!(rows[5].kind, Kind::Skin(1)));
    }

    #[test]
    fn rejects_bad_indices_and_cycles_and_honors_cancellation() {
        for bytes in [
            br#"{"scenes":[{"nodes":[1]}],"nodes":[{}]}"#.as_slice(),
            br#"{"nodes":[{"children":[1]},{"children":[0]}]}"#.as_slice(),
        ] {
            assert!(decode(bytes, &AtomicBool::new(false)).is_err());
        }
        assert!(decode(b"{}", &AtomicBool::new(true)).is_err());
    }
}
