//! 拖拽导航的真实状态机、真实时钟与缩放后布局回归；只模拟布局输出，不写作者目录。

use super::*;
use crate::ui::sidebars::EditorNavigatorTab;
use crate::components::MapEditorLeftPanelScroll;
use crate::map_browser::update_map_browser_drag;
use crate::resources::{MapCreationTask, MapDeletionTask, MapFolderPersistence};
use crate::types::{
    MapBrowserDragPress, MapBrowserIndexSignature, MapFolderDocument, MapFolderRecord,
};
use bevy::math::Affine2;
use feathrise_map_content::MapId;
use std::time::Duration;

fn fixture(scale: f32) -> (App, Entity, Entity, Entity, Entity) {
    let mut app = App::new();
    let source = MapBrowserItem::Map(MapId::parse("MAP-1").unwrap());
    let signature = MapBrowserIndexSignature {
        folder_revision: 7,
        catalog_revision: 0,
    };
    app.init_resource::<Time<Real>>()
        .init_resource::<Time<Virtual>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<MapFolderPersistence>()
        .init_resource::<MapCreationTask>()
        .init_resource::<MapDeletionTask>()
        .init_resource::<MapEditorSidebarState>()
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        })
        .insert_resource(MapBrowserState {
            document: MapFolderDocument {
                folders: ["FOLDER-A", "FOLDER-B"]
                    .map(|id| MapFolderRecord {
                        id: id.into(),
                        display_name: id.into(),
                        parent_id: None,
                    })
                    .to_vec(),
                ..default()
            },
            initial_load_resolved: true,
            write_blocked: None,
            document_revision: 7,
            selected_item: Some(source.clone()),
            ..default()
        })
        .insert_resource(MapBrowserIndexState {
            applied_signature: Some(signature.clone()),
            desired_signature: Some(signature.clone()),
            applied_projection: Some((signature.clone(), 0)),
            desired_projection: Some((signature, 0)),
            ..default()
        })
        .insert_resource(MapBrowserDragState {
            press: Some(MapBrowserDragPress {
                item: source,
                document_revision: 7,
                display_name: "拖动地图".into(),
                start_physical: Vec2::ZERO,
            }),
            active: true,
            blocked: true,
            ..default()
        })
        .add_systems(
            Update,
            (update_map_browser_drag, navigate_map_browser_drag).chain(),
        );
    app.world_mut().resource_mut::<Time<Virtual>>().pause(); // 玩法暂停时，悬停与滚动仍应按真实时间推进。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    let mut window = Window::default();
    window.resolution.set_physical_resolution(4096, 4096);
    window.set_physical_cursor_position(Some((Vec2::new(130.0, 200.0) * scale).into()));
    let window = app.world_mut().spawn((window, PrimaryWindow)).id();
    let tree = app
        .world_mut()
        .spawn((
            MapEditorMapTreeScroll,
            Node::default(),
            ComputedNode {
                size: Vec2::new(220.0, 200.0) * scale,
                content_size: Vec2::new(220.0, 900.0) * scale,
                inverse_scale_factor: scale.recip(),
                ..default()
            },
            UiGlobalTransform::from(Affine2::from_translation(Vec2::new(130.0, 200.0) * scale)),
            InheritedVisibility::VISIBLE,
            RelativeCursorPosition {
                cursor_over: true,
                ..default()
            },
            ScrollPosition(Vec2::new(0.0, 250.0)),
        ))
        .id();
    let outer = app
        .world_mut()
        .spawn((
            MapEditorLeftPanelScroll,
            ScrollPosition(Vec2::new(0.0, 49.0)),
        ))
        .id();
    let row = app
        .world_mut()
        .spawn((
            MapEditorMapRow {
                slot: 0,
                item: Some(MapBrowserItem::Folder("FOLDER-A".into())),
            },
            Interaction::Hovered,
            RelativeCursorPosition {
                cursor_over: true,
                ..default()
            },
        ))
        .id();
    tick(&mut app, 0.0); // 首次命中开始计时，不能把进入前的真实时间算进悬停。
    assert!(app.world().resource::<MapBrowserDragState>().active);
    (app, window, tree, row, outer)
}

fn tick(app: &mut App, seconds: f64) {
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_secs_f64(seconds));
    app.update();
}

fn cursor(app: &mut App, window: Entity, position: Vec2) {
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(position.into()));
}

fn expanded(app: &App, id: &str) -> bool {
    app.world()
        .resource::<MapBrowserState>()
        .expanded_folder_ids
        .contains(id)
}

fn assert_no_author_changes(app: &App, original: &MapFolderDocument, outer: Entity) {
    let browser = app.world().resource::<MapBrowserState>();
    assert_eq!(&browser.document, original);
    assert_eq!(browser.document_revision, 7);
    assert_eq!(
        browser.selected_item,
        Some(MapBrowserItem::Map(MapId::parse("MAP-1").unwrap()))
    );
    assert!(!app.world().resource::<MapFolderPersistence>().busy());
    assert_eq!(app.world().get::<ScrollPosition>(outer).unwrap().y, 49.0);
}

#[test]
fn hover_uses_half_a_real_second_and_only_changes_session_expansion_once() {
    for folder_source in [false, true] {
        let (mut app, _, _, _, outer) = fixture(1.0);
        if folder_source {
            app.world_mut()
                .resource_mut::<MapBrowserDragState>()
                .press
                .as_mut()
                .unwrap()
                .item = MapBrowserItem::Folder("FOLDER-B".into());
            tick(&mut app, 0.0); // 换来源必须重新开始当前目录的计时。
        }
        let original = app.world().resource::<MapBrowserState>().document.clone();
        tick(&mut app, 0.49);
        assert!(!expanded(&app, "FOLDER-A"));
        tick(&mut app, 0.02);
        assert!(expanded(&app, "FOLDER-A"));
        assert_eq!(
            app.world().resource::<MapBrowserState>().expansion_revision,
            1
        );
        tick(&mut app, 1.0);
        assert_eq!(
            app.world().resource::<MapBrowserState>().expansion_revision,
            1
        );
        assert!(app.world().resource::<MapBrowserDragState>().active);
        assert_no_author_changes(&app, &original, outer);
        assert_eq!(app.world().resource::<Time<Virtual>>().elapsed_secs(), 0.0);
    }
}

#[test]
fn real_shared_guard_keeps_drag_navigation_running_and_modal_still_stops_it() {
    let (mut app, window, tree, row, outer) = fixture(1.0);
    app.init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .add_systems(
            Update,
            crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard
                .after(navigate_map_browser_drag),
        );
    tick(&mut app, 0.0);
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    tick(&mut app, 0.51);
    assert!(
        expanded(&app, "FOLDER-A"),
        "自己的共享鼠标门不能阻断半秒悬停"
    );
    {
        let mut index = app.world_mut().resource_mut::<MapBrowserIndexState>();
        let projection = (
            MapBrowserIndexSignature {
                folder_revision: 7,
                catalog_revision: 0,
            },
            1,
        );
        index.applied_projection = Some(projection.clone());
        index.desired_projection = Some(projection);
    }
    cursor(&mut app, window, Vec2::new(130.0, 299.0));
    tick(&mut app, 0.02);
    let scroll = app.world().get::<ScrollPosition>(tree).unwrap().y;
    assert!(scroll > 250.0, "共享门已持有时，连续拖拽仍应能自动滚动");
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    assert_eq!(app.world().get::<ScrollPosition>(outer).unwrap().y, 49.0);
    app.insert_resource(MapEditorSettingsState {
        open: true,
        ..default()
    });
    tick(&mut app, 0.2);
    assert_eq!(app.world().get::<ScrollPosition>(tree).unwrap().y, scroll);
    app.world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .open = false;
    let field = app.world_mut().spawn_empty().id();
    app.world_mut()
        .resource_mut::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .entity = Some(field);
    app.world_mut()
        .get_mut::<MapEditorMapRow>(row)
        .unwrap()
        .item = Some(MapBrowserItem::Folder("FOLDER-B".into()));
    cursor(&mut app, window, Vec2::new(130.0, 200.0));
    tick(&mut app, 0.6);
    assert!(
        !expanded(&app, "FOLDER-B"),
        "外部数值拖动仍需阻止导航，不能把全部共享门都放过"
    );
}

#[test]
fn leaving_and_rebinding_the_same_row_restart_hover_instead_of_opening_old_identity() {
    let (mut app, window, _, row, _) = fixture(1.0);
    tick(&mut app, 0.4);
    cursor(&mut app, window, Vec2::new(500.0, 200.0));
    tick(&mut app, 2.0);
    assert!(!expanded(&app, "FOLDER-A"));
    cursor(&mut app, window, Vec2::new(130.0, 200.0));
    tick(&mut app, 0.0);
    tick(&mut app, 0.4);
    app.world_mut()
        .get_mut::<MapEditorMapRow>(row)
        .unwrap()
        .item = Some(MapBrowserItem::Folder("FOLDER-B".into()));
    tick(&mut app, 0.2);
    tick(&mut app, 0.49);
    assert!(!expanded(&app, "FOLDER-A") && !expanded(&app, "FOLDER-B"));
    tick(&mut app, 0.02);
    assert!(expanded(&app, "FOLDER-B") && !expanded(&app, "FOLDER-A"));
}

#[test]
fn scrolling_uses_logical_distance_accelerates_and_clamps_without_moving_outer_page() {
    let mut reference: Option<f32> = None;
    for (ui, dpi) in [
        (0.6, 1.0),
        (1.0, 1.0),
        (1.5, 1.0),
        (0.6, 2.0),
        (1.25, 1.25),
        (1.5, 2.0),
    ] {
        let scale = ui * dpi;
        let (mut app, window, tree, _, outer) = fixture(scale);
        app.world_mut()
            .get_mut::<Window>(window)
            .unwrap()
            .resolution
            .set_scale_factor_override(Some(dpi));
        let original = app.world().resource::<MapBrowserState>().document.clone();
        cursor(&mut app, window, Vec2::new(130.0, 290.0) * scale);
        tick(&mut app, 0.02);
        let distance = app.world().get::<ScrollPosition>(tree).unwrap().y - 250.0;
        assert!(distance > 4.0 && distance < 5.0);
        if let Some(reference) = reference {
            assert!((distance - reference).abs() < 0.001);
        }
        reference = Some(distance);
        assert!(
            app.world()
                .resource::<MapBrowserDragState>()
                .drop_target
                .is_none()
        );
        cursor(&mut app, window, Vec2::new(130.0, 275.0) * scale);
        tick(&mut app, 0.02);
        let slower = app.world().get::<ScrollPosition>(tree).unwrap().y - 250.0 - distance;
        assert!(slower > 0.0 && slower < distance);
        cursor(&mut app, window, Vec2::new(130.0, 110.0) * scale);
        tick(&mut app, 0.02);
        assert!(
            (app.world().get::<ScrollPosition>(tree).unwrap().y - (250.0 + slower)).abs() < 0.001
        );
        app.world_mut().get_mut::<ScrollPosition>(tree).unwrap().y = 699.5;
        cursor(&mut app, window, Vec2::new(130.0, 299.0) * scale);
        tick(&mut app, 3.0);
        assert!((app.world().get::<ScrollPosition>(tree).unwrap().y - 700.0).abs() < 0.001);
        app.world_mut().get_mut::<ScrollPosition>(tree).unwrap().y = 250.0;
        tick(&mut app, 3.0);
        assert!(app.world().get::<ScrollPosition>(tree).unwrap().y <= 268.01); // 卡顿不能一次跨过整屏目标。
        app.world_mut().get_mut::<ScrollPosition>(tree).unwrap().y = 0.1;
        cursor(&mut app, window, Vec2::new(130.0, 101.0) * scale);
        tick(&mut app, 0.02);
        assert_eq!(app.world().get::<ScrollPosition>(tree).unwrap().y, 0.0);
        assert_no_author_changes(&app, &original, outer);
    }
}

#[test]
fn clipped_visible_edges_scroll_and_content_motion_restarts_hover() {
    let (mut app, window, tree, row, _) = fixture(1.0);
    tick(&mut app, 0.4);
    app.world_mut().entity_mut(tree).insert(CalculatedClip {
        clip: Rect::new(20.0, 100.0, 240.0, 250.0),
    });
    cursor(&mut app, window, Vec2::new(130.0, 260.0));
    tick(&mut app, 0.2);
    assert_eq!(app.world().get::<ScrollPosition>(tree).unwrap().y, 250.0);
    assert!(!expanded(&app, "FOLDER-A"));
    cursor(&mut app, window, Vec2::new(130.0, 240.0));
    tick(&mut app, 0.02);
    assert!(app.world().get::<ScrollPosition>(tree).unwrap().y > 250.0);
    app.world_mut()
        .get_mut::<MapEditorMapRow>(row)
        .unwrap()
        .item = Some(MapBrowserItem::Folder("FOLDER-B".into()));
    cursor(&mut app, window, Vec2::new(130.0, 200.0));
    tick(&mut app, 0.0);
    tick(&mut app, 0.49);
    assert!(!expanded(&app, "FOLDER-A") && !expanded(&app, "FOLDER-B"));
    tick(&mut app, 0.02);
    assert!(expanded(&app, "FOLDER-B"));
}

#[test]
fn release_focus_hidden_modal_revision_and_pending_projection_stop_navigation() {
    for blocker in [
        "release",
        "focus",
        "hidden",
        "modal",
        "revision",
        "projection",
        "settings",
        "invisible",
        "center",
        "short",
        "outside",
    ] {
        let (mut app, window, tree, row, outer) = fixture(1.0);
        let original = app.world().resource::<MapBrowserState>().document.clone();
        tick(&mut app, 0.4);
        cursor(&mut app, window, Vec2::new(130.0, 290.0));
        match blocker {
            "release" => {
                *app.world_mut().get_mut::<Interaction>(row).unwrap() = Interaction::None;
                app.world_mut()
                    .resource_mut::<ButtonInput<MouseButton>>()
                    .release(MouseButton::Left);
            }
            "focus" => app.world_mut().get_mut::<Window>(window).unwrap().focused = false,
            "hidden" => {
                app.world_mut().resource_mut::<MapEditorSidebarState>().navigator =
                    EditorNavigatorTab::Scene
            }
            "modal" => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .new_map_dialog_open = true
            }
            "revision" => {
                app.world_mut()
                    .resource_mut::<MapBrowserState>()
                    .document_revision += 1
            }
            "projection" => {
                app.world_mut()
                    .resource_mut::<MapBrowserIndexState>()
                    .rebuilding = true
            }
            "settings" => {
                app.insert_resource(MapEditorSettingsState {
                    open: true,
                    ..default()
                });
            }
            "invisible" => {
                app.world_mut()
                    .entity_mut(tree)
                    .insert(InheritedVisibility::HIDDEN);
            }
            "center" => cursor(&mut app, window, Vec2::new(130.0, 200.0)),
            "short" => {
                app.world_mut()
                    .get_mut::<ComputedNode>(tree)
                    .unwrap()
                    .content_size
                    .y = 150.0;
                app.world_mut().get_mut::<ScrollPosition>(tree).unwrap().y = 0.0;
            }
            "outside" => cursor(&mut app, window, Vec2::new(250.0, 290.0)),
            _ => unreachable!(),
        }
        tick(&mut app, 0.2);
        assert_eq!(
            app.world().get::<ScrollPosition>(tree).unwrap().y,
            if blocker == "short" { 0.0 } else { 250.0 },
            "{blocker}"
        );
        if blocker != "center" && blocker != "short" {
            assert!(!expanded(&app, "FOLDER-A"), "{blocker}");
        }
        assert!(!app.world().resource::<MapFolderPersistence>().busy());
        assert_eq!(app.world().resource::<MapBrowserState>().document, original);
        assert_eq!(app.world().get::<ScrollPosition>(outer).unwrap().y, 49.0);
    }
}

#[test]
fn pending_projection_discards_hover_time_and_invalid_drop_can_expand_for_navigation() {
    let (mut app, _, _, _, _) = fixture(1.0);
    tick(&mut app, 0.4);
    app.world_mut()
        .resource_mut::<MapBrowserIndexState>()
        .rebuilding = true;
    tick(&mut app, 4.0);
    app.world_mut()
        .resource_mut::<MapBrowserIndexState>()
        .rebuilding = false;
    tick(&mut app, 0.0);
    tick(&mut app, 0.49);
    assert!(!expanded(&app, "FOLDER-A"));
    tick(&mut app, 0.02);
    assert!(expanded(&app, "FOLDER-A"));

    let (mut app, _, _, _, _) = fixture(1.0);
    app.world_mut()
        .resource_mut::<MapBrowserDragState>()
        .press
        .as_mut()
        .unwrap()
        .item = MapBrowserItem::Folder("FOLDER-A".into());
    tick(&mut app, 0.0);
    tick(&mut app, 0.51);
    assert!(expanded(&app, "FOLDER-A"));
    assert!(
        app.world()
            .resource::<MapBrowserDragState>()
            .drop_target
            .is_none()
    );
    assert!(
        app.world()
            .resource::<MapBrowserDragState>()
            .drop_error
            .as_ref()
            .unwrap()
            .contains("自身或子文件夹")
    );
    assert!(!app.world().resource::<MapFolderPersistence>().busy());
}
