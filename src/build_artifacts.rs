//! 完整运行版本的只读依赖收集、可取消复制与 SHA 清单；不接触作者文档或 Cargo 构建策略。
//! 每次发布独占目录，活动版本由浅共享租约保护，后台只回收本进程登记且已经无人持有的旧目录。

mod pe;
#[cfg(any(unix, test))]
mod unix;

use crate::build_process::BuildControl;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

const FILE_LIMIT: usize = 256;
const BYTE_LIMIT: u64 = 8 * 1024 * 1024 * 1024;
const MANIFEST_NAME: &str = "runtime-manifest.json";
const OWNER_NAME: &str = ".feathrise-runtime-owner";

/// Cargo 本次 JSON 回执提供的动态库身份，不能扫描缓存并猜测“最新”依赖。
#[derive(Clone, Debug)]
pub(crate) struct CargoDll {
    pub path: PathBuf,
    pub package_id: String,
    pub target_name: String,
    pub dylib: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeFile {
    pub source: PathBuf,
    pub relative: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct PublishedBundle {
    pub directory: PathBuf,
    pub executable: PathBuf,
    pub manifest_sha256: String,
    // 仅由后台发布/回收访问注册表；主线程克隆或释放 Arc 不执行文件 IO 或等待锁。
    _lease: Arc<()>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleManifest {
    format: u32,
    metadata: serde_json::Value,
    executable: PathBuf,
    files: Vec<BundleFile>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleFile {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

struct RegisteredBundle {
    directory: PathBuf,
    owner: String,
    group: PathBuf,
    lease: Weak<()>,
}

static BUNDLES: OnceLock<Mutex<Vec<RegisteredBundle>>> = OnceLock::new();

/// 解析当前平台的实际导入闭包；系统库留给系统加载器，工程/Rust库来自本次确定路径。
pub(crate) fn runtime_files(
    executable: &Path,
    target: &Path,
    dlls: &[CargoDll],
    search_paths: &[PathBuf],
    no_vc_redist: bool,
    control: &BuildControl,
) -> Result<Vec<RuntimeFile>, String> {
    #[cfg(unix)]
    { let _ = no_vc_redist; unix::runtime_files(executable, target, dlls, search_paths, control) }
    #[cfg(windows)]
    { runtime_files_windows(executable, target, dlls, search_paths, no_vc_redist, control) }
}

#[cfg(windows)]
fn runtime_files_windows(
    executable: &Path,
    target: &Path,
    dlls: &[CargoDll],
    search_paths: &[PathBuf],
    no_vc_redist: bool,
    control: &BuildControl,
) -> Result<Vec<RuntimeFile>, String> {
    let mut candidates = BTreeMap::new();
    for record in dlls {
        control.check()?;
        let source = record
            .path
            .canonicalize()
            .map_err(|error| format!("Cargo DLL 不存在：{error}"))?;
        if !source.starts_with(target) || !source.is_file() {
            return Err("Cargo DLL 逃出本次构建缓存".into());
        }
        let name = file_name(&source)?.to_ascii_lowercase();
        if let Some(previous) = candidates.insert(name, source.clone()) {
            if previous != source {
                return Err("本次 Cargo 存在同名不同来源 DLL".into());
            }
        }
    }
    let mut pending = vec![executable.to_owned()];
    let mut visited = BTreeSet::new();
    let mut files = BTreeMap::<String, RuntimeFile>::new();
    while let Some(binary) = pending.pop() {
        control.check()?;
        if !visited.insert(binary.clone()) {
            continue;
        }
        if visited.len() > FILE_LIMIT {
            return Err("运行依赖闭包超过 256 项".into());
        }
        for name in pe::imported_dlls(&binary, no_vc_redist)? {
            control.check()?;
            let lower = name.to_ascii_lowercase();
            // 导出不能把开发机上碰巧已安装的 VC DLL 当成系统自带库。
            if no_vc_redist && pe::is_vc_runtime(&lower) {
                return Err(format!("免装 VC Redist 的导出仍依赖运行库：{} -> {name}。请将相应原生依赖以静态 CRT 重新构建。", binary.display()));
            }
            let source = if let Some(source) = candidates.get(&lower) {
                Some(source.clone())
            } else if lower.starts_with("bevy_dylib-") {
                Some(bevy_import(&name, target, dlls, control)?)
            } else {
                let mut found = None;
                for directory in search_paths {
                    let candidate = directory.join(&name);
                    if candidate.is_file() {
                        found = Some(
                            candidate
                                .canonicalize()
                                .map_err(|error| error.to_string())?,
                        );
                        break;
                    }
                }
                found
            };
            let Some(source) = source else {
                // 只承认系统实际提供的 DLL 和 API-set 合同；漏失工程库不能静默借 PATH 的另一版。
                let system = std::env::var_os("SystemRoot")
                    .map(PathBuf::from)
                    .map(|root| root.join("System32").join(&name));
                if lower.starts_with("api-ms-win-")
                    || lower.starts_with("ext-ms-win-")
                    || system.is_some_and(|path| path.is_file())
                {
                    continue;
                }
                return Err(format!("本次构建缺少实际导入的运行库：{name}"));
            };
            if let Some(previous) = files.get(&lower) {
                if previous.source != source {
                    return Err(format!("运行库导入存在同名冲突：{name}"));
                }
                continue;
            }
            if files.len() == FILE_LIMIT {
                return Err("运行依赖超过 256 文件".into());
            }
            files.insert(
                lower,
                RuntimeFile {
                    source: source.clone(),
                    relative: PathBuf::from(name),
                },
            );
            pending.push(source);
        }
    }
    Ok(files.into_values().collect())
}

#[cfg(windows)]
fn bevy_import(
    name: &str,
    target: &Path,
    dlls: &[CargoDll],
    control: &BuildControl,
) -> Result<PathBuf, String> {
    let lower = name.to_ascii_lowercase();
    let fingerprint = lower
        .strip_prefix("bevy_dylib-")
        .and_then(|name| name.strip_suffix(".dll"));
    if !fingerprint.is_some_and(|value| {
        value.len() == 16 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) {
        return Err(format!("未识别的 Bevy DLL 导入名：{name}"));
    }
    let mut records = dlls.iter().filter(|record| {
        record.dylib
            && record.target_name == "bevy_dylib"
            && file_name(&record.path).is_ok_and(|name| name.eq_ignore_ascii_case("bevy_dylib.dll"))
            && record
                .package_id
                .rsplit_once('#')
                .is_some_and(|(_, package)| package.starts_with("bevy_dylib@"))
    });
    let record = records
        .next()
        .ok_or("本次 Cargo 未报告 Bevy 导入的精确 DLL 或无 hash 副本")?;
    if records.next().is_some() {
        return Err("本次 Cargo 报告多个 Bevy DLL，拒绝猜测版本".into());
    }
    let profile = record.path.parent().ok_or("Bevy DLL 缺少 profile 目录")?;
    let deps = if profile.file_name().is_some_and(|name| name == "deps") {
        profile.to_owned()
    } else {
        profile.join("deps")
    };
    let imported = deps
        .join(name)
        .canonicalize()
        .map_err(|error| format!("Bevy 精确导入不存在：{error}"))?;
    if !imported.starts_with(target)
        || hash_file(&imported, control)?.0 != hash_file(&record.path, control)?.0
    {
        return Err("Bevy 精确导入与本次 Cargo 副本身份不同".into());
    }
    Ok(imported)
}

pub(crate) fn publish(
    target: &Path,
    storage_key: &str,
    executable: &Path,
    extra_files: &[RuntimeFile],
    metadata: serde_json::Value,
    control: &BuildControl,
) -> Result<PublishedBundle, String> {
    control.check()?;
    if storage_key.is_empty()
        || !storage_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err("运行版本的工程缓存键无效".into());
    }
    let target = target
        .canonicalize()
        .map_err(|error| format!("运行版本缓存根无效：{error}"))?;
    let versions = target.join("feathrise-runtime-versions");
    fs::create_dir_all(&versions).map_err(|error| format!("无法准备运行版本目录：{error}"))?;
    let versions = versions.canonicalize().map_err(|error| error.to_string())?;
    contained_cache_directory(&versions, &target)?;
    // 逐级核验后才创建下一层，旧 junction 不能让检查之前的 create_dir_all 在目标外写入工程目录。
    let group = versions.join(storage_key);
    fs::create_dir_all(&group).map_err(|error| format!("无法准备工程运行版本目录：{error}"))?;
    let group = group.canonicalize().map_err(|error| error.to_string())?;
    contained_cache_directory(&group, &target)?;
    let owner = format!(
        "{}-{}",
        std::process::id(),
        bevy::asset::uuid::Uuid::new_v4()
    );
    let directory = group.join(&owner);
    fs::create_dir(&directory).map_err(|error| format!("无法创建独占运行版本：{error}"))?;
    fs::write(directory.join(OWNER_NAME), owner.as_bytes()).map_err(|error| error.to_string())?;
    let staged = (|| {
        let relative_executable = PathBuf::from(file_name(executable)?);
        let mut inputs = vec![RuntimeFile {
            source: executable.to_owned(),
            relative: relative_executable.clone(),
        }];
        inputs.extend_from_slice(extra_files);
        if inputs.len() > FILE_LIMIT {
            return Err("完整运行版本超过 256 文件".into());
        }
        let mut names = BTreeSet::new();
        let mut files = Vec::new();
        let mut bytes = 0u64;
        for input in inputs {
            control.check()?;
            validate_relative(&input.relative)?;
            let name = input.relative.to_string_lossy();
            #[cfg(windows)]
            let name = name.to_ascii_lowercase();
            #[cfg(not(windows))]
            let name = name.into_owned();
            if !names.insert(name) {
                return Err("运行版本包含重复文件名".into());
            }
            let source_hash = hash_file(&input.source, control)?;
            bytes = bytes.checked_add(source_hash.1).ok_or("运行版本大小溢出")?;
            if bytes > BYTE_LIMIT {
                return Err("完整运行版本超过 8 GiB".into());
            }
            let destination = directory.join(&input.relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|error| error.to_string())?;
            }
            copy_file(&input.source, &destination, control)?;
            if hash_file(&destination, control)? != source_hash
                || hash_file(&input.source, control)? != source_hash
            {
                return Err("运行产物在复制期间变化，拒绝发布混合版本".into());
            }
            files.push(BundleFile {
                path: input.relative,
                bytes: source_hash.1,
                sha256: source_hash.0,
            });
        }
        let manifest = BundleManifest {
            format: 1,
            metadata,
            executable: relative_executable.clone(),
            files,
        };
        let bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
        if bytes.len() > 128 * 1024 {
            return Err("运行版本清单超过 128 KiB".into());
        }
        // 清单最后落盘；目录不会先作为半成品发布给任何启动器。
        fs::write(directory.join(MANIFEST_NAME), &bytes).map_err(|error| error.to_string())?;
        control.check()?;
        Ok((directory.join(relative_executable), digest(&bytes)))
    })();
    let (executable, manifest_sha256) = match staged {
        Ok(value) => value,
        Err(error) => {
            let _ = remove_owned(&directory, &group, &owner);
            return Err(error);
        }
    };
    let lease = Arc::new(());
    let mut registry = BUNDLES
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .map_err(|_| "运行版本登记锁异常")?;
    registry.push(RegisteredBundle {
        directory: directory.clone(),
        owner,
        group,
        lease: Arc::downgrade(&lease),
    });
    reclaim(&mut registry);
    Ok(PublishedBundle {
        directory,
        executable,
        manifest_sha256,
        _lease: lease,
    })
}

pub(crate) fn verify(bundle: &PublishedBundle, control: &BuildControl) -> Result<(), String> {
    control.check()?;
    let bytes = read_bounded(&bundle.directory.join(MANIFEST_NAME), 128 * 1024)?;
    if digest(&bytes) != bundle.manifest_sha256 {
        return Err("运行版本清单身份变化，拒绝启动".into());
    }
    let manifest: BundleManifest =
        serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if manifest.format != 1
        || manifest.files.is_empty()
        || manifest.files.len() > FILE_LIMIT
        || bundle.directory.join(&manifest.executable) != bundle.executable
    {
        return Err("运行版本清单格式或入口无效".into());
    }
    for file in manifest.files {
        validate_relative(&file.path)?;
        let actual = hash_file(&bundle.directory.join(file.path), control)?;
        if actual != (file.sha256, file.bytes) {
            return Err("完整运行版本文件身份变化，拒绝启动".into());
        }
    }
    Ok(())
}

/// 自动迭代在版本交付前补入源码与构建上下文；已有共享租约的运行包不允许改清单。
pub(crate) fn bind_metadata(
    bundle: &mut PublishedBundle,
    metadata: serde_json::Value,
    control: &BuildControl,
) -> Result<(), String> {
    control.check()?;
    if Arc::strong_count(&bundle._lease) != 1 {
        return Err("已交付运行版本不能修改构建身份".into());
    }
    verify(bundle, control)?;
    let path = bundle.directory.join(MANIFEST_NAME);
    let mut manifest: BundleManifest = serde_json::from_slice(&read_bounded(&path, 128 * 1024)?)
        .map_err(|error| error.to_string())?;
    manifest.metadata = metadata;
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(|error| error.to_string())?;
    if bytes.len() > 128 * 1024 {
        return Err("运行版本清单超过 128 KiB".into());
    }
    fs::write(path, &bytes).map_err(|error| error.to_string())?;
    bundle.manifest_sha256 = digest(&bytes);
    control.check()
}

/// 只在后台调用；每工程保留最新与前一版本，所有活动 Arc 的版本继续保留。
pub(crate) fn reclaim_unused() {
    if let Some(registry) = BUNDLES.get() {
        if let Ok(mut registry) = registry.lock() {
            reclaim(&mut registry);
        }
    }
}

fn reclaim(registry: &mut Vec<RegisteredBundle>) {
    let mut kept = BTreeMap::<PathBuf, usize>::new();
    let mut remove = BTreeSet::new();
    for (index, bundle) in registry.iter().enumerate().rev() {
        let count = kept.entry(bundle.group.clone()).or_default();
        *count += 1;
        if *count > 2
            && bundle.lease.strong_count() == 0
            && remove_owned(&bundle.directory, &bundle.group, &bundle.owner).is_ok()
        {
            remove.insert(index);
        }
    }
    let mut index = 0;
    registry.retain(|_| {
        let retain = !remove.contains(&index);
        index += 1;
        retain
    });
}

fn remove_owned(directory: &Path, group: &Path, owner: &str) -> Result<(), String> {
    let canonical = directory
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if canonical.parent() != Some(group)
        || canonical.file_name().is_none_or(|name| name != owner)
        || fs::read(canonical.join(OWNER_NAME)).map_err(|error| error.to_string())?
            != owner.as_bytes()
    {
        return Err("运行版本回收归属检查失败".into());
    }
    fs::remove_dir_all(canonical).map_err(|error| format!("运行版本仍被占用，保留目录：{error}"))
}

fn contained_cache_directory(directory: &Path, target: &Path) -> Result<(), String> {
    if directory == target || !directory.starts_with(target) {
        return Err("运行版本目录真实路径逃出本次构建缓存，拒绝发布或回收".into());
    }
    Ok(())
}

fn validate_relative(path: &Path) -> Result<(), String> {
    if path.components().count() > 2
        || path.components().next().is_none()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || path == Path::new(MANIFEST_NAME)
        || path == Path::new(OWNER_NAME)
    {
        return Err("运行版本相对路径无效".into());
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path, control: &BuildControl) -> Result<(), String> {
    let mut source = File::open(source).map_err(|error| error.to_string())?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| error.to_string())?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        control.check()?;
        let size = source
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if size == 0 {
            break;
        }
        destination
            .write_all(&buffer[..size])
            .map_err(|error| error.to_string())?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // 独立运行副本必须保留可执行位；仅复制普通权限，不传播setuid/setgid等特殊位。
        let mode = source.metadata().map_err(|error| error.to_string())?.permissions().mode() & 0o777;
        destination.set_permissions(fs::Permissions::from_mode(mode)).map_err(|error| error.to_string())?;
    }
    destination.sync_all().map_err(|error| error.to_string())
}

pub(crate) fn hash_file(path: &Path, control: &BuildControl) -> Result<(String, u64), String> {
    let mut file =
        File::open(path).map_err(|error| format!("运行产物不可读：{}：{error}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        control.check()?;
        let size = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if size == 0 {
            break;
        }
        bytes = bytes.checked_add(size as u64).ok_or("运行产物大小溢出")?;
        if bytes > BYTE_LIMIT {
            return Err("单个运行产物超过 8 GiB".into());
        }
        hasher.update(&buffer[..size]);
    }
    Ok((
        hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        bytes,
    ))
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|error| error.to_string())?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err("运行清单超过固定容量".into());
    }
    Ok(bytes)
}

fn file_name(path: &Path) -> Result<&str, String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "运行产物文件名不是有效 Unicode".into())
}
pub(crate) fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_paths_reject_escape_and_manifest_overwrite() {
        for path in [
            "../outside.dll",
            "runtime-manifest.json",
            ".feathrise-runtime-owner",
            "a/b/c.dll",
            "C:/outside.dll",
        ] {
            assert!(validate_relative(Path::new(path)).is_err(), "{path}");
        }
        for path in [
            "game.exe",
            "std-fixture.dll",
            "streamline-runtime/sl.common.dll",
        ] {
            validate_relative(Path::new(path)).unwrap();
        }
    }

    #[test]
    fn cache_directory_boundary_rejects_sibling_prefix_and_target_root() {
        let target = Path::new("D:/owned/target");
        contained_cache_directory(
            Path::new("D:/owned/target/feathrise-runtime-versions"),
            target,
        )
        .unwrap();
        for path in [
            "D:/owned/outside",
            "D:/owned/target-other/cache",
            "D:/owned/target",
        ] {
            assert!(
                contained_cache_directory(Path::new(path), target).is_err(),
                "{path}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn redirected_runtime_or_project_directory_cannot_publish_outside_target() {
        let root = std::env::temp_dir().join(format!(
            "feathrise-own-redirect-{}",
            bevy::asset::uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        let outcome = (|| -> Result<(), String> {
            for (index, group_redirect) in [false, true].into_iter().enumerate() {
                let target = root.join(format!("target-{index}"));
                let outside = root.join(format!("outside-{index}"));
                fs::create_dir(&target).map_err(|error| error.to_string())?;
                fs::create_dir(&outside).map_err(|error| error.to_string())?;
                let executable = target.join("game.exe");
                fs::write(&executable, b"owned-executable").map_err(|error| error.to_string())?;
                let versions = target.join("feathrise-runtime-versions");
                let redirect = if group_redirect {
                    fs::create_dir(&versions).map_err(|error| error.to_string())?;
                    versions.join("owned-test")
                } else {
                    versions
                };
                if let Err(error) = std::os::windows::fs::symlink_dir(&outside, &redirect) {
                    if error.kind() == std::io::ErrorKind::PermissionDenied
                        || error.raw_os_error() == Some(1314)
                    {
                        eprintln!(
                            "当前 Windows 未授予符号链接权限，目录重定向实物用例未执行：{error}"
                        );
                        return Ok(());
                    }
                    return Err(error.to_string());
                }
                let result = publish(
                    &target,
                    "owned-test",
                    &executable,
                    &[],
                    serde_json::json!({}),
                    &BuildControl::default(),
                );
                let untouched = fs::read_dir(&outside)
                    .map_err(|error| error.to_string())?
                    .next()
                    .is_none();
                fs::remove_dir(&redirect).map_err(|error| error.to_string())?;
                assert!(result.unwrap_err().contains("逃出本次构建缓存"));
                assert!(untouched, "边界检查前不得向重定向目标创建任何内容");
            }
            Ok(())
        })();
        fs::remove_dir_all(root).unwrap();
        outcome.unwrap();
    }

    #[test]
    fn immutable_copy_detects_tamper_and_keeps_running_version_bytes_separate() {
        let root = std::env::temp_dir().join(format!(
            "feathrise-own-bundle-{}",
            bevy::asset::uuid::Uuid::new_v4()
        ));
        fs::create_dir(&root).unwrap();
        let exe = root.join("game.exe");
        fs::write(&exe, b"version-one").unwrap();
        let control = BuildControl::default();
        let bundle = publish(
            &root,
            "owned-test",
            &exe,
            &[],
            serde_json::json!({"test":true}),
            &control,
        )
        .unwrap();
        verify(&bundle, &control).unwrap();
        fs::write(&exe, b"version-two").unwrap();
        assert_eq!(fs::read(&bundle.executable).unwrap(), b"version-one");
        fs::write(&bundle.executable, b"tampered").unwrap();
        assert!(verify(&bundle, &control).unwrap_err().contains("身份变化"));
        drop(bundle);
        fs::remove_dir_all(root).unwrap();
    }
}
