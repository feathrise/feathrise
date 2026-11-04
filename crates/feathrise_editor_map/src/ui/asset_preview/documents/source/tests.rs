//! 工程文本与资产文本的身份归一、受检落盘和混合批量保存；仅操作本轮独占临时目录。

use super::*;
use super::super::{io, types::*, batch_save, session};
use bevy::prelude::*;
use std::{sync::{Arc, atomic::{AtomicU64, Ordering}}, time::Duration};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("feathrise-project-docs-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();
        std::fs::create_dir(root.join("assets")).unwrap();
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\nname=\"fixture\"\n").unwrap();
        Self(root)
    }
    fn roots(&self) -> SourceRoots { SourceRoots { assets: self.0.join("assets"), project: Some(self.0.clone()) } }
    fn document(&self, key: &str, draft: &str) -> Document {
        let (disk, contents) = io::load_source(&self.roots(), key).unwrap();
        let mut doc = Document {
            path: key.into(), format: crate::asset_catalog::document_format(key).unwrap(), generation: 1,
            disk, pages: contents.pages, source: contents.text, revision: 0, saved_revision: 0,
            history: default(), preview: Arc::new(Vec::new()), preview_revision: None, preview_clipped: false,
            mode: ViewMode::Edit, preview_scroll: Vec2::ZERO, confirm_reload: false, external: default(), status: String::new(),
        };
        doc.edit(draft);
        doc
    }
}
impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

#[test]
fn project_document_keys_reuse_assets_and_reject_unsafe_or_other_project_roots() {
    let fixture = Fixture::new();
    let assets = fixture.0.join("assets");
    assert_eq!(project_key(&assets, &fixture.0, "assets/说明.md").unwrap(), "说明.md");
    assert_eq!(project_key(&assets, &fixture.0, "src/main.rs").unwrap(), "project://src/main.rs");
    for path in ["../other.rs", "src/../other.rs", "C:/other.rs", "/other.rs", "src\\main.rs"] {
        assert!(project_key(&assets, &fixture.0, path).is_err(), "{path}");
    }
    assert_eq!(display_path("project://src/main.rs"), "src/main.rs");
    assert_eq!(display_path("说明.md"), "说明.md");
    let mut state = DocumentState { root: assets, project_root: Some(fixture.0.clone()), ..default() };
    assert!(state.bind_project_source(&fixture.0.join("other"), None).is_err());
    assert_eq!(state.project_root.as_deref(), Some(fixture.0.as_path()));
}

#[test]
fn project_document_io_preserves_encoding_and_refuses_external_changes() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("src/main.rs"), b"\xef\xbb\xbf// old\r\n").unwrap();
    let roots = fixture.roots();
    let (disk, contents) = io::load_source(&roots, "project://src/main.rs").unwrap();
    assert_eq!(&*contents.text, "// old\n");
    let pages = vec![Arc::from("// 中文\r\n")];
    let saved = io::save_source(&roots, "project://src/main.rs", &disk, &pages).unwrap();
    assert_eq!(std::fs::read(fixture.0.join("src/main.rs")).unwrap(), "\u{feff}// 中文\r\n".as_bytes());
    std::fs::write(fixture.0.join("src/main.rs"), "// external\n").unwrap();
    assert!(io::save_source(&roots, "project://src/main.rs", &saved, &pages).is_err());
    assert_eq!(std::fs::read_to_string(fixture.0.join("src/main.rs")).unwrap(), "// external\n");
    assert!(io::load_source(&roots, "project://../Cargo.toml").is_err());
    assert!(io::load_source(&SourceRoots { assets: roots.assets, project: None }, "project://src/main.rs").is_err());
}

#[test]
fn project_and_asset_drafts_use_the_original_batch_save_and_keep_conflicts() {
    let fixture = Fixture::new();
    std::fs::write(fixture.0.join("src/main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(fixture.0.join("assets/说明.md"), "old\n").unwrap();
    let code = fixture.document("project://src/main.rs", "fn main() { println!(\"中文\"); }\n");
    let asset = fixture.document("说明.md", "新的资产说明\n");
    std::fs::write(fixture.0.join("src/main.rs"), "// external\n").unwrap();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).insert_resource(feathrise_editor_documents::content_operations())
        .insert_resource(DocumentState { root: fixture.0.join("assets"), project_root: Some(fixture.0.clone()),
            documents: vec![code, asset], ..default() })
        .init_resource::<batch_save::DocumentBatchSaveState>()
        .add_systems(Update, session::drive_tasks);
    app.world_mut().resource_scope(|world, mut report: Mut<batch_save::DocumentBatchSaveState>| {
        batch_save::request(&mut report, &mut world.resource_mut::<DocumentState>(),
            batch_save::SaveScope::All, feathrise_editor_documents::content_operations());
    });
    let started = std::time::Instant::now();
    while app.world().resource::<DocumentState>().io.is_some() {
        app.update();
        assert!(started.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(1));
    }
    let state = app.world().resource::<DocumentState>();
    assert!(state.documents[0].dirty(), "源码冲突必须保留草稿");
    assert!(!state.documents[1].dirty(), "一个文件冲突不能阻止其他资产文档保存");
    assert_eq!(std::fs::read_to_string(fixture.0.join("src/main.rs")).unwrap(), "// external\n");
    assert_eq!(std::fs::read_to_string(fixture.0.join("assets/说明.md")).unwrap(), "新的资产说明\n");
}
