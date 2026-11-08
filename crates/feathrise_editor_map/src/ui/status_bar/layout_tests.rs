//! 正式底栏与详情BSN使用真实中文字体，在窄窗、局部倍率和DPI下保持固定视口。

use super::components::*;
use super::resources::EditorStatusBarState;
use super::types::*;
use super::*;
use crate::components::MapEditorRoot;
use crate::resources::{MapEditorState, MapLoadingRecovery};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};

#[derive(Component, Clone, Default)]
struct TestViewport;

fn test_scene() -> impl Scene {
    bsn! {
        MapEditorRoot
        Node { width: percent(100), height: percent(100), flex_direction: FlexDirection::Column }
        Children [
            (Node { height: px(28), flex_shrink: 0.0 }),
            (TestViewport Node { width: percent(100), flex_grow: 1.0, min_height: px(0) }),
            (@footer()),
            (@crate::ui::map_loading::map_loading_input_blocker()),
            (@popup()),
        ]
    }
}

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
    .init_resource::<UiSurface>()
    .insert_resource(UiScale(scale))
    .insert_resource(MapEditorState {
        open: true,
        status: "正在显示当前操作反馈".into(),
        ..default()
    })
    .init_resource::<EditorStatusBarState>()
    .init_resource::<MapLoadingRecovery>()
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
    .add_systems(Startup, test_scene.spawn())
    .add_systems(
        Update,
        (
            crate::ui::map_loading::sync_map_loading,
            crate::ui::map_loading_recovery::sync_recovery_buttons,
            systems::observe_status,
            systems::sync_popup_layout,
            systems::sync_presentation,
        ).chain(),
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
    let root = world
        .query_filtered::<Entity, With<MapEditorRoot>>()
        .single(world)
        .unwrap();
    let host = world
        .spawn(Node {
            width: px(width / (scale * dpi)),
            height: px(height / (scale * dpi)),
            ..default()
        })
        .id();
    world.entity_mut(root).insert(ChildOf(host));
    for mut text_font in world.query::<&mut TextFont>().iter_mut(world) {
        text_font.font = font.clone().into();
    }
    frames(&mut app);
    app
}

fn frames(app: &mut App) {
    for _ in 0..5 {
        app.update();
    }
}

fn bounds<Marker: Component>(app: &mut App) -> Rect {
    let world = app.world_mut();
    let (node, transform) = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<Marker>>()
        .single(world)
        .unwrap();
    Rect::from_center_size(transform.translation, node.size())
}

#[test]
fn footer_and_viewport_do_not_move_when_details_expand_or_feedback_grows() {
    for width in [320.0, 640.0, 1280.0, 1920.0] {
        for scale in [0.6, 1.0, 1.5] {
            for dpi in [1.0, 2.0] {
                let mut app = page(width, 360.0, scale, dpi);
                let footer_before = bounds::<StatusFooter>(&mut app);
                let viewport_before = bounds::<TestViewport>(&mut app);
                assert!((footer_before.height() - 24.0 * scale * dpi).abs() <= 1.01);
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .cancel_close_with_error(format!(
                        "保存诊断：{}\n完整尾部",
                        "长中文路径/".repeat(200)
                    ));
                for operation in [
                    crate::types::EditorStatusOperation::SurfacePresetSave,
                    crate::types::EditorStatusOperation::WaterPresetSave,
                    crate::types::EditorStatusOperation::VegetationPresetSave,
                ] {
                    let mut editor = app.world_mut().resource_mut::<MapEditorState>();
                    editor.status = format!("{}\n完整尾部", "另一条长中文诊断/".repeat(200));
                    editor.record_operation_result(
                        operation,
                        &crate::types::EditorStatusTarget::new("对象ID", "冻结的长中文预设名称"),
                        crate::types::EditorStatusKind::Error,
                    );
                }
                app.world_mut()
                    .resource_mut::<EditorStatusBarState>()
                    .popup_open = true;
                frames(&mut app);
                assert_eq!(bounds::<StatusFooter>(&mut app), footer_before);
                assert_eq!(bounds::<TestViewport>(&mut app), viewport_before);
                let panel = bounds::<StatusPopup>(&mut app);
                assert!(
                    panel.min.x >= -1.01 && panel.min.y >= -1.01,
                    "{width}/{scale}/{dpi}: {panel:?}"
                );
                assert!(panel.max.x <= width + 1.01 && panel.max.y <= footer_before.min.y + 1.01);
                let body = bounds::<StatusDetailScroll>(&mut app);
                assert!(
                    body.width() > 0.0 && body.height() > 0.0,
                    "{width}/{scale}/{dpi}: {body:?}"
                );
                let world = app.world_mut();
                for (button, node, transform) in world
                    .query::<(&StatusButton, &ComputedNode, &UiGlobalTransform)>()
                    .iter(world)
                {
                    if button.0 == StatusAction::Toggle {
                        continue;
                    }
                    let rect = Rect::from_center_size(transform.translation, node.size());
                    assert!(rect.min.x >= panel.min.x - 1.01 && rect.max.x <= panel.max.x + 1.01);
                    if button.0 == StatusAction::Close {
                        assert!(
                            rect.min.y >= panel.min.y - 1.01 && rect.max.y <= body.min.y + 1.01
                        );
                    } else {
                        assert!(
                            rect.min.y >= body.max.y - 1.01 && rect.max.y <= panel.max.y + 1.01
                        );
                    }
                }
                app.world_mut()
                    .resource_mut::<EditorStatusBarState>()
                    .popup_open = false;
                frames(&mut app);
                assert_eq!(bounds::<StatusFooter>(&mut app), footer_before);
                assert_eq!(bounds::<TestViewport>(&mut app), viewport_before);
            }
        }
    }
}

#[test]
fn narrow_footer_keeps_phase_summary_and_details_inside_and_context_is_available_in_popup() {
    let mut app = page(640.0, 480.0, 1.5, 2.0);
    app.world_mut().resource_mut::<MapEditorState>().tool = crate::types::EditorTool::Raise;
    app.world_mut()
        .resource_mut::<EditorStatusBarState>()
        .popup_open = true;
    frames(&mut app);
    let footer = bounds::<StatusFooter>(&mut app);
    let world = app.world_mut();
    for (role, node, transform) in world
        .query::<(&StatusText, &ComputedNode, &UiGlobalTransform)>()
        .iter(world)
    {
        if role.0 == StatusTextRole::Context {
            assert_eq!(node.size(), Vec2::ZERO);
        }
        if matches!(role.0, StatusTextRole::Phase | StatusTextRole::Summary) {
            let rect = Rect::from_center_size(transform.translation, node.size());
            assert!(rect.min.x >= footer.min.x - 1.01 && rect.max.x <= footer.max.x + 1.01);
            assert!(rect.min.y >= footer.min.y - 1.01 && rect.max.y <= footer.max.y + 1.01);
        }
    }
    assert!(
        world
            .resource::<EditorStatusBarState>()
            .detail
            .contains("半径")
            && world
                .resource::<EditorStatusBarState>()
                .detail
                .contains("m/s")
    );
}

#[test]
fn loading_footer_keeps_recovery_and_details_inside_without_moving_workbench() {
    use crate::components::{MapEditorMapLoadingPanel, MapEditorMapLoadingPercent, MapLoadingRecoveryButton};
    use feathrise_scene_core::state::{ScenePhase, LoadingProgress, LoadingStage, PendingSceneTransition};
    // 最窄样例为213逻辑像素；保留恢复入口，长目标和完整原因由详情承载。
    for (width, scale, dpi) in [(360.0, 1.5, 1.0), (640.0, 1.5, 2.0), (1280.0, 1.0, 1.0), (1920.0, 0.6, 2.0)] {
        let mut app = page(width, 480.0, scale, dpi);
        let original = (bounds::<TestViewport>(&mut app), bounds::<StatusFooter>(&mut app));
        app.world_mut().resource_mut::<MapEditorState>().map_transition_in_progress = true;
        let transition = PendingSceneTransition::editor_map(ScenePhase::Active, "很长的中文加载地图名称".repeat(30));
        let mut progress = LoadingProgress::for_transition(&transition);
        progress.stage = LoadingStage::Failed;
        progress.label = format!("资源加载失败：{}\n完整尾部", "很长的中文资产路径/terrain.glb/".repeat(40));
        app.insert_resource(progress);
        frames(&mut app);
        assert_eq!((bounds::<TestViewport>(&mut app), bounds::<StatusFooter>(&mut app)), original);
        let loading = bounds::<MapEditorMapLoadingPanel>(&mut app);
        assert!(loading.height() <= original.1.height() + 1.1);
        let world = app.world_mut();
        let content = world.query_filtered::<&Node, With<StatusFooterContent>>().single(world).unwrap();
        assert_eq!(content.display, Display::None);
        let (details_node, details_at) = world.query::<(&StatusButton, &ComputedNode, &UiGlobalTransform)>()
            .iter(world).find(|(action, ..)| action.0 == StatusAction::Toggle).map(|(_, node, at)| (node, at)).unwrap();
        let details = Rect::from_center_size(details_at.translation, details_node.size());
        assert!(details.width() > 0.0 && details.max.x <= original.1.max.x + 1.1);
        for (node, at) in world.query_filtered::<(&ComputedNode, &UiGlobalTransform), Or<(With<MapLoadingRecoveryButton>, With<MapEditorMapLoadingPercent>)>>()
            .iter(world) {
            let rect = Rect::from_center_size(at.translation, node.size());
            assert!(rect.width() > 0.0 && rect.height() > 0.0, "恢复按钮及失败状态必须有可用面积");
            assert!(rect.min.x >= original.1.min.x - 1.1 && rect.max.x <= details.min.x + 1.1,
                "{width}/{scale}/{dpi}: 恢复入口不能被详情挤出底栏：{rect:?}");
            assert!(rect.min.y >= original.1.min.y - 1.1 && rect.max.y <= original.1.max.y + 1.1);
        }
        world.resource_mut::<EditorStatusBarState>().popup_open = true;
        frames(&mut app);
        assert_eq!((bounds::<TestViewport>(&mut app), bounds::<StatusFooter>(&mut app)), original);
        let popup = bounds::<StatusPopup>(&mut app);
        assert!(popup.width() > 0.0 && popup.height() > 0.0);
        assert!(popup.min.x >= -1.1 && popup.max.x <= width + 1.1);
        assert!(popup.max.y <= original.1.min.y + 1.1);
    }
}

#[test]
fn compact_header_keeps_commands_inside_and_map_before_context_and_details() {
    use crate::components::{MapEditorActionButton, MapEditorCurrentMapText, MapEditorSettingsButton};
    use crate::types::{EditorAction, EditorSettingsAction};
    use crate::workbench::WorkbenchHeaderItem;
    for (width, scale) in [(1280.0, 1.0), (1280.0, 1.5), (1920.0, 1.25)] {
        let mut app = page(width, 720.0, scale, 1.0);
        let world = app.world_mut();
        let root = world.query_filtered::<Entity, With<MapEditorRoot>>().single(world).unwrap();
        let old_header = world.get::<Children>(root).unwrap()[0];
        let font = world.query::<&TextFont>().iter(world).next().unwrap().font.clone();
        world.entity_mut(old_header).despawn();
        let header = world.spawn_scene(crate::ui::scene::header()).unwrap().id();
        world.entity_mut(root).insert_child(0, header);
        for mut text in world.query::<&mut TextFont>().iter_mut(world) { text.font = font.clone(); }
        world.query_filtered::<&mut Text, With<MapEditorCurrentMapText>>().single_mut(world).unwrap().0 =
            format!("当前地图：{} [MAP-长身份]", "很长的地图名称".repeat(30));
        frames(&mut app);
        let header_bounds = bounds::<crate::ui::tooltips::TooltipHeader>(&mut app);
        let footer_bounds = bounds::<StatusFooter>(&mut app);
        assert!((header_bounds.height() - 28.0 * scale).abs() < 1.1);
        let world = app.world_mut();
        for (action, slot) in [
            (EditorAction::Save, Some(WorkbenchHeaderItem::Save)),
            (EditorAction::Undo, Some(WorkbenchHeaderItem::Undo)),
            (EditorAction::Redo, Some(WorkbenchHeaderItem::Redo)),
            (EditorAction::Close, None),
        ] {
            let buttons = world.query::<(Entity, &MapEditorActionButton, &ComputedNode, &UiGlobalTransform)>()
                .iter(world).filter(|(_, button, ..)| button.0 == action).map(|(entity, _, node, at)| (entity, Rect::from_center_size(at.translation, node.size()))).collect::<Vec<_>>();
            assert_eq!(buttons.len(), 1, "顶栏命令不能重复创建：{action:?}");
            let (button, rect) = buttons[0];
            let parent = world.get::<ChildOf>(button).unwrap().parent();
            // 一体标题栏通过明确槽位移动原命令，不能再把槽位容器误判成脱离顶栏。
            if let Some(slot) = slot {
                assert_eq!(world.get::<WorkbenchHeaderItem>(parent), Some(&slot));
                assert_eq!(world.get::<ChildOf>(parent).unwrap().parent(), header);
            } else {
                assert_eq!(parent, header, "启动入口仍须直属顶栏");
            }
            assert!(world.get::<InheritedVisibility>(button).is_some_and(|visibility| visibility.get()), "顶栏命令必须真实可见：{action:?}");
            assert!(rect.width() > 0.0 && rect.height() > 0.0, "顶栏命令必须保留可用面积：{action:?}");
            assert!(rect.min.x >= header_bounds.min.x - 1.1 && rect.max.x <= header_bounds.max.x + 1.1, "窄窗顶栏命令越界：{action:?}");
            assert!(rect.min.y >= header_bounds.min.y - 1.1 && rect.max.y <= header_bounds.max.y + 1.1);
        }
        let settings = world.query::<(Entity, &MapEditorSettingsButton)>().iter(world)
            .find(|(_, button)| button.0 == EditorSettingsAction::Open).unwrap().0;
        let settings_slot = world.get::<ChildOf>(settings).unwrap().parent();
        assert_eq!(world.get::<WorkbenchHeaderItem>(settings_slot), Some(&WorkbenchHeaderItem::Settings));
        assert_eq!(world.get::<ChildOf>(settings_slot).unwrap().parent(), header);
        assert!(world.get::<Children>(settings).unwrap().iter().any(|child| world.get::<Text>(child).is_some_and(|text| text.as_str() == "设置")));
        let (map, computed, at) = world.query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform), With<MapEditorCurrentMapText>>().single(world).unwrap();
        let map_bounds = Rect::from_center_size(at.translation, computed.size());
        assert!(map_bounds.width() > 0.0 && map_bounds.min.x >= footer_bounds.min.x - 1.1);
        assert!(map_bounds.max.x <= footer_bounds.max.x + 1.1 && map_bounds.min.y >= footer_bounds.min.y - 1.1 && map_bounds.max.y <= footer_bounds.max.y + 1.1);
        let footer = world.query_filtered::<Entity, With<StatusFooter>>().single(world).unwrap();
        let content = world.query_filtered::<Entity, With<StatusFooterContent>>().single(world).unwrap();
        assert_eq!(world.get::<ChildOf>(world.get::<ChildOf>(map).unwrap().parent()).unwrap().parent(), content);
        assert_eq!(world.get::<ChildOf>(content).unwrap().parent(), footer);
        assert_eq!(world.get::<TextLayout>(map).unwrap().linebreak, bevy::text::LineBreak::NoWrap);
        // 长地图名及非整数倍率下仍按“地图 → 视图摘要 → 详情”排列，详情固定在底栏最右。
        let (context, context_node, context_at) = world.query::<(Entity, &StatusText, &ComputedNode, &UiGlobalTransform)>()
            .iter(world).find(|(_, role, ..)| role.0 == StatusTextRole::Context)
            .map(|(entity, _, node, at)| (entity, node, at)).unwrap();
        let context_bounds = Rect::from_center_size(context_at.translation, context_node.size());
        let (details, details_node, details_at) = world.query::<(Entity, &StatusButton, &ComputedNode, &UiGlobalTransform)>()
            .iter(world).find(|(_, action, ..)| action.0 == StatusAction::Toggle)
            .map(|(entity, _, node, at)| (entity, node, at)).unwrap();
        let details_bounds = Rect::from_center_size(details_at.translation, details_node.size());
        assert!(context_bounds.width() > 0.0 && map_bounds.max.x <= context_bounds.min.x + 1.1);
        assert!(context_bounds.max.x <= details_bounds.min.x + 1.1);
        assert!((details_bounds.max.x - (footer_bounds.max.x - 8.0 * scale)).abs() < 1.1);
        assert_eq!(world.get::<ChildOf>(context).unwrap().parent(), content);
        assert_eq!(world.get::<ChildOf>(details).unwrap().parent(), footer);
    }
}
