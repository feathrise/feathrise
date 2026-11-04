//! 个人记录真实 IO、迟到快照、访问去重和输入回归；所有文件均在临时项目内。

use super::{constants::*, helpers, resources::AssetShortcuts, tasks, types::*, worker};
use bevy::prelude::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    project: PathBuf,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let project = std::env::temp_dir().join(format!(
            "rpg-asset-shortcuts-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let root = project.join("assets");
        std::fs::create_dir_all(root.join("中文 目录")).unwrap();
        std::fs::write(project.join("Cargo.toml"), "# 临时项目\n").unwrap();
        std::fs::write(root.join("中文 目录/配置.json"), "{\"值\":1}").unwrap();
        Self { project, root }
    }
    fn file(&self) -> PathBuf {
        self.project.join(".local").join(STATE_FILE)
    }
    fn run(&self, kind: JobKind, snapshot: Snapshot) -> Snapshot {
        worker::run(Job {
            root: self.root.clone(),
            revision: 1,
            kind,
            snapshot,
        })
        .result
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(
            self.project.starts_with(std::env::temp_dir())
                && self
                    .project
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("rpg-asset-shortcuts-")
        );
        let _ = std::fs::remove_dir_all(&self.project); // 只清理本测试创建且核对过边界的临时项目。
    }
}
fn target(path: &str, directory: bool) -> ShortcutTarget {
    helpers::target(path, directory).unwrap()
}
fn entry(path: &str, directory: bool) -> ShortcutEntry {
    ShortcutEntry {
        target: target(path, directory),
        available: true,
    }
}

#[test]
fn shortcut_paths_are_project_relative_and_reject_traversal_and_subasset_records() {
    assert_eq!(
        target("中文 目录/配置.json", false).path,
        "assets/中文 目录/配置.json"
    );
    assert_eq!(
        target("gltf/人物.glb#Scene0", false).path,
        "assets/gltf/人物.glb"
    );
    for path in ["../a", "/a", "C:/a", "a\\b", ".trash/a", "", "a/./b"] {
        assert!(helpers::target(path, false).is_err(), "{path}");
    }
    assert!(
        helpers::asset_path(&ShortcutTarget {
            path: "assets/a#Scene0".into(),
            is_directory: false
        })
        .is_err()
    );
    assert!(helpers::target(&"a".repeat(PATH_BYTES), false).is_err());
}

#[test]
fn favorites_and_mru_have_separate_budgets_and_repeat_visits_do_not_dirty() {
    let mut state = AssetShortcuts {
        loaded: true,
        ..default()
    };
    for index in 0..RECENT + 3 {
        helpers::change(
            &mut state,
            Change::Visit(target(&format!("file{index}.rs"), false)),
        )
        .unwrap();
    }
    assert_eq!(state.snapshot.recent.len(), RECENT);
    let recent = state.snapshot.recent[7].target.clone();
    helpers::change(&mut state, Change::Visit(recent.clone())).unwrap();
    assert_eq!(state.snapshot.recent[0].target, recent);
    let revision = state.revision;
    helpers::change(&mut state, Change::Visit(recent.clone())).unwrap();
    assert_eq!(state.revision, revision);
    for index in 0..FAVORITES {
        helpers::change(
            &mut state,
            Change::Favorite(target(&format!("folder{index}"), true), true),
        )
        .unwrap();
    }
    assert!(helpers::change(&mut state, Change::Favorite(target("overflow", true), true)).is_err());
    helpers::change(&mut state, Change::Favorite(target("folder0", true), false)).unwrap();
    assert_eq!(state.snapshot.favorites.len(), FAVORITES - 1);
    assert_eq!(state.snapshot.recent[0].target, recent);
}

#[test]
fn real_save_reload_preserves_unicode_files_folders_and_missing_entries() {
    let fixture = Fixture::new();
    let snapshot = Snapshot {
        favorites: vec![
            entry("中文 目录", true),
            entry("中文 目录/配置.json", false),
            entry("已删除.png", false),
        ],
        recent: vec![entry("中文 目录/配置.json", false)],
    };
    fixture.run(JobKind::Save, snapshot);
    let loaded = fixture.run(JobKind::Load, Snapshot::default());
    assert_eq!(
        loaded
            .favorites
            .iter()
            .map(|entry| entry.available)
            .collect::<Vec<_>>(),
        vec![true, true, false]
    );
    let json = std::fs::read_to_string(fixture.file()).unwrap();
    assert!(json.contains("assets/中文 目录/配置.json"));
    assert!(!json.contains(&fixture.project.to_string_lossy().to_string()));
    let moved = Fixture::new();
    std::fs::create_dir_all(moved.project.join(".local")).unwrap();
    std::fs::copy(fixture.file(), moved.file()).unwrap();
    assert_eq!(
        moved.run(JobKind::Load, default()),
        loaded,
        "换工作树后相对记录仍定位本项目"
    );
}

#[test]
fn deleted_or_type_changed_paths_are_marked_and_can_be_removed_or_rechecked() {
    let fixture = Fixture::new();
    fixture.run(
        JobKind::Save,
        Snapshot {
            favorites: vec![
                entry("中文 目录", false),
                entry("中文 目录/配置.json", true),
                entry("恢复.png", false),
            ],
            ..default()
        },
    );
    let mut loaded = fixture.run(JobKind::Load, default());
    assert!(loaded.favorites.iter().all(|entry| !entry.available));
    std::fs::write(fixture.root.join("恢复.png"), "fixture").unwrap();
    loaded = fixture.run(JobKind::Check, loaded);
    assert!(loaded.favorites[2].available);
    let missing = loaded.favorites[0].target.clone();
    helpers::apply(&mut loaded, &Change::Remove(Section::Favorites, missing)).unwrap();
    assert_eq!(loaded.favorites.len(), 2);
}

#[test]
fn initial_load_replays_clicks_and_old_save_does_not_overwrite_new_changes() {
    let mut state = AssetShortcuts::default();
    helpers::change(
        &mut state,
        Change::Favorite(target("新收藏.md", false), true),
    )
    .unwrap();
    helpers::change(&mut state, Change::Visit(target("已访问.rs", false))).unwrap();
    let disk = Snapshot {
        favorites: vec![entry("旧收藏", true)],
        ..default()
    };
    let root = state.root.clone();
    tasks::adopt(
        &mut state,
        Completion {
            root: root.clone(),
            revision: 0,
            kind: JobKind::Load,
            result: Ok(disk),
        },
    );
    assert!(state.is_favorite("旧收藏", true) && state.is_favorite("新收藏.md", false));
    assert_eq!(state.snapshot.recent[0].target.path, "assets/已访问.rs");
    let revision = state.revision;
    let snapshot = state.snapshot.clone();
    helpers::change(
        &mut state,
        Change::Remove(Section::Favorites, target("新收藏.md", false)),
    )
    .unwrap();
    tasks::adopt(
        &mut state,
        Completion {
            root,
            revision,
            kind: JobKind::Save,
            result: Ok(snapshot),
        },
    );
    assert!(!state.is_favorite("新收藏.md", false));
    assert_ne!(state.revision, state.saved_revision);
}

#[test]
fn invalid_or_incompatible_records_are_preserved_and_never_auto_overwritten() {
    let fixture = Fixture::new();
    for bytes in [
        b"broken".as_slice(),
        b"{\"version\":2,\"favorites\":[],\"recent\":[]}".as_slice(),
    ] {
        std::fs::create_dir_all(fixture.project.join(".local")).unwrap();
        std::fs::write(fixture.file(), bytes).unwrap();
        let completed = worker::run(Job {
            root: fixture.root.clone(),
            revision: 0,
            kind: JobKind::Load,
            snapshot: default(),
        });
        let mut state = AssetShortcuts {
            root: fixture.root.clone(),
            ..default()
        };
        tasks::adopt(&mut state, completed);
        assert!(state.loaded && state.save_blocked);
        helpers::change(
            &mut state,
            Change::Favorite(target("中文 目录", true), true),
        )
        .unwrap();
        let checked = worker::run(Job {
            root: fixture.root.clone(),
            revision: state.revision,
            kind: JobKind::Check,
            snapshot: state.snapshot.clone(),
        });
        tasks::adopt(&mut state, checked);
        assert!(state.feedback.contains("未保存"));
        assert_eq!(std::fs::read(fixture.file()).unwrap(), bytes);
    }
    assert!(
        worker::decode_for_tests(
            &serde_json::to_vec(&SavedNavigation {
                version: 1,
                favorites: vec![target("a", false); FAVORITES + 1],
                recent: vec![]
            })
            .unwrap()
        )
        .is_err()
    );
}

fn app(fixture: &Fixture) -> App {
    let mut app = App::new();
    let mut sidebar = crate::ui::sidebars::MapEditorSidebarState::default();
    sidebar.navigator = crate::ui::sidebars::EditorNavigatorTab::Assets;
    app.add_plugins(MinimalPlugins)
        .insert_resource(crate::resources::MapEditorState {
            open: true,
            ..default()
        })
        .init_resource::<crate::resources::EditorAssetCatalog>()
        .init_resource::<crate::resources::MapEditorNumericScrubMouseGuard>()
        .insert_resource(sidebar)
        .init_resource::<ButtonInput<MouseButton>>()
        .insert_resource(AssetShortcuts {
            root: fixture.root.clone(),
            ..default()
        });
    super::register(&mut app);
    app.add_systems(Update, (super::systems::control, tasks::drive).chain());
    app
}
fn settle(app: &mut App) {
    for _ in 0..1000 {
        app.update();
        let state = app.world().resource::<AssetShortcuts>();
        if state.loaded
            && state.task.is_none()
            && (state.save_blocked
                || state.revision == state.saved_revision
                || state.failed_revision == Some(state.revision))
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("导航后台任务未完成");
}

#[test]
fn actual_message_pipeline_flushes_after_f12_close_and_survives_restart() {
    let fixture = Fixture::new();
    let mut app = app(&fixture);
    app.world_mut().write_message(super::SetAssetFavorite {
        path: "中文 目录/配置.json".into(),
        is_directory: false,
        favorite: true,
    });
    app.world_mut().write_message(super::AssetVisited {
        path: "中文 目录".into(),
        is_directory: true,
    });
    app.update();
    app.world_mut()
        .resource_mut::<crate::resources::MapEditorState>()
        .open = false;
    settle(&mut app);
    assert!(
        app.world()
            .resource::<AssetShortcuts>()
            .is_favorite("中文 目录/配置.json", false)
    );
    assert_eq!(
        fixture.run(JobKind::Load, default()).recent[0].target.path,
        "assets/中文 目录"
    );
    let editor = app.world().resource::<crate::resources::MapEditorState>();
    assert!(!editor.dirty && !editor.save_requested);
}

#[test]
fn compact_list_height_shrinks_and_caps_before_scrolling() {
    assert_eq!(super::scene::list_height(0), 0.0);
    assert_eq!(super::scene::list_height(1), ROW_HEIGHT);
    assert_eq!(super::scene::list_height(2), ROW_HEIGHT * 2.0 + ROW_GAP);
    assert_eq!(super::scene::list_height(FAVORITES), LIST_HEIGHT);
}

#[test]
fn explicit_favorite_intent_is_idempotent_against_unknown_disk_baseline() {
    for already_saved in [false, true] {
        let mut state = AssetShortcuts::default();
        helpers::change(
            &mut state,
            Change::Favorite(target("已有文件.rs", false), true),
        )
        .unwrap();
        helpers::change(
            &mut state,
            Change::Favorite(target("已有文件.rs", false), true),
        )
        .unwrap();
        let snapshot = Snapshot {
            favorites: if already_saved {
                vec![entry("已有文件.rs", false)]
            } else {
                vec![]
            },
            ..default()
        };
        let root = state.root.clone();
        tasks::adopt(
            &mut state,
            Completion {
                root,
                revision: 0,
                kind: JobKind::Load,
                result: Ok(snapshot),
            },
        );
        assert_eq!(state.snapshot.favorites.len(), 1);
        assert!(state.is_favorite("已有文件.rs", false));
        assert_eq!(state.revision == state.saved_revision, already_saved);
        helpers::change(
            &mut state,
            Change::Favorite(target("已有文件.rs", false), false),
        )
        .unwrap();
        helpers::change(
            &mut state,
            Change::Favorite(target("已有文件.rs", false), false),
        )
        .unwrap();
        assert!(state.snapshot.favorites.is_empty());
    }
}
