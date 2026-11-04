//! 已保存启动地图的后台读写事务；固定工程根文件、完整原字节身份与同目录原子提交。
//! 不把无文件状态隐式迁移为startup_scene，也不在普通打开时写入作者工程。

use crate::startup_document::{self, Document, FILE_NAME, MAX_BYTES};
use crate::{
    project::Project,
    project_identity::{self, ProjectIdentity},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
};

#[path = "startup_store_atomic.rs"]
mod atomic;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SceneFileIdentity {
    pub project: ProjectIdentity,
    pub scene_sha256: Option<[u8; 32]>,
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedScene {
    pub document: Document,
    pub identity: SceneFileIdentity,
}

pub(crate) fn load(project: &Project) -> Result<LoadedScene, String> {
    let project_identity = project_identity::identity(project)?;
    let bytes = read_scene(project)?;
    let document = match &bytes {
        Some(bytes) => startup_document::parse(bytes, &project.id)?,
        None => Document::empty(&project.id),
    };
    let identity = SceneFileIdentity {
        project: project_identity,
        scene_sha256: bytes.as_ref().map(|bytes| digest(bytes)),
    };
    ensure_identity(project, &identity)?;
    Ok(LoadedScene { document, identity })
}

pub(crate) fn save(
    project: &Project,
    document: &Document,
    expected: &SceneFileIdentity,
) -> Result<SceneFileIdentity, String> {
    save_with_hook(project, document, expected, || Ok(()))
}

fn save_with_hook(
    project: &Project,
    document: &Document,
    expected: &SceneFileIdentity,
    before_commit: impl FnOnce() -> Result<(), String>,
) -> Result<SceneFileIdentity, String> {
    let bytes = startup_document::to_bytes(document, &project.id)?;
    ensure_identity(project, expected)?;
    let path = project.root.join(FILE_NAME);
    // 即使同值也验证冻结身份；普通保存不会无条件覆盖外部编辑或失效的旧工程清单。
    if read_scene(project)?.as_deref() != Some(bytes.as_slice()) {
        atomic::replace(&path, &bytes, expected.scene_sha256.is_some(), || {
            before_commit()?;
            ensure_identity(project, expected)
        })?;
    }
    if project_identity::identity(project)? != expected.project
        || read_scene(project)?.as_deref() != Some(bytes.as_slice())
    {
        return Err("保存后场景或工程再次被外部修改，请重新打开工程".into());
    }
    Ok(SceneFileIdentity {
        project: expected.project.clone(),
        scene_sha256: Some(digest(&bytes)),
    })
}

fn ensure_identity(project: &Project, expected: &SceneFileIdentity) -> Result<(), String> {
    if project_identity::identity(project)? != expected.project
        || read_scene(project)?.as_ref().map(|bytes| digest(bytes)) != expected.scene_sha256
    {
        return Err("工程、Cargo清单或场景原文已被外部修改，拒绝覆盖；请重新打开工程".into());
    }
    Ok(())
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn reject_link(metadata: &fs::Metadata) -> Result<(), String> {
    #[cfg(windows)]
    let reparse = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let reparse = false;
    if metadata.file_type().is_symlink() || reparse {
        return Err("场景路径不能经过符号链接或junction".into());
    }
    Ok(())
}

fn read_scene(project: &Project) -> Result<Option<Vec<u8>>, String> {
    let root_metadata =
        fs::symlink_metadata(&project.root).map_err(|e| format!("工程根不可访问：{e}"))?;
    reject_link(&root_metadata)?;
    if !root_metadata.is_dir()
        || project.root.canonicalize().map_err(|e| e.to_string())? != project.root
    {
        return Err("场景工程根归属已变化".into());
    }
    let path = project.root.join(FILE_NAME);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("场景文档不可访问：{error}")),
    };
    reject_link(&metadata)?;
    if !metadata.is_file() {
        return Err("scene.layout.json必须是普通文件".into());
    }
    if metadata.len() > MAX_BYTES as u64 {
        return Err("场景文档超过64KiB上限".into());
    }
    check_contained(&path, &project.root)?;
    let mut bytes = Vec::with_capacity(4096);
    File::open(&path)
        .map_err(|e| format!("场景文档不可读：{e}"))?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("场景文档读取失败：{e}"))?;
    if bytes.len() > MAX_BYTES {
        return Err("场景文档超过64KiB上限".into());
    }
    let metadata =
        fs::symlink_metadata(&path).map_err(|e| format!("读取期间场景路径已变化：{e}"))?;
    reject_link(&metadata)?;
    if !metadata.is_file() {
        return Err("读取期间场景文档不再是普通文件".into());
    }
    check_contained(&path, &project.root)?;
    Ok(Some(bytes))
}

fn check_contained(path: &Path, root: &Path) -> Result<(), String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("场景真实路径无效：{e}"))?;
    if canonical != path || !canonical.starts_with(root) {
        return Err("场景真实路径逃出当前工程".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project;
    use serde_json::json;

    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "editor-scene-store-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&root).unwrap();
            fs::write(
                root.join("Cargo.toml"),
                b"[package]\nname='new_editor_runtime'\nversion='0.1.0'\n",
            )
            .unwrap();
            fs::write(root.join("editor-project.json"), serde_json::to_vec(&json!({"format":2,"sdk_abi":crate::project::LEGACY_SDK_ABI,"id":"045f04e0-1234-4abc-8def-123456789abc","name":"scene-test","cargo_manifest":"Cargo.toml","package":"new_editor_runtime","asset_roots":["assets"],"editor_data":".editor","startup_scene":{"root":0,"path":"old.glb","scene":0},"unknown":{"keep":true}})).unwrap()).unwrap();
            Self(root.canonicalize().unwrap())
        }
        fn project(&self) -> Project {
            project::open(&self.0).unwrap()
        }
        fn add(&self, loaded: &mut LoadedScene) {
            loaded.document.select_map("MAP-SELECTED").unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn missing_is_empty_and_does_not_migrate_startup_or_write_on_open() {
        let fixture = Fixture::new();
        let project = fixture.project();
        let loaded = load(&project).unwrap();
        assert!(loaded.document.extra.is_empty());
        assert!(loaded.identity.scene_sha256.is_none());
        assert!(!fixture.0.join(FILE_NAME).exists());
        assert_eq!(
            project.context.startup_scene.as_ref().unwrap().path,
            "old.glb"
        );
    }

    #[test]
    fn save_reopen_and_atomic_replace_keep_unknown_fields_and_other_files() {
        let fixture = Fixture::new();
        let project = fixture.project();
        let descriptor = fs::read(fixture.0.join("editor-project.json")).unwrap();
        let cargo = fs::read(&project.manifest).unwrap();
        let mut loaded = load(&project).unwrap();
        fixture.add(&mut loaded);
        let mut document = loaded.document.clone();
        document.extra.insert("future".into(), json!([1, 2, 3]));
        document
            .extra
            .insert("objects".into(), json!([{ "custom":{"a":true} }]));
        let identity = save(&project, &document, &loaded.identity).unwrap();
        assert_eq!(&load(&project).unwrap().document, &document);
        document.extra.insert("future".into(), json!([2, 3, 4]));
        document.revision += 1;
        save(&project, &document, &identity).unwrap();
        assert_eq!(&load(&project).unwrap().document, &document);
        assert_eq!(
            fs::read(fixture.0.join("editor-project.json")).unwrap(),
            descriptor
        );
        assert_eq!(fs::read(&project.manifest).unwrap(), cargo);
    }

    #[test]
    fn corrupt_oversized_wrong_project_and_directory_are_not_overwritten() {
        let fixture = Fixture::new();
        let project = fixture.project();
        let path = fixture.0.join(FILE_NAME);
        fs::write(&path, b"broken").unwrap();
        assert!(load(&project).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken");
        fs::write(&path, vec![b' '; MAX_BYTES + 1]).unwrap();
        assert!(load(&project).unwrap_err().contains("64KiB"));
        fs::write(
            &path,
            startup_document::to_bytes(&Document::empty("other"), "other").unwrap(),
        )
        .unwrap();
        assert!(load(&project).unwrap_err().contains("当前工程"));
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(load(&project).unwrap_err().contains("普通文件"));
    }

    #[test]
    fn scene_and_raw_descriptor_or_cargo_external_edits_reject_stale_save() {
        for file in [FILE_NAME, "editor-project.json", "Cargo.toml"] {
            let fixture = Fixture::new();
            let project = fixture.project();
            let mut loaded = load(&project).unwrap();
            fixture.add(&mut loaded);
            let path = fixture.0.join(file);
            let bytes = if file == FILE_NAME {
                b"external scene".to_vec()
            } else {
                let mut bytes = fs::read(&path).unwrap();
                bytes.push(b' ');
                bytes
            };
            fs::write(&path, &bytes).unwrap();
            assert!(save(&project, &loaded.document, &loaded.identity).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn before_commit_failure_and_creation_race_preserve_original_and_clean_only_own_temp() {
        let fixture = Fixture::new();
        let project = fixture.project();
        let mut loaded = load(&project).unwrap();
        fixture.add(&mut loaded);
        assert!(
            save_with_hook(&project, &loaded.document, &loaded.identity, || Err(
                "injected failure".into()
            ))
            .is_err()
        );
        assert!(!fixture.0.join(FILE_NAME).exists());
        let external = b"created outside";
        assert!(
            save_with_hook(&project, &loaded.document, &loaded.identity, || {
                fs::write(fixture.0.join(FILE_NAME), external).unwrap();
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(fixture.0.join(FILE_NAME)).unwrap(), external);
        assert!(!fs::read_dir(&fixture.0).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".scene-layout-")
        }));
    }

    #[cfg(windows)]
    #[test]
    fn locked_original_fails_without_deleting_or_truncating_saved_scene() {
        use std::os::windows::fs::OpenOptionsExt;
        let fixture = Fixture::new();
        let project = fixture.project();
        let mut loaded = load(&project).unwrap();
        fixture.add(&mut loaded);
        let identity = save(&project, &loaded.document, &loaded.identity).unwrap();
        let path = fixture.0.join(FILE_NAME);
        let original = fs::read(&path).unwrap();
        let _locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let mut changed = loaded.document.clone();
        changed.revision += 1;
        assert!(save(&project, &changed, &identity).is_err());
        assert_eq!(fs::read(&path).unwrap(), original);
    }
}
