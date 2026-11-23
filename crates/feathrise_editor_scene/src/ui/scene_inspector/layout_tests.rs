//! 项目CJK字体下检查窄栏、缩放、DPI与固定行池；不替代原生窗口视觉验收。

use super::*;
use crate::ui::continuous_list::pool_capacity;
use crate::ui::list_layout::SCENE_OBJECT_ROW_PITCH;

fn set_tree_height(app: &mut App, height: f32) -> Entity {
    let world = app.world_mut();
    let tree = world.query_filtered::<Entity, With<InspectorTree>>().single(world).unwrap();
    let mut host = tree;
    while let Some(parent) = world.get::<ChildOf>(host) { host = parent.parent(); }
    *world.get_mut::<Node>(host).unwrap() = Node {
        width: px(340), height: px(height), flex_direction: FlexDirection::Column, ..default()
    };
    tree
}

#[test]
fn navigation_tree_and_properties_each_use_the_full_column_height() {
    let (mut app, host) = page(1);
    select(&mut app, false);
    let before = author(&app);
    let world = app.world_mut();
    world.resource_mut::<crate::ui::sidebars::SceneEditorSidebarState>().navigator = crate::ui::sidebars::EditorNavigatorTab::SceneTree;
    world.get_mut::<Node>(host).unwrap().height = px(800);
    let tree = set_tree_height(&mut app, 800.0);
    settle(&mut app);
    let world = app.world_mut();
    let body = world.query_filtered::<Entity, With<InspectorBody>>().single(world).unwrap();
    // 左列包含固定工具区和场景树，仍与右侧属性各自使用完整栏高。
    let tree_column = world.get::<ChildOf>(tree).unwrap().parent();
    for entity in [tree_column, body] {
        assert!(world.get::<ComputedNode>(entity).unwrap().size().y >= 790.0);
        assert_eq!(world.get::<Node>(entity).unwrap().flex_grow, 1.0);
    }
    assert_eq!(world.get::<Node>(tree).unwrap().flex_grow, 1.0);
    assert_eq!(author(&app), before);
}

#[test]
fn inspector_tree_and_properties_have_independent_scroll_hosts() {
    let (mut app, _) = page(1);
    select(&mut app, false);
    let before = author(&app);
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorRightPanelScroll>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(world.query_filtered::<Entity, With<crate::components::SceneEditorLeftPanelScroll>>().iter(world).count(), 1);
    for label in [
        "变换",
        "高度与落点",
        "位置",
        "旋转 Y",
        "缩放",
        "网格水体（实验）",
    ] {
        assert_eq!(
            world
                .query::<(Entity, &Text)>()
                .iter(world)
                .filter(|(entity, text)| {
                    if text.as_str() != label { return false; }
                    if !matches!(label, "位置" | "缩放" | "变换" | "高度与落点") { return true; }
                    // 应用菜单和隐藏的源节点属性也有同名文字；核对当前摆放面板参与布局的标签。
                    if world.get::<ComputedNode>(*entity).is_none_or(|node| node.size().min_element() <= 0.0) {
                        return false;
                    }
                    let mut ancestor = *entity;
                    loop {
                        if world.get::<SceneEditorPlacementTransformPanel>(ancestor).is_some() { return true; }
                        let Some(parent) = world.get::<ChildOf>(ancestor) else { return false; };
                        ancestor = parent.parent();
                    }
                })
                .count(),
            1,
            "检查器应保留唯一标签：{label}"
        );
    }
    assert_eq!(world.query::<&crate::ui::sidebars::SceneEditorSidebarHelpButton>()
        .iter(world).filter(|button| button.0 == crate::ui::sidebars::EditorSidebarHelp::Inspector).count(), 1,
        "完整来源通过唯一问号浮层查看，正文不再保留整行展开标签");
    assert!(world.query::<&Text>().iter(world)
        .all(|text| !matches!(text.as_str(), "定位" | "复制" | "删除")),
        "检查器只保留对象属性动作");
    let quarter = world
        .query::<(&SceneEditorPlacementTransformActionButton, &Node)>()
        .iter(world)
        .find(|(marker, _)| marker.0 == PlacementTransformAction::RotateQuarterTurn)
        .unwrap()
        .1;
    assert_eq!(quarter.display, Display::None); // 普通实例不展示不适用的建筑专用动作。
    assert_eq!(author(&app), before);
}

#[test]
fn inspector_reflow_keeps_numeric_entities_specs_values_and_active_buffer() {
    let (mut app, host) = page(1);
    select(&mut app, true);
    app.world_mut().resource_mut::<SceneEditorState>().status =
        "失败反馈中的完整中文路径仍应可读。".repeat(12);
    let world = app.world_mut();
    let fields = world.query_filtered::<(Entity, &NumericInputConfig, &NumericInputValue), With<NumericInputRoot>>()
        .iter(world).map(|(entity, config, value)| (entity, *config, *value)).collect::<Vec<_>>();
    assert_eq!(fields.len(), 27); // 动画三轴旋转和静态字段在隐藏时也保留实体身份。
    let active = world
        .query::<(Entity, &PlacementTransformField)>()
        .iter(world)
        .find(|(_, field)| **field == PlacementTransformField::TranslationX)
        .unwrap()
        .0;
    world.resource_mut::<ActiveNumericInput>().entity = Some(active);
    let mut edit = world.get_mut::<NumericInputEditState>(active).unwrap();
    edit.editing = true;
    edit.buffer = "-123.456".into();
    let before = author(&app);
    let rotation_xyz = crate::authoring::placement_pose::placement_supports_full_rotation();
    for width in [260.0, 320.0, 420.0, 560.0] {
        app.world_mut().get_mut::<Node>(host).unwrap().width = px(width);
        for scale in [0.6, 1.0, 1.5] {
            app.world_mut().resource_mut::<UiScale>().0 = scale;
            for dpi in [1.0, 2.0] {
                let world = app.world_mut();
                for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                    camera.computed.target_info.as_mut().unwrap().scale_factor = dpi;
                }
                settle(&mut app);
                let world = app.world_mut();
                let host_node = world.get::<ComputedNode>(host).unwrap();
                assert!((host_node.size().x - width * scale * dpi).abs() <= 1.1);
                let center = world.get::<UiGlobalTransform>(host).unwrap().translation.x;
                let left = center - host_node.size().x / 2.0;
                let right = center + host_node.size().x / 2.0;
                for (entity, config, value) in &fields {
                    assert_eq!(
                        world.get::<NumericInputConfig>(*entity).unwrap().0,
                        config.0
                    );
                    assert_eq!(world.get::<NumericInputValue>(*entity).unwrap(), value);
                    let node = world.get::<Node>(*entity).unwrap();
                    assert_eq!(node.width, percent(100));
                    assert_eq!(node.min_width, px(0));
                    let computed = world.get::<ComputedNode>(*entity).unwrap();
                    let field = world.get::<PlacementTransformField>(*entity);
                    if field.is_some_and(|field| field.is_cutout() || field.is_cuboid() || field.is_lod()
                        || (field.animation_rotation_axis().is_some() && !rotation_xyz)
                        || (*field == PlacementTransformField::Yaw && rotation_xyz))
                    {
                        assert_eq!(computed.size(), Vec2::ZERO); // 当前 SDK 和对象类型决定可见字段，隐藏项仍保留控件身份。
                    } else {
                        let axis = field.is_some_and(|field| field.translation_axis().is_some()
                            || field.animation_rotation_axis().is_some() || field.scale_axis().is_some());
                        let minimum_width = if axis { 55.0 } else { 160.0 };
                        assert!(computed.size().x > minimum_width * scale * dpi);
                        assert!(computed.size().x <= host_node.size().x + 1.1);
                    }
                }
                for axes in [
                    [PlacementTransformField::TranslationX, PlacementTransformField::TranslationY, PlacementTransformField::TranslationZ],
                    [PlacementTransformField::ScaleX, PlacementTransformField::ScaleY, PlacementTransformField::ScaleZ],
                ] {
                    let positions = axes.map(|axis| world.query::<(&PlacementTransformField, &UiGlobalTransform)>()
                        .iter(world).find(|(field, _)| **field == axis).unwrap().1.translation);
                    assert!((positions[0].y - positions[1].y).abs() <= 1.1);
                    assert!((positions[1].y - positions[2].y).abs() <= 1.1);
                    assert!(positions[0].x < positions[1].x && positions[1].x < positions[2].x);
                } // 同排只是布局变化，下面继续验证原焦点、完整缓冲与固定输入根数量。
                for (text, node, layout) in world.query::<(&Text, &ComputedNode, &TextLayout)>().iter(world) {
                    if node.size().x > 0.0 && matches!(text.as_str(), "位置" | "缩放") {
                        assert_eq!(layout.linebreak, bevy::text::LineBreak::NoWrap);
                        assert!(node.size().x + 1.1 >= 26.0 * scale * dpi);
                    }
                }
                for (entity, node, layout) in world
                    .query::<(Entity, &ComputedNode, &TextLayout)>()
                    .iter(world)
                {
                    if node.size().x == 0.0
                        || layout.linebreak != bevy::text::LineBreak::AnyCharacter
                    {
                        continue;
                    }
                    let center = world
                        .get::<UiGlobalTransform>(entity)
                        .unwrap()
                        .translation
                        .x;
                    assert!(center - node.size().x / 2.0 >= left - 1.1);
                    assert!(center + node.size().x / 2.0 <= right + 1.1);
                }
                assert_eq!(world.resource::<ActiveNumericInput>().entity, Some(active));
                assert_eq!(
                    world.get::<NumericInputEditState>(active).unwrap().buffer,
                    "-123.456"
                );
                assert_eq!(
                    world
                        .query_filtered::<Entity, With<NumericInputRoot>>()
                        .iter(world)
                        .count(),
                    27
                );
            }
        }
    }
    assert_eq!(author(&app), before);
}

#[test]
fn inspector_tree_reuses_rows_and_scrolls_to_the_last_object_in_low_window() {
    for count in [0, 1, 8, 24] {
        let (mut app, _) = page(count);
        let tree = set_tree_height(&mut app, 160.0);
        settle(&mut app);
        let world = app.world_mut();
        let tree_layout = world.get::<ComputedNode>(tree).unwrap();
        let tree_height = tree_layout.size().y * tree_layout.inverse_scale_factor();
        assert!(tree_height > 0.0 && tree_height <= 160.0);
        // 固定主 GLB 根行不占用滚动行池的槽位。
        assert_eq!(
            world
                .query_filtered::<Entity, (With<SceneEditorGltfBrowserRow>, Without<SceneTreeRootRow>)>()
                .iter(world)
                .count(),
            pool_capacity(tree_height, 1, SCENE_OBJECT_ROW_PITCH)
        );
        let list_entity = world
            .query_filtered::<Entity, With<SceneEditorGltfBrowserList>>()
            .single(world)
            .unwrap();
        let list = world.get::<Node>(list_entity).unwrap();
        let content_height = (count as f32 * SCENE_OBJECT_ROW_PITCH - 3.0).max(0.0);
        let visible_height = content_height.min(tree_height);
        assert_eq!(list.height, px(visible_height));
        assert_eq!(list.overflow.y, OverflowAxis::Scroll);
        assert_eq!(
            list.display,
            if count == 0 {
                Display::None
            } else {
                Display::Flex
            }
        );
        let content = world
            .query_filtered::<&Node, With<SceneEditorGltfBrowserListContent>>()
            .single(world)
            .unwrap();
        assert_eq!(content.height, px(content_height));
        let scroll = world.get::<Node>(tree).unwrap();
        assert_eq!(scroll.overflow.y, OverflowAxis::Scroll);
        assert_eq!(scroll.min_height, px(0));
        if count == 24 {
            let end = content_height - visible_height;
            world.get_mut::<ScrollPosition>(list_entity).unwrap().y = end + SCENE_OBJECT_ROW_PITCH;
            settle(&mut app);
            let world = app.world_mut();
            assert_eq!(world.get::<ScrollPosition>(list_entity).unwrap().y, end);
            let mut bindings = world.query::<(&SceneEditorGltfBrowserRow, &Node)>();
            assert!(bindings.iter(world).any(|(row, node)| row.entry_index == Some(count - 1)
                && node.top == px((count - 1) as f32 * SCENE_OBJECT_ROW_PITCH)));
            assert!(!bindings.iter(world).any(|(row, _)| row.entry_index == Some(0)));

            set_tree_height(&mut app, 120.0);
            app.world_mut().resource_mut::<TerrainShadingSession>().preview_expanded = true;
            settle(&mut app);
            let world = app.world_mut();
            let settings = world.query_filtered::<&ComputedNode, With<SceneBaseTerrainDetails>>().single(world).unwrap();
            assert!(settings.size().y > 0.0 && settings.content_size().y > settings.size().y);
            assert!(world.get::<ComputedNode>(tree).unwrap().size().y >= SCENE_OBJECT_ROW_PITCH);
        }
    }
}

#[test]
fn inspector_tree_shows_later_group_in_the_available_viewport_without_inner_scrolling() {
    let mut placements: Vec<_> = (0..10).map(placement).collect();
    std::sync::Arc::make_mut(&mut placements[9].editor).group = "后续组".into();
    let (mut app, _) = page_with_placements(placements);
    let tree = set_tree_height(&mut app, 720.0);
    settle(&mut app);
    let world = app.world_mut();
    let group_index = world.resource::<EditorGltfBrowserState>().entries.iter().position(|entry| {
        matches!(&entry.kind, EditorGltfBrowserEntryKind::Group { name, .. } if name == "后续组")
    }).unwrap();
    assert_eq!(group_index, 9); // 组排在九条现有条目之后，位于旧八行裁切范围之外。
    let list = world.query_filtered::<Entity, With<SceneEditorGltfBrowserList>>().single(world).unwrap();
    assert_eq!(world.get::<ScrollPosition>(list).unwrap().y, 0.0);
    let group = world.query::<(Entity, &SceneEditorGltfBrowserRow)>().iter(world)
        .find(|(_, row)| row.entry_index == Some(group_index)).unwrap().0;
    let vertical_extent = |entity| {
        let center = world.get::<UiGlobalTransform>(entity).unwrap().translation.y;
        let half_height = world.get::<ComputedNode>(entity).unwrap().size().y * 0.5;
        (center - half_height, center + half_height)
    };
    let (top, bottom) = vertical_extent(group);
    assert!(bottom > top);
    for viewport in [tree, list] {
        let (visible_top, visible_bottom) = vertical_extent(viewport);
        assert!(top >= visible_top && bottom <= visible_bottom);
    }
}

#[test]
fn inspector_tree_selection_keeps_the_measured_scroll_positions() {
    let (mut app, _) = page(24);
    app.init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>()
        .init_resource::<crate::resources::EditorViewportSelectionState>()
        .init_resource::<crate::viewport::SelectedPlacementViewportState>()
        .add_systems(Update, handle_gltf_browser_input.before(refresh_gltf_browser_cache));
    let tree = set_tree_height(&mut app, 160.0);
    settle(&mut app);
    let world = app.world_mut();
    let list = world.query_filtered::<Entity, With<SceneEditorGltfBrowserList>>().single(world).unwrap();
    let layout = world.get::<ComputedNode>(tree).unwrap();
    let outer = (layout.content_size().y - layout.size().y) * layout.inverse_scale_factor();
    assert!(outer > 0.0, "低窗必须同时出现外层与摆放列表滚动");
    let inner = 6.0 * SCENE_OBJECT_ROW_PITCH;
    for (entity, y) in [(tree, outer), (list, inner)] {
        world.get_mut::<ScrollPosition>(entity).unwrap().y = y;
    }
    settle(&mut app);

    for index in [7, 8] {
        let world = app.world_mut();
        let row = world.query::<(Entity, &SceneEditorGltfBrowserRow)>().iter(world)
            .find(|(_, row)| row.entry_index == Some(index)).unwrap().0;
        world.entity_mut(row).insert(EditorButtonClick::triggered());
        app.update();

        // 低窗同时滚动两级视区后，生产行点击只能改变所选对象。
        let world = app.world();
        let browser = world.resource::<EditorGltfBrowserState>();
        assert_eq!(browser.selected_placement_id, Some(format!("instance-{index}")));
        assert_eq!(world.get::<ScrollPosition>(tree).unwrap().y, outer);
        assert_eq!(world.get::<ScrollPosition>(list).unwrap().y, inner);
    }
}
