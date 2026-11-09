//! 植被工作台的真实中文字体布局、当前LOD/部件预算与输入保护回归；不写作者文件或启动GPU。

use feathrise_editor_ui::UiInteraction;
use super::super::{components::*, interaction, presentation, types::*};
use super::spawn_editor;
use crate::resources::MapEditorState;
use crate::ui::text_input::EditorTextInputGuard;
use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::ecs::system::SystemState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_map_content::{BladePresetMotion, VegetationPresetCatalogAsset, VegetationPresetRuntime};

const BLADE_SECTIONS: [PresetEditorSection; 5] = [
    PresetEditorSection::Basics,
    PresetEditorSection::BladeLod,
    PresetEditorSection::BladeAppearance,
    PresetEditorSection::BladeView,
    PresetEditorSection::BladeMotion,
];
const MESH_SECTIONS: [PresetEditorSection; 4] = [
    PresetEditorSection::Basics,
    PresetEditorSection::MeshLod,
    PresetEditorSection::MeshPart,
    PresetEditorSection::MeshCollision,
];

#[path = "../../../../../feathrise_editor_ui/tests/support/render.rs"]
mod gpu;

#[test]
#[ignore = "显式渲染草叶与Mesh植被的真实预设分区，不加载模型或写作者文件"]
fn vegetation_workbench_gpu_snapshot() {
    let (mut app, root, font) = gpu::app(960, 800);
    for (blade, sections) in [(true, BLADE_SECTIONS.as_slice()), (false, MESH_SECTIONS.as_slice())] {
        for section in sections {
            gpu::clear(&mut app, root);
            let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/vegetation_presets.vegetation_presets.json"))).unwrap();
            let descriptor = catalog.presets.into_iter().find(|preset| matches!(preset.runtime, VegetationPresetRuntime::Blade { .. }) == blade).unwrap();
            let mut draft = PresetDraft::new(PresetDraftMode::Edit { original_id: descriptor.id.clone() }, descriptor);
            draft.section = *section;
            let mut state = VegetationPresetEditorState::default();
            state.open_draft(draft);
            let mut system = SystemState::<Commands>::new(app.world_mut());
            let mut commands = system.get_mut(app.world_mut()).unwrap();
            spawn_editor(&mut commands, root, &font, state.active_draft().unwrap(), &state, &Assets::default(), &Assets::default());
            system.apply(app.world_mut());
            gpu::snapshot(&mut app, &format!("vegetation-{}-{section:?}", if blade { "blade" } else { "mesh" }).to_lowercase());
        }
    }
}

fn state(blade: bool, section: PresetEditorSection, saved: bool) -> VegetationPresetEditorState {
    let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/vegetation_presets.vegetation_presets.json"
    )))
    .unwrap();
    let mut descriptor = catalog
        .presets
        .into_iter()
        .find(|preset| matches!(preset.runtime, VegetationPresetRuntime::Blade { .. }) == blade)
        .unwrap();
    descriptor.label = "窄窗口中文植被预设名称".repeat(4);
    descriptor.notes = Some("可选作者备注\n".repeat(12));
    match &mut descriptor.runtime {
        VegetationPresetRuntime::Blade {
            lods,
            base_color_texture_path,
            motion,
            ..
        } => {
            for lod in lods {
                lod.mesh_path = "gltf/中文长目录/模型/草木.glb#Mesh0/Primitive0".repeat(5);
                lod.name = "当前细节档的中文名称".repeat(3);
            }
            *base_color_texture_path = Some("textures/中文长目录/综合色.ktx2".repeat(6));
            *motion = BladePresetMotion::WaterFlow {
                direction: [1.0, 0.0],
                speed: 1.0,
                tip_strength: 0.1,
                spatial_scale: 1.0,
            };
        }
        VegetationPresetRuntime::Mesh { lods, .. } => {
            for lod in lods {
                for part in &mut lod.parts {
                    part.mesh_path = "gltf/中文长目录/模型/树木.glb#Mesh0/Primitive0".repeat(5);
                    part.material_path = "gltf/中文长目录/模型/树木.glb#Material0/std".repeat(5);
                }
            }
        }
    }
    let mode = if saved {
        PresetDraftMode::Edit {
            original_id: descriptor.id.clone(),
        }
    } else {
        PresetDraftMode::Create
    };
    let mut draft = PresetDraft::new(mode, descriptor);
    draft.section = section;
    let mut state = VegetationPresetEditorState::default();
    state.open_draft(draft);
    state.status.clear();
    state
}

fn page(width: f32, scale: f32, dpi: f32, state: VegetationPresetEditorState) -> (App, Entity) {
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
    let mut command_state = SystemState::<Commands>::new(app.world_mut());
    {
        let mut commands = command_state.get_mut(app.world_mut()).unwrap();
        spawn_editor(
            &mut commands,
            host,
            &font,
            state.active_draft().unwrap(),
            &state,
            &Assets::default(),
            &Assets::default(),
        );
    }
    command_state.apply(app.world_mut());
    app.insert_resource(state).insert_resource(MapEditorState {
        open: true,
        ..default()
    });
    app.add_systems(
        Update,
        (
            presentation::sync_panel,
            presentation::sync_document_badge,
            presentation::sync_button_styles,
        )
            .chain(),
    );
    for _ in 0..4 {
        app.update();
    }
    (app, host)
}

#[test]
fn shared_map_and_file_tabs_stay_at_workspace_top_across_active_contexts() {
    use crate::components::{MapEditorPresetTabScroll, MapEditorViewport};
    use feathrise_editor_documents_model::components::{DocumentAction, DocumentCenterHost};
    for scale in [1.0, 1.5] {
        let (mut app, old_host) = page(760.0, scale, 1.0, state(true, PresetEditorSection::Basics, true));
        let font = app.world_mut().query::<&TextFont>().iter(app.world()).next().unwrap().font.clone();
        app.world_mut().entity_mut(old_host).despawn();
        app.world_mut().resource_mut::<VegetationPresetEditorState>().tabs.clear();
        app.add_plugins(bevy::scene::ScenePlugin); // 原表单夹具只走Commands；正式BSN壳还需ScenePatch资产资源。
        let root = app.world_mut().spawn_scene(crate::ui::scene::viewport_panel(&feathrise_editor_documents::scenes())).unwrap().id();
        {
            let mut node = app.world_mut().get_mut::<Node>(root).unwrap();
            node.width = px(400); node.min_width = px(0); node.height = px(120);
        }
        super::install_workspace_tab_fixture(&mut app);
        let mut tab_host = None;
        for document_active in [false, true, false] {
            super::set_workspace_tab_fixture_context(app.world_mut(), document_active);
            for _ in 0..5 {
                for mut text in app.world_mut().query::<&mut TextFont>().iter_mut(app.world_mut()) { text.font = font.clone(); }
                app.update();
            }
            let world = app.world_mut();
            let (bar_entity, bar, bar_transform) = world.query_filtered::<(Entity, &ComputedNode, &UiGlobalTransform), With<PresetWorkspaceTabBar>>().single(world).unwrap();
            let bar_top = bar_transform.translation.y - bar.size().y * 0.5;
            let bar_bottom = bar_transform.translation.y + bar.size().y * 0.5;
            let root_top = world.get::<UiGlobalTransform>(root).unwrap().translation.y - world.get::<ComputedNode>(root).unwrap().size().y * 0.5;
            assert!((bar_top - root_top).abs() < 1.1, "标签必须位于中央工作区顶部");
            assert!((bar.size().y - 28.0 * scale).abs() < 1.1, "地图/文件切换不能压缩28px公共行");
            assert_eq!(world.get::<ChildOf>(bar_entity).unwrap().parent(), root);
            let host = world.query_filtered::<Entity, With<MapEditorPresetTabScroll>>().single(world).unwrap();
            assert_eq!(*tab_host.get_or_insert(host), host, "切换正文不得替换横滚容器");
            let labels = world.query::<&Text>().iter(world).map(|text| text.0.clone()).collect::<Vec<_>>();
            for label in ["测试地图 a", "测试地图 b*", "文档 · 工作台配置.json *", "文档 · 编辑说明.md"] {
                assert!(labels.iter().any(|text| text == label), "公共栏缺少真实标签：{label}");
            }
            assert_eq!(world.query::<&DocumentAction>().iter(world).filter(|action| matches!(action, DocumentAction::ActivateTab(_) | DocumentAction::CloseTab(_))).count(), 4);
            if document_active {
                let (node, computed, transform) = world.query_filtered::<(&Node, &ComputedNode, &UiGlobalTransform), With<DocumentCenterHost>>().single(world).unwrap();
                assert_eq!(node.display, Display::Flex);
                assert!(transform.translation.y - computed.size().y * 0.5 >= bar_bottom - 1.1);
            } else {
                let (computed, transform) = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<MapEditorViewport>>().single(world).unwrap();
                assert!(transform.translation.y - computed.size().y * 0.5 >= bar_bottom - 1.1);
            }
            assert!(world.resource::<crate::ui::asset_preview::DocumentState>().has_unsaved_draft("config/工作台配置.json"));
        }
    }
}

#[test]
fn nine_sections_fit_real_chinese_font_width_scale_and_dpi_without_author_changes() {
    for saved in [false, true] {
        for (blade, sections) in [
            (true, BLADE_SECTIONS.as_slice()),
            (false, MESH_SECTIONS.as_slice()),
        ] {
            for section in sections {
                for (width, scale, dpi) in [360.0, 760.0, 1200.0]
                    .into_iter()
                    .flat_map(|width| [0.6, 1.0, 1.5].map(|scale| (width, scale, 1.0)))
                    .chain([(1100.0, 1.25, 2.0)])
                {
                    let state = state(blade, *section, saved);
                    let before =
                        serde_json::to_string(&state.active_draft().unwrap().descriptor).unwrap();
                    let (mut app, host) = page(width, scale, dpi, state);
                    let world = app.world_mut();
                    let (form, node, computed) = world
                        .query_filtered::<(Entity, &Node, &ComputedNode), With<PresetEditorForm>>()
                        .single(world)
                        .unwrap();
                    assert_eq!(node.max_width, Val::Percent(100.0));
                    assert!(
                        computed.size().x > 0.0
                            && computed.size().x <= width + 1.01
                    );
                    let header = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<PresetDocumentHeader>>().single(world).unwrap();
                    let header_bottom = header.1.translation.y + header.0.size().y * 0.5;
                    let navigation = world.query_filtered::<(&ComputedNode, &UiGlobalTransform), With<PresetEditorNavigation>>().single(world).unwrap();
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
                    let mut counts = (0, 0, 0);
                    let mut fields = Vec::new();
                    for (button, kind, computed, transform) in world
                        .query::<(
                            &PresetEditorButton,
                            Option<&PresetButtonKind>,
                            &ComputedNode,
                            &UiGlobalTransform,
                        )>()
                        .iter(world)
                    {
                        assert!(
                            computed.size().x > 0.0 && computed.size().x <= host_width + 1.01,
                            "{blade} {section:?} width={width} scale={scale} action={:?}",
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
                            PresetEditorAction::SelectSection(_) => {
                                counts.0 += 1;
                                assert_eq!(kind, Some(&PresetButtonKind::Navigation));
                            }
                            PresetEditorAction::Save => {
                                counts.1 += 1;
                                assert_eq!(kind, Some(&PresetButtonKind::Primary));
                            }
                            PresetEditorAction::Cancel => counts.2 += 1,
                            PresetEditorAction::Focus(field) => {
                                assert!(!fields.contains(&field), "重复创建字段{field:?}");
                                fields.push(field);
                                assert!(computed.size().y >= 24.0 * scale * dpi - 1.01);
                            }
                            _ => {}
                        }
                    }
                    assert_eq!(counts, (if blade { 5 } else { 4 }, 1, 1));
                    assert!(!fields.is_empty());
                    assert_eq!(
                        world
                            .query::<&PresetReferenceOwner>()
                            .single(world)
                            .unwrap()
                            .0,
                        1
                    );
                    let state = world.resource::<VegetationPresetEditorState>();
                    assert_eq!(
                        before,
                        serde_json::to_string(&state.active_draft().unwrap().descriptor).unwrap()
                    );
                    assert!(!state.active_draft().unwrap().dirty());
                }
            }
        }
    }
}

#[test]
fn single_line_inputs_center_their_line_instead_of_stretching_to_control_height() {
    for section in [PresetEditorSection::Basics, PresetEditorSection::BladeLod] {
        for empty in [false, true] {
            for (scale, dpi) in [(0.6, 1.0), (1.0, 1.0), (1.5, 1.0), (1.25, 2.0)] {
                let mut state = state(true, section, true);
                let draft = state.active_draft_mut().unwrap();
                draft.descriptor.label = if empty { "" } else { "普通草" }.to_owned();
                if empty {
                    let VegetationPresetRuntime::Blade { lods, .. } = &mut draft.descriptor.runtime
                    else {
                        unreachable!()
                    };
                    lods[0].name.clear();
                    lods[0].mesh_path.clear();
                }
                let (mut app, _) = page(760.0, scale, dpi, state);
                let world = app.world_mut();
                let mut editable_count = 0;
                for (field, input, transform, parent, editable) in world
                    .query::<(
                        &PresetEditorFieldText,
                        &ComputedNode,
                        &UiGlobalTransform,
                        &ChildOf,
                        Option<&EditableText>,
                    )>()
                    .iter(world)
                    .filter(|(field, ..)| field.0 != PresetField::Notes)
                {
                    let control = world.get::<ComputedNode>(parent.parent()).unwrap();
                    let control_transform =
                        world.get::<UiGlobalTransform>(parent.parent()).unwrap();
                    assert!(
                        (transform.translation.y - control_transform.translation.y).abs() <= 1.01,
                        "{:?}未在输入框内垂直居中：scale={scale} dpi={dpi}",
                        field.0
                    );
                    if let Some(editable) = editable {
                        editable_count += 1;
                        let line_height = feathrise_editor_ui::metrics::FONT_CONTROL * 1.2 * scale * dpi;
                        assert!(
                            (input.size().y - line_height).abs() <= 1.01,
                            "{:?}应只占一行，而不是撑满输入框：text={} control={} scale={scale} dpi={dpi}",
                            field.0,
                            input.size().y,
                            control.size().y
                        );
                        assert!(input.size().y < control.size().y);
                        assert_eq!(editable.visible_lines, Some(1.0));
                        assert!(!editable.allow_newlines);
                    }
                }
                assert_eq!(
                    editable_count,
                    if section == PresetEditorSection::Basics {
                        1
                    } else {
                        2
                    }
                );
            }
        }
    }
}

#[test]
fn lods_and_mesh_parts_render_only_current_author_fields() {
    for count in 1..=8 {
        for selected in [0, count - 1] {
            let mut state = state(true, PresetEditorSection::BladeLod, true);
            let draft = state.active_draft_mut().unwrap();
            let VegetationPresetRuntime::Blade { lods, .. } = &mut draft.descriptor.runtime else {
                unreachable!()
            };
            lods.resize(count, lods[0].clone());
            draft.selected_lod = selected;
            let (mut app, _) = page(760.0, 1.0, 1.0, state);
            let world = app.world_mut();
            let fields: Vec<_> = world
                .query::<&PresetEditorFieldButton>()
                .iter(world)
                .map(|marker| marker.0)
                .collect();
            assert_eq!(fields.len(), 10);
            assert!(fields.iter().all(
                |field| matches!(field, PresetField::BladeLod { lod, .. } if *lod == selected)
            ));
            assert_eq!(
                world
                    .query::<&PresetEditorButton>()
                    .iter(world)
                    .filter(|button| matches!(button.0, PresetEditorAction::SelectLod(_)))
                    .count(),
                count
            );
        }
    }
    for selected in 0..3 {
        let mut state = state(false, PresetEditorSection::MeshLod, true);
        let draft = state.active_draft_mut().unwrap();
        let VegetationPresetRuntime::Mesh { lods, .. } = &mut draft.descriptor.runtime else {
            unreachable!()
        };
        lods.resize(3, lods[0].clone());
        for (lod, level) in lods.iter_mut().zip([2, 7, u8::MAX]) {
            lod.level = level;
        }
        draft.selected_lod = selected;
        let before = serde_json::to_string(&draft.descriptor).unwrap();
        let (mut app, _) = page(760.0, 1.0, 1.0, state);
        let world = app.world_mut();
        let fields: Vec<_> = world
            .query::<&PresetEditorFieldButton>()
            .iter(world)
            .map(|marker| marker.0)
            .collect();
        assert_eq!(fields.len(), 4);
        for field in [
            MeshLodField::StartBegin,
            MeshLodField::StartFull,
            MeshLodField::EndBegin,
            MeshLodField::EndHidden,
        ] {
            assert!(fields.contains(&PresetField::MeshLod {
                lod: selected,
                field
            }));
        }
        assert_eq!(
            before,
            serde_json::to_string(
                &world
                    .resource::<VegetationPresetEditorState>()
                    .active_draft()
                    .unwrap()
                    .descriptor
            )
            .unwrap()
        );
    }
    let mut state = state(false, PresetEditorSection::MeshPart, true);
    let draft = state.active_draft_mut().unwrap();
    let VegetationPresetRuntime::Mesh { lods, .. } = &mut draft.descriptor.runtime else {
        unreachable!()
    };
    lods.resize(3, lods[0].clone());
    for lod in lods {
        lod.parts.resize(11, lod.parts[0].clone());
    }
    draft.selected_lod = 2;
    draft.selected_part = 10;
    let (mut app, _) = page(360.0, 1.5, 1.0, state);
    let world = app.world_mut();
    let fields: Vec<_> = world
        .query::<&PresetEditorFieldButton>()
        .iter(world)
        .map(|marker| marker.0)
        .collect();
    assert!(fields.iter().all(|field| matches!(
        field,
        PresetField::MeshPart {
            lod: 2,
            part: 10,
            ..
        }
    )));
    assert_eq!(
        world
            .query::<&PresetEditorButton>()
            .iter(world)
            .filter(|button| matches!(button.0, PresetEditorAction::SelectLod(_)))
            .count(),
        3
    );
    assert_eq!(
        world
            .query::<&PresetEditorButton>()
            .iter(world)
            .filter(|button| matches!(button.0, PresetEditorAction::SelectPart(_)))
            .count(),
        11
    );
}

#[test]
fn status_and_full_paths_update_without_replacing_official_inputs() {
    let (mut app, _) = page(
        360.0,
        1.5,
        1.0,
        state(true, PresetEditorSection::BladeLod, true),
    );
    let sidebar = app
        .world_mut()
        .spawn((PresetEditorStatusText, Text::default()))
        .id();
    let field = PresetField::BladeLod {
        lod: 0,
        field: BladeLodField::MeshPath,
    };
    let inputs: Vec<_> = app
        .world_mut()
        .query_filtered::<(Entity, &EditableText), With<PresetEditorFieldText>>()
        .iter(app.world())
        .map(|(entity, input)| (entity, input.value().to_string()))
        .collect();
    {
        let mut state = app
            .world_mut()
            .resource_mut::<VegetationPresetEditorState>();
        state.active_field = Some(field);
        state.input_buffer = "gltf/当前尚未提交的中文长目录/树木.glb#Mesh0/Primitive0".repeat(6);
        state.status = "引用路径校验失败；请修正当前输入".into();
    }
    app.update();
    let world = app.world_mut();
    let state = world.resource::<VegetationPresetEditorState>();
    let expected_path = state.input_buffer.clone();
    assert_eq!(world.get::<Text>(sidebar).unwrap().as_str(), state.status);
    for text in world
        .query_filtered::<&Text, With<PresetEditorStatusText>>()
        .iter(world)
    {
        assert_eq!(text.as_str(), "引用路径校验失败；请修正当前输入");
    }
    assert_eq!(
        world
            .query_filtered::<&Text, With<PresetDocumentBadge>>()
            .single(world)
            .unwrap()
            .as_str(),
        "字段输入中"
    );
    let (marker, text, layout) = world
        .query::<(&PresetPathValue, &Text, &TextLayout)>()
        .single(world)
        .unwrap();
    assert_eq!(marker.0, field);
    assert_eq!(text.as_str(), expected_path);
    assert_eq!(layout.linebreak, LineBreak::AnyCharacter);
    for (entity, value) in inputs {
        assert_eq!(
            world
                .get::<EditableText>(entity)
                .unwrap()
                .value()
                .to_string(),
            value
        );
    }
}

fn enable_input(app: &mut App) {
    app.init_resource::<InputFocus>()
        .init_resource::<EditorTextInputGuard>()
        .init_resource::<crate::authoring::EditorAuthoringSession>()
        .add_message::<KeyboardInput>()
        .add_systems(
            Update,
            (interaction::handle_text_input, interaction::handle_buttons)
                .chain()
                .before(presentation::sync_panel),
        );
}

#[test]
fn primary_save_and_top_navigation_keep_their_style_on_hover_and_idle_frames() {
    let (mut app, _) = page(
        760.0,
        1.0,
        1.0,
        state(false, PresetEditorSection::Basics, true),
    );
    let world = app.world_mut();
    let buttons: Vec<_> = world
        .query::<(Entity, &PresetEditorButton)>()
        .iter(world)
        .map(|(entity, button)| (entity, button.0))
        .collect();
    let save = buttons
        .iter()
        .find(|(_, action)| *action == PresetEditorAction::Save)
        .unwrap()
        .0;
    let selected = buttons
        .iter()
        .find(|(_, action)| {
            *action == PresetEditorAction::SelectSection(PresetEditorSection::Basics)
        })
        .unwrap()
        .0;
    let other = buttons
        .iter()
        .find(|(_, action)| {
            *action == PresetEditorAction::SelectSection(PresetEditorSection::MeshLod)
        })
        .unwrap()
        .0;
    let colors = crate::ui::theme::SELECT_BG;
    assert_eq!(world.get::<BackgroundColor>(save).unwrap().0, colors);
    assert_eq!(world.get::<BackgroundColor>(selected).unwrap().0, colors);
    assert_eq!(world.get::<BackgroundColor>(other).unwrap().0, Color::NONE);
    assert_eq!(
        world.get::<Node>(selected).unwrap().border,
        UiRect::bottom(Val::Px(2.0))
    );
    world.entity_mut(save).insert(UiInteraction::Hovered);
    app.update();
    assert_eq!(
        app.world().get::<BackgroundColor>(save).unwrap().0,
        crate::ui::theme::SELECT_BG_HOVER
    );
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        app.world().get::<BackgroundColor>(save).unwrap().0,
        crate::ui::theme::SELECT_BG_HOVER
    );
    for (entity, _) in buttons {
        assert!(app.world().get_entity(entity).is_ok());
    }
}

fn press(app: &mut App, action: PresetEditorAction) {
    let entity = app
        .world_mut()
        .spawn((PresetEditorButton(action), feathrise_editor_ui::EditorButtonClick::triggered(), UiInteraction::Pressed))
        .id();
    app.update();
    app.world_mut().despawn(entity);
}

#[test]
fn navigation_keeps_invalid_buffer_and_ime_then_commits_latest_official_name() {
    let (mut app, _) = page(
        760.0,
        1.0,
        1.0,
        state(true, PresetEditorSection::Basics, true),
    );
    enable_input(&mut app);
    {
        let mut state = app
            .world_mut()
            .resource_mut::<VegetationPresetEditorState>();
        state.active_field = Some(PresetField::ResolutionX);
        state.input_buffer = "-".into();
    }
    press(
        &mut app,
        PresetEditorAction::SelectSection(PresetEditorSection::BladeLod),
    );
    let state = app.world().resource::<VegetationPresetEditorState>();
    assert_eq!(
        state.active_draft().unwrap().section,
        PresetEditorSection::Basics
    );
    assert_eq!(state.input_buffer, "-");
    let world = app.world_mut();
    let official = world
        .query::<(Entity, &PresetEditorFieldText)>()
        .iter(world)
        .find(|(_, marker)| marker.0 == PresetField::Label)
        .unwrap()
        .0;
    world
        .entity_mut(official)
        .insert(EditableText::new("同帧最新中文名称"));
    world
        .resource_mut::<InputFocus>()
        .set(official, FocusCause::Pressed);
    {
        let mut state = world.resource_mut::<VegetationPresetEditorState>();
        state.active_field = Some(PresetField::Label);
        state.input_buffer = "旧业务镜像".into();
    }
    world.resource_mut::<EditorTextInputGuard>().block_shortcuts = true;
    press(
        &mut app,
        PresetEditorAction::SelectSection(PresetEditorSection::BladeLod),
    );
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_draft()
            .unwrap()
            .section,
        PresetEditorSection::Basics
    );
    app.world_mut()
        .resource_mut::<EditorTextInputGuard>()
        .block_shortcuts = false;
    press(
        &mut app,
        PresetEditorAction::SelectSection(PresetEditorSection::BladeLod),
    );
    let state = app.world().resource::<VegetationPresetEditorState>();
    assert_eq!(
        state.active_draft().unwrap().section,
        PresetEditorSection::BladeLod
    );
    assert_eq!(
        state.active_draft().unwrap().descriptor.label,
        "同帧最新中文名称"
    );
    assert!(state.active_field.is_none());
}

#[test]
fn header_distinguishes_new_saved_dirty_and_busy_and_preserves_close_confirmation() {
    let (mut app, _) = page(
        760.0,
        1.0,
        1.0,
        state(false, PresetEditorSection::Basics, false),
    );
    enable_input(&mut app);
    let badge = app
        .world_mut()
        .query_filtered::<Entity, With<PresetDocumentBadge>>()
        .single(app.world())
        .unwrap();
    assert_eq!(
        app.world().get::<Text>(badge).unwrap().as_str(),
        "新建 · 未保存"
    );
    {
        let mut state = app
            .world_mut()
            .resource_mut::<VegetationPresetEditorState>();
        let draft = state.active_draft_mut().unwrap();
        draft.mode = PresetDraftMode::Edit {
            original_id: draft.descriptor.id.clone(),
        };
        state.touch_ui();
    }
    app.update();
    assert_eq!(app.world().get::<Text>(badge).unwrap().as_str(), "已保存");
    {
        let mut state = app
            .world_mut()
            .resource_mut::<VegetationPresetEditorState>();
        state.active_draft_mut().unwrap().descriptor.spacing_meters += 0.1;
        state.touch_ui();
    }
    app.update();
    assert_eq!(
        app.world().get::<Text>(badge).unwrap().as_str(),
        "未保存修改"
    );
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .adoption = Some(CatalogAdoption {
        baseline_revision: 1,
        preset_id: "fixture".into(),
        deleted: false,
        tab_id: Some(1),
        message: String::new(),
    });
    press(&mut app, PresetEditorAction::Cancel);
    assert_eq!(
        app.world().get::<Text>(badge).unwrap().as_str(),
        "等待热采用…"
    );
    assert_eq!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .tabs
            .len(),
        1
    );
    app.world_mut()
        .resource_mut::<VegetationPresetEditorState>()
        .adoption = None;
    press(&mut app, PresetEditorAction::Cancel);
    assert!(app.world().resource::<VegetationPresetEditorState>().tabs[0].close_confirmation);
    press(&mut app, PresetEditorAction::Cancel);
    assert!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .tabs
            .is_empty()
    );
}
