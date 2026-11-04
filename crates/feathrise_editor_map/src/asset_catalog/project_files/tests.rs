//! 只使用独占临时工程核对目录边界、后台筛选、旧结果拒绝与独立选择，不安装玩法或资产加载器。

use std::{path::PathBuf, sync::atomic::{AtomicU64, Ordering}, time::{SystemTime, UNIX_EPOCH}};

use super::{*, systems::{adopt, scan_request}, types::ScanRequest};
use crate::{asset_catalog::scan::{scan_folder, FolderScope}, types::EditorAssetEntry};
use bevy::prelude::*;
use feathrise_map_content::MapAssetKind;

struct Fixture { root: PathBuf, parent: PathBuf }

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let root = parent.join(format!("feathrise-project-files-{}-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();
        Self { root: root.canonicalize().unwrap(), parent }
    }
    fn file(&self, path: &str) {
        std::fs::OpenOptions::new().write(true).create_new(true).open(self.root.join(path)).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let resolved = self.root.canonicalize().unwrap();
        assert_eq!(resolved.parent(), Some(self.parent.as_path()));
        assert!(resolved.file_name().unwrap().to_string_lossy().starts_with("feathrise-project-files-"));
        std::fs::remove_dir_all(resolved).unwrap(); // 只回收本测试亲自创建并重新核对归属的目录。
    }
}

fn entry(path: &str) -> EditorAssetEntry {
    EditorAssetEntry { asset_path: path.into(), display_name: path.rsplit('/').next().unwrap().into(),
        is_directory: false, kind: MapAssetKind::Data }
}

fn request(catalog: &EditorProjectFileCatalog) -> ScanRequest {
    let (context, Some(project_root)) = catalog.source.as_ref().unwrap() else { panic!("夹具必须绑定工程") };
    ScanRequest { context: *context, project_root: project_root.clone(), revision: catalog.requested_revision,
        folder: catalog.requested_folder.clone(), filter: catalog.filter.clone() }
}

#[test]
fn project_root_and_filename_filter_reuse_typed_scan_and_hide_build_git_entries() {
    let fixture = Fixture::new();
    for folder in ["src", "crates", "assets", "target", ".trash"] { std::fs::create_dir(fixture.root.join(folder)).unwrap(); }
    fixture.file(".git"); // worktree的.git可能是普通文件，同样不能进入工程文件列表。
    fixture.file("Cargo.toml");
    fixture.file("README.md");
    fixture.file("src/main.rs");
    fixture.file("src/controller.rs");
    let mut catalog = EditorProjectFileCatalog::default();
    catalog.bind_project_source(12, Some(fixture.root.clone()));
    let root = scan_request(request(&catalog)).result.unwrap();
    assert_eq!(root.iter().map(|entry| entry.asset_path.as_str()).collect::<Vec<_>>(),
        ["assets", "crates", "src", "Cargo.toml", "README.md"]);
    assert_eq!(root[3].kind, MapAssetKind::Data);
    catalog.set_filter("CARGO");
    let filtered = scan_request(request(&catalog)).result.unwrap();
    assert_eq!(filtered.iter().map(|entry| entry.asset_path.as_str()).collect::<Vec<_>>(),
        ["assets", "crates", "src", "Cargo.toml"], "目录保留可达性，只过滤当前层文件名");
    catalog.request_folder("src").unwrap();
    assert!(catalog.filter.is_empty(), "进入目录不继承上层名称过滤");
    catalog.set_filter("MAIN");
    let selected = scan_request(request(&catalog));
    adopt(&mut catalog, selected);
    assert_eq!(catalog.entries.len(), 1);
    let file = catalog.select_entry(0).unwrap();
    assert_eq!(file.project_root, fixture.root);
    assert_eq!(file.relative_path, "src/main.rs");
    assert_eq!(file.context, 12);
}

#[test]
fn generated_root_output_is_hidden_but_author_src_output_remains_navigable() {
    let fixture = Fixture::new();
    for folder in ["output", "src", "src/output", ".feathrise-export-stage", "src/.feathrise-export-stage",
        "src/target", "src/.git", "src/.trash"] {
        std::fs::create_dir(fixture.root.join(folder)).unwrap();
    }
    fixture.file("output/game.exe");
    fixture.file("src/output/controller.rs");
    let mut catalog = EditorProjectFileCatalog::default();
    catalog.bind_project_source(1, Some(fixture.root.clone()));
    let root = scan_request(request(&catalog)).result.unwrap();
    assert_eq!(root.iter().map(|entry| entry.asset_path.as_str()).collect::<Vec<_>>(), ["src"]);
    assert!(scan_folder(fixture.root.clone(), "output".into(), FolderScope::ProjectFiles).is_err(),
        "直接请求也不能进入默认导出包");
    catalog.request_folder("src").unwrap();
    let source = scan_request(request(&catalog)).result.unwrap();
    assert_eq!(source.iter().map(|entry| entry.asset_path.as_str()).collect::<Vec<_>>(), ["src/output"],
        "发布暂存、target、Git和回收目录继续隐藏，作者output目录应保留");
    catalog.request_folder("src/output").unwrap();
    let author = scan_request(request(&catalog));
    adopt(&mut catalog, author);
    assert_eq!(catalog.entries.len(), 1);
    let selected = catalog.select_entry(0).unwrap();
    assert_eq!(selected.relative_path, "src/output/controller.rs");
}

#[test]
fn project_navigation_rejects_lexical_escape_and_shared_scan_rejects_canonical_escape() {
    let fixture = Fixture::new();
    let mut catalog = EditorProjectFileCatalog::default();
    catalog.bind_project_source(1, Some(fixture.root.clone()));
    for folder in ["..", "src/../target", "/", "C:/outside", ".git", "src/target", "assets/.trash"] {
        assert!(catalog.request_folder(folder).is_err(), "{folder}");
    }
    let error = scan_folder(fixture.root.clone(), "..".into(), FolderScope::ProjectFiles).unwrap_err();
    assert!(error.contains("越过工程根"));
    assert!(catalog.current_folder.is_empty());
}

#[test]
fn failed_or_stale_scan_keeps_adopted_directory_and_source_switch_clears_selection() {
    let fixture = Fixture::new();
    let mut catalog = EditorProjectFileCatalog::default();
    catalog.bind_project_source(1, Some(fixture.root.clone()));
    let initial = request(&catalog);
    adopt(&mut catalog, super::types::ScanResult { request: initial, result: Ok(vec![entry("Cargo.toml")]) });
    catalog.retired_entries = None;
    let selected = catalog.select_entry(0).unwrap();
    catalog.request_folder("missing").unwrap();
    let failed = scan_request(request(&catalog));
    adopt(&mut catalog, failed);
    assert!(catalog.current_folder.is_empty());
    assert_eq!(catalog.entries[0].asset_path, "Cargo.toml");
    assert_eq!(catalog.selected_file.as_ref(), Some(&selected));
    assert!(catalog.feedback.as_ref().unwrap().result.is_err());
    catalog.set_filter("toml");
    let stale = request(&catalog);
    catalog.bind_project_source(2, Some(fixture.root.clone())); // 同根也必须按新的工程会话隔离。
    adopt(&mut catalog, super::types::ScanResult { request: stale, result: Ok(vec![entry("wrong.rs")]) });
    assert!(catalog.entries.is_empty());
    assert!(catalog.selected_file.is_none());
    assert!(catalog.feedback.is_none());
    assert_eq!(catalog.retired_entries.as_ref().unwrap()[0].asset_path, "wrong.rs");
}

#[test]
fn single_inflight_scan_remains_owned_until_it_finishes_and_only_latest_filter_is_adopted() {
    use std::{sync::mpsc::sync_channel, time::Duration};
    use bevy::{app::TaskPoolPlugin, tasks::IoTaskPool};
    let fixture = Fixture::new();
    let mut app = App::new();
    app.add_plugins(TaskPoolPlugin::default());
    let mut catalog = EditorProjectFileCatalog::default();
    catalog.bind_project_source(1, Some(fixture.root.clone()));
    catalog.active = true;
    let old = request(&catalog);
    let (release, wait) = sync_channel(1);
    catalog.task = Some(IoTaskPool::get().spawn(async move {
        wait.recv_timeout(Duration::from_secs(5)).unwrap(); // 等待仅属于测试，生产轮询完全非阻塞。
        Some(super::types::ScanResult { request: old, result: Ok(vec![entry("old.rs")]) })
    }));
    catalog.set_filter("latest");
    app.insert_resource(catalog).add_systems(Update, (start_project_file_scan, poll_project_file_scan).chain());
    for _ in 0..3 { app.update(); assert!(app.world().resource::<EditorProjectFileCatalog>().task.is_some()); }
    release.send(()).unwrap();
    for _ in 0..5_000 {
        app.update();
        let catalog = app.world().resource::<EditorProjectFileCatalog>();
        if catalog.applied_revision == catalog.requested_revision && catalog.task.is_none() { break; }
        std::thread::sleep(Duration::from_millis(1));
    }
    let catalog = app.world().resource::<EditorProjectFileCatalog>();
    assert_eq!(catalog.applied_revision, catalog.requested_revision);
    assert!(catalog.entries.is_empty(), "旧目录结果不能冒充最新筛选版本");
    assert!(catalog.feedback.as_ref().unwrap().result.is_ok());
    assert!(!app.world().contains_resource::<crate::resources::MapEditorState>());
}
