//! 启动地图的后台事务；冻结工程与地图ID，复用场景原文冲突检查和原子保存。

use crate::{project::Project, startup_store};

#[derive(Debug)]
pub(super) struct SavedStartup {
    pub id: String,
    pub name: String,
    pub changed: bool,
}

pub(super) fn save(project: &Project, id: &str) -> Result<SavedStartup, String> {
    feathrise_map_content::MapId::parse(id)?;
    let root = project
        .context
        .asset_roots
        .first()
        .ok_or("工程没有默认资产根")?;
    // 与地图创建／删除使用同一作者锁，验证当前稳定ID到保存启动设置之间不得被另一作者移除。
    let _author = feathrise_map_content::ContentAuthorLock::acquire(root)?;
    let maps = feathrise_project::ProjectMaps::load(root)?;
    let entry = maps
        .index(id)
        .and_then(|index| maps.entries.get(index))
        .ok_or("当前地图已不在本工程的正式目录中，请刷新后重试")?;
    let mut loaded = startup_store::load(project)?;
    let changed = loaded.document.select_map(id)?;
    if changed {
        // 只改production_map及文档修订；原布局对象、其他extra和作者地图文件都不参与此事务。
        startup_store::save(project, &loaded.document, &loaded.identity)?;
    }
    Ok(SavedStartup {
        id: entry.id.clone(),
        name: entry.name.clone(),
        changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        project: Project,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "new-editor-project-menu-{}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&root).unwrap();
            fs::create_dir(root.join("assets")).unwrap();
            feathrise_project::seed::create(&root.join("assets"), "当前工程地图").unwrap();
            fs::write(
                root.join("Cargo.toml"),
                "[package]\nname=\"menu_fixture\"\nversion=\"0.1.0\"\n",
            )
            .unwrap();
            fs::write(root.join("editor-project.json"), serde_json::to_vec(&serde_json::json!({
                "format":2,"sdk_abi":crate::project::LEGACY_SDK_ABI,
                "id":"6a97a1b1-9b38-4d40-9d91-04fcd3ba4712","name":"菜单工程",
                "cargo_manifest":"Cargo.toml","package":"menu_fixture","asset_roots":["assets"],"editor_data":".editor"
            })).unwrap()).unwrap();
            let project = crate::project::open(&root).unwrap();
            let mut document = crate::startup_document::Document::empty(&project.id);
            document.extra.insert("objects".into(), serde_json::json!([{"id":1,"name":"保留已有布局","future":{"keep":true}}]));
            document.extra.insert("next_id".into(), serde_json::json!(2));
            document.extra.insert(
                "custom_extension".into(),
                serde_json::json!({"keep":[1,2,3]}),
            );
            feathrise_project::set_selection(&mut document.extra, "M1-SAMPLE").unwrap();
            fs::write(
                project.root.join(crate::startup_document::FILE_NAME),
                crate::startup_document::to_bytes(&document, &project.id).unwrap(),
            )
            .unwrap();
            Self { project }
        }

        fn scene(&self) -> PathBuf {
            self.project.root.join(crate::startup_document::FILE_NAME)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let root = &self.project.root;
            let temporary = std::env::temp_dir().canonicalize().unwrap();
            // 递归清理前确认精确夹具身份及系统临时根归属；这里从不接收用户工程路径。
            if root.parent() != Some(temporary.as_path())
                || root.canonicalize().ok().as_ref() != Some(root)
                || !root.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with("new-editor-project-menu-")
                })
            {
                return;
            }
            if let Ok(lock) =
                feathrise_map_content::authoring_io::content_author_lock_path(&root.join("assets"))
            {
                let _ = fs::remove_file(lock);
            }
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn startup_save_preserves_layout_extensions_and_map_source_files() {
        let fixture = Fixture::new();
        let before = startup_store::load(&fixture.project)
            .unwrap()
            .document
            .clone();
        let map_path = fixture
            .project
            .root
            .join("assets/maps/M1-SAMPLE/map.map_manifest.json");
        let map_before = fs::read(&map_path).unwrap();
        let saved = save(&fixture.project, "M1-EMPTY").unwrap();
        assert!(saved.changed);
        let after = startup_store::load(&fixture.project)
            .unwrap()
            .document
            .clone();
        assert_eq!(
            feathrise_project::selection(&after.extra).unwrap(),
            Some("M1-EMPTY")
        );
        assert_eq!(after.extra.get("objects"), before.extra.get("objects"));
        assert_eq!(
            after.extra.get("custom_extension"),
            before.extra.get("custom_extension")
        );
        assert_eq!(after.revision, before.revision + 1);
        assert_eq!(fs::read(map_path).unwrap(), map_before);
        let first_save = fs::read(fixture.scene()).unwrap();
        assert!(!save(&fixture.project, "M1-EMPTY").unwrap().changed);
        assert_eq!(fs::read(fixture.scene()).unwrap(), first_save);
    }

    #[test]
    fn missing_map_is_rejected_without_rewriting_saved_startup_or_layout() {
        let fixture = Fixture::new();
        let original = fs::read(fixture.scene()).unwrap();
        assert!(
            save(&fixture.project, "MISSING-MAP")
                .unwrap_err()
                .contains("不在本工程")
        );
        assert_eq!(fs::read(fixture.scene()).unwrap(), original);
    }
}
