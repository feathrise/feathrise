//! 外部工程的路径上下文；只解析有限的清单字段，不在打开工程时创建目录或迁移旧数据。

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

const ROOT_LIMIT: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneSelection {
    pub root: usize,
    pub path: String,
    pub scene: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectContext {
    pub root: PathBuf,
    pub id: String,
    pub name: String,
    pub format: u32,
    pub asset_roots: Vec<PathBuf>,
    /// 已追加工程身份的作者数据目录；复制工程保留UUID表示同一工程，另建工程必须生成新UUID。
    pub editor_data: PathBuf,
    pub storage_key: String,
    pub startup_scene: Option<SceneSelection>,
}

impl ProjectContext {
    pub(super) fn resolve(
        root: &Path,
        id: &str,
        name: &str,
        format: u32,
        asset_roots: Option<Vec<PathBuf>>,
        editor_data: Option<PathBuf>,
        startup_scene: Option<SceneSelection>,
    ) -> Result<Self, String> {
        let storage_key = if is_uuid(id) {
            id.to_owned()
        } else if format == 1 {
            // 早期v1允许任意文本ID；继续支持读取，但绝不把其原文拼入磁盘路径。
            let digest = Sha256::digest(id.as_bytes());
            let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            format!("legacy-{hex}")
        } else {
            return Err("工程清单v2的ID必须是小写标准UUID，不能复用场景ID".into());
        };
        let roots = match (format, asset_roots) {
            (2, None) => return Err("工程清单v2缺少asset_roots".into()),
            (_, Some(roots)) => roots,
            _ => vec![PathBuf::from("assets")],
        };
        if roots.is_empty() || roots.len() > ROOT_LIMIT {
            return Err("资产根必须为1至16个工程内目录".into());
        }
        let data = match (format, editor_data) {
            (2, None) => return Err("工程清单v2缺少editor_data".into()),
            (_, Some(path)) => path,
            _ => PathBuf::from(".editor"),
        };
        let data_root = safe_directory(root, &data)?;
        let mut resolved: Vec<PathBuf> = Vec::with_capacity(roots.len());
        for path in roots {
            let path = safe_directory(root, &path)?;
            if contains_directory(&path, &data_root) || contains_directory(&data_root, &path) {
                return Err("资产根与作者数据目录不得相互包含".into());
            }
            if resolved.iter().any(|previous| {
                contains_directory(previous, &path) && contains_directory(&path, previous)
            }) {
                return Err("资产根存在重复目录".into());
            }
            resolved.push(path);
        }
        let editor_data = safe_directory(root, &data.join(&storage_key))?;
        if let Some(selection) = &startup_scene {
            validate_scene(selection, resolved.len())?;
        }
        Ok(Self {
            root: root.to_owned(),
            id: id.to_owned(),
            name: name.to_owned(),
            format,
            asset_roots: resolved,
            editor_data,
            storage_key,
            startup_scene,
        })
    }

    /// 保存前再次检查每一层归属；打开之后新增的junction也不能把写入引向其它工程。
    pub fn ensure_data_directory(&self) -> Result<PathBuf, String> {
        let relative = self
            .editor_data
            .strip_prefix(&self.root)
            .map_err(|_| "作者数据路径不属于工程".to_string())?;
        let checked = safe_directory(&self.root, relative)?;
        let mut current = self.root.clone();
        for component in relative.components() {
            current.push(component);
            match fs::create_dir(&current) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(error) => return Err(format!("无法创建作者数据目录：{error}")),
            }
            check_existing_directory(&current)?;
        }
        if checked
            .canonicalize()
            .map_err(|e| format!("作者数据目录无效：{e}"))?
            != checked
        {
            return Err("作者数据目录真实路径已变化".into());
        }
        Ok(checked)
    }

    pub fn checked_data_directory(&self) -> Result<PathBuf, String> {
        let relative = self
            .editor_data
            .strip_prefix(&self.root)
            .map_err(|_| "作者数据路径不属于工程".to_string())?;
        safe_directory(&self.root, relative)
    }
}

fn validate_scene(selection: &SceneSelection, roots: usize) -> Result<(), String> {
    if selection.root >= roots || selection.scene >= 256 || selection.path.len() > 1024 {
        return Err("startup_scene根索引未登记、Scene索引超过255或路径过长".into());
    }
    let path = Path::new(&selection.path);
    if path.as_os_str().is_empty()
        || path.components().count() > 16
        || !path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("glb"))
    {
        return Err("startup_scene必须是最多16层的普通相对.glb路径".into());
    }
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err("startup_scene不能含绝对路径或父跳转".into());
        };
        let text = name.to_str().ok_or("startup_scene路径必须为UTF-8")?;
        let stem = text.split('.').next().unwrap_or("").to_ascii_uppercase();
        let device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || stem
                .strip_prefix("COM")
                .or_else(|| stem.strip_prefix("LPT"))
                .is_some_and(|value| {
                    value.len() == 1 && matches!(value.as_bytes()[0], b'1'..=b'9')
                });
        if component_too_long(text)
            || text.contains([':', '#', '?'])
            || text.ends_with(['.', ' '])
            || text.chars().any(char::is_control)
            || device
        {
            return Err("startup_scene包含不安全路径片段".into());
        }
    }
    Ok(())
}

fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
        && value.bytes().any(|byte| byte != b'0' && byte != b'-')
}

fn contains_directory(parent: &Path, child: &Path) -> bool {
    #[cfg(windows)]
    {
        // Windows普通目录默认忽略大小写，不能用大小写变体把数据根伪装成资产根。
        let parent: Vec<_> = parent
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
            .collect();
        let child: Vec<_> = child
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
            .collect();
        child.starts_with(&parent)
    }
    #[cfg(not(windows))]
    child.starts_with(parent)
}

fn component_too_long(text: &str) -> bool {
    // 单个名称遵循当前文件系统的计数单位；清单整体的字节预算仍由调用处单独约束。
    #[cfg(windows)]
    { text.encode_utf16().count() > 255 }
    #[cfg(not(windows))]
    { text.len() > 255 }
}

pub(super) fn safe_directory(root: &Path, relative: &Path) -> Result<PathBuf, String> {
    if relative.as_os_str().is_empty() || relative.components().count() > 16 {
        return Err("工程目录相对路径必须非空且最多16层".into());
    }
    let mut path = root.to_owned();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("工程目录必须是工程根内的相对路径，不能包含父目录跳转".into());
        };
        let text = name.to_str().ok_or("工程目录必须使用UTF-8名称")?;
        if component_too_long(text)
            || text.contains(':')
            || text.ends_with(['.', ' '])
            || text.chars().any(char::is_control)
        {
            return Err("工程目录包含不安全的路径片段".into());
        }
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => validate_directory(&path, &metadata)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(format!("工程目录不可访问：{}：{error}", path.display())),
        }
    }
    Ok(path)
}

fn check_existing_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("工程目录不可访问：{e}"))?;
    validate_directory(path, &metadata)
}

fn validate_directory(path: &Path, metadata: &fs::Metadata) -> Result<(), String> {
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.file_type().is_symlink() || reparse || !metadata.is_dir() {
        return Err(format!(
            "工程目录必须为普通目录，不能是符号链接或junction：{}",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "editor-context-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        root.canonicalize().unwrap()
    }

    #[test]
    fn legacy_id_remains_readable_without_becoming_a_path() {
        let root = fixture();
        let context =
            ProjectContext::resolve(&root, "../旧工程", "旧工程", 1, None, None, None).unwrap();
        assert!(context.storage_key.starts_with("legacy-"));
        assert!(!context.editor_data.exists());
        assert!(context.editor_data.starts_with(&root));
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn v2_requires_uuid_and_rejects_escape_or_asset_data_overlap() {
        let root = fixture();
        let id = "045f04e0-1234-4abc-8def-123456789abc";
        assert!(
            ProjectContext::resolve(
                &root,
                "map-1",
                "n",
                2,
                Some(vec!["assets".into()]),
                Some(".editor".into()),
                None
            )
            .is_err()
        );
        assert!(
            ProjectContext::resolve(
                &root,
                id,
                "n",
                2,
                Some(vec!["../assets".into()]),
                Some(".editor".into()),
                None
            )
            .is_err()
        );
        assert!(
            ProjectContext::resolve(
                &root,
                id,
                "n",
                2,
                Some(vec![".editor/assets".into()]),
                Some(".editor".into()),
                None
            )
            .is_err()
        );
        #[cfg(windows)]
        assert!(
            ProjectContext::resolve(
                &root,
                id,
                "n",
                2,
                Some(vec![".EDITOR/assets".into()]),
                Some(".editor".into()),
                None
            )
            .is_err()
        );
        let context = ProjectContext::resolve(
            &root,
            id,
            "n",
            2,
            Some(vec!["assets".into()]),
            Some(".editor".into()),
            None,
        )
        .unwrap();
        assert!(context.editor_data.ends_with(id));
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn rejects_data_path_replaced_by_a_file() {
        let root = fixture();
        let context = ProjectContext::resolve(&root, "legacy", "n", 1, None, None, None).unwrap();
        fs::write(root.join(".editor"), b"file").unwrap();
        assert!(context.ensure_data_directory().is_err());
        fs::remove_file(root.join(".editor")).unwrap();
        fs::remove_dir(root).unwrap();
    }

    #[test]
    fn startup_scene_is_part_of_identity_and_rejects_unregistered_or_escaped_paths() {
        let root = fixture();
        let id = "045f04e0-1234-4abc-8def-123456789abc";
        let valid = SceneSelection {
            root: 0,
            path: "scene/probe.glb".into(),
            scene: 0,
        };
        let context = ProjectContext::resolve(
            &root,
            id,
            "n",
            2,
            Some(vec!["assets".into()]),
            Some(".editor".into()),
            Some(valid.clone()),
        )
        .unwrap();
        assert_eq!(context.startup_scene, Some(valid.clone()));
        for candidate in [
            SceneSelection {
                root: 1,
                ..valid.clone()
            },
            SceneSelection {
                path: "../other.glb".into(),
                ..valid.clone()
            },
            SceneSelection {
                scene: 256,
                ..valid.clone()
            },
        ] {
            assert!(
                ProjectContext::resolve(
                    &root,
                    id,
                    "n",
                    2,
                    Some(vec!["assets".into()]),
                    Some(".editor".into()),
                    Some(candidate)
                )
                .is_err()
            );
        }
        fs::remove_dir(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn windows_startup_scene_names_use_utf16_component_limits() {
        let selection = SceneSelection {
            root: 0,
            path: format!("{}.glb", "场".repeat(100)),
            scene: 0,
        };
        assert!(selection.path.len() > 255);
        assert!(validate_scene(&selection, 1).is_ok());
        assert!(validate_scene(&SceneSelection {
            path: format!("{}.glb", "😀".repeat(126)),
            ..selection
        }, 1).is_err(), "非BMP字符占两个UTF-16单元，仍需拒绝真实超长文件名");
    }

    #[cfg(windows)]
    #[test]
    fn windows_long_chinese_project_directories_can_be_resolved_and_created() {
        let root = fixture(); // 独占创建；失败回归也只清理本次精确创建的目录。
        let asset_name = "资".repeat(100);
        let data_name = "作".repeat(100);
        let id = "045f04e0-1234-4abc-8def-123456789abc";
        let result = (|| -> Result<(Vec<PathBuf>, PathBuf), String> {
            fs::create_dir(root.join(&asset_name)).map_err(|error| error.to_string())?;
            let context = ProjectContext::resolve(
                &root, id, "中文目录回归", 2,
                Some(vec![PathBuf::from(&asset_name)]),
                Some(PathBuf::from(&data_name)), None,
            )?;
            let data = context.ensure_data_directory()?;
            Ok((context.asset_roots, data))
        })();
        // 不递归删除，不触碰任何共享夹具或作者工程；先清理再报告业务断言失败。
        let _ = fs::remove_dir(root.join(&data_name).join(id));
        let _ = fs::remove_dir(root.join(&data_name));
        let _ = fs::remove_dir(root.join(&asset_name));
        fs::remove_dir(&root).unwrap();
        let (assets, data) = result.unwrap();
        assert_eq!(assets, vec![root.join(asset_name)]);
        assert_eq!(data, root.join(data_name).join(id));
    }
}
