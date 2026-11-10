//! 真实设置BSN、中文字体和公共数值框布局；低窗口、大倍率及DPI下字段不溢出，完成按钮可见。

use super::*;
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_editor_ui::{ActiveNumericInput, ActiveNumericInputScrub, NumericInputSetValue};

fn page(width: f32, height: f32, scale: f32, dpi: f32) -> App {
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
        bevy::camera::visibility::VisibilityPlugin,
    ))
    .init_resource::<EditorSettings>()
    .init_resource::<UiSurface>()
    .init_resource::<ActiveNumericInput>()
    .init_resource::<ActiveNumericInputScrub>()
    .add_message::<NumericInputSetValue>()
    .insert_resource(MapEditorState {
        open: true,
        ..default()
    })
    .insert_resource(MapEditorSettingsState {
        open: true,
        ..default()
    })
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
    )
    // 用游戏内 F12 面板核对吸附字段的布局。
    .add_systems(Startup, (|| settings_panel(false)).spawn())
    .add_systems(
        Update,
        (setup_snap_inputs, sync_settings_panel, sync_snap_inputs).chain(),
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
    app.update();
    let world = app.world_mut();
    let panel = world
        .query_filtered::<Entity, With<MapEditorSettingsPanel>>()
        .single(world)
        .unwrap();
    let host = world
        .spawn(Node {
            width: px(width / (scale * dpi)),
            height: px(height / (scale * dpi)),
            ..default()
        })
        .id();
    world.entity_mut(panel).insert(ChildOf(host));
    for mut text_font in world.query::<&mut TextFont>().iter_mut(world) {
        text_font.font = font.clone().into();
    }
    for _ in 0..4 {
        app.update();
    }
    app
}

#[test]
fn snapping_settings_layout_scrolls_body_and_keeps_fields_footer_inside() {
    for (width, height, scale, dpi) in [
        (1280.0, 720.0, 1.0, 1.0),
        (960.0, 600.0, 1.5, 1.0),
        (1920.0, 1080.0, 1.25, 2.0),
        (720.0, 480.0, 1.5, 1.0),
    ] {
        let mut app = page(width, height, scale, dpi);
        let world = app.world_mut();
        let (body, transform) = world
            .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<components::SettingsBody>>()
            .single(world)
            .unwrap();
        let left = transform.translation.x - body.size().x * 0.5;
        let right = transform.translation.x + body.size().x * 0.5;
        assert!(body.size().y > 26.0 * scale * dpi);
        if height / (scale * dpi) <= 480.0 {
            assert!(body.content_size().y > body.size().y);
        }
        let mut count = 0;
        for (node, transform) in world
            .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<snapping::SnapField>>()
            .iter(world)
        {
            assert!(transform.translation.x - node.size().x * 0.5 >= left - 1.01);
            assert!(transform.translation.x + node.size().x * 0.5 <= right + 1.01);
            assert!(node.size().y >= 26.0 * scale * dpi - 1.01);
            count += 1;
        }
        assert_eq!(count, 3);
        for (button, node, transform) in world
            .query::<(&MapEditorSettingsButton, &ComputedNode, &UiGlobalTransform)>()
            .iter(world)
        {
            if button.0 == EditorSettingsAction::Close {
                assert!(transform.translation.y - node.size().y * 0.5 >= -1.01);
                assert!(transform.translation.y + node.size().y * 0.5 <= height + 1.01);
            }
        }
    }
}
