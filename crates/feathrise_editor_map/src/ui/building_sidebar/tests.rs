//! 使用真实BSN、原领域输入与中文字体验证工具页；不启动GPU、保存文件或执行建筑作者事务。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::BuildingSidebarPage, scene::building_tools_panel};
use crate::MapEditorSystemSet;
use crate::authoring::EditorAuthoringSession;
use crate::resources::{MapEditorState, MapEditorViewportState};
use crate::types::EditorTool;
use crate::ui::asset_preview::AssetPreviewState;
use crate::ui::building_presets::{
    BuildingPlacementControls, BuildingPresetEditorState, BuildingPresetStatusText,
};
use crate::ui::courtyard::{
    CourtyardNameInput, CourtyardStatusText, CourtyardUiRequest, CourtyardUiState,
    CourtyardViewportMode,
};
use crate::ui::placement_transform::PlacementTransformInputState;
use crate::ui::sidebars::{EditorLeftSidebarTab, MapEditorSidebarState};
use crate::ui::text_input::EditorTextInputGuard;
use crate::ui::theme as colors;
use crate::ui::vegetation_preset_editor::VegetationPresetEditorState;
use crate::viewport::modes::{ViewportMode, ViewportModeState};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::{EditableText, LineHeight};
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_map_content::{
    BuildingPresetCatalogAsset, BuildingPresetRegistry, CourtyardPresetCatalogAsset,
    CourtyardPresetRegistry, ResolvedBuildingPresetCatalog, ResolvedCourtyardPresetCatalog,
};

fn page() -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
    ));
    app.init_resource::<UiSurface>()
        .init_resource::<InputFocus>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<PlacementTransformInputState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<AssetPreviewState>()
        .init_resource::<EditorAuthoringSession>()
        .insert_resource(MapEditorViewportState {
            _image: default(),
            camera: Entity::PLACEHOLDER,
            center: Vec3::ZERO,
            scene_center: Vec3::ZERO,
            scene_half_extents: Vec3::ONE,
            extent: 100.0,
            yaw: 0.0,
            pitch: 0.0,
            framed_map_id: None,
            render_size: UVec2::splat(4096),
        })
        .init_resource::<ViewportModeState>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .insert_resource(UiScale(1.0))
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        })
        .add_message::<KeyboardInput>();
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.right = crate::ui::sidebars::EditorRightSidebarTab::Tools;
    sidebar.left = EditorLeftSidebarTab::Building;
    app.insert_resource(sidebar);
    let buildings: BuildingPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/building_presets.building_presets.json"
    )))
    .unwrap();
    let courtyards: CourtyardPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/courtyard_presets.courtyard_presets.json"
    )))
    .unwrap();
    app.insert_resource(BuildingPresetRegistry {
        catalog: default(),
        revision: 1,
        resolved: ResolvedBuildingPresetCatalog::build(&buildings).unwrap(),
        last_error: None,
    });
    app.insert_resource(CourtyardPresetRegistry {
        catalog: default(),
        revision: 1,
        resolved: ResolvedCourtyardPresetCatalog::build(&courtyards).unwrap(),
        last_error: None,
    });
    app.configure_sets(
        Update,
        (
            MapEditorSystemSet::Input,
            MapEditorSystemSet::Tasks,
            MapEditorSystemSet::Presentation,
        )
            .chain(),
    );
    crate::ui::building_presets::install(&mut app);
    crate::ui::courtyard::install(&mut app);
    super::install(&mut app);
    app.add_systems(
        Update,
        crate::viewport::modes::sync_mode
            .after(MapEditorSystemSet::Input)
            .before(MapEditorSystemSet::Tasks),
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
            bevy::ui::widget::update_editable_text_content_size,
            ui_layout_system,
        )
            .chain(),
    );
    app.add_systems(Startup, building_tools_panel.spawn());
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
                    physical_size: UVec2::new(4096, 4096),
                    scale_factor: 1.0,
                }),
                ..default()
            },
            viewport: Some(Viewport {
                physical_size: UVec2::new(4096, 4096),
                ..default()
            }),
            ..default()
        },
    ));
    app.update();
    let world = app.world_mut();
    let root = world
        .query_filtered::<Entity, With<BuildingSidebarRoot>>()
        .single(world)
        .unwrap();
    let host = world
        .spawn(Node {
            width: px(240),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .id();
    world.entity_mut(root).insert(ChildOf(host));
    for mut text_font in world.query::<&mut TextFont>().iter_mut(world) {
        text_font.font = font.clone().into();
    }
    for _ in 0..6 {
        app.update();
    }
    (app, host)
}

fn button(app: &mut App, prefix: &str) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<(Entity, &Children), With<InteractionButton>>()
        .iter(world)
        .find(|(entity, children)| {
            world
                .get::<ComputedNode>(*entity)
                .is_some_and(|node| node.size().x > 0.0)
                && children.iter().any(|child| {
                    world
                        .get::<Text>(child)
                        .is_some_and(|text| text.as_str().starts_with(prefix))
                })
        })
        .unwrap_or_else(|| panic!("缺少真实按钮：{prefix}"))
        .0
}

fn press(app: &mut App, prefix: &str) -> Entity {
    let entity = button(app, prefix);
    app.world_mut()
        .entity_mut(entity)
        .insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    app.world_mut().entity_mut(entity).insert(UiInteraction::None);
    app.update();
    entity
}

fn name_entity(app: &mut App) -> Entity {
    let world = app.world_mut();
    world
        .query_filtered::<Entity, With<CourtyardNameInput>>()
        .single(world)
        .unwrap()
}

fn text<Marker: Component>(app: &mut App) -> String {
    let world = app.world_mut();
    world
        .query_filtered::<&Text, With<Marker>>()
        .single(world)
        .unwrap()
        .0
        .clone()
}

fn button_label(app: &App, entity: Entity) -> String {
    app.world()
        .get::<Children>(entity)
        .unwrap()
        .iter()
        .find_map(|child| app.world().get::<Text>(child).map(|text| text.0.clone()))
        .unwrap()
}

#[test]
fn top_tabs_keep_one_official_name_and_original_domain_entities() {
    let (mut app, _) = page();
    let name = name_entity(&mut app);
    let mut before: Vec<_> = app
        .world()
        .iter_entities()
        .map(|entity| entity.id())
        .collect();
    before.sort_unstable(); // 添加点击组件会迁移archetype，实体身份集合不依赖ECS迭代顺序。
    assert_eq!(
        *app.world().resource::<BuildingSidebarPage>(),
        BuildingSidebarPage::Single
    );
    for _ in 0..3 {
        press(&mut app, "院落组合");
        assert_eq!(
            *app.world().resource::<BuildingSidebarPage>(),
            BuildingSidebarPage::Courtyard
        );
        press(&mut app, "单栋建筑");
        assert_eq!(name_entity(&mut app), name);
    }
    let mut after: Vec<_> = app
        .world()
        .iter_entities()
        .map(|entity| entity.id())
        .collect();
    after.sort_unstable();
    assert_eq!(before, after, "切页不得销毁目录行或正式输入控件");
    let world = app.world_mut();
    assert_eq!(world.query::<&BuildingSidebarTab>().iter(world).count(), 2);
    for (panel, node) in world.query::<(&BuildingSidebarPanel, &Node)>().iter(world) {
        assert_eq!(
            node.display,
            if panel.0 == BuildingSidebarPage::Single {
                Display::Flex
            } else {
                Display::None
            }
        );
    }
}

#[test]
fn switching_cancels_hidden_tool_but_preserves_latest_name_selection_and_parameters() {
    let (mut app, _) = page();
    press(&mut app, "院落组合");
    press(&mut app, "盖印所选院落");
    assert_eq!(
        app.world().resource::<CourtyardUiState>().mode,
        CourtyardViewportMode::Stamp
    );
    let name = name_entity(&mut app);
    app.world_mut()
        .entity_mut(name)
        .insert(EditableText::new("同帧最新中文院落"));
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(name, FocusCause::Navigated);
    app.world_mut()
        .resource_mut::<CourtyardUiState>()
        .selected_placement_ids = vec!["member-a".into(), "member-b".into()];
    let controls = BuildingPlacementControls {
        yaw_degrees: 270.0,
        grid_step_meters: 2.0,
        connection_snap: true,
        level_terrain: true,
    };
    app.world_mut().insert_resource(controls.clone());
    press(&mut app, "单栋建筑");
    let state = app.world().resource::<CourtyardUiState>();
    assert_eq!(state.mode, CourtyardViewportMode::Idle);
    assert_eq!(state.name, "同帧最新中文院落");
    assert_eq!(state.selected_placement_ids, ["member-a", "member-b"]);
    assert_eq!(
        *app.world().resource::<BuildingPlacementControls>(),
        controls
    );
    assert_eq!(
        app.world().resource::<ViewportModeState>().mode,
        ViewportMode::Select
    );
    assert_eq!(
        app.world().resource::<MapEditorState>().tool,
        EditorTool::Inspect
    );
    assert!(app.world().resource::<InputFocus>().get().is_none());
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .is_none()
    );
    assert_eq!(name_entity(&mut app), name);
}

#[test]
fn ime_confirmation_guard_blocks_leaving_but_same_tab_keeps_focus() {
    let (mut app, _) = page();
    press(&mut app, "院落组合");
    let name = name_entity(&mut app);
    app.world_mut()
        .entity_mut(name)
        .insert(EditableText::new("已确认名称"));
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(name, FocusCause::Navigated);
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = true;
    press(&mut app, "院落组合");
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(name));
    press(&mut app, "单栋建筑");
    assert_eq!(
        *app.world().resource::<BuildingSidebarPage>(),
        BuildingSidebarPage::Courtyard
    );
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(name));
    assert!(text::<CourtyardStatusText>(&mut app).contains("请先确认输入法候选"));
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = false;
    press(&mut app, "单栋建筑");
    assert_eq!(
        app.world().resource::<CourtyardUiState>().name,
        "已确认名称"
    );
}

#[test]
fn closed_modal_or_other_module_does_not_consume_hidden_tabs() {
    let (mut app, _) = page();
    for condition in 0..4 {
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.open = condition != 0;
            editor.close_requested = condition == 1;
            editor.new_map_dialog_open = condition == 2;
        }
        app.world_mut().resource_mut::<MapEditorSidebarState>().left = if condition == 3 {
            EditorLeftSidebarTab::Terrain
        } else {
            EditorLeftSidebarTab::Building
        };
        press(&mut app, "院落组合");
        assert_eq!(
            *app.world().resource::<BuildingSidebarPage>(),
            BuildingSidebarPage::Single
        );
    }
}

#[test]
fn armed_tools_reveal_their_page_and_original_cancel_restores_single_tool() {
    let (mut app, _) = page();
    press(&mut app, "院落组合");
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
    app.world_mut().resource_mut::<MapEditorState>().tool = EditorTool::Building;
    app.update();
    assert_eq!(
        *app.world().resource::<BuildingSidebarPage>(),
        BuildingSidebarPage::Single
    );
    press(&mut app, "院落组合");
    press(&mut app, "盖印所选院落");
    assert_eq!(
        *app.world().resource::<BuildingSidebarPage>(),
        BuildingSidebarPage::Courtyard
    );
    assert_eq!(
        app.world().resource::<ViewportModeState>().mode,
        ViewportMode::Edit
    );
    assert_eq!(
        app.world().resource::<MapEditorState>().tool,
        EditorTool::Inspect
    );
    press(&mut app, "取消当前工具");
    assert_eq!(
        app.world().resource::<CourtyardUiState>().mode,
        CourtyardViewportMode::Idle
    );
    assert_eq!(
        app.world().resource::<MapEditorState>().tool,
        EditorTool::Building
    );
    assert_eq!(
        *app.world().resource::<BuildingSidebarPage>(),
        BuildingSidebarPage::Single
    );
}

#[test]
fn parameter_labels_and_highlights_follow_the_original_actions() {
    let (mut app, _) = page();
    for step in [2.0, 4.0, 0.0, 0.5, 1.0] {
        let entity = press(&mut app, "网格：");
        assert_eq!(
            app.world()
                .resource::<BuildingPlacementControls>()
                .grid_step_meters,
            step
        );
        let value = if step == 0.0 {
            "关闭".into()
        } else {
            format!("{step:.1} m")
        };
        assert_eq!(button_label(&app, entity), format!("网格：{value} · 切换"));
        assert_eq!(
            app.world().get::<BackgroundColor>(entity).unwrap().0,
            if step > 0.0 {
                colors::SELECT_BG
            } else {
                colors::BUTTON_DEV_SMALL_NORMAL
            }
        );
    }
    for on in [true, false] {
        let connection = press(&mut app, "连接吸附：");
        let leveling = press(&mut app, "单栋整平：");
        let value = if on { "开启" } else { "关闭" };
        assert_eq!(button_label(&app, connection), format!("连接吸附：{value}"));
        assert_eq!(button_label(&app, leveling), format!("单栋整平：{value}"));
        let controls = app.world().resource::<BuildingPlacementControls>();
        assert_eq!(controls.connection_snap, on);
        assert_eq!(controls.level_terrain, on);
        for entity in [connection, leveling] {
            assert_eq!(
                app.world().get::<BackgroundColor>(entity).unwrap().0,
                if on {
                    colors::SELECT_BG
                } else {
                    colors::BUTTON_DEV_SMALL_NORMAL
                }
            );
        }
    }
    press(&mut app, "右转 90°");
    assert_eq!(
        app.world()
            .resource::<BuildingPlacementControls>()
            .yaw_degrees,
        90.0
    );
    press(&mut app, "左转 90°");
    assert_eq!(
        app.world()
            .resource::<BuildingPlacementControls>()
            .yaw_degrees,
        0.0
    );
}

#[test]
fn common_feedback_retains_domain_errors_and_save_selection_message() {
    let (mut app, _) = page();
    app.world_mut()
        .resource_mut::<BuildingPresetEditorState>()
        .report_status("地基坡差超过允许值");
    app.world_mut()
        .resource_mut::<BuildingPresetRegistry>()
        .last_error = Some("坏建筑目录".into());
    app.world_mut()
        .resource_mut::<CourtyardPresetRegistry>()
        .last_error = Some("坏院落目录".into());
    app.world_mut().resource_mut::<CourtyardUiState>().status = "成员门前空间冲突".into();
    app.update();
    assert!(text::<BuildingPresetStatusText>(&mut app).contains("地基坡差超过允许值"));
    assert!(text::<BuildingPresetStatusText>(&mut app).contains("坏建筑目录"));
    press(&mut app, "院落组合");
    assert!(text::<CourtyardStatusText>(&mut app).contains("成员门前空间冲突"));
    assert!(text::<CourtyardStatusText>(&mut app).contains("坏院落目录"));
    let name = name_entity(&mut app);
    app.world_mut()
        .entity_mut(name)
        .insert(EditableText::new("新院落名称"));
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(name, FocusCause::Navigated);
    app.world_mut()
        .resource_mut::<CourtyardUiState>()
        .selected_placement_ids = vec!["member-a".into()];
    press(&mut app, "保存选集为院落预设");
    let messages: Vec<_> = app
        .world_mut()
        .resource_mut::<Messages<CourtyardUiRequest>>()
        .drain()
        .collect();
    assert!(
        matches!(&messages[..], [CourtyardUiRequest::SaveSelection { name, placement_ids }] if name == "新院落名称" && placement_ids == &["member-a"])
    );
}

#[test]
fn chinese_buttons_fit_narrow_sidebar_at_multiple_scales_and_dpi() {
    let (mut app, host) = page();
    let name = name_entity(&mut app);
    for width in [180.0, 200.0, 280.0, 420.0] {
        for scale in [0.85, 1.0, 1.5] {
            for dpi in [1.0, 2.0] {
                app.world_mut().get_mut::<Node>(host).unwrap().width = px(width);
                app.world_mut().resource_mut::<UiScale>().0 = scale;
                let world = app.world_mut();
                for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                    camera.computed.target_info.as_mut().unwrap().scale_factor = dpi;
                }
                for label in ["单栋建筑", "院落组合"] {
                    press(&mut app, label);
                    app.update();
                    let world = app.world_mut();
                    let host_width = world.get::<ComputedNode>(host).unwrap().size().x;
                    let center = world.get::<UiGlobalTransform>(host).unwrap().translation.x;
                    let mut visible = 0;
                    for (node, transform) in world
                        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<InteractionButton>>()
                        .iter(world)
                    {
                        if node.size().x == 0.0 {
                            continue;
                        }
                        visible += 1;
                        assert!(
                            node.size().x <= host_width + 1.1,
                            "{label} {width} {scale} {dpi}"
                        );
                        assert!(
                            transform.translation.x - node.size().x * 0.5
                                >= center - host_width * 0.5 - 1.1
                        );
                        assert!(
                            transform.translation.x + node.size().x * 0.5
                                <= center + host_width * 0.5 + 1.1
                        );
                    }
                    assert!(visible >= 10);
                    for (_, children, computed) in world
                        .query::<(&BuildingSidebarTab, &Children, &ComputedNode)>()
                        .iter(world)
                    {
                        let text = children.iter().next().unwrap();
                        assert!(
                            world
                                .get::<bevy::text::TextLayoutInfo>(text)
                                .unwrap()
                                .size
                                .x
                                <= computed.size().x + 1.1
                        );
                    }
                    if label == "院落组合" {
                        assert_eq!(
                            *world.get::<LineHeight>(name).unwrap(),
                            LineHeight::Px(18.0)
                        );
                        let parent = world.get::<ChildOf>(name).unwrap().parent();
                        let field_center =
                            world.get::<UiGlobalTransform>(name).unwrap().translation.y;
                        let frame_center = world
                            .get::<UiGlobalTransform>(parent)
                            .unwrap()
                            .translation
                            .y;
                        assert!(
                            (field_center - frame_center).abs() <= 1.1,
                            "中文名称未垂直居中"
                        );
                    }
                }
            }
        }
    }
}
