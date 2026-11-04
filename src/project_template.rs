//! 在后台创建独立工程和正式地图种子；先完成独占临时目录，再发布成作者工程。
//! SDK 与渲染补丁由同一份来源配置解析，工程打开和工程运行不隐式升级作者依赖。

#[path = "project_template_empty.rs"]
mod empty;
#[path = "project_template_files.rs"]
mod files;
#[path = "project_template_sdk.rs"]
mod sdk;
pub(crate) use sdk::apply_project_network_policy;

use crate::build_process::BuildControl;
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_TEMPLATE_FILES: usize = 512;
// 模板复制仍在唯一可取消后台槽执行，并保留原文件数和字节预算。
const MAX_TEMPLATE_BYTES: u64 = 512 * 1024 * 1024;

/// 新建工程的依赖锁定也是构建工具任务；切工程／退出必须能取消其专属子树。
pub(crate) fn create_with_control(
    destination: &Path,
    name: &str,
    template_root: &Path,
    control: &BuildControl,
) -> Result<PathBuf, String> {
    create_empty_with_control(
        destination,
        name,
        template_root,
        control,
        Default::default(),
    )
}

/// 默认工程只包含作者选定的场景视图和空目录；视角随工程保存，不借用原游戏内容。
pub(crate) fn create_empty_with_control(
    destination: &Path,
    name: &str,
    template_root: &Path,
    control: &BuildControl,
    game_view: feathrise_runtime::GameView,
) -> Result<PathBuf, String> {
    create_checked(
        destination,
        name,
        template_root,
        control,
        game_view,
    )
}

fn create_checked(
    destination: &Path,
    name: &str,
    template_root: &Path,
    control: &BuildControl,
    game_view: feathrise_runtime::GameView,
) -> Result<PathBuf, String> {
    control.check()?;
    let name = name.trim();
    if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err("工程名称必须为 1—256 字节的普通文本".into());
    }
    let destination = checked_destination(destination)?;
    let parent = destination.parent().ok_or("工程目录缺少父目录")?;
    let id = new_uuid()?;
    let temporary = parent.join(format!(".editor-create-{id}"));
    control.check()?;
    fs::create_dir(&temporary).map_err(|e| format!("无法创建工程临时目录：{e}"))?;
    // Guard 只清理本次 create_dir 成功创建的独占目录，从不清理作者选择的目标。
    let mut guard = TemporaryProject {
        path: Some(temporary.clone()),
        parent: parent.to_path_buf(),
        control: control.clone(),
    };
    empty::write(&temporary, name, &id, template_root, control, game_view)?;

    // 作者可能在后台准备期间向原空目录写入文件；发布前重新核验，绝不覆盖这些内容。
    control.check()?;
    require_empty_or_absent(&destination)?;
    // 最后一次取消检查之后才开始发布。此短临界段不半途取消，成功发布的作者路径绝不由Guard删除。
    if destination.exists() {
        fs::remove_dir(&destination).map_err(|e| format!("目标目录已不再为空：{e}"))?;
    }
    fs::rename(&temporary, &destination).map_err(|e| format!("无法发布新工程：{e}"))?;
    guard.path = None;
    destination
        .canonicalize()
        .map_err(|e| format!("无法确认已创建的工程目录：{e}"))
}

/// 同一构建缓存中，每个工程的可执行目标名必须带独立身份。
fn personalize_targets(source: &str, id: &str) -> Result<String, String> {
    if source.matches("default-run = \"project\"").count() != 1
        || source.matches("name = \"project\"").count() != 1
        || source.contains("[lib]") || source.contains("cdylib") {
        return Err("正式工程模板必须只有唯一可执行入口".into());
    }
    let name = format!("project-{id}");
    Ok(source.replacen("default-run = \"project\"", &format!("default-run = \"{name}\""), 1)
        .replacen("name = \"project\"", &format!("name = \"{name}\""), 1))
}

fn toml_path(path: &Path) -> Result<String, String> {
    let path = path.to_str().ok_or("模板依赖路径必须是 Unicode")?;
    // Windows canonicalize 会带扩展前缀；普通 Cargo 源路径保持盘符形式，避免 NGX 扩展路径缺陷。
    let path = path
        .strip_prefix(r"\\?\")
        .unwrap_or(path)
        .replace('\\', "/");
    if path.contains(['"', '\r', '\n']) {
        return Err("模板依赖路径包含 TOML 不支持的字符".into());
    }
    Ok(path)
}

fn checked_destination(destination: &Path) -> Result<PathBuf, String> {
    let absolute = if destination.is_absolute() {
        destination.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(destination)
    };
    let name = absolute.file_name().ok_or("请选择新的工程子目录")?;
    let parent = absolute.parent().ok_or("工程目录缺少父目录")?;
    // 显式拒绝已有路径中的符号链接和 junction，不能借新建操作写入作者未选择的目录。
    let mut cursor = Some(parent);
    while let Some(path) = cursor {
        reject_reparse(path)?;
        cursor = path.parent();
    }
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("工程父目录无效：{e}"))?;
    let destination = parent.join(name);
    require_empty_or_absent(&destination)?;
    Ok(destination)
}

fn require_empty_or_absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            reject_reparse_metadata(&metadata)?;
            if !metadata.is_dir() {
                return Err("新工程目标必须是目录".into());
            }
            if fs::read_dir(path)
                .map_err(|e| e.to_string())?
                .next()
                .is_some()
            {
                return Err("目标目录已有内容，拒绝覆盖作者文件".into());
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("无法检查新工程目标：{error}")),
    }
}

fn reject_reparse(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("无法检查模板路径：{e}"))?;
    reject_reparse_metadata(&metadata)
}

fn reject_reparse_metadata(metadata: &fs::Metadata) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    if metadata.file_type().is_symlink() || metadata.file_attributes() & 0x400 != 0 {
        return Err("新工程和模板路径不能经过符号链接或 junction".into());
    }
    Ok(())
}

#[derive(Default)]
struct CopyBudget {
    files: usize,
    bytes: u64,
}

fn copy_tree(
    source: &Path,
    destination: &Path,
    budget: &mut CopyBudget,
    control: &BuildControl,
) -> Result<(), String> {
    control.check()?;
    reject_reparse(source)?;
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        control.check()?;
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
        reject_reparse_metadata(&metadata)?;
        let target = destination.join(entry.file_name());
        if metadata.is_dir() {
            copy_tree(&entry.path(), &target, budget, control)?;
        } else if metadata.is_file() {
            budget.files += 1;
            budget.bytes = budget.bytes.saturating_add(metadata.len());
            if budget.files > MAX_TEMPLATE_FILES || budget.bytes > MAX_TEMPLATE_BYTES {
                return Err("模板超过 512 文件或 512 MiB 固定复制预算".into());
            }
            fs::copy(entry.path(), target).map_err(|e| e.to_string())?;
        } else {
            return Err("模板只允许普通目录和文件".into());
        }
    }
    Ok(())
}

struct TemporaryProject {
    path: Option<PathBuf>,
    parent: PathBuf,
    control: BuildControl,
}

impl Drop for TemporaryProject {
    fn drop(&mut self) {
        // 未确认工具退出时保留现场，避免删除仍被写入的目录；作者目录从不放进此守卫。
        if self.control.cleanup_failed() {
            return;
        }
        if let Some(path) = self.path.take() {
            // 递归删除前复核绝对真实父目录、UUID临时名称和reparse位，不跟随被替换的链接。
            if let Ok(actual) = path.canonicalize() {
                if actual.parent() == Some(self.parent.as_path())
                    && actual
                        .file_name()
                        .is_some_and(|name| name.to_string_lossy().starts_with(".editor-create-"))
                    && reject_reparse(&path).is_ok()
                {
                    let _ = fs::remove_dir_all(actual);
                }
            }
        }
    }
}

fn new_uuid() -> Result<String, String> {
    #[repr(C)]
    struct Guid {
        first: u32,
        second: u16,
        third: u16,
        tail: [u8; 8],
    }
    #[link(name = "ole32")]
    unsafe extern "system" {
        fn CoCreateGuid(guid: *mut Guid) -> i32;
    }
    let mut guid = Guid {
        first: 0,
        second: 0,
        third: 0,
        tail: [0; 8],
    };
    let result = unsafe { CoCreateGuid(&mut guid) };
    if result < 0 {
        return Err(format!("无法生成工程 UUID：0x{:08x}", result as u32));
    }
    Ok(format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        guid.first,
        guid.second,
        guid.third,
        guid.tail[0],
        guid.tail[1],
        guid.tail[2],
        guid.tail[3],
        guid.tail[4],
        guid.tail[5],
        guid.tail[6],
        guid.tail[7]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_projects_preserve_both_views_without_game_content_and_reopen_independently() {
        let scratch = std::env::temp_dir().join(format!("feathrise-empty-{}", new_uuid().unwrap()));
        fs::create_dir(&scratch).unwrap();
        let scratch = scratch.canonicalize().unwrap();
        let mut identities = Vec::new();
        for (index, view) in [
            feathrise_runtime::GameView::ThirdPerson,
            feathrise_runtime::GameView::FixedTopDown,
        ]
        .into_iter()
        .enumerate()
        {
            let root = create_empty_with_control(
                &scratch.join(format!("project-{index}")),
                "空工程",
                &crate::project_template_root(),
                &BuildControl::default(),
                view,
            )
            .unwrap();
            let project =
                feathrise_runtime::ProjectContext::load(&root, "feathrise_game_template").unwrap();
            assert_eq!(feathrise_runtime::GameView::load(&project).unwrap(), view);
            let maps = feathrise_project::ProjectMaps::load(project.asset_root()).unwrap();
            assert!(maps.entries.is_empty());
            assert!(maps.preset_counts.values().all(|count| *count == 0));
            identities.push(project.id);
            for forbidden in [
                "assets/gltf",
                "assets/scene",
                "assets/config/player_start.player_start.json",
                "assets/dialogue",
            ] {
                assert!(!root.join(forbidden).exists(), "{forbidden}");
            }
            let code = fs::read_to_string(root.join("src/controller.rs")).unwrap();
            assert!(code.contains("ProjectScenePlugin"));
            let descriptor: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join("editor-project.json")).unwrap())
                    .unwrap();
            assert_eq!(descriptor["game_view"], serde_json::to_value(view).unwrap());
            assert_eq!(descriptor["package"], "feathrise_game_template");
            let manifest = fs::read_to_string(root.join("Cargo.toml")).unwrap();
            assert!(!manifest.contains("apps/game"));
            assert!(!manifest.contains("rpg_editor_"));
            assert!(!manifest.contains("feathrise_editor_"));
            let manifest: toml::Value = toml::from_str(&manifest).unwrap();
            assert_eq!(manifest["package"]["name"].as_str(), descriptor["package"].as_str());
            assert_eq!(manifest["profile"]["dev"]["opt-level"].as_integer(), Some(0));
            assert_eq!(manifest["profile"]["dev"]["package"]["*"]["opt-level"].as_integer(), Some(3));
            assert_eq!(manifest["profile"]["render-dev"]["inherits"].as_str(), Some("dev"));
            assert!(manifest["dependencies"].get("feathrise_runtime").is_some());
            assert!(
                manifest["dependencies"]
                    .get("feathrise_project_runtime")
                    .is_none()
            );
            assert!(
                manifest["dependencies"]
                    .get("new_editor_game_runtime")
                    .is_none()
            );
            let main = fs::read_to_string(root.join("src/main.rs")).unwrap();
            assert!(main.contains("feathrise_runtime::run("));
            assert!(code.contains("use feathrise_runtime::{"));
            // 验证真正创建到磁盘的空目录，而非只检查内存种子或模板字符串。
            assert!(
                !fs::read_to_string(root.join("assets/maps/catalog.map_catalog.json"))
                    .unwrap()
                    .contains("M1-SAMPLE")
            );
        }
        assert_ne!(identities[0], identities[1]);
        // 仅清理本测试创建并重新核对的UUID目录，不接触作者工程或Cargo缓存。
        assert_eq!(
            scratch.parent(),
            Some(std::env::temp_dir().canonicalize().unwrap().as_path())
        );
        if std::env::var_os("FEATHRISE_KEEP_EMPTY_TEST_PROJECTS").is_some() {
            // 仅显式运行验收时保留本测试生成的UUID目录，供实际Cargo启动两种视角；普通单测仍清理。
            println!("FEATHRISE_EMPTY_TEST_PROJECTS={}", scratch.display());
        } else {
            fs::remove_dir_all(scratch).unwrap();
        }
    }

    #[test]
    fn uuid_is_independent_and_canonical() {
        let left = new_uuid().unwrap();
        let right = new_uuid().unwrap();
        assert_ne!(left, right);
        assert_eq!(left.len(), 36);
        assert_eq!(left.as_bytes()[8], b'-');
        assert_eq!(left.as_bytes()[13], b'-');
    }

    #[test]
    fn populated_destination_is_never_overwritten() {
        let root = std::env::temp_dir().join(format!("editor-create-test-{}", new_uuid().unwrap()));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("author.txt"), b"keep").unwrap();
        assert!(checked_destination(&root).is_err());
        assert_eq!(fs::read(root.join("author.txt")).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pre_cancelled_create_does_not_create_temporary_or_touch_author_destination() {
        let root =
            std::env::temp_dir().join(format!("editor-cancel-create-{}", new_uuid().unwrap()));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let author = root.join("作者现有目录");
        fs::create_dir(&author).unwrap();
        fs::write(author.join("keep.txt"), b"author bytes").unwrap();
        let control = BuildControl::default();
        control.cancel();
        for destination in [&author, &root.join("尚未创建")] {
            let error = create_with_control(
                destination,
                "取消测试",
                Path::new("missing-template"),
                &control,
            )
            .unwrap_err();
            assert!(error.contains("取消"));
        }
        assert_eq!(fs::read(author.join("keep.txt")).unwrap(), b"author bytes");
        assert!(!root.join("尚未创建").exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        // 本测试只删除已核对仍位于其UUID临时根内的自有路径。
        assert!(author.canonicalize().unwrap().starts_with(&root));
        fs::remove_file(author.join("keep.txt")).unwrap();
        fs::remove_dir(author).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn cancelled_temporary_guard_only_removes_its_unpublished_project() {
        let root =
            std::env::temp_dir().join(format!("editor-cancel-guard-{}", new_uuid().unwrap()));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let temporary = root.join(format!(".editor-create-{}", new_uuid().unwrap()));
        fs::create_dir(&temporary).unwrap();
        fs::write(temporary.join("partial.txt"), b"unpublished").unwrap();
        fs::write(root.join("author.txt"), b"author bytes").unwrap();
        let control = BuildControl::default();
        control.cancel();
        drop(TemporaryProject {
            path: Some(temporary.clone()),
            parent: root.clone(),
            control,
        });
        assert!(!temporary.exists());
        assert_eq!(fs::read(root.join("author.txt")).unwrap(), b"author bytes");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_file(root.join("author.txt")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn production_template_is_bin_only_and_cannot_accidentally_include_the_prototype_library() {
        let source = include_str!("../assets/templates/empty/Cargo.toml");
        let a = personalize_targets(source, "first").unwrap();
        let b = personalize_targets(source, "second").unwrap();
        assert!(a.contains("default-run = \"project-first\""));
        assert!(b.contains("name = \"project-second\""));
        assert_ne!(a, b); // 共享缓存不能把另一工程的顶层可执行文件当作自己的。
        assert!(!a.contains("[lib]"));
        let contaminated = format!("{source}\n[lib]\ncrate-type = [\"cdylib\"]\n");
        assert!(personalize_targets(&contaminated, "first").is_err());
    }

}
