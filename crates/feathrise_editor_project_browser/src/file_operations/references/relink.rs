//! 缺失资产的替代文件身份、子资源检查与正式引用事务。不移动物理文件。

use super::{files, inspect, publish};
use super::super::Control;
use feathrise_editor_project_browser_model::file_operations::*;
use feathrise_scene_content::{ContentAuthorLock, authoring_io::file_hash};
use serde_json::Value;
use std::{collections::{BTreeSet, HashSet}, fs, path::{Path, PathBuf}, sync::Arc};

pub(super) fn execute(request: &FileOperationRequest, root: &Path, roots: &[PathBuf], update: &ReferenceUpdate,
    control: &Control<'_>, receipt: &mut FileOperationReceipt) -> Result<(), String> {
    let FileOperation::Relink { missing, target, stamp } = &request.operation else { unreachable!() };
    let root = files::checked(root, roots, true)?;
    let _author_lock = ContentAuthorLock::acquire(&root)?;
    let (source, target) = replacement(&root, missing, target, *stamp)?;
    let mut preview = inspect(root, &request.operation, [(source, target.clone())].into(), control)?;
    preview.replacement_digest = Some(file_hash(&target)?);
    if preview.documents.is_empty() { return Err("未找到仍使用该缺失路径的场景或预设，请重新检查".into()); }
    match update {
        ReferenceUpdate::Inspect(_) => { receipt.reference_preview = Some(Arc::new(preview)); }
        ReferenceUpdate::Confirm(old) if old.moves == preview.moves && old.documents == preview.documents
            && old.replacement_digest == preview.replacement_digest => {
            let checked_digest = std::cell::Cell::new(false);
            let paths = publish(&preview, &preview.moves, &|| {
                control.check()?;
                replacement(&preview.root, missing, &target, *stamp)?;
                // 完成文档暂存后再次核对替代文件字节。后续每次发布仍检查源缺失及目标身份。
                if !checked_digest.replace(true) && Some(file_hash(&target)?) != preview.replacement_digest {
                    return Err("替代文件在写入前被修改，请重新选择".into());
                }
                Ok(())
            })?;
            receipt.repaired_documents = paths.len();
            receipt.repaired_paths = paths.into();
        }
        _ => return Err("缺失路径、替代文件或引用文档已变化，请重新检查并预览".into()),
    }
    Ok(())
}

/// 缺失允许任一级父目录不存在。逐级检查已经存在的部分，不能通过目录联接或无权访问推断缺失。
fn replacement(root: &Path, missing: &str, target: &Path, stamp: FileEntryStamp) -> Result<(PathBuf, PathBuf), String> {
    if missing.is_empty() || missing.contains(['\\', '#', ':']) || missing.split('/').any(|p| p.is_empty() || p == "." || p == "..") {
        return Err("缺失来源必须是工程内的资产相对路径".into());
    }
    let source = root.join(missing);
    let mut part = root.to_owned();
    for name in missing.split('/') {
        part.push(name);
        match fs::symlink_metadata(&part) {
            Ok(meta) => { files::reject_link(&part, &meta)?; if !meta.is_dir() || part == source { return Err(format!("旧资产路径已存在或父目录不可用：{}", part.display())); } }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.to_string()),
        }
    }
    let (target, metadata) = files::checked_entry(target, &[root.to_owned()])?;
    files::verify_source_stamp(&target, stamp, &metadata)?;
    if !metadata.is_file() || !target.strip_prefix(root).ok().and_then(Path::to_str).is_some_and(|s| !s.contains('#')) {
        return Err("替代资产必须是工程资产根内可表示为引用的普通文件".into());
    }
    if source.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb")) && !target.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb")) {
        return Err("请选择另一个 GLB 作为缺失模型的替代文件".into());
    }
    Ok((source, target))
}

pub(super) fn check_target(target: &Path, documents: &[ReferencingDocument], labels: &BTreeSet<String>) -> Result<(), String> {
    if documents.iter().any(|doc| doc.path == target) { return Err("替代文件本身包含旧路径，重新关联将产生自身引用".into()); }
    if !labels.is_empty() && target.extension().is_some_and(|e| e.eq_ignore_ascii_case("glb")) {
        // 正式引用事务在后台读取 GLB 元数据。浏览列表的摘要预算不能拦截可加载的场景模型。
        let gltf = {
            let (json, _, _) = feathrise_editor_assets::glb_metadata::json_prefix(target, u32::MAX.into(), None)?;
            serde_json::from_slice(&json).map_err(|e| format!("无法读取替代 GLB 元数据：{e}"))?
        };
        let missing = |label: &str| format!("替代 GLB 缺少引用的子资源 #{label}，请选择包含该子资源的文件");
        let mut inverted = Vec::new();
        for label in labels {
            if !exists(&gltf, label) { return Err(missing(label)); }
            let material = label.strip_suffix("/std").unwrap_or(label);
            if let Some(i) = material.strip_suffix(" (inverted)").and_then(|s| index(s, "Material")) {
                inverted.push((label, i));
            }
        }
        if !inverted.is_empty() {
            let used = inverted_materials(gltf)?;
            if let Some((label, _)) = inverted.iter().find(|(_, i)| !used.contains(i)) { return Err(missing(label)); }
        }
    }
    Ok(())
}

// Bevy 在实例化每个场景的节点时才生成倒置材质。累计奇数个负缩放轴，且网格实际用了该材质才有标签。
fn inverted_materials(json: Value) -> Result<HashSet<usize>, String> {
    use bevy::{gltf::gltf, math::{Mat4, Vec3}};
    let root = serde_json::from_value(json).map_err(|e| format!("替代 GLB 的节点元数据无效：{e}"))?;
    let document = gltf::Document::from_json(root).map_err(|e| format!("无法核对替代 GLB 的倒置材质：{e}"))?;
    let mut pending: Vec<_> = document.scenes().flat_map(|s| s.nodes().map(|n| (n, false))).collect();
    let mut visited = HashSet::new();
    let mut meshes = HashSet::new();
    let mut used = HashSet::new();
    while let Some((node, parent_inverted)) = pending.pop() {
        let local_inverted = match node.transform() {
            gltf::scene::Transform::Decomposed { scale, .. } => Vec3::from_array(scale).is_negative_bitmask().count_ones() % 2 == 1,
            gltf::scene::Transform::Matrix { matrix } => Mat4::from_cols_array_2d(&matrix).determinant().is_sign_negative(),
        };
        let inverted = parent_inverted ^ local_inverted;
        if !visited.insert((node.index(), inverted)) { continue; }
        if inverted && let Some(mesh) = node.mesh() && meshes.insert(mesh.index()) {
            used.extend(mesh.primitives().filter_map(|p| p.material().index()));
        }
        pending.extend(node.children().map(|child| (child, inverted)));
    }
    Ok(used)
}

fn index(label: &str, prefix: &str) -> Option<usize> {
    let value = label.strip_prefix(prefix)?;
    let index: usize = value.parse().ok()?;
    (index.to_string() == value).then_some(index)
}

fn exists(document: &Value, label: &str) -> bool {
    let at = |key: &str, index: usize| document.get(key)?.as_array()?.get(index);
    // 工程预设引用的是实际 StandardMaterial 子资产（/std），索引仍来自同一份 glTF 材质表。
    let material = label.strip_suffix("/std").unwrap_or(label);
    if material == "DefaultMaterial" { return true; }
    if let Some(i) = index(material.strip_suffix(" (inverted)").unwrap_or(material), "Material") {
        return at("materials", i).is_some();
    }
    if let Some((mesh, primitive)) = label.split_once("/Primitive") {
        return index(mesh, "Mesh").and_then(|i| at("meshes", i)).and_then(|mesh| mesh.get("primitives"))
            .and_then(|primitives| primitives.get(index(primitive, "")?)).is_some();
    }
    if let Some(skin) = label.strip_suffix("/InverseBindMatrices") {
        return index(skin, "Skin").and_then(|i| at("skins", i)).is_some();
    }
    for (prefix, key) in [("Scene","scenes"), ("Mesh","meshes"), ("Node","nodes"), ("Texture","textures"),
        ("Skin","skins"), ("Animation","animations")] {
        if let Some(i) = index(label, prefix) { return at(key, i).is_some(); }
    }
    false
}
