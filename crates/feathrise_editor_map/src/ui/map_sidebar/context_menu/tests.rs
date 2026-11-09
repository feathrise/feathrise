//! 目录菜单运行真实右键、原文件夹事务入口与鼠标门；不访问地图文件或启动原生窗口。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::super::components::{MapSidebarActionBinding, MapSidebarActionLabel};
use super::super::tests::{entry, initialize, map_item, published_index};
use super::{MapDirectoryMenuState, components::*, systems};
use crate::{components::*, resources::*, types::*};
use bevy::{
    input_focus::InputFocus, math::DVec2, prelude::*, time::Real, ui::RelativeCursorPosition,
    window::PrimaryWindow,
};

struct Fixture {
    app: App,
    row: Entity,
    root: Entity,
    popup: Entity,
    backdrop: Entity,
}

fn fixture(item: Option<MapBrowserItem>) -> Fixture {
    let mut app = App::new();
    app.add_plugins(feathrise_editor_ui::EditorButtonInputPlugin);
    initialize(&mut app);
    app.init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<Time<Real>>()
        .init_resource::<InputFocus>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .init_resource::<MapDirectoryMenuState>()
        .add_systems(
            Update,
            (
                systems::control_menu,
                crate::map_browser::handle_map_browser_input,
                crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
                super::super::systems::sync_sidebar,
                super::scene::sync,
            )
                .chain(),
        );
    let mut window = Window {
        focused: true,
        ..default()
    };
    window.set_physical_cursor_position(Some(DVec2::new(400.0, 300.0)));
    app.world_mut().spawn((PrimaryWindow, window));
    app.world_mut().spawn((
        MapEditorRoot,
        ComputedNode {
            size: Vec2::new(1280.0, 720.0),
            inverse_scale_factor: 1.0,
            ..default()
        },
        UiGlobalTransform::from_xy(640.0, 360.0),
    ));
    let row = app
        .world_mut()
        .spawn((
            MapEditorMapRow {
                slot: 0,
                item: item.clone(),
            },
            Node::default(),
            if matches!(
                item,
                Some(MapBrowserItem::Map(_) | MapBrowserItem::Folder(_))
            ) {
                UiInteraction::Hovered
            } else {
                UiInteraction::None
            },
        ))
        .id();
    let root = app
        .world_mut()
        .spawn((
            MapEditorMapRootRow,
            Node::default(),
            if item == Some(MapBrowserItem::Root) {
                UiInteraction::Hovered
            } else {
                UiInteraction::None
            },
        ))
        .id();
    app.world_mut().spawn((
        MapEditorMapTreeScroll,
        ScrollPosition::default(),
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::splat(0.5)),
            ..default()
        },
    ));
    if let Some(item) = item.filter(|item| *item != MapBrowserItem::Root) {
        app.insert_resource(published_index(vec![entry(item.clone(), "右键目标")]));
        if let MapBrowserItem::Folder(id) = item {
            app.world_mut()
                .resource_mut::<MapBrowserState>()
                .document
                .folders
                .push(MapFolderRecord {
                    id,
                    display_name: "右键目标".into(),
                    parent_id: None,
                });
        }
    }
    for action in [
        EditorAction::NewMap,
        EditorAction::MapFolderCreate,
        EditorAction::EnterSelectedMap,
        EditorAction::MapFolderRename,
        EditorAction::DeleteMap,
    ] {
        app.world_mut().spawn((
            InteractionButton,
            InheritedVisibility::VISIBLE,
            ComputedNode { size: Vec2::new(120.0, 24.0), ..default() },
            RelativeCursorPosition { cursor_over: true, ..default() },
            MapEditorActionButton(action),
            MapSidebarActionBinding {
                menu_owned: true,
                ..default()
            },
            UiInteraction::None,
            Node::default(),
            BackgroundColor::default(),
            BorderColor::default(),
        ));
        app.world_mut().spawn((
            MapSidebarActionLabel(action),
            Text::default(),
            TextColor::default(),
        ));
    }
    let popup = app
        .world_mut()
        .spawn((DirectoryMenuPopup, Node::default()))
        .id();
    let backdrop = app
        .world_mut()
        .spawn((DirectoryMenuBackdrop, Node::default()))
        .id();
    let mut fixture = Fixture {
        app,
        row,
        root,
        popup,
        backdrop,
    };
    step(&mut fixture.app);
    fixture
}

fn step(app: &mut App) {
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
}

fn open(test: &mut Fixture) {
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    step(&mut test.app);
    assert!(
        test.app
            .world()
            .resource::<MapDirectoryMenuState>()
            .target
            .is_some()
    );
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    step(&mut test.app);
}

fn action_entity(test: &mut Fixture, action: EditorAction) -> Entity {
    let world = test.app.world_mut();
    world
        .query::<(Entity, &MapEditorActionButton)>()
        .iter(world)
        .find(|(_, button)| button.0 == action)
        .unwrap()
        .0
}

fn click(test: &mut Fixture, action: EditorAction) {
    let button = action_entity(test, action);
    *test.app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut test.app);
}

fn finish_click(test: &mut Fixture) {
    let world = test.app.world_mut();
    for mut interaction in world.query_filtered::<&mut UiInteraction, With<MapEditorActionButton>>().iter_mut(world) {
        if *interaction == UiInteraction::Pressed { *interaction = UiInteraction::Hovered; }
    } // 无窗口夹具模拟官方命中回到原菜单项，再交共享协议确认这次释放。
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    for _ in 0..4 {
        step(&mut test.app);
    }
}

#[test]
fn right_click_selects_stable_context_without_expansion_or_entry_and_changes_menu_items() {
    for item in [
        Some(map_item("MAP-2")),
        Some(MapBrowserItem::Folder("FOLDER-1".into())),
        Some(MapBrowserItem::Root),
        None,
    ] {
        let mut test = fixture(item.clone());
        open(&mut test);
        let browser = test.app.world().resource::<MapBrowserState>();
        assert_eq!(browser.selected_item, item);
        assert!(browser.expanded_folder_ids.is_empty());
        assert_eq!(browser.expansion_revision, 0);
        assert!(
            test.app
                .world()
                .resource::<MapEditorState>()
                .pending_map_transition
                .is_none()
        );
        let expected = match item {
            Some(MapBrowserItem::Map(_)) => 5,
            Some(MapBrowserItem::Folder(_)) => 4,
            _ => 2,
        };
        let world = test.app.world_mut();
        assert_eq!(
            world
                .query::<(&MapSidebarActionBinding, &Node)>()
                .iter(world)
                .filter(|(_, node)| node.display == Display::Flex)
                .count(),
            expected
        );
        assert_eq!(
            world.get::<Node>(test.backdrop).unwrap().display,
            Display::Flex
        );
    }
}

#[test]
fn folder_action_uses_original_dialog_only_after_mouse_release_and_quiet_frame() {
    let folder = MapBrowserItem::Folder("FOLDER-1".into());
    let mut test = fixture(Some(folder));
    open(&mut test);
    click(&mut test, EditorAction::MapFolderCreate);
    assert!(
        test.app
            .world()
            .resource::<MapBrowserState>()
            .dialog
            .is_none()
    );
    assert!(
        test.app
            .world()
            .resource::<MapDirectoryMenuState>()
            .queued
            .is_none(),
        "菜单项按住期间不冻结可派发动作"
    );
    assert_eq!(
        test.app.world().get::<Node>(test.backdrop).unwrap().display,
        Display::Flex
    );
    step(&mut test.app);
    assert!(
        test.app
            .world()
            .resource::<MapBrowserState>()
            .dialog
            .is_none(),
        "按住关闭点击不产生事务"
    );
    finish_click(&mut test);
    let browser = test.app.world().resource::<MapBrowserState>();
    let dialog = browser.dialog.as_ref().expect("唯一原目录入口应打开原模态");
    assert_eq!(dialog.mode, MapFolderDialogMode::Create);
    assert_eq!(dialog.target_folder_id.as_deref(), Some("FOLDER-1"));
    assert_eq!(browser.document_revision, 1, "菜单和打开模态均不写目录数据");
}

#[test]
fn queued_click_is_cancelled_if_selection_or_source_changes_before_release() {
    for change_source in [false, true] {
        let mut test = fixture(Some(MapBrowserItem::Folder("FOLDER-1".into())));
        open(&mut test);
        click(&mut test, EditorAction::MapFolderRename);
        if change_source {
            test.app
                .world_mut()
                .resource_mut::<MapBrowserState>()
                .document_revision += 1;
        } else {
            test.app
                .world_mut()
                .resource_mut::<MapBrowserState>()
                .select_only(Some(MapBrowserItem::Root));
        }
        finish_click(&mut test);
        assert!(
            test.app
                .world()
                .resource::<MapBrowserState>()
                .dialog
                .is_none()
        );
        assert!(
            test.app
                .world()
                .resource::<MapDirectoryMenuState>()
                .queued
                .is_none()
        );
    }
}

#[test]
fn outside_click_escape_hidden_page_and_scrub_never_dispatch_old_buttons() {
    for mode in 0..4 {
        let mut test = fixture(Some(MapBrowserItem::Folder("FOLDER-1".into())));
        open(&mut test);
        match mode {
            0 => test
                .app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left),
            1 => test
                .app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape),
            2 => {
                test.app
                    .world_mut()
                    .resource_mut::<crate::ui::sidebars::MapEditorSidebarState>()
                    .navigator = crate::ui::sidebars::EditorNavigatorTab::Scene
            }
            _ => {
                test.app
                    .world_mut()
                    .resource_mut::<feathrise_editor_ui::ActiveNumericInputScrub>()
                    .entity = Some(test.root)
            }
        }
        step(&mut test.app);
        assert!(
            test.app
                .world()
                .resource::<MapDirectoryMenuState>()
                .target
                .is_none()
        );
        assert!(
            test.app
                .world()
                .resource::<MapDirectoryMenuState>()
                .queued
                .is_none()
        );
        assert!(
            test.app
                .world()
                .resource::<MapBrowserState>()
                .dialog
                .is_none()
        );
    }
}

#[test]
fn row_rebinding_cannot_redirect_an_open_menu_and_folder_children_disable_delete() {
    let mut test = fixture(Some(MapBrowserItem::Folder("FOLDER-1".into())));
    let child = MapBrowserEntry {
        depth: 2,
        parent_folder_id: Some("FOLDER-1".into()),
        ..entry(map_item("MAP-2"), "子地图")
    };
    {
        let mut index = test.app.world_mut().resource_mut::<MapBrowserIndexState>();
        std::sync::Arc::make_mut(&mut index.full_entries).push(child);
    }
    open(&mut test);
    let delete = action_entity(&mut test, EditorAction::DeleteMap);
    assert_eq!(
        test.app.world().get::<Node>(delete).unwrap().display,
        Display::None
    );
    test.app
        .world_mut()
        .get_mut::<MapEditorMapRow>(test.row)
        .unwrap()
        .item = Some(map_item("MAP-2"));
    click(&mut test, EditorAction::MapFolderRename);
    finish_click(&mut test);
    let browser = test.app.world().resource::<MapBrowserState>();
    assert_eq!(
        browser.dialog.as_ref().unwrap().target_folder_id.as_deref(),
        Some("FOLDER-1")
    );
}

#[test]
fn popup_is_inside_scaled_root_at_bottom_right_and_has_only_five_original_buttons() {
    let mut test = fixture(Some(map_item("MAP-2")));
    open(&mut test);
    test.app
        .world_mut()
        .resource_mut::<MapDirectoryMenuState>()
        .anchor = Vec2::new(1279.0, 719.0);
    step(&mut test.app);
    let node = test.app.world().get::<Node>(test.popup).unwrap();
    let (Val::Px(left), Val::Px(top), Val::Px(width), Val::Px(height)) =
        (node.left, node.top, node.width, node.height)
    else {
        panic!("菜单应使用根逻辑像素")
    };
    assert!(left >= 0.0 && top >= 0.0 && left + width <= 1280.0 && top + height <= 720.0);
    let world = test.app.world_mut();
    assert_eq!(
        world
            .query::<&MapSidebarActionBinding>()
            .iter(world)
            .count(),
        5
    );
}
