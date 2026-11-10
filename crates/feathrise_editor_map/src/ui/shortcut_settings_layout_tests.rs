//! 真实BSN与中文字体的快捷键模态布局回归；验证大倍率/矮窗口下正文滚动及底部操作可见。

use super::*;
use crate::components::{MapEditorSettingsButton, MapEditorSettingsPanel};
use crate::ui::editor_settings::{settings_panel, sync_settings_panel};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_editor_core::EditorShortcutAction;

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
    .insert_resource(MapEditorSettingsState {
        open: true,
        shortcuts: true,
        ..default()
    })
    .init_resource::<EditorSettings>()
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
    )
    // 快捷键夹具使用游戏内 F12 设置面板。
    .add_systems(Startup, (|| settings_panel(false)).spawn())
    .add_systems(
        Update,
        (sync_settings_panel, sync_shortcut_settings_panel).chain(),
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
fn shortcut_modal_scrolls_body_and_keeps_header_footer_and_key_columns_inside() {
    for (width, height, scale, dpi) in [
        (1280.0, 720.0, 0.6, 1.0),
        (1280.0, 720.0, 1.0, 1.0),
        (1280.0, 720.0, 1.5, 1.0),
        (960.0, 600.0, 1.5, 1.0),
        (1920.0, 1080.0, 1.25, 2.0),
        (720.0, 480.0, 1.0, 1.0),
    ] {
        let mut app = page(width, height, scale, dpi);
        let world = app.world_mut();
        let (body, body_transform) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<MapEditorShortcutSettingsBody>>().single(world).unwrap();
        let left = body_transform.translation.x - body.size().x * 0.5;
        let right = body_transform.translation.x + body.size().x * 0.5;
        assert!(body.size().y > 28.0 * scale * dpi);
        let mut rows = 0;
        let mut add_menu_bindings = 0;
        let mut add_menu_unbindings = 0;
        for (button, node, transform) in world
            .query::<(&MapEditorSettingsButton, &ComputedNode, &UiGlobalTransform)>()
            .iter(world)
        {
            if matches!(
                button.0,
                EditorSettingsAction::RebindShortcut(_) | EditorSettingsAction::UnbindShortcut(_)
            ) {
                assert!(transform.translation.x - node.size().x * 0.5 >= left - 1.01);
                assert!(transform.translation.x + node.size().x * 0.5 <= right + 1.01);
                assert!(node.size().y >= 26.0 * scale * dpi);
                if matches!(button.0, EditorSettingsAction::RebindShortcut(_)) {
                    rows += 1;
                }
                match button.0 {
                    EditorSettingsAction::RebindShortcut(EditorShortcutAction::AddObjectMenu) => add_menu_bindings += 1,
                    EditorSettingsAction::UnbindShortcut(EditorShortcutAction::AddObjectMenu) => add_menu_unbindings += 1,
                    _ => {}
                }
            }
            if button.0 == EditorSettingsAction::ResetShortcuts {
                assert!(transform.translation.y - node.size().y * 0.5 >= 0.0);
                assert!(transform.translation.y + node.size().y * 0.5 <= height + 1.01);
            }
        }
        assert_eq!(rows, 13);
        assert_eq!((add_menu_bindings, add_menu_unbindings), (1, 1));
        let node = world
            .query_filtered::<&Node, With<MapEditorShortcutSettingsBody>>()
            .single(world)
            .unwrap();
        assert_eq!(node.overflow, Overflow::scroll_y());
    }
}
