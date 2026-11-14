//! 水域侧栏真实字体布局、统一对象选择、输入所有权和原创建事务回归；不写场景或设置。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::*, types::WaterSidebarFeedbackKind};
use crate::SceneEditorSystemSet;
use crate::authoring::{
    EditorAuthoringSession, WaterBrushStroke, WaterNetworkEditState, WaterPreviewState,
    apply_water_network_actions, setup_water_network_numeric_inputs, test_heightfield_draft,
    water_asset_with_layer_for_tests,
};
use crate::components::{
    SceneEditorActionButton, SceneEditorRightPanelScroll, SceneEditorWaterFeatureRow,
};
use crate::resources::{
    SceneBrowserDragState, SceneBrowserState, SceneDeletionTask, SceneEditorNumericScrubMouseGuard,
    SceneEditorState,
};
use crate::types::{EditorTool, WaterAuthoringFeatureKind};
use crate::ui::courtyard::CourtyardUiState;
use crate::ui::gltf_browser::EditorGltfBrowserState;
use crate::ui::sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState};
use crate::ui::text_input::EditorTextInputGuard;
use crate::ui::water_feature_list::{
    handle_water_feature_row_clicks, spawn_water_feature_rows, sync_water_feature_rows,
};
use crate::ui::water_layers::{PendingClipmapWaterLayerCreate, handle_water_layer_buttons};
use crate::viewport::SelectedWaterViewportState;
use crate::viewport::modes::{ViewportMode, ViewportModeState};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::prelude::*;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_editor_ui::NumericInputRoot;
use feathrise_scene_content::{SceneWaterRiver, SceneWaterSegmentKind, SceneWaterSplinePoint, SceneWaterfall};

fn dispatch_creation(
    buttons: Query<(&feathrise_editor_ui::EditorButtonClick, &SceneEditorActionButton), Changed<feathrise_editor_ui::EditorButtonClick>>,
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    stroke: Res<WaterBrushStroke>,
    network: Res<WaterNetworkEditState>,
) {
    for (interaction, action) in &buttons {
        if interaction.is_clicked() {
            crate::ui::water_actions::handle_water_action(
                action.0,
                &mut editor,
                &session,
                &stroke,
                &network,
            );
        }
    }
}

fn page(shell: bool) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
    ));
    app.init_asset::<Image>()
        .init_resource::<UiSurface>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInput>()
        .init_resource::<SceneBrowserState>()
        .init_resource::<SceneBrowserDragState>()
        .init_resource::<SceneDeletionTask>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<WaterPreviewState>()
        .init_resource::<EditorGltfBrowserState>()
        .init_resource::<SelectedWaterViewportState>()
        .init_resource::<PendingClipmapWaterLayerCreate>()
        .init_resource::<ViewportModeState>()
        .init_resource::<CourtyardUiState>()
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .insert_resource(UiScale(1.0));
    let mut draft = test_heightfield_draft(vec![0; 121].into());
    // 水网事务属于高度地形；保留主作者文档而不把中性GLB夹具误作可编辑高度源。
    draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
    draft.manifest.terrain.height_path = Some(format!("{}/terrain/height.sparse.json", draft.folder));
    let mut water = water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder);
    water.layers[0].label = "湖泊名称用于验证窄侧栏的完整目标回显".into();
    water.layers[0].resolution = [11, 11];
    draft
        .water_masks
        .insert(water.layers[0].id.clone(), vec![0; 121].into());
    let point = SceneWaterSplinePoint {
        id: "first-point".into(),
        position: [1.0, 3.0, 1.0],
        width: 12.0,
        depth: 2.0,
        flow_speed: 1.0,
        outgoing: SceneWaterSegmentKind::River,
    };
    water.rivers.push(SceneWaterRiver {
        id: "shared-river".into(),
        label: "统一列表里的河流".into(),
        source_layer_id: None,
        target_layer_id: None,
        points: vec![point.clone()],
        base_color_srgba: [0.05, 0.2, 0.3, 0.72],
        perceptual_roughness: 0.06,
    });
    water.waterfalls.push(SceneWaterfall {
        id: "shared-waterfall".into(),
        label: "未完成的瀑布".into(),
        source: None,
        target: None,
        points: vec![point],
        foam_strength: 1.0,
        base_color_srgba: [0.05, 0.2, 0.3, 0.72],
        perceptual_roughness: 0.06,
    });
    draft.water = Some(water);
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(draft.scene_id.clone()),
        ..default()
    });
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    });
    let mut sidebar = SceneEditorSidebarState::default();
    sidebar.right = crate::ui::sidebars::EditorRightSidebarTab::Tools;
    sidebar.left = EditorLeftSidebarTab::Water;
    app.insert_resource(sidebar);
    app.configure_sets(
        Update,
        (
            SceneEditorSystemSet::Input,
            SceneEditorSystemSet::Tasks,
            SceneEditorSystemSet::Presentation,
        )
            .chain(),
    );
    super::install(&mut app);
    app.add_systems(
        Update,
        (
            handle_water_feature_row_clicks,
            dispatch_creation,
            handle_water_layer_buttons,
            apply_water_network_actions,
        )
            .chain()
            .in_set(SceneEditorSystemSet::Input),
    );
    app.add_systems(
        Update,
        crate::viewport::modes::sync_mode
            .after(SceneEditorSystemSet::Input)
            .before(SceneEditorSystemSet::Tasks),
    );
    app.add_systems(
        Update,
        (
            crate::ui::water_level_input::setup_water_level_input,
            setup_water_network_numeric_inputs,
            spawn_water_feature_rows,
            sync_water_feature_rows,
            crate::ui::sidebars::sync_editor_sidebar_context,
            crate::authoring::sync_water_network_text,
        )
            .chain()
            .in_set(SceneEditorSystemSet::Presentation),
    );
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
    if shell {
        app.add_systems(Startup, crate::ui::sidebars::test_right_sidebar.spawn());
        app.add_systems(
            Update,
            crate::ui::sidebars::sync_editor_sidebar_presentation
                .in_set(SceneEditorSystemSet::Presentation),
        );
    } else {
        app.add_systems(
            Startup,
            crate::ui::water_scene::water_authoring_panel.spawn(),
        );
    }
    let face = app
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
                    physical_size: UVec2::splat(4096),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::splat(4096),
                ..default()
            }),
            ..default()
        },
    ));
    if !shell { app.add_systems(Startup, crate::ui::water_scene::water_properties_panel.spawn()); }
    app.update();
    let world = app.world_mut();
    let root = if shell {
        let root = world
            .query_filtered::<Entity, With<crate::components::SceneEditorPane>>()
            .single(world)
            .unwrap();
        world.get_mut::<Node>(root).unwrap().width = percent(100);
        root
    } else {
        world
            .query_filtered::<Entity, With<WaterSidebarRoot>>()
            .single(world)
            .unwrap()
    };
    let host = world
        .spawn(Node {
            width: px(240),
            height: px(500),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .id();
    world.entity_mut(root).insert(ChildOf(host));
    // 单域布局夹具同时挂载迁出的属性；完整左右栏分区由检查器集成测试验证。
    let properties = world.query::<(Entity, &crate::ui::scene_inspector::InspectorSection)>().iter(world)
        .find(|(_, section)| **section == crate::ui::scene_inspector::InspectorSection::Water).unwrap().0;
    world.entity_mut(properties).insert(ChildOf(host));
    world.get_mut::<Node>(properties).unwrap().display = Display::Flex;
    for mut font in world.query::<&mut TextFont>().iter_mut(world) {
        font.font = face.clone().into();
    }
    for _ in 0..6 {
        app.update();
    }
    (app, host)
}

fn button(app: &mut App, label: &str) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<(Entity, &Children), With<InteractionButton>>()
        .iter(world)
        .find_map(|(entity, children)| {
            children
                .iter()
                .any(|child| {
                    world
                        .get::<Text>(child)
                        .is_some_and(|text| text.as_str() == label)
                })
                .then_some(entity)
        })
        .unwrap_or_else(|| panic!("缺少按钮：{label}"))
}

fn press(app: &mut App, label: &str) {
    let entity = button(app, label);
    app.world_mut()
        .entity_mut(entity)
        .insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    app.world_mut().entity_mut(entity).insert(UiInteraction::None);
    app.update();
}

fn feedback(app: &mut App, kind: WaterSidebarFeedbackKind) -> String {
    let world = app.world_mut();
    world
        .query::<(&WaterSidebarFeedbackText, &Text)>()
        .iter(world)
        .find(|(marker, _)| marker.0 == kind)
        .unwrap()
        .1
        .0
        .clone()
}

fn author(app: &App) -> String {
    serde_json::to_string(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .water
            .as_ref()
            .unwrap(),
    )
    .unwrap()
}

fn edit(app: &mut App) {
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
    app.update();
}

#[test]
fn tabs_prepare_tools_without_changing_authors_indices_or_mode() {
    let (mut app, _) = page(false);
    let before = author(&app);
    for (label, tool) in [
        ("河流", EditorTool::RiverSpline),
        ("瀑布", EditorTool::Waterfall),
        ("静水", EditorTool::Water),
    ] {
        press(&mut app, label);
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Inspect
        );
        assert!(feedback(&mut app, WaterSidebarFeedbackKind::Mode).contains("预备"));
        edit(&mut app);
        assert_eq!(app.world().resource::<SceneEditorState>().tool, tool);
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
        app.update();
    }
    assert_eq!(author(&app), before);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, 0);
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .selected_water_control_point,
        None
    );
}

#[test]
fn one_object_pool_stays_complete_and_real_row_selection_follows_category() {
    let (mut app, _) = page(false);
    edit(&mut app);
    let before = author(&app);
    let world = app.world_mut();
    let rows = world
        .query::<(Entity, &SceneEditorWaterFeatureRow)>()
        .iter(world)
        .filter_map(|(entity, row)| {
            row.feature_id
                .as_ref()
                .map(|id| (entity, row.kind, id.clone()))
        })
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 3);
    for (entity, kind, id) in &rows {
        app.world_mut()
            .entity_mut(*entity)
            .insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.update();
        app.world_mut()
            .entity_mut(*entity)
            .insert(UiInteraction::None);
        app.update();
        let expected = match kind {
            WaterAuthoringFeatureKind::StillWater => WaterSidebarPage::Still,
            WaterAuthoringFeatureKind::River => WaterSidebarPage::River,
            WaterAuthoringFeatureKind::Waterfall => WaterSidebarPage::Waterfall,
        };
        assert_eq!(app.world().resource::<WaterSidebarState>().page, expected);
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            expected.tool()
        );
        assert!(feedback(&mut app, WaterSidebarFeedbackKind::Target).contains(id));
        let world = app.world_mut();
        assert_eq!(
            world
                .query::<&SceneEditorWaterFeatureRow>()
                .iter(world)
                .filter(|row| row.feature_id.is_some())
                .count(),
            3
        );
    }
    assert_eq!(author(&app), before);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn explicit_creations_use_original_transactions_and_never_happen_on_tab_switch() {
    let (mut app, _) = page(false);
    edit(&mut app);
    for (label, create_label) in [
        ("静水", "新建静水体"),
        ("河流", "新建河流"),
        ("瀑布", "新建瀑布"),
    ] {
        let before = author(&app);
        let revision = app.world().resource::<EditorAuthoringSession>().revision;
        press(&mut app, label);
        assert_eq!(author(&app), before);
        assert_eq!(
            app.world().resource::<EditorAuthoringSession>().revision,
            revision
        );
        press(&mut app, create_label);
        // 显式静水新建仍走正式单槽R8后台准备；导航断言在上方同步完成，只有新建后等待事务采用。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.world().resource::<EditorAuthoringSession>().revision == revision {
            app.update();
            assert!(std::time::Instant::now() < deadline,
                "显式新建应完成原作者事务：{}", app.world().resource::<SceneEditorState>().status);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            app.world().resource::<EditorAuthoringSession>().revision,
            revision + 1
        );
        assert_ne!(author(&app), before);
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    let water = session.draft.as_ref().unwrap().water.as_ref().unwrap();
    assert_eq!(
        [
            water.layers.len(),
            water.rivers.len(),
            water.waterfalls.len()
        ],
        [2, 2, 2]
    );
    assert_eq!(session.undo.len(), 3);
}

#[test]
fn blocked_tab_edges_are_consumed_without_late_replay() {
    for blocker in 0..11 {
        let (mut app, _) = page(false);
        match blocker {
            0 => {
                app.world_mut()
                    .resource_mut::<EditorTextInputGuard>()
                    .block_shortcuts = true
            }
            1 => app
                .world_mut()
                .resource_mut::<SceneEditorNumericScrubMouseGuard>()
                .advance(true, true, false),
            2 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .close_requested = true
            }
            3 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .new_scene_dialog_open = true
            }
            4 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .scene_transition_in_progress = true
            }
            5 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .saving = true
            }
            6 => app.world_mut().resource_mut::<WaterBrushStroke>().active = true,
            7 => {
                let scene_id = app
                    .world()
                    .resource::<SceneEditorState>()
                    .selected_scene_id
                    .clone();
                let mut network = app.world_mut().resource_mut::<WaterNetworkEditState>();
                network.scene_id = scene_id;
                network.feature_id = Some("shared-river".into());
                network.point_id = Some("first-point".into());
            }
            8 => app.world_mut().resource_mut::<SceneEditorState>().open = false,
            9 => {
                app.world_mut().resource_mut::<SceneEditorSidebarState>().left =
                    EditorLeftSidebarTab::Terrain
            }
            _ => {
                app.world_mut()
                    .resource_mut::<feathrise_editor_ui::ActiveNumericInput>()
                    .entity = Some(Entity::PLACEHOLDER)
            }
        }
        let tab = button(&mut app, "河流");
        app.world_mut().entity_mut(tab).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.update();
        assert_eq!(
            app.world().resource::<WaterSidebarState>().page,
            WaterSidebarPage::Still,
            "阻塞门{blocker}"
        );
        app.world_mut()
            .resource_mut::<EditorTextInputGuard>()
            .block_shortcuts = false;
        app.world_mut()
            .resource_mut::<SceneEditorNumericScrubMouseGuard>()
            .advance(false, false, true);
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            editor.open = true;
            editor.close_requested = false;
            editor.new_scene_dialog_open = false;
            editor.scene_transition_in_progress = false;
        }
        app.world_mut().resource_mut::<SceneEditorSidebarState>().left = EditorLeftSidebarTab::Water;
        app.world_mut()
            .resource_mut::<feathrise_editor_ui::ActiveNumericInput>()
            .entity = None;
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .saving = false;
        app.world_mut().resource_mut::<WaterBrushStroke>().active = false;
        app.world_mut()
            .resource_mut::<WaterNetworkEditState>()
            .clear_drag();
        app.update();
        app.world_mut()
            .resource_mut::<SceneEditorNumericScrubMouseGuard>()
            .advance(false, false, false);
        app.update();
        assert_eq!(
            app.world().resource::<WaterSidebarState>().page,
            WaterSidebarPage::Still,
            "旧边沿不补发{blocker}"
        );
    }
}

#[test]
fn feedback_uses_current_joint_preview_identity_and_reports_empty_or_unsupported_targets() {
    let (mut app, _) = page(false);
    let scene_id = app
        .world()
        .resource::<SceneEditorState>()
        .selected_scene_id
        .clone();
    {
        let mut preview = app.world_mut().resource_mut::<WaterPreviewState>();
        preview.failed_scene_id = scene_id.clone();
    }
    app.update();
    assert!(feedback(&mut app, WaterSidebarFeedbackKind::Availability).contains("预览失败"));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .water_revision = 1;
    app.update();
    assert!(!feedback(&mut app, WaterSidebarFeedbackKind::Availability).contains("预览失败"));
    {
        let mut preview = app.world_mut().resource_mut::<WaterPreviewState>();
        preview.applied_scene_id = scene_id;
        preview.applied_water_revision = 1;
    }
    app.update();
    assert!(feedback(&mut app, WaterSidebarFeedbackKind::Availability).contains("已采用"));
    press(&mut app, "河流");
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .manifest
        .terrain
        .kind = feathrise_scene_content::SceneTerrainKind::Gltf;
    app.update();
    assert!(feedback(&mut app, WaterSidebarFeedbackKind::Availability).contains("不支持"));
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = None;
    app.update();
    assert!(feedback(&mut app, WaterSidebarFeedbackKind::Availability).contains("等待当前场景"));
}

#[test]
fn all_pages_fit_real_cjk_font_sidebar_widths_scales_and_dpi_with_fixed_navigation() {
    let (mut app, host) = page(false);
    edit(&mut app);
    let world = app.world_mut();
    let numeric_entities = world
        .query_filtered::<Entity, With<NumericInputRoot>>()
        .iter(world)
        .collect::<Vec<_>>();
    assert_eq!(numeric_entities.len(), 5);
    // 长数字仅作为布局夹具，原作者值和公共输入协议没有被修改。
    for mut text in world
        .query_filtered::<&mut Text, With<feathrise_editor_ui::NumericInputDisplayText>>()
        .iter_mut(world)
    {
        text.0 = "-10000.000".into();
    }
    for width in [200.0, 240.0, 320.0, 420.0] {
        for scale in [0.6, 1.0, 1.5] {
            for dpi in [1.0, 2.0] {
                app.world_mut().get_mut::<Node>(host).unwrap().width = px(width);
                app.world_mut().resource_mut::<UiScale>().0 = scale;
                let world = app.world_mut();
                for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                    camera.computed.target_info.as_mut().unwrap().scale_factor = dpi;
                }
                for label in ["静水", "河流", "瀑布"] {
                    press(&mut app, label);
                    let world = app.world_mut();
                    let host_width = world.get::<ComputedNode>(host).unwrap().size().x;
                    let center = world.get::<UiGlobalTransform>(host).unwrap().translation.x;
                    for (node, transform) in world
                        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<InteractionButton>>()
                        .iter(world)
                    {
                        if node.size().x == 0.0 {
                            continue;
                        }
                        assert!(
                            transform.translation.x - node.size().x * 0.5
                                >= center - host_width * 0.5 - 1.1,
                            "左侧溢出{label}/{width}"
                        );
                        assert!(
                            transform.translation.x + node.size().x * 0.5
                                <= center + host_width * 0.5 + 1.1,
                            "右侧溢出{label}/{width}"
                        );
                    }
                    for (node, transform, children) in world.query_filtered::<(&ComputedNode, &UiGlobalTransform, &Children), With<NumericInputRoot>>().iter(world) {
                        if node.size().x == 0.0 { continue; }
                        for child in children.iter() {
                            if world.get::<Text>(child).is_none() { continue; }
                            let child_node = world.get::<ComputedNode>(child).unwrap();
                            let child_center = world.get::<UiGlobalTransform>(child).unwrap().translation.x;
                            assert!(child_center - child_node.size().x * 0.5 >= transform.translation.x - node.size().x * 0.5 - 1.1, "数字左溢出{label}/{width}");
                            assert!(child_center + child_node.size().x * 0.5 <= transform.translation.x + node.size().x * 0.5 + 1.1, "数字右溢出{label}/{width}");
                        }
                    }
                    let scroll = world
                        .query_filtered::<Entity, With<WaterSidebarScroll>>()
                        .single(world)
                        .unwrap();
                    let before = world
                        .query::<(&WaterSidebarTab, &UiGlobalTransform)>()
                        .iter(world)
                        .map(|(_, transform)| transform.translation.y)
                        .collect::<Vec<_>>();
                    world.get_mut::<ScrollPosition>(scroll).unwrap().y = 100.0;
                    app.update();
                    let world = app.world_mut();
                    assert_eq!(
                        world
                            .query::<(&WaterSidebarTab, &UiGlobalTransform)>()
                            .iter(world)
                            .map(|(_, transform)| transform.translation.y)
                            .collect::<Vec<_>>(),
                        before
                    );
                }
            }
        }
    }
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<NumericInputRoot>>()
            .iter(world)
            .collect::<Vec<_>>(),
        numeric_entities
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<SceneEditorRightPanelScroll>>()
            .iter(world)
            .count(),
        1
    );
}

#[test]
fn full_sidebar_keeps_long_feedback_inside_scroll_and_navigation_fixed_at_low_height() {
    let (mut app, host) = page(true);
    app.world_mut().get_mut::<Node>(host).unwrap().width = px(200);
    app.world_mut().get_mut::<Node>(host).unwrap().height = px(350);
    app.world_mut().resource_mut::<SceneEditorState>().status =
        "预览构建失败，请检查完整的水域端点连接和中文资源路径。".repeat(16);
    app.update();
    let world = app.world_mut();
    for (_, node, layout) in world
        .query::<(
            &WaterSidebarFeedbackText,
            &ComputedNode,
            &bevy::text::TextLayoutInfo,
        )>()
        .iter(world)
    {
        assert!(layout.size.x <= node.size().x + 1.1);
        assert!(layout.size.y <= node.size().y + 1.1);
    }
    let scroll = world
        .query_filtered::<Entity, With<WaterSidebarScroll>>()
        .single(world)
        .unwrap();
    let node = world.get::<ComputedNode>(scroll).unwrap();
    assert!(node.content_size().y > node.size().y);
    let before = world
        .query::<(&WaterSidebarTab, &UiGlobalTransform)>()
        .iter(world)
        .map(|(_, transform)| transform.translation.y)
        .collect::<Vec<_>>();
    world.get_mut::<ScrollPosition>(scroll).unwrap().y = 120.0;
    app.update();
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<(&WaterSidebarTab, &UiGlobalTransform)>()
            .iter(world)
            .map(|(_, transform)| transform.translation.y)
            .collect::<Vec<_>>(),
        before
    );
}

#[test]
fn stable_frames_do_not_rewrite_fixed_feedback_tabs_or_panels() {
    #[derive(Resource, Default)]
    struct SidebarWrites(usize, usize, usize);

    fn probe(
        mut writes: ResMut<SidebarWrites>,
        texts: Query<(), (With<WaterSidebarFeedbackText>, Changed<Text>)>,
        styles: Query<
            (),
            (
                With<WaterSidebarTab>,
                Or<(Changed<BackgroundColor>, Changed<BorderColor>)>,
            ),
        >,
        panels: Query<(), (With<WaterSidebarPanel>, Changed<Node>)>,
    ) {
        *writes = SidebarWrites(
            texts.iter().count(),
            styles.iter().count(),
            panels.iter().count(),
        );
    }

    let (mut app, _) = page(false);
    app.init_resource::<SidebarWrites>().add_systems(
        Update,
        probe
            .after(super::systems::sync_feedback)
            .after(super::systems::sync_pages)
            .in_set(SceneEditorSystemSet::Presentation),
    );
    app.update();
    for _ in 0..5 {
        app.update();
        let writes = app.world().resource::<SidebarWrites>();
        assert_eq!((writes.0, writes.1, writes.2), (0, 0, 0));
    }
}
