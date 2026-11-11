//! 后台线程使用的安全单层assets目录扫描。

use std::path::{Path, PathBuf}; // 规范化工作区资产根并拒绝越界目录。

use feathrise_scene_content::SceneAssetKind; // 文件扩展名映射到编辑器用途。

use super::types::{AssetScanContents, AssetScanRequest, AssetScanResult}; // 定位索引与完整目录在同一后台批次生成。
use crate::types::EditorAssetEntry; // 扫描结果是纯拥有数据，可安全跨线程返回。

/// 回收区只用于人工恢复，不得重新作为可摆放或可选择的正式作者资产出现。
fn hidden_author_directory(name: &str) -> bool {
    name == ".trash" // 当前只隐藏场景删除事务的唯一受控回收目录。
}

/// 两个浏览视图共用目录IO、路径边界、分类与排序；只由来源决定隐藏项和诊断名称。
#[derive(Clone, Copy)]
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
pub(super) fn scan_asset_folder(
    asset_root: PathBuf,
    current_folder: String,
) -> Result<Vec<EditorAssetEntry>, String> {
    scan_folder(asset_root, current_folder, FolderScope::Assets)
}

/// 单层扫描完整结果；目录枚举和任意规模排序只在调用方的后台槽运行。
pub(super) fn scan_folder(
    asset_root: PathBuf,
    current_folder: String,
    scope: FolderScope,
) -> Result<Vec<EditorAssetEntry>, String> {
    let root_label = scope.root_label();
    let entry_label = scope.entry_label();
    let root = asset_root
        .canonicalize()
        .map_err(|error| format!("无法定位{root_label}根{}：{error}", asset_root.display()))?;
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
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&folder)
        .map_err(|error| format!("读取{entry_label}目录{}失败：{error}", folder.display()))?
    {
        // 完整目录不设置产品数量上限；IO、排序与真实内存申请仍全部留在唯一后台槽。
        let entry = entry.map_err(|error| format!("读取目录项失败：{error}"))?;
        let display_name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        if matches!(scope, FolderScope::ProjectFiles)
            && hidden_project_path(&root, &path) {
            continue; // 不枚举构建产物或Git内部目录，含同名链接与worktree的.git文件。
        }
        let resolved = path
            .canonicalize()
            .map_err(|error| format!("解析{}失败：{error}", path.display()))?;
        if !resolved.starts_with(&root) {
            return Err(format!("{entry_label}目录项越过{root_label}根：{}", path.display()));
        }
        if matches!(scope, FolderScope::ProjectFiles) && hidden_project_path(&root, &resolved) {
            continue; // 工程内链接也不能用别名暴露构建、Git或作者回收目录。
        }
        let metadata = entry
            .metadata()
            .map_err(|error| format!("读取{}元数据失败：{error}", path.display()))?;
        let is_directory = metadata.is_dir();
        if !is_directory && !metadata.is_file() {
            continue;
        }
        if matches!(scope, FolderScope::Assets) && is_directory && hidden_author_directory(&display_name) {
            continue; // 资产浏览器不暴露已从catalog移除的场景私有文件。
        }
        let asset_path = asset_relative_path(&root, &path)?;
        entries.try_reserve(1).map_err(|error| {
            format!("{entry_label}目录{}无法分配完整条目列表：{error}", folder.display()) // 只报告真实系统资源不足，不静默截断目录。
        })?;
        entries.push(EditorAssetEntry {
            asset_path,
            display_name,
            is_directory,
            kind: if is_directory {
                SceneAssetKind::Unsupported
            } else {
                classify(&path)
            },
        });
    }
    entries.sort_by(|left, right| {
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
    Ok(entries)
}

/// 唯一 IO 任务先完整扫描排序，再在后台精确定位文件；主线程不再线性搜索整表。
pub(super) fn scan_asset_request(
    asset_root: PathBuf,       // 本工作树的唯一资产根，规范化和越界校验在后台执行。
    request: AssetScanRequest, // 所有可影响采用的输入身份都随请求冻结。
) -> AssetScanResult {
    let entries = scan_asset_folder(asset_root, request.folder.clone()); // 所有目录验证和 IO 都在此后台调用内完成。
    finish_asset_scan(request, entries) // 普通扫描与定位共享同一完成值构造和错误语义。
}

/// 对完整排序结果计算可见页所需索引；测试可以注入纯条目，生产只从 IO 任务调用。
pub(super) fn finish_asset_scan(
    request: AssetScanRequest, // 冻结的父目录、全文件路径和修订。
    entries: Result<Vec<EditorAssetEntry>, String>, // 失败保持具体 IO 诊断，不伪造空目录成功。
) -> AssetScanResult {
    let result = entries.and_then(|entries| {
        let reveal_index = if let Some(file_path) = request.reveal_file.as_deref() {
            Some(
                entries
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
            entries,      // 只有完整成功才把所有目录条目转交主线程。
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
