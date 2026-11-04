//! 真实中文字体与UI布局：竖向模式图标及旁侧菜单在窄视口不重叠、不撑出画面。

use super::*;
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::asset::AssetPlugin;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};

fn page(width: f32, scale: f32, dpi: f32) -> App {
    let mut app = App::new();
    let height = 600.0;
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
            ..default()
        },
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
        bevy::mesh::MeshPlugin,
        bevy::camera::visibility::VisibilityPlugin,
    ))
    .init_resource::<ViewportModeState>()
    .init_asset::<Image>() // 正式图标句柄与字形图集共用Image资产，不能只测文本节点矩形。
    .init_resource::<UiSurface>()
    .insert_resource(UiScale(scale))
    .add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
        PostUpdate,
    ))
    .add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
        PostUpdate,
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
            bevy::ui::widget::measure_text_system
                .after(bevy::text::detect_text_needs_rerender)
                .after(bevy::text::load_font_assets_into_font_collection),
            ui_layout_system,
            bevy::ui::widget::text_system, // 与正式UI一致生成实际字形，居中断言必须检查渲染坐标。
        )
            .chain(),
    )
    .add_systems(Update, (setup, style).chain());
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(width as u32, height as u32),
                    scale_factor: dpi,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::new(width as u32, height as u32),
                ..default()
            }),
            ..default()
        },
    ));
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    app.world_mut().spawn((
        MapEditorViewport,
        Node {
            width: px(width / (scale * dpi)),
            height: px(height / (scale * dpi)),
            ..default()
        },
    ));
    app.update();
    let world = app.world_mut();
    for mut text_font in world.query::<&mut TextFont>().iter_mut(world) {
        text_font.font = font.clone().into();
    }
    for _ in 0..4 {
        app.update();
    }
    app
}

#[test]
fn vertical_mode_icons_and_context_menu_fit_narrow_viewports_and_real_font() {
    for (width, scale, dpi) in [
        (240.0, 1.0, 1.0),
        (360.0, 1.0, 1.0),
        (760.0, 0.6, 1.0),
        (760.0, 1.5, 1.0),
        (960.0, 1.0, 2.0),
        (528.0, 1.7, 1.0),
        (544.0, 1.7, 1.0),
        (560.0, 1.7, 1.0),
        (600.0, 1.7, 1.0),
        (640.0, 2.0, 1.0),
        (544.0, 1.36, 1.25), // 同一物理倍率由独立界面缩放与非整数DPI共同组成。
    ] {
        let mut app = page(width, scale, dpi);
        let types = {
            let world = app.world_mut();
            world.query::<(&Text, &ChildOf)>().iter(world)
                .find(|(text, _)| text.as_str() == "笔刷类型")
                .unwrap().1.parent()
        };
        for mode in [
            ViewportMode::Select,
            ViewportMode::Rotate,
            ViewportMode::Scale,
            ViewportMode::Edit,
        ] {
            app.world_mut().resource_mut::<ViewportModeState>().mode = mode;
            // 本回归只装布局系统；模拟三维编辑时类型栏的实际可见状态。
            app.world_mut().get_mut::<Node>(types).unwrap().display =
                if mode == ViewportMode::Edit { Display::Flex } else { Display::None };
            app.update();
            let world = app.world_mut();
            let (panel, panel_transform) = world
                .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<ControlsRoot>>()
                .single(world)
                .unwrap();
            let left = panel_transform.translation.x - panel.size().x * 0.5;
            let right = panel_transform.translation.x + panel.size().x * 0.5;
            let top = panel_transform.translation.y - panel.size().y * 0.5;
            let bottom = panel_transform.translation.y + panel.size().y * 0.5;
            let (rail_node, rail_transform) = world
                .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<ModeToolsRoot>>()
                .single(world).unwrap();
            let rail = Rect::from_center_size(rail_transform.translation, rail_node.size());
            assert!((rail.width() - MODE_RAIL_WIDTH * scale * dpi).abs() <= 1.1);
            assert!(rail.min.x >= 0.0 && rail.max.x <= left - 1.0, "模式图标与筛选/笔刷菜单应并排且不重叠");
            assert!(rail.min.y >= 0.0 && rail.max.y <= 600.0 + 1.0);
            assert!(
                left >= 0.0 && right <= width + 1.0,
                "panel outside viewport: {left}..{right}, width={width}"
            );
            let mut count = 0;
            for (button, node, position, children) in world
                .query::<(&ModeButton, &ComputedNode, &UiGlobalTransform, &Children)>()
                .iter(world)
            {
                if node.size().min_element() <= 0.0 {
                    continue;
                }
                if matches!(button, ModeButton::Mode(_)) {
                    count += 1;
                    assert!((node.size().x - MODE_BUTTON_SIZE * scale * dpi).abs() <= 1.1);
                    assert!((node.size().y - MODE_BUTTON_SIZE * scale * dpi).abs() <= 1.1);
                }
                let (left, right, top, bottom) = if matches!(button, ModeButton::Mode(_)) {
                    (rail.min.x, rail.max.x, rail.min.y, rail.max.y)
                } else { (left, right, top, bottom) };
                let button_left = position.translation.x - node.size().x * 0.5;
                let button_right = position.translation.x + node.size().x * 0.5;
                assert!(button_left >= left - 1.0 && button_right <= right + 1.0);
                let button_top = position.translation.y - node.size().y * 0.5;
                let button_bottom = position.translation.y + node.size().y * 0.5;
                assert!(
                    button_top >= top - 1.0 && button_bottom <= bottom + 1.0,
                    "折行按钮必须完整位于面板背景内：{button_top}..{button_bottom}，面板{top}..{bottom}，{mode:?}/{width}/{scale}/{dpi}"
                );
                for child in children.iter() {
                    if world.get::<ImageNode>(child).is_some() {
                        let icon = world.get::<ComputedNode>(child).unwrap();
                        assert!((icon.size().x - 20.0 * scale * dpi).abs() <= 1.1);
                        assert!((icon.size().y - 20.0 * scale * dpi).abs() <= 1.1);
                        assert!(world.get::<Text>(child).is_none(), "模式由透明线框图标表示，名称与真实键位移到悬停提示");
                    }
                    if let Some(text) = world.get::<bevy::text::TextLayoutInfo>(child) {
                        assert!(!text.glyphs.is_empty(), "中文按钮必须生成实际字形");
                        assert!(
                            text.size.x * text.scale_factor <= node.size().x + 1.0,
                            "Chinese label wider than button"
                        );
                        // 沿用UI渲染器的内容框原点与字形四边形位置，不能只检查文本节点居中。
                        let text_node = world.get::<ComputedNode>(child).unwrap();
                        let transform = world.get::<UiGlobalTransform>(child).unwrap().affine();
                        let (mut glyph_left, mut glyph_right) = (f32::INFINITY, f32::NEG_INFINITY);
                        for glyph in &text.glyphs {
                            let origin = text_node.content_box().min + glyph.position;
                            let half_size = glyph.atlas_info.rect.size() * 0.5;
                            glyph_left = glyph_left.min(transform.transform_point2(origin - half_size).x);
                            glyph_right = glyph_right.max(transform.transform_point2(origin + half_size).x);
                        }
                        assert!(
                            ((glyph_left + glyph_right) * 0.5 - position.translation.x).abs()
                                <= 1.1 * (scale * dpi).max(1.0),
                            "实际字形应在按钮内居中：{}，字形范围{glyph_left}..{glyph_right}，按钮中心{}，{width}/{scale}/{dpi}",
                            world.get::<Text>(child).unwrap().as_str(),
                            position.translation.x,
                        );
                    }
                }
            }
            assert_eq!(count, 5);
            let mut last_bottom = rail.min.y;
            let rail_entity = world.query_filtered::<Entity, With<ModeToolsRoot>>().single(world).unwrap();
            for button in world.get::<Children>(rail_entity).unwrap().iter() {
                let node = world.get::<ComputedNode>(button).unwrap();
                let transform = world.get::<UiGlobalTransform>(button).unwrap();
                let rect = Rect::from_center_size(transform.translation, node.size());
                assert!(rect.min.y >= last_bottom - 1.0, "五种模式应按顺序纵向排列");
                last_bottom = rect.max.y;
            }
            if mode == ViewportMode::Edit {
                let type_node = world.get::<ComputedNode>(types).unwrap();
                let type_transform = world.get::<UiGlobalTransform>(types).unwrap();
                assert!(
                    type_transform.translation.x - type_node.size().x * 0.5 >= rail.max.x,
                    "笔刷类型保留在旁侧菜单，不覆盖竖向图标栏"
                );
                for child in world.get::<Children>(types).unwrap().iter() {
                    let node = world.get::<ComputedNode>(child).unwrap();
                    let position = world.get::<UiGlobalTransform>(child).unwrap();
                    assert!(node.size().x > 0.0);
                    assert!(position.translation.x - node.size().x * 0.5 >= left - 1.0);
                    assert!(position.translation.x + node.size().x * 0.5 <= right + 1.0);
                    assert!(position.translation.y - node.size().y * 0.5 >= top - 1.0);
                    assert!(position.translation.y + node.size().y * 0.5 <= bottom + 1.0);
                    if let Some(children) = world.get::<Children>(child) {
                        for text in children.iter() {
                            if let Some(layout) = world.get::<bevy::text::TextLayoutInfo>(text) {
                                assert!(
                                    layout.size.x <= node.size().x + 1.0,
                                    "笔刷类型文字不应超出按钮"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn mesh_element_toolbar_wraps_beside_the_shared_transform_rail() {
    use crate::authoring::mesh_edit::MeshEditState;
    use crate::viewport::modes::{MeshEditToolbarRoot, mesh_edit_ui};
    for (width, scale, dpi) in [(240.0, 1.0, 1.0), (640.0, 2.0, 1.0), (760.0, 1.5, 1.0)] {
        let mut app = page(width, scale, dpi);
        app.init_resource::<MeshEditState>();
        app.add_systems(Update, mesh_edit_ui::style);
        app.world_mut().resource_mut::<MeshEditState>().activate_for_test();
        for _ in 0..4 { app.update(); }
        let world = app.world_mut();
        let toolbar = world.query_filtered::<Entity, With<MeshEditToolbarRoot>>().single(world).unwrap();
        let node = world.get::<ComputedNode>(toolbar).unwrap();
        let position = world.get::<UiGlobalTransform>(toolbar).unwrap();
        let rect = Rect::from_center_size(position.translation, node.size());
        assert!(rect.min.x >= 0.0 && rect.max.x <= width + 1.0, "元素工具栏不能越出视口");
        let (rail, rail_node, rail_position) = world.query_filtered::<(&Node, &ComputedNode, &UiGlobalTransform), With<ModeToolsRoot>>().single(world).unwrap();
        assert_eq!(rail.display, Display::Flex, "点边面编辑沿用原选择与变换工具位置");
        let rail_rect = Rect::from_center_size(rail_position.translation, rail_node.size());
        assert!(rect.min.x >= rail_rect.max.x, "元素栏在竖栏右侧折行，不能覆盖原工具");
        for (button, node) in world.query::<(&ModeButton, &Node)>().iter(world) {
            if let ModeButton::Mode(mode) = button {
                assert_eq!(node.display, if *mode != ViewportMode::Edit {
                    Display::Flex
                } else { Display::None });
            }
        }
        let mut visible = Vec::new();
        for button in world.get::<Children>(toolbar).unwrap().iter() {
            let node = world.get::<ComputedNode>(button).unwrap();
            if node.size().min_element() <= 0.0 { continue; }
            let position = world.get::<UiGlobalTransform>(button).unwrap();
            let button_rect = Rect::from_center_size(position.translation, node.size());
            assert!(button_rect.min.x >= rect.min.x - 1.0 && button_rect.max.x <= rect.max.x + 1.0);
            assert!(button_rect.min.y >= rect.min.y - 1.0 && button_rect.max.y <= rect.max.y + 1.0);
            let Some(children) = world.get::<Children>(button) else {
                assert_eq!(world.get::<Text>(button).unwrap().as_str(), "已选 8 点");
                continue; // 计数是同一工具栏内的只读正文，不把它当作按钮。
            };
            let child = children.iter().next().unwrap();
            let text = world.get::<Text>(child).unwrap();
            let layout = world.get::<bevy::text::TextLayoutInfo>(child).unwrap();
            assert!(!layout.glyphs.is_empty(), "元素按钮需要真实中文与数字字形");
            assert!(layout.size.x * layout.scale_factor <= node.size().x + 1.1);
            visible.push(text.as_str().to_owned());
        }
        assert_eq!(visible, ["返回对象 Tab", "点 1", "边 2", "面 3", "全选 A", "框选 B", "更多"]);
    }
}

#[test]
fn mesh_single_face_and_advanced_actions_wrap_within_the_same_toolbar() {
    use crate::authoring::mesh_edit::{MeshEditState, MeshElementKind};
    use crate::viewport::modes::{MeshEditToolbarRoot, mesh_edit_ui};
    for (width, scale, dpi) in [(240.0, 1.0, 1.0), (640.0, 2.0, 1.0), (760.0, 1.5, 1.0), (1100.0,1.0,1.0)] {
        let mut app = page(width, scale, dpi);
        app.init_resource::<MeshEditState>()
            .init_resource::<mesh_edit_ui::MeshAdvancedMenuState>()
            .init_resource::<MapEditorNumericScrubMouseGuard>()
            .insert_resource(MapEditorState { open: true, ..default() })
            .add_systems(Update, (mesh_edit_ui::input, mesh_edit_ui::style).chain());
        {
            let mut state = app.world_mut().resource_mut::<MeshEditState>();
            state.activate_for_test();
            state.select_for_test(MeshElementKind::Face, 1);
        }
        for _ in 0..4 { app.update(); }
        let toggle = {
            let world = app.world_mut();
            world.query_filtered::<Entity, With<mesh_edit_ui::MeshAdvancedToggle>>().single(world).unwrap()
        };
        app.world_mut().entity_mut(toggle).insert(feathrise_editor_ui::EditorButtonClick::triggered());
        for _ in 0..4 { app.update(); }
        let world = app.world_mut();
        let root = world.query_filtered::<Entity, With<MeshEditToolbarRoot>>().single(world).unwrap();
        let rect = Rect::from_center_size(world.get::<UiGlobalTransform>(root).unwrap().translation,
            world.get::<ComputedNode>(root).unwrap().size());
        assert!(rect.min.x >= 0.0 && rect.max.x <= width + 1.0);
        let menu = world.query_filtered::<Entity, With<mesh_edit_ui::MeshAdvancedMenuRoot>>().single(world).unwrap();
        assert_eq!(world.get::<Node>(menu).unwrap().display, Display::Flex);
        if width/(scale*dpi)>=610.0 {
            assert_eq!(world.get::<Node>(menu).unwrap().max_width,Val::Auto,"宽视口保留原整行高级区域");
            assert!((world.get::<ComputedNode>(menu).unwrap().size().x-(rect.width()-6.0*scale*dpi)).abs()<1.1);
        }
        let mut labels = Vec::new();
        for button in world.get::<Children>(menu).unwrap().iter() {
            let node = world.get::<ComputedNode>(button).unwrap();
            if node.size().min_element() <= 0.0 { continue; }
            let button_rect = Rect::from_center_size(world.get::<UiGlobalTransform>(button).unwrap().translation, node.size());
            assert!(button_rect.min.x >= rect.min.x - 1.0 && button_rect.max.x <= rect.max.x + 1.0);
            assert!(button_rect.min.y >= rect.min.y - 1.0 && button_rect.max.y <= rect.max.y + 1.0);
            let text = world.get::<Children>(button).unwrap().iter().next().unwrap();
            let layout = world.get::<bevy::text::TextLayoutInfo>(text).unwrap();
            assert!(!layout.glyphs.is_empty() && layout.size.x * layout.scale_factor <= node.size().x + 1.1);
            labels.push(world.get::<Text>(text).unwrap().as_str().to_owned());
        }
        assert_eq!(labels, ["内插面", "切割", "细分"]);
    }
}
