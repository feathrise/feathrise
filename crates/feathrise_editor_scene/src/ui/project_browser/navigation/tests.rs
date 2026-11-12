//! 单一工程目录跨入和退出嵌套资产根的真实后台回归；只操作独占临时工程，不安装渲染或玩法。

use super::*;
use crate::{asset_catalog::project_files::{start_project_file_scan, poll_project_file_scan},
    resources::SceneEditorNumericScrubMouseGuard,
    ui::sidebars::EditorNavigatorTab};
use bevy::{app::TaskPoolPlugin, ecs::system::RunSystemOnce};
use std::{sync::atomic::{AtomicU64, Ordering}, time::{Duration, SystemTime, UNIX_EPOCH}};

struct Fixture { root: PathBuf, parent: PathBuf }

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::temp_dir().canonicalize().unwrap();
        let root = parent.join(format!("feathrise-engineering-navigation-{}-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&root).unwrap();
        for folder in ["content", "content/data", "content/data/models", "src"] { std::fs::create_dir(root.join(folder)).unwrap(); }
        Self { root: root.canonicalize().unwrap(), parent }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let root = self.root.canonicalize().unwrap();
        assert_eq!(root.parent(), Some(self.parent.as_path()));
        assert!(root.file_name().unwrap().to_string_lossy().starts_with("feathrise-engineering-navigation-"));
        std::fs::remove_dir_all(root).unwrap(); // 仅回收本测试亲自创建、重新核对归属的目录。
    }
}

fn app(fixture: &Fixture, folder: &str) -> App {
    let mut app = App::new();
    app.insert_resource(feathrise_editor_project_browser::directory_operations());
    app.add_plugins(TaskPoolPlugin::default());
    let mut files = EditorProjectFileCatalog::default();
    files.bind_project_source(21, Some(fixture.root.clone()));
    files.active = true;
    files.request_folder(folder).unwrap();
    let mut assets = EditorAssetCatalog::default();
    assets.bind_project_source(21, Some(fixture.root.join("content/data")));
    let mut sidebar = SceneEditorSidebarState::default();
    sidebar.navigator = EditorNavigatorTab::Assets;
    app.insert_resource(files).insert_resource(assets)
        .insert_resource(WorkbenchModel { context: 21, project_root: Some(fixture.root.clone()),
            asset_root: Some(fixture.root.join("content/data")), ..default() })
        .insert_resource(SceneEditorState { open: true, selected_asset: Some("models/保留选择.glb".into()), ..default() })
        .insert_resource(sidebar)
        .init_resource::<SceneEditorLayoutState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<ProjectBrowserState>()
        .add_systems(Update, (start_project_file_scan, poll_project_file_scan,
            crate::asset_catalog::start_asset_scan, crate::asset_catalog::poll_asset_scan, follow_directory).chain());
    app
}

fn advance(app: &mut App, ready: impl Fn(&World) -> bool) {
    let start = std::time::Instant::now();
    loop {
        app.update();
        if ready(app.world()) { return; }
        assert!(start.elapsed() < Duration::from_secs(5), "工程目录后台导航未在截止前收敛");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn nested_asset_root_uses_original_catalog_and_root_up_returns_to_real_project_parent_once() {
    let fixture = Fixture::new();
    let mut app = app(&fixture, "content/data");
    advance(&mut app, |world| world.resource::<ProjectBrowserState>().view == ProjectBrowserView::Assets);
    assert_eq!(app.world().resource::<ProjectBrowserState>().asset_prefix.as_deref(), Some("content/data"));
    assert!(app.world().resource::<EditorAssetCatalog>().current_folder.is_empty());
    assert!(app.world().resource::<EditorAssetCatalog>().revealed_revision().is_some());
    let click = app.world_mut().spawn((SceneEditorActionButton(EditorAction::AssetFolderUp), EditorButtonClick::triggered())).id();
    app.world_mut().run_system_once(asset_root_up).unwrap();
    assert!(!app.world().get::<EditorButtonClick>(click).unwrap().is_clicked(), "原动作必须消费不到这次释放");
    assert_eq!(app.world().resource::<ProjectBrowserState>().view, ProjectBrowserView::ProjectFiles);
    advance(&mut app, |world| {
        let files = world.resource::<EditorProjectFileCatalog>();
        !files.is_loading() && files.current_folder == "content"
    });
    assert_eq!(app.world().resource::<SceneEditorState>().selected_asset.as_deref(), Some("models/保留选择.glb"));
    assert_eq!(app.world().resource::<ProjectBrowserState>().view, ProjectBrowserView::ProjectFiles);
}

#[test]
fn plain_asset_startup_scan_does_not_take_over_project_root() {
    let fixture = Fixture::new();
    let mut app = app(&fixture, "");
    advance(&mut app, |world| {
        !world.resource::<EditorProjectFileCatalog>().is_loading()
            && world.resource::<EditorAssetCatalog>().feedback.as_ref().is_some_and(|feedback| feedback.result.is_ok())
    });
    assert_eq!(app.world().resource::<ProjectBrowserState>().view, ProjectBrowserView::ProjectFiles);
    assert!(app.world().resource::<EditorAssetCatalog>().revealed_revision().is_none());
}

#[test]
fn new_project_navigation_invalidates_inflight_asset_route_and_preserves_selection() {
    let fixture = Fixture::new();
    let mut app = app(&fixture, "content/data");
    advance(&mut app, |world| world.resource::<EditorAssetCatalog>().pending_reveal_revision().is_some());
    app.world_mut().resource_mut::<EditorProjectFileCatalog>().request_folder("").unwrap();
    advance(&mut app, |world| !world.resource::<EditorProjectFileCatalog>().is_loading());
    for _ in 0..5 { app.update(); }
    assert_eq!(app.world().resource::<ProjectBrowserState>().view, ProjectBrowserView::ProjectFiles);
    assert!(app.world().resource::<EditorProjectFileCatalog>().current_folder.is_empty());
    assert_eq!(app.world().resource::<SceneEditorState>().selected_asset.as_deref(), Some("models/保留选择.glb"));
}

#[test]
fn explicit_asset_folder_location_follows_original_success_intent() {
    let fixture = Fixture::new();
    let mut app = app(&fixture, "");
    advance(&mut app, |world| !world.resource::<EditorProjectFileCatalog>().is_loading());
    app.world_mut().resource_scope(|world, mut assets: Mut<EditorAssetCatalog>| {
        world.resource_scope(|world, mut editor: Mut<SceneEditorState>| {
            let mut sidebar = world.resource_mut::<SceneEditorSidebarState>();
            request_asset_folder(&mut assets, &mut editor, &mut sidebar, "models").unwrap();
        });
    });
    advance(&mut app, |world| world.resource::<EditorAssetCatalog>().current_folder == "models");
    assert_eq!(app.world().resource::<ProjectBrowserState>().view, ProjectBrowserView::Assets);
    assert_eq!(app.world().resource::<SceneEditorState>().selected_asset.as_deref(), Some("models/保留选择.glb"));
}
