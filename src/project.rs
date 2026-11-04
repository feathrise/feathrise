//! 正式工程的只读清单与路径校验；构建和F12运行由native_play唯一后台槽负责。
use serde::Deserialize;
use std::{fs::File, io::Read, path::{Path, PathBuf}};
#[path = "project_context.rs"]
mod context;
pub use context::ProjectContext;
const MANIFEST_LIMIT: usize = 64 * 1024;
// 只兼容旧清单里的固定字段值；它不再作为正式编辑器与游戏的DLL接口版本。
#[cfg(test)]
pub(crate) const LEGACY_SDK_ABI: u32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub manifest: PathBuf,
    pub root: PathBuf,
    pub package: String,
    pub context: ProjectContext,
}

#[derive(Deserialize)]
struct ProjectManifest {
    format: u32,
    id: String,
    name: String,
    cargo_manifest: PathBuf,
    package: String,
    #[serde(default)]
    asset_roots: Option<Vec<PathBuf>>,
    #[serde(default)]
    editor_data: Option<PathBuf>,
    #[serde(default)]
    startup_scene: Option<context::SceneSelection>,
}

/// 目录与工程清单共用只读入口；普通打开不升级、改写或创建工程文件。
pub fn open(root: &Path) -> Result<Project, String> {
    let selected = root
        .canonicalize()
        .map_err(|e| error(format!("工程目录无效：{e}")))?;
    let root = if selected.is_file() {
        if selected.file_name().is_none_or(|name| name != "editor-project.json") {
            return Err("请选择工程目录或editor-project.json清单".into());
        }
        selected.parent().ok_or("工程清单缺少所在目录")?.to_owned()
    } else {
        selected
    };
    if !root.is_dir() {
        return Err("工程根必须是目录".into());
    }
    let descriptor = contained_file(&root.join("editor-project.json"), &root)?;
    let mut bytes = Vec::with_capacity(MANIFEST_LIMIT.min(4096));
    File::open(&descriptor)
        .map_err(|e| error(format!("无法打开工程清单：{e}")))?
        .take((MANIFEST_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| error(format!("无法读取工程清单：{e}")))?;
    if bytes.len() > MANIFEST_LIMIT {
        return Err("工程清单超过 64 KiB 上限".into());
    }
    let manifest: ProjectManifest =
        serde_json::from_slice(&bytes).map_err(|e| error(format!("工程清单 JSON 无效：{e}")))?;
    if !matches!(manifest.format, 1 | 2) {
        return Err(format!("不支持的工程清单版本：{}", manifest.format));
    }
    valid_text("工程 ID", &manifest.id, 128)?;
    valid_text("工程名称", &manifest.name, 256)?;
    valid_text("Cargo package", &manifest.package, 128)?;
    if manifest.cargo_manifest.as_os_str().is_empty() || manifest.cargo_manifest.is_absolute() {
        return Err("Cargo 入口必须是工程内的相对路径".into());
    }
    // canonical 后再检查包含关系，拒绝 ../、符号链接或 junction 逃出工程根。
    let cargo_manifest = contained_file(&root.join(manifest.cargo_manifest), &root)?;
    // v1继续只读打开；新增路径按工程显式声明解析，不以宿主当前目录补全。
    let context = ProjectContext::resolve(
        &root,
        &manifest.id,
        &manifest.name,
        manifest.format,
        manifest.asset_roots,
        manifest.editor_data,
        manifest.startup_scene,
    )?;
    Ok(Project {
        id: manifest.id,
        name: manifest.name,
        manifest: cargo_manifest,
        root,
        package: manifest.package,
        context,
    })
}

fn valid_text(label: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(format!("{label} 必须非空、不含控制字符且不超过 {max} 字节"));
    }
    Ok(())
}

fn contained_file(path: &Path, root: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| error(format!("工程文件不存在或不可访问：{}：{e}", path.display())))?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return Err("工程文件必须是工程根内的普通文件".into());
    }
    Ok(canonical)
}

fn error(message: String) -> String { message }

/// 工程路径的只读展示：隐藏 Windows 文件系统规范前缀，原 PathBuf 继续承担全部 IO 与身份比较。
pub(crate) fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    let Some(rest) = text.strip_prefix(r"\\?\") else {
        return text.into_owned();
    };
    let bytes = rest.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1..3] == *b":\\" {
        return rest.to_owned();
    }
    if rest.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("UNC\\")) {
        let network = &rest[4..];
        let mut parts = network.split('\\');
        if parts.next().is_some_and(|server| !server.is_empty())
            && parts.next().is_some_and(|share| !share.is_empty())
        {
            return format!(r"\\{network}");
        }
    }
    // 设备、卷 GUID 与不完整前缀不等同于普通盘符/共享路径，避免为美观改变其含义。
    text.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        fs,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn displayed_paths_hide_only_recognized_windows_filesystem_prefixes() {
        for (source, expected) in [
            (r"\\?\D:\worktrees\中文 工程\rpg", r"D:\worktrees\中文 工程\rpg"),
            (r"\\?\C:\", r"C:\"),
            (r"\\?\UNC\server\share\中文 工程", r"\\server\share\中文 工程"),
            (r"\\?\unc\server\share", r"\\server\share"),
            (r"D:\worktrees\rpg", r"D:\worktrees\rpg"),
            (r"\\server\share\rpg", r"\\server\share\rpg"),
            ("/home/中文 工程/rpg", "/home/中文 工程/rpg"),
            (r"\\?\UNC\server", r"\\?\UNC\server"),
            (r"\\?\UNC\\share", r"\\?\UNC\\share"),
            (r"\\?\Volume{1234}\rpg", r"\\?\Volume{1234}\rpg"),
            (r"\\.\COM1", r"\\.\COM1"),
        ] {
            let path = PathBuf::from(source);
            assert_eq!(display_path(&path), expected);
            assert_eq!(path.as_os_str(), source); // 展示不会回写最近工程或进程启动所用的真实路径。
        }
    }

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "独立编辑器 项目-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            fs::write(
                path.join("Cargo.toml"),
                "[package]\nname = \"new_editor_runtime\"\nversion = \"0.1.0\"\n",
            )
            .unwrap();
            let fixture = Self(path);
            fixture.write_manifest(LEGACY_SDK_ABI, "Cargo.toml");
            fixture
        }

        fn write_manifest(&self, sdk_abi: u32, cargo_manifest: &str) {
            fs::write(self.0.join("editor-project.json"), serde_json::to_vec(&json!({
                "format": 1, "sdk_abi": sdk_abi, "id": "test-project", "name": "中文工程 空格", "cargo_manifest": cargo_manifest, "package": "new_editor_runtime"
            })).unwrap()).unwrap();
        }

    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn opens_chinese_and_spaces_without_working_directory_dependency() {
        let fixture = Fixture::new();
        let project = open(&fixture.0).unwrap();
        assert_eq!(project.name, "中文工程 空格");
        assert_eq!(
            project.manifest,
            fixture.0.join("Cargo.toml").canonicalize().unwrap()
        );
    }

    #[test]
    fn descriptor_and_directory_open_the_same_project_without_writing() {
        let fixture = Fixture::new();
        let descriptor = fixture.0.join("editor-project.json");
        let before = fs::read(&descriptor).unwrap();
        assert_eq!(open(&fixture.0).unwrap(), open(&descriptor).unwrap());
        assert_eq!(fs::read(&descriptor).unwrap(), before);
        assert!(open(&fixture.0.join("Cargo.toml")).unwrap_err().contains("editor-project.json"));
    }

    #[test]
    fn legacy_abi_does_not_control_saved_world_and_manifest_escape_is_rejected() {
        let fixture = Fixture::new();
        fixture.write_manifest(LEGACY_SDK_ABI + 1, "Cargo.toml");
        assert!(open(&fixture.0).is_ok()); // 旧DLL版本不支配普通游戏入口。
        let child = fixture.0.join("child");
        fs::create_dir(&child).unwrap();
        fs::copy(
            fixture.0.join("editor-project.json"),
            child.join("editor-project.json"),
        )
        .unwrap();
        let child_fixture = Fixture(child);
        child_fixture.write_manifest(LEGACY_SDK_ABI, "../Cargo.toml");
        assert!(open(&child_fixture.0).unwrap_err().contains("工程根"));
    }

    #[test]
    fn rejects_large_or_missing_manifest_inputs() {
        let fixture = Fixture::new();
        fs::write(
            fixture.0.join("editor-project.json"),
            vec![b' '; MANIFEST_LIMIT + 1],
        )
        .unwrap();
        assert!(open(&fixture.0).unwrap_err().contains("64 KiB"));
        fixture.write_manifest(LEGACY_SDK_ABI, "missing.toml");
        assert!(open(&fixture.0).is_err());
    }

}
