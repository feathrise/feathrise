//! 真实中文字体的对象管理排版；窄侧栏、缩放和DPI下按钮折行、字段裁剪。

use super::components::{ObjectButton, ObjectInput};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::asset::AssetPlugin;
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::prelude::*;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};

#[test]
fn controls_fit_real_chinese_font_in_narrow_scaled_sidebars() {
    for (width, scale, dpi) in [
        (256.0, 1.0, 1.0),
        (360.0, 1.0, 1.0),
        (480.0, 1.5, 1.0),
        (640.0, 1.0, 2.0),
    ] {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin {
                file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets").into(),
                ..default()
            },
            bevy::text::TextPlugin,
            bevy::scene::ScenePlugin,
            bevy::transform::TransformPlugin,
            bevy::mesh::MeshPlugin,
            bevy::camera::visibility::VisibilityPlugin,
        ))
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
                bevy::ui::widget::measure_text_system.after(bevy::text::detect_text_needs_rerender),
                ui_layout_system,
            )
                .chain(),
        );
        let logical_width = width / (scale * dpi);
        app.add_systems(Startup, bevy::prelude::SpawnSystem::spawn(move || bsn! {
            Node { width: px(logical_width), min_width: px(0), flex_direction: FlexDirection::Column }
            Children [(super::scene::panel()), (super::scene::properties_panel())]
        }));
        app.world_mut().spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(width as u32, 800),
                        scale_factor: dpi,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: UVec2::new(width as u32, 800),
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
        app.update();
        let world = app.world_mut();
        for mut node in world.query_filtered::<&mut Node, With<crate::ui::scene_inspector::InspectorSection>>().iter_mut(world) { node.display = Display::Flex; }
        // 布局夹具显式展开搜索；正式界面只在存在摆放资产或搜索词时显示它。
        for mut node in world.query_filtered::<&mut Node, With<super::components::SceneAssetSearch>>().iter_mut(world) { node.display = Display::Flex; }
        for mut text_font in world.query::<&mut TextFont>().iter_mut(world) {
            text_font.font = font.clone().into();
        }
        for mut input in world
            .query_filtered::<&mut bevy::text::EditableText, With<ObjectInput>>()
            .iter_mut(world)
        {
            *input = crate::ui::text_input::single_line_input(&"灯".repeat(128), 128);
        }
        for _ in 0..4 {
            app.update();
        }
        let world = app.world_mut();
        let mut count = 0;
        for (_, node, transform, children) in world
            .query::<(&ObjectButton, &ComputedNode, &UiGlobalTransform, &Children)>()
            .iter(world)
        {
            let left = transform.translation.x - node.size().x * 0.5;
            let right = transform.translation.x + node.size().x * 0.5;
            assert!(
                left >= -1.0 && right <= width + 1.0,
                "管理控件越出侧栏：{left}..{right}, {width}"
            );
            assert!(node.size().y > 0.0);
            for child in children.iter() {
                if world.get::<ObjectInput>(child).is_some() {
                    continue;
                } // 长字段由双方Clip_x裁剪，不能强制缩小文字。
                if let Some(text) = world.get::<bevy::text::TextLayoutInfo>(child) {
                    assert!(text.size.x <= node.size().x + 1.0);
                }
            }
            count += 1;
        }
        assert_eq!(count, 9);
    }
}
