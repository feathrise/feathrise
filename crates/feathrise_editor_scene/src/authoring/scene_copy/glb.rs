//! 主场景 GLB 只重绑标识和引用，动作、网格及未知二进制块沿用原始字节。

use super::{
    check_cancelled,
    content::{CopyPaths, Documents},
    filesystem::{self, CopyEntry},
};
use feathrise_editor_assets::{files::decode_gltf_uri, glb_metadata};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs::File, io::{Read, Seek, SeekFrom, Write}, sync::atomic::AtomicBool};

const GEOMETRY_HASH: &str = "feathrise_geometry_hash";

pub(super) fn copy(
    documents: &Documents,
    entry: &CopyEntry,
    output: &mut File,
    scene: bool,
    cancelled: &AtomicBool,
) -> Result<(), String> {
    let path = documents.paths.assets.join(&entry.source);
    let (json, mut input, tail) = match glb_metadata::json_prefix(&path, u64::from(u32::MAX), None) {
        Ok(chunks) => chunks,
        // 普通私有资源可以是尚未完成的素材；只有场景权威 GLB 必须能解析后才登记。
        Err(_) if !scene => return filesystem::stream(File::open(path).map_err(|e| e.to_string())?, output, entry.length, cancelled),
        Err(error) => return Err(error),
    };
    let mut document: Value = match serde_json::from_slice(&json) {
        Ok(document) => document,
        Err(_) if !scene => {
            input.rewind().map_err(|error| error.to_string())?;
            return filesystem::stream(input, output, entry.length, cancelled);
        }
        Err(error) => return Err(error.to_string()),
    };
    let marker = if scene { Some(rebind_scene_root(documents, &mut document)?) } else { None };
    let changed_uri = rewrite_uris(documents.paths, entry, &mut document);
    if !scene && !changed_uri {
        input.rewind().map_err(|error| error.to_string())?;
        return filesystem::stream(input, output, entry.length, cancelled);
    }
    if let Some(index) = marker.filter(|_| changed_uri)
        && let Some(expected) = document["nodes"][index]["extras"][GEOMETRY_HASH].as_str() {
        // 新素材相对路径也参与参数化网格摘要。只有原本匹配的配方才随合法路径调整移动摘要。
        let previous = serde_json::from_slice(&json).map_err(|error| error.to_string())?;
        if let Some(next) = rebound_geometry_hash(&mut input, tail, cancelled, geometry_digest(&previous), geometry_digest(&document), expected)? {
            document["nodes"][index]["extras"][GEOMETRY_HASH] = Value::String(next);
        }
    }
    let json = serde_json::to_vec(&document).map_err(|error| error.to_string())?;
    let padded = json.len().next_multiple_of(4);
    let size = u32::try_from(20 + padded as u64 + tail).map_err(|_| "副本超出 GLB v2 长度范围")?;
    let mut write = || -> std::io::Result<()> {
        output.write_all(b"glTF")?;
        output.write_all(&2u32.to_le_bytes())?;
        output.write_all(&size.to_le_bytes())?;
        output.write_all(&(padded as u32).to_le_bytes())?;
        output.write_all(b"JSON")?;
        output.write_all(&json)?;
        output.write_all(&b"   "[..padded - json.len()])?;
        Ok(())
    };
    write().map_err(|error| error.to_string())?;
    filesystem::stream(input, output, tail, cancelled)
}

fn rebind_scene_root(documents: &Documents, document: &mut Value) -> Result<usize, String> {
    let scene = document.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize;
    let roots = document["scenes"][scene]["nodes"].as_array().ok_or("场景 GLB 缺少默认根节点")?;
    let mut markers = roots.iter().filter_map(|node| node.as_u64().map(|i| i as usize))
        .filter(|index| document["nodes"][*index]["extras"].get("feathrise_scene_id").is_some());
    let index = markers.next().ok_or("场景 GLB 缺少归属根节点")?;
    if markers.next().is_some() { return Err("场景 GLB 含有多个归属根节点".into()); }
    let marker = &mut document["nodes"][index]["extras"];
    if marker["feathrise_scene_version"].as_u64() != Some(u64::from(feathrise_scene_content::SCENE_SCHEMA_VERSION)) {
        return Err("场景 GLB 格式版本无效".into());
    }
    documents.set_owner(marker, "feathrise_scene_id")?;
    for node in document["nodes"].as_array_mut().ok_or("场景 GLB 缺少节点数组")? {
        let Some(value) = node.get_mut("extras").and_then(|extras| extras.get_mut("feathrise_placement")) else { continue; };
        let Some(raw) = value.as_str() else { continue; };
        let mut placement = serde_json::from_str(raw).map_err(|error| format!("场景 GLB 的摆放属性无效：{error}"))?;
        documents.rewrite_paths("", &mut placement);
        *value = Value::String(serde_json::to_string(&placement).map_err(|error| error.to_string())?);
    }
    Ok(index)
}

fn geometry_digest(document: &Value) -> Sha256 {
    let mut hash = Sha256::new();
    for key in ["buffers", "bufferViews", "accessors", "meshes", "materials", "textures", "images"] {
        hash.update(document[key].to_string());
    }
    hash
}

fn rebound_geometry_hash(
    input: &mut File, tail: u64, cancelled: &AtomicBool, mut old: Sha256, mut next: Sha256, expected: &str,
) -> Result<Option<String>, String> {
    let start = input.stream_position().map_err(|error| error.to_string())?;
    if tail >= 8 {
        let mut header = [0; 8];
        input.read_exact(&mut header).map_err(|error| error.to_string())?;
        if &header[4..] == b"BIN\0" {
            let mut remaining = u32::from_le_bytes(header[..4].try_into().unwrap()) as u64;
            if remaining > tail - 8 { return Err("场景 GLB 二进制块长度无效".into()); }
            let mut buffer = [0; 64 * 1024];
            while remaining != 0 {
                check_cancelled(cancelled)?;
                let size = (buffer.len() as u64).min(remaining) as usize;
                input.read_exact(&mut buffer[..size]).map_err(|error| error.to_string())?;
                old.update(&buffer[..size]);
                next.update(&buffer[..size]);
                remaining -= size as u64;
            }
        }
    }
    input.seek(SeekFrom::Start(start)).map_err(|error| error.to_string())?;
    let digest_hex = |digest: Sha256| digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    Ok((digest_hex(old) == expected).then(|| digest_hex(next)))
}

pub(super) fn rewrite_uris(paths: &CopyPaths, entry: &CopyEntry, document: &mut Value) -> bool {
    let mut changed = false;
    for key in ["buffers", "images"] {
        let Some(items) = document.get_mut(key).and_then(Value::as_array_mut) else { continue; };
        for item in items {
            let Some(uri) = item.get_mut("uri") else { continue; };
            if let Some(rebased) = uri.as_str().and_then(|uri| rebased_uri(paths, entry, uri)) {
                *uri = Value::String(rebased);
                changed = true;
            }
        }
    }
    changed
}

fn rebased_uri(paths: &CopyPaths, entry: &CopyEntry, uri: &str) -> Option<String> {
    let (uri, suffix) = uri.find(['?', '#']).map_or((uri, ""), |index| uri.split_at(index));
    let relative = decode_gltf_uri(uri).ok()?;
    if relative.starts_with('/') || relative.contains([':', '\0', '\\']) { return None; }
    let source = resolve_relative(&entry.source, &relative)?;
    let target = paths.rebase(&source).unwrap_or(source);
    if resolve_relative(&entry.target, &relative).as_deref() == Some(target.as_str()) {
        return None;
    }
    // 文件夹深度不同或链接越过另一个已复制的 SDK 目录时，仍将 URI 指向同一逻辑素材。
    let (parent, _) = entry.target.rsplit_once('/')?;
    let common = parent.split('/').zip(target.split('/')).take_while(|(a, b)| a == b).count();
    let mut path = "../".repeat(parent.split('/').count() - common);
    path.push_str(&target.split('/').skip(common).collect::<Vec<_>>().join("/"));
    let mut encoded = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            encoded.push(byte as char);
        } else {
            use std::fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded.push_str(suffix);
    Some(encoded)
}

fn resolve_relative(owner: &str, relative: &str) -> Option<String> {
    let (parent, _) = owner.rsplit_once('/')?;
    let mut components = parent.split('/').collect::<Vec<_>>();
    for component in relative.split('/') {
        match component {
            "" | "." => {}
            ".." => { components.pop()?; }
            _ => components.push(component),
        }
    }
    Some(components.join("/"))
}
