//! 复用原生UI命中与真正GPU绘制，文件只写隔离的临时项目。

use super::{components::*, constants::*, resources::AssetShortcuts, types::Section};
use crate::{
    components::MapEditorAssetRow,
    resources::{EditorAssetCatalog, MapEditorState},
    ui::asset_context_menu,
};
use bevy::{ecs::system::RunSystemOnce, prelude::*, ui::RelativeCursorPosition};

fn frames(app: &mut App, count: usize) {
    for _ in 0..count {
        app.update();
    }
}
fn settle(app: &mut App) {
    for _ in 0..1000 {
        app.update();
        let state = app.world().resource::<AssetShortcuts>();
        let catalog = app.world().resource::<EditorAssetCatalog>();
        if state.loaded
            && state.task.is_none()
            && state.revision == state.saved_revision
            && catalog.requested_revision == catalog.applied_revision
        {
            frames(app, 4);
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("收藏/定位任务未完成");
}

fn click(
    app: &mut App,
    window: Entity,
    camera: Entity,
    entity: Entity,
    button: MouseButton,
    menu: bool,
) {
    let position = app
        .world()
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .affine()
        .translation;
    let target = app
        .world()
        .get::<bevy::camera::RenderTarget>(camera)
        .unwrap()
        .clone();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_physical_cursor_position(Some(position.as_dvec2()));
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = bevy::camera::RenderTarget::Window(bevy::window::WindowRef::Entity(window));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(button);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    assert_eq!(
        *app.world().get::<Interaction>(entity).unwrap(),
        if button == MouseButton::Right {
            Interaction::Hovered
        } else {
            Interaction::Pressed
        },
        "真实鼠标未命中导航控件"
    );
    if menu {
        asset_context_menu::control_menu_for_tests(app.world_mut());
    }
    if button == MouseButton::Left {
        app.world_mut()
            .run_system_once(super::systems::control)
            .unwrap();
        app.world_mut()
            .resource_mut::<Messages<super::SetAssetFavorite>>()
            .clear(); // 本夹具已运行正式采用点，不能再让另一个MessageReader重放同一点击。
    }
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(button);
    app.world_mut()
        .run_system_cached(bevy::ui::ui_focus_system)
        .unwrap();
    app.world_mut().run_system_once(feathrise_editor_ui::update_editor_button_clicks).unwrap();
    if menu { asset_context_menu::control_menu_for_tests(app.world_mut()); }
    if button == MouseButton::Left {
        app.world_mut().run_system_once(super::systems::control).unwrap();
        app.world_mut().resource_mut::<Messages<super::SetAssetFavorite>>().clear();
    }
    *app.world_mut()
        .get_mut::<bevy::camera::RenderTarget>(camera)
        .unwrap() = target;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    frames(app, 4);
}
fn row(app: &mut App, index: usize) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &MapEditorAssetRow)>()
        .iter(world)
        .find(|(_, row)| row.entry_index == Some(index))
        .unwrap()
        .0
}
fn action(
    app: &mut App,
    predicate: impl Fn(&ShortcutAction, Option<&ShortcutBinding>) -> bool,
) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &ShortcutAction, Option<&ShortcutBinding>)>()
        .iter(world)
        .find(|(_, action, binding)| predicate(action, *binding))
        .unwrap()
        .0
}
fn favorite(app: &mut App, window: Entity, camera: Entity, index: usize) {
    let entry = row(app, index);
    click(app, window, camera, entry, MouseButton::Right, true);
    let button = {
        let world = app.world_mut();
        world
            .query_filtered::<Entity, With<asset_context_menu::AssetMenuFavoriteButton>>()
            .single(world)
            .unwrap()
    };
    let children = app.world().get::<Children>(button).unwrap();
    assert_eq!(app.world().get::<Text>(children[0]).unwrap().0, "收藏");
    click(app, window, camera, button, MouseButton::Left, true);
    settle(app);
    assert_eq!(
        app.world()
            .resource::<AssetShortcuts>()
            .snapshot
            .favorites
            .len(),
        index + 1
    );
}

pub(crate) fn assert_gpu(
    app: &mut App,
    window: Entity,
    camera: Entity,
    target: Handle<Image>,
    width: u32,
    path: &str,
    snapshot: fn(&mut App, Handle<Image>, &str, u32),
) {
    settle(app);
    app.world_mut()
        .resource_mut::<EditorAssetCatalog>()
        .entries
        .push(crate::types::EditorAssetEntry {
            asset_path: "常用目录".into(),
            display_name: "常用目录".into(),
            is_directory: true,
            kind: feathrise_map_content::MapAssetKind::Unsupported,
        });
    frames(app, 4);
    let selected = app
        .world()
        .resource::<MapEditorState>()
        .selected_asset
        .clone();
    favorite(app, window, camera, 0);
    favorite(app, window, camera, 1);
    assert_eq!(
        app.world().resource::<MapEditorState>().selected_asset,
        selected,
        "收藏不能连带选择资产"
    );
    let list = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ShortcutList)>()
            .iter(world)
            .find(|(_, list)| list.0 == Section::Favorites)
            .unwrap()
            .0
    };
    assert_eq!(
        app.world().get::<ComputedNode>(list).unwrap().size().y,
        ROW_HEIGHT * 2.0 + ROW_GAP
    );
    snapshot(
        app,
        target.clone(),
        "asset-browser-favorites-and-recent.png",
        width,
    );
    let toggle = action(app, |action, _| {
        matches!(action, ShortcutAction::Collapse(Section::Favorites))
    });
    click(app, window, camera, toggle, MouseButton::Left, false);
    assert_eq!(
        app.world().get::<Node>(list).unwrap().display,
        Display::None
    );
    click(app, window, camera, toggle, MouseButton::Left, false);
    let directory = action(app, |action, binding| {
        matches!(action, ShortcutAction::Locate(Section::Favorites))
            && binding
                .is_some_and(|binding| binding.0.as_ref().is_some_and(|target| target.is_directory))
    });
    click(app, window, camera, directory, MouseButton::Left, false);
    settle(app);
    assert_eq!(
        app.world().resource::<EditorAssetCatalog>().current_folder,
        "常用目录"
    );
    assert_eq!(
        app.world().resource::<MapEditorState>().selected_asset,
        selected
    );
    assert_eq!(
        app.world().resource::<AssetShortcuts>().snapshot.recent[0]
            .target
            .path,
        "assets/常用目录"
    );
    let file = action(app, |action, binding| {
        matches!(action, ShortcutAction::Locate(Section::Favorites))
            && binding.is_some_and(|binding| {
                binding
                    .0
                    .as_ref()
                    .is_some_and(|target| !target.is_directory)
            })
    });
    click(app, window, camera, file, MouseButton::Left, false);
    settle(app);
    assert_eq!(
        app.world().resource::<EditorAssetCatalog>().current_folder,
        ""
    );
    assert_eq!(
        app.world().resource::<AssetShortcuts>().snapshot.recent[0]
            .target
            .path,
        format!("assets/{path}")
    );
    let toggle_recent = action(app, |action, _| {
        matches!(action, ShortcutAction::Collapse(Section::Recent))
    });
    click(app, window, camera, toggle_recent, MouseButton::Left, false);
    snapshot(
        app,
        target.clone(),
        "asset-browser-favorites-recent-expanded.png",
        width,
    );

    // 实际删除临时资产后校验、显示失效并只移除记录；不调用作者保存。
    let root = app.world().resource::<AssetShortcuts>().root.clone();
    std::fs::remove_file(root.join(path)).unwrap();
    let refresh = action(app, |action, _| matches!(action, ShortcutAction::Refresh));
    click(app, window, camera, refresh, MouseButton::Left, false);
    settle(app);
    assert!(!app.world().resource::<AssetShortcuts>().snapshot.favorites[0].available);
    snapshot(
        app,
        target.clone(),
        "asset-browser-favorites-missing.png",
        width,
    );
    let remove = action(app, |action, binding| {
        matches!(action, ShortcutAction::Remove(Section::Favorites))
            && binding.is_some_and(|binding| {
                binding
                    .0
                    .as_ref()
                    .is_some_and(|target| !target.is_directory)
            })
    });
    click(app, window, camera, remove, MouseButton::Left, false);
    settle(app);
    assert_eq!(
        app.world().get::<ComputedNode>(list).unwrap().size().y,
        ROW_HEIGHT
    );

    for index in 0..20 {
        app.world_mut().write_message(super::AssetVisited {
            path: format!("常用目录/文件{index:02}.txt"),
            is_directory: false,
        });
    }
    settle(app);
    let recent = {
        let world = app.world_mut();
        world
            .query::<(Entity, &ShortcutList)>()
            .iter(world)
            .find(|(_, list)| list.0 == Section::Recent)
            .unwrap()
            .0
    };
    assert_eq!(
        app.world().get::<ComputedNode>(recent).unwrap().size().y,
        LIST_HEIGHT
    );
    {
        let world = app.world_mut();
        for mut cursor in world
            .query_filtered::<&mut RelativeCursorPosition, With<ShortcutList>>()
            .iter_mut(world)
        {
            cursor.cursor_over = false; // 模拟唯一真正悬停视区，不能同时伪造两个局部列表命中。
        }
        world
            .get_mut::<RelativeCursorPosition>(recent)
            .unwrap()
            .cursor_over = true;
    }
    app.world_mut()
        .write_message(bevy::input::mouse::MouseWheel {
            unit: bevy::input::mouse::MouseScrollUnit::Pixel,
            x: 0.0,
            y: -50.0,
            window,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    app.world_mut()
        .run_system_once(crate::ui::systems::scroll_editor_side_panels)
        .unwrap();
    assert!(
        app.world().get::<ScrollPosition>(recent).unwrap().y > 0.0,
        "多条记录必须能局部滚动"
    );
    snapshot(app, target, "asset-browser-recent-scrolling.png", width);
    assert!(!app.world().resource::<MapEditorState>().dirty);
}
