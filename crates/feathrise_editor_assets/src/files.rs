//! 作者文件安全写入辅助函数。

use std::io::Write; // 临时文件完整写入和落盘。
use std::path::{Component, Path, PathBuf}; // 资产根约束与同目录原子替换。
use std::sync::atomic::{AtomicU64, Ordering}; // 同进程临时文件名不会碰撞。

use feathrise_scene_content::{SCENE_SCHEMA_VERSION, SceneCatalogAsset}; // 创建与删除共用同一目录版本校验。

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0); // 只用于临时文件唯一后缀。

/// glTF URI 使用百分号编码。缩略图依赖与组合导出共享同一解码，磁盘边界由调用方核对。
pub fn decode_gltf_uri(uri: &str) -> Result<String, String> {
    let mut bytes = uri.bytes();
    let mut decoded = Vec::with_capacity(uri.len());
    while let Some(b) = bytes.next() {
        if b == b'%' {
            let mut digit = || bytes.next().and_then(|v| (v as char).to_digit(16)).ok_or("模型依赖路径编码无效");
            decoded.push((digit()? * 16 + digit()?) as u8);
        } else { decoded.push(b); }
    }
    String::from_utf8(decoded).map_err(|_| "模型依赖路径编码无效".into())
}

/// 返回当前作者工程assets绝对路径；一体入口未绑定工程时沿用SDK工作区。
pub fn workspace_asset_root() -> PathBuf {
    feathrise_editor_core::workspace_asset_root()
}

/// 返回当前工作区场景作者目录。
pub fn workspace_scenes_root() -> PathBuf {
    workspace_asset_root().join("scenes")
}

/// 将正斜杠相对作者路径约束在assets根内；不接受绝对路径或父级跳转。
pub fn safe_asset_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute()
        || relative.contains('\\')
        || relative.contains('\0')
        || relative.split('/').any(|component| {
            component.is_empty()
                || component == "."
                || component == ".."
                || component.contains(':')
                || component.ends_with('.')
                || component.ends_with(' ')
        })
        || path.components().any(|part| {
            matches!(
                part,
                Component::CurDir
                    | Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
            )
        })
    {
        return Err(format!("场景作者路径必须位于assets内：{relative}"));
    }
    Ok(root.join(path))
}

/// 保存零写入预检使用的跨Windows/Linux词法路径键；输入必须先通过`safe_asset_path`。
pub fn asset_path_collision_key(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase() // Windows默认大小写不敏感，不能让大小写别名绕过覆盖检查。
}

/// 约束水域作者源和派生纹理只能写入当前目录条目的私有`water/`子目录。
pub fn safe_water_asset_path(
    root: &Path,
    scene_folder: &str,
    relative: &str,
) -> Result<PathBuf, String> {
    let path = safe_asset_path(root, relative)?; // 先拒绝绝对路径、父级跳转与词法别名。
    let folder = scene_folder.trim_end_matches('/');
    let expected_prefix = format!("{folder}/water/").to_lowercase();
    if folder.is_empty() || !relative.to_lowercase().starts_with(&expected_prefix) {
        return Err(format!(
            "水域作者路径必须位于当前场景私有目录{folder}/water/：{relative}"
        )); // 防止旧文件或手工JSON把场景A的保存事务指向场景B。
    }
    Ok(path)
}

/// 约束目录中可自定义文件名的权威JSON仍属于当前场景私有目录。
pub fn safe_scene_private_asset_path(
    root: &Path,
    scene_folder: &str,
    relative: &str,
) -> Result<PathBuf, String> {
    let path = safe_asset_path(root, relative)?;
    let folder = scene_folder.trim_end_matches('/');
    let expected_prefix = format!("{folder}/").to_lowercase();
    if folder.is_empty() || !relative.to_lowercase().starts_with(&expected_prefix) {
        return Err(format!(
            "场景权威路径必须位于当前私有目录{folder}/：{relative}"
        ));
    }
    Ok(path)
}

/// 从UTF-8字节解析并校验场景目录版本，供创建和可恢复删除事务复用。
pub fn decode_scene_catalog(path: &Path, bytes: &[u8]) -> Result<SceneCatalogAsset, String> {
    let catalog: SceneCatalogAsset = serde_json::from_slice(bytes)
        .map_err(|error| format!("解析场景目录{}失败：{error}", path.display()))?; // 损坏目录绝不能被后续写入覆盖。
    if catalog.schema_version != SCENE_SCHEMA_VERSION {
        return Err(format!(
            "场景目录版本{}与编辑器版本{}不一致",
            catalog.schema_version, SCENE_SCHEMA_VERSION
        )); // 不兼容版本必须先迁移，禁止静默重写。
    }
    Ok(catalog) // 返回经过统一格式版本门的完整目录。
}

/// 写入同目录临时文件并原子替换目标，避免AssetServer读到半份JSON或GLB。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    write_staged(path, bytes, true, || Ok(()))
}

/// 作者另存使用独占发布；并发创建同名输出时保留先到达的文件。
pub fn atomic_create(path: &Path, bytes: &[u8], before_publish: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    write_staged(path, bytes, false, before_publish)
}

pub use feathrise_scene_content::authoring_io::file_hash as destination_hash;

/// 副本由作者确认覆盖时，在正式事务最终发布前再次核验作者看到的磁盘版本。
pub fn replace_confirmed(path: &Path, bytes: &[u8], baseline: &str, before_publish: impl Fn() -> Result<(), String>) -> Result<(), String> {
    use feathrise_scene_content::authoring_transaction::{PreparedWrite, commit_prepared_writes_checked};
    commit_prepared_writes_checked(vec![PreparedWrite::borrowed(path.to_owned(), bytes)], |_, _| {
        if destination_hash(path)? != baseline { return Err("目标文件已经改变，请重新确认覆盖".into()); }
        before_publish()
    }).map_err(|error| error.message().to_string())
}

fn write_staged(path: &Path, bytes: &[u8], replace: bool, before_publish: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("作者文件没有父目录：{}", path.display()))?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("创建作者目录{}失败：{error}", parent.display()))?;
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("作者文件名不是有效Unicode：{}", path.display()))?;
    let temporary = parent.join(format!(".{file_name}.tmp-{}-{counter}", std::process::id()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)
        .map_err(|error| format!("创建临时作者文件{}失败：{error}", temporary.display()))?;
    let write_result = (|| {
        file.write_all(bytes)
            .map_err(|error| format!("写入临时作者文件{}失败：{error}", temporary.display()))?;
        file.sync_all()
            .map_err(|error| format!("刷新临时作者文件{}失败：{error}", temporary.display()))?;
        drop(file);
        before_publish()?; // 完整暂存就绪后才检查授权，取消不发布同名目标。
        publish_staged_file(&temporary, path, replace).map_err(|error| format!("发布{}失败：{error}", path.display()))
    })();
    // Unix 独占发布使用硬链接，成功后也要释放由本次 create_new 确认归属的暂存目录项。
    let _ = std::fs::remove_file(&temporary);
    write_result
}

/// 以UTF-8漂亮格式写JSON并补终止换行。
pub fn atomic_write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("序列化{}失败：{error}", path.display()))?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

#[cfg(windows)]
pub fn publish_staged_file(source: &Path, destination: &Path, replace: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt; // Windows API要求NUL结尾UTF-16路径。
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            (if replace { MOVEFILE_REPLACE_EXISTING } else { 0 }) | MOVEFILE_WRITE_THROUGH,
        )
    };
    (result != 0).then_some(()).ok_or_else(std::io::Error::last_os_error)
}

#[cfg(not(windows))]
pub fn publish_staged_file(source: &Path, destination: &Path, replace: bool) -> std::io::Result<()> {
    if replace { std::fs::rename(source, destination) } else { std::fs::hard_link(source, destination) }
}
