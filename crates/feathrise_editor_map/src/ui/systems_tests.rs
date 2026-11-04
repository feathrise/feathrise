//! F12壳层纯输入与滚动换算回归。

use super::*;
use crate::game_authoring::map_interaction::DomainHorizontalScroll;
use crate::resources::MapBrowserDragState; // 拖拽门已移入模态参数组，沿用夹具显式导入自己的原资源。

#[test]
fn editor_shell_refresh_preserves_another_text_fields_ime() {
    let mut app = App::new(); // 直接执行真实壳体系统，覆盖菜单与 F12 同时安装的场景。
    app.init_resource::<MapEditorState>(); // F12 默认关闭，但壳体仍需响应后台目录变化。
    app.init_resource::<EditorAssetCatalog>(); // 模拟启动目录就绪触发壳体回显。
    app.init_resource::<Assets<MapCatalogAsset>>(); // 标题查询所需的空资源，无需启动 AssetServer。
    app.init_resource::<Assets<MapManifestAsset>>(); // 旧地图兼容查询的空资源。
    app.insert_resource(State::new(ScenePhase::Idle)); // 用户在新游戏姓名页，尚未进入地图。
    let window = app
        .world_mut()
        .spawn((
            Window {
                ime_enabled: true,
                ..default()
            }, // 模拟菜单本帧刚为姓名框开启 IME。
            PrimaryWindow, // 与游戏运行时一样使用唯一主窗口。
        ))
        .id(); // 后续精确检查该窗口，而不是测试一份复制状态。
    app.add_systems(Update, sync_editor_shell); // 壳体回显不能撤销另一个输入域的窗口请求。
    app.update(); // 首次目录回显必须保留主菜单的中文输入。
    assert!(app.world().get::<Window>(window).unwrap().ime_enabled);
    app.world_mut().resource_mut::<MapEditorState>().open = true; // F12 打开后其他文本框也可能拥有焦点。
    app.update(); // 新地图模态关闭时，壳体也不能抢占角色预设等字段的 IME。
    assert!(app.world().get::<Window>(window).unwrap().ime_enabled);
}

#[test]
fn left_panel_scroll_uses_down_as_positive_for_wheel_and_touchpad() {
    assert_eq!(
        editor_scroll_delta(MouseScrollUnit::Line, -1.0),
        EDITOR_SCROLL_LINE_HEIGHT
    );
    assert_eq!(editor_scroll_delta(MouseScrollUnit::Pixel, -12.0), 12.0);
}

#[test]
fn editor_scroll_system_keeps_tree_tabs_and_panels_disjoint() {
    use bevy::ecs::system::{IntoSystem, System};

    let mut world = World::new();
    let mut system = IntoSystem::into_system(scroll_editor_side_panels);
    system.initialize(&mut world); // 三个可变ScrollPosition查询必须在真实系统元数据中保持互斥。
}

#[test]
fn domain_status_scroll_shows_full_summary_without_scrolling_the_form() {
    let mut app = App::new();
    app.add_message::<MouseWheel>()
        .insert_resource(MapEditorState { open: true, ..default() })
        .init_resource::<MapBrowserDragState>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .add_systems(Update, scroll_editor_side_panels);
    let cursor = RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::splat(0.5)) };
    let form = app.world_mut().spawn((
        MapEditorPresetWorkspaceScroll,
        cursor,
        ScrollPosition(Vec2::new(0.0, 70.0)),
        ComputedNode { size: Vec2::new(300.0, 200.0), content_size: Vec2::new(300.0, 900.0), ..default() },
    )).id();
    let status = app.world_mut().spawn((
        DomainHorizontalScroll,
        cursor,
        ScrollPosition::default(),
        ComputedNode { size: Vec2::new(300.0, 20.0), content_size: Vec2::new(900.0, 20.0), ..default() },
        ChildOf(form),
    )).id();
    for (distance, expected) in [(27.0, 27.0), (10_000.0, 600.0), (-10_000.0, 0.0)] {
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Pixel, x: 0.0, y: -distance,
            window: Entity::PLACEHOLDER, phase: bevy::input::touch::TouchPhase::Moved,
        });
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(status).unwrap().0, Vec2::new(expected, 0.0));
        assert_eq!(app.world().get::<ScrollPosition>(form).unwrap().y, 70.0);
    }
}
