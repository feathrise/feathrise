//! 后台线程使用的受检目录扫描，类型筛选时只保留含匹配后代的路径。

use std::{fs::Metadata, path::{Path, PathBuf}}; // 规范化工作区资产根并拒绝越界目录。

use feathrise_scene_content::SceneAssetKind; // 文件扩展名映射到编辑器用途。

use super::{
    filter::{AssetTypeFilter, DirectoryFilters, DirectorySnapshot, GlbMetadata, ResourceTypes},
    types::{AssetScanContents, AssetScanRequest, AssetScanResult},
    watch::{DirectoryCache, DirectoryWorker, ScanLocation},
};
use crate::types::EditorAssetEntry; // 扫描结果是纯拥有数据，可安全跨线程返回。

/// 回收区只用于人工恢复，不得重新作为可摆放或可选择的正式作者资产出现。
fn hidden_author_directory(name: &str) -> bool {
    name == ".trash" // 当前只隐藏场景删除事务的唯一受控回收目录。
}

/// 两个浏览视图共用目录IO、路径边界、分类与排序；只由来源决定隐藏项和诊断名称。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FolderScope { Assets, ProjectFiles }

impl FolderScope {
    fn root_label(self) -> &'static str {
        match self { Self::Assets => "assets", Self::ProjectFiles => "工程" }
    }
    fn entry_label(self) -> &'static str {
        match self { Self::Assets => "资产", Self::ProjectFiles => "工程文件" }
    }
}

pub(super) fn hidden_project_entry(name: &str) -> bool {
    // 发布暂存前缀全局保留；output只在工程根表示默认导出，不能隐藏src/output等作者目录。
    name.eq_ignore_ascii_case("target")
        || name.starts_with(".feathrise-export-") || name.eq_ignore_ascii_case(".git") || name.eq_ignore_ascii_case(".trash")
}

fn hidden_project_path(root: &Path, path: &Path) -> bool {
    path.strip_prefix(root).is_ok_and(|relative| relative.components().enumerate()
        .any(|(index, part)| {
            let name = part.as_os_str().to_string_lossy();
            hidden_project_entry(&name) || index == 0 && name.eq_ignore_ascii_case("output")
        }))
}

pub(super) fn hidden_path(root: &Path, path: &Path, scope: FolderScope) -> bool {
    match scope {
        FolderScope::ProjectFiles => hidden_project_path(root, path),
        FolderScope::Assets => path.strip_prefix(root).is_ok_and(|relative| relative.components()
            .any(|part| hidden_author_directory(&part.as_os_str().to_string_lossy()))),
    }
}

/// 根据扩展名判断场景编辑器可消费用途。
fn classify(path: &Path) -> SceneAssetKind {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "glb" | "gltf" => SceneAssetKind::Scene,
        "png" | "jpg" | "jpeg" | "ktx2" => SceneAssetKind::Texture,
        _ if path
            .to_str()
            .is_some_and(|path| super::document_format(path).is_some()) =>
        {
            SceneAssetKind::Data // 文本仍属于数据资产，不增加会落盘的SceneAssetKind枚举值。
        }
        "r16" | "bin" => SceneAssetKind::Data,
        _ => SceneAssetKind::Unsupported,
    }
}

/// 把系统路径转换成AssetServer使用的正斜杠相对路径。
fn asset_relative_path(root: &Path, path: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("资产路径越过assets根：{}", path.display()))?;
    Ok(relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/"))
}

/// 扫描当前一层目录，目录优先、名称不区分大小写排序。
#[cfg(test)]
pub(super) fn scan_asset_folder(
    asset_root: PathBuf,
    current_folder: String,
) -> Result<Vec<EditorAssetEntry>, String> {
    scan_folder(asset_root, current_folder, FolderScope::Assets)
}

#[cfg(test)]
pub(super) fn scan_folder(
    asset_root: PathBuf,
    current_folder: String,
    scope: FolderScope,
) -> Result<Vec<EditorAssetEntry>, String> {
    scan_directory(asset_root, current_folder, DirectoryQuery {
        context: 0, scope, asset_root: None, filters: &DirectoryFilters::default(), reveal_file: None,
    }, &DirectoryWorker::default()).map(|snapshot| snapshot.entries)
}

pub(super) struct DirectoryQuery<'a> {
    pub(super) context: u64,
    pub(super) scope: FolderScope,
    pub(super) asset_root: Option<&'a Path>,
    pub(super) filters: &'a DirectoryFilters,
    pub(super) reveal_file: Option<&'a str>,
}

/// 现有目录任务完成全部筛选及排序，列表和缩略图采用同一个快照。
pub(super) fn scan_directory(
    source: PathBuf, current_folder: String, query: DirectoryQuery<'_>, worker: &DirectoryWorker,
) -> Result<DirectorySnapshot, String> {
    let scope = query.scope;
    let root_label = scope.root_label();
    let entry_label = scope.entry_label();
    let root = source
        .canonicalize()
        .map_err(|error| format!("无法定位{root_label}根{}：{error}", source.display()))?;
    let requested = if current_folder.is_empty() {
        root.clone()
    } else {
        root.join(&current_folder)
    };
    let folder = requested
        .canonicalize()
        .map_err(|error| format!("无法打开{entry_label}目录{}：{error}", requested.display()))?;
    if !folder.starts_with(&root) {
        return Err(format!("{entry_label}目录越过{root_label}根：{}", folder.display()));
    }
    if matches!(scope, FolderScope::ProjectFiles) && hidden_project_path(&root, &folder) {
        return Err("工程文件目录指向隐藏的构建/Git/回收目录".into());
    }
    let asset_root = match scope {
        FolderScope::Assets => Some(root.clone()),
        FolderScope::ProjectFiles => query.asset_root.and_then(|path| path.canonicalize().ok())
            .filter(|asset_root| asset_root.starts_with(&root)),
    };
    worker.run(|cache| {
        worker.prepare(cache, ScanLocation {
            context: query.context, root: root.clone(), folder: folder.clone(), scope, asset_root: asset_root.clone(),
        }, query.filters.kind != AssetTypeFilter::All);
        let source = CheckedFolder { root: &root, asset_root: asset_root.as_deref(), scope };
        source.snapshot(&folder, query.filters, query.reveal_file, cache)
    })
}

struct CheckedFolder<'a> {
    root: &'a Path,
    asset_root: Option<&'a Path>,
    scope: FolderScope,
}

struct ScanEntry {
    path: PathBuf,
    display_name: String,
    metadata: Metadata,
}

impl CheckedFolder<'_> {
    fn read(&self, folder: &Path, visible_layer: bool) -> Result<Vec<ScanEntry>, String> {
        let scope = self.scope;
        let root = self.root;
        let root_label = scope.root_label();
        let entry_label = scope.entry_label();
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(folder)
            .map_err(|error| format!("读取{entry_label}目录{}失败：{error}", folder.display()))?
        {
            // 完整目录不设置产品数量上限；IO、排序与真实内存申请仍全部留在唯一后台槽。
            let entry = entry.map_err(|error| format!("读取目录项失败：{error}"))?;
            let display_name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if matches!(scope, FolderScope::ProjectFiles) && hidden_project_path(&root, &path) {
                continue; // 不枚举构建产物或Git内部目录，含同名链接与worktree的.git文件。
            }
            let metadata = entry
                .metadata()
                .map_err(|error| format!("读取{}元数据失败：{error}", path.display()))?;
            // 递归筛选不跟随重解析点，避免循环、重复目录和目录范围意外扩张。
            if !visible_layer && is_link(&metadata) {
                continue;
            }
            let resolved = path
                .canonicalize()
                .map_err(|error| format!("解析{}失败：{error}", path.display()))?;
            if !resolved.starts_with(&root) {
                return Err(format!(
                    "{entry_label}目录项越过{root_label}根：{}",
                    path.display()
                ));
            }
            if matches!(scope, FolderScope::ProjectFiles) && hidden_project_path(&root, &resolved) {
                continue; // 工程内链接也不能用别名暴露构建、Git或作者回收目录。
            }
            let is_directory = metadata.is_dir();
            if !is_directory && !metadata.is_file() {
                continue;
            }
            if matches!(scope, FolderScope::Assets)
                && is_directory
                && hidden_author_directory(&display_name)
            {
                continue; // 资产浏览器不暴露已从catalog移除的场景私有文件。
            }
            if !visible_layer
                && matches!(scope, FolderScope::Assets)
                && hidden_path(root, &resolved, scope)
            {
                continue;
            }
            entries.try_reserve(1).map_err(|error| {
                format!(
                    "{entry_label}目录{}无法分配完整条目列表：{error}",
                    folder.display()
                )
            })?;
            entries.push(ScanEntry {
                path,
                display_name,
                metadata,
            });
        }
        Ok(entries)
    }

    fn snapshot(
        &self,
        folder: &Path,
        filters: &DirectoryFilters,
        reveal_file: Option<&str>,
        cache: &mut DirectoryCache,
    ) -> Result<DirectorySnapshot, String> {
        let mut snapshot = DirectorySnapshot::default();
        let filename = filters.filename.to_lowercase();
        let filter = filters.kind;
        let with_glb = filter.needs_glb_metadata();
        let reveal_path = reveal_file.map(|path| self.root.join(path));
        for entry in self.read(folder, true)? {
            let is_directory = entry.metadata.is_dir();
            let mut animation = None;
            if is_directory {
                if filter == AssetTypeFilter::Asset
                    && self.asset_root.is_none_or(|root| {
                        !entry.path.starts_with(root) && !root.starts_with(&entry.path)
                    })
                {
                    continue; // 目录与登记根没有交集，里面不会有登记资产。
                }
                if filter != AssetTypeFilter::All
                    && (is_link(&entry.metadata)
                        || !self
                            .subtree_types(entry.path.clone(), with_glb, cache)?
                            .matches(filter))
                {
                    continue;
                }
            } else {
                if !filename.is_empty() && !entry.display_name.to_lowercase().contains(&filename) {
                    continue;
                }
                if filter != AssetTypeFilter::All {
                    let (types, metadata) = self.file_types(&entry, with_glb, cache);
                    if !types.matches(filter) {
                        if reveal_path.as_ref() == Some(&entry.path) {
                            return Err("当前类型筛选隐藏了定位目标，请选择「全部」后重试".into());
                        }
                        continue;
                    }
                    if filter == AssetTypeFilter::Animation && self.scope == FolderScope::Assets {
                        animation = metadata.map(|metadata| metadata.clips);
                    }
                }
            }
            let asset_path = asset_relative_path(self.root, &entry.path)?;
            if let Some(clips) = animation {
                snapshot.animations.insert(asset_path.clone(), clips);
            }
            snapshot.entries.try_reserve(1).map_err(|error| {
                format!("目录{}无法分配完整条目列表：{error}", folder.display())
            })?;
            snapshot.entries.push(EditorAssetEntry {
                asset_path,
                display_name: entry.display_name,
                is_directory,
                kind: if is_directory {
                    SceneAssetKind::Unsupported
                } else {
                    classify(&entry.path)
                },
            });
        }
        snapshot.entries.sort_by(|left, right| {
            right
                .is_directory
                .cmp(&left.is_directory)
                .then_with(|| {
                    left.display_name
                        .to_lowercase()
                        .cmp(&right.display_name.to_lowercase())
                })
                .then_with(|| left.asset_path.cmp(&right.asset_path)) // 同折叠名称按完整路径稳定排序，定位页码不随机变化。
        });
        Ok(snapshot)
    }

    fn file_types(
        &self,
        entry: &ScanEntry,
        with_glb: bool,
        cache: &mut DirectoryCache,
    ) -> (ResourceTypes, Option<GlbMetadata>) {
        let mut types = ResourceTypes::from_path(
            &entry.path,
            self.asset_root
                .is_some_and(|root| entry.path.starts_with(root)),
        );
        let metadata = (with_glb && types.matches(AssetTypeFilter::Glb))
            .then(|| cache.glbs.read(&entry.path, &entry.metadata));
        if let Some(metadata) = &metadata {
            types.merge(metadata.types);
        }
        (types, metadata)
    }

    fn subtree_types(
        &self,
        path: PathBuf,
        with_glb: bool,
        cache: &mut DirectoryCache,
    ) -> Result<ResourceTypes, String> {
        if let Some(types) = cache.folder_types(&path, with_glb) {
            return Ok(types);
        }
        // 后序汇总，每层只缓存匹配位，能在一次解析中为多种筛选保存完整祖先路径。
        let mut stack = vec![(
            self.read(&path, false)?.into_iter(),
            path,
            ResourceTypes::default(),
        )];
        while let Some((entries, _, types)) = stack.last_mut() {
            if let Some(entry) = entries.next() {
                if entry.metadata.is_dir() {
                    if let Some(cached) = cache.folder_types(&entry.path, with_glb) {
                        types.merge(cached);
                    } else {
                        stack.push((
                            self.read(&entry.path, false)?.into_iter(),
                            entry.path,
                            ResourceTypes::default(),
                        ));
                    }
                } else {
                    types.merge(self.file_types(&entry, with_glb, cache).0);
                }
            } else {
                let (_, path, types) = stack.pop().unwrap();
                cache.remember_folder(path, with_glb, types);
                let Some((_, _, parent)) = stack.last_mut() else {
                    return Ok(types);
                };
                parent.merge(types);
            }
        }
        unreachable!("目录遍历至少有一个根")
    }
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

/// 唯一 IO 任务先完整扫描排序，再在后台精确定位文件；主线程不再线性搜索整表。
pub(super) fn scan_asset_request(
    asset_root: PathBuf,       // 本工作树的唯一资产根，规范化和越界校验在后台执行。
    request: AssetScanRequest, // 所有可影响采用的输入身份都随请求冻结。
    worker: &DirectoryWorker,
) -> AssetScanResult {
    let snapshot = scan_directory(asset_root, request.folder.clone(), DirectoryQuery {
        context: request.context, scope: FolderScope::Assets, asset_root: None, filters: &request.filters,
        reveal_file: request.reveal_file.as_deref(),
    }, worker);
    finish_asset_snapshot(request, snapshot)
}

/// 对完整排序结果计算可见页所需索引；测试可以注入纯条目，生产只从 IO 任务调用。
#[cfg(test)]
pub(super) fn finish_asset_scan(
    request: AssetScanRequest, // 冻结的父目录、全文件路径和修订。
    entries: Result<Vec<EditorAssetEntry>, String>, // 失败保持具体 IO 诊断，不伪造空目录成功。
) -> AssetScanResult {
    finish_asset_snapshot(request, entries.map(|entries| DirectorySnapshot { entries, ..Default::default() }))
}

fn finish_asset_snapshot(request: AssetScanRequest, snapshot: Result<DirectorySnapshot, String>) -> AssetScanResult {
    let result = snapshot.and_then(|snapshot| {
        let reveal_index = if let Some(file_path) = request.reveal_file.as_deref() {
            Some(
                snapshot.entries
                    .iter()
                    .position(|entry| !entry.is_directory && entry.asset_path == file_path)
                    .ok_or_else(|| {
                        format!(
                            "在 assets/{} 中找不到文件 assets/{file_path}",
                            request.folder
                        )
                    })?,
            ) // 必须完整路径一致，不能匹配别处同名文件。
        } else {
            None // 普通浏览不需要全表定位。
        };
        Ok(AssetScanContents {
            snapshot,    // 目录条目与片段索引一起原子采用。
            reveal_index, // 页码由现有十二行容量在采用点计算。
        })
    });
    AssetScanResult { request, result } // 完成值保留全部输入身份供主线程再次核对。
}

#[cfg(test)]
mod io_tests; // 超过旧目录上限与根外路径均在独立临时夹具中验证。

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_extensions_have_typed_editor_roles() {
        assert_eq!(classify(Path::new("tree.glb")), SceneAssetKind::Scene);
        assert_eq!(classify(Path::new("grass.PNG")), SceneAssetKind::Texture);
        assert_eq!(classify(Path::new("density.ktx2")), SceneAssetKind::Texture);
        for path in [
            "README.MD", "shader.WGSL", "tool.py", "config.TOML", "dialogue.yarn", "README",
        ] {
            assert_eq!(classify(Path::new(path)), SceneAssetKind::Data, "{path}");
        }
        assert_eq!(classify(Path::new("sound.ogg")), SceneAssetKind::Unsupported);
    }

    #[test]
    fn author_trash_is_hidden_from_regular_asset_browsing() {
        assert!(hidden_author_directory(".trash")); // 删除备份不能被误选为正式场景资产。
        assert!(!hidden_author_directory("terrain")); // 普通场景私有目录仍可浏览。
    }

    #[test]
    fn only_project_root_output_is_hidden_while_reserved_names_remain_global() {
        let root = Path::new("fixture-project");
        for path in ["output", "output/game.exe", "OUTPUT/assets/scene.glb",
            ".feathrise-export-stage", "src/.feathrise-export-stage/data", "src/target",
            "src/.git/config", "assets/.trash/deleted.glb"] {
            assert!(hidden_project_path(root, &root.join(path)), "{path}");
        }
        for path in ["src/output", "src/output/module.rs", "assets/output/texture.png", "output_helpers"] {
            assert!(!hidden_project_path(root, &root.join(path)), "{path}");
        }
    }
}
