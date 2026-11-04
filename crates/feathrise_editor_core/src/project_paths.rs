//! 作者实例的不可变工程路径；工程数据与编辑器界面／工具资源使用不同来源。

use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;

/// 一个独立编辑实例终生只编辑一个工程，后台任务不会因打开其他工程而改写来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorProjectPaths {
    project_root: PathBuf,
    asset_root: PathBuf,
}

impl EditorProjectPaths {
    /// 接收启动器已校验的绝对工程路径；本函数只校验路径形状，不在帧系统中访问磁盘。
    ///
    /// 启动器应先在后台完成工程描述文件、真实路径和目录权限检查，再于安装作者插件前绑定。
    pub fn new(
        project_root: impl Into<PathBuf>,
        asset_root: impl Into<PathBuf>,
    ) -> Result<Self, String> {
        let project_root = project_root.into();
        let asset_root = asset_root.into();
        for (label, path) in [("工程根", &project_root), ("工程资产根", &asset_root)] {
            if !path.is_absolute()
                || path
                    .components()
                    .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
            {
                return Err(format!(
                    "{label}必须是无父级跳转的绝对路径：{}",
                    path.display()
                ));
            }
        }
        if asset_root == project_root || !asset_root.starts_with(&project_root) {
            return Err(format!(
                "作者资产根必须位于当前工程目录内：{}",
                asset_root.display()
            ));
        }
        Ok(Self {
            project_root,
            asset_root,
        })
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }
    pub fn asset_root(&self) -> &Path {
        &self.asset_root
    }
}

static EDITOR_PROJECT_PATHS: OnceLock<EditorProjectPaths> = OnceLock::new();

/// 编辑器自身的只读资源根，与工程资产根分别绑定，切换工程不会切换字体、模板或工具。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorResourcePaths {
    root: PathBuf,
}

impl EditorResourcePaths {
    /// 宿主在启动阶段完成资源完整性检查；此处只核验形状，不向帧系统引入文件 IO。
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        if !root.is_absolute()
            || root
                .components()
                .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
        {
            return Err(format!("编辑器资源根必须是无父级跳转的绝对路径：{}", root.display()));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

static EDITOR_RESOURCE_PATHS: OnceLock<EditorResourcePaths> = OnceLock::new();

/// 只有正式宿主绑定资源后才启用内置作者字体；原游戏内嵌作者仍读取应用自己的字体。
pub(super) fn has_bound_editor_resources() -> bool {
    EDITOR_RESOURCE_PATHS.get().is_some()
}

/// 必须在创建 AssetServer 和后台工程任务前调用；同一进程不得中途更换工具来源。
pub fn bind_editor_resource_paths(paths: EditorResourcePaths) -> Result<(), String> {
    bind_resources_once(&EDITOR_RESOURCE_PATHS, paths)
}

fn bind_resources_once(
    slot: &OnceLock<EditorResourcePaths>,
    paths: EditorResourcePaths,
) -> Result<(), String> {
    match slot.set(paths) {
        Ok(()) => Ok(()),
        Err(paths) if slot.get() == Some(&paths) => Ok(()),
        Err(_) => Err("本编辑实例已经绑定其他资源根；更换安装目录后请重启编辑器".into()),
    }
}

/// 必须在AssetServer和作者资源初始化前调用；重复绑定同一工程允许，换工程必须新建实例。
pub fn bind_editor_project_paths(paths: EditorProjectPaths) -> Result<(), String> {
    bind_once(&EDITOR_PROJECT_PATHS, paths)
}

fn bind_once(slot: &OnceLock<EditorProjectPaths>, paths: EditorProjectPaths) -> Result<(), String> {
    match slot.set(paths) {
        Ok(()) => Ok(()),
        Err(paths) if slot.get() == Some(&paths) => Ok(()),
        Err(_) => Err("本编辑实例已经绑定其他工程；切换工程应启动独立编辑实例".into()),
    }
}

pub fn editor_project_paths() -> Option<&'static EditorProjectPaths> {
    EDITOR_PROJECT_PATHS.get()
}

/// 编辑器资源根，并非工程所依赖的 feathrise_runtime SDK 源码根。
/// 正式宿主先校验并绑定；原游戏内嵌作者工具未绑定时仍保留原来的路径策略。
pub fn editor_resource_root() -> PathBuf {
    if let Some(paths) = EDITOR_RESOURCE_PATHS.get() {
        return paths.root.clone();
    }
    if let Some(path) = std::env::var_os("FEATHRISE_EDITOR_ROOT") { return path.into(); }
    if let Some(path) = std::env::var_os("EDITOR_HOST_REPO_ROOT") {
        let path = PathBuf::from(path);
        return if has_editor_resources(&path) { path } else { path.join("feathrise") };
    }
    // Release资源与exe相邻；源码开发才回退本crate所属的编辑器仓库，不能回退原游戏仓库。
    if let Some(path) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        if has_editor_resources(&path) { return path; }
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("editor_core 必须位于编辑器的 crates 目录")
        .to_path_buf()
}

fn has_editor_resources(path: &Path) -> bool {
    path.join("assets/templates").is_dir()
        // 兼容已有发行目录；新布局只在 assets 内保存必须随程序分发的资源。
        || (path.join("assets").is_dir() && path.join("templates").is_dir())
}

pub fn editor_asset_root() -> PathBuf {
    editor_resource_root().join("assets")
}

/// 兼容已有作者集成调用；新代码请使用语义准确的 `editor_resource_root`。
pub fn sdk_workspace_root() -> PathBuf {
    editor_resource_root()
}

/// 兼容已有作者集成调用；这里始终是编辑器资源，和 SDK 源码目录无关。
pub fn sdk_asset_root() -> PathBuf {
    editor_asset_root()
}

/// 所有作者读取、写入和目录扫描的唯一资产根；独立模式绝不回退仓库同名作者数据。
pub fn workspace_asset_root() -> PathBuf {
    if let Some(paths) = editor_project_paths() {
        return paths.asset_root.clone();
    }
    #[cfg(feature = "runtime_test_support")]
    if let Some(root) = super::RUNTIME_TEST_ASSET_ROOT.read().unwrap().as_ref() {
        return root.clone();
    }
    editor_asset_root()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(label: &str) -> PathBuf {
        std::env::temp_dir().join(label)
    }

    #[test]
    fn binding_cannot_change_the_project_seen_by_background_jobs() {
        let slot = OnceLock::new();
        let a = root("editor-path-a");
        let b = root("editor-path-b");
        let paths = EditorProjectPaths::new(&a, a.join("assets")).unwrap();
        bind_once(&slot, paths.clone()).unwrap();
        bind_once(&slot, paths.clone()).unwrap();
        assert!(
            bind_once(
                &slot,
                EditorProjectPaths::new(&b, b.join("assets")).unwrap()
            )
            .is_err()
        );
        assert_eq!(slot.get(), Some(&paths));
    }

    #[test]
    fn author_assets_must_remain_in_the_explicit_absolute_project() {
        let a = root("editor-path-a");
        assert!(EditorProjectPaths::new("relative-project", "relative-project/assets").is_err());
        assert!(EditorProjectPaths::new(&a, root("editor-path-b/assets")).is_err());
        assert!(EditorProjectPaths::new(&a, a.join("assets/../other")).is_err());
        assert!(EditorProjectPaths::new(&a, &a).is_err());
        let paths = EditorProjectPaths::new(&a, a.join("assets")).unwrap();
        assert_eq!(paths.project_root(), a);
        assert_eq!(paths.asset_root(), a.join("assets"));
        assert_ne!(paths.asset_root(), editor_asset_root());
    }

    #[test]
    fn resource_binding_is_immutable_and_separate_from_author_assets() {
        let resources = OnceLock::new();
        let a = EditorResourcePaths::new(root("editor-resource-a")).unwrap();
        let b = EditorResourcePaths::new(root("editor-resource-b")).unwrap();
        bind_resources_once(&resources, a.clone()).unwrap();
        bind_resources_once(&resources, a.clone()).unwrap();
        assert!(bind_resources_once(&resources, b).is_err());
        assert_eq!(resources.get(), Some(&a));
        let projects = OnceLock::new();
        let project = root("editor-resource-author");
        bind_once(&projects, EditorProjectPaths::new(&project, project.join("assets")).unwrap()).unwrap();
        assert_ne!(resources.get().unwrap().root(), projects.get().unwrap().asset_root());
        assert!(EditorResourcePaths::new("relative").is_err());
        assert!(EditorResourcePaths::new(root("editor-resource-a/../b")).is_err());
    }
}
