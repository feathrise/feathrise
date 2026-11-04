//! 真实BSN与中文字体布局回归，验证固定一排、视口矩形稳定、横滚与原共享输入桥；不创建地图或GPU视口。

use super::{components::*, viewport_toolbar};
use crate::authoring::EditorAuthoringSession;
use crate::components::{MapEditorActionButton, MapEditorBrushSlider, MapEditorBrushSliderKind};
use crate::resources::{MapBrowserDragState, MapEditorNumericScrubMouseGuard, MapEditorState};
use crate::types::{EditorAction, EditorTool, EditorView};
use crate::ui::brush_controls::{
    BrushNumericInputState, configure_brush_slider_focus, handle_brush_numeric_inputs,
    sync_brush_sliders,
};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, EditorUiPlugin, EditorUiSystemSet,
    NumericInputConfig, NumericInputDisabled, NumericInputDisplayText, NumericInputEditState,
    NumericInputProgressFill, NumericInputScrubConfig, NumericInputScrubSensitivityText,
    NumericInputValue, NumericValue,
};
use feathrise_editor_core::EditorSettings;

#[derive(Component)]
struct TestViewport;

fn tick(app: &mut App) {
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
}

fn page(width: f32, scale: f32, dpi: f32, tool: EditorTool, domain_brush: bool) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
            ..default()
        },
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
        bevy::mesh::MeshPlugin,
        bevy::camera::visibility::VisibilityPlugin, // 复用真实层级显隐传播，否则无GPU夹具的InheritedVisibility始终为false。
    ));
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_plugins(EditorUiPlugin)
        .insert_resource(MapEditorState {
            open: true,
            tool,
            domain_brush_active: domain_brush,
            ..default()
        })
        .init_resource::<EditorSettings>()
        .init_resource::<BrushNumericInputState>()
        .init_resource::<EditorAuthoringSession>()
        .init_resource::<UiSurface>()
        .insert_resource(UiScale(scale));
    app.add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
        PostUpdate,
    ));
    app.add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
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
            bevy::ui::widget::measure_text_system.after(bevy::text::detect_text_needs_rerender),
            ui_layout_system,
        )
            .chain(),
    );
    app.add_systems(Startup, viewport_toolbar.spawn());
    app.add_systems(
        Update,
        (
            configure_brush_slider_focus,
            handle_brush_numeric_inputs,
            sync_brush_sliders,
            crate::ui::actions::sync_editor_action_button_styles,
            crate::viewport::snapping::sync_buttons,
        )
            .chain()
            .after(EditorUiSystemSet::NumericInput),
    );
    let font = app
        .world_mut()
        .resource_mut::<Assets<Font>>()
        .add(Font::from_bytes(
            feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
        ));
    app.world_mut().spawn((
        Camera2d,
        Camera {
            computed: ComputedCameraValues {
                target_info: Some(RenderTargetInfo {
                    physical_size: UVec2::new(4096, 2160),
                    scale_factor: dpi,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::new(4096, 2160),
                ..default()
            }),
            ..default()
        },
    ));
    tick(&mut app);
    let world = app.world_mut();
    let toolbar = world
        .query_filtered::<Entity, With<ViewportToolbar>>()
        .single(world)
        .unwrap();
    let host = world
        .spawn(Node {
            width: Val::Px(width / (scale * dpi)),
            height: Val::Px(500.0 / (scale * dpi)),
            min_width: Val::Px(0.0),
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .id();
    world.entity_mut(toolbar).insert(ChildOf(host));
    world.spawn((
        TestViewport,
        Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(0.0),
            flex_grow: 1.0,
            ..default()
        },
        ChildOf(host),
    ));
    // 同步装载真实中文字体，避免异步字体就绪速度掩盖窄窗字宽；不替换生产节点。
    for mut text_font in world.query::<&mut TextFont>().iter_mut(world) {
        text_font.font = font.clone().into();
    }
    for _ in 0..4 {
        tick(&mut app);
    }
    app
}

fn roots(app: &mut App) -> Vec<(MapEditorBrushSliderKind, Entity)> {
    let world = app.world_mut();
    let mut controls: Vec<_> = world
        .query::<(Entity, &MapEditorBrushSlider)>()
        .iter(world)
        .map(|(entity, field)| (field.0, entity))
        .collect();
    controls.sort_unstable_by_key(|(_, entity)| entity.to_bits()); // 显隐会迁移archetype，不能把查询顺序误当作根身份。
    controls
}

fn displayed_text(app: &mut App, root: Entity) -> String {
    let world = app.world_mut();
    world
        .query::<(&NumericInputDisplayText, &Text)>()
        .iter(world)
        .find(|(marker, _)| marker.root == root)
        .unwrap()
        .1
        .to_string()
}

fn first_view_button(world: &mut World) -> (Entity, Entity) {
    world.query::<(Entity, &Text, &ChildOf)>().iter(world)
        .find_map(|(text, label, parent)| (label.as_str() == "自由").then_some((parent.parent(), text)))
        .expect("真实工具条应保留自由视图按钮")
}

fn expected_fields(tool: EditorTool, domain_brush: bool) -> Vec<MapEditorBrushSliderKind> {
    use MapEditorBrushSliderKind::*;
    if domain_brush {
        return vec![Radius, Spacing];
    }
    match tool {
        EditorTool::Raise | EditorTool::Lower | EditorTool::Smooth | EditorTool::Flatten => vec![Radius, Strength, TerrainSpeed],
        EditorTool::VolumeDig | EditorTool::VolumeFill | EditorTool::VolumeSmooth | EditorTool::VolumeFlatten
        | EditorTool::Surface
        | EditorTool::Puddles
        | EditorTool::Vegetation
        | EditorTool::Water => vec![Radius, Strength],
        _ => Vec::new(),
    }
}

#[test]
fn toolbar_keeps_one_row_with_real_chinese_font_scale_and_dpi() {
    for (tool, domain_brush) in [
        (EditorTool::Inspect, false),
        (EditorTool::Raise, false),
        (EditorTool::Smooth, false),
        (EditorTool::Flatten, false),
        (EditorTool::Surface, false),
        (EditorTool::Puddles, false),
        (EditorTool::PlaceAsset, false),
        (EditorTool::Domain("fixture.domain"), true),
    ] {
        for (width, scale, dpi) in [240.0, 360.0, 760.0, 1200.0]
            .into_iter()
            .flat_map(|width| [0.6, 1.0, 1.5].map(|scale| (width, scale, 1.0)))
            .chain([(1100.0, 1.25, 2.0)])
        {
            let mut app = page(width, scale, dpi, tool, domain_brush);
            let controls = roots(&mut app);
            assert_eq!(controls.len(), 4);
            let world = app.world_mut();
            let toolbar = world.query_filtered::<(&Node, &ComputedNode, &UiGlobalTransform), With<ViewportToolbar>>().single(world).unwrap();
            assert_eq!(toolbar.0.height, Val::Px(32.0));
            assert_eq!(toolbar.0.overflow, Overflow::scroll_x());
            assert!((toolbar.1.size().y - 32.0 * scale * dpi).abs() <= 1.01);
            let toolbar_left = toolbar.2.translation.x - toolbar.1.size().x * 0.5;
            let toolbar_bottom = toolbar.2.translation.y + toolbar.1.size().y * 0.5;
            let toolbar_content_right =
                toolbar_left + toolbar.1.content_size().x.max(toolbar.1.size().x);
            assert!(toolbar.1.size().x <= width + 1.01);
            let views = world
                .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<ViewportViewControls>>()
                .single(world)
                .unwrap();
            let views_bottom = views.1.translation.y + views.0.size().y * 0.5;
            let views_center = views.1.translation.y;
            let views_left = views.1.translation.x - views.0.size().x * 0.5;
            assert!((views.0.size().y - 26.0 * scale * dpi).abs() <= 1.01);
            // 创建统一位于顶部网格工具，视口工具栏从视图组开始。
            let views_content_right = views_left + views.0.content_size().x.max(views.0.size().x);
            assert!(views_content_right <= toolbar_content_right + 1.01);
            assert!((views_left - toolbar_left).abs() <= 1.01);
            assert!(!world.query::<&Text>().iter(world).any(|text| text.as_str() == "添加"));
            let (add, label) = first_view_button(world);
            let add_node = world.get::<ComputedNode>(add).unwrap();
            let add_transform = world.get::<UiGlobalTransform>(add).unwrap();
            let add_left = add_transform.translation.x - add_node.size().x * 0.5;
            let add_right = add_transform.translation.x + add_node.size().x * 0.5;
            let label_node = world.get::<ComputedNode>(label).unwrap();
            let label_transform = world.get::<UiGlobalTransform>(label).unwrap();
            assert!((add_node.size().y - 26.0 * scale * dpi).abs() <= 1.01);
            assert!((add_transform.translation.y - views_center).abs() <= 1.01);
            assert!(add_left >= toolbar_left - 1.01);
            assert!(add_right <= toolbar_left + width + 1.01);
            assert!(label_transform.translation.x - label_node.size().x * 0.5 >= add_left - 1.01);
            assert!(label_transform.translation.x + label_node.size().x * 0.5 <= add_right + 1.01);
            let parameters = world.query_filtered::<(&Node, &ComputedNode, &UiGlobalTransform), With<ViewportBrushParameters>>().single(world).unwrap();
            let fields = expected_fields(tool, domain_brush);
            assert_eq!(
                parameters.0.display,
                if fields.is_empty() {
                    Display::None
                } else {
                    Display::Flex
                }
            );
            assert_eq!(parameters.0.height, Val::Px(26.0));
            if fields.is_empty() {
                assert_eq!(parameters.1.size().y, 0.0);
            } else {
                assert!((parameters.1.size().y - 26.0 * scale * dpi).abs() <= 1.01);
                assert!((parameters.2.translation.y - views_center).abs() <= 1.01);
                assert!(
                    parameters.2.translation.x - parameters.1.size().x * 0.5
                        >= views_content_right + 8.0 * scale * dpi - 1.01
                );
            }
            assert!((toolbar_bottom - views_bottom - 6.0 * scale * dpi).abs() <= 1.01);
            assert_eq!(
                world
                    .query_filtered::<Entity, With<ViewportToolbarScroll>>()
                    .iter(world)
                    .count(),
                1
            ); // 只有整条工具栏滚动，不能保留旧的双排或嵌套滚动宿主。
            let viewport = world
                .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<TestViewport>>()
                .single(world)
                .unwrap();
            assert!(viewport.1.translation.y - viewport.0.size().y * 0.5 >= toolbar_bottom - 1.01);
            assert!(viewport.0.size().y > 100.0);
            let mut views_count = 0;
            for (button, computed, transform) in world
                .query::<(&MapEditorActionButton, &ComputedNode, &UiGlobalTransform)>()
                .iter(world)
            {
                assert!(matches!(
                    button.0,
                    EditorAction::ViewPerspective
                        | EditorAction::ViewTop
                        | EditorAction::ViewFront
                        | EditorAction::ViewSide
                        | EditorAction::ToggleOrthographic
                        | EditorAction::FocusSelected
                        | EditorAction::FrameMap
                ));
                views_count += 1;
                assert!((computed.size().y - 26.0 * scale * dpi).abs() <= 1.01);
                assert!(transform.translation.x - computed.size().x * 0.5 >= views_left - 1.01);
                assert!(
                    transform.translation.x + computed.size().x * 0.5 <= views_content_right + 1.01
                );
            }
            assert_eq!(views_count, 7); // 方向、投影与取景共用一行；移动属于视口模式组。
            let mut snap_positions: Vec<_> = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<crate::viewport::snapping::SnapButton>>()
                .iter(world).map(|(node, transform)| {
                    assert!((node.size().y - 26.0 * scale * dpi).abs() <= 1.01);
                    assert!((transform.translation.y - views_center).abs() <= 1.01);
                    assert!(node.size().x > 16.0 * scale * dpi);
                    (transform.translation.x - node.size().x * 0.5, transform.translation.x + node.size().x * 0.5)
                }).collect();
            assert_eq!(snap_positions.len(), 4);
            snap_positions.sort_by(|left, right| left.0.total_cmp(&right.0));
            assert!(snap_positions.windows(2).all(|pair| pair[0].1 <= pair[1].0 + 1.01));
            assert!(snap_positions[0].0 >= views_content_right + 8.0 * scale * dpi - 1.01);
            for (kind, entity) in controls {
                let computed = world.get::<ComputedNode>(entity).unwrap();
                let node = world.get::<Node>(entity).unwrap();
                assert_eq!(
                    world.get::<NumericInputDisabled>(entity).is_none(),
                    fields.contains(&kind)
                );
                assert_eq!(node.width, Val::Percent(100.0));
                if fields.contains(&kind) {
                    let transform = world.get::<UiGlobalTransform>(entity).unwrap();
                    assert!((computed.size().y - 26.0 * scale * dpi).abs() <= 1.01);
                    assert!(computed.size().x >= 160.0 * scale * dpi - 1.01);
                    assert!(computed.size().x <= 220.0 * scale * dpi + 1.01);
                    assert!(
                        transform.translation.x - computed.size().x * 0.5 >= toolbar_left - 1.01
                    );
                    assert!(
                        transform.translation.x + computed.size().x * 0.5
                            <= toolbar_content_right + 1.01
                    );
                    let children = world.get::<Children>(entity).unwrap();
                    let mut texts = 0;
                    for child in children.iter() {
                        if world.get::<NumericInputScrubSensitivityText>(child).is_some() {
                            assert_eq!(world.get::<Node>(child).unwrap().display, Display::None);
                            assert_eq!(world.get::<ComputedNode>(child).unwrap().size(), Vec2::ZERO);
                            continue;
                        } // 右拖的倍率读数平时不参与排版，不能把隐藏节点的原点当作可见文字中心。
                        if world.get::<Text>(child).is_some() {
                            texts += 1;
                            let child_computed = world.get::<ComputedNode>(child).unwrap();
                            let child_transform = world.get::<UiGlobalTransform>(child).unwrap();
                            assert!(
                                child_transform.translation.x - child_computed.size().x * 0.5
                                    >= transform.translation.x - computed.size().x * 0.5 - 1.01
                            );
                            assert!(
                                child_transform.translation.x + child_computed.size().x * 0.5
                                    <= transform.translation.x + computed.size().x * 0.5 + 1.01
                            );
                            assert!(
                                (child_transform.translation.y - transform.translation.y).abs()
                                    <= 1.01
                            );
                        }
                    }
                    assert_eq!(texts, 3); // 普通状态只排版标签、数值和单位，右拖读数保持零占位。
                    assert_eq!(
                        children.iter().filter(|child| world.get::<NumericInputScrubSensitivityText>(*child).is_some()).count(),
                        1,
                    );
                    assert_eq!(
                        children
                            .iter()
                            .filter(|child| world.get::<NumericInputProgressFill>(*child).is_some())
                            .count(),
                        1
                    );
                }
            }
        }
    }
}

#[test]
fn all_tool_contexts_keep_applicable_field_rules_and_stable_entities() {
    let mut app = page(360.0, 1.0, 1.0, EditorTool::Raise, false);
    let original = roots(&mut app);
    for tool in [
        EditorTool::Raise,
        EditorTool::Lower,
        EditorTool::Smooth,
        EditorTool::Flatten,
        EditorTool::VolumeDig,
        EditorTool::VolumeFill,
        EditorTool::VolumeSmooth,
        EditorTool::VolumeFlatten,
        EditorTool::Surface,
        EditorTool::Puddles,
        EditorTool::Vegetation,
        EditorTool::Water,
        EditorTool::Inspect,
        EditorTool::PlaceAsset,
        EditorTool::Portal,
        EditorTool::Boundary,
        EditorTool::Building,
        EditorTool::Domain("fixture.domain"),
        EditorTool::RiverSpline,
        EditorTool::Waterfall,
    ] {
        for domain_brush in [false, true] {
            {
                let mut editor = app.world_mut().resource_mut::<MapEditorState>();
                editor.tool = tool;
                editor.domain_brush_active = domain_brush;
            }
            tick(&mut app);
            let expected = expected_fields(tool, domain_brush);
            for (kind, entity) in &original {
                assert!(app.world().get_entity(*entity).is_ok());
                assert_eq!(
                    app.world().get::<NumericInputDisabled>(*entity).is_none(),
                    expected.contains(kind)
                );
            }
            assert_eq!(roots(&mut app), original);
        }
    }
    let original_values: Vec<_> = original
        .iter()
        .map(|(_, entity)| app.world().get::<NumericInputValue>(*entity).unwrap().0)
        .collect();
    app.world_mut().resource_mut::<UiScale>().0 = 1.5;
    for _ in 0..3 {
        tick(&mut app);
    }
    assert_eq!(roots(&mut app), original);
    for ((_, entity), value) in original.iter().zip(original_values) {
        assert_eq!(
            app.world().get::<NumericInputValue>(*entity).unwrap().0,
            value
        );
    }
}

#[test]
fn current_view_highlight_keeps_original_action_bindings() {
    let mut app = page(760.0, 1.0, 1.0, EditorTool::Inspect, false);
    let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let map_id = draft.map_id.clone();
    app.insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active));
    app.insert_resource(feathrise_map_content::ActiveDynamicMap { map_id: Some(map_id.clone()) });
    app.world_mut().resource_mut::<MapEditorState>().selected_map_id = Some(map_id.clone());
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft = Some(draft);
    app.insert_resource(crate::resources::MapEditorViewportState {
        _image: Handle::default(), camera: Entity::PLACEHOLDER,
        center: Vec3::ZERO, scene_center: Vec3::ZERO, scene_half_extents: Vec3::ONE,
        extent: 50.0, yaw: 0.0, pitch: 0.5, framed_map_id: Some(map_id),
        render_size: UVec2::new(800, 600),
    });
    for (view, action) in [
        (EditorView::Perspective, EditorAction::ViewPerspective),
        (EditorView::Top, EditorAction::ViewTop),
        (EditorView::Front, EditorAction::ViewFront),
        (EditorView::Side, EditorAction::ViewSide),
    ] {
        app.world_mut().resource_mut::<MapEditorState>().view = view;
        tick(&mut app);
        let world = app.world_mut();
        let mut selected = 0;
        for (button, background) in world
            .query::<(&MapEditorActionButton, &BackgroundColor)>()
            .iter(world)
        {
            assert_eq!(
                background.0,
                if button.0 == action {
                    crate::ui::theme::SELECT_BG
                } else if button.0 == EditorAction::FocusSelected {
                    crate::ui::theme::SLOT_BG_DISABLED // 当前没有确认对象，聚焦不能冒充普通可用按钮。
                } else {
                    crate::ui::theme::BUTTON_DEV_SMALL_NORMAL
                }
            );
            selected += usize::from(button.0 == action);
        }
        assert_eq!(selected, 1);
    }
}

#[test]
fn external_numeric_updates_use_the_same_shared_text_and_fill_root() {
    let mut app = page(760.0, 1.0, 1.0, EditorTool::Raise, false);
    let controls = roots(&mut app);
    let radius = controls
        .iter()
        .find(|(kind, _)| *kind == MapEditorBrushSliderKind::Radius)
        .unwrap()
        .1;
    let config = app
        .world()
        .get::<NumericInputScrubConfig>(radius)
        .unwrap()
        .clone();
    let input = app.world().get::<NumericInputConfig>(radius).unwrap().0;
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .brush_radius = 128.5;
    for _ in 0..2 {
        tick(&mut app);
    }
    assert_eq!(
        app.world().get::<NumericInputValue>(radius).unwrap().0,
        NumericValue::F32(128.5)
    );
    assert_eq!(displayed_text(&mut app, radius), "128.5");
    assert_eq!(
        app.world().get::<NumericInputConfig>(radius).unwrap().0,
        input
    );
    assert_eq!(
        *app.world().get::<NumericInputScrubConfig>(radius).unwrap(),
        config
    );
    assert_eq!(roots(&mut app), controls);
    assert!(
        app.world()
            .resource::<ActiveNumericInput>()
            .entity
            .is_none()
    );
    assert!(
        app.world()
            .resource::<ActiveNumericInputScrub>()
            .entity
            .is_none()
    );
}

fn point_at(app: &mut App, entity: Entity, normalized_x: f32) {
    app.world_mut().entity_mut(entity).insert((
        Interaction::Hovered,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::new(normalized_x, 0.5)),
        },
    ));
}

fn viewport_geometry(app: &mut App) -> (Vec2, Vec2) {
    let world = app.world_mut();
    let (computed, transform) = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<TestViewport>>()
        .single(world)
        .unwrap();
    (computed.size(), transform.translation)
}

#[test]
fn every_tool_switch_preserves_toolbar_height_and_map_rectangle() {
    for (width, scale, dpi) in [240.0, 360.0, 760.0, 1200.0]
        .into_iter()
        .flat_map(|width| [0.6, 1.0, 1.5].map(|scale| (width, scale, 1.0)))
        .chain([(1100.0, 1.25, 2.0)])
    {
        let mut app = page(width, scale, dpi, EditorTool::Raise, false);
        let original = viewport_geometry(&mut app);
        let original_roots = roots(&mut app);
        for tool in [
            EditorTool::Inspect,
            EditorTool::Domain("fixture.domain"),
            EditorTool::Raise,
            EditorTool::Lower,
            EditorTool::Smooth,
            EditorTool::Flatten,
            EditorTool::Water,
            EditorTool::RiverSpline,
            EditorTool::Waterfall,
            EditorTool::PlaceAsset,
            EditorTool::Building,
            EditorTool::Portal,
            EditorTool::Surface,
            EditorTool::Puddles,
            EditorTool::Vegetation,
            EditorTool::Boundary,
        ] {
            for domain_brush in [false, true] {
                {
                    let mut editor = app.world_mut().resource_mut::<MapEditorState>();
                    editor.tool = tool;
                    editor.domain_brush_active = domain_brush;
                }
                tick(&mut app);
                assert_eq!(viewport_geometry(&mut app), original); // 单帧就必须稳定，不能靠动画掩盖RTT矩形跳变。
                assert_eq!(roots(&mut app), original_roots);
                let world = app.world_mut();
                let height = world
                    .query_filtered::<&ComputedNode, With<ViewportToolbar>>()
                    .single(world)
                    .unwrap()
                    .size()
                    .y;
                assert!((height - 32.0 * scale * dpi).abs() <= 1.01);
            }
        }
    }
}

#[test]
fn single_row_scrolls_to_last_controls_and_consumes_wheel_during_numeric_capture() {
    let mut app = page(240.0, 1.5, 1.0, EditorTool::Raise, false);
    app.add_message::<MouseWheel>()
        .init_resource::<MapBrowserDragState>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .add_systems(
            Update,
            (
                crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard,
                crate::ui::systems::scroll_editor_side_panels,
            )
                .chain()
                .after(EditorUiSystemSet::NumericInput),
        );
    let speed = roots(&mut app)
        .into_iter()
        .find(|(kind, _)| *kind == MapEditorBrushSliderKind::TerrainSpeed)
        .unwrap()
        .1;
    let world = app.world_mut();
    let toolbar = world
        .query_filtered::<Entity, With<ViewportToolbar>>()
        .single(world)
        .unwrap();
    let last_view = world
        .query::<(Entity, &MapEditorActionButton)>()
        .iter(world)
        .find(|(_, action)| action.0 == EditorAction::ViewSide)
        .unwrap()
        .0;
    let toolbar_node = world.get::<ComputedNode>(toolbar).unwrap();
    let toolbar_right = world
        .get::<UiGlobalTransform>(toolbar)
        .unwrap()
        .translation
        .x
        + toolbar_node.size().x * 0.5;
    let view_right = world
        .get::<UiGlobalTransform>(last_view)
        .unwrap()
        .translation
        .x
        + world.get::<ComputedNode>(last_view).unwrap().size().x * 0.5;
    let view_scroll = ((view_right - toolbar_right) * toolbar_node.inverse_scale_factor()).max(0.0);
    let original = viewport_geometry(&mut app);
    for (distance, last_control) in [(view_scroll, last_view), (10_000.0, speed)] {
        point_at(&mut app, toolbar, 0.5);
        app.world_mut().write_message(MouseWheel {
            unit: MouseScrollUnit::Pixel,
            x: 0.0,
            y: -distance,
            window: Entity::PLACEHOLDER,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        tick(&mut app);
        let computed = app.world().get::<ComputedNode>(toolbar).unwrap();
        let maximum =
            (computed.content_size().x - computed.size().x) * computed.inverse_scale_factor();
        assert!(maximum > 0.0);
        assert!(
            (app.world().get::<ScrollPosition>(toolbar).unwrap().x - distance.min(maximum)).abs()
                <= 1.01
        );
        let transform = app.world().get::<UiGlobalTransform>(toolbar).unwrap();
        let control = app.world().get::<ComputedNode>(last_control).unwrap();
        let control_transform = app.world().get::<UiGlobalTransform>(last_control).unwrap();
        assert!(
            control_transform.translation.x - control.size().x * 0.5
                >= transform.translation.x - computed.size().x * 0.5 - 1.01
        );
        assert!(
            control_transform.translation.x + control.size().x * 0.5
                <= transform.translation.x + computed.size().x * 0.5 + 1.01
        );
        assert_eq!(viewport_geometry(&mut app), original); // 视图和参数由同一个横滚宿主访问，不挤压地图。
    }
    let scroll_before = app.world().get::<ScrollPosition>(toolbar).unwrap().x;
    point_at(&mut app, speed, 0.5);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Pixel,
        x: 0.0,
        y: 10_000.0,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    tick(&mut app);
    assert_eq!(
        app.world().resource::<ActiveNumericInputScrub>().entity,
        Some(speed)
    );
    assert_eq!(
        app.world().get::<ScrollPosition>(toolbar).unwrap().x,
        scroll_before
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Pixel,
        x: 0.0,
        y: 10_000.0,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    tick(&mut app);
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    assert_eq!(
        app.world().get::<ScrollPosition>(toolbar).unwrap().x,
        scroll_before
    );
    tick(&mut app);
    assert!(
        !app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    assert_eq!(
        app.world().get::<ScrollPosition>(toolbar).unwrap().x,
        scroll_before
    ); // 被隔离的滚轮已经消费，静默帧不能补放。
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Pixel,
        x: 0.0,
        y: 10_000.0,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    tick(&mut app);
    assert_eq!(app.world().get::<ScrollPosition>(toolbar).unwrap().x, 0.0);
    {
        let world = app.world_mut();
        let (add, _) = first_view_button(world);
        let bar_node = world.get::<ComputedNode>(toolbar).unwrap();
        let bar_transform = world.get::<UiGlobalTransform>(toolbar).unwrap();
        let add_node = world.get::<ComputedNode>(add).unwrap();
        let add_transform = world.get::<UiGlobalTransform>(add).unwrap();
        assert!(add_transform.translation.x - add_node.size().x * 0.5
            >= bar_transform.translation.x - bar_node.size().x * 0.5 - 1.01);
        assert!(add_transform.translation.x + add_node.size().x * 0.5
            <= bar_transform.translation.x + bar_node.size().x * 0.5 + 1.01);
    } // 窄窗大缩放下，从末端参数横滚回起点后，首个视图按钮仍能完整点击。
    app.world_mut().write_message(MouseWheel {
        unit: MouseScrollUnit::Pixel,
        x: 0.0,
        y: -10_000.0,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    });
    tick(&mut app);
    app.world_mut().resource_mut::<MapEditorState>().tool = EditorTool::Inspect;
    tick(&mut app);
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Node, With<ViewportBrushParameters>>()
            .single(world)
            .unwrap()
            .display,
        Display::None
    );
    assert!((world.get::<ComputedNode>(toolbar).unwrap().size().y - 48.0).abs() <= 1.01); // 当前1.5倍缩放下仍严格是一排32px。
    assert_eq!(viewport_geometry(&mut app), original);
}

#[test]
fn shared_left_right_and_middle_gestures_keep_realtime_values_and_independent_sensitivity() {
    let mut app = page(760.0, 1.0, 1.0, EditorTool::Raise, false);
    let controls = roots(&mut app);
    let radius = controls
        .iter()
        .find(|(kind, _)| *kind == MapEditorBrushSliderKind::Radius)
        .unwrap()
        .1;
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .brush_radius = 32.0;
    for _ in 0..2 {
        tick(&mut app);
    }
    let normalized_delta = 48.0 / app.world().get::<ComputedNode>(radius).unwrap().size().x;
    point_at(&mut app, radius, 0.5);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    tick(&mut app);
    assert_eq!(
        app.world().resource::<ActiveNumericInputScrub>().entity,
        Some(radius)
    );
    point_at(&mut app, radius, 0.5 + normalized_delta);
    tick(&mut app);
    assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 64.0); // 尚未松开就更新真实业务值。
    assert_eq!(displayed_text(&mut app, radius), "64.0");
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    tick(&mut app);
    assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 64.0);
    assert!(
        app.world()
            .resource::<ActiveNumericInputScrub>()
            .entity
            .is_none()
    );

    let settings = app
        .world()
        .resource::<EditorSettings>()
        .map_editor_scrub_sensitivities
        .clone();
    point_at(&mut app, radius, 0.5);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Right);
    tick(&mut app);
    point_at(&mut app, radius, 0.5 + normalized_delta);
    tick(&mut app);
    assert!(
        (app.world()
            .get::<NumericInputScrubConfig>(radius)
            .unwrap()
            .sensitivity
            - settings.brush_radius * 2.0)
            .abs()
            < 0.0001
    );
    assert_eq!(
        app.world()
            .resource::<EditorSettings>()
            .map_editor_scrub_sensitivities
            .brush_radius,
        settings.brush_radius
    );
    assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 64.0);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Right);
    tick(&mut app);
    let after = &app
        .world()
        .resource::<EditorSettings>()
        .map_editor_scrub_sensitivities;
    assert!((after.brush_radius - settings.brush_radius * 2.0).abs() < 0.0001);
    assert_eq!(after.brush_spacing, settings.brush_spacing);
    assert_eq!(after.brush_strength, settings.brush_strength);
    assert_eq!(after.terrain_speed, settings.terrain_speed);

    point_at(&mut app, radius, 0.5);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Middle);
    tick(&mut app);
    assert_eq!(
        app.world().resource::<ActiveNumericInput>().entity,
        Some(radius)
    );
    assert!(
        app.world()
            .get::<NumericInputEditState>(radius)
            .unwrap()
            .editing
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Middle);
    tick(&mut app);
    for key in [KeyCode::Digit9, KeyCode::Period, KeyCode::Digit5] {
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(key);
        tick(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(key);
        tick(&mut app);
    }
    assert_eq!(
        app.world()
            .get::<NumericInputEditState>(radius)
            .unwrap()
            .buffer,
        "9.5"
    );
    assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 64.0);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    tick(&mut app);
    assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 9.5);
    assert_eq!(displayed_text(&mut app, radius), "9.5");
    assert!(
        app.world()
            .resource::<ActiveNumericInput>()
            .entity
            .is_none()
    );
    assert_eq!(roots(&mut app), controls);
}

#[test]
fn smooth_and_flatten_share_editable_sculpt_speed_and_preserve_numeric_root() {
    let mut app = page(760.0, 1.0, 1.0, EditorTool::Smooth, false);
    let controls = roots(&mut app);
    let speed = controls.iter().find(|(kind, _)| *kind == MapEditorBrushSliderKind::TerrainSpeed).unwrap().1;
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    app.world_mut().resource_mut::<MapEditorState>().terrain_sculpt_speed = 12.0;
    for _ in 0..2 { tick(&mut app); }
    assert!(app.world().get::<NumericInputDisabled>(speed).is_none());
    let delta = 48.0 / app.world().get::<ComputedNode>(speed).unwrap().size().x;
    point_at(&mut app, speed, 0.5);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    tick(&mut app);
    point_at(&mut app, speed, 0.5 + delta);
    tick(&mut app);
    assert_eq!(app.world().resource::<MapEditorState>().terrain_sculpt_speed, 24.0);
    assert_eq!(displayed_text(&mut app, speed), "24.0");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    tick(&mut app);
    app.world_mut().resource_mut::<MapEditorState>().tool = EditorTool::Flatten;
    tick(&mut app);
    assert!(app.world().get::<NumericInputDisabled>(speed).is_none());
    // 压平复用平滑调整后的同一字段，继续通过真实中键字符输入提交，不能另建工具私有速度。
    point_at(&mut app, speed, 0.5);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Middle);
    tick(&mut app);
    assert_eq!(app.world().resource::<ActiveNumericInput>().entity, Some(speed));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Middle);
    tick(&mut app);
    for key in [KeyCode::Digit6, KeyCode::Period, KeyCode::Digit5] {
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(key);
        tick(&mut app);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(key);
        tick(&mut app);
    }
    assert_eq!(app.world().resource::<MapEditorState>().terrain_sculpt_speed, 24.0);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Enter);
    tick(&mut app);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Enter);
    tick(&mut app);
    assert_eq!(app.world().resource::<MapEditorState>().terrain_sculpt_speed, 6.5);
    for tool in [EditorTool::Raise, EditorTool::Lower, EditorTool::Smooth, EditorTool::Flatten] {
        app.world_mut().resource_mut::<MapEditorState>().tool = tool;
        tick(&mut app);
        assert_eq!(displayed_text(&mut app, speed), "6.5");
        assert_eq!(roots(&mut app), controls);
        assert!(app.world().get::<NumericInputDisabled>(speed).is_none());
    }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, revision);
    assert!(!app.world().resource::<MapEditorState>().dirty, "工具速度调整不属于地图作者修改");
}

#[test]
fn hidden_parameter_area_restores_raw_and_keeps_capture_until_release() {
    let mut app = page(360.0, 1.0, 1.0, EditorTool::Raise, false);
    let controls = roots(&mut app);
    let radius = controls
        .iter()
        .find(|(kind, _)| *kind == MapEditorBrushSliderKind::Radius)
        .unwrap()
        .1;
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .brush_radius = 32.0;
    for _ in 0..2 {
        tick(&mut app);
    }
    let delta = 48.0 / app.world().get::<ComputedNode>(radius).unwrap().size().x;
    point_at(&mut app, radius, 0.5);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    tick(&mut app);
    point_at(&mut app, radius, 0.5 + delta);
    tick(&mut app);
    assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 64.0);
    app.world_mut().resource_mut::<MapEditorState>().tool = EditorTool::PlaceAsset;
    for _ in 0..2 {
        tick(&mut app);
    }
    assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 32.0);
    assert_eq!(
        app.world().resource::<ActiveNumericInputScrub>().entity,
        Some(radius)
    ); // 业务已取消，但按住周期仍隔离鼠标，不能提前点透到地图。
    assert!(app.world().get::<NumericInputDisabled>(radius).is_some());
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Node, With<ViewportBrushParameters>>()
            .single(world)
            .unwrap()
            .display,
        Display::None
    ); // 参数组退出同排布局，原数值根已隐藏并禁用，外层工具栏高度不变。
    point_at(&mut app, radius, 0.5 + delta * 2.0);
    tick(&mut app);
    assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 32.0); // 隐藏后的迟到位移不能覆盖恢复值。
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    tick(&mut app);
    assert!(
        app.world()
            .resource::<ActiveNumericInputScrub>()
            .entity
            .is_none()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Middle);
    tick(&mut app);
    assert!(
        app.world()
            .resource::<ActiveNumericInput>()
            .entity
            .is_none()
    );
    assert_eq!(app.world().resource::<MapEditorState>().brush_radius, 32.0);
    assert_eq!(roots(&mut app), controls);
}
