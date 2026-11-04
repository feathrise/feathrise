//! 真实检查器、原结构索引和公共数值根的回归；只使用内存草稿，不写用户配置。

use bevy::app::{HierarchyPropagatePlugin, PropagateSet};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::prelude::*;
use bevy::ui::{
    ComputedUiRenderTargetInfo, ComputedUiTargetCamera, ui_layout_system, ui_surface::UiSurface,
    update::propagate_ui_target_cameras,
};
use bevy::world_serialization::WorldAsset;
use feathrise_editor_ui::*;
use feathrise_map_content::{MapId, MapPlacementHeightMode, MapPlacementRecord};

use super::{components::*, sync_scene_inspector, types::*};
use crate::authoring::EditorAuthoringSession;
use crate::components::MapEditorRightPanelScroll;
use crate::resources::{MapEditorState, TerrainShadingSession};
use crate::ui::gltf_browser::*;
use crate::ui::placement_transform::*;
use crate::ui::placement_water::*;

#[derive(Resource)]
struct TestFont(Handle<Font>);

fn placement(index: usize) -> MapPlacementRecord {
    MapPlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        id: format!("instance-{index}"),
        asset_path: "gltf/用于检查长中文来源路径的目录/场景中的测试资产.glb".into(),
        asset_target: default(),
        building_preset_id: None,
        courtyard: None,
        terrain_patch: None,
        translation: [1.0, 2.0, 3.0],
        height_mode: MapPlacementHeightMode::FixedHeight,
        terrain_offset: 0.0,
        yaw_degrees: 45.0,
        scale: [1.0; 3],
        water_surfaces: vec![],
    }
}

pub(crate) fn page(count: usize) -> (App, Entity) {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
        bevy::text::TextPlugin,
        bevy::transform::TransformPlugin,
        EditorUiPlugin,
    ));
    app.init_resource::<UiSurface>()
        .insert_resource(UiScale(1.0))
        .init_resource::<feathrise_editor_core::EditorSettings>()
        .init_resource::<ActiveNumericInput>()
        .init_resource::<ActiveNumericInputScrub>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<PlacementTransformInputState>()
        .init_resource::<TerrainShadingSession>()
        .insert_resource({
            let mut sidebar = crate::ui::sidebars::MapEditorSidebarState::default();
            sidebar.navigator = crate::ui::sidebars::EditorNavigatorTab::Scene;
            sidebar
        })
        .init_resource::<crate::viewport::content_selection::LayerCandidateState>()
        .init_resource::<crate::game_authoring::map_interaction::MapInteractionState>()
        .init_resource::<EditorGltfBrowserState>()
        .init_resource::<PlacementWaterSelectionCache>()
        .init_resource::<PlacementWaterRuntimeSelectionCache>()
        .init_resource::<PlacementInstanceIndex>()
        .init_resource::<feathrise_scene::MapPlacementRuntimeStatus>()
        .init_asset::<WorldAsset>()
        .add_message::<NumericInputSetValue>();
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    draft.manifest.display_name = "检查器测试地图".into();
    draft.placements.placements = (0..count).map(placement).collect();
    let map_id = draft.map_id.clone();
    app.insert_resource(MapEditorState {
        open: true,
        selected_map_id: Some(map_id),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    });
    app.add_systems(Startup, (crate::ui::sidebars::scene_page.spawn(), crate::ui::sidebars::scene_tree_page.spawn()));
    app.add_systems(
        Update,
        (
            spawn_gltf_browser_rows,
            setup_placement_transform_numeric_inputs,
            setup_placement_water_numeric_inputs,
            refresh_gltf_browser_cache,
            refresh_placement_water_selection_cache,
            refresh_placement_water_runtime_selection_cache,
            sync_placement_transform_panel,
            crate::ui::sidebars::sync_editor_sidebar_context,
            sync_map_base_gltf_row,
            sync_gltf_browser_rows,
            sync_placement_water_text,
            sync_scene_inspector,
            crate::ui::sidebars::sync_editor_sidebar_presentation,
        )
            .chain()
            .after(EditorUiSystemSet::NumericInput),
    ); // 使用正式唯一控件运行时消费权威赋值；不在测试中替造输入协议。
    app.add_plugins((
        HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
        HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
    ));
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
    app.insert_resource(TestFont(font));
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
    app.update();
    let world = app.world_mut();
    let root = world
        .query::<(Entity, &crate::ui::sidebars::MapEditorSidebarPage)>()
        .iter(world)
        .find(|(_, page)| page.0 == crate::ui::sidebars::EditorSidebarTab::Right(crate::ui::sidebars::EditorRightSidebarTab::Scene))
        .unwrap().0;
    let tree = world.query::<(Entity, &crate::ui::sidebars::MapEditorSidebarPage)>().iter(world)
        .find(|(_, page)| page.0 == crate::ui::sidebars::EditorSidebarTab::Navigator(crate::ui::sidebars::EditorNavigatorTab::Scene)).unwrap().0;
    // 属性布局矩阵只测右栏；左侧真实树独立保留，不叠在属性上参与排版。
    let tree_host = world.spawn(Node { display: Display::None, width: px(340), height: px(350), flex_direction: FlexDirection::Column, ..default() }).id();
    world.entity_mut(tree).insert(ChildOf(tree_host));
    let host = world
        .spawn(Node {
            width: px(340),
            height: px(350),
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .id();
    world.entity_mut(root).insert(ChildOf(host));
    settle(&mut app);
    (app, host)
}

pub(crate) fn settle(app: &mut App) {
    for _ in 0..6 {
        app.update();
        let font = app.world().resource::<TestFont>().0.clone();
        let world = app.world_mut();
        for mut text in world.query::<&mut TextFont>().iter_mut(world) {
            if text.font != font.clone().into() {
                text.font = font.clone().into();
            }
        }
    }
    app.update();
}

fn select(app: &mut App, mesh: bool) {
    let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
    browser.selected_placement_id = Some("instance-0".into());
    browser.selection = mesh.then(|| EditorGltfMeshSelection {
        placement_id: "instance-0".into(),
        node_path: vec!["根节点".into(), "河面".into()],
        mesh_name: "河面网格".into(),
        mesh_index: Some(0),
        valid: true,
        orphaned_water_binding: false,
    });
    settle(app);
}

fn text(app: &mut App, field: SceneInspectorField) -> String {
    let world = app.world_mut();
    world
        .query::<(&SceneInspectorText, &Text)>()
        .iter(world)
        .find(|(marker, _)| marker.0 == field)
        .unwrap()
        .1
        .0
        .clone()
}

fn properties_display(app: &mut App) -> Display {
    let world = app.world_mut();
    world
        .query_filtered::<&Node, With<SceneInspectorProperties>>()
        .single(world)
        .unwrap()
        .display
}

#[test]
fn formal_selection_routes_one_property_domain_and_does_not_follow_cached_placement() {
    let (mut app, _) = page(1);
    select(&mut app, false);
    let before = author(&app);
    for (target, expected) in [
        (crate::viewport::LayerTarget::TerrainObject("mesh-a".into()), InspectorSection::Terrain),
        (crate::viewport::LayerTarget::Water(crate::types::WaterAuthoringFeatureKind::StillWater, "pond".into()), InspectorSection::Water),
        (crate::viewport::LayerTarget::Domain("guard".into()), InspectorSection::Domain),
        (crate::viewport::LayerTarget::Vegetation("grass".into()), InspectorSection::Vegetation),
        (crate::viewport::LayerTarget::SubMap("village".into()), InspectorSection::SubMap),
    ] {
        app.world_mut().resource_mut::<crate::viewport::LayerCandidateState>().selected = Some(target);
        app.update();
        assert_eq!(properties_display(&mut app), Display::None);
        let world = app.world_mut();
        for (section, node) in world.query::<(&InspectorSection, &Node)>().iter(world) {
            assert_eq!(node.display, if *section == expected { Display::Flex } else { Display::None });
        }
    }
    assert_eq!(author(&app), before);
}

#[test]
fn multiple_selection_shows_management_and_hides_single_object_transform() {
    let (mut app, _) = page(2);
    select(&mut app, false);
    let session = app.world().resource::<EditorAuthoringSession>();
    let mut set = crate::viewport::multi_select::PlacementSelectionSet::default();
    set.map = session.draft.as_ref().map(|draft| draft.map_id.clone());
    set.author_revision = session.revision;
    set.structure_revision = session.placement_structure_revision;
    set.ready = true;
    set.members = std::sync::Arc::new((0..2).map(|index| crate::viewport::multi_select::SelectedMember {
            id: format!("instance-{index}"), index, position: Vec3::ZERO,
        }).collect());
    app.insert_resource(set);
    app.update();
    assert_eq!(properties_display(&mut app), Display::None);
    assert!(text(&mut app, SceneInspectorField::Name).contains("2 个对象"));
    let world = app.world_mut();
    assert_eq!(world.query::<(&InspectorSection, &Node)>().iter(world)
        .filter(|(_, node)| node.display == Display::Flex).count(), 1);
}

fn author(app: &App) -> (String, u64, usize, usize, bool) {
    let session = app.world().resource::<EditorAuthoringSession>();
    (
        serde_json::to_string(&session.draft.as_ref().unwrap().placements.placements).unwrap(),
        session.revision,
        session.undo.len(),
        session.redo.len(),
        app.world().resource::<MapEditorState>().dirty,
    )
}

#[test]
fn inspector_overview_separates_identity_and_keeps_original_mesh_diagnostics() {
    let (mut app, _) = page(1);
    let before = author(&app);
    assert_eq!(text(&mut app, SceneInspectorField::Name), "地图 · 检查器测试地图");
    assert_eq!(properties_display(&mut app), Display::None);
    select(&mut app, false);
    assert_eq!(
        text(&mut app, SceneInspectorField::Name),
        "场景中的测试资产.glb"
    );
    let details = text(&mut app, SceneInspectorField::Details);
    assert!(details.contains("普通摆放实例") && details.contains("instance-0"));
    assert!(details.contains("gltf/") && !details.contains("位置"));
    assert_eq!(properties_display(&mut app), Display::Flex);
    select(&mut app, true);
    assert_eq!(text(&mut app, SceneInspectorField::Name), "河面网格");
    assert!(text(&mut app, SceneInspectorField::Details).contains("根节点 / 河面"));
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .selection
        .as_mut()
        .unwrap()
        .valid = false;
    app.update();
    assert!(text(&mut app, SceneInspectorField::Hint).contains("名称不稳定"));
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .selection
        .as_mut()
        .unwrap()
        .orphaned_water_binding = true;
    app.update();
    assert!(text(&mut app, SceneInspectorField::Hint).contains("不会删除整个实例"));
    let world = app.world_mut();
    let water = world
        .query_filtered::<&Text, With<MapEditorPlacementWaterText>>()
        .single(world)
        .unwrap();
    assert!(water.as_str().contains("失效水绑定")); // 保留原恢复入口和诊断，不将失效改写成成功。
    assert_eq!(author(&app), before);
}

#[test]
fn inspector_no_map_loading_and_stale_map_never_show_old_object_properties() {
    let (mut app, _) = page(1);
    select(&mut app, false);
    let before = author(&app);
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = None;
    app.update();
    assert_eq!(text(&mut app, SceneInspectorField::Name), "未进入地图");
    assert_eq!(properties_display(&mut app), Display::None);
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = Some(MapId::parse("OTHER").unwrap());
    app.update();
    assert_eq!(text(&mut app, SceneInspectorField::Name), "等待地图草稿");
    assert!(!text(&mut app, SceneInspectorField::Details).contains("instance-0"));
    let mut editor = app.world_mut().resource_mut::<MapEditorState>();
    editor.map_transition_in_progress = true;
    editor.status = "加载失败：实际GLB文件不存在".into();
    app.update();
    assert_eq!(text(&mut app, SceneInspectorField::Name), "地图加载中");
    assert_eq!(app.world().resource::<MapEditorState>().status, "加载失败：实际GLB文件不存在"); // 实际失败仍供全局底栏显示。
    assert_eq!(properties_display(&mut app), Display::None);
    assert_eq!(author(&app), before);
}

#[test]
fn inspector_invalid_structure_index_hides_fields_until_original_index_is_ready() {
    let (mut app, _) = page(1);
    select(&mut app, false);
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .selected_placement_id = Some("deleted-instance".into());
    app.update();
    assert_eq!(
        text(&mut app, SceneInspectorField::Name),
        "所选对象暂不可用"
    );
    assert_eq!(properties_display(&mut app), Display::None);
    let world = app.world_mut();
    let mut session = world.resource_mut::<EditorAuthoringSession>();
    session
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements
        .push(placement(0)); // 重复ID由原结构构建拒绝，不猜测首条。
    session.placement_structure_revision += 1;
    select(&mut app, false);
    assert_eq!(properties_display(&mut app), Display::None);
}

#[test]
fn inspector_base_terrain_and_building_use_distinct_read_only_summary() {
    let (mut app, _) = page(1);
    let before = author(&app);
    let map_id = app
        .world()
        .resource::<MapEditorState>()
        .selected_map_id
        .clone();
    let mut shading = app.world_mut().resource_mut::<TerrainShadingSession>();
    shading.map_id = map_id;
    shading.preview_expanded = true;
    app.update();
    assert_eq!(text(&mut app, SceneInspectorField::Name), "原地图基础网格");
    assert!(text(&mut app, SceneInspectorField::Hint).contains("下方显示网格属性"));
    assert_eq!(properties_display(&mut app), Display::None);
    assert_eq!(author(&app), before);
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    session.draft.as_mut().unwrap().placements.placements[0].building_preset_id =
        Some("courtyard-gate".into());
    session.revision += 1;
    select(&mut app, false);
    assert!(text(&mut app, SceneInspectorField::Details).contains("建筑预设实例"));
    assert!(text(&mut app, SceneInspectorField::Hint).contains("90°"));
    let world = app.world_mut();
    for (_, disabled) in world
        .query::<(&PlacementTransformField, Has<NumericInputDisabled>)>()
        .iter(world)
    {
        assert!(disabled); // 建筑仍沿用原任意变换禁用门，不能因排版重新开放。
    }
    let quarter = world
        .query::<(&MapEditorPlacementTransformActionButton, &Node)>()
        .iter(world)
        .find(|(marker, _)| marker.0 == PlacementTransformAction::RotateQuarterTurn)
        .unwrap()
        .1;
    assert_eq!(quarter.display, Display::Flex);
}

#[test]
fn inspector_locate_uses_original_context_gate_without_changing_tool_or_selection() {
    let (mut app, _) = page(1);
    select(&mut app, false);
    let before = author(&app);
    let world = app.world_mut();
    let button = world
        .query_filtered::<Entity, With<MapEditorPlacementTransformLocate>>()
        .single(world)
        .unwrap();
    world.resource_mut::<MapEditorState>().tool = crate::types::EditorTool::Inspect;
    app.update();
    assert!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(button)
            .is_none()
    );
    app.world_mut().resource_mut::<MapEditorState>().tool = crate::types::EditorTool::Portal;
    app.update();
    assert!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(button)
            .is_some()
    );
    assert!(text(&mut app, SceneInspectorField::Hint).contains("定位请先切到选择模式"));
    assert_eq!(
        app.world()
            .resource::<EditorGltfBrowserState>()
            .selected_placement_id
            .as_deref(),
        Some("instance-0")
    );
    assert_eq!(
        app.world().resource::<MapEditorState>().tool,
        crate::types::EditorTool::Portal
    );
    app.world_mut().resource_mut::<MapEditorState>().tool = crate::types::EditorTool::Inspect;
    select(&mut app, true);
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .selection
        .as_mut()
        .unwrap()
        .mesh_name
        .clear();
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .selection
        .as_mut()
        .unwrap()
        .valid = false;
    app.update();
    assert_eq!(text(&mut app, SceneInspectorField::Name), "未命名网格");
    assert!(
        app.world()
            .get::<bevy::ui::InteractionDisabled>(button)
            .is_some()
    );
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    app.update();
    assert!(text(&mut app, SceneInspectorField::Hint).contains("地图保存中"));
    assert_eq!(author(&app), before);
}

#[derive(Resource, Default)]
struct InspectorChanges {
    texts: usize,
    nodes: usize,
}

fn observe_changes(
    texts: Query<Ref<Text>, With<SceneInspectorText>>,
    nodes: Query<Ref<Node>, Or<(With<SceneInspectorText>, With<SceneInspectorProperties>)>>,
    mut changes: ResMut<InspectorChanges>,
) {
    changes.texts = texts.iter().filter(|text| text.is_changed()).count();
    changes.nodes = nodes.iter().filter(|node| node.is_changed()).count();
}

#[test]
fn inspector_stable_frames_and_global_status_do_not_rewrite_properties_or_layout() {
    let (mut app, _) = page(1);
    select(&mut app, false);
    app.init_resource::<InspectorChanges>()
        .add_systems(Update, observe_changes.after(sync_scene_inspector));
    app.update();
    app.update();
    assert_eq!(app.world().resource::<InspectorChanges>().texts, 0);
    assert_eq!(app.world().resource::<InspectorChanges>().nodes, 0);
    app.world_mut().resource_mut::<MapEditorState>().status = "保存失败：写入被拒绝".into();
    app.update();
    assert_eq!(app.world().resource::<InspectorChanges>().texts, 0);
    assert_eq!(app.world().resource::<InspectorChanges>().nodes, 0);
    let world = app.world_mut();
    assert!(world.query_filtered::<&Text, With<SceneInspectorText>>().iter(world)
        .all(|text| !text.as_str().contains("最近操作") && !text.as_str().contains("保存失败")));
    app.world_mut().resource_mut::<MapEditorState>().status = "保存失败：写入被拒绝".into();
    app.update();
    assert_eq!(app.world().resource::<InspectorChanges>().texts, 0);
}

#[test]
fn inspector_builtin_cuboid_exposes_shape_fields_and_hides_glb_terrain_actions() {
    let (mut app, _) = page(1);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
        record.asset_path.clear();
        record.asset_target = feathrise_map_content::MapPlacementAssetTarget::DEFAULT_CUBOID;
        session.placement_structure_revision += 1;
    }
    select(&mut app, false);
    let world = app.world_mut();
    let fields = world.query::<(&PlacementTransformField, Has<NumericInputDisabled>, &ComputedNode)>().iter(world)
        .filter(|(field, ..)| field.is_cuboid()).collect::<Vec<_>>();
    assert_eq!(fields.len(), 7);
    assert!(fields.iter().all(|(_, disabled, computed)| !disabled && computed.size().y > 0.0));
    for (button, node) in world.query::<(&MapEditorPlacementTransformActionButton, &Node)>().iter(world) {
        match button.0 {
            PlacementTransformAction::ToggleTerrainPatch | PlacementTransformAction::ToggleTerrainCutout => assert_eq!(node.display, Display::None),
            PlacementTransformAction::ToggleCuboidCollision => assert_eq!(node.display, Display::Flex),
            _ => {}
        }
    }
    assert!(text(&mut app, SceneInspectorField::Name).contains("立方体"));
    let details = text(&mut app, SceneInspectorField::Details);
    assert!(details.contains("类型  网格\n来源  内置立方体"));
    assert!(!details.contains("摆放"));
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}

#[test]
fn builtin_mesh_properties_keep_four_continuous_capability_groups_and_reuse_the_existing_inputs() {
    let (mut app, _) = page(1);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
        record.asset_path.clear();
        record.asset_target = feathrise_map_content::MapPlacementAssetTarget::DEFAULT_CUBOID;
        session.placement_structure_revision += 1;
    }
    select(&mut app, false);
    let world = app.world_mut();
    let panel = world.query_filtered::<Entity, With<MapEditorPlacementTransformPanel>>().single(world).unwrap();
    let groups = world.get::<Children>(panel).unwrap().iter()
        .filter_map(|entity| world.get::<PlacementPropertiesSection>(entity).map(|section| (entity, *section)))
        .collect::<Vec<_>>();
    assert_eq!(groups.iter().map(|(_, section)| *section).collect::<Vec<_>>(), [
        PlacementPropertiesSection::Transform, PlacementPropertiesSection::Geometry,
        PlacementPropertiesSection::Material, PlacementPropertiesSection::Collision,
    ]);
    for &(entity, _) in &groups {
        assert_eq!(world.get::<Node>(entity).unwrap().display, Display::Flex);
        assert!(world.get::<ComputedNode>(entity).unwrap().size().y > 0.0);
    }
    let fields = world.query::<(Entity, &PlacementTransformField)>().iter(world)
        .map(|(entity, field)| (entity, *field)).collect::<Vec<_>>();
    for &(entity, field) in &fields {
        let expected = match field {
            PlacementTransformField::CuboidSizeX | PlacementTransformField::CuboidSizeY | PlacementTransformField::CuboidSizeZ => PlacementPropertiesSection::Geometry,
            PlacementTransformField::CuboidColorR | PlacementTransformField::CuboidColorG | PlacementTransformField::CuboidColorB | PlacementTransformField::CuboidRoughness => PlacementPropertiesSection::Material,
            field if field.is_cutout() => PlacementPropertiesSection::Geometry,
            _ => PlacementPropertiesSection::Transform,
        };
        let mut ancestor = entity;
        loop {
            if let Some(actual) = world.get::<PlacementPropertiesSection>(ancestor) {
                assert_eq!(*actual, expected);
                break;
            }
            ancestor = world.get::<ChildOf>(ancestor).unwrap().parent();
        }
    }
    {
        let mut session = world.resource_mut::<EditorAuthoringSession>();
        let record = &mut session.draft.as_mut().unwrap().placements.placements[0];
        record.asset_path = "gltf/external.glb".into();
        record.asset_target = default();
        session.placement_structure_revision += 1;
    }
    select(&mut app, false);
    let world = app.world_mut();
    for (entity, section) in groups {
        let expected = if matches!(section, PlacementPropertiesSection::Material | PlacementPropertiesSection::Collision) { Display::None } else { Display::Flex };
        assert_eq!(world.get::<Node>(entity).unwrap().display, expected);
    }
    for (entity, field) in fields {
        assert_eq!(world.get::<PlacementTransformField>(entity), Some(&field)); // 换能力只改显隐，不重建专业数字输入。
    }
    let dimension = world.query_filtered::<&Node, With<MapEditorCuboidGeometryRow>>().single(world).unwrap();
    assert_eq!(dimension.display, Display::None);
    assert!(world.resource::<EditorAuthoringSession>().undo.is_empty());
}

#[path = "layout_tests.rs"]
mod layout_tests;
