//! 将已保存的场景复制成独立作者数据，最后发布目录登记。

mod content;
mod filesystem;
mod glb;
#[cfg(test)]
mod character_tests;
#[cfg(test)]
mod tests;

use super::{
    create::{ensure_unique_scene_name, next_scene_id},
    creation_input::{normalize_display_name, scene_directory_name},
    files::{atomic_write_json, decode_scene_catalog, safe_asset_path},
};
use crate::{
    constants::SCENE_FOLDER_ASSET_PATH,
    scene_browser::{decode_folder_document, prepare_scene_assignment_mutation},
    types::SceneFolderFileSnapshot,
};
use content::{CopyPaths, SCENE_CHARACTERS_FILE};
use feathrise_scene_content::{SceneId, SCENE_CATALOG_ASSET_PATH, authoring_io::ContentAuthorLock};
use filesystem::{OwnedFolders, collect_entries, ordinary};
use std::{
    fs, io,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug)]
pub(crate) struct CopiedScene {
    pub(crate) scene_id: SceneId,
    pub(crate) display_name: String,
    #[cfg(test)]
    pub(crate) folder: String,
    pub(crate) folder_snapshot: Option<SceneFolderFileSnapshot>,
}

/// 由作者后台任务调用。整个复制与场景写入共用同一作者锁，场景目录最后提交。
pub(crate) fn copy_scene(
    asset_root: &Path,
    source_id: &SceneId,
    display_name: &str,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(&'static str),
) -> Result<CopiedScene, String> {
    let display_name = normalize_display_name(display_name)?;
    let directory_name = scene_directory_name(&display_name)?;
    SceneId::parse(source_id.as_str())?;
    check_cancelled(cancelled)?;

    let asset_root = asset_root
        .canonicalize()
        .map_err(|error| format!("无法定位项目资产目录 {}：{error}", asset_root.display()))?;
    let scenes_root = asset_root.join("scenes");
    filesystem::checked_directory(&scenes_root)?;
    let _author_lock = ContentAuthorLock::acquire(&asset_root)?;
    progress("正在读取源场景…");

    let catalog_path = asset_root.join(SCENE_CATALOG_ASSET_PATH);
    let original_catalog = read_file(&catalog_path)?;
    let catalog = decode_scene_catalog(&catalog_path, &original_catalog)?;
    let source_index = catalog.scenes.iter().position(|entry| &entry.id == source_id)
        .ok_or_else(|| format!("场景目录中没有来源场景 {source_id}"))?;
    let source = &catalog.scenes[source_index];
    ensure_unique_scene_name(&catalog.scenes, &display_name, &directory_name)?;
    let unix_millis = SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|error| format!("系统时间早于 Unix 纪元：{error}"))?.as_millis();
    let scene_id = next_scene_id(&scenes_root, &catalog.scenes, unix_millis)?;
    let paths = CopyPaths::new(&asset_root, source, &scene_id, &directory_name)?;
    for other in catalog.scenes.iter().filter(|entry| &entry.id != source_id) {
        paths.check_scene_isolation(other)?;
    }

    let original_folder = match fs::symlink_metadata(asset_root.join(SCENE_FOLDER_ASSET_PATH)) {
        Ok(_) => Some(read_file(&asset_root.join(SCENE_FOLDER_ASSET_PATH))?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("检查场景分类失败：{error}")),
    };
    let folder_id = original_folder.as_deref().map(decode_folder_document).transpose()?
        .and_then(|document| document.scene_folders.get(source_id.as_str()).cloned());
    let folder_mutation = prepare_scene_assignment_mutation(
        &asset_root, original_folder, &scene_id, folder_id.as_deref(),
    )?;

    let roles = source.characters_path.clone().unwrap_or_else(|| {
        format!("{}/{SCENE_CHARACTERS_FILE}", source.folder.trim_end_matches('/'))
    });
    // 旧角色模块也读取私有目录内未登记的标准文件；登记仍沿用源场景的语义。
    let roles = match fs::symlink_metadata(safe_asset_path(&asset_root, &roles)?) {
        Ok(_) => Some(roles),
        Err(error) if error.kind() == io::ErrorKind::NotFound && source.characters_path.is_none() => None,
        Err(error) => return Err(format!("读取源场景角色文件 {roles} 失败：{error}")),
    };
    if let Some(roles) = &roles {
        content::require_private_file(&paths, roles)?;
    }
    let mut target = source.clone();
    target.id = scene_id.clone();
    target.display_name.clone_from(&display_name);
    target.folder = paths.target_folder().to_owned();
    for field in [
        &mut target.surface_path, &mut target.vegetation_path, &mut target.water_path,
        &mut target.boundary_path, &mut target.characters_path,
    ] {
        if let Some(relative) = field {
            content::require_private_file(&paths, relative)?;
            *relative = paths.rebase(relative)
                .ok_or_else(|| format!("场景作者文件不在源场景目录：{relative}"))?;
        }
    }
    content::require_private_file(&paths, &source.manifest_path())?;
    if source.placements_glb {
        content::require_private_file(&paths, &source.placements_path())?;
    }

    // 在写目标以前冻结源文件清单，避免把同名、外部重解析链接或其他场景文件带入副本。
    let entries = collect_entries(&paths, cancelled)?;
    let mut owned = OwnedFolders::new(&scenes_root);
    let result = (|| {
        progress("正在复制场景文件…");
        check_cancelled(cancelled)?;
        for root in paths.roots() {
            owned.create(&asset_root.join(&root.target))?;
        }
        let mut documents = content::Documents::new(&paths, source, &target, roles.as_deref());
        for file in entries.iter().filter(|entry| entry.directory) {
            file.create_directory(&asset_root)?;
        }
        // 按作者文档找出模型植被块。先重绑属主及内容地址，再复制引用这些块的文档。
        let model_blocks = documents.copy_model_blocks(&entries, cancelled)?;
        for (_, file) in entries.iter().enumerate().filter(|(index, entry)| !entry.directory && !model_blocks.contains(index)) {
            check_cancelled(cancelled)?;
            documents.copy_file(file, cancelled)?;
        }

        progress("正在校验场景副本…");
        for entry in &entries {
            check_cancelled(cancelled)?;
            entry.recheck(&asset_root)?;
        }
        progress("正在登记场景副本…");
        check_cancelled(cancelled)?;
        if read_file(&catalog_path)? != original_catalog {
            return Err("场景目录在复制期间发生变化，请重试".into());
        }
        // 保留原目录项中的额外数据。只用副本的已知字段覆盖它自己的克隆条目。
        let mut catalog_json: serde_json::Value = serde_json::from_slice(&original_catalog)
            .map_err(|error| error.to_string())?;
        let scenes = catalog_json["scenes"].as_array_mut().ok_or("场景目录缺少场景列表")?;
        let mut target_json = scenes[source_index].clone();
        let target_fields = serde_json::to_value(&target).map_err(|error| error.to_string())?;
        target_json.as_object_mut().ok_or("来源场景目录项无效")?
            .extend(target_fields.as_object().ok_or("副本场景目录项无效")?.clone());
        scenes.push(target_json);

        // 从此处开始将两个目录索引视为一次提交，归类先写，运行时可见的目录最后写。
        if let Some(mutation) = &folder_mutation {
            mutation.publish_checked()?;
        }
        if let Err(error) = atomic_write_json(&catalog_path, &catalog_json) {
            let rollback = folder_mutation.as_ref().and_then(|mutation| mutation.rollback().err());
            return match rollback {
                Some(rollback) => Err(format!("发布场景副本失败：{error}，恢复原归类失败：{rollback}")),
                None => Err(format!("发布场景副本失败：{error}")),
            };
        }
        Ok(CopiedScene {
            scene_id,
            display_name,
            #[cfg(test)]
            folder: target.folder,
            folder_snapshot: folder_mutation.as_ref().map(|mutation| mutation.snapshot()),
        })
    })();
    match result {
        Ok(receipt) => Ok(receipt),
        Err(error) => match owned.cleanup() {
            Ok(()) => Err(error),
            Err(cleanup) => Err(format!("{error}，清理未登记副本失败：{cleanup}")),
        },
    }
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) { Err("已取消复制场景".into()) } else { Ok(()) }
}

fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    if !ordinary(path)?.is_file() {
        return Err(format!("场景作者文件不是普通文件：{}", path.display()));
    }
    fs::read(path).map_err(|error| format!("读取 {} 失败：{error}", path.display()))
}
