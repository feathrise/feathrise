//! 独立作者实例的资产源：项目内容只读本工程，作者资源只提供编辑器图标，默认字体由 SDK 内嵌提供。

use bevy::{
    asset::{
        AssetApp, AssetMode, AssetPlugin,
        io::{
            AssetReader, AssetReaderError, AssetSourceBuilder, AssetSourceId, PathStream, Reader,
            file::FileAssetReader,
        },
    },
    prelude::App,
    tasks::IoTaskPool,
};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

/// 按原工程登记顺序安装来源；监听仍复用Bevy原文件监听器，源身份在进程生命周期内不变。
pub(super) fn install(app: &mut App, roots: &[PathBuf]) -> Result<AssetPlugin, String> {
    let first = roots.first().ok_or("工程未登记默认资产根")?;
    let editor = feathrise_editor_core::editor_asset_root()
        .canonicalize()
        .map_err(|error| format!("编辑器图标资产不可访问：{error}"))?;
    for (index, root) in roots.iter().enumerate() {
        let root = root
            .canonicalize()
            .map_err(|error| format!("工程资产根不可访问：{}：{error}", root.display()))?;
        if !root.is_dir() {
            return Err(format!("工程资产根必须是目录：{}", root.display()));
        }
        let source = AssetSourceBuilder::platform_default(&root.to_string_lossy(), None);
        let root_for_reader = root;
        let editor_for_reader = (index == 0).then(|| editor.clone());
        let mut source = source.with_reader(move || {
            Box::new(ProjectReader::new(
                root_for_reader.clone(),
                editor_for_reader.clone(),
            ))
        });
        // 作者保存继续经唯一受检事务；AssetServer只提供读取和当前工程目录监听。
        source.writer = None;
        let id = if index == 0 {
            AssetSourceId::Default
        } else {
            AssetSourceId::from(format!("root{index}"))
        };
        app.register_asset_source(id, source);
    }
    Ok(AssetPlugin {
        file_path: first.to_string_lossy().into_owned(),
        mode: AssetMode::Unprocessed,
        watch_for_changes_override: Some(true),
        ..Default::default()
    })
}

struct ProjectReader {
    root: PathBuf,
    project: FileAssetReader,
    editor: Option<(PathBuf, FileAssetReader)>,
}

impl ProjectReader {
    fn new(root: PathBuf, editor: Option<PathBuf>) -> Self {
        let project = FileAssetReader::new(&root);
        let editor = editor.map(|path| {
            let reader = FileAssetReader::new(&path);
            (path, reader)
        });
        Self { root, project, editor }
    }

    fn source<'a>(&'a self, path: &Path) -> (&'a Path, &'a FileAssetReader) {
        // 只借用工具图标；字体不从磁盘借用，工程其他素材始终由当前工程提供。
        if is_editor_ui(path) {
            if let Some((root, reader)) = &self.editor {
                return (root, reader);
            }
        }
        (&self.root, &self.project)
    }

    async fn check(&self, path: &Path) -> Result<(), AssetReaderError> {
        let root = self.source(path).0.to_owned();
        let relative = path.to_owned();
        // 真实路径与junction检查属于IO准备，不能把文件系统访问移入作者帧关键路径。
        IoTaskPool::get()
            .spawn(async move { checked_location(&root, &relative) })
            .await
    }
}

fn is_editor_ui(path: &Path) -> bool {
    path.starts_with("icons/editor")
        || path.starts_with("icons/cursors")
}

fn checked_location(root: &Path, relative: &Path) -> Result<(), AssetReaderError> {
    let denied = || {
        AssetReaderError::from(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "资产路径不得离开当前工程登记根或编辑器图标命名空间",
        ))
    };
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(denied());
    }
    let full = root.join(relative);
    let canonical = match full.canonicalize() {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AssetReaderError::NotFound(full));
        }
        Err(error) => return Err(error.into()),
    };
    if !canonical.starts_with(root) {
        return Err(denied());
    }
    // 每一层拒绝链接和junction；打开后新增的目录跳转也不能串到另一工程。
    let mut current = root.to_owned();
    for part in relative.components() {
        current.push(part);
        let metadata = fs::symlink_metadata(&current)?;
        #[cfg(windows)]
        let reparse = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let reparse = false;
        if metadata.file_type().is_symlink() || reparse {
            return Err(denied());
        }
    }
    Ok(())
}

impl AssetReader for ProjectReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        self.check(path).await?;
        self.source(path).1.read(path).await
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let mut meta = path.as_os_str().to_owned();
        meta.push(".meta");
        self.check(Path::new(&meta)).await?;
        self.source(path).1.read_meta(path).await
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        self.check(path).await?;
        self.source(path).1.read_directory(path).await
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        self.check(path).await?;
        self.source(path).1.is_directory(path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ui_namespaces_use_editor_and_author_paths_cannot_escape() {
        assert!(!is_editor_ui(Path::new(feathrise_editor_core::FONT_PATH)));
        assert!(is_editor_ui(Path::new("icons/editor/tree_folder.png")));
        assert!(is_editor_ui(Path::new("icons/cursors/a.png")));
        for path in [
            "maps/a.glb",
            "textures/icons.png",
            "icons-map.glb",
            "skills/a.json",
            "icons/skills/custom.png",
            "icons/items/custom.png",
            "fonts/custom.otf",
        ] {
            assert!(!is_editor_ui(Path::new(path)), "{path}");
        }
        let root = std::env::temp_dir().join(format!(
            "native-editor-reader-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        fs::write(root.join("model.glb"), b"current project").unwrap();
        assert!(checked_location(&root, Path::new("model.glb")).is_ok());
        assert!(checked_location(&root, Path::new("../model.glb")).is_err());
        assert!(checked_location(&root, &root.join("model.glb")).is_err());
        assert!(matches!(
            checked_location(&root, Path::new("sdk-model.glb")),
            Err(AssetReaderError::NotFound(_))
        ));
        fs::remove_file(root.join("model.glb")).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
