//! 编译资源在创建窗口前释放到用户缓存；工程资产始终走工程自己的 AssetReader。
//! 缓存按内容身份分代并原子发布，SDK 配置仍从 EXE 旁读取，不能被缓存改变相对路径。

use super::*;
use bevy_embedded_assets::EmbeddedAssetReader;
use sha2::{Digest, Sha256};
use std::io::Write;

include!(concat!(env!("OUT_DIR"), "/resource_identity.rs"));

pub(super) fn retain_identity() {
    std::hint::black_box(RESOURCE_IDENTITY);
}

fn payload() -> Result<Vec<(String, &'static [u8])>, String> {
    let reader = EmbeddedAssetReader::preloaded();
    let mut files = required_resource_files()?;
    files.sort_unstable();
    let mut identity = Sha256::new();
    let mut payload = Vec::with_capacity(files.len());
    for name in files {
        let bytes = if name == "Cargo.lock" {
            include_bytes!("../../Cargo.lock").as_slice()
        } else {
            let relative = name.strip_prefix("assets/").ok_or("内嵌资源必须位于 assets 内")?;
            reader.load_path_sync(Path::new(relative))
                .map_err(|e| format!("编译内嵌资源缺失 {name}：{e}；请检查 BEVY_ASSET_PATH"))?.0
        };
        identity.update(name.as_bytes());
        identity.update([0]);
        identity.update(Sha256::digest(bytes));
        payload.push((name, bytes));
    }
    let identity: String = identity.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    if identity != RESOURCE_ID {
        return Err("内嵌资源与编译身份不一致；请使用编辑器 assets 目录重新构建".into());
    }
    Ok(payload)
}

pub(super) fn materialize() -> Result<PathBuf, String> {
    let variable = if cfg!(windows) { "LOCALAPPDATA" } else { "XDG_CACHE_HOME" };
    let base = match std::env::var_os(variable) {
        Some(value) => PathBuf::from(value),
        None => crate::writable_paths::user_home()?.join(if cfg!(windows) { "AppData/Local" } else { ".cache" }),
    };
    crate::writable_paths::validate_absolute(&base, variable)?;
    materialize_at(&base.join("Feathrise/resources"))
}

fn materialize_at(base: &Path) -> Result<PathBuf, String> {
    let payload = payload()?;
    fs::create_dir_all(base).map_err(|e| format!("无法创建作者资源缓存 {}：{e}", base.display()))?;
    let base = base.canonicalize().map_err(|e| e.to_string())?;
    let destination = base.join(RESOURCE_ID);
    if destination.try_exists().map_err(|e| e.to_string())? {
        verify(&destination, &payload)?;
        return Ok(destination);
    }
    // 只认领本次 create_dir 成功的唯一临时目录；两个实例同时启动时只采用完整胜出版本。
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?.as_nanos();
    let temporary = base.join(format!(".{RESOURCE_ID}-{}-{stamp}", std::process::id()));
    fs::create_dir(&temporary).map_err(|e| e.to_string())?;
    let result = (|| {
        for (name, bytes) in &payload {
            let path = temporary.join(name);
            fs::create_dir_all(path.parent().ok_or("资源路径缺少父目录")?).map_err(|e| e.to_string())?;
            let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&path)
                .map_err(|e| format!("无法释放作者资源 {name}：{e}"))?;
            file.write_all(bytes).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
        }
        verify(&temporary, &payload)?;
        match fs::rename(&temporary, &destination) {
            Ok(()) => Ok(destination.clone()),
            Err(_) if destination.is_dir() => {
                verify(&destination, &payload)?;
                Ok(destination.clone())
            }
            Err(e) => Err(format!("无法发布作者资源缓存：{e}")),
        }
    })();
    // 不碰其他内容身份的缓存，也不清理已有 target 或用户工程。
    if temporary.parent() == Some(base.as_path()) && temporary.is_dir() {
        let _ = fs::remove_dir_all(&temporary);
    }
    result
}

fn verify(root: &Path, payload: &[(String, &[u8])]) -> Result<(), String> {
    let canonical = root.canonicalize().map_err(|e| e.to_string())?;
    for (name, expected) in payload {
        let path = root.join(name);
        let valid = (|| -> std::io::Result<bool> {
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.is_file() || metadata.len() != expected.len() as u64
                || !path.canonicalize()?.starts_with(&canonical) { return Ok(false); }
            Ok(Sha256::digest(fs::read(&path)?) == Sha256::digest(expected))
        })().unwrap_or(false);
        if !valid {
            return Err(format!("作者资源缓存损坏：{}；请移除此内容版本的缓存目录 {} 后重启，工程数据不在此目录", name, root.display()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_payload_survives_relocation_and_detects_cache_corruption() {
        let fixture = super::super::tests::Fixture::new();
        let root = materialize_at(&fixture.0).unwrap();
        assert!(root.join("assets/templates/empty/src/main.rs").is_file());
        // 安装版不带开发仓库；热更新必须取得与当前 EXE 同一内容身份的链接代理源码。
        assert_eq!(fs::read(root.join("assets/hotpatch/原生热补丁链接代理.rs")).unwrap(),
            include_bytes!("../../assets/hotpatch/原生热补丁链接代理.rs"));
        assert!(!root.join("sdk-source.toml").exists());
        assert!(!root.join("assets/fonts").exists());
        assert_eq!(materialize_at(&fixture.0).unwrap(), root);
        fs::write(root.join("assets/templates/empty/src/main.rs"), b"corrupt").unwrap();
        assert!(materialize_at(&fixture.0).unwrap_err().contains("缓存损坏"));
    }
    #[test]
    fn concurrent_materialization_adopts_one_complete_payload() {
        let fixture = super::super::tests::Fixture::new();
        std::thread::scope(|scope| {
            let first = scope.spawn(|| materialize_at(&fixture.0));
            let second = scope.spawn(|| materialize_at(&fixture.0));
            assert_eq!(first.join().unwrap().unwrap(), second.join().unwrap().unwrap());
        });
    }
}
