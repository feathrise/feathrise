//! 显式原生验收的中性资产夹具；只准备本轮独占工程，不参与普通新建工程或复制原游戏内容。

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const OWNER_FILE: &str = ".feathrise-native-validation-owner.json";
const OWNER_PURPOSE: &str = "ordinary-author-boundary-validation";
const EMPTY_REFERENCE: &str = ".native-fixture-empty-reference";
const STAGING_ASSETS: &str = ".native-fixture-staging-assets";
const EMPTY_BACKUP: &str = ".native-fixture-original-empty-assets";
const MAX_EMPTY_ENTRIES: usize = 64;
const MAX_SMALL_BYTES: u64 = 128 * 1024;

/// 调用方必须先create_dir独占工作根并create_new写入父级owner，再新建空工程；不得用于已有作者工程。
/// 本函数只在窗口启动前的显式验收流程调用，固定场景/纹理生成不进入Bevy帧关键路径。
pub(super) fn prepare(root: &Path, label: &str) -> Result<(), String> {
    let root = owned_project_root(root)?;
    let project = crate::project::open(&root)?;
    let assets = root.join("assets");
    if project.package != "feathrise_game_template"
        || project.context.asset_roots != [assets.clone()]
    {
        return Err("验收夹具只接受本轮创建、仅有assets根的正式空模板".into());
    }

    // 不靠“目录看起来很空”判断作者数据：用同一create_empty产生参照，逐路径逐字节核对。
    // 参照、暂存和旧空骨架均保留；重复执行或部分失败不会删除/覆盖上次证据。
    let reference = root.join(EMPTY_REFERENCE);
    fs::create_dir(&reference).map_err(|error| format!("空骨架参照不能独占创建：{error}"))?;
    feathrise_project::seed::create_empty(&reference)?;
    let expected = empty_tree(&reference)?;
    if empty_tree(&assets)? != expected {
        return Err("工程assets已不是新建的空骨架，拒绝替换任何作者内容".into());
    }
    if root.join(EMPTY_BACKUP).try_exists().map_err(|error| error.to_string())? {
        return Err("验收原空资产备份已存在，拒绝覆盖".into());
    }
    let staging = root.join(STAGING_ASSETS);
    fs::create_dir(&staging).map_err(|error| format!("中性夹具暂存不能独占创建：{error}"))?;
    feathrise_project::seed::create(&staging, label)?;
    // 验收能力只写入本轮独占夹具的依赖配置；普通新工程与既有工程不开启故障注入。
    let manifest_path = root.join("Cargo.toml");
    let mut manifest: toml::Value = toml::from_str(&fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    enable_validation_features(&mut manifest)?;
    fs::write(&manifest_path, toml::to_string_pretty(&manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let sources = copy_probe(&staging)?;
    write_animation(&staging)?;
    encode_surface(&staging)?;
    write_new(
        &root.join(".native-fixture-provenance.json"),
        &serde_json::to_vec_pretty(&json!({
            "schema":1,"purpose":OWNER_PURPOSE,"project_root":root,
            "project_id":project.id,"label":label,
            "scope":"explicit-owned-native-fixture-only",
            "scene_source":"feathrise_project::seed::create",
            "surface_encoder":"feathrise_editor_assets::ktx2",
            "animation_source":"procedural named triangle with two translation keys",
            "scene_view":"scene-owned Camera3d and PrimarySceneView positioned by template; no player or RPG fixture",
            "copied_files":sources,"original_empty_assets_backup":EMPTY_BACKUP,
            "not_in_normal_template":true
        })).map_err(|error| error.to_string())?,
    )?;

    // 生成期间原空目录若被写入，整体发布失败；不把生成好的部分文件混进作者目录。
    if owned_project_root(&root)? != root || empty_tree(&assets)? != expected {
        return Err("夹具准备期间工程归属或空骨架变化，拒绝发布".into());
    }
    let backup = root.join(EMPTY_BACKUP);
    fs::rename(&assets, &backup).map_err(|error| format!("保留原空资产失败：{error}"))?;
    if let Err(error) = fs::rename(&staging, &assets) {
        // 仅恢复刚刚由本函数重命名的自有目录；恢复失败仍保留备份并报告具体位置。
        let restored = fs::rename(&backup, &assets);
        return Err(format!("中性夹具发布失败：{error}；原空骨架恢复结果：{restored:?}；备份位置：{}", backup.display()));
    }
    Ok(())
}

fn enable_validation_features(manifest: &mut toml::Value) -> Result<(), String> {
    // 夹具也可能遇到不完整模板；按可诊断错误退出，不用索引 panic 中断验收收口。
    let features = manifest.get_mut("dependencies")
        .and_then(|dependencies| dependencies.get_mut("feathrise_runtime"))
        .and_then(|sdk| sdk.get_mut("features"))
        .and_then(toml::Value::as_array_mut)
        .ok_or("中性模板缺少 SDK features 数组")?;
    if !features.iter().any(|feature| feature.as_str() == Some("validation")) {
        features.push(toml::Value::String("validation".into()));
    }
    Ok(())
}

fn write_animation(assets: &Path) -> Result<(), String> {
    // 两关键帧平移通道只服务原动画UI的编辑/UndoRedo/保存；无骨骼、角色或原游戏动作依赖。
    let positions = [-0.5_f32, 0., 0., 0.5, 0., 0., 0., 1., 0.];
    let times = [0_f32, 1.];
    let translations = [0_f32, 0., 0., 0., 0.1, 0.];
    let mut binary = Vec::new();
    for value in positions.into_iter().chain(times).chain(translations) {
        binary.extend_from_slice(&value.to_le_bytes());
    }
    let document = json!({
        "asset":{"version":"2.0","generator":"Feathrise owned native validation"},
        "scene":0,"scenes":[{"nodes":[0]}],
        "nodes":[{"name":"NativeAnimation","mesh":0}],
        "meshes":[{"primitives":[{"attributes":{"POSITION":0},"material":0}]}],
        "materials":[{"pbrMetallicRoughness":{"baseColorFactor":[0.4,0.7,0.9,1.0],"metallicFactor":0.0,"roughnessFactor":0.8}}],
        "buffers":[{"byteLength":binary.len()}],
        "bufferViews":[
            {"buffer":0,"byteOffset":0,"byteLength":36,"target":34962},
            {"buffer":0,"byteOffset":36,"byteLength":8},
            {"buffer":0,"byteOffset":44,"byteLength":24}
        ],
        "accessors":[
            {"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[-0.5,0.0,0.0],"max":[0.5,1.0,0.0]},
            {"bufferView":1,"componentType":5126,"count":2,"type":"SCALAR","min":[0.0],"max":[1.0]},
            {"bufferView":2,"componentType":5126,"count":2,"type":"VEC3"}
        ],
        "animations":[{"name":"中性平移动画","samplers":[{"input":1,"output":2,"interpolation":"LINEAR"}],
            "channels":[{"sampler":0,"target":{"node":0,"path":"translation"}}]}]
    });
    let mut json = serde_json::to_vec(&document).map_err(|error| error.to_string())?;
    while json.len() % 4 != 0 {
        json.push(b' ');
    }
    let mut glb = b"glTF".to_vec();
    glb.extend_from_slice(&2_u32.to_le_bytes());
    glb.extend_from_slice(&((28 + json.len() + binary.len()) as u32).to_le_bytes());
    glb.extend_from_slice(&(json.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x4e4f534a_u32.to_le_bytes());
    glb.extend_from_slice(&json);
    glb.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    glb.extend_from_slice(&0x004e4942_u32.to_le_bytes());
    glb.extend_from_slice(&binary);
    write_new(&assets.join("scene/animation.glb"), &glb)
}

fn owned_project_root(root: &Path) -> Result<PathBuf, String> {
    require_ordinary(root, true)?;
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let work = root.parent().ok_or("验收工程缺少工作父目录")?;
    require_ordinary(work, true)?;
    let owner: Value = serde_json::from_slice(&read_small(&work.join(OWNER_FILE))?)
        .map_err(|error| format!("验收owner JSON无效：{error}"))?;
    if owner["schema"] != 1
        || owner["purpose"] != OWNER_PURPOSE
        || owner["work_root"] != json!(work)
    {
        return Err("验收工程父目录缺少匹配的本轮独占owner，拒绝准备夹具".into());
    }
    Ok(root)
}

/// 显式验收只内嵌两份已有小夹具，分发目录不必夹带测试源码或依赖编译机路径。
/// 普通新建不调用此入口，仍保持零场景、零预设的空工程。
pub(super) fn scene_fixture(name: &str) -> Result<&'static [u8], String> {
    match name {
        "probe.glb" => Ok(include_bytes!("../tests/fixtures/scene/probe.glb")),
        "albedo.png" => Ok(include_bytes!("../tests/fixtures/scene/albedo.png")),
        _ => Err(format!("未知内嵌验收素材：{name}")),
    }
}

fn copy_probe(assets: &Path) -> Result<Vec<Value>, String> {
    let mut sources = Vec::new();
    for name in ["probe.glb", "albedo.png"] {
        let relative = format!("tests/fixtures/scene/{name}");
        let bytes = scene_fixture(name)?;
        write_new(&assets.join("scene").join(name), bytes)?;
        let sha256: String = Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect();
        sources.push(json!({"source":relative,"embedded":true,"destination":format!("scene/{name}"),"bytes":bytes.len(),"sha256":sha256}));
    }
    Ok(sources)
}

fn encode_surface(assets: &Path) -> Result<(), String> {
    use feathrise_editor_assets::ktx2::{Rgba8ArrayKind, encode_rgba8_array, encode_surface_maps};
    for (name, pixel, kind) in [
        ("base.ktx2", vec![255, 255, 255, 255], Rgba8ArrayKind::SrgbColor),
        ("normal.ktx2", vec![128, 128, 255, 255], Rgba8ArrayKind::TangentNormal),
        ("arm_height.ktx2", vec![255, 204, 0, 128], Rgba8ArrayKind::LinearData),
    ] {
        write_new(&assets.join("textures/surface").join(name), &encode_rgba8_array(&[pixel], [1, 1], kind)?)?;
    }
    let terrain = assets.join(feathrise_project::seed::SAMPLE_TERRAIN_FOLDER);
    let (indices, weights) = encode_surface_maps(
        &fs::read(terrain.join("surface/indices.rgba32")).map_err(|error| error.to_string())?,
        &fs::read(terrain.join("surface/weights.rgba8")).map_err(|error| error.to_string())?,
        feathrise_project::seed::SAMPLE_SURFACE_RESOLUTION,
        0,
    )?;
    write_new(&terrain.join("surface/indices.ktx2"), &indices)?;
    write_new(&terrain.join("surface/weights.ktx2"), &weights)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::create_dir_all(path.parent().ok_or("夹具文件缺少父目录")?).map_err(|error| error.to_string())?;
    OpenOptions::new().write(true).create_new(true).open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| format!("夹具文件不能独占创建 {}：{error}", path.display()))
}

fn read_small(path: &Path) -> Result<Vec<u8>, String> {
    require_ordinary(path, false)?;
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(|error| error.to_string())?
        .take(MAX_SMALL_BYTES + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_SMALL_BYTES {
        return Err(format!("验收小文件超过128KiB预算：{}", path.display()));
    }
    Ok(bytes)
}

fn require_ordinary(path: &Path, directory: bool) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| format!("验收路径不可读取 {}：{error}", path.display()))?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.file_type().is_symlink() || reparse
        || if directory { !metadata.is_dir() } else { !metadata.is_file() }
    {
        return Err(format!("验收路径必须是无链接的普通{}：{}", if directory { "目录" } else { "文件" }, path.display()));
    }
    Ok(())
}

fn empty_tree(root: &Path) -> Result<BTreeMap<PathBuf, Option<Vec<u8>>>, String> {
    require_ordinary(root, true)?;
    let mut entries = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            let relative = path.strip_prefix(root).map_err(|error| error.to_string())?.to_path_buf();
            if entries.len() >= MAX_EMPTY_ENTRIES || relative.components().count() > 8 {
                return Err("验收空骨架超过固定文件/深度预算".into());
            }
            let directory = fs::symlink_metadata(&path).map_err(|error| error.to_string())?.is_dir();
            require_ordinary(&path, directory)?;
            let bytes = if directory {
                pending.push(path);
                None
            } else {
                Some(read_small(&path)?)
            };
            entries.insert(relative, bytes);
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_feature_reports_missing_sdk_and_is_idempotent() {
        for source in ["[package]\nname='fixture'", "[dependencies]\nfeathrise_runtime='0.1'", "[dependencies.feathrise_runtime]\nfeatures='validation'"] {
            let mut manifest = toml::from_str(source).unwrap();
            assert!(enable_validation_features(&mut manifest).unwrap_err().contains("features"));
        }
        let mut manifest: toml::Value = toml::from_str("[dependencies.feathrise_runtime]\nfeatures=['existing']").unwrap();
        enable_validation_features(&mut manifest).unwrap();
        enable_validation_features(&mut manifest).unwrap();
        assert_eq!(manifest["dependencies"]["feathrise_runtime"]["features"].as_array().unwrap(), &vec![
            toml::Value::String("existing".into()), toml::Value::String("validation".into()),
        ]);
    }

    struct OwnedDirectory(PathBuf);
    impl OwnedDirectory {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "feathrise-native-fixture-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
            ));
            fs::create_dir(&directory).unwrap();
            Self(directory.canonicalize().unwrap())
        }

        fn owner(&self) {
            write_new(&self.0.join(OWNER_FILE), &serde_json::to_vec(&json!({
                "schema":1,"purpose":OWNER_PURPOSE,"work_root":self.0
            })).unwrap()).unwrap();
        }
    }
    impl Drop for OwnedDirectory {
        fn drop(&mut self) {
            // 仅回收本测试create_dir成功的唯一临时根，先验证绝对路径身份与临时父目录。
            assert_eq!(self.0.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
            assert_eq!(self.0.canonicalize().unwrap(), self.0);
            assert!(self.0.file_name().unwrap().to_string_lossy().starts_with("feathrise-native-fixture-"));
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn fixture_requires_matching_exclusive_parent_owner() {
        let directory = OwnedDirectory::new();
        let project = directory.0.join("project");
        fs::create_dir(&project).unwrap();
        assert!(owned_project_root(&project).is_err());
        directory.owner();
        assert_eq!(owned_project_root(&project).unwrap(), project);
        let other = directory.0.join("other");
        fs::create_dir(&other).unwrap();
        fs::write(directory.0.join(OWNER_FILE), serde_json::to_vec(&json!({
            "schema":1,"purpose":OWNER_PURPOSE,"work_root":other
        })).unwrap()).unwrap();
        assert!(owned_project_root(&project).is_err());
    }

    #[test]
    fn fixture_never_replaces_a_modified_empty_project() {
        let directory = OwnedDirectory::new();
        directory.owner();
        let project = directory.0.join("project");
        fs::create_dir(&project).unwrap();
        fs::create_dir(project.join(".editor")).unwrap();
        write_new(&project.join("Cargo.toml"), b"[package]\nname=\"feathrise_game_template\"\nversion=\"0.1.0\"\n").unwrap();
        write_new(&project.join("editor-project.json"), &serde_json::to_vec(&json!({
            "format":2,"sdk_abi":crate::project::LEGACY_SDK_ABI,"id":"045f04e0-1234-4abc-8def-123456789abc",
            "name":"fixture","package":"feathrise_game_template","cargo_manifest":"Cargo.toml",
            "asset_roots":["assets"],"editor_data":".editor"
        })).unwrap()).unwrap();
        let assets = project.join("assets");
        feathrise_project::seed::create_empty(&assets).unwrap();
        fs::write(assets.join("scenes/catalog.scene_catalog.json"), b"author-owned-modification").unwrap();
        let before = empty_tree(&assets).unwrap();
        let error = prepare(&project, "fixture").unwrap_err();
        assert!(error.contains("空骨架"), "{error}");
        assert_eq!(empty_tree(&assets).unwrap(), before);
        assert!(!project.join(STAGING_ASSETS).exists());
        assert!(!project.join(EMPTY_BACKUP).exists());
    }

    #[test]
    fn procedural_animation_has_real_named_translation_keys_and_valid_chunks() {
        let directory = OwnedDirectory::new();
        write_animation(&directory.0).unwrap();
        let bytes = fs::read(directory.0.join("scene/animation.glb")).unwrap();
        let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        assert_eq!(&bytes[..4], b"glTF");
        assert_eq!(word(4), 2);
        assert_eq!(word(8) as usize, bytes.len());
        assert_eq!(word(16), 0x4e4f534a);
        let json_len = word(12) as usize;
        let document: Value = serde_json::from_slice(&bytes[20..20 + json_len]).unwrap();
        let binary_header = 20 + json_len;
        assert_eq!(word(binary_header + 4), 0x004e4942);
        assert_eq!(word(binary_header) as usize + binary_header + 8, bytes.len());
        assert_eq!(document["nodes"][0]["name"], "NativeAnimation");
        assert_eq!(document["animations"][0]["channels"][0]["target"]["path"], "translation");
        assert_eq!(document["accessors"][1]["count"], 2);
        assert_eq!(document["accessors"][2]["count"], 2);
        let binary = &bytes[binary_header + 8..];
        let float = |offset| f32::from_le_bytes(binary[offset..offset + 4].try_into().unwrap());
        assert_eq!((float(36), float(40)), (0., 1.));
        assert_eq!((float(56), float(60), float(64)), (0., 0.1, 0.));
    }
}
