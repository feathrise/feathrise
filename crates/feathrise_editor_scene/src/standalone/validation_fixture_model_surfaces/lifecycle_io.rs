//! 本轮owned GLB的单槽后台变体与恢复；每次写入重新核对归属和精确旧字节。

use super::types::{MODEL, SOURCE};
use serde_json::{Value, json};
use std::{fs, io::Write, path::{Path, PathBuf}, sync::Arc};

#[derive(Clone, Copy, Debug)]
pub(super) enum Mutation { Raise, Topology, Restore }

pub(super) struct Source {
    project: PathBuf,
    assets: PathBuf,
    work: PathBuf,
    path: PathBuf,
    original: Arc<[u8]>,
    raised: Arc<[u8]>,
    topology: Arc<[u8]>,
}

pub(super) fn prepare(project: PathBuf, assets: PathBuf, output: PathBuf, placement_hash: String) -> Result<Arc<Source>, String> {
    let work = output.parent().ok_or("模型热更没有owned工作根")?.to_owned();
    owned(&project, &assets, &work)?;
    let project = project.canonicalize().map_err(|e|e.to_string())?;
    let assets = assets.canonicalize().map_err(|e|e.to_string())?;
    let work = work.canonicalize().map_err(|e|e.to_string())?;
    let path = assets.join(SOURCE);
    for directory in [assets.join("scenes"), assets.join("scenes/M1-SAMPLE")] { ordinary(&directory, true)?; }
    ordinary(&path, false)?;
    if path.canonicalize().map_err(|e|e.to_string())? != path {
        return Err("模型热更源不是已确认的普通绝对路径".into());
    }
    let original = fs::read(&path).map_err(|e|e.to_string())?;
    // 热更前后复用同一摆放保存基线，恢复的字节必须仍能通过原保存冲突检查。
    if feathrise_scene_content::authoring_io::bytes_hash(&original) != placement_hash {
        return Err("模型热更私有 GLB 与作者摆放保存基线不一致".into());
    }
    let (mut document, mut binary) = decode(&original)?;
    // 原摆放写入器允许重排节点。只按已确认场景、稳定目标和私有网格定位热更来源。
    let placements = feathrise_scene_content::read_scene_placements(&original, SOURCE)?;
    if document["asset"]["generator"] != "Feathrise owned model surface validation"
        || placements.scene_id.as_str() != "M1-SAMPLE"
        || !placements.placements.iter().any(|record| record.id == MODEL && record.asset_path == SOURCE
            && matches!(record.asset_target, feathrise_scene_content::ScenePlacementAssetTarget::Primitive {
                mesh: 0, primitive: 0, collision: false, ..
            }))
        || document["bufferViews"][0]["byteLength"] != 96
        || document["bufferViews"][2]["byteOffset"] != 192
        || binary.len() != 240
    { return Err("模型热更只接受本轮生成的有限双层夹具".into()); }
    let positions: [[f32;3];8] = [[-2.,0.,-2.],[2.,0.,-2.],[2.,0.,2.],[-2.,0.,2.],
        [-2.,0.15,-2.],[2.,0.15,-2.],[2.,0.15,2.],[-2.,0.15,2.]];
    let mut expected = Vec::new();
    for value in positions.iter().flatten() { expected.extend_from_slice(&value.to_le_bytes()); }
    for _ in 0..8 { for value in [0_f32,1.,0.] { expected.extend_from_slice(&value.to_le_bytes()); } }
    for value in [0_u32,2,1,0,3,2,4,6,5,4,7,6] { expected.extend_from_slice(&value.to_le_bytes()); }
    if binary != expected { return Err("模型热更夹具顶点或拓扑已被外部修改".into()); }
    // 第一变体只移动上层顶点，不改变primitive、三角数和索引；重心锚点应随几何更新。
    for vertex in 4..8 { binary[vertex * 12 + 4..vertex * 12 + 8].copy_from_slice(&0.35_f32.to_le_bytes()); }
    document["accessors"][0]["max"][1] = json!(0.35);
    let raised = encode(&document, &binary)?;
    // 第二变体替换上层四边形对角线；形状不变但旧三角锚点必须失效并撤销旧运行输出。
    for (index, value) in [4_u32,7,5,5,7,6].into_iter().enumerate() {
        binary[216 + index * 4..220 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    let topology = encode(&document, &binary)?;
    Ok(Arc::new(Source { project, assets, work, path, original: original.into(),
        raised: raised.into(), topology: topology.into() }))
}

pub(super) fn apply(source: &Source, mutation: Mutation) -> Result<(), String> {
    owned(&source.project, &source.assets, &source.work)?;
    for directory in [source.assets.join("scenes"), source.assets.join("scenes/M1-SAMPLE")] { ordinary(&directory, true)?; }
    ordinary(&source.path, false)?;
    if source.path.canonicalize().map_err(|e|e.to_string())? != source.path {
        return Err("模型热更发布源已改变真实路径".into());
    }
    let current = fs::read(&source.path).map_err(|e|e.to_string())?;
    let (expected, replacement) = match mutation {
        Mutation::Raise => (&source.original, &source.raised),
        Mutation::Topology => (&source.raised, &source.topology),
        Mutation::Restore => {
            if current.as_slice() == source.original.as_ref() { return Ok(()); }
            if current.as_slice() != source.raised.as_ref() && current.as_slice() != source.topology.as_ref() {
                return Err("模型验收恢复拒绝覆盖不属于本轮的源字节".into());
            }
            (&source.topology, &source.original)
        }
    };
    if !matches!(mutation, Mutation::Restore) && current.as_slice() != expected.as_ref() {
        return Err("模型热更前精确源字节已改变，拒绝覆盖".into());
    }
    let temporary = source.path.with_file_name(".scene.glb.model-validation.tmp");
    let mut file = fs::OpenOptions::new().create_new(true).write(true).open(&temporary).map_err(|e|e.to_string())?;
    file.write_all(replacement).and_then(|_|file.sync_all()).map_err(|e|e.to_string())?;
    drop(file);
    if fs::read(&source.path).map_err(|e|e.to_string())? != current {
        let _ = fs::remove_file(&temporary);
        return Err("模型热更发布前源字节发生竞争，保留后来文件".into());
    }
    fs::rename(&temporary, &source.path).map_err(|e|e.to_string())?;
    if fs::read(&source.path).map_err(|e|e.to_string())?.as_slice() != replacement.as_ref() {
        return Err("模型热更写入回读不一致".into());
    }
    Ok(())
}

fn owned(project: &Path, assets: &Path, work: &Path) -> Result<(), String> {
    for directory in [work, project, assets] { ordinary(directory, true)?; }
    let work = work.canonicalize().map_err(|e|e.to_string())?;
    let project = project.canonicalize().map_err(|e|e.to_string())?;
    let assets = assets.canonicalize().map_err(|e|e.to_string())?;
    let owner = read_json(&work.join(".feathrise-native-validation-owner.json"))?;
    let provenance = read_json(&project.join(".native-fixture-provenance.json"))?;
    if project.parent() != Some(work.as_path()) || assets != project.join("assets")
        || owner["schema"] != 1 || owner["purpose"] != "ordinary-author-boundary-validation"
        || owner["work_root"] != json!(work)
        || provenance["schema"] != 1 || provenance["purpose"] != "ordinary-author-boundary-validation"
        || provenance["project_root"] != json!(project)
        || provenance["scope"] != "explicit-owned-native-fixture-only"
    { return Err("模型热更owner或工程provenance不匹配，拒绝改写".into()); }
    Ok(())
}
fn ordinary(path: &Path, directory: bool) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e|e.to_string())?;
    #[cfg(windows)] {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return Err("模型热更不接受reparse路径".into()); }
    }
    if metadata.file_type().is_symlink() || metadata.is_dir() != directory || (!directory && !metadata.is_file()) {
        return Err("模型热更只接受普通文件或目录".into());
    }
    if !directory && metadata.len() > 64 * 1024 { return Err("模型夹具文件超出64KiB固定预算".into()); }
    Ok(())
}
fn read_json(path: &Path) -> Result<Value, String> {
    ordinary(path, false)?;
    serde_json::from_slice(&fs::read(path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())
}
fn decode(bytes: &[u8]) -> Result<(Value, Vec<u8>), String> {
    fn word(bytes: &[u8], offset: usize) -> Result<u32, String> {
        Ok(u32::from_le_bytes(bytes.get(offset..offset+4).ok_or("模型GLB字段截断")?.try_into().unwrap()))
    }
    if bytes.get(..4) != Some(b"glTF") || word(bytes,4)? != 2 || word(bytes,8)? as usize != bytes.len()
        || word(bytes,16)? != 0x4e4f534a { return Err("模型夹具GLB头不合法".into()); }
    let length = word(bytes,12)? as usize;
    let binary = 20_usize.checked_add(length).ok_or("模型GLB长度溢出")?;
    let document = serde_json::from_slice(bytes.get(20..binary).ok_or("模型GLB JSON截断")?).map_err(|e|e.to_string())?;
    if word(bytes,binary+4)? != 0x004e4942 || binary + 8 + word(bytes,binary)? as usize != bytes.len() {
        return Err("模型夹具GLB BIN截断".into());
    }
    Ok((document,bytes[binary+8..].to_vec()))
}
fn encode(document: &Value, binary: &[u8]) -> Result<Vec<u8>, String> {
    let mut json = serde_json::to_vec(document).map_err(|e|e.to_string())?;
    while json.len() % 4 != 0 { json.push(b' '); }
    let mut bytes = b"glTF".to_vec();
    for value in [2_u32,(28 + json.len() + binary.len()) as u32,json.len() as u32,0x4e4f534a] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&json);
    bytes.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"BIN\0"); bytes.extend_from_slice(binary);
    Ok(bytes)
}
