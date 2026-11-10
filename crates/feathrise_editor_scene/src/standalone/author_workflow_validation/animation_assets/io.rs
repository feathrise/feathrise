//! 仅后台读写自有验收工程副本；每次原子替换重新核对所有权、普通文件和预期原字节。

use super::types::*;
use crate::ui::asset_preview::animation::workflow_validation::{self as animation, KeyIdentity};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use feathrise_scene_content::authoring_transaction::{PreparedWrite, commit_prepared_writes_checked};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

#[cfg(test)]
mod tests;

const OWNER: &str = ".new-editor-recovery-owner.json";
const LIMIT: u64 = 2 * 1024 * 1024;

pub(super) fn prepare(paths: &Paths) -> Result<IoResult, String> {
    check_owner(paths)?;
    let animation = read_asset(paths, ANIMATION)?;
    let model = read_asset(paths, MODEL)?;
    let texture = read_asset(paths, TEXTURE)?;
    Ok(IoResult::Prepared(Prepared {
        animation_sha256: hash(&animation),
        model: Arc::new(model),
        texture: Arc::new(texture),
    }))
}

pub(super) fn saved(paths: &Paths, identity: KeyIdentity) -> Result<IoResult, String> {
    check_owner(paths)?;
    let bytes = read_asset(paths, ANIMATION)?;
    let key = animation::read_saved(&paths.assets.join(ANIMATION), identity)?;
    Ok(IoResult::SavedAnimation {
        sha256: hash(&bytes),
        key,
    })
}

pub(super) fn write(
    paths: &Paths,
    mutation: Mutation,
    expected: Arc<Vec<u8>>,
) -> Result<IoResult, String> {
    check_owner(paths)?;
    let relative = if matches!(mutation, Mutation::Texture) {
        TEXTURE
    } else {
        MODEL
    };
    if read_asset(paths, relative)?.as_slice() != expected.as_slice() {
        return Err("自有热更源已被外部修改，拒绝覆盖".into());
    }
    let bytes = match mutation {
        Mutation::Mesh => patch_glb(&expected, true)?,
        Mutation::Material => patch_glb(&expected, false)?,
        Mutation::Texture => {
            let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(2, 2, Rgba(PIXEL)));
            let mut output = Cursor::new(Vec::new());
            image
                .write_to(&mut output, ImageFormat::Png)
                .map_err(|error| error.to_string())?;
            output.into_inner()
        }
    };
    let path = checked_asset(paths, relative)?;
    commit_prepared_writes_checked(vec![PreparedWrite::new(path, bytes.clone())], |_, _| {
        check_owner(paths)?;
        if read_asset(paths, relative)?.as_slice() != expected.as_slice() {
            return Err("热更发布前原字节变化，拒绝覆盖".into());
        }
        Ok(())
    })
    .map_err(|error| error.message().to_owned())?;
    if read_asset(paths, relative)? != bytes {
        return Err("自有热更文件没有完整发布".into());
    }
    Ok(IoResult::Written {
        sha256: hash(&bytes),
        bytes: Arc::new(bytes),
    })
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn ordinary(path: &Path) -> Result<fs::Metadata, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| format!("{}：{error}", path.display()))?;
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if linked {
        return Err("有限热更拒绝链接或重解析路径".into());
    }
    Ok(metadata)
}

fn read_small(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    let metadata = ordinary(path)?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err("有限热更夹具必须使用已知的小型普通文件".into());
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > maximum {
        return Err("有限热更源读取期间超出预算".into());
    }
    Ok(bytes)
}

fn check_owner(paths: &Paths) -> Result<(), String> {
    if paths
        .project
        .canonicalize()
        .map_err(|error| error.to_string())?
        != paths.project
        || paths
            .assets
            .canonicalize()
            .map_err(|error| error.to_string())?
            != paths.assets
        || !paths.assets.starts_with(&paths.project)
    {
        return Err("有限热更必须位于当前规范工程资产根".into());
    }
    ordinary(&paths.project)?;
    ordinary(&paths.assets)?;
    let owner: Value = serde_json::from_slice(&read_small(&paths.project.join(OWNER), 4096)?)
        .map_err(|error| error.to_string())?;
    let descriptor: Value = serde_json::from_slice(&read_small(
        &paths.project.join("editor-project.json"),
        64 * 1024,
    )?)
    .map_err(|error| error.to_string())?;
    let owner_root = owner["project_root"]
        .as_str()
        .map(PathBuf::from)
        .ok_or("热更缺少自有工程根")?;
    if owner["schema"] != 1
        || owner["purpose"] != "native-play-recovery"
        || owner["project_id"].as_str().is_none()
        || owner["project_id"] != descriptor["id"]
        || owner_root
            .canonicalize()
            .map_err(|error| error.to_string())?
            != paths.project
    {
        return Err("热更标记不属于本工程，拒绝写入".into());
    }
    Ok(())
}

fn checked_asset(paths: &Paths, relative: &str) -> Result<PathBuf, String> {
    if !matches!(relative, ANIMATION | MODEL | TEXTURE) {
        return Err("有限热更目标不在三项资产白名单".into());
    }
    let path = paths.assets.join(relative);
    let mut cursor = Some(path.as_path());
    while let Some(ancestor) = cursor {
        ordinary(ancestor)?;
        if ancestor == paths.project {
            break;
        }
        cursor = ancestor.parent();
    }
    if path.canonicalize().map_err(|error| error.to_string())? != path {
        return Err("热更目标不是当前工程普通文件".into());
    }
    Ok(path)
}

fn read_asset(paths: &Paths, relative: &str) -> Result<Vec<u8>, String> {
    read_small(&checked_asset(paths, relative)?, LIMIT)
}

/// 已知四顶点GLB的小型外部工具改写；不承担生产GLB编辑或导出器职责。
fn patch_glb(bytes: &[u8], geometry: bool) -> Result<Vec<u8>, String> {
    fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            bytes
                .get(offset..offset + 4)
                .ok_or("probe GLB头截断")?
                .try_into()
                .map_err(|_| "probe GLB字段截断")?,
        ))
    }
    if bytes.get(..4) != Some(b"glTF")
        || u32_at(bytes, 4)? != 2
        || u32_at(bytes, 8)? as usize != bytes.len()
    {
        return Err("热更probe必须是完整GLB2".into());
    }
    let json_length = u32_at(bytes, 12)? as usize;
    let bin_header = 20usize.checked_add(json_length).ok_or("probe偏移溢出")?;
    if u32_at(bytes, 16)? != 0x4e4f534a || u32_at(bytes, bin_header + 4)? != 0x004e4942 {
        return Err("热更probe GLB块不符".into());
    }
    let mut json: Value =
        serde_json::from_slice(bytes.get(20..bin_header).ok_or("probe JSON截断")?)
            .map_err(|error| error.to_string())?;
    if json["asset"]["generator"] != "independent-editor-asset-fixture"
        || json["accessors"][0]["count"] != 4
        || json["accessors"][0]["type"] != "VEC3"
    {
        return Err("不是本轮已知四顶点probe，拒绝热更".into());
    }
    let mut binary = bytes.get(bin_header + 8..).ok_or("probe BIN截断")?.to_vec();
    if geometry {
        for index in 0..4 {
            let offset = index * 12;
            let x = f32::from_le_bytes(
                binary
                    .get(offset..offset + 4)
                    .ok_or("probe顶点截断")?
                    .try_into()
                    .map_err(|_| "probe顶点无效")?,
            ) * 1.375;
            binary[offset..offset + 4].copy_from_slice(&x.to_le_bytes());
        }
        json["accessors"][0]["min"][0] = json!(-2.75);
        json["accessors"][0]["max"][0] = json!(2.75);
    } else {
        json["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"] = json!(COLOR);
        json["materials"][0]["pbrMetallicRoughness"]["roughnessFactor"] = json!(0.45);
    }
    let mut encoded = serde_json::to_vec(&json).map_err(|error| error.to_string())?;
    while encoded.len() % 4 != 0 {
        encoded.push(b' ');
    }
    let total = 12 + 8 + encoded.len() + 8 + binary.len();
    let mut output = Vec::with_capacity(total);
    output.extend_from_slice(b"glTF");
    output.extend_from_slice(&2u32.to_le_bytes());
    output.extend_from_slice(&(total as u32).to_le_bytes());
    output.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
    output.extend_from_slice(&0x4e4f534au32.to_le_bytes());
    output.extend(encoded);
    output.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    output.extend_from_slice(&0x004e4942u32.to_le_bytes());
    output.extend(binary);
    Ok(output)
}
