//! 由现有 Bevy 目录资源读取发布模板，仅在新建工程的后台槽执行。
use super::{BuildControl, CopyBudget};
use bevy::{asset::io::{AssetReader, AssetReaderError}, tasks::{block_on, futures_lite::StreamExt}};
use bevy_embedded_assets::EmbeddedAssetReader;
use std::{fs, path::Path};

pub(super) const DIRECTORY: &str = "templates/empty";

pub(super) fn copy(root: &Path, control: &BuildControl) -> Result<Option<(String, Vec<u8>)>, String> {
    let reader = EmbeddedAssetReader::preloaded();
    let template = Path::new(DIRECTORY);
    let read_error = |error| format!("无法读取发行模板：{error}");
    block_on(async {
        let mut paths = match reader.read_directory(template).await {
            Ok(paths) => paths,
            Err(AssetReaderError::NotFound(_)) => return Ok(None),
            Err(error) => return Err(read_error(error)),
        };
        let manifest_path = template.join("Cargo.toml");
        let lock_path = template.join("Cargo.lock");
        let manifest = reader.load_path_sync(&manifest_path).map_err(read_error)?.0;
        let lock = reader.load_path_sync(&lock_path).map_err(read_error)?.0;
        let mut budget = CopyBudget::default();
        while let Some(path) = paths.next().await {
            control.check()?;
            let bytes = reader.load_path_sync(&path).map_err(read_error)?.0;
            budget.add_file(bytes.len() as u64)?;
            // 清单需要按工程身份改写，锁与生成配置由共用的新建入口写入。
            if path == manifest_path || path == lock_path {
                continue;
            }
            let relative = path.strip_prefix(template).map_err(|error| error.to_string())?;
            let target = root.join(relative);
            fs::create_dir_all(target.parent().ok_or("模板文件目标缺少父目录")?)
                .map_err(|error| error.to_string())?;
            fs::write(&target, bytes).map_err(|error| format!("无法写入模板文件 {}：{error}", relative.display()))?;
        }
        let manifest = std::str::from_utf8(manifest).map_err(|error| error.to_string())?;
        Ok(Some((manifest.to_owned(), lock.to_vec())))
    })
}
