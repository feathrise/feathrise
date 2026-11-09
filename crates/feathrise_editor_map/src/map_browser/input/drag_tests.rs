//! 文件夹整树移动、目标拒绝和真实鼠标状态机回归；不启动项目目录写入任务。

use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::types::MapFolderRecord;
use crate::ui::sidebars::{EditorNavigatorTab, MapEditorSidebarState};
use feathrise_map_content::MapId;
use std::collections::{BTreeMap, BTreeSet};

fn folder(id: &str, name: &str, parent: Option<&str>) -> MapFolderRecord {
    MapFolderRecord {
        id: id.into(),
        display_name: name.into(),
        parent_id: parent.map(str::to_owned),
    }
}

fn browser() -> MapBrowserState {
    MapBrowserState {
        document: MapFolderDocument {
            folders: vec![
                folder("FOLDER-A", "区域", None),
                folder("FOLDER-B", "子目录", Some("FOLDER-A")),
                folder("FOLDER-C", "孙目录", Some("FOLDER-B")),
                folder("FOLDER-D", "目标", None),
            ],
            map_folders: BTreeMap::from([
                ("MAP-1".into(), "FOLDER-A".into()),
                ("MAP-2".into(), "FOLDER-C".into()),
            ]),
            ..default()
        },
        document_revision: 7,
        expanded_folder_ids: BTreeSet::from(["FOLDER-A".into(), "FOLDER-B".into()]),
        selected_item: Some(MapBrowserItem::Folder("FOLDER-A".into())),
        initial_load_resolved: true,
        write_blocked: None,
        ..default()
    }
}

fn press(item: MapBrowserItem) -> MapBrowserDragPress {
    MapBrowserDragPress {
        item,
        document_revision: 7,
        display_name: "区域".into(),
        start_physical: Vec2::new(10.0, 10.0),
    }
}

#[test]
fn prepared_folder_move_keeps_descendant_links_maps_selection_and_expansion() {
    let mut browser = browser();
    let original = browser.document.clone();
    let source = press(MapBrowserItem::Folder("FOLDER-A".into()));
    let change = prepare_drop(
        &browser,
        &source,
        &MapBrowserDropTarget::Folder("FOLDER-D".into()),
    )
    .unwrap()
    .unwrap();
    let mut expected = original.clone();
    expected.folders[0].parent_id = Some("FOLDER-D".into());
    assert_eq!(change.document, expected); // 唯一变化是源目录的父链接，所有地图和更深子目录都整体保留。
    assert_eq!(change.selected, source.item);
    assert!(change.expanded.is_superset(&browser.expanded_folder_ids));
    assert!(change.expanded.contains("FOLDER-D"));
    assert_eq!(browser.document, original); // 准备失败或尚未启动保存时不污染权威文档。
    browser.document = change.document;
    let change = prepare_drop(&browser, &source, &MapBrowserDropTarget::Root)
        .unwrap()
        .unwrap();
    assert_eq!(change.document, original);
    assert!(
        prepare_drop(
            &browser,
            &source,
            &MapBrowserDropTarget::Folder("FOLDER-D".into())
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn folder_move_rejects_self_descendants_missing_targets_duplicates_and_deep_subtrees() {
    let original = browser().document;
    for target in ["FOLDER-A", "FOLDER-B", "FOLDER-C", "FOLDER-missing"] {
        let mut document = original.clone();
        assert!(move_folder(&mut document, "FOLDER-A", Some(target)).is_err());
        assert_eq!(document, original, "失败不能改变结构：{target}");
    }
    let mut document = original.clone();
    document
        .folders
        .push(folder("FOLDER-DUP", "区域", Some("FOLDER-D")));
    let duplicate = document.clone();
    assert!(
        move_folder(&mut document, "FOLDER-A", Some("FOLDER-D"))
            .unwrap_err()
            .contains("同名")
    );
    assert_eq!(document, duplicate);
    assert!(move_folder(&mut document, "FOLDER-missing", None).is_err());
    let mut document = original;
    for depth in 0..14 {
        document.folders.push(folder(
            &format!("FOLDER-DEEP{depth}"),
            &format!("层{depth}"),
            (depth > 0)
                .then(|| format!("FOLDER-DEEP{}", depth - 1))
                .as_deref(),
        ));
    }
    let deep = document.clone();
    assert!(
        move_folder(&mut document, "FOLDER-A", Some("FOLDER-DEEP13"))
            .unwrap_err()
            .contains("层级")
    );
    assert_eq!(document, deep); // 根部还能放下，也必须检查整棵子树的最深后代。
}

#[test]
fn map_drop_keeps_selection_and_stale_folder_drop_cannot_be_retargeted() {
    let mut browser = browser();
    let map = press(MapBrowserItem::Map(MapId::parse("MAP-1").unwrap()));
    let change = prepare_drop(&browser, &map, &MapBrowserDropTarget::Root)
        .unwrap()
        .unwrap();
    assert!(!change.document.map_folders.contains_key("MAP-1"));
    assert_eq!(change.document.folders, browser.document.folders);
    assert_eq!(change.selected, map.item);
    browser.document_revision += 1;
    let folder = press(MapBrowserItem::Folder("FOLDER-A".into()));
    assert!(
        prepare_drop(
            &browser,
            &folder,
            &MapBrowserDropTarget::Folder("FOLDER-D".into())
        )
        .is_err()
    );
}

fn drag_app() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.insert_resource(browser())
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        })
        .init_resource::<MapFolderPersistence>()
        .init_resource::<MapCreationTask>()
        .init_resource::<MapDeletionTask>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<MapEditorSidebarState>()
        .insert_resource(MapBrowserDragState {
            press: Some(press(MapBrowserItem::Folder("FOLDER-A".into()))),
            ..default()
        })
        .add_systems(Update, update_map_browser_drag);
    let mut window = Window::default();
    window.set_physical_cursor_position(Some(Vec2::new(20.0, 10.0).into()));
    let window = app.world_mut().spawn((window, PrimaryWindow)).id();
    let row = app
        .world_mut()
        .spawn((
            MapEditorMapRow {
                slot: 0,
                item: Some(MapBrowserItem::Folder("FOLDER-A".into())),
            },
            UiInteraction::Hovered,
        ))
        .id();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(app.world().resource::<MapBrowserDragState>().active);
    (app, window, row)
}

#[test]
fn mixed_release_starts_one_batch_and_keeps_the_old_document_until_full_success() {
    bevy::tasks::AsyncComputeTaskPool::get_or_init(Default::default);
    let (mut app, _, row) = drag_app();
    let original = app.world().resource::<MapBrowserState>().document.clone();
    let selection = {
        let mut browser = app.world_mut().resource_mut::<MapBrowserState>();
        browser.source_bytes =
            Some(crate::map_browser::helpers::encode_folder_document(&original).unwrap());
        browser.select_only(Some(MapBrowserItem::Folder("FOLDER-A".into())));
        browser.toggle_selection(MapBrowserItem::Map(MapId::parse("MAP-2").unwrap()));
        browser.selection.clone()
    };
    app.world_mut()
        .resource_mut::<MapBrowserDragState>()
        .selection = Some(selection);
    let maps = (1..=2)
        .map(|number| {
            let id = MapId::parse(format!("MAP-{number}")).unwrap();
            crate::types::MapBrowserEntry {
                parent_folder_id: original.map_folders.get(id.as_str()).cloned(),
                item: MapBrowserItem::Map(id),
                display_name: "测试地图".into(),
                depth: 0,
                published: false,
            }
        })
        .collect();
    app.insert_resource(crate::resources::MapBrowserIndexState {
        applied_signature: Some(crate::types::MapBrowserIndexSignature {
            folder_revision: 7,
            catalog_revision: 0,
        }),
        full_entries: std::sync::Arc::new(crate::map_browser::helpers::build_browser_tree(
            original.folders.clone(),
            maps,
        )),
        rebuilding: true, // 只有展开投影待采用时，完整来源仍有效，释放不必无意义拒绝。
        ..default()
    });
    app.world_mut()
        .get_mut::<MapEditorMapRow>(row)
        .unwrap()
        .item = Some(MapBrowserItem::Folder("FOLDER-D".into()));
    app.update();
    assert_eq!(
        app.world().resource::<MapBrowserDragState>().drop_target,
        Some(MapBrowserDropTarget::Folder("FOLDER-D".into()))
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    let persistence = app.world().resource::<MapFolderPersistence>();
    assert!(persistence.batch.is_some());
    assert!(persistence.task.is_none());
    assert!(persistence.busy());
    assert_eq!(app.world().resource::<MapBrowserState>().document, original);
    assert_eq!(
        app.world().resource::<MapBrowserState>().selection_count(),
        2
    );
    assert!(!app.world().resource::<MapBrowserDragState>().active);
    assert!(app.world().resource::<MapBrowserDragState>().blocked);
    assert!(!app.world().resource::<MapEditorState>().dirty);
}

#[test]
fn invalid_hover_is_not_a_drop_target_and_release_never_starts_a_save() {
    let (mut app, _, row) = drag_app();
    let original = app.world().resource::<MapBrowserState>().document.clone();
    for id in ["FOLDER-A", "FOLDER-B", "FOLDER-C"] {
        app.world_mut()
            .get_mut::<MapEditorMapRow>(row)
            .unwrap()
            .item = Some(MapBrowserItem::Folder(id.into()));
        app.update();
        let drag = app.world().resource::<MapBrowserDragState>();
        assert!(drag.drop_target.is_none());
        assert!(drag.drop_error.as_ref().unwrap().contains("自身或子文件夹"));
    }
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert_eq!(app.world().resource::<MapBrowserState>().document, original);
    assert!(!app.world().resource::<MapFolderPersistence>().busy());
    assert!(app.world().resource::<MapBrowserDragState>().blocked);
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .status
            .contains("自身或子文件夹")
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.update();
    assert!(!app.world().resource::<MapBrowserDragState>().blocked);
}

#[test]
fn valid_folder_and_root_hover_are_highlighted_without_changing_the_document() {
    let (mut app, _, row) = drag_app();
    let original = app.world().resource::<MapBrowserState>().document.clone();
    app.world_mut()
        .get_mut::<MapEditorMapRow>(row)
        .unwrap()
        .item = Some(MapBrowserItem::Folder("FOLDER-D".into()));
    app.update();
    assert_eq!(
        app.world().resource::<MapBrowserDragState>().drop_target,
        Some(MapBrowserDropTarget::Folder("FOLDER-D".into()))
    );
    *app.world_mut().get_mut::<UiInteraction>(row).unwrap() = UiInteraction::None;
    app.world_mut()
        .spawn((MapEditorMapRootRow, UiInteraction::Hovered));
    app.update();
    assert_eq!(
        app.world().resource::<MapBrowserDragState>().drop_target,
        Some(MapBrowserDropTarget::Root)
    );
    assert_eq!(app.world().resource::<MapBrowserState>().document, original);
    assert!(!app.world().resource::<MapFolderPersistence>().busy());
}

#[test]
fn pressed_origin_does_not_mask_the_hovered_destination_or_root() {
    let (mut app, _, origin) = drag_app();
    *app.world_mut().get_mut::<UiInteraction>(origin).unwrap() = UiInteraction::Pressed;
    app.world_mut()
        .entity_mut(origin)
        .insert(RelativeCursorPosition::default()); // 光标已离开，源行仍保持原生Pressed。
    let target = app
        .world_mut()
        .spawn((
            MapEditorMapRow {
                slot: 1,
                item: Some(MapBrowserItem::Folder("FOLDER-D".into())),
            },
            UiInteraction::Hovered,
            RelativeCursorPosition {
                cursor_over: true,
                ..default()
            },
        ))
        .id();
    app.update();
    assert_eq!(
        app.world().resource::<MapBrowserDragState>().drop_target,
        Some(MapBrowserDropTarget::Folder("FOLDER-D".into()))
    );
    *app.world_mut().get_mut::<UiInteraction>(target).unwrap() = UiInteraction::None;
    app.world_mut()
        .spawn((MapEditorMapRootRow, UiInteraction::Hovered));
    app.update();
    assert_eq!(
        app.world().resource::<MapBrowserDragState>().drop_target,
        Some(MapBrowserDropTarget::Root)
    );
}

#[test]
fn revision_modal_hidden_page_focus_and_write_block_cancel_the_entire_gesture() {
    for blocker in [
        "revision",
        "modal",
        "hidden",
        "candidate_hidden",
        "focus",
        "readonly",
    ] {
        let (mut app, window, _) = drag_app();
        let original = app.world().resource::<MapBrowserState>().document.clone();
        match blocker {
            "revision" => {
                app.world_mut()
                    .resource_mut::<MapBrowserState>()
                    .document_revision += 1
            }
            "modal" => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .new_map_dialog_open = true
            }
            "hidden" => {
                app.world_mut().resource_mut::<MapEditorSidebarState>().navigator =
                    EditorNavigatorTab::Scene
            }
            "candidate_hidden" => {
                app.world_mut().resource_mut::<MapBrowserDragState>().active = false;
                app.world_mut()
                    .resource_mut::<MapBrowserDragState>()
                    .blocked = false;
                app.world_mut().resource_mut::<MapEditorSidebarState>().navigator =
                    EditorNavigatorTab::Scene;
            }
            "focus" => app.world_mut().get_mut::<Window>(window).unwrap().focused = false,
            "readonly" => {
                app.world_mut()
                    .resource_mut::<MapBrowserState>()
                    .write_blocked = Some("测试禁写".into())
            }
            _ => unreachable!(),
        }
        app.update();
        let drag = app.world().resource::<MapBrowserDragState>();
        assert!(
            drag.press.is_none() && !drag.active && drag.blocked,
            "{blocker}"
        );
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.update();
        assert!(
            !app.world().resource::<MapBrowserDragState>().blocked,
            "{blocker}"
        );
        assert_eq!(app.world().resource::<MapBrowserState>().document, original);
        assert!(!app.world().resource::<MapFolderPersistence>().busy());
    }
}
