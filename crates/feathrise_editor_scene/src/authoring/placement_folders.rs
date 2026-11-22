//! 场景外层文件夹与摆放同文件保存。GLB 使用顶级 extras，旧场景使用原 JSON 可选字段。

use bevy::gltf::gltf::binary::Glb;
use feathrise_scene_content::ScenePlacementsAsset;
use serde_json::{Value, json};
use std::{borrow::Cow, collections::BTreeSet};

const FOLDERS: &str = "feathrise_object_folders";

pub(crate) fn validate_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty()
        || name.trim() != name
        || name.chars().count() > 128
        || name.chars().any(char::is_control)
    {
        return Err("文件夹名称需为 1 至 128 个有效字符");
    }
    Ok(())
}

fn folder_names(entry: Option<&Value>) -> Result<BTreeSet<String>, String> {
    let Some(entry) = entry else {
        return Ok(BTreeSet::new());
    };
    let names = entry.as_array().ok_or("场景文件夹目录格式无效")?;
    let mut folders = BTreeSet::new();
    for entry in names {
        let name = entry.as_str().ok_or("场景文件夹名称无效")?;
        validate_name(name)?;
        folders.insert(name.to_owned());
    }
    Ok(folders)
}

pub(super) fn read_glb(bytes: &[u8]) -> Result<BTreeSet<String>, String> {
    let glb = Glb::from_slice(bytes).map_err(|error| error.to_string())?;
    let json: Value = serde_json::from_slice(&glb.json).map_err(|error| error.to_string())?;
    folder_names(json.get("extras").and_then(|extras| extras.get(FOLDERS)))
}

pub(super) fn read_json(bytes: &[u8]) -> Result<BTreeSet<String>, String> {
    let json: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    folder_names(json.get(FOLDERS))
}

pub(super) fn write_glb(bytes: &[u8], folders: &BTreeSet<String>) -> Result<Vec<u8>, String> {
    let mut glb = Glb::from_slice(bytes).map_err(|error| error.to_string())?;
    let mut json: Value = serde_json::from_slice(&glb.json).map_err(|error| error.to_string())?;
    let extras = &mut json["extras"];
    if extras.is_null() {
        *extras = json!({});
    }
    let extras = extras
        .as_object_mut()
        .ok_or("场景 GLB 的 extras 不是可写入的对象")?;
    if folders.is_empty() {
        extras.remove(FOLDERS);
    } else {
        extras.insert(FOLDERS.to_owned(), json!(folders));
    }
    // 只更新作者目录。节点、动画、几何摘要和原始 BIN 保持现有内容。
    glb.json = Cow::Owned(serde_json::to_vec(&json).map_err(|error| error.to_string())?);
    glb.to_vec().map_err(|error| error.to_string())
}

pub(super) fn write_json(
    placements: &ScenePlacementsAsset,
    folders: &BTreeSet<String>,
) -> Result<Value, String> {
    let mut json = serde_json::to_value(placements).map_err(|error| error.to_string())?;
    if !folders.is_empty() {
        json[FOLDERS] = json!(folders);
    }
    Ok(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_round_trip_without_changing_gltf_placement_and_legacy_identities() {
        use feathrise_scene_content::{read_scene_placements, write_scene_placements_glb};
        // 无成员的声明依然需要落盘并重开。
        let source = ScenePlacementsAsset {
            scene_id: feathrise_scene_content::SceneId("TEST-FOLDERS".to_owned()),
            ..Default::default()
        };
        let asset_path = "scenes/test/scene.glb";
        let glb = write_scene_placements_glb(&source, asset_path, None).unwrap();
        let names = BTreeSet::from(["独立模型".to_owned(), "小院".to_owned()]);
        assert!(read_glb(&glb).unwrap().is_empty());
        let modified = write_glb(&glb, &names).unwrap();
        assert_eq!(read_glb(&modified).unwrap(), names);
        let adopted = read_scene_placements(&modified, asset_path).unwrap();
        assert_eq!(adopted.scene_id, source.scene_id);
        assert_eq!(adopted.placements, source.placements);
        let legacy = serde_json::to_vec(&write_json(&adopted, &names).unwrap()).unwrap();
        assert_eq!(read_json(&legacy).unwrap(), names);
        assert!(serde_json::from_slice::<ScenePlacementsAsset>(&legacy).is_ok());
    }
}
