//! 原场景页BSN、中文字体及固定二十八槽的布局回归；不是原生窗口视觉验收。

use feathrise_editor_ui::UiInteraction;
use super::*;
use crate::constants::{SCENE_BROWSER_ROW_HEIGHT, SCENE_BROWSER_ROW_PITCH, SCENE_BROWSER_ROW_POOL_COUNT};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::text::LineBreak;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_editor_ui::EditorUiPlugin;
use feathrise_editor_core::EditorSettings;

#[derive(Resource)]
struct TestFont(Handle<Font>);

/// 复用正式Scene与池化系统，排除目录扫描、场景加载及后台文件事务。
fn page() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
        EditorUiPlugin,
    ));
    initialize(&mut app);
    app.init_resource::<UiSurface>()
        .init_asset::<Image>()
        .insert_resource(UiScale(1.0))
        .init_resource::<EditorSettings>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<EditorAssetCatalog>();
    crate::ui::context_help::install(&mut app);
    crate::ui::status_bar::install(&mut app); // 复用正式反馈收口，面板文字源不得因夹具缺系统而重新参与布局。
    let long_name = "完整中文场景名称".repeat(16);
    let entries = (0..120)
        .map(|number| entry(scene_item(&format!("MAP-{number}")), &long_name))
        .collect();
    app.insert_resource(published_index(entries));
    register_manifest(&mut app, manifest("MAP-1", &long_name));
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = Some(SceneId::parse("MAP-1").unwrap());
    app.world_mut()
        .resource_mut::<SceneBrowserState>()
        .selected_item = Some(scene_item("MAP-1"));
    app.add_systems(Startup, crate::ui::scene_sidebar::scene::scene_page.spawn());
    app.add_systems(
        Update,
        (
            crate::ui::rows::spawn_dynamic_rows,
            crate::ui::rows::sync_dynamic_list_layouts,
            crate::ui::rows::sync_scene_rows,
            crate::scene_browser::sync_scene_browser_root_row,
            sync_sidebar,
            crate::ui::sidebars::sync_editor_sidebar_presentation,
        )
            .chain(),
    );
    app.add_plugins((
        HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
        HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
    ));
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiTargetCamera>::default()
            .after(propagate_ui_target_cameras)
            .before(bevy::ui::widget::measure_text_system),
    );
    app.configure_sets(
        PostUpdate,
        PropagateSet::<ComputedUiRenderTargetInfo>::default()
            .after(propagate_ui_target_cameras)
            .before(bevy::ui::widget::measure_text_system),
    );
    app.add_systems(
        PostUpdate,
        (
            propagate_ui_target_cameras,
            bevy::ui::widget::measure_text_system.after(bevy::text::detect_text_needs_rerender),
            ui_layout_system,
        )
            .chain()
            .after(bevy::ui::UiSystems::Prepare), // 正式帮助移挂与底栏隐藏均先于本夹具的实际布局。
    );
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    app.insert_resource(TestFont(font));
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::splat(4096),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::splat(4096),
                ..default()
            }),
            ..default()
        },
    ));
    app.update();
    let world = app.world_mut();
    let root = world
        .query_filtered::<Entity, With<SceneEditorLeftPanelScroll>>()
        .single(world)
        .unwrap();
    let host = world
        .spawn(Node {
            width: px(340),
            height: px(500),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .id();
    world.entity_mut(root).insert(ChildOf(host));
    let editor_root = world.spawn((SceneEditorRoot, Node {
        width: percent(100), height: percent(100), min_width: px(0), min_height: px(0),
        ..default()
    })).id();
    world.entity_mut(host).insert(ChildOf(editor_root));
    let popup = world.spawn_scene(crate::ui::context_help::popup()).unwrap().id();
    world.entity_mut(popup).insert(ChildOf(editor_root)); // 同正式根结构：帮助由全屏根定位，不继承左栏宽度。
    settle(&mut app);
    (app, host, root)
}

fn settle(app: &mut App) {
    for _ in 0..6 {
        app.update();
        let font = app.world().resource::<TestFont>().0.clone();
        let world = app.world_mut();
        for mut text in world.query::<&mut TextFont>().iter_mut(world) {
            if text.font != font.clone().into() {
                text.font = font.clone().into();
            }
        }
    }
    app.update();
}

fn horizontal_bounds(world: &World, entity: Entity) -> (f32, f32) {
    let computed = world.get::<ComputedNode>(entity).unwrap();
    let center = world.get::<UiGlobalTransform>(entity).unwrap().translation.x;
    (center - computed.size().x / 2.0, center + computed.size().x / 2.0)
}

#[test]
fn directory_rows_separate_icons_short_names_and_current_badge() {
    let (mut app, host, _) = page();
    let folder = SceneBrowserItem::Folder("FOLDER-A".into());
    let nested = SceneBrowserEntry { depth: 2, parent_folder_id: Some("FOLDER-A".into()), ..entry(scene_item("MAP-1"), "很长的中文场景名称".repeat(12).as_str()) };
    app.insert_resource(published_index(vec![entry(folder.clone(), "测试场景"), nested, entry(scene_item("MAP-2"), "另一个场景")]));
    app.world_mut().resource_mut::<SceneEditorState>().selected_scene_id = Some(SceneId::parse("MAP-2").unwrap());
    {
        let mut browser = app.world_mut().resource_mut::<SceneBrowserState>();
        browser.select_only(Some(scene_item("MAP-1")));
        browser.expanded_folder_ids.insert("FOLDER-A".into());
    }
    settle(&mut app);
    let world = app.world_mut();
    let icons = world.resource::<crate::ui::scene_sidebar::SceneBrowserTreeIcons>().clone();
    let mut nested_left = Val::ZERO;
    let mut folder_left = Val::ZERO;
    for (row, node, children) in world.query::<(&SceneEditorSceneRow, &Node, &Children)>().iter(world) {
        let Some(item) = &row.item else { continue };
        assert_eq!(node.border, UiRect::ZERO, "树行不能保留逐行按钮边框");
        assert_eq!(children.len(), 4, "箭头、类型图标、名称裁切槽、当前标记必须独立");
        let current = children.iter().find_map(|child| world.get::<SceneEditorSceneRowCurrent>(child).and_then(|_| world.get::<Text>(child))).unwrap();
        assert_eq!(current.as_str(), if *item == scene_item("MAP-2") { "当前" } else { "" });
        let name = children.iter().find_map(|child| world.get::<SceneEditorSceneRowNameCell>(child).map(|_| child)).unwrap();
        assert_eq!(world.get::<Node>(name).unwrap().overflow, Overflow::clip_x());
        let text = world.get::<Children>(name).unwrap().iter().find_map(|child| world.get::<Text>(child)).unwrap();
        assert!(!text.contains("MAP-") && !text.contains("[目录]") && !text.contains("当前"));
        if *item == folder {
            folder_left = node.padding.left;
            for child in children.iter() {
                if let Some(role) = world.get::<SceneEditorSceneRowGlyph>(child) {
                    let image = world.get::<ImageNode>(child).unwrap();
                    match role {
                        SceneEditorSceneRowGlyph::Entry => assert_eq!(image.image, icons.folder),
                        SceneEditorSceneRowGlyph::Disclosure => assert_eq!(image.image, icons.collapse),
                        _ => unreachable!(),
                    }
                }
            }
        } else if *item == scene_item("MAP-1") {
            nested_left = node.padding.left;
            assert!(text.contains("很长的中文场景名称"));
        }
    }
    let (Val::Px(nested_left), Val::Px(folder_left)) = (nested_left, folder_left) else { panic!("树缩进必须使用逻辑像素") };
    assert_eq!(nested_left - folder_left, 18.0);
    assert_eq!(world.resource::<SceneBrowserState>().selected_item, Some(scene_item("MAP-1")));
    assert_eq!(world.resource::<SceneEditorState>().selected_scene_id, Some(SceneId::parse("MAP-2").unwrap()), "目录高亮不能改变当前场景");
    app.world_mut().resource_mut::<SceneBrowserState>().expanded_folder_ids.clear();
    settle(&mut app);
    let world = app.world_mut();
    let (_, children) = world.query::<(&SceneEditorSceneRow, &Children)>().iter(world).find(|(row, _)| row.item.as_ref() == Some(&folder)).unwrap();
    assert!(children.iter().any(|child| matches!(world.get::<SceneEditorSceneRowGlyph>(child), Some(SceneEditorSceneRowGlyph::Disclosure)) && world.get::<ImageNode>(child).unwrap().image == icons.expand));
    // 最深合法层级和最窄栏同时出现时，缩进不能把独立当前标记挤出行。
    app.world_mut().get_mut::<Node>(host).unwrap().width = px(187);
    std::sync::Arc::make_mut(&mut app.world_mut().resource_mut::<SceneBrowserIndexState>().entries)[2].depth = 16;
    settle(&mut app);
    let world = app.world_mut();
    let (_, size, at, children) = world.query::<(&SceneEditorSceneRow, &ComputedNode, &UiGlobalTransform, &Children)>().iter(world)
        .find(|(row, _, _, _)| row.item == Some(scene_item("MAP-2"))).unwrap();
    let badge = children.iter().find(|child| world.get::<SceneEditorSceneRowCurrent>(*child).is_some()).unwrap();
    let badge_size = world.get::<ComputedNode>(badge).unwrap().size();
    let badge_at = world.get::<UiGlobalTransform>(badge).unwrap().translation;
    assert!(badge_size.x > 0.0);
    assert!(badge_at.x + badge_size.x / 2.0 <= at.translation.x + size.size().x / 2.0 + 1.0);
}

#[test]
fn chinese_metadata_and_single_row_buttons_fit_36_width_scale_dpi_combinations() {
    let (mut app, host, root) = page();
    app.world_mut().resource_mut::<SceneEditorSidebarState>()
        .toggle_help(crate::ui::sidebars::EditorSidebarHelp::SceneBrowser);
    let original_selected = app
        .world()
        .resource::<SceneBrowserState>()
        .selected_item
        .clone();
    // 187/227对应200/240逻辑像素左栏扣除原有左右内边距和边框后的正文宽度。
    for width in [187, 227, 260, 320, 420, 560] {
        for scale in [0.6, 1.0, 1.5] {
            for dpi in [1.0, 2.0] {
                app.world_mut().get_mut::<Node>(host).unwrap().width = px(width);
                app.world_mut().resource_mut::<UiScale>().0 = scale;
                let world = app.world_mut();
                for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                    camera.computed.target_info.as_mut().unwrap().scale_factor = dpi;
                }
                settle(&mut app);
                let world = app.world_mut();
                let size = world.get::<ComputedNode>(root).unwrap().size().x;
                // 问号正文已借到根浮层：按真实所属容器验收，仍逐级保证正文、滚动区、窗口均无横向溢出。
                let (body, scroll) = world.query_filtered::<(Entity, &ChildOf),
                    With<crate::ui::context_help::ContextHelpMounted>>().single(world).unwrap();
                let scroll = scroll.parent();
                let popup = world.get::<ChildOf>(scroll).unwrap().parent();
                let editor_root = world.get::<ChildOf>(popup).unwrap().parent();
                for (child, parent) in [(body, scroll), (scroll, popup), (popup, editor_root)] {
                    let (left, right) = horizontal_bounds(world, child);
                    let (parent_left, parent_right) = horizontal_bounds(world, parent);
                    assert!(right > left, "帮助容器必须有实际宽度：{width}/{scale}/{dpi}");
                    assert!(left >= parent_left - 1.1 && right <= parent_right + 1.1,
                        "帮助容器横向溢出：{child:?}/{width}/{scale}/{dpi}");
                }
                let mut visible_help_fields = 0;
                for (marker, node, layout, computed, position, parent) in world
                    .query::<(
                        &SceneSidebarText,
                        &Node,
                        &TextLayout,
                        &ComputedNode,
                        &UiGlobalTransform,
                        &ChildOf,
                    )>()
                    .iter(world)
                {
                    if node.display == Display::None {
                        continue;
                    }
                    let owner = parent.parent();
                    assert!(owner == root || owner == body, "目录字段只能属于正文或唯一帮助浮层");
                    if owner == body { visible_help_fields += 1; }
                    let (owner_left, owner_right) = horizontal_bounds(world, owner);
                    assert_eq!(layout.linebreak, LineBreak::AnyCharacter);
                    assert!(
                        position.translation.x - computed.size().x / 2.0
                            >= owner_left - 1.1,
                        "左侧溢出 {:?}/{width}/{scale}/{dpi}",
                        marker.0
                    );
                    assert!(
                        position.translation.x + computed.size().x / 2.0
                            <= owner_right + 1.1,
                        "右侧溢出 {:?}/{width}/{scale}/{dpi}",
                        marker.0
                    );
                    if marker.0 == SidebarField::SelectedName {
                        assert!(computed.size().y > 20.0 * scale * dpi, "完整中文名称应换行");
                    }
                }
                assert!(visible_help_fields > 0, "帮助字段必须真实显示，不能靠全部隐藏跳过边界验收");
                for node in world.query_filtered::<&Node, With<crate::ui::status_bar::PanelStatusText>>().iter(world) {
                    assert_eq!(node.display, Display::None, "反馈文字只能交给底栏与详情，不参与面板或帮助布局");
                }
                assert_eq!(world.query_filtered::<Entity, With<SceneSidebarActionBinding>>()
                    .iter(world).count(), 0, "目录正文的操作已迁入根级右键浮层，不能继续占用常驻行");
                for (node, computed) in world
                    .query_filtered::<(&Node, &ComputedNode), With<SceneEditorSceneRow>>()
                    .iter(world)
                {
                    if node.display != Display::None {
                        assert!(
                            (computed.size().y - SCENE_BROWSER_ROW_HEIGHT * scale * dpi).abs() <= 1.1
                        );
                        assert!(computed.size().x <= size + 1.1);
                    }
                }
            }
        }
    }
    assert_eq!(
        app.world().resource::<SceneBrowserState>().selected_item,
        original_selected
    );
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn folder_projection_never_moves_the_root_or_tree_between_frames() {
    let (mut app, host, _) = page();
    let folder = SceneBrowserItem::Folder("FOLDER-1".into());
    let collapsed = vec![
        entry(folder.clone(), "111"),
        entry(SceneBrowserItem::Folder("FOLDER-2".into()), "222"),
    ];
    let child = SceneBrowserEntry {
        depth: 2,
        parent_folder_id: Some("FOLDER-1".into()),
        ..entry(scene_item("MAP-1"), "子场景")
    };
    let expanded = vec![collapsed[0].clone(), child, collapsed[1].clone()];
    let mut index = published_index(expanded.clone());
    index.entries = Arc::new(collapsed.clone());
    app.insert_resource(index);
    app.world_mut().resource_mut::<SceneBrowserState>().selected_item = Some(folder);
    settle(&mut app);
    let world = app.world_mut();
    let root = world.query_filtered::<Entity, With<SceneEditorSceneRootRow>>().single(world).unwrap();
    let tree = world.query_filtered::<Entity, With<SceneEditorSceneTreeScroll>>().single(world).unwrap();
    let feedback = field(&mut app, SidebarField::DirectoryFeedback);
    let bounds = |app: &App, entity| {
        let world = app.world();
        Rect::from_center_size(
            world.get::<UiGlobalTransform>(entity).unwrap().translation,
            world.get::<ComputedNode>(entity).unwrap().size(),
        )
    };
    // 逐帧检查等待和采用两个阶段，不能只测任务完成后的最终截图。
    for (width, scale) in [(232.0, 1.0), (360.0, 1.25)] {
        app.world_mut().get_mut::<Node>(host).unwrap().width = px(width);
        app.world_mut().resource_mut::<UiScale>().0 = scale;
        settle(&mut app);
        let before = [root, tree].map(|entity| bounds(&app, entity));
        for open in [true, false, true, false] {
            {
                let mut browser = app.world_mut().resource_mut::<SceneBrowserState>();
                if open {
                    browser.expanded_folder_ids.insert("FOLDER-1".into());
                } else {
                    browser.expanded_folder_ids.remove("FOLDER-1");
                }
                browser.expansion_revision += 1;
            }
            {
                let revision = app.world().resource::<SceneBrowserState>().expansion_revision;
                let mut index = app.world_mut().resource_mut::<SceneBrowserIndexState>();
                index.desired_projection.as_mut().unwrap().1 = revision;
                index.rebuilding = true;
            }
            for adopted in [false, true] {
                if adopted {
                    let mut index = app.world_mut().resource_mut::<SceneBrowserIndexState>();
                    index.entries = Arc::new(if open { expanded.clone() } else { collapsed.clone() });
                    index.applied_projection = index.desired_projection.clone();
                    index.rebuilding = false;
                }
                for _ in 0..3 {
                    app.update();
                    assert_eq!(app.world().get::<Node>(feedback).unwrap().display, Display::None);
                    for (entity, expected) in [root, tree].into_iter().zip(before) {
                        assert_eq!(bounds(&app, entity), expected, "目录展开等待或采用不应挤动根目录与树区域");
                    }
                }
            }
        }
    }
}

#[test]
fn original_scroll_owners_fixed_pool_and_selection_entities_survive_navigation() {
    let (mut app, host, root) = page();
    // 原名称/ID字段仍然存在，帮助浮层借用原实体且不改变目录正文或所选场景。
    let details = {
        let world = app.world_mut();
        world.query::<(Entity, &crate::ui::sidebars::SceneEditorSidebarHelpBody)>()
            .iter(world).find(|(_, help)| help.0 == crate::ui::sidebars::EditorSidebarHelp::SceneBrowser).unwrap().0
    };
    assert_eq!(app.world().get::<Node>(details).unwrap().display, Display::None);
    assert!(!text(&mut app, SidebarField::SelectedSummary).is_empty());
    assert!(text(&mut app, SidebarField::DirectoryFeedback).is_empty());
    let before = app.world().get::<ComputedNode>(root).unwrap().size();
    app.world_mut().resource_mut::<SceneEditorSidebarState>()
        .toggle_help(crate::ui::sidebars::EditorSidebarHelp::SceneBrowser);
    settle(&mut app);
    assert_eq!(app.world().get::<Node>(details).unwrap().display, Display::Flex);
    assert!(app.world().get::<crate::ui::context_help::ContextHelpMounted>(details).is_some());
    assert_ne!(app.world().get::<ChildOf>(details).unwrap().parent(), root);
    assert_eq!(app.world().get::<ComputedNode>(root).unwrap().size(), before);
    app.world_mut().resource_mut::<SceneEditorSidebarState>()
        .toggle_help(crate::ui::sidebars::EditorSidebarHelp::SceneBrowser);
    settle(&mut app);
    let world = app.world_mut();
    let tree = world
        .query_filtered::<Entity, With<SceneEditorSceneTreeScroll>>()
        .single(world)
        .unwrap();
    let rows = world
        .query_filtered::<Entity, With<SceneEditorSceneRow>>()
        .iter(world)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), SCENE_BROWSER_ROW_POOL_COUNT);
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorSceneList>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorSceneRootRow>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneSidebarActionBinding>>()
            .iter(world)
            .count(),
        0
    );
    let selected = field(&mut app, SidebarField::SelectedName);
    app.world_mut().get_mut::<Node>(host).unwrap().height = px(160);
    app.world_mut().get_mut::<ScrollPosition>(root).unwrap().y = 49.0;
    app.world_mut().get_mut::<ScrollPosition>(tree).unwrap().y = SCENE_BROWSER_ROW_PITCH * 48.0;
    settle(&mut app);
    assert_eq!(app.world().get::<ScrollPosition>(root).unwrap().y, 49.0);
    assert_eq!(
        app.world().get::<ScrollPosition>(tree).unwrap().y,
        SCENE_BROWSER_ROW_PITCH * 48.0
    );
    assert!(app.world().get::<Node>(root).unwrap().overflow.y == bevy::ui::OverflowAxis::Scroll);
    assert!(app.world().get::<Node>(tree).unwrap().overflow.y == bevy::ui::OverflowAxis::Scroll);
    app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator = EditorNavigatorTab::SceneTree;
    app.update();
    app.world_mut().resource_mut::<SceneEditorSidebarState>().navigator = EditorNavigatorTab::Scenes;
    settle(&mut app);
    assert_eq!(field(&mut app, SidebarField::SelectedName), selected);
    for entity in rows {
        assert!(app.world().get::<SceneEditorSceneRow>(entity).is_some());
    }
    assert_eq!(
        app.world().get::<ScrollPosition>(tree).unwrap().y,
        SCENE_BROWSER_ROW_PITCH * 48.0
    );
}

#[test]
fn automatic_drag_scroll_rebinds_the_original_pool_and_stops_on_release() {
    use bevy::time::TimeUpdateStrategy;
    use bevy::ui::RelativeCursorPosition;
    use bevy::window::PrimaryWindow;
    use std::time::Duration;

    let (mut app, _, root) = page();
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
        20,
    )));
    let world = app.world_mut();
    let tree = world
        .query_filtered::<Entity, With<SceneEditorSceneTreeScroll>>()
        .single(world)
        .unwrap();
    let rows: Vec<_> = world
        .query_filtered::<Entity, With<SceneEditorSceneRow>>()
        .iter(world)
        .collect();
    let first = *rows
        .iter()
        .find(|entity| world.get::<SceneEditorSceneRow>(**entity).unwrap().slot == 0)
        .unwrap();
    let original = world.get::<SceneEditorSceneRow>(first).unwrap().item.clone();
    let bounds = Rect::from_center_size(
        world.get::<UiGlobalTransform>(tree).unwrap().translation,
        world.get::<ComputedNode>(tree).unwrap().size(),
    );
    let mut window = Window::default();
    window.resolution.set_physical_resolution(4096, 4096);
    window.set_physical_cursor_position(Some(
        Vec2::new(bounds.center().x, bounds.max.y - 1.0).into(),
    ));
    world.spawn((window, PrimaryWindow));
    world.entity_mut(tree).insert((
        InheritedVisibility::VISIBLE,
        RelativeCursorPosition {
            cursor_over: true,
            ..default()
        },
    ));
    *world.resource_mut::<SceneBrowserDragState>() = SceneBrowserDragState {
        press: Some(crate::types::SceneBrowserDragPress {
            item: scene_item("MAP-1"),
            document_revision: 1,
            display_name: "拖动场景".into(),
            start_physical: Vec2::ZERO,
        }),
        active: true,
        blocked: true,
        ..default()
    };
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.add_systems(
        Update,
        (
            crate::scene_browser::update_scene_browser_drag,
            crate::scene_browser::navigate_scene_browser_drag,
        )
            .chain()
            .before(crate::ui::rows::sync_dynamic_list_layouts),
    );
    for _ in 0..150 {
        app.update();
    } // 26px行高的一整池超过728px；模拟三秒拖拽，验证跨池且不新建行。
    let world = app.world_mut();
    assert!(
        world.get::<ScrollPosition>(tree).unwrap().y
            > SCENE_BROWSER_ROW_POOL_COUNT as f32 * SCENE_BROWSER_ROW_PITCH
    );
    assert_ne!(world.get::<SceneEditorSceneRow>(first).unwrap().item, original);
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorSceneRow>>()
            .iter(world)
            .count(),
        SCENE_BROWSER_ROW_POOL_COUNT
    );
    assert!(
        rows.iter()
            .all(|entity| world.get::<SceneEditorSceneRow>(*entity).is_some())
    );
    assert_eq!(world.get::<ScrollPosition>(root).unwrap().y, 0.0);
    assert_eq!(
        world.resource::<SceneEditorState>().selected_scene_id,
        Some(SceneId::parse("MAP-1").unwrap())
    );
    assert_eq!(
        world.resource::<SceneBrowserState>().selected_item,
        Some(scene_item("MAP-1"))
    );
    assert!(!world.resource::<SceneFolderPersistence>().busy());
    let stopped = world.get::<ScrollPosition>(tree).unwrap().y;
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    app.update();
    assert_eq!(app.world().get::<ScrollPosition>(tree).unwrap().y, stopped);
}

#[test]
fn multi_selection_highlights_every_member_across_pool_rebinding() {
    let (mut app, _, _) = page();
    {
        let mut browser = app.world_mut().resource_mut::<SceneBrowserState>();
        browser.select_only(Some(scene_item("MAP-1")));
        browser.toggle_selection(scene_item("MAP-2"));
        browser.toggle_selection(scene_item("MAP-100"));
    }
    settle(&mut app);
    let world = app.world_mut();
    let rows: Vec<_> = world
        .query_filtered::<Entity, With<SceneEditorSceneRow>>()
        .iter(world)
        .collect();
    for (row, background) in world
        .query::<(&SceneEditorSceneRow, &BackgroundColor)>()
        .iter(world)
    {
        if let Some(item) = row.item.as_ref() {
            let selected = matches!(item, SceneBrowserItem::Scene(id) if ["MAP-1", "MAP-2"].contains(&id.as_str()));
            assert_eq!(
                background.0,
                crate::ui::scene_sidebar::tree_row_colors(selected, false, UiInteraction::None).0
            );
        }
    }
    let tree = world
        .query_filtered::<Entity, With<SceneEditorSceneTreeScroll>>()
        .single(world)
        .unwrap();
    world.get_mut::<ScrollPosition>(tree).unwrap().y = SCENE_BROWSER_ROW_PITCH * 98.0;
    settle(&mut app);
    let world = app.world_mut();
    let (_, background) = world
        .query::<(&SceneEditorSceneRow, &BackgroundColor)>()
        .iter(world)
        .find(|(row, _)| row.item == Some(scene_item("MAP-100")))
        .unwrap();
    assert_eq!(
        background.0,
        crate::ui::scene_sidebar::tree_row_colors(true, false, UiInteraction::None).0
    );
    assert!(
        rows.iter()
            .all(|row| world.get::<SceneEditorSceneRow>(*row).is_some())
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorSceneRow>>()
            .iter(world)
            .count(),
        SCENE_BROWSER_ROW_POOL_COUNT
    );
    assert_eq!(world.resource::<SceneBrowserState>().selection_count(), 3);
}
