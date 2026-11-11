//! GLB 调权另存。原二进制整段保留，只重指向有修改的图元的权重和关节索引。

use super::{
    asset::{Document, MAX_FILE, decode},
    influences::VertexWeights,
};
use crate::ui::asset_preview::rigging::files::{self, Stage, active};
use bevy::tasks::IoTaskPool;
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};

pub(super) async fn load(
    root: PathBuf,
    relative: String,
    cancel: Arc<AtomicBool>,
) -> Result<Arc<Document>, String> {
    let reader_cancel = cancel.clone();
    let (identity, bytes) = IoTaskPool::get()
        .spawn(async move {
            let root = files::checked_root(&root)?;
            files::freeze_bytes(&root, &relative, &reader_cancel)
        })
        .await?;
    decode(identity, bytes, &cancel)
}

struct Encoded {
    json: Vec<u8>,
    weights: Vec<u8>,
}

fn append(array: &mut Value, value: Value) -> Result<usize, String> {
    let list = array.as_array_mut().ok_or("GLB 缺少标准访问器列表")?;
    let index = list.len();
    list.push(value);
    Ok(index)
}

fn encode(
    doc: &Document,
    draft: &[Arc<[VertexWeights]>],
    cancel: &AtomicBool,
) -> Result<Encoded, String> {
    if draft.len() != doc.surfaces.len() {
        return Err("草稿与蒙皮图元数量不一致".into());
    }
    let mut json = doc.json.clone();
    let mut weights = Vec::new();
    for (surface, draft) in doc.surfaces.iter().zip(draft) {
        if Arc::ptr_eq(&surface.original, draft) {
            continue;
        }
        active(cancel)?;
        if draft.len() != surface.original.len() {
            return Err("草稿顶点数改变，无法另存权重".into());
        }
        let offset = doc.binary.len() + weights.len();
        let view = append(
            &mut json["bufferViews"],
            json!({
                "buffer": 0, "byteOffset": offset, "byteLength": draft.len() * 24,
                "byteStride": 24, "target": 34962
            }),
        )?;
        let joints = append(
            &mut json["accessors"],
            json!({
                "bufferView": view, "componentType": 5123, "count": draft.len(), "type": "VEC4"
            }),
        )?;
        let influence = append(
            &mut json["accessors"],
            json!({
                "bufferView": view, "byteOffset": 8, "componentType": 5126,
                "count": draft.len(), "type": "VEC4"
            }),
        )?;
        for block in draft.chunks(4096) {
            active(cancel)?;
            for row in block {
                let sum: f32 = row.weights.iter().sum();
                if VertexWeights::checked(row.joints, row.weights, surface.skin.joints.len())
                    .is_none()
                    || (sum - 1.0).abs() > 1e-4
                {
                    return Err("草稿含无效关节或非归一化权重".into());
                }
                for joint in row.joints {
                    weights.extend(joint.to_le_bytes());
                }
                for weight in row.weights {
                    weights.extend(weight.to_le_bytes());
                }
            }
        }
        let attributes =
            &mut json["meshes"][surface.mesh]["primitives"][surface.primitive]["attributes"];
        attributes["JOINTS_0"] = json!(joints);
        attributes["WEIGHTS_0"] = json!(influence);
    }
    if weights.is_empty() {
        return Err("当前模型没有待保存的权重修改".into());
    }
    json["buffers"][0]["byteLength"] = json!(doc.binary.len() + weights.len());
    let mut json = serde_json::to_vec(&json).map_err(|error| error.to_string())?;
    json.resize(json.len().next_multiple_of(4), b' ');
    if json.len() > 16 * 1024 * 1024 {
        return Err("新 GLB 的 JSON 块超过预算".into());
    }
    let size =
        28 + json.len() + doc.binary.len() + weights.len() + doc.source.len() - doc.binary.end;
    if size > MAX_FILE {
        return Err("另存后的 GLB 超过 256 MiB".into());
    }
    Ok(Encoded { json, weights })
}

fn write(
    doc: &Document,
    encoded: &Encoded,
    path: &Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("无法创建权重暂存文件：{error}"))?;
    let mut writer = BufWriter::new(file);
    let binary = &doc.source[doc.binary.clone()];
    let trailing = &doc.source[doc.binary.end..];
    let size = 28 + encoded.json.len() + binary.len() + encoded.weights.len() + trailing.len();
    for bytes in [
        b"glTF".as_slice(),
        &2_u32.to_le_bytes(),
        &(size as u32).to_le_bytes(),
        &(encoded.json.len() as u32).to_le_bytes(),
        b"JSON",
        &encoded.json,
        &((binary.len() + encoded.weights.len()) as u32).to_le_bytes(),
        b"BIN\0",
        binary,
        &encoded.weights,
        trailing,
    ] {
        for chunk in bytes.chunks(64 * 1024) {
            active(cancel)?;
            writer
                .write_all(chunk)
                .map_err(|error| format!("写入权重 GLB 失败：{error}"))?;
        }
    }
    writer
        .flush()
        .map_err(|error| format!("写入权重 GLB 失败：{error}"))
}

pub(super) async fn save(
    root: PathBuf,
    doc: Arc<Document>,
    draft: Vec<Arc<[VertexWeights]>>,
    cancel: Arc<AtomicBool>,
) -> Result<String, String> {
    let encoded = encode(&doc, &draft, &cancel)?;
    IoTaskPool::get()
        .spawn(async move {
            let root = files::checked_root(&root)?;
            let stage = Stage::new(&root)?;
            let path = stage.path("weights.glb");
            write(&doc, &encoded, &path, &cancel)?;
            files::check_frozen(&root, &doc.identity, &cancel)
                .map_err(|error| format!("原模型已变化，权重草稿仍保留：{error}"))?;
            files::publish_named(&path, &root, &doc.identity, "调权", &cancel)
        })
        .await
}
