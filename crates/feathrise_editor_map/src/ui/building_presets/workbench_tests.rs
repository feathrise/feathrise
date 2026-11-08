//! 建筑工作台的真实中文字体布局、单连接点控件预算与输入/关闭保护回归；不写作者文件或启动GPU。

use feathrise_editor_ui::InteractionButton;
use super::{components::*, interaction, list, presentation, types::*, workspace};
use crate::authoring::EditorAuthoringSession;
use crate::resources::MapEditorState;
use crate::ui::asset_preview::AssetPreviewState;
use crate::ui::text_input::{EditorTextInput, EditorTextInputGuard};
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::ecs::system::SystemState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, LineHeight};
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_map_content::{
    BuildingConnectionPointDescriptor, BuildingPresetDescriptor, BuildingPresetRegistry,
    MAX_BUILDING_CONNECTION_POINTS, MapPlacementAssetTarget,
};

const SECTIONS: [BuildingPresetSection; 4] = [
    BuildingPresetSection::Basics,
    BuildingPresetSection::Footprint,
    BuildingPresetSection::Entrance,
    BuildingPresetSection::Connections,
];

#[path = "../../../../feathrise_editor_ui/tests/support/render.rs"]
mod gpu;

#[test]
#[ignore = "显式渲染四个真实建筑预设分区，不加载三维场景或写作者文件"]
fn building_workbench_gpu_snapshot() {
    let (mut app, root, font) = gpu::app(960, 800);
    for section in SECTIONS {
        let mut state = state(section, 4, true);
        state.tabs[0].draft.descriptor.label = "城镇住宅".into();
        state.tabs[0].draft.descriptor.asset_path = "gltf/城镇/住宅.glb".into();
        let mut shared = VegetationPresetEditorState::default();
        shared.active_tab = PresetWorkspaceTab::Building(state.tabs[0].id);
        app.insert_resource(state).insert_resource(shared);
        render(&mut app, root, &font);
        gpu::snapshot(&mut app, &format!("building-{section:?}").to_lowercase());
        if section == BuildingPresetSection::Footprint {
            let world = app.world_mut();
            for (button, parent, node, size, at) in world.query::<(&BuildingPresetButton, &ChildOf, &Node, &ComputedNode, &UiGlobalTransform)>().iter(world) {
                if !matches!(button.0, BuildingPresetAction::Focus(_)) { continue; }
                let parent_node = world.get::<Node>(parent.parent()).unwrap();
                assert_eq!(parent_node.flex_direction, FlexDirection::Row);
                assert_eq!(node.height, Val::Px(26.0));
                for sibling in world.get::<Children>(parent.parent()).unwrap().iter() {
                    if let Some(text) = world.get::<Text>(sibling) {
                        let label = world.get::<ComputedNode>(sibling).unwrap();
                        assert!(label.size().y > 0.0 && label.size().y <= 36.0, "标签应保持一至两行：{}", text.0);
                        assert!((world.get::<UiGlobalTransform>(sibling).unwrap().translation.y - at.translation.y).abs() <= 1.0);
                    }
                }
                assert!(size.size().y <= 27.0);
            }
        }
    }
}

fn state(
    section: BuildingPresetSection,
    connections: usize,
    saved: bool,
) -> BuildingPresetEditorState {
    let mut descriptor = BuildingPresetDescriptor::from_asset(
        "house-a".into(),
        "窄窗口中文建筑预设名称".repeat(4),
        "gltf/中文长目录/城镇/住宅.glb".repeat(20),
        MapPlacementAssetTarget::Scene0,
    );
    descriptor.connections = (0..connections)
        .map(|index| BuildingConnectionPointDescriptor {
            id: format!("point-{index}"),
            ..default()
        })
        .collect();
    let mode = if saved {
        BuildingDraftMode::Edit {
            original_id: descriptor.id.clone(),
        }
    } else {
        BuildingDraftMode::Create
    };
    let mut draft = BuildingDraft::new(mode, descriptor);
    draft.section = section;
    let mut state = BuildingPresetEditorState::default();
    state.open_draft(draft);
    state.status.clear();
    state
}

fn active_draft(world: &World) -> &BuildingDraft {
    world
        .resource::<BuildingPresetEditorState>()
        .active_draft(world.resource::<VegetationPresetEditorState>().active_tab)
        .unwrap()
}

fn render(app: &mut App, host: Entity, font: &Handle<Font>) {
    // 测试显式重建与正式工作区共用同一渲染入口，不复制四分区布局或业务分派。
    gpu::clear(app, host);
    let mut command_state = SystemState::<(
        Commands,
        Res<BuildingPresetEditorState>,
        Res<VegetationPresetEditorState>,
    )>::new(app.world_mut());
    {
        let (mut commands, state, workspace) = command_state.get_mut(app.world_mut()).unwrap();
        assert!(workspace::spawn_active_building_editor(
            &mut commands,
            host,
            font,
            &state,
            workspace.active_tab
        ));
    }
    command_state.apply(app.world_mut());
}

fn page(
    width: f32,
    scale: f32,
    dpi: f32,
    state: BuildingPresetEditorState,
) -> (App, Entity, Handle<Font>) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
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
            bevy::ui::widget::update_editable_text_content_size,
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
    let host = app
        .world_mut()
        .spawn(Node {
            width: Val::Px(width / (scale * dpi)),
            min_width: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .id();
    let mut workspace = VegetationPresetEditorState::default();
    workspace.active_tab = PresetWorkspaceTab::Building(state.tabs[0].id);
    app.insert_resource(state)
        .insert_resource(workspace)
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        });
    app.insert_resource(BuildingPresetRegistry {
        catalog: default(),
        revision: 1,
        resolved: default(),
        last_error: None,
    });
    app.init_resource::<InputFocus>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<AssetPreviewState>()
        .init_resource::<EditorAuthoringSession>()
        .init_resource::<BuildingPlacementControls>()
        .add_message::<KeyboardInput>();
    app.add_systems(
        Update,
        (
            interaction::handle_text_input,
            interaction::handle_buttons,
            list::sync_building_preset_button_styles,
            interaction::sync_panel_text,
            presentation::sync_document_badge,
            presentation::sync_button_styles,
        )
            .chain(),
    );
    render(&mut app, host, &font);
    for _ in 0..4 {
        app.update();
    }
    (app, host, font)
}

fn button_entity(app: &mut App, action: BuildingPresetAction) -> Entity {
    let world = app.world_mut();
    world
        .query::<(Entity, &BuildingPresetButton)>()
        .iter(world)
        .find(|(_, button)| button.0 == action)
        .unwrap()
        .0
}

fn press(app: &mut App, action: BuildingPresetAction) {
    let world = app.world_mut();
    for mut interaction in world.query::<&mut Interaction>().iter_mut(world) {
        if *interaction == Interaction::Pressed {
            *interaction = Interaction::None;
        }
    }
    let entity = button_entity(app, action);
    app.world_mut()
        .entity_mut(entity)
        .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    app.world_mut().entity_mut(entity).insert(Interaction::None);
    app.update();
}

fn stale_action(app: &mut App, action: BuildingPresetAction) {
    // 仅为已退役标签/连接点票据构造迟到动作；正常用户路径通过真实渲染按钮测试。
    let entity = app
        .world_mut()
        .spawn((BuildingPresetButton(action), feathrise_editor_ui::EditorButtonClick::triggered(), Interaction::Pressed))
        .id();
    app.update();
    app.world_mut().despawn(entity);
}

#[test]
fn four_sections_fit_real_chinese_font_width_scale_and_dpi_without_author_changes() {
    for saved in [false, true] {
        for section in SECTIONS {
            for (width, scale, dpi) in [360.0, 760.0, 1200.0]
                .into_iter()
                .flat_map(|width| [0.6, 1.0, 1.5].map(|scale| (width, scale, 1.0)))
                .chain([(1100.0, 1.25, 2.0)])
            {
                let state = state(section, 3, saved);
                let before = state.tabs[0].draft.descriptor.clone();
                let (mut app, host, _) = page(width, scale, dpi, state);
                let world = app.world_mut();
                let (form, node, computed) = world
                    .query_filtered::<(Entity, &Node, &ComputedNode), With<BuildingDocumentForm>>()
                    .single(world)
                    .unwrap();
                assert_eq!(node.max_width, Val::Px(960.0));
                assert!(
                    computed.size().x > 0.0
                        && computed.size().x <= width.min(960.0 * scale * dpi) + 1.01
                );
                let header = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<BuildingDocumentHeader>>().single(world).unwrap();
                let header_bottom = header.1.translation.y + header.0.size().y * 0.5;
                let navigation = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<BuildingDocumentNavigation>>().single(world).unwrap();
                assert!(
                    navigation.1.translation.y - navigation.0.size().y * 0.5
                        >= header_bottom - 1.01
                );
                assert!(
                    world.get::<UiGlobalTransform>(form).unwrap().translation.y
                        - world.get::<ComputedNode>(form).unwrap().size().y * 0.5
                        >= navigation.1.translation.y + navigation.0.size().y * 0.5 - 1.01
                );
                let host_center = world.get::<UiGlobalTransform>(host).unwrap().translation.x;
                let host_width = world.get::<ComputedNode>(host).unwrap().size().x;
                let mut counts = (0, 0, 0, 0);
                let mut fields = Vec::new();
                for (button, kind, computed, transform) in world
                    .query::<(
                        &BuildingPresetButton,
                        &BuildingButtonKind,
                        &ComputedNode,
                        &UiGlobalTransform,
                    )>()
                    .iter(world)
                {
                    assert!(
                        computed.size().x > 0.0 && computed.size().x <= host_width + 1.01,
                        "{section:?} {width} {scale} {:?}",
                        button.0
                    );
                    assert!(
                        transform.translation.x - computed.size().x * 0.5
                            >= host_center - host_width * 0.5 - 1.01
                    );
                    assert!(
                        transform.translation.x + computed.size().x * 0.5
                            <= host_center + host_width * 0.5 + 1.01
                    );
                    match button.0 {
                        BuildingPresetAction::SelectSection(_) => {
                            counts.0 += 1;
                            assert_eq!(*kind, BuildingButtonKind::Navigation);
                        }
                        BuildingPresetAction::Save => {
                            counts.1 += 1;
                            assert_eq!(*kind, BuildingButtonKind::Primary);
                        }
                        BuildingPresetAction::Reset => counts.2 += 1,
                        BuildingPresetAction::Cancel => counts.3 += 1,
                        BuildingPresetAction::Focus(field) => {
                            assert!(!fields.contains(&field));
                            fields.push(field);
                            assert!((computed.size().y - 26.0 * scale * dpi).abs() <= 1.01);
                        }
                        _ => {}
                    }
                }
                assert_eq!(counts, (4, 1, 1, 1));
                assert_eq!(
                    fields.len(),
                    match section {
                        BuildingPresetSection::Basics => 1,
                        BuildingPresetSection::Footprint => 6,
                        BuildingPresetSection::Entrance => 10,
                        BuildingPresetSection::Connections => 5,
                    }
                );
                let mut source_rows = 0;
                for (text, layout, computed) in world
                    .query::<(&Text, &TextLayout, &ComputedNode)>()
                    .iter(world)
                {
                    if text.contains(&before.asset_path) {
                        source_rows += 1;
                        assert_eq!(layout.linebreak, LineBreak::AnyCharacter);
                        assert!(computed.size().y > 14.0 * scale * dpi);
                        assert!(computed.size().x <= host_width + 1.01);
                    }
                }
                assert_eq!(
                    source_rows,
                    usize::from(section == BuildingPresetSection::Basics)
                );
                assert_eq!(active_draft(world).descriptor, before);
                assert!(!active_draft(world).dirty());
            }
        }
    }
}

#[test]
fn official_single_line_controls_center_text_without_shrinking_the_click_area() {
    for section in [
        BuildingPresetSection::Basics,
        BuildingPresetSection::Connections,
    ] {
        for empty in [false, true] {
            for (scale, dpi) in [(0.6, 1.0), (1.0, 1.0), (1.5, 1.0), (1.25, 2.0)] {
                let mut state = state(section, 1, true);
                state.tabs[0].draft.descriptor.label = if empty { "" } else { "普通房屋" }.into();
                if empty {
                    state.tabs[0].draft.descriptor.connections[0].id.clear();
                    state.tabs[0].draft.descriptor.connections[0].kind.clear();
                }
                let (mut app, _, _) = page(760.0, scale, dpi, state);
                let world = app.world_mut();
                let mut entities = Vec::new();
                for (entity, editable, button, node, computed, line_height, font) in world
                    .query_filtered::<(
                        Entity,
                        &EditableText,
                        &BuildingPresetButton,
                        &Node,
                        &ComputedNode,
                        &LineHeight,
                        &TextFont,
                    ), With<EditorTextInput>>()
                    .iter(world)
                {
                    assert!(
                        matches!(button.0, BuildingPresetAction::Focus(field) if field.accepts_text())
                    );
                    assert!(world.get::<InteractionButton>(entity).is_some());
                    assert_eq!(*line_height, LineHeight::Px(14.0));
                    assert_eq!(node.height, Val::Px(26.0));
                    assert_eq!(node.padding.top, Val::Px(5.0));
                    assert_eq!(node.padding.bottom, Val::Px(5.0));
                    assert_eq!(font.font_size, FontSize::Px(feathrise_editor_ui::metrics::FONT_CONTROL));
                    assert!((computed.size().y - 26.0 * scale * dpi).abs() <= 1.01);
                    entities.push((entity, editable.value().to_string()));
                }
                assert_eq!(
                    entities.len(),
                    if section == BuildingPresetSection::Basics {
                        1
                    } else {
                        2
                    }
                );
                for _ in 0..3 {
                    app.update();
                }
                for (entity, value) in entities {
                    assert_eq!(
                        app.world()
                            .get::<EditableText>(entity)
                            .unwrap()
                            .value()
                            .to_string(),
                        value
                    );
                }
            }
        }
    }
}

#[test]
fn connection_count_does_not_multiply_editable_groups_or_mutate_selection_on_render() {
    for count in [0, 1, MAX_BUILDING_CONNECTION_POINTS] {
        for selected in [0, count.saturating_sub(1), usize::MAX] {
            let mut state = state(BuildingPresetSection::Connections, count, true);
            state.tabs[0].draft.selected_connection = selected;
            for (index, connection) in state.tabs[0]
                .draft
                .descriptor
                .connections
                .iter_mut()
                .enumerate()
            {
                connection.id = format!("{}-{index}", "connection".repeat(12));
            }
            state.tabs[0].draft.baseline = state.tabs[0].draft.descriptor.clone();
            let before = state.tabs[0].draft.descriptor.clone();
            let (mut app, host, _) = page(360.0, 1.5, 1.0, state);
            let world = app.world_mut();
            let host_width = world.get::<ComputedNode>(host).unwrap().size().x;
            for (button, computed) in world
                .query::<(&BuildingPresetButton, &ComputedNode)>()
                .iter(world)
            {
                if matches!(button.0, BuildingPresetAction::SelectConnection(_)) {
                    assert!(computed.size().x > 0.0 && computed.size().x <= host_width + 1.01);
                }
            }
            let actions: Vec<_> = world
                .query::<&BuildingPresetButton>()
                .iter(world)
                .map(|button| button.0)
                .collect();
            assert_eq!(
                actions
                    .iter()
                    .filter(|action| matches!(action, BuildingPresetAction::SelectConnection(_)))
                    .count(),
                count
            );
            assert_eq!(
                actions
                    .iter()
                    .filter(|action| matches!(action, BuildingPresetAction::Focus(_)))
                    .count(),
                if count == 0 { 0 } else { 5 }
            );
            assert_eq!(
                actions
                    .iter()
                    .filter(|action| matches!(
                        action,
                        BuildingPresetAction::RemoveConnectionPoint(_)
                    ))
                    .count(),
                usize::from(count > 0)
            );
            if count > 0 {
                let current = selected.min(count - 1);
                assert!(actions.contains(&BuildingPresetAction::Focus(
                    BuildingPresetField::ConnectionId(current)
                )));
                assert!(actions.contains(&BuildingPresetAction::RemoveConnectionPoint(current)));
            }
            assert_eq!(active_draft(world).selected_connection, selected);
            assert_eq!(active_draft(world).descriptor, before);
            assert!(!active_draft(world).dirty());
        }
    }
}

#[test]
fn section_navigation_keeps_invalid_buffer_and_ime_then_commits_latest_official_name() {
    let (mut app, host, font) = page(
        760.0,
        1.0,
        1.0,
        state(BuildingPresetSection::Basics, 1, true),
    );
    let controls = app.world().resource::<BuildingPlacementControls>().clone();
    press(
        &mut app,
        BuildingPresetAction::SelectSection(BuildingPresetSection::Footprint),
    );
    render(&mut app, host, &font);
    press(
        &mut app,
        BuildingPresetAction::Focus(BuildingPresetField::FootprintHalfX),
    );
    app.world_mut()
        .resource_mut::<BuildingPresetEditorState>()
        .input_buffer = "-".into();
    let before = active_draft(app.world()).descriptor.clone();
    press(
        &mut app,
        BuildingPresetAction::SelectSection(BuildingPresetSection::Entrance),
    );
    assert_eq!(
        active_draft(app.world()).section,
        BuildingPresetSection::Footprint
    );
    assert_eq!(active_draft(app.world()).descriptor, before);
    assert_eq!(
        app.world()
            .resource::<BuildingPresetEditorState>()
            .input_buffer,
        "-"
    );
    app.world_mut()
        .resource_mut::<BuildingPresetEditorState>()
        .input_buffer = "2".into();
    press(
        &mut app,
        BuildingPresetAction::SelectSection(BuildingPresetSection::Basics),
    );
    render(&mut app, host, &font);
    let official = button_entity(
        &mut app,
        BuildingPresetAction::Focus(BuildingPresetField::Label),
    );
    press(
        &mut app,
        BuildingPresetAction::Focus(BuildingPresetField::Label),
    );
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(official));
    app.world_mut()
        .entity_mut(official)
        .insert(EditableText::new("同帧最新中文建筑名称"));
    app.world_mut()
        .resource_mut::<BuildingPresetEditorState>()
        .input_buffer = "旧业务镜像".into();
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = true;
    press(
        &mut app,
        BuildingPresetAction::SelectSection(BuildingPresetSection::Footprint),
    );
    assert_eq!(
        active_draft(app.world()).section,
        BuildingPresetSection::Basics
    );
    assert_eq!(active_draft(app.world()).descriptor.label, before.label);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(official));
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = false;
    press(
        &mut app,
        BuildingPresetAction::SelectSection(BuildingPresetSection::Footprint),
    );
    assert_eq!(
        active_draft(app.world()).section,
        BuildingPresetSection::Footprint
    );
    assert_eq!(
        active_draft(app.world()).descriptor.label,
        "同帧最新中文建筑名称"
    );
    assert!(
        app.world()
            .resource::<BuildingPresetEditorState>()
            .active_field
            .is_none()
    );
    assert!(app.world().resource::<InputFocus>().get().is_none());
    assert_eq!(
        *app.world().resource::<BuildingPlacementControls>(),
        controls
    );
}

#[test]
fn connection_actions_select_new_point_keep_surviving_identity_and_preserve_the_existing_limit() {
    let (mut app, host, font) = page(
        760.0,
        1.0,
        1.0,
        state(BuildingPresetSection::Connections, 3, true),
    );
    let controls = app.world().resource::<BuildingPlacementControls>().clone();
    let before = active_draft(app.world()).descriptor.connections.clone();
    press(&mut app, BuildingPresetAction::SelectConnection(2));
    assert!(!active_draft(app.world()).dirty());
    press(&mut app, BuildingPresetAction::AddConnectionPoint);
    assert_eq!(active_draft(app.world()).selected_connection, 3);
    assert_eq!(
        active_draft(app.world()).descriptor.connections[..3],
        before
    );
    render(&mut app, host, &font);
    press(&mut app, BuildingPresetAction::RemoveConnectionPoint(3));
    assert_eq!(active_draft(app.world()).selected_connection, 2);
    assert_eq!(active_draft(app.world()).descriptor.connections, before);
    stale_action(&mut app, BuildingPresetAction::RemoveConnectionPoint(0));
    assert_eq!(active_draft(app.world()).selected_connection, 1);
    assert_eq!(
        active_draft(app.world()).descriptor.connections[1].id,
        "point-2"
    );
    let before = active_draft(app.world()).descriptor.clone();
    stale_action(&mut app, BuildingPresetAction::SelectConnection(30));
    assert_eq!(active_draft(app.world()).descriptor, before);
    assert_eq!(active_draft(app.world()).selected_connection, 1);
    assert!(
        app.world()
            .resource::<BuildingPresetEditorState>()
            .status
            .contains("已经不存在")
    );
    assert_eq!(
        *app.world().resource::<BuildingPlacementControls>(),
        controls
    );

    let (mut app, _, _) = page(
        760.0,
        1.0,
        1.0,
        state(
            BuildingPresetSection::Connections,
            MAX_BUILDING_CONNECTION_POINTS,
            true,
        ),
    );
    let before = active_draft(app.world()).descriptor.clone();
    press(&mut app, BuildingPresetAction::AddConnectionPoint);
    assert_eq!(active_draft(app.world()).descriptor, before);
    assert!(!active_draft(app.world()).dirty());
    assert!(
        app.world()
            .resource::<BuildingPresetEditorState>()
            .status
            .contains("最多只能配置32")
    );
}

#[test]
fn each_document_remembers_its_view_without_dirtying_or_serializing_it_and_reset_clamps_selection()
{
    let (mut app, host, font) = page(
        760.0,
        1.0,
        1.0,
        state(BuildingPresetSection::Connections, 3, true),
    );
    press(&mut app, BuildingPresetAction::SelectConnection(2));
    let original = active_draft(app.world()).clone();
    let first = app
        .world()
        .resource::<VegetationPresetEditorState>()
        .active_tab;
    let original_id = app.world().resource::<BuildingPresetEditorState>().tabs[0].id;
    let reused = app
        .world_mut()
        .resource_mut::<BuildingPresetEditorState>()
        .open_draft(original.clone());
    assert_eq!(reused, original_id);
    let mut second = original.clone();
    second.descriptor.id = "house-b".into();
    second.baseline = second.descriptor.clone();
    second.mode = BuildingDraftMode::Edit {
        original_id: "house-b".into(),
    };
    second.section = BuildingPresetSection::Basics;
    second.selected_connection = 0;
    let second_id = app
        .world_mut()
        .resource_mut::<BuildingPresetEditorState>()
        .open_draft(second);
    stale_action(&mut app, BuildingPresetAction::ActivateTab(second_id));
    render(&mut app, host, &font);
    press(
        &mut app,
        BuildingPresetAction::SelectSection(BuildingPresetSection::Footprint),
    );
    assert!(!active_draft(app.world()).dirty());
    stale_action(&mut app, BuildingPresetAction::ActivateTab(original_id));
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        first
    );
    assert_eq!(
        active_draft(app.world()).section,
        BuildingPresetSection::Connections
    );
    assert_eq!(active_draft(app.world()).selected_connection, 2);
    assert_eq!(
        serde_json::to_string(&active_draft(app.world()).descriptor).unwrap(),
        serde_json::to_string(&original.descriptor).unwrap()
    );
    render(&mut app, host, &font);
    press(&mut app, BuildingPresetAction::AddConnectionPoint);
    assert_eq!(active_draft(app.world()).selected_connection, 3);
    assert!(active_draft(app.world()).dirty());
    press(&mut app, BuildingPresetAction::Reset);
    assert_eq!(active_draft(app.world()).descriptor, original.descriptor);
    assert_eq!(
        active_draft(app.world()).section,
        BuildingPresetSection::Connections
    );
    assert_eq!(active_draft(app.world()).selected_connection, 2);
    assert!(!active_draft(app.world()).dirty());
    assert_eq!(
        app.world().resource::<BuildingPresetEditorState>().tabs[1]
            .draft
            .section,
        BuildingPresetSection::Footprint
    );
}

#[test]
fn header_badges_diagnostics_and_button_styles_keep_busy_and_dirty_close_protection() {
    let (mut app, _, _) = page(
        760.0,
        1.0,
        1.0,
        state(BuildingPresetSection::Basics, 0, true),
    );
    let world = app.world_mut();
    let sidebar = world
        .spawn((Text::default(), BuildingPresetStatusText))
        .id();
    let badge = world
        .query_filtered::<Entity, With<BuildingDocumentBadge>>()
        .single(world)
        .unwrap();
    assert_eq!(world.get::<Text>(badge).unwrap().as_str(), "已保存");
    let save = button_entity(&mut app, BuildingPresetAction::Save);
    assert_eq!(
        app.world().get::<BackgroundColor>(save).unwrap().0,
        crate::ui::theme::SELECT_BG
    );
    app.world_mut()
        .entity_mut(save)
        .insert(Interaction::Hovered);
    app.update();
    assert_eq!(
        app.world().get::<BackgroundColor>(save).unwrap().0,
        crate::ui::theme::BUTTON_DEV_SMALL_HOVERED
    );
    app.world_mut().entity_mut(save).insert(Interaction::None);
    app.update();
    assert_eq!(
        app.world().get::<BackgroundColor>(save).unwrap().0,
        crate::ui::theme::SELECT_BG
    );
    let selected = button_entity(
        &mut app,
        BuildingPresetAction::SelectSection(BuildingPresetSection::Basics),
    );
    let inactive = button_entity(
        &mut app,
        BuildingPresetAction::SelectSection(BuildingPresetSection::Entrance),
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(selected).unwrap().0,
        crate::ui::theme::SELECT_BG
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(inactive).unwrap().0,
        Color::NONE
    );
    {
        let mut state = app.world_mut().resource_mut::<BuildingPresetEditorState>();
        state.tabs[0].draft.descriptor.label = "未保存房屋".into();
        state.status = "目录错误保留上一完整快照".into();
        state.touch_ui();
    }
    app.update();
    assert_eq!(
        app.world().get::<Text>(badge).unwrap().as_str(),
        "未保存修改"
    );
    let world = app.world_mut();
    for text in world
        .query_filtered::<&Text, With<BuildingPresetStatusText>>()
        .iter(world)
    {
        assert_eq!(text.as_str(), "目录错误保留上一完整快照");
    }
    assert_eq!(
        world.get::<Text>(sidebar).unwrap().as_str(),
        "目录错误保留上一完整快照"
    );
    {
        let mut state = world.resource_mut::<BuildingPresetEditorState>();
        state.adoption = Some(BuildingCatalogAdoption {
            baseline_revision: 1,
            preset_id: "house-a".into(),
            deleted: false,
            tab_id: Some(state.tabs[0].id),
            message: "已写入等待采用".into(),
        });
    }
    app.update();
    assert_eq!(
        app.world().get::<Text>(badge).unwrap().as_str(),
        "等待热采用"
    );
    press(&mut app, BuildingPresetAction::Cancel);
    assert_eq!(
        app.world()
            .resource::<BuildingPresetEditorState>()
            .tabs
            .len(),
        1
    );
    assert!(!app.world().resource::<BuildingPresetEditorState>().tabs[0].close_confirmation);
    app.world_mut()
        .resource_mut::<BuildingPresetEditorState>()
        .adoption = None;
    press(&mut app, BuildingPresetAction::Cancel);
    assert!(app.world().resource::<BuildingPresetEditorState>().tabs[0].close_confirmation);
    press(&mut app, BuildingPresetAction::Cancel);
    assert!(
        app.world()
            .resource::<BuildingPresetEditorState>()
            .tabs
            .is_empty()
    );
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Map
    );

    let (mut app, _, _) = page(
        760.0,
        1.0,
        1.0,
        state(BuildingPresetSection::Basics, 0, false),
    );
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Text, With<BuildingDocumentBadge>>()
            .single(world)
            .unwrap()
            .as_str(),
        "新建未保存"
    );
    press(
        &mut app,
        BuildingPresetAction::Focus(BuildingPresetField::Label),
    );
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Text, With<BuildingDocumentBadge>>()
            .single(world)
            .unwrap()
            .as_str(),
        "字段输入中"
    );
}
