//! 项目CJK字体下检查窄栏、缩放、DPI与固定行池；不替代原生窗口视觉验收。

use super::*;
use crate::components::SceneEditorActionButton;
use crate::types::EditorAction;

#[test]
fn navigation_tree_and_properties_each_use_the_full_column_height() {
    let (mut app, host) = page(1);
    select(&mut app, false);
    let before = author(&app);
    let world = app.world_mut();
    world.resource_mut::<crate::ui::sidebars::SceneEditorSidebarState>().navigator = crate::ui::sidebars::EditorNavigatorTab::SceneTree;
    world.get_mut::<Node>(host).unwrap().height = px(800);
    let tree = world.query_filtered::<Entity, With<InspectorTree>>().single(world).unwrap();
    let mut tree_host = tree;
    while let Some(parent) = world.get::<ChildOf>(tree_host) { tree_host = parent.parent(); }
    *world.get_mut::<Node>(tree_host).unwrap() = Node { width: px(340), height: px(800), flex_direction: FlexDirection::Column, ..default() };
    settle(&mut app);
    let world = app.world_mut();
    let body = world.query_filtered::<Entity, With<InspectorBody>>().single(world).unwrap();
    // 左树和右属性不再按42/58分割；同一窗口高度分别拥有完整、独立的滚动视口。
    for entity in [tree, body] {
        assert!(world.get::<ComputedNode>(entity).unwrap().size().y >= 790.0);
        assert_eq!(world.get::<Node>(entity).unwrap().flex_grow, 1.0);
    }
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
        "网格水体",
    ] {
        assert_eq!(
            world
                .query::<(Entity, &Text)>()
                .iter(world)
                .filter(|(entity, text)| {
                    if text.as_str() != label { return false; }
                    if !matches!(label, "位置" | "缩放" | "变换") { return true; }
                    // 隐藏的网格面板也有XYZ行；只在摆放子树检查同名组标签，避免误报重复。
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
    let locate = world
        .query::<(Entity, &SceneEditorActionButton)>()
        .iter(world)
        .find(|(_, action)| action.0 == EditorAction::FocusSelected)
        .unwrap()
        .0;
    let [duplicate, delete] = [
        PlacementTransformAction::Duplicate,
        PlacementTransformAction::Delete,
    ]
    .map(|action| {
        world
            .query::<(Entity, &SceneEditorPlacementTransformActionButton)>()
            .iter(world)
            .find(|(_, marker)| marker.0 == action)
            .unwrap()
            .0
    });
    let row = world.get::<ChildOf>(locate).unwrap().parent();
    assert_eq!(world.get::<ChildOf>(duplicate).unwrap().parent(), row);
    assert_eq!(world.get::<ChildOf>(delete).unwrap().parent(), row);
    for entity in [locate, duplicate, delete] {
        let node = world.get::<Node>(entity).unwrap();
        assert_eq!(node.height, px(26));
        assert_eq!(node.align_items, AlignItems::Center);
        assert_eq!(node.justify_content, JustifyContent::Center);
    }
    assert_eq!(
        world.get::<BackgroundColor>(delete).unwrap().0,
        crate::ui::theme::DANGER_BG
    );
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
    assert_eq!(fields.len(), 20); // 七个变换、四个洞口、七个立方体和两个网格水字段均保持唯一输入树。
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
                    if world
                        .get::<PlacementTransformField>(*entity)
                        .is_some_and(|field| field.is_cutout() || field.is_cuboid())
                    {
                        assert_eq!(computed.size(), Vec2::ZERO); // 普通GLB的洞口及内置形状字段保留身份，但不占布局。
                    } else {
                        let axis = world.get::<PlacementTransformField>(*entity).is_some_and(|field| matches!(field,
                            PlacementTransformField::TranslationX | PlacementTransformField::TranslationY | PlacementTransformField::TranslationZ
                            | PlacementTransformField::ScaleX | PlacementTransformField::ScaleY | PlacementTransformField::ScaleZ));
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
                    20
                );
            }
        }
    }
    assert_eq!(author(&app), before);
}

#[test]
fn inspector_tree_keeps_eight_rows_and_real_page_heights_in_low_window() {
    for count in [0, 1, 8, 9] {
        let (mut app, host) = page(count);
        app.world_mut().get_mut::<Node>(host).unwrap().height = px(160);
        settle(&mut app);
        let world = app.world_mut();
        assert_eq!(
            world
                .query_filtered::<Entity, With<SceneEditorGltfBrowserRow>>()
                .iter(world)
                .count(),
            8
        );
        let list = world
            .query_filtered::<&Node, With<SceneEditorGltfBrowserList>>()
            .single(world)
            .unwrap();
        let rows = count.min(8) as f32;
        assert_eq!(list.height, px(rows * feathrise_editor_ui::metrics::ROW_HEIGHT + (rows - 1.0).max(0.0) * 2.0));
        assert_eq!(
            list.display,
            if count == 0 {
                Display::None
            } else {
                Display::Flex
            }
        );
        let pager = world
            .query_filtered::<&Node, With<SceneEditorGltfBrowserPagination>>()
            .single(world)
            .unwrap();
        assert_eq!(
            pager.display,
            if count > 8 {
                Display::Flex
            } else {
                Display::None
            }
        );
        let scroll = world
            .query_filtered::<&Node, With<InspectorTree>>()
            .single(world)
            .unwrap();
        assert_eq!(scroll.overflow.y, OverflowAxis::Scroll);
        assert_eq!(scroll.min_height, px(0));
        if count == 9 {
            world.resource_mut::<EditorGltfBrowserState>().page = 1;
            settle(&mut app);
            let world = app.world_mut();
            let list = world
                .query_filtered::<&Node, With<SceneEditorGltfBrowserList>>()
                .single(world)
                .unwrap();
            assert_eq!(list.height, px(feathrise_editor_ui::metrics::ROW_HEIGHT));
            assert_eq!(
                world
                    .query::<&SceneEditorGltfBrowserRow>()
                    .iter(world)
                    .filter(|row| row.entry_index.is_some())
                    .count(),
                1
            );
        }
    }
}
