//! 使用真实中文字体验证窄窗口、UI缩放与长诊断；说明可滚动，三种操作仍在面板内。

use super::*;
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_scene_content::SceneId;

#[test]
fn long_chinese_confirmation_keeps_all_actions_inside_small_and_scaled_windows() {
    for (closing, width, height, scale, dpi) in [
        (false, 300.0, 240.0, 1.5, 1.25),
        (false, 800.0, 390.0, 1.0, 1.0),
        (false, 1200.0, 390.0, 0.6, 1.0),
        (true, 300.0, 240.0, 1.5, 1.25),
        (true, 800.0, 390.0, 1.0, 1.0),
        (true, 1200.0, 390.0, 0.6, 1.0),
    ] {
        let mut app = App::new();
        if closing {
            let mut docs = crate::ui::scene_documents::SceneDocuments::default();
            docs.closing = Some(SceneId::parse("MAP-close-layout").unwrap());
            app.insert_resource(docs);
        }
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            bevy::scene::ScenePlugin,
            bevy::text::TextPlugin,
            bevy::transform::TransformPlugin,
        ));
        app.add_plugins(HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(
            PostUpdate,
        ));
        app.add_plugins(HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(
            PostUpdate,
        ));
        app.init_resource::<UiSurface>()
            .insert_resource(UiScale(scale));
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
        let font = app
            .world_mut()
            .resource_mut::<Assets<Font>>()
            .add(Font::from_bytes(
                feathrise_runtime::app::default_font::DEFAULT_FONT_BYTES.to_vec(),
            ));
        let physical_size = UVec2::new((width * scale * dpi) as u32, (height * scale * dpi) as u32);
        app.world_mut().spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size,
                        scale_factor: dpi,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size,
                    ..default()
                }),
                ..default()
            },
        ));
        let host = app
            .world_mut()
            .spawn(Node {
                width: px(width),
                height: px(height),
                ..default()
            })
            .id();
        let root = app
            .world_mut()
            .spawn_scene(scene_switch_dialog())
            .unwrap()
            .id();
        app.world_mut().entity_mut(host).add_child(root);
        let card = app.world().get::<Children>(root).unwrap()[0];
        for mut text_font in app
            .world_mut()
            .query::<&mut TextFont>()
            .iter_mut(app.world_mut())
        {
            text_font.font = font.clone().into();
        }
        app.insert_resource(SceneEditorState {
            open: true,
            dirty: true,
            status: format!(
                "保存未开始：{}",
                "场景作者资源仍有未解决的外部冲突；请修正后重试。".repeat(8)
            ),
            scene_switch_confirmation: Some(SceneSwitchConfirmation {
                source_scene_id: None,
                source_revision: 0,
                global_revision: 0,
                target_scene_id: SceneId::parse("MAP-long-name-confirmation").unwrap(),
                target_display_name: "用于检查切换场景确认的中文场景名".repeat(4),
                diagnostic: None,
                stage: SceneSwitchStage::Choosing,
            }),
            ..default()
        })
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .add_systems(Update, sync_scene_switch_dialog);
        for _ in 0..4 {
            app.update();
        }
        let root_size = app.world().get::<ComputedNode>(root).unwrap().size();
        let card_size = app.world().get::<ComputedNode>(card).unwrap().size();
        assert!(card_size.x > 0.0 && card_size.y > 0.0);
        assert!(
            card_size.x <= root_size.x + 1.0 && card_size.y <= root_size.y + 1.0,
            "确认面板必须完整留在窗口内"
        );
        let action_nodes: Vec<_> = app
            .world_mut()
            .query_filtered::<(Entity, &ComputedNode), With<SceneEditorActionButton>>()
            .iter(app.world())
            .map(|(entity, node)| (entity, node.size()))
            .collect();
        assert_eq!(action_nodes.len(), 3);
        for (button, size) in action_nodes {
            assert!(size.x > 0.0 && size.y > 0.0 && size.x <= card_size.x);
            let text_entity = app.world().get::<Children>(button).unwrap()[0];
            let text = app.world().get::<ComputedNode>(text_entity).unwrap();
            assert!(
                text.content_size().x <= text.size().x + 2.0
                    && text.content_size().y <= text.size().y + 2.0,
                "三个中文操作标签不能被挤出按钮"
            );
        }
        let body = app
            .world_mut()
            .query_filtered::<&ComputedNode, With<SceneSwitchDialogBody>>()
            .single(app.world())
            .unwrap();
        assert!(body.size().y > 0.0 && body.size().y < card_size.y);
        let body_size = body.size();
        let body_content = body.content_size();
        let text_sizes: Vec<_> = app
            .world_mut()
            .query::<(&Text, &ComputedNode)>()
            .iter(app.world())
            .map(|(text, node)| (text.0.len(), node.size(), node.content_size()))
            .collect();
        if height <= 240.0 {
            assert!(
                body_content.y > body_size.y,
                "矮窗口的长说明应留在可滚动正文内：窗口{width}x{height}，面板{card_size:?}，正文{body_size:?}/{body_content:?}，文字{text_sizes:?}"
            );
        } // 宽窗口允许正文完整显示；不要求已经容纳全部说明的面板产生多余滚动。
    }
}
