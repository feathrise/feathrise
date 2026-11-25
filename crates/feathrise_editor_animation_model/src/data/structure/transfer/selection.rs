//! 所选资源另存时求依赖闭包。祖先与动作目标仅携带骨架变换，选择对象才携带几何和蒙皮。

use super::*;
use crate::data::{AnimationDocument, ContentItem, ContentKind};
use std::collections::BTreeMap;
use std::path::Path;

pub(super) fn read_image(
    root: &Path,
    source: &Path,
    uri: &str,
) -> Result<(Vec<u8>, &'static str), String> {
    let uri = feathrise_editor_assets::files::decode_gltf_uri(uri)?;
    if uri.contains(['\\', ':', '\0']) || uri.starts_with('/') {
        return Err("模型贴图需要工程内的相对 URI".into());
    }
    let file = source
        .parent()
        .ok_or("模型路径无效")?
        .join(&uri)
        .canonicalize()
        .map_err(|e| format!("无法读取贴图 {uri}：{e}"))?;
    if !file.starts_with(root) {
        return Err(format!("贴图超出当前工程资产根：{uri}"));
    }
    let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
    let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if bytes.starts_with(b"\xabKTX 20\xbb\r\n\x1a\n") {
        "image/ktx2"
    } else {
        return Err(format!("贴图格式无法内嵌：{uri}"));
    };
    Ok((bytes, mime))
}

impl AnimationDocument {
    /// 输出从当前草稿编码，同一份文件的未保存片段和曲线随所选资源一起导出。
    pub fn encode_selection(&self, selection: &[ContentItem]) -> Result<Vec<u8>, String> {
        self.encode_selection_in(selection, None)
    }

    pub fn encode_selection_in(
        &self,
        selection: &[ContentItem],
        root: Option<&Path>,
    ) -> Result<Vec<u8>, String> {
        if self.read_only || selection.is_empty() {
            return Err("请在独立 GLB 中选择要另存的内容".into());
        }
        let glb = GlbStructure::from_bytes(&self.encode()?)?;
        if glb
            .chunks
            .iter()
            .any(|chunk| !matches!(chunk.kind, JSON_CHUNK | BIN_CHUNK))
        {
            return Err("该 GLB 含有未支持的私有数据块，无法安全裁剪所选资源".into());
        }
        let mut full_nodes = HashSet::new();
        let mut nodes = BTreeSet::new();
        let mut skins = BTreeSet::new();
        let mut animations = BTreeSet::new();
        let mut assets = BTreeSet::new();
        let file_clips = self.clips_in_file_order();
        for item in selection {
            match item.kind {
                ContentKind::Node => full_nodes.extend(subtree_indices(&glb.json, item.id)?),
                ContentKind::Skin => {
                    skins.insert(item.id);
                }
                ContentKind::Animation => {
                    let pos = file_clips
                        .binary_search_by_key(&item.id, |c| c.source_index)
                        .map_err(|_| "所选动画已改变")?;
                    animations.insert(pos);
                    for channel in list(object_at(&glb.json, "animations", pos)?, "channels")? {
                        let target = index_field(&channel["target"], "node")?;
                        nodes.insert(target);
                        // 形态键的播放依赖原网格及 morph targets，不能只输出一副空骨架。
                        if channel["target"]["path"] == "weights" {
                            full_nodes.insert(target);
                        }
                    }
                }
                kind => {
                    assets.insert((kind.key(), item.id));
                }
            }
        }
        for &node in &full_nodes {
            nodes.insert(node);
            if let Some(skin) = object_at(&glb.json, "nodes", node)?.get("skin") {
                skins.insert(index_value(skin)?);
            }
        }
        for &index in &skins {
            let skin = object_at(&glb.json, "skins", index)?;
            nodes.extend(indices(skin, "joints")?);
            if let Some(root) = skin.get("skeleton") {
                nodes.insert(index_value(root)?);
            }
        }
        // 汇集所有必要祖先，每个节点最多加入一次；同名节点依然按源索引区分。
        for node in nodes.clone() {
            let mut parent = self
                .nodes
                .get(node)
                .ok_or("所选内容引用了不存在的节点")?
                .parent;
            while let Some(index) = parent {
                if !nodes.insert(index) {
                    break;
                }
                parent = self.nodes[index].parent;
            }
        }
        let mapped: HashMap<_, _> = nodes
            .iter()
            .copied()
            .enumerate()
            .map(|(to, from)| (from, to))
            .collect();
        let mut json = json!({ "asset": glb.json["asset"] });
        let mut binary = Vec::new();
        let mut copier = Copier {
            source: &glb.json,
            binary: &glb.binary,
            draft: &mut json,
            output: &mut binary,
            indices: HashMap::new(),
            extensions: BTreeSet::new(),
            external_images: root.map(|r| (r, self.source_path())),
        };
        for (kind, id) in assets {
            copier.copy(kind, id)?;
        }
        let mut skin_map = BTreeMap::new();
        for (to, from) in skins.into_iter().enumerate() {
            let mut skin = object_at(&glb.json, "skins", from)?.clone();
            copier.check_extensions(&skin, &[])?;
            skin["joints"] = json!(
                indices(&skin, "joints")?
                    .into_iter()
                    .map(|i| mapped[&i])
                    .collect::<Vec<_>>()
            );
            if let Some(root) = skin.get_mut("skeleton") {
                *root = json!(mapped[&index_value(root)?]);
            }
            copier.remap_field(&mut skin, "inverseBindMatrices", "accessors")?;
            push(copier.draft, "skins", skin);
            skin_map.insert(from, to);
        }
        let mut roots = Vec::new();
        for from in nodes {
            let mut node = object_at(&glb.json, "nodes", from)?.clone();
            // 动作目标和必要祖先也依赖原变换，未知节点扩展不能在裁剪属性时静默丢掉。
            copier.check_extensions(&node, &[])?;
            if full_nodes.contains(&from) {
                copier.remap_field(&mut node, "mesh", "meshes")?;
                copier.remap_field(&mut node, "camera", "cameras")?;
                if let Some(skin) = node.get_mut("skin") {
                    *skin = json!(skin_map[&index_value(skin)?]);
                }
            } else {
                node.as_object_mut().ok_or("节点无效")?.retain(|key, _| {
                    matches!(
                        key.as_str(),
                        "name" | "translation" | "rotation" | "scale" | "matrix" | "children"
                    )
                });
            }
            let children: Vec<_> = indices(&node, "children")?
                .iter()
                .filter_map(|id| mapped.get(id))
                .collect();
            if children.is_empty() {
                node.as_object_mut().unwrap().remove("children");
            } else {
                node["children"] = json!(children);
            }
            if self.nodes[from].parent.is_none() {
                roots.push(mapped[&from]);
            }
            push(copier.draft, "nodes", node);
        }
        if !roots.is_empty() {
            copier.draft["scenes"] = json!([{"nodes": roots}]);
            copier.draft["scene"] = json!(0);
        }
        for pos in animations {
            let animation = object_at(&glb.json, "animations", pos)?;
            copier.copy_animations(std::slice::from_ref(animation), &mapped, usize::MAX, None)?;
        }
        copier.publish_extensions()?;
        if !binary.is_empty() {
            json["buffers"] = json!([{"byteLength": binary.len()}]);
        }
        // 新文件仅包含已重新编号的标准块，旧的私有二进制块不能带入新的编号空间。
        let chunks = [Chunk {
            kind: JSON_CHUNK,
            bytes: Arc::default(),
        }];
        encode_chunks(&chunks, &json, &binary)
    }
}
