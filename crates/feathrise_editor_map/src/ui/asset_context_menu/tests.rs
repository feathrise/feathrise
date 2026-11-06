//! 菜单纯计算与真实ECS输入回归；从不读取或改写测试机器的系统剪贴板。

use bevy::math::DVec2; // Window物理光标测试使用双精度平台坐标。
use bevy::prelude::*; // 最小应用复用生产资源和系统。
use bevy::window::PrimaryWindow; // 仅创建窗口数据，不启动Winit。
use feathrise_map_content::{MapAssetKind, MapId}; // 夹具使用正式路径和地图身份类型。
use feathrise_editor_ui::ActiveNumericInputScrub; // 覆盖已有鼠标所有权。

use super::components::{AssetMenuBackdrop, AssetMenuCopyButton, AssetMenuPopup}; // 菜单专用标记。
use super::helpers::*; // 直接验证生产坐标与身份判定。
use super::messages::CopyAssetPath; // 只记录复制意图，绝不安装真实剪贴板写系统。
use super::resources::AssetContextMenuState; // 可观察菜单会话。
use super::types::{AssetMenuSource, AssetMenuTarget}; // 身份失效测试所用的冻结来源。
use super::types::AssetPathCopyKind;
use crate::authoring::TerrainExternalChangeState; // 默认关闭地形冲突模态。
use crate::components::{AssetRowBinding, MapEditorAssetRow, MapEditorRoot}; // 复用正式固定行池和根节点。
use crate::resources::{
    EditorAssetCatalog, MapBrowserDragState, MapBrowserState, MapDeletionTask,
    MapEditorNumericScrubMouseGuard, MapEditorState,
}; // 系统所需资源不依赖地图或磁盘。
use crate::types::EditorAssetEntry; // 普通模型、纹理和目录共享一条路径语义。
use crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard; // 真正的打开/关闭/松键帧隔离。
use crate::ui::rows::ASSET_ROW_COUNT; // 测试与生产固定资产行池使用同一页容量。
use crate::ui::sidebars::{EditorNavigatorTab, MapEditorSidebarState}; // 仅左侧资产页可打开菜单。

mod input; // 开关、点击、失效及点透回归。
mod project; // 工程根到assets之前的同一右键菜单及可见行身份回归。

/// 只收集业务复制消息，避免测试对操作系统剪贴板产生副作用。
#[derive(Resource, Default)]
struct CopiedPaths(Vec<String>, Vec<AssetPathCopyKind>); // 一次按下最多产生一条明确意图。

#[test]
fn external_asset_menu_uses_right_clicked_path_without_switching_selection_or_copying() {
    use crate::ui::external_files::OpenExternalFile;
    for (_, action) in crate::ui::external_files::EXTERNAL_MENU_ITEMS {
        let mut test = fixture("中文 空格/配置.json");
        test.app.insert_resource(crate::ui::external_files::ExternalEditors::for_tests(&crate::ui::external_files::ExternalEditor::ALL));
        let selected = test.app.world().resource::<MapEditorState>().selected_asset.clone();
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        step(&mut test.app);
        let frozen = test.app.world().resource::<AssetContextMenuState>().target.as_ref().unwrap().path.clone();
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
        step(&mut test.app);
        test.app.world_mut().spawn((super::components::AssetMenuExternalButton(action), feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed));
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut test.app);
        let requests: Vec<_> = test.app.world_mut().resource_mut::<Messages<OpenExternalFile>>().drain().collect();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, frozen);
        assert_eq!(requests[0].action, action);
        assert_eq!(test.app.world().resource::<MapEditorState>().selected_asset, selected);
        assert!(test.app.world().resource::<CopiedPaths>().0.is_empty());
    }
}

#[test]
fn external_asset_menu_rejects_old_pressed_items_after_an_editor_is_removed() {
    use crate::ui::external_files::{ExternalEditors, ExternalEditor, OpenExternalFile};
    for (_, action) in crate::ui::external_files::EXTERNAL_MENU_ITEMS.into_iter().filter(|(_, action)| action.editor().is_some()) {
        let mut test = fixture("中文 空格/配置.json");
        test.app.insert_resource(ExternalEditors::for_tests(&ExternalEditor::ALL));
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Right);
        step(&mut test.app);
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Right);
        step(&mut test.app);
        // 检测换代与旧节点Pressed可能同帧到达，输入端也必须复核安装身份。
        test.app.insert_resource(ExternalEditors::default());
        test.app.world_mut().spawn((super::components::AssetMenuExternalButton(action), feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed));
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut test.app);
        assert!(test.app.world().resource::<Messages<OpenExternalFile>>().is_empty());
        let popup = test.app.world().get::<Node>(test.popup).unwrap();
        assert_eq!((popup.width, popup.height), (px(144), px(112))); // 收藏增加一项，IDE入口失效仍按实际可见项收缩。
    }
}

#[test]
fn external_project_menu_accepts_directories_and_only_files_offer_file_location_labels() {
    use crate::ui::external_files::{ExternalEditors, ExternalEditor, OpenExternalFile};
    for directory in [false, true] {
        for (label, action) in crate::ui::external_files::EXTERNAL_MENU_ITEMS.into_iter().filter(|(_, action)| action.in_project()) {
            let path = if directory { "中文 空格" } else { "中文 空格/配置.json" };
            let mut test = fixture(path);
            test.app.insert_resource(ExternalEditors::for_tests(&ExternalEditor::ALL));
            test.app.world_mut().resource_mut::<EditorAssetCatalog>().entries[0].is_directory = directory;
            let text = test.app.world_mut().spawn(Text::new(label)).id();
            let button = test.app.world_mut().spawn((
                super::components::AssetMenuExternalButton(action),
                Node::default(), Interaction::None, BackgroundColor::default(), BorderColor::default(),
            )).add_child(text).id();
            open_menu(&mut test);
            assert_eq!(test.app.world().get::<Node>(button).unwrap().display, Display::Flex);
            assert_eq!(test.app.world().get::<Text>(text).unwrap().0,
                if directory { format!("用 {} 打开工程", action.editor().unwrap().name()) } else { label.into() });
            let popup = test.app.world().get::<Node>(test.popup).unwrap();
            assert_eq!((popup.width, popup.height), (px(240), px(if directory { 268 } else { 294 })));
            *test.app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
            test.app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
            test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut test.app);
            step(&mut test.app); // 按住不能重复打开工程，也不改变目录选择。
            let requests: Vec<_> = test.app.world_mut().resource_mut::<Messages<OpenExternalFile>>().drain().collect();
            assert_eq!(requests.len(), 1);
            assert_eq!((requests[0].path.as_str(), requests[0].action), (path, action));
            let editor = test.app.world().resource::<MapEditorState>();
            assert_eq!(editor.selected_asset.as_deref(), Some("other/原预览.png"));
            assert!(!editor.dirty && !editor.save_requested);
        }
    }
}

/// 代替平台写入端，其余输入链保持生产系统不变。
fn collect_copies(mut messages: MessageReader<CopyAssetPath>, mut copied: ResMut<CopiedPaths>) {
    for message in messages.read() {
        copied.0.push(message.path.clone());
        copied.1.push(message.kind); // 断言右键身份和显式选择的形式，绝不写测试机器的剪贴板。
    }
}

/// 夹具只保存后续需要调整的固定实体。
struct Fixture {
    app: App,         // 不启动窗口、渲染或资源IO。
    window: Entity,   // 控制物理鼠标位置和焦点。
    row: Entity,      // 模拟Bevy已完成的单行命中。
    copy: Entity,     // 相对路径复制按钮，绝对路径测试显式切换形式。
    backdrop: Entity, // 观察菜单开关是否退出布局。
    popup: Entity,    // 观察边界修正和缩放。
}

/// 最小资源也能初始化所有真实查询，捕获B0001等运行时ECS错误。
fn fixture(path: &str) -> Fixture {
    let mut catalog = EditorAssetCatalog::default(); // 不创建后台扫描责任。
    catalog.requested_revision = 7; // 请求和已采用版本必须一致。
    catalog.applied_revision = 7; // 已显示的稳定目录。
    catalog.visible_count = 1; // 最小连续窗口只绑定这个真实条目。
    catalog.entries.push(EditorAssetEntry {
        asset_path: path.to_owned(),         // 中文和空格不预先转义。
        display_name: "右键目标".to_owned(), // 不通过显示名称猜测路径。
        is_directory: false,                 // 单击会改变选择，便于检测点透。
        kind: MapAssetKind::Scene,           // 本测试不实际加载资产。
    });
    let mut app = App::new(); // 裸ECS足以覆盖输入分派。
    app.insert_resource(catalog)
        .insert_resource(MapEditorState {
            open: true,                                               // F12已经显示。
            selected_asset: Some("other/原预览.png".to_owned()),      // 必须与右键目标不同。
            selected_map_id: Some(MapId("MAP-menu-test".to_owned())), // 冻结真实会话标识。
            ..default() // 无关闭请求、地图事务或作者脏状态。
        })
        .init_resource::<MapEditorSidebarState>() // 下方明确进入资产页。
        .init_resource::<MapEditorNumericScrubMouseGuard>() // 菜单共用真实和弦释放门。
        .init_resource::<ActiveNumericInputScrub>() // 初始无数值拖动。
        .init_resource::<MapBrowserDragState>() // 初始无地图树拖动。
        .init_resource::<MapBrowserState>() // 初始无文件夹模态。
        .init_resource::<MapDeletionTask>() // 初始无删除确认。
        .init_resource::<TerrainExternalChangeState>() // 初始无外部冲突。
        .init_resource::<AssetContextMenuState>() // 尚未打开菜单。
        .init_resource::<crate::ui::asset_preview::AssetSearchState>()
        .init_resource::<crate::ui::asset_preview::DocumentState>()
        .init_resource::<ButtonInput<MouseButton>>() // 显式模拟按下/松开边沿。
        .init_resource::<ButtonInput<KeyCode>>() // Esc关闭也是实际边沿。
        .init_resource::<CopiedPaths>() // 只在内存观察复制意图。
        .add_message::<CopyAssetPath>() // 与正式输入系统一致。
        .add_message::<crate::ui::external_files::OpenExternalFile>()
        .insert_resource(crate::ui::external_files::ExternalEditors::for_tests(&[crate::ui::external_files::ExternalEditor::VsCode]))
        .add_systems(
            Update,
            (
                super::systems::control_menu, // 冻结右键行并处理菜单点击。
                update_map_editor_numeric_scrub_mouse_guard, // 同帧发布鼠标独占。
                crate::asset_catalog::handle_asset_row_clicks, // 检查原选择入口是否被误触。
                super::scene::sync_menu,      // 初始化真实表现查询并采用显隐/位置。
                collect_copies,               // 不调用操作系统剪贴板。
            )
                .chain(),
        ); // 与生产输入、表现的先后关系一致。
    app.world_mut()
        .resource_mut::<MapEditorSidebarState>()
        .navigator = EditorNavigatorTab::Assets; // 显式满足左侧浏览器菜单资格。
    let mut window = Window {
        focused: true,
        ..default()
    }; // 不创建原生窗口。
    window.set_physical_cursor_position(Some(DVec2::new(400.0, 300.0))); // 使用物理坐标，防止重复应用缩放。
    let window = app.world_mut().spawn((PrimaryWindow, window)).id(); // 唯一鼠标来源。
    app.world_mut().spawn((
        MapEditorRoot, // 正式根标记。
        ComputedNode {
            size: Vec2::new(1280.0, 720.0),
            inverse_scale_factor: 1.0,
            ..default()
        }, // 明确非零布局。
        UiGlobalTransform::from_xy(640.0, 360.0), // 根从窗口左上角开始。
    ));
    let row = app
        .world_mut()
        .spawn((
            MapEditorAssetRow {
                slot: 0,
                entry_index: Some(0),
            }, // 当前窗口第零槽，身份使用全目录绝对索引。
            AssetRowBinding { revision: 7, viewport_revision: 0, index: Some(0) },
            Interaction::Hovered, // 命中由输入单测显式提供，非截图验证。
            Node::default(),      // 非隐藏真实行。
        ))
        .id();
    let copy = app
        .world_mut()
        .spawn((
            AssetMenuCopyButton(AssetPathCopyKind::Relative),
            Interaction::None,
            BackgroundColor::default(),
            BorderColor::default(),
        ))
        .id(); // 真实表现查询也必须无冲突。
    let backdrop = app
        .world_mut()
        .spawn((AssetMenuBackdrop, Node::default()))
        .id(); // 观察显隐。
    let popup = app
        .world_mut()
        .spawn((AssetMenuPopup, Node::default()))
        .id(); // 观察边界。
    let mut test = Fixture {
        app,
        window,
        row,
        copy,
        backdrop,
        popup,
    }; // 固定小规模夹具。
    step(&mut test.app); // 消费初始Changed标记，不让初始化干扰输入边沿。
    test // 后续测试仅通过正式系统改变菜单。
}

/// 每个模拟帧后清除边沿，保留按住的电平。
fn step(app: &mut App) {
    app.update(); // 执行真实输入、鼠标门和表现系统。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear(); // 对齐InputPlugin帧生命周期。
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear(); // 不重放Esc或F12。
}

/// 打开后释放右键，菜单仍应保持等待明确选择。
fn open_menu(test: &mut Fixture) {
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right); // 真实右键按下边沿。
    step(&mut test.app); // 正式系统冻结路径。
    assert!(
        test.app
            .world()
            .resource::<AssetContextMenuState>()
            .target
            .is_some()
    ); // 失败不能继续伪造后续复制。
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right); // 普通右键菜单在松键后保留。
    step(&mut test.app); // 活动菜单继续取得输入权。
}

#[test] // 全屏倍率和系统DPI只参与一次物理到逻辑换算。
fn menu_anchor_and_edge_avoidance_match_window_and_fullscreen_scales() {
    for (physical, scale) in [
        (Vec2::new(1280.0, 720.0), 1.0),
        (Vec2::new(3840.0, 2160.0), 2.0),
        (Vec2::new(3840.0, 2160.0), 3.0),
    ] {
        let offset = Vec2::new(20.0, 12.0); // 同时覆盖不在窗口原点的根。
        let root = Rect::from_corners(offset, offset + physical); // 实际UI物理矩形。
        let expected = Vec2::new(250.0, 120.0); // 各倍率相同的逻辑锚点。
        assert_eq!(
            logical_anchor(offset + expected * scale, root, scale.recip()),
            Some(expected)
        ); // 不用Window逻辑坐标二次除DPI。
        let viewport = physical / scale; // 最终可用逻辑尺寸。
        let rect = menu_rect(viewport - Vec2::ONE, viewport); // 在右下角打开。
        assert_eq!(rect.size(), Vec2::new(240.0, 164.0)); // 两种复制、引用、已安装IDE及文件夹六项逻辑尺寸保持一致。
        assert!(rect.min.cmpge(Vec2::ZERO).all() && rect.max.cmple(viewport).all()); // 边界内完整可见。
    }
    assert_eq!(
        menu_rect(Vec2::splat(99.0), Vec2::new(50.0, 20.0)).size(),
        Vec2::new(50.0, 20.0)
    ); // 极小窗口仍不能向外溢出。
    assert!(logical_anchor(Vec2::NAN, Rect::from_corners(Vec2::ZERO, Vec2::ONE), 1.0).is_none()); // 无效鼠标位置不猜测。
    assert!(logical_anchor(Vec2::ZERO, Rect::default(), 1.0).is_none()); // 首帧零布局不弹菜单。
}

#[test] // 相对复制使用当前项目目录，资产内部身份不变，也不附加加载器子资产地址。
fn clipboard_adapter_preserves_unicode_spaces_and_reports_real_errors() {
    for path in [
        "gltf/草地/普通 草.glb",
        "textures/植被/草 叶.png",
        "gltf/植被",
    ] {
        let mut captured = String::new(); // 只写测试内存。
        let request = CopyAssetPath { path: path.into(), kind: AssetPathCopyKind::Relative };
        let result = copy_path_status(&request, std::path::Path::new(""), |value| {
            captured = value.to_owned();
            Ok(())
        }); // 不触碰实际剪贴板。
        assert_eq!(captured, format!("assets/{path}")); // 包含项目内assets/，不加引号或Mesh后缀。
        assert!(result.starts_with("已复制相对路径：")); // 只有成功才能回显成功。
    }
    let request = CopyAssetPath { path: "test.glb".into(), kind: AssetPathCopyKind::Relative };
    let failure = copy_path_status(&request, std::path::Path::new(""), |_| {
        Err(bevy::clipboard::ClipboardError::ClipboardOccupied)
    }); // 注入真实平台错误类型。
    assert!(failure.starts_with("复制相对路径失败：")); // 失败不能伪报已复制。
}

#[test]
fn copied_asset_paths_preserve_worktree_unicode_and_do_not_require_existing_files() {
    let root = std::env::temp_dir().join("当前 工作树").join("crates/editor/../../assets");
    let relative = "config/中文 空格&$配置.json";
    let expected = std::env::temp_dir().join("当前 工作树").join("assets").join("config").join("中文 空格&$配置.json");
    assert_eq!(path_to_copy(&root, relative, AssetPathCopyKind::Relative).unwrap(), format!("assets/{relative}"));
    assert_eq!(path_to_copy(&root, "config/中文目录", AssetPathCopyKind::Relative).unwrap(), "assets/config/中文目录");
    assert_eq!(path_to_copy(&root, relative, AssetPathCopyKind::Absolute).unwrap(), expected.to_str().unwrap());
    assert_eq!(path_to_copy(&root, "config/中文目录", AssetPathCopyKind::Absolute).unwrap(),
        std::env::temp_dir().join("当前 工作树").join("assets").join("config").join("中文目录").to_str().unwrap());
    for path in ["", "../外部.json", "/外部.json", "config\\外部.json"] {
        for kind in AssetPathCopyKind::ALL { assert!(path_to_copy(&root, path, kind).is_err()); }
    }
    assert!(path_to_copy(std::path::Path::new("assets"), relative, AssetPathCopyKind::Absolute).is_err());
    let request = CopyAssetPath { path: relative.into(), kind: AssetPathCopyKind::Absolute };
    let mut writes = 0;
    let status = copy_path_status(&request, &root, |path| { writes += 1; assert_eq!(path, expected.to_str().unwrap()); Ok(()) });
    assert_eq!(writes, 1);
    assert!(status.starts_with("已复制绝对路径："));
    let failure = copy_path_status(&request, &root, |_| Err(bevy::clipboard::ClipboardError::ClipboardOccupied));
    assert!(failure.starts_with("复制绝对路径失败："));
    let invalid = CopyAssetPath { path: "../外部.json".into(), kind: AssetPathCopyKind::Absolute };
    let status = copy_path_status(&invalid, &root, |_| panic!("无效路径不得覆盖剪贴板"));
    assert!(status.starts_with("复制绝对路径失败："));
}

#[cfg(windows)]
#[test]
fn copied_absolute_paths_strip_windows_extended_disk_and_unc_prefixes() {
    assert_eq!(path_to_copy(std::path::Path::new(r"\\?\D:\当前 工作树\assets"), "config/配置.json", AssetPathCopyKind::Absolute).unwrap(),
        r"D:\当前 工作树\assets\config\配置.json");
    assert_eq!(path_to_copy(std::path::Path::new(r"\\?\UNC\server\share\assets"), "配置.json", AssetPathCopyKind::Absolute).unwrap(),
        r"\\server\share\assets\配置.json");
}

#[test] // 目录固定槽重用、刷新、切图和关闭均使旧路径失效。
fn frozen_target_requires_the_original_path_window_revision_and_map() {
    let test = fixture("gltf/原模型.glb"); // 取当前完整可用来源。
    let catalog = test.app.world().resource::<EditorAssetCatalog>(); // 无目录扫描或磁盘访问。
    let mut editor = MapEditorState {
        open: true,
        selected_map_id: Some(MapId("MAP-menu-test".into())),
        ..default()
    }; // 同一地图会话。
    let mut target = AssetMenuTarget {
        path: "gltf/原模型.glb".into(),
        index: 0,
        revision: 7,
        viewport_revision: catalog.viewport_revision,
        map_id: editor.selected_map_id.clone(),
        source: AssetMenuSource::Assets,
    }; // 与当前条目完整匹配。
    assert!(target_is_current(&target, catalog, &editor)); // 有效时允许点击复制。
    target.viewport_revision = target.viewport_revision.wrapping_add(1);
    assert!(!target_is_current(&target, catalog, &editor), "同路径仍不能复用另一窗口的菜单");
    target.viewport_revision = catalog.viewport_revision;
    target.revision += 1; // 修订不匹配，哪怕文件名一样也不能复制。
    assert!(!target_is_current(&target, catalog, &editor));
    target.revision = 7; // 恢复后单独测试路径身份。
    target.path = "gltf/替换模型.glb".into(); // 同一下标但不同文件。
    assert!(!target_is_current(&target, catalog, &editor));
    target.path = "gltf/原模型.glb".into(); // 恢复真实来源。
    target.index = ASSET_ROW_COUNT; // 可见窗口外的绝对下标不是当前绑定目标。
    assert!(!target_is_current(&target, catalog, &editor));
    target.index = 0; // 继续验证会话取消门。
    editor.pending_map_transition = Some(MapId("MAP-next".into())); // 切图请求已经开始。
    assert!(!target_is_current(&target, catalog, &editor));
    editor.pending_map_transition = None; // 无切图请求。
    editor.selected_map_id = None; // 实际地图身份改变也失效。
    assert!(!target_is_current(&target, catalog, &editor));
}

#[test]
fn frozen_target_supports_late_absolute_entries_and_rejects_scroll_rebinding() {
    let mut catalog = EditorAssetCatalog::default();
    catalog.entries = (0..96).map(|index| EditorAssetEntry { asset_path: format!("models/item-{index}.glb"),
        display_name: format!("item-{index}"), is_directory: false, kind: MapAssetKind::Scene }).collect();
    catalog.requested_revision = 7;
    catalog.applied_revision = 7;
    catalog.visible_start = 72;
    catalog.visible_count = 12;
    catalog.viewport_revision = 5;
    let editor = MapEditorState { open: true, ..default() };
    let target = AssetMenuTarget { path: "models/item-77.glb".into(), index: 77,
        revision: 7, viewport_revision: 5, map_id: None, source: AssetMenuSource::Assets };
    assert!(target_is_current(&target, &catalog, &editor), "任意可见条目都可使用菜单，不限旧第一页");
    catalog.viewport_revision += 1;
    assert!(!target_is_current(&target, &catalog, &editor), "窗口换绑后不能沿用旧菜单");
    catalog.viewport_revision = 5;
    catalog.visible_start = 84;
    assert!(!target_is_current(&target, &catalog, &editor), "已离开窗口的原绝对索引不能按槽位复用");
}
