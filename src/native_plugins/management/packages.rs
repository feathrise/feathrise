//! 单文件插件的后台存储事务；全局版本对象不可变，工程副本优先，绝不改写作者资产。
use super::types::*;
use feathrise_plugin_api::{
    ABI_MAJOR, MAX_PLUGINS, read_plugin_metadata, validate_current_platform,
};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_JSON: usize = 128 * 1024;
const MAX_ENTRIES: usize = 256;
const MAX_BYTES: u64 = 1024 * 1024 * 1024;
const JOURNAL_FORMAT: u32 = 2;
static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);

#[path = "legacy_archive.rs"]
mod legacy_archive;

pub(crate) fn legacy_diagnostic_origin(entry: &PackageEntry) -> Option<PluginOrigin> {
    legacy_archive::diagnostic_origin(entry).ok()
}

#[derive(Clone)]
struct Store {
    scope: PluginScope,
    plugins: PathBuf,
    manager: PathBuf,
}
struct JournalRead {
    journal: Journal,
    legacy: Option<serde_json::Value>,
}

/// 安装只解析数据，不调用来源二进制；外平台和伪造后缀都在静态检查时拒绝。
pub(crate) fn inspect_package(path: &Path) -> Result<(PackageManifest, PathBuf), String> {
    ordinary_chain(path)?;
    if !ordinary(path)?.is_file() {
        return Err("插件必须是单个 .dll、.so 或 .dylib 文件；旧目录包已停用".into());
    }
    let path = path.canonicalize().map_err(error)?;
    if ordinary(&path)?.len() > MAX_BYTES {
        return Err("插件文件超过1GiB预算".into());
    }
    let metadata = read_plugin_metadata(&path)?;
    validate_current_platform(&metadata)?;
    Ok((metadata, path))
}
pub(crate) fn global_plugins_root() -> Result<PathBuf, String> {
    #[cfg(any(test, feature = "validation"))]
    if let Some(path) = std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS") {
        let path = PathBuf::from(path);
        let owner = path.join(".feathrise-plugin-fixture-owner");
        if !path.is_absolute()
            || !ordinary(&path)?.is_dir()
            || read_limited(&owner, 128)?.as_slice() != b"feathrise-single-binary-validation-v1"
        {
            return Err("验证全局插件根缺少本轮夹具归属标记".into());
        }
        return Ok(path);
    }
    // 真实作者验收缺隔离根时必须拒绝，而不能在发现线程里采用用户待生效插件。
    // 普通交互式validation构建没有这些入口开关，仍按当前用户规则定位。
    #[cfg(feature = "validation")]
    if validation_editor_requested(|name| std::env::var_os(name)) {
        return Err("真实作者验收未设置隔离全局插件目录，拒绝读取或采用用户插件变更".into());
    }
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE");
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME");
    let home = home
        .filter(|v| !v.is_empty())
        .ok_or("无法确定当前用户主目录，不能定位全局插件库")?;
    Ok(PathBuf::from(home).join(".feathrise/plugins"))
}

#[cfg(any(test, feature = "validation"))]
fn validation_editor_requested(mut value: impl FnMut(&str) -> Option<std::ffi::OsString>) -> bool {
    // 仅列出实际安装作者/工作台自动操作的入口，工具路径、父窗口和普通工程环境不算验收。
    const ENTRIES: &[&str] = &[
        "NEW_EDITOR_NATIVE_SMOKE_DIR", "FEATHRISE_EMBEDDED_SMOKE_DIR",
        "FEATHRISE_CODE_ITERATION_REPORT", "FEATHRISE_EXPORT_VALIDATION_REPORT",
        "FEATHRISE_SCENE_FORMAT_VALIDATION",
        "FEATHRISE_EMBEDDED_BUILD_CANCEL_DIR", "FEATHRISE_WORKSPACE_SMOKE_DIR",
        "FEATHRISE_HOME_SCALE_VALIDATION_DIR", "FEATHRISE_ABOUT_VALIDATION_DIR",
        "FEATHRISE_CHROME_VALIDATION_DIR", "FEATHRISE_FRAME_VALIDATION_DIR",
        "FEATHRISE_WINDOW_PLACEMENT_VALIDATION_DIR",
        "FEATHRISE_PLUGIN_HOST_VALIDATION_DIR",
        "FEATHRISE_BUILD_ENVIRONMENT_VALIDATION_DIR",
        "FEATHRISE_RIG_VALIDATION_DIR",
        "FEATHRISE_PROJECT_BROWSER_VALIDATION",
        "FEATHRISE_FILE_MANAGER_VALIDATION",
    ];
    ENTRIES.iter().any(|name| value(name).is_some())
}

#[cfg(test)]
mod validation_environment_tests {
    use super::validation_editor_requested;
    use std::ffi::OsString;

    #[test]
    fn known_live_acceptance_entries_require_an_isolated_store_even_when_the_value_is_empty() {
        for active in ["NEW_EDITOR_NATIVE_SMOKE_DIR", "FEATHRISE_EMBEDDED_SMOKE_DIR",
            "FEATHRISE_CODE_ITERATION_REPORT", "FEATHRISE_EXPORT_VALIDATION_REPORT",
            "FEATHRISE_SCENE_FORMAT_VALIDATION",
            "FEATHRISE_EMBEDDED_BUILD_CANCEL_DIR", "FEATHRISE_WORKSPACE_SMOKE_DIR",
            "FEATHRISE_HOME_SCALE_VALIDATION_DIR", "FEATHRISE_ABOUT_VALIDATION_DIR",
            "FEATHRISE_CHROME_VALIDATION_DIR", "FEATHRISE_FRAME_VALIDATION_DIR",
            "FEATHRISE_WINDOW_PLACEMENT_VALIDATION_DIR",
            "FEATHRISE_PLUGIN_HOST_VALIDATION_DIR",
            "FEATHRISE_BUILD_ENVIRONMENT_VALIDATION_DIR",
            "FEATHRISE_RIG_VALIDATION_DIR", "FEATHRISE_PROJECT_BROWSER_VALIDATION", "FEATHRISE_FILE_MANAGER_VALIDATION"] {
            assert!(validation_editor_requested(|name| (name == active).then(OsString::new)), "{active}");
        }
    }

    #[test]
    fn normal_projects_and_tool_configuration_do_not_activate_the_acceptance_guard() {
        let normal = ["FEATHRISE_WORKSPACE_PARENT_PID", "FEATHRISE_WORKSPACE_CANDIDATE",
            "FEATHRISE_DX_PATH", "FEATHRISE_PREFERENCES_FILE", "FEATHRISE_CODE_ITERATION_FUNCTION_HOTPATCH"];
        assert!(!validation_editor_requested(|name| normal.contains(&name).then(|| OsString::from("configured"))));
        assert!(!validation_editor_requested(|_| None));
    }
}
pub(crate) fn roots_for_project(root: &Path) -> Result<PluginRoots, String> {
    #[cfg(test)]
    let global = if std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS").is_none() {
        root.join(".editor/test-global-plugins")
    } else {
        global_plugins_root()?
    };
    #[cfg(not(test))]
    let global = global_plugins_root()?;
    Ok(PluginRoots {
        project: Some(root.to_path_buf()),
        global,
    })
}
pub(crate) fn binary_filename(id: &str) -> String {
    format!("{id}.{}", library_extension())
}
fn library_extension() -> &'static str {
    match std::env::consts::OS {
        "windows" => "dll",
        "macos" => "dylib",
        _ => "so",
    }
}
fn candidate(path: &Path) -> bool {
    path.extension()
        .and_then(|v| v.to_str())
        .is_some_and(|v| ["dll", "so", "dylib"].contains(&v))
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
// 旧工程根便捷入口只用于隔离回归；生产始终显式传递工程/全局来源和作用域。
#[cfg(test)]
pub(crate) fn disabled_ids(root: &Path) -> Result<BTreeSet<String>, String> {
    let roots = normalize_roots(&roots_for_project(root)?)?;
    Ok(read_settings(&store_for(&roots, PluginScope::Project, false)?)?.disabled)
}
#[cfg(test)]
pub(crate) fn catalog(root: &Path) -> Result<Catalog, String> {
    catalog_at(&roots_for_project(root)?, PluginScope::Project)
}

/// 工程同ID副本即使损坏或停用也占位，不能静默降级到全局另一个版本。
pub(crate) fn catalog_at(roots: &PluginRoots, scope: PluginScope) -> Result<Catalog, String> {
    let roots = normalize_roots(roots)?;
    if scope == PluginScope::Project && roots.project.is_none() {
        return Err("尚未打开工程，请管理全局插件".into());
    }
    let dependencies = roots
        .project
        .as_ref()
        .map(|root| project_dependencies(root))
        .transpose();
    let mut warnings = Vec::new();
    let dependencies = match dependencies {
        Ok(value) => value.unwrap_or_default(),
        Err(e) => {
            warnings.push(e);
            BTreeSet::new()
        }
    };
    let mut global = match scan_global(
        &store_for(&roots, PluginScope::Global, false)?,
        &dependencies,
    ) {
        Ok(value) => value,
        Err(e) => {
            warnings.push(format!("全局插件库读取失败：{e}"));
            Catalog::default()
        }
    };
    let mut project = if roots.project.is_some() {
        scan_project(
            &store_for(&roots, PluginScope::Project, false)?,
            &dependencies,
        )?
    } else {
        Catalog::default()
    };
    let blockers: BTreeSet<_> = project
        .entries
        .iter()
        .flat_map(|entry| {
            let stem = entry
                .path
                .file_stem()
                .and_then(|v| v.to_str())
                .unwrap_or_default()
                .to_string();
            [entry.id.clone(), stem]
        })
        .collect();
    let global_ids: BTreeSet<_> = global.entries.iter().map(|e| e.id.clone()).collect();
    for entry in &mut global.entries {
        entry.overridden_by_project = blockers.contains(&entry.id);
        entry.effective = !entry.overridden_by_project;
    }
    for entry in &mut project.entries {
        entry.global_available = global_ids.contains(&entry.id);
    }
    let mut result = match scope {
        PluginScope::Global => global,
        PluginScope::Project => {
            let settings = read_settings(&store_for(&roots, PluginScope::Project, false)?)?;
            for mut entry in global.entries {
                if blockers.contains(&entry.id) {
                    continue;
                }
                entry.inherited = true;
                entry.enabled &= !settings.inherited_disabled.contains(&entry.id);
                project.entries.push(entry);
            }
            project.warnings.extend(global.warnings);
            project.notices.extend(global.notices);
            project
        }
    };
    warnings.append(&mut result.warnings);
    result.warnings = warnings;
    attach_pending(
        &mut result,
        &store_for(&roots, scope, false)?,
        &dependencies,
    )?;
    result
        .entries
        .sort_by(|a, b| a.id.cmp(&b.id).then(a.path.cmp(&b.path)));
    Ok(result)
}
fn scan_project(store: &Store, dependencies: &BTreeSet<String>) -> Result<Catalog, String> {
    let settings = read_settings(store)?;
    let mut result = Catalog::default();
    if !store.plugins.exists() {
        return Ok(result);
    }
    if !ordinary(&store.plugins)?.is_dir() {
        return Err("工程plugins路径不是普通目录".into());
    }
    let mut paths = Vec::new();
    for (index, entry) in fs::read_dir(&store.plugins).map_err(error)?.enumerate() {
        if index >= MAX_ENTRIES {
            result
                .warnings
                .push("plugins目录超过256项，后续文件未扫描".into());
            break;
        }
        match entry {
            Ok(entry) => paths.push(entry.path()),
            Err(e) => result.warnings.push(e.to_string()),
        }
    }
    paths.sort();
    for path in paths {
        // 旧目录包保留原文件，显示迁移诊断，不再进入动态装载或单文件事务。
        if path.is_dir() && path.join("plugin.json").exists() {
            let mut row = invalid_entry(
                &path,
                PluginScope::Project,
                "旧plugin.json目录包不再加载；请安装带嵌入元数据的单文件插件".into(),
            );
            row.legacy = true;
            if let Ok(value) = read_json::<serde_json::Value>(&path.join("plugin.json"), MAX_JSON)
                && let Some(id) = value
                    .get("id")
                    .and_then(|v| v.as_str())
                    .filter(|id| valid_id(id))
            {
                row.id = id.into();
            }
            result.entries.push(row);
            continue;
        }
        if !candidate(&path) {
            continue;
        }
        let row = match inspect_package(&path) {
            Ok((metadata, library)) => {
                let id = metadata.id.clone();
                let mut row = entry_from_manifest(
                    metadata,
                    library,
                    PluginScope::Project,
                    &settings,
                    dependencies,
                );
                if path.file_name().and_then(|v| v.to_str()) != Some(binary_filename(&id).as_str())
                {
                    row.error = Some(format!(
                        "工程插件文件名必须是{}，请通过安装功能导入",
                        binary_filename(&id)
                    ));
                }
                row
            }
            Err(e) => invalid_entry(&path, PluginScope::Project, e),
        };
        result.entries.push(row);
    }
    // 一次切换时旧目录必须保留，但不能把它算成新格式的第二个同ID候选。
    let new_ids: BTreeSet<_> = result
        .entries
        .iter()
        .filter(|entry| !entry.legacy)
        .map(|entry| entry.id.clone())
        .collect();
    result.entries.retain(|entry| {
        if entry.legacy && new_ids.contains(&entry.id) {
            result.notices.push(format!(
                "旧目录包{}保留在{}，同ID单文件已接管",
                entry.id,
                crate::project::display_path(&entry.path)
            ));
            false
        } else {
            true
        }
    });
    reject_duplicates(&mut result);
    Ok(result)
}
fn scan_global(store: &Store, dependencies: &BTreeSet<String>) -> Result<Catalog, String> {
    let settings = read_settings(store)?;
    let index = read_index(store)?;
    let mut result = Catalog::default();
    for (id, record) in index.entries {
        let path = object_path(store, &record)?;
        let mut row = match inspect_package(&path) {
            Ok((metadata, library)) if metadata.id == id => entry_from_manifest(
                metadata,
                library,
                PluginScope::Global,
                &settings,
                dependencies,
            ),
            Ok(_) => invalid_entry(
                &path,
                PluginScope::Global,
                "全局索引与插件自带ID不一致".into(),
            ),
            Err(e) => invalid_entry(&path, PluginScope::Global, e),
        };
        row.id = id;
        if row
            .origin
            .as_ref()
            .is_some_and(|origin| origin.digest != record.digest)
        {
            row.error = Some("全局不可变版本对象被外部修改，拒绝加载".into());
        }
        result.entries.push(row);
    }
    Ok(result)
}
fn reject_duplicates(catalog: &mut Catalog) {
    let mut counts = BTreeMap::<String, usize>::new();
    for entry in &catalog.entries {
        *counts.entry(entry.id.clone()).or_default() += 1;
    }
    for entry in &mut catalog.entries {
        if counts[&entry.id] > 1 {
            entry.error = Some(format!("插件ID重复：{}；请保留唯一文件后重开", entry.id));
        }
    }
}
fn invalid_entry(path: &Path, scope: PluginScope, error: String) -> PackageEntry {
    let id = path
        .file_stem()
        .or_else(|| path.file_name())
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    PackageEntry {
        id: id.clone(),
        tool: id,
        version: None,
        directory: path.into(),
        path: path.into(),
        scope,
        inherited: false,
        effective: true,
        overridden_by_project: false,
        global_available: false,
        legacy: false,
        origin: None,
        enabled: false,
        error: Some(error),
        pending: None,
        runtime_dependencies: Vec::new(),
        runtime_missing: Vec::new(),
    }
}
fn entry_from_manifest(
    metadata: PackageManifest,
    path: PathBuf,
    scope: PluginScope,
    settings: &Settings,
    dependencies: &BTreeSet<String>,
) -> PackageEntry {
    let runtime_missing = metadata
        .runtime_dependencies
        .iter()
        .filter(|name| !dependencies.contains(name.as_str()))
        .cloned()
        .collect();
    let digest = binary_digest(&path);
    let origin = digest.as_ref().ok().map(|digest| PluginOrigin {
        scope,
        path: path.clone(),
        digest: digest.clone(),
    });
    PackageEntry {
        enabled: !settings.disabled.contains(&metadata.id),
        id: metadata.id,
        tool: metadata.tool,
        version: Some(metadata.version),
        directory: path.clone(),
        path,
        scope,
        inherited: false,
        effective: true,
        overridden_by_project: false,
        global_available: false,
        legacy: false,
        origin,
        error: digest.err(),
        pending: None,
        runtime_dependencies: metadata.runtime_dependencies,
        runtime_missing,
    }
}
fn attach_pending(
    catalog: &mut Catalog,
    store: &Store,
    dependencies: &BTreeSet<String>,
) -> Result<(), String> {
    let read = read_journal(store)?;
    let settings = read_settings(store)?;
    if let Some(legacy) = read.legacy {
        catalog.warnings.push(
            "检测到旧目录包待生效日志，已停止执行；可逐项撤销，旧发布文件、暂存和备份均保留".into(),
        );
        if let Some(changes) = legacy.get("changes").and_then(|v| v.as_object()) {
            for (id, record) in changes {
                if !valid_id(id) {
                    continue;
                }
                let pending = Some(PendingChange {
                    kind: record
                        .get("kind")
                        .cloned()
                        .and_then(|v| serde_json::from_value(v).ok())
                        .unwrap_or(PendingKind::Install),
                    version: record
                        .pointer("/manifest/version")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                });
                if let Some(entry) = catalog.entries.iter_mut().find(|e| &e.id == id) {
                    entry.pending = pending;
                } else {
                    let mut entry = invalid_entry(
                        &store.plugins.join(id),
                        store.scope,
                        "旧格式待生效计划不再执行；请撤销后重新安装单文件插件".into(),
                    );
                    entry.id = id.clone();
                    entry.legacy = true;
                    entry.pending = pending;
                    catalog.entries.push(entry);
                }
            }
        }
        return Ok(());
    }
    for (id, record) in read.journal.changes {
        let pending = Some(PendingChange {
            kind: record.kind,
            // 仅安装/更新交付版本；坏包移除使用的事务占位元数据不得冒充真实版本。
            version: matches!(record.kind, PendingKind::Install | PendingKind::Update)
                .then(|| record.manifest.version.clone()),
        });
        if let Some(entry) = catalog.entries.iter_mut().find(|e| e.id == id) {
            entry.pending = pending;
        } else {
            let mut entry = entry_from_manifest(
                record.manifest,
                store.plugins.join(&record.destination),
                store.scope,
                &settings,
                dependencies,
            );
            entry.error = None;
            entry.origin = None;
            entry.enabled = false;
            entry.pending = pending;
            catalog.entries.push(entry);
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn stage(root: &Path, command: ManagementCommand) -> Result<Catalog, String> {
    stage_at(&roots_for_project(root)?, PluginScope::Project, command)
}
pub(crate) fn stage_at(
    roots: &PluginRoots,
    scope: PluginScope,
    command: ManagementCommand,
) -> Result<Catalog, String> {
    stage_inner(roots, scope, command, None)
}
pub(crate) fn stage_update_at(
    roots: &PluginRoots,
    scope: PluginScope,
    source: PathBuf,
    id: &str,
) -> Result<Catalog, String> {
    stage_inner(
        roots,
        scope,
        ManagementCommand::Install {
            source,
            replace: true,
        },
        Some(id),
    )
}

/// 锁、来源摘要、复制后元数据和完整副本摘要共同冻结本次操作；主帧从不等待这些IO。
fn stage_inner(
    roots: &PluginRoots,
    scope: PluginScope,
    command: ManagementCommand,
    expected: Option<&str>,
) -> Result<Catalog, String> {
    let roots = normalize_roots(roots)?;
    let store = store_for(&roots, scope, true)?;
    let lock = transaction_lock(&store.manager)?;
    let read = read_journal(&store)?;
    if let Some(legacy) = read.legacy {
        if let ManagementCommand::Cancel { id } = command {
            cancel_legacy(&store, &id, legacy)?;
            drop(lock);
            return catalog_at(&roots, scope);
        }
        return Err("旧目录包待生效日志已停用，请先撤销旧计划；旧文件和备份不会删除".into());
    }
    let mut journal = read.journal;
    let current = catalog_at(&roots, scope)?;
    match command {
        ManagementCommand::OpenDirectory => {
            return Err("打开目录不属于插件安装事务".into());
        }
        ManagementCommand::Cancel { id } => {
            if !valid_id(&id) {
                return Err("插件身份无效".into());
            }
            let record = journal.changes.remove(&id).ok_or("插件没有待生效操作")?;
            write_journal(&store, &journal)?;
            let staged = staged_path(&store, &record)?;
            if staged.exists() {
                remove_owned_file(&store.manager, &staged)?;
            }
        }
        ManagementCommand::Install { source, replace } => {
            let before_source = binary_digest(&source)?;
            let (manifest, source) = inspect_package(&source)?;
            if expected.is_some_and(|id| manifest.id != id) {
                return Err(format!(
                    "选中的插件是{}，不能用于更新{}",
                    manifest.id,
                    expected.unwrap()
                ));
            }
            let id = manifest.id.clone();
            if journal.changes.contains_key(&id) {
                return Err("该插件已有待生效操作，请先撤销".into());
            }
            let local: Vec<_> = current
                .entries
                .iter()
                .filter(|e| e.id == id && !e.inherited && !e.legacy)
                .collect();
            if local.len() > 1 {
                return Err("安装范围内有重复插件ID，请先修复安装文件".into());
            }
            let old = local.first().copied();
            if old.is_some() && !replace {
                return Err("此安装范围已有相同ID，请使用更新".into());
            }
            if old.is_none() && replace {
                return Err("此安装范围未安装该插件，请使用安装".into());
            }
            // 旧目录和继承行本身不占本范围单文件名额；挂在其上的本范围Install仍须预留一个槽位。
            if old.is_none()
                && current.entries.iter().filter(|entry| {
                    (!entry.inherited && !entry.legacy)
                        || entry.pending.as_ref().is_some_and(|pending| pending.kind == PendingKind::Install)
                }).count() >= MAX_PLUGINS
            {
                return Err("每个安装范围最多安装16个原生插件".into());
            }
            let destination = binary_filename(&id);
            let installed = if scope == PluginScope::Project {
                Some(checked_child(
                    &store.plugins,
                    Path::new(&destination),
                    false,
                )?)
            } else {
                None
            };
            if let Some(installed) = &installed
                && installed.exists()
                && old.is_none()
            {
                return Err("安装文件已被另一个插件或坏包占用，拒绝覆盖".into());
            }
            let before_digest = old
                .filter(|e| e.path.exists())
                .map(|e| binary_digest(&e.path))
                .transpose()?;
            let before_index = if scope == PluginScope::Global {
                read_index(&store)?.entries.get(&id).cloned()
            } else {
                None
            };
            let mut record = PendingRecord {
                manifest,
                kind: if old.is_some() {
                    PendingKind::Update
                } else {
                    PendingKind::Install
                },
                ticket: ticket(),
                destination,
                before_digest,
                before_index,
                after_digest: None,
                inherited: false,
            };
            let staged = staged_path(&store, &record)?;
            if source.starts_with(&store.manager) {
                return Err("不能把管理暂存文件作为插件来源".into());
            }
            ensure_parent(&store.manager, &staged)?;
            let copied = (|| {
                copy_binary(&source, &staged)?;
                let (copied, _) = inspect_package(&staged)?;
                if copied != record.manifest || binary_digest(&staged)? != before_source {
                    return Err("复制期间插件文件或元数据发生变化".into());
                }
                record.after_digest = Some(before_source);
                Ok(())
            })();
            if let Err(e) = copied {
                if staged.exists() {
                    let _ = remove_owned_file(&store.manager, &staged);
                }
                return Err(e);
            }
            journal.changes.insert(id, record);
            if let Err(e) = write_journal(&store, &journal) {
                let _ = remove_owned_file(&store.manager, &staged);
                return Err(e);
            }
        }
        ManagementCommand::SetEnabled { id, enabled } => {
            let old = unique_installed(&current, &id)?;
            if old.legacy {
                return Err("旧目录包已停止加载，请先安装单文件插件".into());
            }
            if journal.changes.contains_key(&id) {
                return Err("该插件已有待生效操作，请先撤销".into());
            }
            if old.enabled == enabled {
                return Err("插件已经处于此启用状态".into());
            }
            if old.inherited && enabled {
                let global = store_for(&roots, PluginScope::Global, false)?;
                if read_settings(&global)?.disabled.contains(&id) {
                    return Err("全局副本已停用，请先切换到全局插件启用".into());
                }
            }
            let manifest = if enabled {
                inspect_package(&old.path)?.0
            } else {
                removal_manifest(old)
            };
            let before_index = if scope == PluginScope::Global {
                read_index(&store)?.entries.get(&id).cloned()
            } else {
                None
            };
            let record = PendingRecord {
                manifest,
                kind: if enabled {
                    PendingKind::Enable
                } else {
                    PendingKind::Disable
                },
                ticket: ticket(),
                destination: old
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                before_digest: Some(binary_digest(&old.path)?),
                before_index,
                after_digest: None,
                inherited: old.inherited,
            };
            journal.changes.insert(id, record);
            write_journal(&store, &journal)?;
        }
        ManagementCommand::Remove { id } => {
            let old = unique_installed(&current, &id)?;
            if old.inherited {
                return Err("此插件继承自全局，请切换到全局范围移除，或在本工程停用".into());
            }
            if journal.changes.contains_key(&id) {
                return Err("该插件已有待生效操作，请先撤销".into());
            }
            if old.legacy {
                // 旧格式从未进入当前作者加载链；确认后归档移出扫描根，原文件全部保留。
                let mut archived = legacy_archive::archive(&store, old)?;
                drop(lock);
                let mut catalog = catalog_at(&roots, scope)?;
                // 同一后台快照检查原位是否被复用；未知错误也不能被当作空位。
                archived.original_vacant = fs::symlink_metadata(&archived.origin.path)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
                catalog.completion = Some(format!(
                    "旧格式插件已归档移除；原文件保存在{}",
                    crate::project::display_path(&archived.archived),
                ));
                catalog.archived_legacy = Some(archived);
                return Ok(catalog);
            }
            let before_index = if scope == PluginScope::Global {
                read_index(&store)?.entries.get(&id).cloned()
            } else {
                None
            };
            let before_digest = if old.path.exists() {
                Some(binary_digest(&old.path)?)
            } else {
                None
            };
            let record = PendingRecord {
                manifest: removal_manifest(old),
                kind: PendingKind::Remove,
                ticket: ticket(),
                destination: old
                    .path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                before_digest,
                before_index,
                after_digest: None,
                inherited: false,
            };
            journal.changes.insert(id, record);
            write_journal(&store, &journal)?;
        }
    }
    drop(lock);
    catalog_at(&roots, scope)
}
fn unique_installed<'a>(catalog: &'a Catalog, id: &str) -> Result<&'a PackageEntry, String> {
    if !valid_id(id) {
        return Err("插件身份无效".into());
    }
    let mut matches = catalog.entries.iter().filter(|e| e.id == id);
    let entry = matches.next().ok_or("此范围没有安装该插件")?;
    if matches.next().is_some() {
        return Err("此范围有重复插件ID，请先修复安装文件".into());
    }
    if entry
        .pending
        .as_ref()
        .is_some_and(|p| p.kind == PendingKind::Install)
    {
        return Err("插件尚未安装生效，请先重开工程或撤销安装".into());
    }
    Ok(entry)
}
fn removal_manifest(entry: &PackageEntry) -> PackageManifest {
    inspect_package(&entry.path)
        .map(|v| v.0)
        .unwrap_or_else(|_| PackageManifest {
            format: 1,
            id: entry.id.clone(),
            tool: entry.tool.clone(),
            version: entry.version.clone().unwrap_or_else(|| "0.0.0".into()),
            target_os: std::env::consts::OS.into(),
            target_arch: std::env::consts::ARCH.into(),
            abi_major: ABI_MAJOR,
            runtime_dependencies: Vec::new(),
            runtime: None,
        })
}
fn cancel_legacy(store: &Store, id: &str, mut value: serde_json::Value) -> Result<(), String> {
    if !valid_id(id) {
        return Err("插件身份无效".into());
    }
    let original = value.clone();
    let changes = value
        .get_mut("changes")
        .and_then(|v| v.as_object_mut())
        .ok_or("旧待生效日志无法按ID撤销，请备份检查日志")?;
    if changes.remove(id).is_none() {
        return Err("插件没有旧格式待生效操作".into());
    }
    let empty = changes.is_empty();
    write_json(
        &store.manager,
        &format!("legacy/pending-{}.json", ticket()),
        &original,
    )?;
    if empty {
        write_journal(store, &empty_journal(store.scope))
    } else {
        write_json(&store.manager, "pending.json", &value)
    }
}

#[cfg(test)]
pub(crate) fn apply_pending(root: &Path) -> Vec<String> {
    match roots_for_project(root) {
        Ok(roots) => apply_pending_with_roots(&roots),
        Err(e) => vec![e],
    }
}
/// 仅在新作者实例的后台发现线程调用。全局先采用，再按工程显式偏好选择最终来源。
pub(crate) fn apply_pending_with_roots(roots: &PluginRoots) -> Vec<String> {
    let mut warnings = Vec::new();
    let roots = match normalize_roots(roots) {
        Ok(value) => value,
        Err(e) => return vec![format!("插件管理初始化失败：{e}")],
    };
    for scope in [PluginScope::Global, PluginScope::Project] {
        if scope == PluginScope::Project && roots.project.is_none() {
            continue;
        }
        if let Err(e) = apply_store(&roots, scope, &mut warnings) {
            warnings.push(format!("{scope:?}插件管理初始化失败：{e}"));
        }
    }
    warnings
}
fn apply_store(
    roots: &PluginRoots,
    scope: PluginScope,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let store = store_for(roots, scope, false)?;
    if !store.manager.exists() {
        return Ok(());
    }
    let _lock = transaction_lock(&store.manager)?;
    let read = read_journal(&store)?;
    if read.legacy.is_some() {
        return Err(
            "旧目录包待生效日志已停用，请撤销后重新安装单文件插件；原文件、暂存和备份保留".into(),
        );
    }
    let mut journal = read.journal;
    let mut settings = read_settings(&store)?;
    let mut index = read_index(&store)?;
    for (id, record) in journal.changes.clone() {
        let mut next_settings = settings.clone();
        let mut next_index = index.clone();
        let result = if scope == PluginScope::Global {
            apply_global(&store, &record, &mut next_settings, &mut next_index)
        } else {
            apply_project(roots, &store, &record, &mut next_settings)
        };
        match result {
            Ok(()) => {
                settings = next_settings;
                index = next_index;
                journal.changes.remove(&id);
                if let Err(e) = write_journal(&store, &journal) {
                    warnings.push(format!("插件{id}已处理但日志回执未保存：{e}"));
                    break;
                }
            }
            Err(e) => warnings.push(format!("插件{id}未应用，原文件和待生效操作保留：{e}")),
        }
    }
    Ok(())
}
fn validate_record(record: &PendingRecord) -> Result<(), String> {
    if !valid_id(&record.manifest.id) {
        return Err("待生效插件ID无效".into());
    }
    validate_component(&record.destination)?;
    validate_component(&record.ticket)?;
    if record.ticket.len() > 96
        || record
            .before_digest
            .as_deref()
            .is_some_and(|v| !valid_digest(v))
        || record
            .after_digest
            .as_deref()
            .is_some_and(|v| !valid_digest(v))
    {
        return Err("插件事务身份或摘要无效".into());
    }
    if !candidate(Path::new(&record.destination)) {
        return Err("待生效目标不是原生插件文件".into());
    }
    Ok(())
}
fn verify_binary(path: &Path, record: &PendingRecord, expected: &str) -> Result<(), String> {
    if binary_digest(path)? != expected {
        return Err("插件文件被外部修改，拒绝采用".into());
    }
    let (metadata, _) = inspect_package(path)?;
    if metadata != record.manifest {
        return Err("插件静态元数据与待生效记录不一致".into());
    }
    if binary_digest(path)? != expected {
        return Err("校验期间插件文件发生变化".into());
    }
    Ok(())
}
fn apply_project(
    roots: &PluginRoots,
    store: &Store,
    record: &PendingRecord,
    settings: &mut Settings,
) -> Result<(), String> {
    validate_record(record)?;
    if record.inherited {
        if !matches!(record.kind, PendingKind::Enable | PendingKind::Disable) {
            return Err("继承插件只能修改本工程启停偏好".into());
        }
        // 停用是工程按ID的偏好，不操作全局对象；同轮全局升级或移除不能令停用计划失效。
        // 未来全局重新安装同ID时仍遵守该偏好；启用则继续验证当时明确选择的真实来源。
        if record.kind == PendingKind::Disable {
            return apply_enabled(store, record, settings);
        }
        let global = store_for(roots, PluginScope::Global, false)?;
        let index = read_index(&global)?;
        let active = index
            .entries
            .get(&record.manifest.id)
            .ok_or("继承的全局插件已移除，请刷新后重新操作")?;
        if Some(active.digest.as_str()) != record.before_digest.as_deref() {
            return Err("继承的全局插件版本已经改变，请刷新后重新操作".into());
        }
        let path = object_path(&global, active)?;
        if binary_digest(&path)? != active.digest {
            return Err("全局对象被外部修改".into());
        }
        if record.kind == PendingKind::Enable {
            verify_binary(&path, record, &active.digest)?;
            if read_settings(&global)?
                .disabled
                .contains(&record.manifest.id)
            {
                return Err("全局插件已停用，请先启用全局副本".into());
            }
        }
        return apply_enabled(store, record, settings);
    }
    ensure_directory(&store.plugins)?;
    let destination = checked_child(&store.plugins, Path::new(&record.destination), false)?;
    let backup = backup_path(store, record)?;
    let staged = staged_path(store, record)?;
    match record.kind {
        PendingKind::Enable | PendingKind::Disable => {
            let expected = record
                .before_digest
                .as_deref()
                .ok_or("启停计划缺少来源摘要")?;
            if binary_digest(&destination)? != expected {
                return Err("启停期间正式插件被修改".into());
            }
            if record.kind == PendingKind::Enable {
                verify_binary(&destination, record, expected)?;
            }
            apply_enabled(store, record, settings)?;
        }
        PendingKind::Install | PendingKind::Update => {
            let after = record.after_digest.as_deref().ok_or("待安装文件缺少摘要")?;
            if destination.exists() && binary_digest(&destination)? == after {
                verify_binary(&destination, record, after)?;
                return write_receipt(store, record);
            }
            if let Err(e) = verify_binary(&staged, record, after) {
                if backup.exists() && !destination.exists() {
                    verify_before(&backup, record)?;
                    fs::rename(&backup, &destination)
                        .map_err(|rollback| format!("{e}；旧文件恢复失败：{rollback}"))?;
                }
                return Err(e);
            }
            if backup.exists() {
                verify_before(&backup, record)?;
                if destination.exists() {
                    return Err("备份和正式文件同时存在，拒绝覆盖外部改动".into());
                }
            } else if record.before_digest.is_some() {
                verify_before(&destination, record)?;
                ensure_parent(&store.manager, &backup)?;
                fs::rename(&destination, &backup)
                    .map_err(|e| format!("无法备份旧插件（可能仍被另一编辑器占用）：{e}"))?;
            } else if destination.exists() {
                return Err("安装目标已存在，拒绝覆盖".into());
            }
            if let Err(e) = fs::rename(&staged, &destination) {
                if backup.exists() && !destination.exists() {
                    fs::rename(&backup, &destination)
                        .map_err(|rollback| format!("安装失败：{e}；旧文件恢复失败：{rollback}"))?;
                }
                return Err(format!("安装失败，旧文件保留：{e}"));
            }
            if let Err(e) = write_receipt(store, record) {
                fs::rename(&destination, &staged)
                    .map_err(|rollback| format!("回执失败：{e}；新文件退回暂存失败：{rollback}"))?;
                if backup.exists() {
                    fs::rename(&backup, &destination)
                        .map_err(|rollback| format!("回执失败：{e}；旧文件恢复失败：{rollback}"))?;
                }
                return Err(format!("安装回执失败，已恢复旧文件：{e}"));
            }
        }
        PendingKind::Remove => {
            if !backup.exists() {
                verify_before(&destination, record)?;
                ensure_parent(&store.manager, &backup)?;
                fs::rename(&destination, &backup)
                    .map_err(|e| format!("无法移除插件（可能仍被另一编辑器占用）：{e}"))?;
            } else {
                verify_before(&backup, record)?;
                if destination.exists() {
                    return Err("移除备份和正式文件同时存在，拒绝覆盖".into());
                }
            }
            if let Err(e) = write_receipt(store, record) {
                fs::rename(&backup, &destination)
                    .map_err(|rollback| format!("回执失败：{e}；旧文件恢复失败：{rollback}"))?;
                return Err(e);
            }
        }
    }
    Ok(())
}
fn verify_before(path: &Path, record: &PendingRecord) -> Result<(), String> {
    let before = record
        .before_digest
        .as_deref()
        .ok_or("变更计划缺少原文件摘要")?;
    if binary_digest(path)? != before {
        return Err("正式插件在计划后被修改，拒绝覆盖或移除".into());
    }
    Ok(())
}
fn apply_enabled(
    store: &Store,
    record: &PendingRecord,
    settings: &mut Settings,
) -> Result<(), String> {
    let previous = settings.clone();
    let disabled = if store.scope == PluginScope::Project && record.inherited {
        &mut settings.inherited_disabled
    } else {
        &mut settings.disabled
    };
    if record.kind == PendingKind::Enable {
        disabled.remove(&record.manifest.id);
    } else {
        disabled.insert(record.manifest.id.clone());
    }
    write_settings(store, settings)?;
    if let Err(e) = write_receipt(store, record) {
        write_settings(store, &previous)
            .map_err(|rollback| format!("启停回执失败：{e}；配置回滚失败：{rollback}"))?;
        *settings = previous;
        return Err(e);
    }
    Ok(())
}

/// 全局只切换原子索引，永不覆盖/删除已发布对象；其他编辑器映射的旧库始终有效。
fn apply_global(
    store: &Store,
    record: &PendingRecord,
    settings: &mut Settings,
    index: &mut GlobalIndex,
) -> Result<(), String> {
    validate_record(record)?;
    if record.inherited {
        return Err("全局事务不能带工程继承偏好".into());
    }
    let current = index.entries.get(&record.manifest.id).cloned();
    if matches!(record.kind, PendingKind::Install | PendingKind::Update)
        && current.as_ref().map(|v| v.digest.as_str()) == record.after_digest.as_deref()
    {
        let current = current.as_ref().ok_or("全局回执缺少实际版本")?;
        verify_binary(&object_path(store, current)?, record, &current.digest)?;
        return write_receipt(store, record);
    }
    if current != record.before_index {
        // 已完成移除但日志清理被中断时，仍须存在与原来源对应的真实回执。
        if record.kind == PendingKind::Remove
            && current.is_none()
            && receipt_matches(store, record)?
        {
            return Ok(());
        }
        return Err("全局活动版本在计划后改变，拒绝覆盖另一编辑器的操作".into());
    }
    if let Some(current) = &current {
        let path = object_path(store, current)?;
        let actual = if path.exists() {
            Some(binary_digest(&path)?)
        } else {
            None
        };
        if actual != record.before_digest {
            return Err("全局原版本对象在计划后被外部修改".into());
        }
    }
    match record.kind {
        PendingKind::Enable | PendingKind::Disable => {
            if record.kind == PendingKind::Enable {
                let current = current.as_ref().ok_or("全局插件已移除")?;
                verify_binary(&object_path(store, current)?, record, &current.digest)?;
            }
            return apply_enabled(store, record, settings);
        }
        PendingKind::Install | PendingKind::Update => {
            let after = record.after_digest.as_ref().ok_or("待安装文件缺少摘要")?;
            let active = GlobalRecord {
                digest: after.clone(),
                filename: record.destination.clone(),
            };
            let object = object_path(store, &active)?;
            let staged = staged_path(store, record)?;
            if object.exists() {
                verify_binary(&object, record, after)?;
            } else {
                verify_binary(&staged, record, after)?;
                ensure_parent(&store.plugins, &object)?;
                fs::rename(&staged, &object).map_err(|e| format!("无法发布全局不可变版本：{e}"))?;
            }
            index.entries.insert(record.manifest.id.clone(), active);
        }
        PendingKind::Remove => {
            index.entries.remove(&record.manifest.id);
        }
    }
    let previous = read_index(store)?;
    write_json(
        &store.manager,
        &format!("backups/{}/active.json", record.ticket),
        &previous,
    )?;
    if let Err(e) = write_index(store, index) {
        *index = previous;
        return Err(e);
    }
    if let Err(e) = write_receipt(store, record) {
        write_index(store, &previous)
            .map_err(|rollback| format!("全局回执失败：{e}；索引回滚失败：{rollback}"))?;
        *index = previous;
        return Err(e);
    }
    Ok(())
}
fn receipt_matches(store: &Store, record: &PendingRecord) -> Result<bool, String> {
    let path = checked_child(
        &store.manager,
        &Path::new("receipts").join(format!("{}.json", record.ticket)),
        false,
    )?;
    if !path.exists() {
        return Ok(false);
    }
    let value: serde_json::Value = read_json(&path, MAX_JSON)?;
    Ok(
        value.get("scope") == Some(&serde_json::to_value(store.scope).map_err(error)?)
            && value.get("record") == Some(&serde_json::to_value(record).map_err(error)?),
    )
}

fn normalize_roots(roots: &PluginRoots) -> Result<PluginRoots, String> {
    Ok(PluginRoots {
        project: roots
            .project
            .as_ref()
            .map(|p| canonical_root(p, true))
            .transpose()?,
        global: canonical_root(&roots.global, false)?,
    })
}
fn canonical_root(path: &Path, required: bool) -> Result<PathBuf, String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err("插件根必须是明确绝对路径".into());
    }
    if path.exists() {
        ordinary_chain(path)?;
        if !ordinary(path)?.is_dir() {
            return Err("插件根不是普通目录".into());
        }
        return path.canonicalize().map_err(error);
    }
    if required {
        return Err(format!(
            "插件工程根不存在：{}",
            crate::project::display_path(path)
        ));
    }
    let mut missing = Vec::new();
    let mut ancestor = path;
    while !ancestor.exists() {
        missing.push(
            ancestor
                .file_name()
                .ok_or("无法定位插件根的父目录")?
                .to_os_string(),
        );
        ancestor = ancestor.parent().ok_or("插件根没有父目录")?;
    }
    if !ordinary(ancestor)?.is_dir() {
        return Err("插件根的父路径不是普通目录".into());
    }
    let mut result = ancestor.canonicalize().map_err(error)?;
    for part in missing.into_iter().rev() {
        result.push(part);
    }
    Ok(result)
}
fn store_for(roots: &PluginRoots, scope: PluginScope, create: bool) -> Result<Store, String> {
    let (plugins, manager) = match scope {
        PluginScope::Project => {
            let root = roots
                .project
                .as_ref()
                .ok_or("尚未打开工程，不能管理工程插件")?;
            (
                checked_child(root, Path::new("plugins"), false)?,
                checked_child(root, Path::new(".editor/plugin-management"), false)?,
            )
        }
        PluginScope::Global => (
            roots.global.clone(),
            checked_child(&roots.global, Path::new(".management"), false)?,
        ),
    };
    if create {
        ensure_directory(&plugins)?;
        ensure_directory(&manager)?;
    }
    Ok(Store {
        scope,
        plugins,
        manager,
    })
}
fn ensure_directory(path: &Path) -> Result<(), String> {
    if path.exists() {
        if !ordinary(path)?.is_dir() {
            return Err("插件事务目录不是普通目录".into());
        }
        return Ok(());
    }
    let parent = path.parent().ok_or("插件目录没有父目录")?;
    ensure_directory(parent)?;
    fs::create_dir(path).map_err(error)?;
    if !ordinary(path)?.is_dir() {
        return Err("插件事务目录创建失败".into());
    }
    Ok(())
}
fn transaction_lock(manager: &Path) -> Result<fs::File, String> {
    let path = checked_child(manager, Path::new("transaction.lock"), false)?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(error)?;
    file.try_lock()
        .map_err(|_| "另一编辑器正在处理此范围的插件，请稍后重试".to_string())?;
    Ok(file)
}
fn read_settings(store: &Store) -> Result<Settings, String> {
    let path = checked_child(&store.manager, Path::new("state.json"), false)?;
    if !path.exists() {
        return Ok(Settings {
            format: 1,
            disabled: BTreeSet::new(),
            inherited_disabled: BTreeSet::new(),
        });
    }
    let value: Settings = read_json(&path, MAX_JSON)?;
    validate_settings(&value)?;
    Ok(value)
}
/// 读取与发布共用上限，避免失败操作先写出第257个ID，再令整个范围的配置无法读回。
fn validate_settings(value: &Settings) -> Result<(), String> {
    if value.format != 1
        || value.disabled.len() > MAX_ENTRIES
        || value.disabled.iter().any(|id| !valid_id(id))
        || value.inherited_disabled.len() > MAX_ENTRIES
        || value.inherited_disabled.iter().any(|id| !valid_id(id))
    {
        return Err("插件启停配置无效".into());
    }
    Ok(())
}
fn empty_journal(scope: PluginScope) -> Journal {
    Journal {
        format: JOURNAL_FORMAT,
        scope,
        changes: BTreeMap::new(),
    }
}
fn read_journal(store: &Store) -> Result<JournalRead, String> {
    let path = checked_child(&store.manager, Path::new("pending.json"), false)?;
    if !path.exists() {
        return Ok(JournalRead {
            journal: empty_journal(store.scope),
            legacy: None,
        });
    }
    let value: serde_json::Value = read_json(&path, MAX_JSON)?;
    if value.get("format").and_then(|v| v.as_u64()) == Some(1) {
        return Ok(JournalRead {
            journal: empty_journal(store.scope),
            legacy: Some(value),
        });
    }
    let journal: Journal =
        serde_json::from_value(value).map_err(|e| format!("插件待生效日志无效：{e}"))?;
    validate_journal(&journal, store.scope)?;
    Ok(JournalRead {
        journal,
        legacy: None,
    })
}
/// 继承项和本地项共用工程计划预算；写入前拒绝超限，不能破坏此前已登记的合法计划。
fn validate_journal(journal: &Journal, scope: PluginScope) -> Result<(), String> {
    if journal.format != JOURNAL_FORMAT
        || journal.scope != scope
        || journal.changes.len() > MAX_PLUGINS
    {
        return Err("插件待生效日志格式、范围或数量无效".into());
    }
    for (id, record) in &journal.changes {
        validate_record(record)?;
        if *id != record.manifest.id {
            return Err("待生效插件身份与记录不一致".into());
        }
    }
    Ok(())
}
fn read_index(store: &Store) -> Result<GlobalIndex, String> {
    if store.scope != PluginScope::Global {
        return Ok(GlobalIndex {
            format: 1,
            entries: BTreeMap::new(),
        });
    }
    let path = checked_child(&store.plugins, Path::new("index.json"), false)?;
    if !path.exists() {
        return Ok(GlobalIndex {
            format: 1,
            entries: BTreeMap::new(),
        });
    }
    let index: GlobalIndex = read_json(&path, MAX_JSON)?;
    if index.format != 1 || index.entries.len() > MAX_ENTRIES {
        return Err("全局插件索引格式或数量无效".into());
    }
    for (id, record) in &index.entries {
        if !valid_id(id) || !valid_digest(&record.digest) || record.filename != binary_filename(id)
        {
            return Err("全局插件索引身份或对象路径无效".into());
        }
    }
    Ok(index)
}
fn write_index(store: &Store, index: &GlobalIndex) -> Result<(), String> {
    write_json(&store.plugins, "index.json", index)
}
fn write_settings(store: &Store, settings: &Settings) -> Result<(), String> {
    validate_settings(settings)?;
    write_json(&store.manager, "state.json", settings)
}
fn write_journal(store: &Store, journal: &Journal) -> Result<(), String> {
    validate_journal(journal, store.scope)?;
    write_json(&store.manager, "pending.json", journal)
}
fn write_receipt(store: &Store, record: &PendingRecord) -> Result<(), String> {
    write_json(
        &store.manager,
        &format!("receipts/{}.json", record.ticket),
        &serde_json::json!({"format":2,"scope":store.scope,"record":record}),
    )
}
fn staged_path(store: &Store, record: &PendingRecord) -> Result<PathBuf, String> {
    validate_record(record)?;
    checked_child(
        &store.manager,
        &Path::new("staged")
            .join(&record.ticket)
            .join(&record.destination),
        false,
    )
}
fn backup_path(store: &Store, record: &PendingRecord) -> Result<PathBuf, String> {
    checked_child(
        &store.manager,
        &Path::new("backups")
            .join(&record.ticket)
            .join(&record.destination),
        false,
    )
}
fn object_path(store: &Store, record: &GlobalRecord) -> Result<PathBuf, String> {
    if !valid_digest(&record.digest) {
        return Err("全局对象摘要无效".into());
    }
    validate_component(&record.filename)?;
    checked_child(
        &store.plugins,
        &Path::new("objects")
            .join(&record.digest)
            .join(&record.filename),
        false,
    )
}
fn ticket() -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT_TICKET.fetch_add(1, Ordering::Relaxed)
    )
}
fn project_dependencies(root: &Path) -> Result<BTreeSet<String>, String> {
    let descriptor = checked_child(root, Path::new("editor-project.json"), false)?;
    let relative = if descriptor.exists() {
        let value: serde_json::Value = read_json(&descriptor, MAX_JSON)?;
        value
            .get("cargo_manifest")
            .and_then(|v| v.as_str())
            .unwrap_or("Cargo.toml")
            .to_string()
    } else {
        "Cargo.toml".into()
    };
    let path = checked_child(root, Path::new(&relative), true)?;
    let bytes = read_limited(&path, 256 * 1024)?;
    let source = std::str::from_utf8(&bytes).map_err(error)?;
    let manifest: toml::Value =
        toml::from_str(source).map_err(|e| format!("运行端依赖检查失败：{e}"))?;
    let mut dependencies = BTreeSet::new();
    add_dependencies(manifest.get("dependencies"), &mut dependencies);
    if let Some(targets) = manifest.get("target").and_then(|v| v.as_table()) {
        for target in targets.values() {
            add_dependencies(target.get("dependencies"), &mut dependencies);
        }
    }
    Ok(dependencies)
}
fn add_dependencies(value: Option<&toml::Value>, names: &mut BTreeSet<String>) {
    if let Some(table) = value.and_then(|v| v.as_table()) {
        for (key, dependency) in table {
            names.insert(key.clone());
            if let Some(package) = dependency.get("package").and_then(|v| v.as_str()) {
                names.insert(package.to_string());
            }
        }
    }
}
fn relative_normal(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|c| matches!(c,Component::Normal(part)if !part.to_string_lossy().contains(':')))
}
fn validate_component(value: &str) -> Result<(), String> {
    if !relative_normal(Path::new(value)) || Path::new(value).components().count() != 1 {
        return Err("插件事务路径必须是普通单级文件名".into());
    }
    Ok(())
}
fn checked_child(root: &Path, relative: &Path, required: bool) -> Result<PathBuf, String> {
    if !relative_normal(relative) {
        return Err(format!("插件路径不是普通相对路径：{}", relative.display()));
    }
    let mut path = root.to_path_buf();
    if root.exists() {
        ordinary(root)?;
    }
    for component in relative.components() {
        path.push(component.as_os_str());
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                ordinary(&path)?;
                let real = path.canonicalize().map_err(error)?;
                let canonical = canonical_root(root, false)?;
                if !real.starts_with(canonical) {
                    return Err("插件路径越界".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound && !required => {}
            Err(e) => return Err(format!("{}：{e}", crate::project::display_path(&path))),
        }
    }
    Ok(path)
}
fn ordinary(path: &Path) -> Result<fs::Metadata, String> {
    let info = fs::symlink_metadata(path)
        .map_err(|e| format!("{}：{e}", crate::project::display_path(path)))?;
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        info.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if info.file_type().is_symlink() || reparse || !(info.is_file() || info.is_dir()) {
        return Err(format!(
            "插件路径不能经过链接、junction或特殊文件：{}",
            crate::project::display_path(path)
        ));
    }
    Ok(info)
}
fn ordinary_chain(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        if !ancestor.as_os_str().is_empty() {
            ordinary(ancestor)?;
        }
    }
    Ok(())
}
fn ensure_parent(root: &Path, path: &Path) -> Result<(), String> {
    let parent = path.parent().ok_or("插件事务路径没有父目录")?;
    let relative = parent.strip_prefix(root).map_err(|_| "插件事务路径越界")?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err("插件事务路径越界".into());
        }
        current.push(component);
        if !current.exists() {
            fs::create_dir(&current).map_err(error)?;
        }
        if !ordinary(&current)?.is_dir()
            || !current
                .canonicalize()
                .map_err(error)?
                .starts_with(root.canonicalize().map_err(error)?)
        {
            return Err("插件事务父路径越界".into());
        }
    }
    Ok(())
}
fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    if !ordinary(path)?.is_file() {
        return Err("插件配置不是普通文件".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(error)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() > limit {
        return Err(format!(
            "插件文件超过{limit}字节预算：{}",
            crate::project::display_path(path)
        ));
    }
    Ok(bytes)
}
fn read_json<T: DeserializeOwned>(path: &Path, limit: usize) -> Result<T, String> {
    let bytes = read_limited(path, limit)?;
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    serde_json::from_slice(bytes)
        .map_err(|e| format!("{}：{e}", crate::project::display_path(path)))
}
fn write_json(root: &Path, relative: &str, value: &impl Serialize) -> Result<(), String> {
    let path = checked_child(root, Path::new(relative), false)?;
    ensure_parent(root, &path)?;
    let bytes = serde_json::to_vec_pretty(value).map_err(error)?;
    if bytes.len() > MAX_JSON {
        return Err("插件管理配置超过128KiB预算".into());
    }
    let temporary = path.with_extension(format!("{}.tmp", ticket()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(error)?;
    let result = (|| {
        file.write_all(&bytes).map_err(error)?;
        file.sync_all().map_err(error)?;
        drop(file);
        fs::rename(&temporary, &path).map_err(error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
/// SHA覆盖唯一二进制全部字节；既不信任版本号，也不沿用旧目录树摘要。
pub(crate) fn binary_digest(path: &Path) -> Result<String, String> {
    ordinary_chain(path)?;
    if !ordinary(path)?.is_file() {
        return Err("插件不是普通单文件".into());
    }
    let mut file = fs::File::open(path).map_err(error)?;
    let mut digest = Sha256::new();
    let mut total = 0u64;
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut chunk).map_err(error)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_BYTES {
            return Err("插件文件读取超过1GiB预算".into());
        }
        digest.update(&chunk[..n]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}
fn copy_binary(source: &Path, destination: &Path) -> Result<(), String> {
    if !ordinary(source)?.is_file() {
        return Err("插件来源必须是普通二进制文件".into());
    }
    let mut input = fs::File::open(source).map_err(error)?;
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(error)?;
    let copied =
        std::io::copy(&mut (&mut input).take(MAX_BYTES + 1), &mut output).map_err(error)?;
    if copied > MAX_BYTES {
        return Err("插件复制超过1GiB预算".into());
    }
    output.sync_all().map_err(error)
}
fn remove_owned_file(manager: &Path, target: &Path) -> Result<(), String> {
    let manager = manager.canonicalize().map_err(error)?;
    ordinary(target)?;
    let target = target.canonicalize().map_err(error)?;
    if !target.starts_with(manager.join("staged")) || !target.is_file() {
        return Err("拒绝清理非插件暂存文件".into());
    }
    fs::remove_file(target).map_err(error)
}
fn error(error: impl std::fmt::Display) -> String {
    error.to_string()
}
