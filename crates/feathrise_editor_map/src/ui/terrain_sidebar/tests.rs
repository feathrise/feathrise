//! 真实BSN、原工具/地表输入与中文字体验证；不启动GPU，也不保存或修改正式作者文件。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::*, scene::terrain_tools_panel};
use crate::MapEditorSystemSet;
use crate::authoring::{
    DraftLoadTask, EditorAuthoringSession, PuddleAuthoringState, SurfaceBrushStroke,
    SurfaceLayerEditTask, TerrainExternalChangeState, TerrainPreviewState, WaterBrushStroke,
    WaterNetworkEditState, test_heightfield_draft,
};
use crate::components::{
    MapEditorActionButton, MapEditorRightPanelScroll, MapEditorPane, MapEditorSurfaceText,
};
use crate::resources::{
    EditorAssetCatalog, MapBrowserDragState, MapBrowserState, MapCreationTask, MapDeletionTask,
    MapEditorNumericScrubMouseGuard, MapEditorState, MapFolderPersistence,
};
use crate::types::{EditorAction, EditorTool, EditorView};
use crate::ui::courtyard::CourtyardUiState;
use crate::ui::sidebars::{EditorLeftSidebarTab, MapEditorSidebarState};
use crate::ui::text_input::EditorTextInputGuard;
use crate::ui::theme as colors;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use crate::viewport::modes::{ViewportMode, ViewportModeState};
use bevy::app::{AppExit, HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use feathrise_editor_ui::NumericInputRoot;
use feathrise_map_content::{
    MAP_SURFACE_SCHEMA_VERSION, MapCatalogAsset, MapPuddleDescriptor, MapSurfaceAsset,
    ResolvedSurfacePresetCatalog, SurfacePresetCatalogAsset, SurfacePresetRegistry,
};
use std::sync::Arc;

fn page() -> (App, Entity) {
    page_with_shell(false)
}

fn page_with_shell(shell: bool) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins(feathrise_editor_surface::SurfaceEditorPlugin);
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
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<MapBrowserState>()
        .init_resource::<MapBrowserDragState>()
        .init_resource::<MapFolderPersistence>()
        .init_resource::<MapCreationTask>()
        .init_resource::<MapDeletionTask>()
        .init_resource::<EditorAssetCatalog>()
        .init_resource::<DraftLoadTask>()
        .init_resource::<SurfaceBrushStroke>()
        .init_resource::<SurfaceLayerEditTask>()
        .init_resource::<PuddleAuthoringState>()
        .init_resource::<TerrainPreviewState>()
        .init_resource::<TerrainExternalChangeState>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<ViewportModeState>()
        .init_resource::<CourtyardUiState>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInput>()
        .init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .add_message::<feathrise_editor_ui::NumericInputChanged>()
        .add_message::<feathrise_editor_ui::NumericInputCommitted>()
        .add_message::<feathrise_editor_ui::NumericInputSetValue>()
        .init_asset::<MapCatalogAsset>()
        .insert_resource(UiScale(1.0))
        .add_message::<KeyboardInput>()
        .add_message::<AppExit>();
    let mut draft = test_heightfield_draft(vec![0; 121].into());
    draft.surface = Some(MapSurfaceAsset {
        schema_version: MAP_SURFACE_SCHEMA_VERSION,
        map_id: draft.map_id.clone(),
        weight_source_path: "maps/EDGE-TEST/surface/weights.rgba8".into(),
        weight_texture_path: "maps/EDGE-TEST/surface/weights.ktx2".into(),
        index_source_path: "maps/EDGE-TEST/surface/indices.rgba32".into(),
        index_texture_path: "maps/EDGE-TEST/surface/indices.ktx2".into(),
        resolution: [2, 2],
        layers: vec![],
    });
    draft.surface_indices = Some(Arc::new(vec![0; 64]));
    draft.surface_weights = Some(Arc::new(vec![0; 16]));
    app.insert_resource(MapEditorState {
        open: true,
        selected_map_id: Some(draft.map_id.clone()),
        brush_radius: 7.0,
        brush_strength: 0.35,
        ..default()
    });
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    });
    let mut sidebar = MapEditorSidebarState::default();
    sidebar.right = crate::ui::sidebars::EditorRightSidebarTab::Tools;
    sidebar.left = EditorLeftSidebarTab::Terrain;
    app.insert_resource(sidebar);
    let catalog: SurfacePresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/surface_presets.surface_presets.json"
    )))
    .unwrap();
    app.insert_resource(SurfacePresetRegistry {
        catalog: default(),
        revision: 1,
        resolved: ResolvedSurfacePresetCatalog::build(&catalog).unwrap(),
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
    crate::ui::surface_preset_editor::install(&mut app);
    crate::ui::surface_layers::install(&mut app);
    super::install(&mut app);
    app.add_systems(
        Update,
        crate::ui::actions::handle_editor_action_buttons.in_set(MapEditorSystemSet::Input),
    );
    app.add_systems(
        Update,
        crate::ui::actions::sync_editor_action_button_styles
            .in_set(MapEditorSystemSet::Presentation),
    );
    app.add_systems(
        Update,
        crate::viewport::modes::sync_mode
            .after(MapEditorSystemSet::Input)
            .before(MapEditorSystemSet::Tasks),
    );
    app.add_systems(
        Update,
        crate::ui::layer_status::sync_editor_layer_status.in_set(MapEditorSystemSet::Presentation),
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
                .in_set(MapEditorSystemSet::Presentation),
        );
    } else {
        app.add_systems(Startup, terrain_tools_panel.spawn());
    }
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
    // 类型栏由视口浮层承载；侧栏回归仍使用同一个真实构造器与参数系统。
    app.world_mut()
        .commands()
        .spawn((
            crate::viewport::modes::ViewportControlsRoot,
            Node { width: px(360), height: px(120), ..default() },
        ))
        .with_children(|parent| super::spawn_volume_brush_types(parent, &font));
    app.update();
    let world = app.world_mut();
    let root = if shell {
        let root = world
            .query_filtered::<Entity, With<MapEditorPane>>()
            .single(world)
            .unwrap();
        world.get_mut::<Node>(root).unwrap().width = percent(100);
        root
    } else {
        world
            .query_filtered::<Entity, With<TerrainSidebarRoot>>()
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
    for mut text_font in world.query::<&mut TextFont>().iter_mut(world) {
        text_font.font = font.clone().into();
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
        .find(|(entity, children)| {
            world
                .get::<ComputedNode>(*entity)
                .is_some_and(|node| node.size().x > 0.0)
                && children.iter().any(|child| {
                    world
                        .get::<Text>(child)
                        .is_some_and(|text| text.as_str() == label)
                })
        })
        .unwrap_or_else(|| panic!("缺少可见按钮：{label}"))
        .0
}

fn press(app: &mut App, label: &str) -> Entity {
    let entity = button(app, label);
    app.world_mut()
        .entity_mut(entity)
        .insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    app.world_mut().entity_mut(entity).insert(UiInteraction::None);
    app.update();
    entity
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

#[test]
fn tabs_keep_original_entities_and_only_one_body_scroll_owner() {
    let (mut app, _) = page();
    let world = app.world_mut();
    let mut entities = world.query::<Entity>().iter(world).collect::<Vec<_>>();
    entities.sort_unstable(); // 点击组件可改变archetype顺序；原实体身份及数量仍须完全保留。
    let scroll = world
        .query_filtered::<Entity, With<TerrainSidebarScroll>>()
        .single(world)
        .unwrap();
    assert_eq!(
        world
            .query_filtered::<Entity, With<MapEditorRightPanelScroll>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<MapEditorSurfaceText>>()
            .iter(world)
            .count(),
        1
    );
    assert_eq!(
        world
            .query_filtered::<Entity, With<NumericInputRoot>>()
            .iter(world)
            .count(),
        1 // 三维规则刷的半高控件常驻；切页只禁用，不重建输入实体。
    );
    for (entity, _) in world.query::<(Entity, &TerrainSidebarTab)>().iter(world) {
        assert_ne!(world.get::<ChildOf>(entity).unwrap().parent(), scroll);
    }
    for label in ["地表", "积水", "雕刻", "地表"] {
        press(&mut app, label);
        let world = app.world_mut();
        let mut current = world.query::<Entity>().iter(world).collect::<Vec<_>>();
        current.sort_unstable();
        assert_eq!(
            current,
            entities
        );
        for (panel, node) in world.query::<(&TerrainSidebarPanel, &Node)>().iter(world) {
            let selected = panel.0 == world.resource::<TerrainSidebarState>().page;
            assert_eq!(
                node.display,
                if selected {
                    Display::Flex
                } else {
                    Display::None
                }
            );
        }
    }
}

#[test]
fn volume_tools_keep_select_mode_and_restore_the_chosen_tool_after_page_changes() {
    let (mut app, _) = page();
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    for (label, tool) in [
        ("三维挖掘", EditorTool::VolumeDig),
        ("三维填充", EditorTool::VolumeFill),
        ("三维平滑", EditorTool::VolumeSmooth),
        ("三维整平", EditorTool::VolumeFlatten),
    ] {
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
        app.update();
        let selected = press(&mut app, label);
        assert_eq!(
            app.world().resource::<MapEditorState>().tool,
            EditorTool::Inspect
        );
        assert_eq!(
            app.world().resource::<TerrainSidebarState>().sculpt_tool,
            tool
        );
        assert_eq!(app.world().get::<BackgroundColor>(selected).unwrap().0, colors::SELECT_BG);
        assert_eq!(*app.world().get::<BorderColor>(selected).unwrap(), BorderColor::all(colors::COPPER_BORDER_WEAK));
        for page in ["地表", "积水", "雕刻"] {
            press(&mut app, page);
        }
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
        app.update();
        assert_eq!(app.world().resource::<MapEditorState>().tool, tool);
        assert!(app.world().resource::<MapEditorState>().brush_radius <= 8.0);
        assert_eq!(
            app.world().resource::<EditorAuthoringSession>().revision,
            revision
        );
    }
    // 二维平滑仍是独立入口，不能因为三维工具记忆而误选。
    press(&mut app, "平滑");
    assert_eq!(
        app.world().resource::<MapEditorState>().tool,
        EditorTool::Smooth
    );
}

#[test]
fn stable_frames_do_not_rewrite_fixed_controls_or_feedback() {
    let (mut app, _) = page();
    press(&mut app, "平滑");
    let snapshot = |world: &mut World| {
        let controls = world.query_filtered::<(Entity, Ref<BackgroundColor>, Ref<BorderColor>), Or<(With<TerrainSidebarTab>, With<TerrainSidebarTool>)>>()
            .iter(world).map(|(entity, background, border)| (entity, background.last_changed(), border.last_changed())).collect::<Vec<_>>();
        let feedback = world
            .query_filtered::<(Entity, Ref<Text>, Ref<TextColor>), Or<(
                With<TerrainSidebarModeText>,
                With<TerrainSidebarHintText>,
                With<TerrainSidebarAvailabilityText>,
                With<TerrainSidebarMessageText>,
                With<TerrainSidebarPuddleTargetText>,
            )>>()
            .iter(world)
            .map(|(entity, text, color)| (entity, text.last_changed(), color.last_changed()))
            .collect::<Vec<_>>();
        (controls, feedback)
    };
    let before = snapshot(app.world_mut());
    for _ in 0..4 {
        app.update();
    }
    assert_eq!(snapshot(app.world_mut()), before);
}

#[test]
fn independent_mesh_context_and_sculpt_controls_follow_the_same_source_capabilities() {
    for kind in [feathrise_map_content::MapTerrainKind::Gltf, feathrise_map_content::MapTerrainKind::Heightfield,
        feathrise_map_content::MapTerrainKind::SparseClipmap, feathrise_map_content::MapTerrainKind::Voxel] {
        let (mut app, _) = page();
        let (map, id) = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
            object.display_name = "当前独立网格".into();
            object.terrain.kind = kind;
            let id = object.id.clone();
            draft.manifest.terrain = feathrise_map_content::MapTerrainDescriptor::empty();
            draft.manifest.terrains = Some(vec![object].into());
            (draft.map_id.clone(), id)
        };
        let mut selection = crate::ui::terrain_objects::TerrainObjectsEditorState::default();
        selection.map = Some(map);
        selection.selected = Some(id);
        selection.selected_index = Some(0);
        app.insert_resource(selection);
        app.update();
        assert!(text::<TerrainSidebarMeshContext>(&mut app).contains("当前独立网格"));
        let world = app.world_mut();
        for (tool, disabled) in world.query::<(&TerrainSidebarTool, Has<bevy::ui::InteractionDisabled>)>().iter(world) {
            let supports = if tool.0.is_volume_sculpt() { kind.supports_volume_sculpt() } else { kind.supports_height_sculpt() };
            assert_eq!(disabled, !supports, "{kind:?} · {:?}", tool.0);
        }
    }
}

#[test]
fn volume_shape_and_height_controls_share_session_parameters_and_disable_when_hidden() {
    let (mut app, _) = page();
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
    press(&mut app, "三维挖掘");
    let shape = press(&mut app, "方盒");
    let row = app.world().get::<ChildOf>(shape).unwrap().parent();
    let controls = app.world().get::<ChildOf>(row).unwrap().parent();
    assert!(app.world().get::<crate::viewport::modes::ViewportControlsRoot>(controls).is_some(), "笔刷类型必须属于视口浮层的鼠标占用范围");
    assert_eq!(app.world().resource::<MapEditorState>().volume_brush_shape, feathrise_map_terrain::volume::VolumeBrushShape::Box);
    assert_eq!(app.world().get::<BackgroundColor>(shape).unwrap().0, colors::SELECT_BG);
    assert_eq!(*app.world().get::<BorderColor>(shape).unwrap(), BorderColor::all(colors::COPPER_BORDER_WEAK));
    let root = app.world_mut().query_filtered::<Entity, With<VolumeBrushHeightField>>().single(app.world()).unwrap();
    assert!(app.world().get::<feathrise_editor_ui::NumericInputDisabled>(root).is_none());
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    app.world_mut().write_message(feathrise_editor_ui::NumericInputChanged { entity: root, value: feathrise_editor_ui::NumericValue::F32(0.7) });
    app.update();
    assert_eq!(app.world().resource::<MapEditorState>().volume_brush_half_height, 0.7);
    press(&mut app, "三维整平");
    assert_eq!(app.world().resource::<MapEditorState>().tool, EditorTool::VolumeFlatten);
    assert_eq!(app.world().resource::<MapEditorState>().volume_brush_shape, feathrise_map_terrain::volume::VolumeBrushShape::Box);
    let lock = app.world_mut().query_filtered::<Entity, With<VolumeBrushHeightLockButton>>().single(app.world()).unwrap();
    assert_eq!(app.world().get::<BackgroundColor>(lock).unwrap().0, colors::SELECT_BG);
    assert_eq!(*app.world().get::<BorderColor>(lock).unwrap(), BorderColor::all(colors::COPPER_BORDER_WEAK));
    press(&mut app, "球形");
    assert!(app.world().get::<feathrise_editor_ui::NumericInputDisabled>(root).is_some());
    app.world_mut().write_message(feathrise_editor_ui::NumericInputChanged { entity: root, value: feathrise_editor_ui::NumericValue::F32(8.) });
    app.update();
    assert_eq!(app.world().resource::<MapEditorState>().volume_brush_half_height, 0.7, "隐藏字段不能接收迟到输入");
    press(&mut app, "圆柱");
    assert!(app.world().get::<feathrise_editor_ui::NumericInputDisabled>(root).is_none());
    press(&mut app, "地表");
    assert!(app.world().get::<feathrise_editor_ui::NumericInputDisabled>(root).is_some());
    assert_eq!(app.world().get::<Node>(row).unwrap().display, Display::None, "普通地表工具不能误显示三维几何类型");
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, revision, "刷子设置不能制造地图撤销或脏标记");
}

#[test]
fn tool_pages_preserve_mode_sculpt_preference_and_author_data() {
    let (mut app, _) = page();
    let before = {
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        (
            session.revision,
            draft.surface_indices.clone().unwrap(),
            draft.surface_weights.clone().unwrap(),
        )
    };
    press(&mut app, "平滑");
    assert_eq!(
        app.world().resource::<MapEditorState>().tool,
        EditorTool::Inspect
    );
    assert_eq!(
        app.world().resource::<TerrainSidebarState>().sculpt_tool,
        EditorTool::Smooth
    );
    for label in ["地表", "积水", "雕刻"] {
        press(&mut app, label);
        assert_eq!(
            app.world().resource::<ViewportModeState>().mode,
            ViewportMode::Select
        );
        assert_eq!(
            app.world().resource::<MapEditorState>().tool,
            EditorTool::Inspect
        );
    }
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
    app.update();
    assert_eq!(
        app.world().resource::<MapEditorState>().tool,
        EditorTool::Smooth
    );
    for (label, tool) in [
        ("地表", EditorTool::Surface),
        ("积水", EditorTool::Puddles),
        ("雕刻", EditorTool::Smooth),
    ] {
        press(&mut app, label);
        assert_eq!(
            app.world().resource::<ViewportModeState>().mode,
            ViewportMode::Edit
        );
        assert_eq!(app.world().resource::<MapEditorState>().tool, tool);
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(session.revision, before.0);
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert!(Arc::ptr_eq(
        draft.surface_indices.as_ref().unwrap(),
        &before.1
    ));
    assert!(Arc::ptr_eq(
        draft.surface_weights.as_ref().unwrap(),
        &before.2
    ));
    assert!(draft.manifest.puddles.is_none() && draft.puddle_mask.is_none());
    let editor = app.world().resource::<MapEditorState>();
    assert_eq!((editor.brush_radius, editor.brush_strength), (7.0, 0.35));
}

#[test]
fn original_preset_and_explicit_map_layer_creation_remain_separate() {
    let (mut app, _) = page();
    press(&mut app, "地表");
    let preset = app.world().resource::<SurfacePresetRegistry>().presets()[0].clone();
    press(&mut app, &preset.label);
    assert!(text::<TerrainSidebarAvailabilityText>(&mut app).contains("显式新建"));
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .layers
            .is_empty()
    );
    press(&mut app, "新建层");
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .surface
            .as_ref()
            .unwrap()
            .layers
            .len(),
        1
    );
    assert_eq!(session.undo.len(), 1);
    assert!(text::<MapEditorSurfaceText>(&mut app).contains(&preset.label));
    press(&mut app, "新建预设");
    assert!(matches!(
        app.world()
            .resource::<VegetationPresetEditorState>()
            .active_tab,
        PresetWorkspaceTab::Surface(_)
    ));
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
    assert_eq!(
        app.world().resource::<SurfacePresetRegistry>().presets()[0].id,
        preset.id
    );
}

#[test]
fn blocked_tab_clicks_are_consumed_without_delayed_replay() {
    for blocker in 0..7 {
        let (mut app, _) = page();
        let tab = button(&mut app, "地表");
        match blocker {
            0 => app.world_mut().resource_mut::<MapEditorState>().open = false,
            1 => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .close_requested = true
            }
            2 => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .new_map_dialog_open = true
            }
            3 => {
                app.world_mut().resource_mut::<MapEditorSidebarState>().left =
                    EditorLeftSidebarTab::Water
            }
            4 => app
                .world_mut()
                .resource_mut::<MapEditorNumericScrubMouseGuard>()
                .advance(true, true, false),
            5 => {
                app.world_mut()
                    .resource_mut::<EditorTextInputGuard>()
                    .block_shortcuts = true
            }
            _ => {
                let map_id = app
                    .world()
                    .resource::<MapEditorState>()
                    .selected_map_id
                    .clone()
                    .unwrap();
                app.world_mut()
                    .resource_mut::<TerrainExternalChangeState>()
                    .register_save_conflict(map_id, [1; 32]);
            }
        }
        app.world_mut().entity_mut(tab).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.update();
        assert_eq!(
            app.world().resource::<TerrainSidebarState>().page,
            TerrainSidebarPage::Sculpt
        );
        app.world_mut().resource_mut::<MapEditorState>().open = true;
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .close_requested = false;
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .new_map_dialog_open = false;
        app.world_mut().resource_mut::<MapEditorSidebarState>().left =
            EditorLeftSidebarTab::Terrain;
        app.world_mut()
            .resource_mut::<MapEditorNumericScrubMouseGuard>()
            .advance(false, false, false);
        app.world_mut()
            .resource_mut::<EditorTextInputGuard>()
            .block_shortcuts = false;
        app.world_mut()
            .resource_mut::<TerrainExternalChangeState>()
            .conflict = None;
        app.update();
        assert_eq!(
            app.world().resource::<TerrainSidebarState>().page,
            TerrainSidebarPage::Sculpt
        );
        app.world_mut().entity_mut(tab).insert(UiInteraction::None);
        app.update();
        press(&mut app, "地表");
        assert_eq!(
            app.world().resource::<TerrainSidebarState>().page,
            TerrainSidebarPage::Surface
        );
    }
}

#[test]
fn actual_tool_requests_reveal_their_page_and_keep_sculpt_highlight() {
    let (mut app, _) = page();
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit;
    for (tool, page) in [
        (EditorTool::Puddles, TerrainSidebarPage::Puddles),
        (EditorTool::Surface, TerrainSidebarPage::Surface),
        (EditorTool::Flatten, TerrainSidebarPage::Sculpt),
    ] {
        app.world_mut().resource_mut::<MapEditorState>().tool = tool;
        app.update();
        assert_eq!(app.world().resource::<TerrainSidebarState>().page, page);
    }
    let flatten = press(&mut app, "压平");
    assert_eq!(
        app.world().get::<MapEditorActionButton>(flatten).unwrap().0,
        EditorAction::ToolFlatten
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(flatten).unwrap().0,
        colors::SELECT_BG
    );
    assert_eq!(
        *app.world().get::<BorderColor>(flatten).unwrap(),
        BorderColor::all(colors::COPPER_BORDER_WEAK)
    );
    assert!(text::<TerrainSidebarModeText>(&mut app).contains("编辑 · 压平"));
    app.world_mut().resource_mut::<MapEditorState>().view = EditorView::Front;
    app.update();
    assert!(text::<TerrainSidebarAvailabilityText>(&mut app).contains("不接受新笔划"));
}

#[test]
fn feedback_reports_real_wait_errors_and_puddle_canvas_without_creating_one() {
    let (mut app, _) = page();
    press(&mut app, "地表");
    app.world_mut()
        .resource_mut::<SurfacePresetRegistry>()
        .last_error = Some("坏目录：中文诊断".into());
    app.world_mut().resource_mut::<MapEditorState>().status = "上一笔查询失败，仍保留旧画面".into();
    app.update();
    assert!(text::<TerrainSidebarAvailabilityText>(&mut app).contains("坏目录：中文诊断"));
    assert!(text::<TerrainSidebarMessageText>(&mut app).contains("上一笔查询失败"));
    assert!(
        !app.world()
            .resource::<SurfacePresetRegistry>()
            .presets()
            .is_empty()
    );
    press(&mut app, "积水");
    assert!(text::<TerrainSidebarPuddleTargetText>(&mut app).contains("尚未创建"));
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.puddles = Some(MapPuddleDescriptor {
            source_path: "maps/EDGE-TEST/puddles.r8".into(),
            texture_path: "maps/EDGE-TEST/puddles.ktx2".into(),
            resolution: [2, 4],
        });
        draft.puddle_mask = Some(Arc::new(vec![0; 8]));
    }
    app.update();
    assert!(text::<TerrainSidebarPuddleTargetText>(&mut app).contains("2 × 4"));
    assert!(text::<TerrainSidebarAvailabilityText>(&mut app).contains("预览正在准备"));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    app.update();
    assert!(text::<TerrainSidebarAvailabilityText>(&mut app).contains("正在保存"));
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = None;
    app.update();
    assert!(text::<TerrainSidebarAvailabilityText>(&mut app).contains("等待当前地图"));
    assert!(text::<TerrainSidebarPuddleTargetText>(&mut app).contains("等待当前地图"));
}

#[test]
fn three_pages_fit_narrow_sidebar_scales_dpi_and_keep_tabs_out_of_scroll() {
    let (mut app, host) = page();
    for width in [200.0, 240.0, 320.0, 420.0] {
        for scale in [0.6, 1.0, 1.5] {
            for dpi in [1.0, 2.0] {
                app.world_mut().get_mut::<Node>(host).unwrap().width = px(width);
                app.world_mut().resource_mut::<UiScale>().0 = scale;
                let world = app.world_mut();
                for mut camera in world.query::<&mut Camera>().iter_mut(world) {
                    camera.computed.target_info.as_mut().unwrap().scale_factor = dpi;
                }
                app.update();
                for label in ["雕刻", "地表", "积水"] {
                    press(&mut app, label);
                    let world = app.world_mut();
                    let host_node = world.get::<ComputedNode>(host).unwrap();
                    let width = host_node.size().x;
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
                                >= center - width * 0.5 - 1.1,
                            "左侧溢出：{label}"
                        );
                        assert!(
                            transform.translation.x + node.size().x * 0.5
                                <= center + width * 0.5 + 1.1,
                            "右侧溢出：{label}"
                        );
                    }
                    for (children, node) in world
                        .query_filtered::<(&Children, &ComputedNode), Or<(With<TerrainSidebarTab>, With<TerrainSidebarTool>)>>()
                        .iter(world)
                    {
                        if node.size().x == 0.0 {
                            continue;
                        }
                        let text = children.iter().next().unwrap();
                        assert!(
                            world
                                .get::<bevy::text::TextLayoutInfo>(text)
                                .unwrap()
                                .size
                                .x
                                <= node.size().x + 1.1
                        );
                    }
                    let scroll = world
                        .query_filtered::<Entity, With<TerrainSidebarScroll>>()
                        .single(world)
                        .unwrap();
                    let before = world
                        .query::<(&TerrainSidebarTab, &UiGlobalTransform)>()
                        .iter(world)
                        .map(|(_, transform)| transform.translation.y)
                        .collect::<Vec<_>>();
                    world.get_mut::<ScrollPosition>(scroll).unwrap().y = 80.0;
                    app.update();
                    let world = app.world_mut();
                    assert_eq!(
                        world
                            .query::<(&TerrainSidebarTab, &UiGlobalTransform)>()
                            .iter(world)
                            .map(|(_, transform)| transform.translation.y)
                            .collect::<Vec<_>>(),
                        before
                    );
                }
            }
        }
    }
}

#[test]
fn full_sidebar_keeps_navigation_fixed_while_long_diagnostics_wrap_in_body() {
    let (mut app, host) = page_with_shell(true);
    app.world_mut().get_mut::<Node>(host).unwrap().width = px(200);
    app.world_mut().get_mut::<Node>(host).unwrap().height = px(350);
    app.update();
    press(&mut app, "地表");
    app.world_mut()
        .resource_mut::<SurfacePresetRegistry>()
        .last_error = Some("纹理目录加载失败，请核对中文路径与贴图格式。".repeat(12));
    for _ in 0..3 {
        app.update();
    }
    let world = app.world_mut();
    let scroll = world
        .query_filtered::<Entity, With<TerrainSidebarScroll>>()
        .single(world)
        .unwrap();
    let (diagnostic, layout) = world.query_filtered::<(&ComputedNode, &bevy::text::TextLayoutInfo), With<TerrainSidebarAvailabilityText>>().single(world).unwrap();
    assert!(layout.size.x <= diagnostic.size().x + 1.1);
    assert!(layout.size.y <= diagnostic.size().y + 1.1);
    assert!(
        world.get::<ComputedNode>(scroll).unwrap().content_size().y
            > world.get::<ComputedNode>(scroll).unwrap().size().y
    );
    let before = world
        .query::<(&TerrainSidebarTab, &UiGlobalTransform)>()
        .iter(world)
        .map(|(_, transform)| transform.translation.y)
        .collect::<Vec<_>>();
    world.get_mut::<ScrollPosition>(scroll).unwrap().y = 120.0;
    app.update();
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<(&TerrainSidebarTab, &UiGlobalTransform)>()
            .iter(world)
            .map(|(_, transform)| transform.translation.y)
            .collect::<Vec<_>>(),
        before
    );
    let host_width = world.get::<ComputedNode>(host).unwrap().size().x;
    let center = world.get::<UiGlobalTransform>(host).unwrap().translation.x;
    for (node, transform) in world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<InteractionButton>>()
        .iter(world)
    {
        if node.size().x == 0.0 {
            continue;
        }
        assert!(transform.translation.x - node.size().x * 0.5 >= center - host_width * 0.5 - 1.1);
        assert!(transform.translation.x + node.size().x * 0.5 <= center + host_width * 0.5 + 1.1);
    }
}
