//! 真实中文字体排版，空间控件必须随模式栏在窄视口/缩放下折行。
use super::{components::*, resources::SpatialBrushState, types::ApplyMode, ui};
use crate::{resources::SceneEditorState, types::EditorTool, viewport::modes};
use bevy::{
    app::{HierarchyPropagatePlugin, PropagateSet},
    asset::AssetPlugin,
    camera::{ComputedCameraValues, RenderTargetInfo, Viewport},
    prelude::*,
    ui::{
        ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system,
        ui_surface::UiSurface, update::propagate_ui_target_cameras,
    },
};

#[test]
fn spatial_controls_wrap_with_real_chinese_font_in_narrow_and_scaled_viewports() {
    for (mode, (width, scale, dpi)) in
        [ApplyMode::Line, ApplyMode::Path]
            .into_iter()
            .flat_map(|mode| {
                [
                    (240., 1., 1.),
                    (360., 1., 1.),
                    (760., 1.5, 1.),
                    (960., 1., 2.),
                ]
                .into_iter()
                .map(move |size| (mode, size))
            })
    {
        let (mut app, viewport) = super::tests::app();
        app.add_plugins((
            AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
                ..default()
            },
            bevy::text::TextPlugin,
            bevy::transform::TransformPlugin,
            bevy::mesh::MeshPlugin,
            bevy::camera::visibility::VisibilityPlugin,
        ))
        .init_asset::<Image>() // 竖向模式栏正式加载透明图标；布局夹具也须登记图像资产类型。
        .init_resource::<UiSurface>()
        .insert_resource(UiScale(scale))
        .insert_resource({ let mut sidebar = crate::ui::sidebars::SceneEditorSidebarState::default(); sidebar.left = crate::ui::sidebars::EditorLeftSidebarTab::Terrain; sidebar })
        .add_message::<feathrise_editor_ui::NumericInputSetValue>()
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
                bevy::ui::widget::measure_text_system.after(bevy::text::detect_text_needs_rerender),
                ui_layout_system,
            )
                .chain(),
        );
        modes::install(&mut app);
        app.add_systems(
            Update,
            ui::setup
                .after(crate::SceneEditorSystemSet::Tasks)
                .before(crate::SceneEditorSystemSet::Presentation),
        );
        app.add_systems(
            Update,
            ui::sync.in_set(crate::SceneEditorSystemSet::Presentation),
        );
        app.world_mut().spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(width as u32, 600),
                        scale_factor: dpi,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: UVec2::new(width as u32, 600),
                    ..default()
                }),
                ..default()
            },
        ));
        app.world_mut().entity_mut(viewport).insert(Node {
            width: px(width / (scale * dpi)),
            height: px(600. / (scale * dpi)),
            ..default()
        });
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(
                feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
            ));
        app.update();
        app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::VolumeDig;
        app.world_mut()
            .resource_mut::<modes::ViewportModeState>()
            .mode = modes::ViewportMode::Edit;
        {
            let mut state = app.world_mut().resource_mut::<SpatialBrushState>();
            state.enabled = true;
            state.mode = mode;
            state.start = Some(Vec3::new(-20., -8., 12.));
            state.end = Some(Vec3::new(20., -8., 12.));
        }
        app.update();
        let world = app.world_mut();
        for mut text in world.query::<&mut TextFont>().iter_mut(world) {
            text.font = font.clone().into();
        }
        for _ in 0..4 {
            app.update();
        }
        let world = app.world_mut();
        let root = world
            .query_filtered::<Entity, With<modes::ViewportControlsRoot>>()
            .single(world)
            .unwrap();
        let root_node = world.get::<ComputedNode>(root).unwrap();
        let root_transform = world.get::<UiGlobalTransform>(root).unwrap();
        let left = root_transform.translation.x - root_node.size().x * 0.5;
        let right = root_transform.translation.x + root_node.size().x * 0.5;
        let mut buttons = world
            .query_filtered::<(&ComputedNode, &UiGlobalTransform, &Children), With<ActionButton>>();
        assert_eq!(buttons.iter(world).count(), 10);
        for (node, position, children) in buttons.iter(world) {
            assert!(node.size().x > 0.);
            assert!(position.translation.x - node.size().x * 0.5 >= left - 1.);
            assert!(
                position.translation.x + node.size().x * 0.5 <= right + 1.,
                "空间按钮不能越出模式浮层"
            );
            for child in children.iter() {
                if let Some(text) = world.get::<bevy::text::TextLayoutInfo>(child) {
                    assert!(text.size.x <= node.size().x + 1., "中文按钮文字不能溢出");
                }
            }
        }
        let mut fields =
            world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Coordinate>>();
        assert_eq!(fields.iter(world).count(), 3);
        for (node, position) in fields.iter(world) {
            assert!(
                node.size().x > 0. && position.translation.x + node.size().x * 0.5 <= right + 1.
            );
        }
        if mode == ApplyMode::Path {
            let mut buttons = world.query_filtered::<(&ComputedNode, &UiGlobalTransform, &Children), With<super::path::PathButton>>();
            assert_eq!(buttons.iter(world).count(), 13);
            for (node, position, children) in buttons.iter(world) {
                assert!(node.size().x > 0.);
                assert!(position.translation.x - node.size().x * 0.5 >= left - 1.);
                assert!(position.translation.x + node.size().x * 0.5 <= right + 1.);
                for child in children.iter() {
                    if let Some(text) = world.get::<bevy::text::TextLayoutInfo>(child) {
                        assert!(text.size.x <= node.size().x + 1., "路径按钮的中文不能溢出");
                    }
                }
            }
            let mut fields = world
                .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<super::path::PathSize>>(
                );
            assert_eq!(fields.iter(world).count(), 2);
            for (node, position) in fields.iter(world) {
                assert!(
                    node.size().x > 0.
                        && position.translation.x + node.size().x * 0.5 <= right + 1.
                );
            }
        }
    }
}
