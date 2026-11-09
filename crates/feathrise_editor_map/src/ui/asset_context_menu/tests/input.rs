//! 复用正式菜单、资产点击和鼠标门系统的交互回归，不伪装已进行游戏截图验收。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::*; // 共享无窗口、无剪贴板副作用的夹具。

#[test]
fn menu_copy_waits_for_release_preserves_target_and_cancels_outside_or_stale_press() {
    for cancel in 0..4 {
        let mut test = fixture("gltf/冻结的右键目标.glb");
        test.app.add_plugins(feathrise_editor_ui::EditorButtonInputPlugin);
        test.app.world_mut().entity_mut(test.copy).insert((
            InteractionButton, Node::default(),
            InheritedVisibility::VISIBLE, // 裸ECS夹具显式提供正式可见按钮的传播结果。
            bevy::ui::RelativeCursorPosition { cursor_over: true, ..default() },
            ComputedNode { size: Vec2::new(100.0, 24.0), ..default() },
        ));
        open_menu(&mut test);
        // 真实鼠标电平先武装共享捕获；按住只保留菜单与原文件身份。
        *test.app.world_mut().get_mut::<UiInteraction>(test.copy).unwrap() = UiInteraction::Pressed;
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut test.app);
        step(&mut test.app);
        assert!(test.app.world().resource::<CopiedPaths>().0.is_empty());
        assert_eq!(test.app.world().resource::<AssetContextMenuState>().target.as_ref().unwrap().path,
            "gltf/冻结的右键目标.glb");
        match cancel {
            1 => *test.app.world_mut().get_mut::<UiInteraction>(test.copy).unwrap() = UiInteraction::None,
            2 => test.app.world_mut().resource_mut::<EditorAssetCatalog>().requested_revision += 1,
            3 => test.app.world_mut().get_mut::<Window>(test.window).unwrap().focused = false,
            _ => {
                test.app.world_mut().resource_mut::<MapEditorState>().selected_asset = Some("其他预览.png".into());
                *test.app.world_mut().get_mut::<UiInteraction>(test.copy).unwrap() = UiInteraction::Hovered;
            }
        }
        test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        step(&mut test.app);
        step(&mut test.app);
        assert_eq!(test.app.world().resource::<CopiedPaths>().0,
            if cancel == 0 { vec!["gltf/冻结的右键目标.glb".to_owned()] } else { Vec::new() });
        assert_eq!(test.app.world().resource::<AssetContextMenuState>().target.is_some(), cancel == 1,
            "移出松开只取消动作；来源失效或失焦才关闭旧菜单");
    }
}

#[test]
fn menu_outside_press_closes_before_later_release_over_an_item() {
    let mut test = fixture("gltf/不能补执行.glb");
    test.app.add_plugins(feathrise_editor_ui::EditorButtonInputPlugin);
    test.app.world_mut().entity_mut(test.copy).insert((
        InteractionButton, Node::default(),
        InheritedVisibility::VISIBLE, // 真实命中资格不能依赖未安装的可见性传播插件。
        bevy::ui::RelativeCursorPosition { cursor_over: true, ..default() },
        ComputedNode { size: Vec2::new(100.0, 24.0), ..default() },
    ));
    open_menu(&mut test);
    test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut test.app); // 按在外部时立即关闭，不武装菜单项。
    assert!(test.app.world().resource::<AssetContextMenuState>().target.is_none());
    *test.app.world_mut().get_mut::<UiInteraction>(test.copy).unwrap() = UiInteraction::Hovered;
    test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    step(&mut test.app);
    assert!(test.app.world().resource::<CopiedPaths>().0.is_empty());
}

#[test]
fn favorite_menu_freezes_files_and_folders_and_rejects_stale_targets() {
    for is_directory in [false, true] {
        for stale in [false, true] {
            let mut test = fixture("中文 目录/右键目标");
            crate::ui::asset_shortcuts::register(&mut test.app);
            test.app.world_mut().resource_mut::<EditorAssetCatalog>().entries[0].is_directory = is_directory;
            let button = test.app.world_mut().spawn((super::super::components::AssetMenuFavoriteButton,
                Node::default(), UiInteraction::None, BackgroundColor::default(), BorderColor::default())).id();
            open_menu(&mut test);
            if stale { test.app.world_mut().resource_mut::<EditorAssetCatalog>().requested_revision += 1; }
            *test.app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
            test.app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
            test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut test.app);
            step(&mut test.app);
            let requests: Vec<_> = test.app.world_mut().resource_mut::<Messages<crate::ui::asset_shortcuts::SetAssetFavorite>>().drain().collect();
            assert_eq!(requests.len(), usize::from(!stale));
            if let Some(request) = requests.first() { assert_eq!(request.path, "中文 目录/右键目标"); assert_eq!(request.is_directory, is_directory); }
            let editor = test.app.world().resource::<MapEditorState>();
            assert_eq!(editor.selected_asset.as_deref(), Some("other/原预览.png"));
            assert!(!editor.dirty && !editor.save_requested);
        }
    }
}

#[test]
fn references_menu_freezes_file_and_hides_on_directories_and_stale_targets() {
    use super::super::components::AssetMenuReferencesButton;
    use crate::ui::asset_preview::AssetSearchState;
    for directory in [false, true] {
        for stale in [false, true] {
            let mut test = fixture("gltf/中文 空格.glb");
            test.app.world_mut().resource_mut::<EditorAssetCatalog>().entries[0].is_directory = directory;
            let button = test.app.world_mut().spawn((AssetMenuReferencesButton, Node::default(), UiInteraction::None,
                BackgroundColor::default(), BorderColor::default())).id();
            open_menu(&mut test);
            assert_eq!(test.app.world().get::<Node>(button).unwrap().display, if directory { Display::None } else { Display::Flex });
            if stale { test.app.world_mut().resource_mut::<EditorAssetCatalog>().requested_revision += 1; }
            *test.app.world_mut().get_mut::<UiInteraction>(button).unwrap() = UiInteraction::Pressed;
            test.app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
            test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut test.app);
            step(&mut test.app);
            assert_eq!(test.app.world().resource::<AssetSearchState>().requested_reference_target(),
                (!directory && !stale).then_some("gltf/中文 空格.glb"));
            assert!(test.app.world().resource::<CopiedPaths>().0.is_empty());
            assert!(test.app.world().resource::<Messages<crate::ui::external_files::OpenExternalFile>>().is_empty());
            let editor = test.app.world().resource::<MapEditorState>();
            assert_eq!(editor.selected_asset.as_deref(), Some("other/原预览.png"));
            assert!(!editor.dirty && !editor.save_requested);
        }
    }
}

#[test] // 右键与左键选择/预览是独立操作。
fn right_click_copies_the_clicked_row_once_without_changing_selection() {
    let path = "gltf/植被/白桦 树.glb"; // 文件名保留中文与空格。
    let mut test = fixture(path); // 当前左键选中的是另一个纹理。
    open_menu(&mut test); // 右键仅冻结当前行，不选择或复制。
    assert!(test.app.world().resource::<CopiedPaths>().0.is_empty()); // 弹菜单不是复制授权。
    assert_eq!(
        test.app.world().get::<Node>(test.backdrop).unwrap().display,
        Display::Flex
    ); // 覆盖层实际参与命中。
    assert_eq!(
        test.app.world().get::<Node>(test.popup).unwrap().left,
        px(404)
    ); // 使用物理根换算后的逻辑位置。
    *test
        .app
        .world_mut()
        .get_mut::<UiInteraction>(test.copy)
        .unwrap() = UiInteraction::Pressed;
    test.app.world_mut().entity_mut(test.copy).insert(feathrise_editor_ui::EditorButtonClick::triggered()); // 模拟复制按钮收到左击。
    *test
        .app
        .world_mut()
        .get_mut::<UiInteraction>(test.row)
        .unwrap() = UiInteraction::Pressed;
    test.app.world_mut().entity_mut(test.row).insert(feathrise_editor_ui::EditorButtonClick::triggered()); // 即使底层有旧按下也不能点透。
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left); // 唯一明确复制边沿。
    step(&mut test.app); // 当帧关闭菜单并保留输入权。
    step(&mut test.app); // 按住不能重复复制。
    assert_eq!(test.app.world().resource::<CopiedPaths>().0, vec![path]); // 精确路径且恰好一次。
    let editor = test.app.world().resource::<MapEditorState>(); // 右键不能改写地图或原预览。
    assert_eq!(editor.selected_asset.as_deref(), Some("other/原预览.png")); // 绝不误复制原先选择项。
    assert!(!editor.dirty && !editor.save_requested); // 复制不启动保存或作者修改。
    assert_eq!(
        test.app.world().get::<Node>(test.backdrop).unwrap().display,
        Display::None
    ); // 复制后菜单立即退出命中。
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left); // 释放最后一枚鼠标键。
    step(&mut test.app); // release帧仍不能点透。
    assert!(
        test.app
            .world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    step(&mut test.app); // 完整静默帧才归还所有权。
    assert!(
        !test
            .app
            .world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    assert_eq!(
        test.app
            .world()
            .resource::<MapEditorState>()
            .selected_asset
            .as_deref(),
        Some("other/原预览.png")
    ); // Changed事件不会延迟重放。
}

#[test]
fn copied_path_menu_actions_freeze_the_clicked_asset_for_both_files_and_directories() {
    for kind in AssetPathCopyKind::ALL {
        for directory in [false, true] {
            let mut test = fixture("config/右键 中文目标");
            test.app.world_mut().resource_mut::<EditorAssetCatalog>().entries[0].is_directory = directory;
            test.app.world_mut().get_mut::<AssetMenuCopyButton>(test.copy).unwrap().0 = kind;
            open_menu(&mut test);
            *test.app.world_mut().get_mut::<UiInteraction>(test.copy).unwrap() = UiInteraction::Pressed;
            test.app.world_mut().entity_mut(test.copy).insert(feathrise_editor_ui::EditorButtonClick::triggered());
            test.app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
            step(&mut test.app);
            step(&mut test.app); // 按住不能多次复制或选中后方资产。
            let copied = test.app.world().resource::<CopiedPaths>();
            assert_eq!(copied.0, vec!["config/右键 中文目标"]);
            assert_eq!(copied.1, vec![kind]);
            let editor = test.app.world().resource::<MapEditorState>();
            assert_eq!(editor.selected_asset.as_deref(), Some("other/原预览.png"));
            assert!(!editor.dirty && !editor.save_requested);
        }
    }
}

#[test] // 空白、Esc、第二次右键和中键都只是关闭菜单。
fn dismissal_does_not_copy_or_activate_the_underlying_row() {
    for button in [
        None,
        Some(MouseButton::Left),
        Some(MouseButton::Right),
        Some(MouseButton::Middle),
    ] {
        let mut test = fixture("textures/草.png"); // 每种关闭方式使用独立会话。
        open_menu(&mut test); // 菜单已显示并松开起始右键。
        if let Some(button) = button {
            test.app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(button); // 复制按钮未命中，左键代表空白处。
        } else {
            test.app
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape); // Esc不能泄漏到后方备注输入。
        }
        *test
            .app
            .world_mut()
            .get_mut::<UiInteraction>(test.row)
            .unwrap() = UiInteraction::Pressed;
        test.app.world_mut().entity_mut(test.row).insert(feathrise_editor_ui::EditorButtonClick::triggered()); // 底层潜在点击必须被公共门消费。
        step(&mut test.app); // 正式取消分支。
        assert!(
            test.app
                .world()
                .resource::<AssetContextMenuState>()
                .target
                .is_none()
        ); // 菜单已撤销。
        assert!(
            test.app
                .world()
                .resource::<MapEditorNumericScrubMouseGuard>()
                .blocked()
        ); // 关闭帧保留输入权。
        assert!(test.app.world().resource::<CopiedPaths>().0.is_empty()); // 无显式复制意图。
        assert_eq!(
            test.app
                .world()
                .resource::<MapEditorState>()
                .selected_asset
                .as_deref(),
            Some("other/原预览.png")
        ); // 后方列表不补选。
    }
}

#[test] // 活动菜单不能在来源已经改变后复制旧行。
fn catalog_and_session_changes_close_before_a_copy_click() {
    for change in 0..7 {
        let mut test = fixture("gltf/old.glb"); // 初始可用来源。
        open_menu(&mut test); // 冻结旧目录。
        match change {
            0 => {
                test.app
                    .world_mut()
                    .resource_mut::<EditorAssetCatalog>()
                    .requested_revision += 1
            } // 扫描尚未完成即失效。
            1 => {
                test.app
                    .world_mut()
                    .resource_mut::<EditorAssetCatalog>()
                    .applied_revision += 1
            } // 新完整目录已采用。
            2 => {
                test.app
                    .world_mut()
                    .resource_mut::<EditorAssetCatalog>()
                    .viewport_revision += 1
            } // 连续滚动换绑复用相同行槽。
            3 => {
                test.app
                    .world_mut()
                    .resource_mut::<EditorAssetCatalog>()
                    .entries[0]
                    .asset_path = "gltf/new.glb".into()
            } // 同一槽已换成其他文件。
            4 => test.app.world_mut().resource_mut::<MapEditorState>().open = false, // 关闭F12。
            5 => {
                test.app
                    .world_mut()
                    .get_mut::<Window>(test.window)
                    .unwrap()
                    .focused = false
            } // 切换其他应用。
            _ => {
                test.app
                    .world_mut()
                    .resource_mut::<MapEditorSidebarState>()
                    .navigator = EditorNavigatorTab::Scene
            } // 切到其他侧栏页。
        }
        *test
            .app
            .world_mut()
            .get_mut::<UiInteraction>(test.copy)
            .unwrap() = UiInteraction::Pressed;
        test.app.world_mut().entity_mut(test.copy).insert(feathrise_editor_ui::EditorButtonClick::triggered()); // 即使旧UI仍残留一次点击。
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left); // 来源检查必须优先。
        step(&mut test.app); // 不允许复制过时身份。
        assert!(
            test.app
                .world()
                .resource::<AssetContextMenuState>()
                .target
                .is_none()
        ); // 菜单同步撤下。
        assert!(test.app.world().resource::<CopiedPaths>().0.is_empty()); // 无错误复制。
    }
}

#[test] // 隐藏、过期行、模态和已有手势不能抢占新右键。
fn opening_requires_a_current_visible_hovered_row_and_no_other_owner() {
    for blocked in 0..11 {
        let mut test = fixture("gltf/test.glb"); // 每次都从完全静默状态开始。
        match blocked {
            0 => {
                test.app
                    .world_mut()
                    .get_mut::<Node>(test.row)
                    .unwrap()
                    .display = Display::None
            } // 裁切/隐藏槽不是目标。
            1 => {
                *test
                    .app
                    .world_mut()
                    .get_mut::<UiInteraction>(test.row)
                    .unwrap() = UiInteraction::None
            } // 空白区域不回退到旧选择。
            2 => {
                test.app
                    .world_mut()
                    .get_mut::<MapEditorAssetRow>(test.row)
                    .unwrap()
                    .entry_index = Some(ASSET_ROW_COUNT)
            } // 行池未完成换绑。
            3 => {
                test.app
                    .world_mut()
                    .resource_mut::<EditorAssetCatalog>()
                    .requested_revision += 1
            } // 目录请求正在进行。
            4 => {
                test.app
                    .world_mut()
                    .resource_mut::<MapEditorState>()
                    .new_map_dialog_open = true
            } // 更高优先级模态。
            5 => {
                test.app
                    .world_mut()
                    .resource_mut::<MapBrowserDragState>()
                    .blocked = true
            } // 地图树仍拥有鼠标。
            6 => {
                test.app
                    .world_mut()
                    .resource_mut::<ActiveNumericInputScrub>()
                    .entity = Some(test.row)
            } // 数字框同帧已经开始拖动。
            7 => test
                .app
                .world_mut()
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Middle), // 已有鼠标和弦。
            8 => test.app.world_mut().get_mut::<AssetRowBinding>(test.row).unwrap().revision = 6,
            9 => test.app.world_mut().get_mut::<AssetRowBinding>(test.row).unwrap().viewport_revision = 1,
            _ => test.app.world_mut().get_mut::<AssetRowBinding>(test.row).unwrap().index = Some(1),
        }
        test.app
            .world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Right); // 新右键必须让位。
        step(&mut test.app); // 执行真实资格门。
        assert!(
            test.app
                .world()
                .resource::<AssetContextMenuState>()
                .target
                .is_none()
        ); // 不抢占已有动作。
        assert!(test.app.world().resource::<CopiedPaths>().0.is_empty()); // 不隐式复制任何文件。
    }
}

#[test] // 普通左键仍走已有选择与预览入口。
fn a_normal_left_click_still_selects_the_asset() {
    let mut test = fixture("gltf/selected.glb"); // 没有打开菜单。
    *test
        .app
        .world_mut()
        .get_mut::<UiInteraction>(test.row)
        .unwrap() = UiInteraction::Pressed;
    test.app.world_mut().entity_mut(test.row).insert(feathrise_editor_ui::EditorButtonClick::triggered()); // 正常Bevy左键命中。
    test.app
        .world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left); // 正常边沿不会取得菜单所有权。
    step(&mut test.app); // 调用原资产选择系统。
    assert_eq!(
        test.app
            .world()
            .resource::<MapEditorState>()
            .selected_asset
            .as_deref(),
        Some("gltf/selected.glb")
    ); // 左键行为完全保留。
    assert!(test.app.world().resource::<CopiedPaths>().0.is_empty()); // 左键不能自动改写系统剪贴板。
}

#[test]
fn right_tool_tabs_do_not_close_the_left_asset_menu() {
    let mut test = fixture("gltf/context.glb");
    open_menu(&mut test);
    test.app.world_mut().resource_mut::<MapEditorSidebarState>().right = crate::ui::sidebars::EditorRightSidebarTab::Tools;
    step(&mut test.app);
    assert!(test.app.world().resource::<AssetContextMenuState>().target.is_some());
    test.app.world_mut().resource_mut::<MapEditorSidebarState>().navigator = EditorNavigatorTab::Maps;
    step(&mut test.app);
    assert!(test.app.world().resource::<AssetContextMenuState>().target.is_none());
}
