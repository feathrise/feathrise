//! 水体工作台的真实字体布局、分区会话与输入/资源事务回归；不写正式目录或启动GPU。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, interaction, presentation, types::*, workspace};
use crate::resources::SceneEditorState;
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
use feathrise_scene_content::{WaterMacroWave, WaterMicroNormalLayer, WaterPresetDescriptor};

const SECTIONS: [WaterSection; 4] = [
    WaterSection::Appearance,
    WaterSection::Waves,
    WaterSection::Normals,
    WaterSection::Material,
];

#[path = "../../../../feathrise_editor_ui/tests/support/render.rs"]
mod gpu;

#[test]
#[ignore = "显式渲染四个真实水体预设分区，不加载三维预览或写作者文件"]
fn water_workbench_gpu_snapshot() {
    let (mut app, root, font) = gpu::app(960, 800);
    for section in SECTIONS {
        gpu::clear(&mut app, root);
        let mut state = state(section, true);
        state.tabs[0].draft.descriptor.label = "清澈河水".into();
        for normal in &mut state.tabs[0].draft.descriptor.micro_normals { normal.texture_path = Some("textures/水面/normal.ktx2".into()); }
        let mut system = SystemState::<Commands>::new(app.world_mut());
        let mut commands = system.get_mut(app.world_mut()).unwrap();
        assert!(workspace::spawn_active_water_editor(&mut commands, root, &font, &state, PresetWorkspaceTab::Water(1)));
        system.apply(app.world_mut());
        gpu::snapshot(&mut app, &format!("water-{section:?}").to_lowercase());
    }
}

fn state(section: WaterSection, saved: bool) -> WaterPresetEditorState {
    let descriptor = WaterPresetDescriptor {
        id: "water-fixture".into(),
        aliases: vec![],
        label: "用于窄窗口检查的水体长名称".repeat(4),
        base_color_srgba: [0.05, 0.18, 0.24, 0.72],
        perceptual_roughness: 0.12,
        reflectance: 0.5,
        trough_darkening: 0.08,
        shore_wave_attenuation: 1.0,
        macro_waves: [WaterMacroWave {
            direction: [1.0, 0.0],
            amplitude_meters: 0.1,
            wavelength_meters: 8.0,
            speed_meters_per_second: 0.6,
            steepness: 0.1,
        }; 4],
        micro_normals: std::array::from_fn(|_| WaterMicroNormalLayer {
            texture_path: Some("textures/用于检查换行的很长中文目录/normal.ktx2".repeat(5)),
            tile_meters: 1.0,
            velocity_meters_per_second: [0.0, 0.0],
            strength: 0.4,
        }),
    };
    let mode = if saved {
        WaterDraftMode::Edit {
            original_id: descriptor.id.clone(),
        }
    } else {
        WaterDraftMode::Create
    };
    let mut state = WaterPresetEditorState::default();
    state.open_draft(WaterDraft::new(mode, descriptor));
    state.tabs[0].section = section;
    state
}

fn page(width: f32, scale: f32, dpi: f32, state: WaterPresetEditorState) -> (App, Entity) {
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
        assert!(workspace::spawn_active_water_editor(
            &mut commands,
            host,
            &face,
            &state,
            PresetWorkspaceTab::Water(1)
        ));
    }
    command_state.apply(app.world_mut());
    let mut shared = VegetationPresetEditorState::default();
    shared.active_tab = PresetWorkspaceTab::Water(1);
    app.insert_resource(state)
        .insert_resource(shared)
        .insert_resource(SceneEditorState {
            open: true,
            ..default()
        });
    for _ in 0..4 {
        app.update();
    }
    (app, host)
}

pub(super) fn interaction_app(state: WaterPresetEditorState) -> App {
    let mut app = App::new();
    let mut shared = VegetationPresetEditorState::default();
    shared.active_tab = PresetWorkspaceTab::Water(1);
    app.insert_resource(state)
        .insert_resource(shared)
        .insert_resource(SceneEditorState {
            open: true,
            ..default()
        });
    app.init_resource::<InputFocus>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<crate::authoring::EditorAuthoringSession>()
        .add_systems(Update, interaction::handle_buttons);
    app
}

pub(super) fn press(app: &mut App, action: WaterPresetAction) {
    let button = app
        .world_mut()
        .spawn((WaterPresetButton(action), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
        .id();
    app.update();
    app.world_mut().despawn(button);
}

#[test]
fn four_sections_fit_real_font_width_scale_and_dpi_without_mutating_authors() {
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
                    .query_filtered::<(Entity, &Node, &ComputedNode), With<WaterEditorForm>>()
                    .single(world)
                    .unwrap();
                let form_width = computed.size().x;
                assert_eq!(node.max_width, Val::Percent(100.0));
                assert!(
                    form_width > 0.0
                        && form_width <= width + 1.01
                );
                let header = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<WaterDocumentHeader>>().single(world).unwrap();
                let header_bottom = header.1.translation.y + header.0.size().y * 0.5;
                let navigation = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<WaterEditorNavigation>>().single(world).unwrap();
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
                        &WaterPresetButton,
                        Option<&WaterButtonKind>,
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
                        WaterPresetAction::Focus(field) => {
                            fields.push(field);
                            assert!(computed.size().x <= form_width + 1.01);
                            assert!(computed.size().y >= 24.0 * scale * dpi - 1.01);
                        }
                        WaterPresetAction::Section(_) => {
                            navigation_count += 1;
                            assert_eq!(kind, Some(&WaterButtonKind::Navigation));
                        }
                        WaterPresetAction::Save => {
                            primary_count += 1;
                            assert_eq!(kind, Some(&WaterButtonKind::Primary));
                        }
                        WaterPresetAction::ApplySelectedAsset(_) => textures += 1,
                        _ => {}
                    }
                }
                let expected = match section {
                    WaterSection::Appearance => vec![
                        WaterPresetField::Label,
                        WaterPresetField::BaseColor(0),
                        WaterPresetField::BaseColor(1),
                        WaterPresetField::BaseColor(2),
                        WaterPresetField::BaseColor(3),
                    ],
                    WaterSection::Waves => (0..4)
                        .flat_map(|wave| {
                            [
                                WaterPresetField::WaveDirection { wave, component: 0 },
                                WaterPresetField::WaveDirection { wave, component: 1 },
                                WaterPresetField::WaveAmplitude(wave),
                                WaterPresetField::WaveWavelength(wave),
                                WaterPresetField::WaveSpeed(wave),
                                WaterPresetField::WaveSteepness(wave),
                            ]
                        })
                        .collect(),
                    WaterSection::Normals => (0..2)
                        .flat_map(|layer| {
                            [
                                WaterPresetField::NormalTile(layer),
                                WaterPresetField::NormalVelocity {
                                    layer,
                                    component: 0,
                                },
                                WaterPresetField::NormalVelocity {
                                    layer,
                                    component: 1,
                                },
                                WaterPresetField::NormalStrength(layer),
                            ]
                        })
                        .collect(),
                    WaterSection::Material => vec![
                        WaterPresetField::Roughness,
                        WaterPresetField::Reflectance,
                        WaterPresetField::TroughDarkening,
                        WaterPresetField::ShoreWaveAttenuation,
                    ],
                };
                assert_eq!(fields.len(), expected.len());
                assert!(expected.iter().all(|field| fields.contains(field)));
                assert_eq!(
                    (navigation_count, primary_count, textures),
                    (
                        4,
                        1,
                        if section == WaterSection::Normals {
                            2
                        } else {
                            0
                        }
                    )
                );
                let before: Vec<_> = world
                    .query_filtered::<Entity, With<WaterPresetButton>>()
                    .iter(world)
                    .collect();
                for _ in 0..3 {
                    app.update();
                }
                let world = app.world_mut();
                assert_eq!(
                    before,
                    world
                        .query_filtered::<Entity, With<WaterPresetButton>>()
                        .iter(world)
                        .collect::<Vec<_>>()
                );
                let state = world.resource::<WaterPresetEditorState>();
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
fn sections_are_per_document_and_do_not_change_author_data_or_start_scene_work() {
    let mut state = state(WaterSection::Appearance, true);
    let mut descriptor = state.tabs[0].draft.descriptor.clone();
    descriptor.id = "second-water".into();
    state.open_draft(WaterDraft::new(
        WaterDraftMode::Edit {
            original_id: descriptor.id.clone(),
        },
        descriptor,
    ));
    let before: Vec<_> = state
        .tabs
        .iter()
        .map(|tab| tab.draft.descriptor.clone())
        .collect();
    let mut app = interaction_app(state);
    press(&mut app, WaterPresetAction::Section(WaterSection::Waves));
    let revision = app.world().resource::<WaterPresetEditorState>().ui_revision;
    let shared_revision = app
        .world()
        .resource::<VegetationPresetEditorState>()
        .ui_revision;
    press(&mut app, WaterPresetAction::Section(WaterSection::Waves));
    assert_eq!(
        app.world().resource::<WaterPresetEditorState>().ui_revision,
        revision
    );
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .ui_revision,
        shared_revision
    );
    press(&mut app, WaterPresetAction::ActivateTab(2));
    press(&mut app, WaterPresetAction::Section(WaterSection::Normals));
    press(&mut app, WaterPresetAction::ActivateTab(1));
    let state = app.world().resource::<WaterPresetEditorState>();
    assert_eq!(
        (state.tabs[0].section, state.tabs[1].section),
        (WaterSection::Waves, WaterSection::Normals)
    );
    assert_eq!(
        before,
        state
            .tabs
            .iter()
            .map(|tab| tab.draft.descriptor.clone())
            .collect::<Vec<_>>()
    );
    assert!(state.tabs.iter().all(|tab| !tab.draft.dirty()));
    assert!(!state.busy());
    let session = app
        .world()
        .resource::<crate::authoring::EditorAuthoringSession>();
    assert_eq!(session.water_revision, 0);
    assert!(session.undo.is_empty() && session.redo.is_empty());
}

#[test]
fn section_change_respects_ime_guard_and_reads_latest_confirmed_name() {
    let mut state = state(WaterSection::Appearance, true);
    state.active_field = Some(WaterPresetField::Label);
    state.input_buffer = "旧缓冲".into();
    let mut app = interaction_app(state);
    let name = app
        .world_mut()
        .spawn((
            WaterPresetButton(WaterPresetAction::Focus(WaterPresetField::Label)),
            EditableText::new("最后确认的湖面名称"),
        ))
        .id();
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(name, FocusCause::Navigated);
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = true;
    let before = app.world().resource::<WaterPresetEditorState>().ui_revision;
    press(&mut app, WaterPresetAction::Section(WaterSection::Material));
    let state = app.world().resource::<WaterPresetEditorState>();
    assert_eq!(state.tabs[0].section, WaterSection::Appearance);
    assert_eq!(state.ui_revision, before);
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(name));
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = false;
    press(&mut app, WaterPresetAction::Section(WaterSection::Material));
    let state = app.world().resource::<WaterPresetEditorState>();
    assert_eq!(state.tabs[0].section, WaterSection::Material);
    assert_eq!(state.tabs[0].draft.descriptor.label, "最后确认的湖面名称");
    assert!(state.active_field.is_none());
    assert_eq!(app.world().resource::<InputFocus>().get(), None);
}

#[test]
fn invalid_numeric_buffer_keeps_section_and_valid_value_commits_before_leaving() {
    let mut state = state(WaterSection::Waves, true);
    state.active_field = Some(WaterPresetField::WaveAmplitude(2));
    state.input_buffer = "1e".into();
    let author = state.tabs[0].draft.descriptor.clone();
    let revision = state.ui_revision;
    let mut app = interaction_app(state);
    press(&mut app, WaterPresetAction::Section(WaterSection::Normals));
    let state = app.world().resource::<WaterPresetEditorState>();
    assert_eq!(state.tabs[0].section, WaterSection::Waves);
    assert_eq!(state.input_buffer, "1e");
    assert_eq!(state.tabs[0].draft.descriptor, author);
    assert_eq!(state.ui_revision, revision);
    app.world_mut()
        .resource_mut::<WaterPresetEditorState>()
        .input_buffer = "0.45".into();
    press(&mut app, WaterPresetAction::Section(WaterSection::Normals));
    let state = app.world().resource::<WaterPresetEditorState>();
    assert_eq!(state.tabs[0].section, WaterSection::Normals);
    assert_eq!(
        state.tabs[0].draft.descriptor.macro_waves[2].amplitude_meters,
        0.45
    );
    assert_eq!(
        state.tabs[0].draft.descriptor.macro_waves[1].amplitude_meters,
        0.1
    );
    assert!(state.active_field.is_none() && state.input_buffer.is_empty());
}

#[test]
fn micro_normal_adoption_is_explicit_and_isolated_per_layer() {
    let mut app = interaction_app(state(WaterSection::Normals, true));
    let original = app.world().resource::<WaterPresetEditorState>().tabs[0]
        .draft
        .descriptor
        .clone();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("textures/water.KTX2".into());
    app.update();
    assert_eq!(
        app.world().resource::<WaterPresetEditorState>().tabs[0]
            .draft
            .descriptor,
        original
    );
    press(&mut app, WaterPresetAction::ApplySelectedAsset(1));
    let descriptor = &app.world().resource::<WaterPresetEditorState>().tabs[0]
        .draft
        .descriptor;
    assert_eq!(descriptor.micro_normals[0], original.micro_normals[0]);
    assert_eq!(
        descriptor.micro_normals[1].texture_path.as_deref(),
        Some("textures/water.KTX2")
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("models/water.glb".into());
    press(&mut app, WaterPresetAction::ApplySelectedAsset(0));
    assert_eq!(
        app.world().resource::<WaterPresetEditorState>().tabs[0]
            .draft
            .descriptor
            .micro_normals[0],
        original.micro_normals[0]
    );
    press(&mut app, WaterPresetAction::ClearTexture(1));
    let descriptor = &app.world().resource::<WaterPresetEditorState>().tabs[0]
        .draft
        .descriptor;
    assert!(descriptor.micro_normals[1].texture_path.is_none());
    assert_eq!(descriptor.micro_normals[0], original.micro_normals[0]);
    assert!(!app.world().resource::<WaterPresetEditorState>().busy());
}

#[test]
fn header_diagnostics_primary_style_and_busy_badge_remain_in_sync() {
    let (mut app, _) = page(760.0, 1.0, 1.0, state(WaterSection::Appearance, true));
    app.add_systems(
        Update,
        (
            presentation::sync_panel,
            presentation::sync_document_badge,
            presentation::sync_button_styles,
        )
            .chain(),
    );
    app.world_mut()
        .spawn((WaterPresetStatusText, Text::new("原左栏诊断")));
    app.world_mut()
        .resource_mut::<WaterPresetEditorState>()
        .status = "微法线资源加载失败".into();
    app.update();
    let world = app.world_mut();
    let statuses: Vec<_> = world
        .query_filtered::<&Text, With<WaterPresetStatusText>>()
        .iter(world)
        .map(|text| text.0.clone())
        .collect();
    assert_eq!(statuses, vec!["微法线资源加载失败"; 2]);
    let save = world
        .query::<(Entity, &WaterPresetButton)>()
        .iter(world)
        .find(|(_, button)| button.0 == WaterPresetAction::Save)
        .unwrap()
        .0;
    for (interaction, expected) in [
        (UiInteraction::Hovered, crate::ui::theme::SELECT_BG_HOVER),
        (UiInteraction::None, crate::ui::theme::SELECT_BG),
    ] {
        app.world_mut().entity_mut(save).insert(interaction);
        app.update();
        assert_eq!(
            app.world().get::<BackgroundColor>(save).unwrap().0,
            expected
        );
    }
    app.world_mut()
        .resource_mut::<WaterPresetEditorState>()
        .adoption = Some(WaterCatalogAdoption {
        baseline_revision: 1,
        preset_id: "water-fixture".into(),
        deleted: false,
        tab_id: Some(1),
        message: "等待采用".into(),
    });
    app.update();
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Text, With<WaterDocumentBadge>>()
            .single(world)
            .unwrap()
            .as_str(),
        "保存 / 热采用中"
    );
    world.resource_mut::<WaterPresetEditorState>().adoption = None;
    app.update();
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<&Text, With<WaterDocumentBadge>>()
            .single(world)
            .unwrap()
            .as_str(),
        "已保存"
    );
}

#[test]
fn close_retains_busy_gate_and_dirty_discard_confirmation() {
    let fresh = state(WaterSection::Appearance, false);
    assert_eq!(
        workspace::draft_badge(&fresh.tabs[0].draft, false),
        "新建 · 未保存"
    );
    let mut state = state(WaterSection::Material, true);
    assert_eq!(
        workspace::draft_badge(&state.tabs[0].draft, false),
        "已保存"
    );
    state.tabs[0].draft.descriptor.reflectance = 0.7;
    assert_eq!(
        workspace::draft_badge(&state.tabs[0].draft, false),
        "未保存"
    );
    state.adoption = Some(WaterCatalogAdoption {
        baseline_revision: 1,
        preset_id: "water-fixture".into(),
        deleted: false,
        tab_id: Some(1),
        message: "等待采用".into(),
    });
    let mut app = interaction_app(state);
    press(&mut app, WaterPresetAction::Cancel);
    assert!(!app.world().resource::<WaterPresetEditorState>().tabs[0].close_confirmation);
    app.world_mut()
        .resource_mut::<WaterPresetEditorState>()
        .adoption = None;
    press(&mut app, WaterPresetAction::Cancel);
    assert!(app.world().resource::<WaterPresetEditorState>().tabs[0].close_confirmation);
    press(&mut app, WaterPresetAction::Cancel);
    assert!(
        app.world()
            .resource::<WaterPresetEditorState>()
            .tabs
            .is_empty()
    );
}
