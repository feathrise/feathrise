//! 地表工作台的真实字体布局、分区会话与输入/资源事务回归；不写正式目录或启动GPU。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, interaction, presentation, types::*, workspace};
use crate::resources::MapEditorState;
use crate::ui::text_input::EditorTextInputGuard;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::ecs::system::SystemState;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_map_content::{SurfacePresetDescriptor, SurfaceProjection};

const SECTIONS: [SurfaceSection; 3] = [
    SurfaceSection::Textures,
    SurfaceSection::Material,
    SurfaceSection::Mapping,
];

#[path = "../../../../feathrise_editor_ui/tests/support/render.rs"]
mod gpu;

#[test]
#[ignore = "显式渲染三个真实地表预设分区，不加载派生贴图或写作者文件"]
fn surface_workbench_gpu_snapshot() {
    let (mut app, root, font) = gpu::app(960, 800);
    let extension = feathrise_editor_surface::extension();
    for section in SECTIONS {
        gpu::clear(&mut app, root);
        let mut state = state(section, true);
        state.tabs[0].draft.descriptor.label = "城镇石板路".into();
        let mut system = SystemState::<Commands>::new(app.world_mut());
        let mut commands = system.get_mut(app.world_mut()).unwrap();
        assert!(workspace::spawn_active_surface_editor(&mut commands, root, &font, &state,
            PresetWorkspaceTab::Surface(1), Some(&extension)));
        system.apply(app.world_mut());
        gpu::snapshot(&mut app, &format!("surface-{section:?}").to_lowercase());
    }
}

fn state(section: SurfaceSection, saved: bool) -> SurfacePresetEditorState {
    let descriptor = SurfacePresetDescriptor {
        id: "surface-fixture".into(),
        aliases: vec![],
        label: "用于窄窗口检查的地表长名称".repeat(4),
        base_color_srgba: [0.55, 0.48, 0.36, 1.0],
        base_color_texture_path: Some("textures/很长的中文目录/base.png".repeat(5)),
        normal_texture_path: Some("textures/很长的中文目录/normal.png".repeat(5)),
        arm_height_texture_path: Some("textures/很长的中文目录/arm.png".repeat(5)),
        tile_meters: 4.0,
        normal_strength: 1.0,
        normal_y_flip: false,
        height_blend_strength: 0.0,
        perceptual_roughness: 0.9,
        metallic: 0.0,
        crater_response: 1.0,
        projection: SurfaceProjection::WorldXz,
    };
    let mode = if saved {
        SurfaceDraftMode::Edit {
            original_id: descriptor.id.clone(),
        }
    } else {
        SurfaceDraftMode::Create
    };
    let mut state = SurfacePresetEditorState::default();
    state.open_draft(SurfaceDraft::new(mode, descriptor));
    state.tabs[0].section = section;
    state
}

fn page(width: f32, scale: f32, dpi: f32, state: SurfacePresetEditorState) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(feathrise_editor_surface::SurfaceEditorPlugin);
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
            ui_layout_system,
        )
            .chain(),
    );
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
    let mut command_state = SystemState::<Commands>::new(app.world_mut());
    {
        let mut commands = command_state.get_mut(app.world_mut()).unwrap();
        assert!(workspace::spawn_active_surface_editor(
            &mut commands,
            host,
            &face,
            &state,
            PresetWorkspaceTab::Surface(1),
            Some(&feathrise_editor_surface::extension())
        ));
    }
    command_state.apply(app.world_mut());
    let mut shared = VegetationPresetEditorState::default();
    shared.active_tab = PresetWorkspaceTab::Surface(1);
    app.insert_resource(state)
        .insert_resource(shared)
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        });
    for _ in 0..4 {
        app.update();
    }
    (app, host)
}

fn interaction_app(state: SurfacePresetEditorState) -> App {
    let mut app = App::new();
    app.add_plugins(feathrise_editor_surface::SurfaceEditorPlugin);
    let mut shared = VegetationPresetEditorState::default();
    shared.active_tab = PresetWorkspaceTab::Surface(1);
    app.insert_resource(state)
        .insert_resource(shared)
        .insert_resource(MapEditorState {
            open: true,
            ..default()
        });
    app.init_resource::<InputFocus>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<crate::authoring::EditorAuthoringSession>()
        .init_resource::<crate::authoring::SurfaceBrushStroke>()
        .init_resource::<crate::authoring::SurfaceLayerEditTask>()
        .add_systems(Update, interaction::handle_buttons);
    app
}

fn press(app: &mut App, action: SurfacePresetAction) {
    let button = app
        .world_mut()
        .spawn((SurfacePresetButton(action), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
        .id();
    app.update();
    app.world_mut().despawn(button);
}

#[test]
fn three_sections_fit_real_font_width_scale_and_dpi_without_mutating_authors() {
    for saved in [false, true] {
        for section in SECTIONS {
            for (width, scale, dpi) in [360.0, 760.0, 1200.0]
                .into_iter()
                .flat_map(|width| [0.6, 1.0, 1.5].map(|scale| (width, scale, 1.0)))
                .chain([(1100.0, 1.25, 2.0)])
            {
                let state = state(section, saved);
                let author = serde_json::to_string(&state.tabs[0].draft.descriptor).unwrap();
                let (mut app, host) = page(width, scale, dpi, state);
                let world = app.world_mut();
                let (form, node, computed) = world
                    .query_filtered::<(Entity, &Node, &ComputedNode), With<SurfaceEditorForm>>()
                    .single(world)
                    .unwrap();
                let form_width = computed.size().x;
                assert_eq!(node.max_width, Val::Px(960.0));
                assert!(
                    form_width > 0.0
                        && form_width <= width + 1.01
                        && form_width <= 960.0 * scale * dpi + 1.01
                );
                let header = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<SurfaceDocumentHeader>>().single(world).unwrap();
                let header_bottom = header.1.translation.y + header.0.size().y * 0.5;
                let navigation = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<SurfaceEditorNavigation>>().single(world).unwrap();
                assert!(
                    navigation.1.translation.y - navigation.0.size().y * 0.5
                        >= header_bottom - 1.01
                );
                assert!(
                    world.get::<UiGlobalTransform>(form).unwrap().translation.y
                        - world.get::<ComputedNode>(form).unwrap().size().y * 0.5
                        >= navigation.1.translation.y + navigation.0.size().y * 0.5 - 1.01
                );
                let host_at = world.get::<UiGlobalTransform>(host).unwrap().translation.x;
                let host_width = world.get::<ComputedNode>(host).unwrap().size().x;
                let mut fields = Vec::new();
                let mut navigation_count = 0;
                let mut primary_count = 0;
                let mut textures = 0;
                for (button, kind, computed, transform) in world
                    .query::<(
                        &SurfacePresetButton,
                        Option<&SurfaceButtonKind>,
                        &ComputedNode,
                        &UiGlobalTransform,
                    )>()
                    .iter(world)
                {
                    assert!(computed.size().x > 0.0 && computed.size().x <= width + 1.01);
                    assert!(
                        transform.translation.x - computed.size().x * 0.5
                            >= host_at - host_width * 0.5 - 1.01
                    );
                    assert!(
                        transform.translation.x + computed.size().x * 0.5
                            <= host_at + host_width * 0.5 + 1.01
                    );
                    match button.0 {
                        SurfacePresetAction::Focus(field) => {
                            fields.push(field);
                            assert!(computed.size().x <= form_width + 1.01);
                            assert!(computed.size().y >= feathrise_editor_ui::metrics::CONTROL_HEIGHT * scale * dpi - 1.01);
                        }
                        SurfacePresetAction::Section(_) => {
                            navigation_count += 1;
                            assert_eq!(kind, Some(&SurfaceButtonKind::Navigation));
                        }
                        SurfacePresetAction::Save => {
                            primary_count += 1;
                            assert_eq!(kind, Some(&SurfaceButtonKind::Primary));
                        }
                        SurfacePresetAction::ApplySelectedAsset(_) => textures += 1,
                        _ => {}
                    }
                }
                let expected = match section {
                    SurfaceSection::Textures => vec![SurfacePresetField::Label],
                    SurfaceSection::Material => vec![
                        SurfacePresetField::BaseColor(0),
                        SurfacePresetField::BaseColor(1),
                        SurfacePresetField::BaseColor(2),
                        SurfacePresetField::BaseColor(3),
                        SurfacePresetField::Roughness,
                        SurfacePresetField::Metallic,
                        SurfacePresetField::NormalStrength,
                    ],
                    SurfaceSection::Mapping => vec![
                        SurfacePresetField::TileMeters,
                        SurfacePresetField::HeightBlendStrength,
                        SurfacePresetField::CraterResponse,
                    ],
                };
                assert_eq!(fields.len(), expected.len());
                assert!(expected.iter().all(|field| fields.contains(field)));
                assert_eq!(
                    (navigation_count, primary_count, textures),
                    (
                        3,
                        1,
                        if section == SurfaceSection::Textures {
                            3
                        } else {
                            0
                        }
                    )
                );
                let before: Vec<_> = world
                    .query_filtered::<Entity, With<SurfacePresetButton>>()
                    .iter(world)
                    .collect();
                for _ in 0..3 {
                    app.update();
                }
                let world = app.world_mut();
                assert_eq!(
                    before,
                    world
                        .query_filtered::<Entity, With<SurfacePresetButton>>()
                        .iter(world)
                        .collect::<Vec<_>>()
                );
                let state = world.resource::<SurfacePresetEditorState>();
                assert_eq!(
                    author,
                    serde_json::to_string(&state.tabs[0].draft.descriptor).unwrap()
                );
                assert!(!state.tabs[0].draft.dirty() && !state.busy());
            }
        }
    }
}

#[test]
fn sections_are_per_document_and_do_not_change_author_data_or_start_work() {
    let mut state = state(SurfaceSection::Textures, true);
    let mut descriptor = state.tabs[0].draft.descriptor.clone();
    descriptor.id = "second-surface".into();
    state.open_draft(SurfaceDraft::new(
        SurfaceDraftMode::Edit {
            original_id: descriptor.id.clone(),
        },
        descriptor,
    ));
    let before: Vec<_> = state
        .tabs
        .iter()
        .map(|tab| serde_json::to_string(&tab.draft.descriptor).unwrap())
        .collect();
    let mut app = interaction_app(state);
    press(
        &mut app,
        SurfacePresetAction::Section(SurfaceSection::Material),
    );
    let revision = app
        .world()
        .resource::<SurfacePresetEditorState>()
        .ui_revision;
    press(
        &mut app,
        SurfacePresetAction::Section(SurfaceSection::Material),
    );
    assert_eq!(
        app.world()
            .resource::<SurfacePresetEditorState>()
            .ui_revision,
        revision
    );
    press(&mut app, SurfacePresetAction::ActivateTab(2));
    press(
        &mut app,
        SurfacePresetAction::Section(SurfaceSection::Mapping),
    );
    press(&mut app, SurfacePresetAction::ActivateTab(1));
    let state = app.world().resource::<SurfacePresetEditorState>();
    assert_eq!(
        (state.tabs[0].section, state.tabs[1].section),
        (SurfaceSection::Material, SurfaceSection::Mapping)
    );
    assert_eq!(
        before,
        state
            .tabs
            .iter()
            .map(|tab| serde_json::to_string(&tab.draft.descriptor).unwrap())
            .collect::<Vec<_>>()
    );
    assert!(state.tabs.iter().all(|tab| !tab.draft.dirty()));
    assert!(!state.busy());
    assert!(
        app.world()
            .resource::<crate::authoring::EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert!(!app.world().resource::<MapEditorState>().dirty);
}

#[test]
fn section_click_respects_ime_then_commits_latest_official_name() {
    let mut state = state(SurfaceSection::Textures, true);
    state.active_field = Some(SurfacePresetField::Label);
    state.input_buffer = "旧缓冲".into();
    let mut app = interaction_app(state);
    let name = app
        .world_mut()
        .spawn((
            SurfacePresetButton(SurfacePresetAction::Focus(SurfacePresetField::Label)),
            EditableText::new("最新确认中文"),
        ))
        .id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(name, FocusCause::Navigated);
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = true;
    press(
        &mut app,
        SurfacePresetAction::Section(SurfaceSection::Material),
    );
    let state = app.world().resource::<SurfacePresetEditorState>();
    assert_eq!(state.tabs[0].section, SurfaceSection::Textures);
    assert!(!state.tabs[0].draft.dirty());
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(name));
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = false;
    press(
        &mut app,
        SurfacePresetAction::Section(SurfaceSection::Material),
    );
    let state = app.world().resource::<SurfacePresetEditorState>();
    assert_eq!(state.tabs[0].draft.descriptor.label, "最新确认中文");
    assert_eq!(state.tabs[0].section, SurfaceSection::Material);
    assert!(state.active_field.is_none() && state.tabs[0].draft.dirty() && !state.busy());
    assert!(app.world().resource::<InputFocus>().get().is_none());
}

#[test]
fn invalid_pending_number_cannot_hide_its_page_and_valid_input_keeps_ranges() {
    let mut state = state(SurfaceSection::Mapping, true);
    state.active_field = Some(SurfacePresetField::TileMeters);
    state.input_buffer = "0".into();
    let mut app = interaction_app(state);
    for value in ["0", "-1", "NaN"] {
        app.world_mut()
            .resource_mut::<SurfacePresetEditorState>()
            .input_buffer = value.into();
        press(
            &mut app,
            SurfacePresetAction::Section(SurfaceSection::Material),
        );
        let state = app.world().resource::<SurfacePresetEditorState>();
        assert_eq!(state.tabs[0].section, SurfaceSection::Mapping);
        assert_eq!(state.tabs[0].draft.descriptor.tile_meters, 4.0);
        assert_eq!(state.active_field, Some(SurfacePresetField::TileMeters));
        assert!(!state.tabs[0].draft.dirty());
    }
    app.world_mut()
        .resource_mut::<SurfacePresetEditorState>()
        .input_buffer = "8".into();
    press(
        &mut app,
        SurfacePresetAction::Section(SurfaceSection::Material),
    );
    let state = app.world().resource::<SurfacePresetEditorState>();
    assert_eq!(state.tabs[0].draft.descriptor.tile_meters, 8.0);
    assert_eq!(state.tabs[0].section, SurfaceSection::Material);
    assert!(state.active_field.is_none() && state.tabs[0].draft.dirty());
}

#[test]
fn asset_browsing_remains_explicit_and_preserves_channel_and_format_rules() {
    let state = state(SurfaceSection::Textures, true);
    let before = state.tabs[0].draft.descriptor.clone();
    let mut app = interaction_app(state);
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_asset = Some("textures/new.PNG".into());
    app.update();
    assert_eq!(
        app.world().resource::<SurfacePresetEditorState>().tabs[0]
            .draft
            .descriptor,
        before
    );
    press(
        &mut app,
        SurfacePresetAction::ApplySelectedAsset(SurfaceTextureField::BaseColor),
    );
    let state = app.world().resource::<SurfacePresetEditorState>();
    assert_eq!(
        state.tabs[0]
            .draft
            .descriptor
            .base_color_texture_path
            .as_deref(),
        Some("textures/new.PNG")
    );
    assert_eq!(
        state.tabs[0].draft.descriptor.normal_texture_path,
        before.normal_texture_path
    );
    assert_eq!(
        state.tabs[0].draft.descriptor.arm_height_texture_path,
        before.arm_height_texture_path
    );
    assert_eq!(state.tabs[0].draft.descriptor.id, before.id);
    press(
        &mut app,
        SurfacePresetAction::ClearTexture(SurfaceTextureField::BaseColor),
    );
    assert!(
        app.world().resource::<SurfacePresetEditorState>().tabs[0]
            .draft
            .descriptor
            .base_color_texture_path
            .is_none()
    );
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_asset = Some("gltf/not-an-image.glb".into());
    press(
        &mut app,
        SurfacePresetAction::ApplySelectedAsset(SurfaceTextureField::BaseColor),
    );
    let state = app.world().resource::<SurfacePresetEditorState>();
    assert!(
        state.tabs[0]
            .draft
            .descriptor
            .base_color_texture_path
            .is_none()
    );
    assert!(!state.busy());
}

#[test]
fn primary_style_and_both_status_locations_survive_hover_and_adoption_edges() {
    let (mut app, _) = page(760.0, 1.0, 1.0, state(SurfaceSection::Textures, true));
    app.add_systems(
        Update,
        (
            presentation::sync_panel,
            presentation::sync_button_styles,
            presentation::sync_document_badge,
        )
            .chain(),
    );
    app.world_mut()
        .spawn((SurfacePresetStatusText, Text::new("旧侧栏诊断")));
    app.world_mut()
        .resource_mut::<SurfacePresetEditorState>()
        .status = "当前保存诊断".into();
    app.update();
    let save = app
        .world_mut()
        .query::<(Entity, &SurfacePresetButton)>()
        .iter(app.world())
        .find(|(_, button)| button.0 == SurfacePresetAction::Save)
        .unwrap()
        .0;
    for interaction in [UiInteraction::Hovered, UiInteraction::None] {
        *app.world_mut().get_mut::<UiInteraction>(save).unwrap() = interaction;
        app.update();
        assert_eq!(
            app.world().get::<BorderColor>(save).unwrap(),
            &BorderColor::all(crate::ui::theme::COPPER_BORDER)
        );
        let expected = if interaction == UiInteraction::None {
            crate::ui::theme::SELECT_BG
        } else {
            crate::ui::theme::SELECT_BG_HOVER
        };
        assert_eq!(
            app.world().get::<BackgroundColor>(save).unwrap().0,
            expected
        );
    }
    assert!(
        app.world_mut()
            .query_filtered::<&Text, With<SurfacePresetStatusText>>()
            .iter(app.world())
            .all(|text| text.as_str() == "当前保存诊断")
    );
    app.world_mut()
        .resource_mut::<SurfacePresetEditorState>()
        .adoption = Some(SurfaceCatalogAdoption {
        baseline_revision: 1,
        preset_id: "surface-fixture".into(),
        deleted: false,
        tab_id: Some(1),
        message: "等待采用".into(),
    });
    app.update();
    assert_eq!(
        app.world_mut()
            .query_filtered::<&Text, With<SurfaceDocumentBadge>>()
            .single(app.world())
            .unwrap()
            .as_str(),
        "保存 / 热采用中"
    );
    app.world_mut()
        .resource_mut::<SurfacePresetEditorState>()
        .adoption = None;
    app.update();
    assert_eq!(
        app.world_mut()
            .query_filtered::<&Text, With<SurfaceDocumentBadge>>()
            .single(app.world())
            .unwrap()
            .as_str(),
        "已保存"
    );
}

#[test]
fn header_close_retains_dirty_confirmation_and_busy_guard() {
    let mut state = state(SurfaceSection::Mapping, true);
    state.tabs[0].draft.descriptor.metallic = 0.2;
    let mut app = interaction_app(state);
    app.world_mut()
        .resource_mut::<SurfacePresetEditorState>()
        .adoption = Some(SurfaceCatalogAdoption {
        baseline_revision: 1,
        preset_id: "surface-fixture".into(),
        deleted: false,
        tab_id: Some(1),
        message: "等待采用".into(),
    });
    press(&mut app, SurfacePresetAction::Cancel);
    assert_eq!(
        app.world()
            .resource::<SurfacePresetEditorState>()
            .tabs
            .len(),
        1
    );
    app.world_mut()
        .resource_mut::<SurfacePresetEditorState>()
        .adoption = None;
    press(&mut app, SurfacePresetAction::Cancel);
    let state = app.world().resource::<SurfacePresetEditorState>();
    assert_eq!(state.tabs.len(), 1);
    assert!(state.tabs[0].close_confirmation);
    press(&mut app, SurfacePresetAction::Cancel);
    assert!(
        app.world()
            .resource::<SurfacePresetEditorState>()
            .tabs
            .is_empty()
    );
}
