//! 混合选集的整树去重、完整拒绝、受检落盘及取消/迟到回执保护。

use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::{map_browser::helpers::build_browser_tree, types::MapFolderRecord};

fn folder(id: &str, name: &str, parent: Option<&str>) -> MapFolderRecord {
    MapFolderRecord {
        id: id.into(),
        display_name: name.into(),
        parent_id: parent.map(str::to_owned),
    }
}

fn map(id: &str) -> MapBrowserItem {
    MapBrowserItem::Map(feathrise_map_content::MapId::parse(id).unwrap())
}

fn item(id: &str) -> MapBrowserItem {
    MapBrowserItem::Folder(id.into())
}

fn document() -> MapFolderDocument {
    MapFolderDocument {
        folders: vec![
            folder("FOLDER-A", "父目录", None),
            folder("FOLDER-AA", "子目录", Some("FOLDER-A")),
            folder("FOLDER-B", "目标", None),
            folder("FOLDER-C", "另一个目录", None),
        ],
        map_folders: [
            ("MAP-1".into(), "FOLDER-AA".into()),
            ("MAP-2".into(), "FOLDER-B".into()),
        ]
        .into(),
        ..default()
    }
}

fn entries(document: &MapFolderDocument) -> Vec<MapBrowserEntry> {
    let maps = (1..=3)
        .map(|id| {
            let id = format!("MAP-{id}");
            MapBrowserEntry {
                item: map(&id),
                display_name: id.clone(),
                depth: 0,
                published: false,
                parent_folder_id: document.map_folders.get(&id).cloned(),
            }
        })
        .collect();
    build_browser_tree(document.folders.clone(), maps)
}

#[test]
fn mixed_move_deduplicates_selected_descendants_and_keeps_subtree_links() {
    let original = document();
    let selected = BTreeSet::from([
        item("FOLDER-A"),
        item("FOLDER-AA"),
        map("MAP-1"),
        item("FOLDER-C"),
        map("MAP-3"),
    ]);
    let snapshot = prepare_batch(
        original.clone(),
        &entries(&original),
        &selected,
        &MapBrowserDropTarget::Folder("FOLDER-B".into()),
    )
    .unwrap()
    .unwrap();
    let next = snapshot.document;
    assert_eq!(
        folder_by_id(&next, "FOLDER-A")
            .unwrap()
            .parent_id
            .as_deref(),
        Some("FOLDER-B")
    );
    assert_eq!(
        folder_by_id(&next, "FOLDER-C")
            .unwrap()
            .parent_id
            .as_deref(),
        Some("FOLDER-B")
    );
    assert_eq!(
        folder_by_id(&next, "FOLDER-AA")
            .unwrap()
            .parent_id
            .as_deref(),
        Some("FOLDER-A")
    );
    assert_eq!(next.map_folders["MAP-1"], "FOLDER-AA");
    assert_eq!(next.map_folders["MAP-2"], "FOLDER-B");
    assert_eq!(next.map_folders["MAP-3"], "FOLDER-B");
    let back = prepare_batch(
        next.clone(),
        &entries(&next),
        &selected,
        &MapBrowserDropTarget::Root,
    )
    .unwrap()
    .unwrap();
    assert_eq!(back.document, original);
}

#[test]
fn noop_mixed_sources_write_nothing_and_missing_members_reject_the_whole_batch() {
    let original = document();
    let selected = BTreeSet::from([
        item("FOLDER-A"),
        item("FOLDER-AA"),
        map("MAP-1"),
        item("FOLDER-C"),
        map("MAP-3"),
    ]);
    assert!(
        prepare_batch(
            original.clone(),
            &entries(&original),
            &selected,
            &MapBrowserDropTarget::Root
        )
        .unwrap()
        .is_none()
    );
    for missing in [map("MAP-removed"), item("removed"), MapBrowserItem::Root] {
        let group = BTreeSet::from([map("MAP-3"), missing]);
        assert!(
            prepare_batch(
                original.clone(),
                &entries(&original),
                &group,
                &MapBrowserDropTarget::Folder("FOLDER-B".into())
            )
            .is_err()
        );
    }
}

#[test]
fn one_invalid_member_rejects_cycles_missing_targets_and_two_sources_with_same_name() {
    let mut original = document();
    for destination in ["FOLDER-A", "FOLDER-AA", "removed"] {
        assert!(
            prepare_batch(
                original.clone(),
                &entries(&original),
                &BTreeSet::from([item("FOLDER-A"), map("MAP-3")]),
                &MapBrowserDropTarget::Folder(destination.into())
            )
            .is_err()
        );
    }
    original
        .folders
        .extend([folder("FOLDER-CA", "子目录", Some("FOLDER-C"))]);
    let selected = BTreeSet::from([item("FOLDER-AA"), item("FOLDER-CA"), map("MAP-3")]);
    assert!(
        prepare_batch(
            original.clone(),
            &entries(&original),
            &selected,
            &MapBrowserDropTarget::Folder("FOLDER-B".into())
        )
        .unwrap_err()
        .contains("同")
    );
    // 即使批中地图本可移动，失败输入保持原值，不能出现半批结果。
    assert!(!original.map_folders.contains_key("MAP-3"));
    assert_eq!(
        folder_by_id(&original, "FOLDER-AA")
            .unwrap()
            .parent_id
            .as_deref(),
        Some("FOLDER-A")
    );
}

#[test]
fn deep_subtrees_are_validated_once_after_all_parent_links_change() {
    let mut original = document();
    let mut parent = "FOLDER-B".to_owned();
    for depth in 2..=MAP_FOLDER_MAX_DEPTH {
        let id = format!("FOLDER-D{depth}");
        original
            .folders
            .push(folder(&id, &format!("层{depth}"), Some(&parent)));
        parent = id;
    }
    assert!(
        prepare_batch(
            original.clone(),
            &entries(&original),
            &BTreeSet::from([item("FOLDER-A"), map("MAP-3")]),
            &MapBrowserDropTarget::Folder(parent)
        )
        .is_err()
    );
}

#[test]
fn batch_snapshot_round_trips_through_the_original_checked_atomic_writer() {
    let root = std::env::temp_dir().join(format!(
        "map-browser-batch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let path = super::super::persistence::folder_file_path(&root);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = document();
    let bytes = encode_folder_document(&original).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let selected = BTreeSet::from([item("FOLDER-A"), map("MAP-3")]);
    let snapshot = prepare_batch(
        original.clone(),
        &entries(&original),
        &selected,
        &MapBrowserDropTarget::Folder("FOLDER-B".into()),
    )
    .unwrap()
    .unwrap();
    let saved = write_snapshot_if_unchanged(&root, Some(bytes.clone()), snapshot.clone()).unwrap();
    assert_eq!(
        decode_folder_document(&std::fs::read(&path).unwrap()).unwrap(),
        saved.document
    );
    std::fs::write(&path, b"external-change").unwrap();
    assert!(matches!(
        write_snapshot_if_unchanged(&root, Some(bytes), snapshot),
        Err(MapFolderWriteFailure::ConcurrentChange(_))
    ));
    assert_eq!(std::fs::read(&path).unwrap(), b"external-change");
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}

fn poll_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .insert_resource(MapBrowserState {
            document: document(),
            document_revision: 7,
            initial_load_resolved: true,
            write_blocked: None,
            ..default()
        })
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        })
        .init_resource::<MapFolderPersistence>()
        .add_systems(Update, poll_map_folder_batch);
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    app
}

fn settle(app: &mut App) {
    for _ in 0..1000 {
        app.update();
        if !app.world().resource::<MapFolderPersistence>().busy() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("批量后台任务应能非阻塞完成");
}

#[test]
fn cancelled_preparation_never_enters_io_and_failed_io_keeps_the_entire_old_session() {
    for changed_revision in [false, true] {
        let mut app = poll_app();
        let original = app.world().resource::<MapBrowserState>().document.clone();
        let snapshot = prepare_batch(
            original.clone(),
            &entries(&original),
            &BTreeSet::from([item("FOLDER-A"), map("MAP-3")]),
            &MapBrowserDropTarget::Folder("FOLDER-B".into()),
        )
        .unwrap()
        .unwrap();
        let phase = if changed_revision {
            app.world_mut()
                .resource_mut::<MapBrowserState>()
                .document_revision += 1;
            MapFolderBatchPhase::Preparing(
                AsyncComputeTaskPool::get().spawn(async move { Ok(Some(snapshot)) }),
            )
        } else {
            MapFolderBatchPhase::Saving(
                IoTaskPool::get()
                    .spawn(async { Err(MapFolderWriteFailure::Io("模拟设备错误".into())) }),
            )
        };
        app.world_mut()
            .resource_mut::<MapBrowserState>()
            .select_only(Some(map("MAP-3")));
        app.world_mut()
            .resource_mut::<MapBrowserState>()
            .toggle_selection(item("FOLDER-A"));
        app.world_mut().resource_mut::<MapFolderPersistence>().batch = Some(MapFolderBatchMove {
            asset_root: workspace_asset_root(),
            ticket: 0,
            source: MapBrowserIndexSignature {
                folder_revision: 7,
                catalog_revision: 0,
            },
            target: MapBrowserDropTarget::Folder("FOLDER-B".into()),
            expected_bytes: None,
            cancelled: false,
            phase,
        });
        settle(&mut app);
        let browser = app.world().resource::<MapBrowserState>();
        assert_eq!(browser.document, original);
        assert_eq!(browser.selection_count(), 2);
        assert!(browser.source_bytes.is_none());
        assert!(!app.world().resource::<MapEditorState>().dirty);
    }
}

#[test]
fn successful_batch_receipt_publishes_once_preserves_selection_and_rejects_late_receipts() {
    for late in [false, true] {
        let mut app = poll_app();
        let original = document();
        let snapshot = prepare_batch(
            original.clone(),
            &entries(&original),
            &BTreeSet::from([item("FOLDER-A"), map("MAP-3")]),
            &MapBrowserDropTarget::Folder("FOLDER-B".into()),
        )
        .unwrap()
        .unwrap();
        let expected = snapshot.document.clone();
        {
            let mut browser = app.world_mut().resource_mut::<MapBrowserState>();
            browser.select_only(Some(map("MAP-3")));
            browser.toggle_selection(item("FOLDER-A"));
            if late {
                browser.document_revision += 1;
            }
        }
        app.world_mut().resource_mut::<MapEditorState>().dirty = true;
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .selected_map_id = Some(feathrise_map_content::MapId::parse("MAP-current").unwrap());
        app.world_mut().resource_mut::<MapFolderPersistence>().batch = Some(MapFolderBatchMove {
            asset_root: workspace_asset_root(),
            ticket: 0,
            source: MapBrowserIndexSignature {
                folder_revision: 7,
                catalog_revision: 0,
            },
            target: MapBrowserDropTarget::Folder("FOLDER-B".into()),
            expected_bytes: None,
            cancelled: false,
            phase: MapFolderBatchPhase::Saving(
                IoTaskPool::get().spawn(async move { Ok(snapshot) }),
            ),
        });
        settle(&mut app);
        let browser = app.world().resource::<MapBrowserState>();
        assert_eq!(browser.selection_count(), 2);
        assert_eq!(browser.document_revision, 8);
        if late {
            assert_eq!(browser.document, original);
            assert!(browser.write_blocked.is_some());
        } else {
            assert_eq!(browser.document, expected);
            assert!(browser.expanded_folder_ids.contains("FOLDER-B"));
            assert_eq!(
                decode_folder_document(browser.source_bytes.as_deref().unwrap()).unwrap(),
                expected
            );
        }
        assert!(app.world().resource::<MapEditorState>().dirty);
        assert_eq!(
            app.world()
                .resource::<MapEditorState>()
                .selected_map_id
                .as_ref()
                .unwrap()
                .as_str(),
            "MAP-current"
        );
    }
}

#[test]
fn hidden_closed_unfocused_and_modal_preparation_is_cancelled_without_writing() {
    for reason in 0..5 {
        let mut app = poll_app();
        match reason {
            0 => app.world_mut().resource_mut::<MapEditorState>().open = false,
            1 => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .new_map_dialog_open = true
            }
            2 => {
                let world = app.world_mut();
                world
                    .query_filtered::<&mut Window, With<PrimaryWindow>>()
                    .single_mut(world)
                    .unwrap()
                    .focused = false;
            }
            3 => {
                let mut sidebar = crate::ui::sidebars::MapEditorSidebarState::default();
                sidebar.navigator = crate::ui::sidebars::EditorNavigatorTab::Scene;
                app.insert_resource(sidebar);
            }
            _ => {
                app.insert_resource(crate::resources::MapEditorSettingsState {
                    open: true,
                    ..default()
                });
            }
        }
        app.world_mut().resource_mut::<MapFolderPersistence>().batch = Some(MapFolderBatchMove {
            asset_root: workspace_asset_root(),
            ticket: 0,
            source: MapBrowserIndexSignature {
                folder_revision: 7,
                catalog_revision: 0,
            },
            target: MapBrowserDropTarget::Root,
            expected_bytes: None,
            cancelled: false,
            phase: MapFolderBatchPhase::Preparing(
                AsyncComputeTaskPool::get().spawn(async { Ok(None) }),
            ),
        });
        settle(&mut app);
        assert!(
            app.world()
                .resource::<MapEditorState>()
                .status
                .contains("取消批量整理")
        );
        assert_eq!(
            app.world().resource::<MapBrowserState>().document,
            document()
        );
    }
}

#[test]
fn mixed_drop_survives_the_real_shared_mouse_release_guard_and_saves_the_whole_batch() {
    use crate::{
        components::{MapEditorMapRootRow, MapEditorMapRow},
        resources::{
            MapBrowserDragState, MapCreationTask, MapDeletionTask, MapEditorNumericScrubMouseGuard,
        },
        types::MapBrowserDragPress,
        ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
    };
    use feathrise_editor_ui::ActiveNumericInputScrub;

    for root_target in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "map-browser-release-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = super::super::persistence::folder_file_path(&root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = document();
        let baseline = encode_folder_document(&original).unwrap();
        std::fs::write(&path, &baseline).unwrap();
        let mut app = poll_app();
        app.init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<ActiveNumericInputScrub>()
            .init_resource::<MapEditorNumericScrubMouseGuard>()
            .init_resource::<MapCreationTask>()
            .init_resource::<MapDeletionTask>()
            .insert_resource(MapBrowserIndexState {
                applied_signature: Some(MapBrowserIndexSignature {
                    folder_revision: 7,
                    catalog_revision: 0,
                }),
                full_entries: Arc::new(entries(&original)),
                ..default()
            });
        let selection = {
            let mut browser = app.world_mut().resource_mut::<MapBrowserState>();
            browser.source_bytes = Some(baseline.clone());
            browser.select_only(Some(item("FOLDER-AA")));
            browser.toggle_selection(map("MAP-1"));
            browser.toggle_selection(map("MAP-2"));
            browser.selection.clone()
        };
        app.insert_resource(MapBrowserDragState {
            active: true,
            blocked: true,
            selection: Some(selection),
            press: Some(MapBrowserDragPress {
                item: item("FOLDER-AA"),
                document_revision: 7,
                display_name: "所选目录".into(),
                start_physical: Vec2::ZERO,
            }),
            ..default()
        });
        if root_target {
            app.world_mut()
                .spawn((MapEditorMapRootRow, UiInteraction::Hovered));
        } else {
            app.world_mut().spawn((
                MapEditorMapRow {
                    slot: 0,
                    item: Some(item("FOLDER-B")),
                },
                UiInteraction::Hovered,
            ));
        }
        // 使用正式拖拽→共享仲裁→后台事务顺序；只将作者根替换为本例独立临时目录。
        let isolated_root = root.clone();
        app.add_systems(
            Update,
            (
                crate::map_browser::update_map_browser_drag,
                move |mut persistence: ResMut<MapFolderPersistence>| {
                    if let Some(batch) = persistence.batch.as_mut() {
                        batch.asset_root = isolated_root.clone();
                    }
                },
                update_map_editor_numeric_scrub_mouse_guard,
            )
                .chain()
                .before(poll_map_folder_batch),
        );
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        assert!(
            app.world()
                .resource::<MapEditorNumericScrubMouseGuard>()
                .blocked()
        );
        if root_target {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Right);
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        assert!(app.world().resource::<MapBrowserDragState>().blocked);
        assert!(
            app.world()
                .resource::<MapEditorNumericScrubMouseGuard>()
                .blocked()
        );
        let batch = app
            .world()
            .resource::<MapFolderPersistence>()
            .batch
            .as_ref()
            .expect("自己的release保护不能取消刚释放的批量移动");
        assert!(
            !batch.cancelled,
            "共享门包含拖拽自身的鼠标所有权，不能当成外部冲突"
        );
        assert_eq!(std::fs::read(&path).unwrap(), baseline);
        if root_target {
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .clear();
            app.update(); // 和弦中的右键仍按住时继续等待，不能提前提交。
            assert!(app.world().resource::<MapFolderPersistence>().busy());
            assert_eq!(std::fs::read(&path).unwrap(), baseline);
            app.world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .release(MouseButton::Right);
            app.update();
            assert!(
                app.world()
                    .resource::<MapEditorNumericScrubMouseGuard>()
                    .blocked()
            );
        }
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        settle(&mut app);
        let browser = app.world().resource::<MapBrowserState>();
        assert_eq!(
            folder_by_id(&browser.document, "FOLDER-AA")
                .unwrap()
                .parent_id
                .as_deref(),
            if root_target { None } else { Some("FOLDER-B") }
        );
        assert_eq!(browser.document.map_folders["MAP-1"], "FOLDER-AA"); // 被选后代不会被拆出子树。
        assert_eq!(
            browser
                .document
                .map_folders
                .get("MAP-2")
                .map(String::as_str),
            if root_target { None } else { Some("FOLDER-B") }
        );
        assert_eq!(browser.selection_count(), 3);
        assert_eq!(
            decode_folder_document(&std::fs::read(&path).unwrap()).unwrap(),
            browser.document
        );
        assert!(!app.world().resource::<MapEditorState>().dirty);
        drop(app);
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn an_external_numeric_scrub_still_cancels_preparation_during_own_release() {
    let mut app = poll_app();
    app.init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<crate::resources::MapEditorNumericScrubMouseGuard>()
        .insert_resource(crate::resources::MapBrowserDragState {
            blocked: true,
            ..default()
        });
    let field = app.world_mut().spawn_empty().id();
    app.world_mut()
        .resource_mut::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .entity = Some(field);
    app.world_mut()
        .resource_mut::<crate::resources::MapEditorNumericScrubMouseGuard>()
        .advance(true, false, false);
    app.world_mut().resource_mut::<MapFolderPersistence>().batch = Some(MapFolderBatchMove {
        ticket: 0,
        source: MapBrowserIndexSignature {
            folder_revision: 7,
            catalog_revision: 0,
        },
        target: MapBrowserDropTarget::Root,
        expected_bytes: None,
        asset_root: workspace_asset_root(),
        cancelled: false,
        phase: MapFolderBatchPhase::Preparing(
            AsyncComputeTaskPool::get().spawn(async { Ok(None) }),
        ),
    });
    settle(&mut app);
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .status
            .contains("取消批量整理")
    );
    assert_eq!(
        app.world().resource::<MapBrowserState>().document,
        document()
    );
}
