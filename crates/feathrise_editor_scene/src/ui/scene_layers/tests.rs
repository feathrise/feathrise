//! 使用自建多作者层草稿核对正式树行、末尾滚动与切换场景撤销身份，不读取游戏场景。

use super::*;
use crate::{
    authoring::EditorAuthoringSession,
    resources::SceneEditorState,
    ui::scene_context_menu::{SceneObjectKey, SceneObjectRow},
};
use bevy::prelude::*;
use crate::game_authoring::scene_interaction::{fixtures, DomainItemKind, SceneInteractionState};

fn test_object(session: &EditorAuthoringSession, list: types::LayerList, index: usize) -> Option<(crate::ui::scene_context_menu::SceneObjectTarget, String)> {
    let draft = session.draft.as_ref()?;
    let mut inventory = inventory::LayerInventory::default();
    inventory.sync(session, draft, None);
    rows::object(session, &inventory, None, list, index, Some(&fixtures::state(draft.adopted_domain_hash(), 60)), (&Default::default(), None))
}


pub(crate) fn install_snapshot_sync(app: &mut App) {
    app.add_systems(Update, (systems::sync, systems::sync_vegetation_toggles).chain());
}

#[test]
fn water_category_controls_keep_editor_visibility_separate_from_saved_participation() {
    use crate::authoring::layer_categories;
    use crate::resources::SceneEditorNumericScrubMouseGuard;
    use crate::ui::{scene_context_menu::SceneContextMenuState, scene_sidebar::TreeRowToggle};
    use feathrise_editor_ui::EditorButtonClick;
    use feathrise_scene_content::SceneManifestAsset;

    let mut app = App::new();
    app.init_resource::<SceneEditorState>()
        .init_resource::<EditorAuthoringSession>()
        .init_resource::<SceneContextMenuState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<LayerCategoryVisibility>()
        .add_systems(Update, systems::toggle_categories);
    let draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    let scene = draft.scene_id.clone();
    let supported = layer_categories::supported(&draft);
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft = Some(draft);
    app.world_mut().spawn((Window { focused: true, ..default() }, bevy::window::PrimaryWindow));
    let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
    editor.open = true;
    editor.selected_scene_id = Some(scene.clone());
    let click = |app: &mut App, toggle| {
        let button = app.world_mut().spawn((components::SectionToggle {
            list: types::LayerList::Water, toggle, scene: Some(scene.clone()),
        }, EditorButtonClick::triggered())).id();
        app.update();
        app.world_mut().despawn(button);
    };
    let scope = |app: &App| layer_categories::category_state(
        app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap(), false);
    click(&mut app, TreeRowToggle::Visibility);
    assert!(app.world().resource::<LayerCategoryVisibility>().hidden(&scene, false));
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    assert_eq!(scope(&app), (true, false));
    if !supported { return; }

    click(&mut app, TreeRowToggle::Lock);
    click(&mut app, TreeRowToggle::Participation);
    assert_eq!(scope(&app), (true, true));
    click(&mut app, TreeRowToggle::Lock);
    click(&mut app, TreeRowToggle::Participation);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(scope(&app), (false, false));
    assert!(app.world().resource::<SceneEditorState>().dirty && !session.undo.is_empty());
    let saved = serde_json::to_vec(&draft.manifest).unwrap();
    let reopened: SceneManifestAsset = serde_json::from_slice(&saved).unwrap();
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    draft.manifest = reopened;
    assert_eq!(layer_categories::category_state(&draft, false), (false, false));
    assert_eq!(layer_categories::category_state(&draft, true), (true, false));
}

/// 保存回归通过正式树行解析器取当前快照，避免只检查底层水槽而漏掉UI再次丢项。
pub(crate) fn water_snapshot(session: &EditorAuthoringSession) -> Vec<crate::ui::scene_context_menu::SceneObjectTarget> {
    let Some(draft) = session.draft.as_ref() else { return Vec::new(); };
    let mut inventory = inventory::LayerInventory::default();
    inventory.sync(session, draft, None);
    (0..inventory.count(true)).filter_map(|index|
        rows::object(session, &inventory, None, types::LayerList::Water, index, None, (&default(), None)).map(|item| item.0)).collect()
}

pub(crate) fn assert_snapshot_glyphs(app: &mut App) {
    let world = app.world_mut();
    let mut kinds = [false; 4];
    for (row, node, children) in world
        .query::<(&components::Row, &ComputedNode, &Children)>()
        .iter(world)
    {
        if node.size() == Vec2::ZERO {
            continue;
        }
        kinds[row.list as usize] = true;
        assert!(children.iter().find_map(|child| {
            world.get::<components::RowLabel>(child)?;
            world.get::<Children>(child)?.first().copied()
        }).is_some_and(|child| world.get::<bevy::text::TextLayoutInfo>(child)
            .is_some_and(|layout| !layout.glyphs.is_empty())));
    }
    assert!(kinds.into_iter().all(|present| present));
}

pub(crate) fn load_author_layers_fixture(app: &mut App) {
    use feathrise_scene_content::*;
    use std::sync::Arc;
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.scene_id.clone();
    draft.manifest.display_name = "中性作者树夹具".into();
    draft.manifest.terrains = Some(draft.manifest.terrain_objects_for_edit());
    // 三张私有静水画布属于同一主网格；明确绑定来源，不能把网格水误算成场景根水。
    let primary = draft.manifest.terrains.as_ref().unwrap()[0].clone();
    draft.terrain_documents.active = Some(crate::authoring::terrain_documents::TerrainDocumentIdentity {
        manifest: primary.source_manifest(&draft.manifest),
        folder: draft.folder.clone(),
        definition: Arc::new(primary),
    });
    let mut water = crate::authoring::default_water_asset(&map);
    water.layers = (0..3).map(|index| {
        let mut layer = crate::authoring::default_water_layer(&draft.folder);
        layer.id = format!("fixture-water-{index}");
        layer.label = format!("测试静水{index}");
        layer.mask_source_path = format!("{}/water/{}.r8", draft.folder, layer.id);
        layer.mask_texture_path = format!("{}/water/{}.ktx2", draft.folder, layer.id);
        layer.water_level = index as f32;
        layer.resolution = [2, 2];
        layer
    }).collect();
    draft.water = Some(water);

    let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/vegetation_presets.vegetation_presets.json"
    ))).unwrap();
    let presets = ResolvedVegetationPresetCatalog::build(&catalog).unwrap();
    let layers = presets.presets().iter().take(3).map(|preset| {
        let mut output = preset.create_scene_layer(&draft.folder);
        output.resolution = [2, 2];
        output
    }).collect::<Vec<_>>();
    let authoring = SceneVegetationAuthoring {
        groups: vec![SceneVegetationAuthorGroup {
            id: "fixture-group".into(), label: "测试植被组".into(),
            mode: SceneVegetationAuthorGroupMode::Additive,
            output_merge: SceneVegetationOutputMergeMode::Maximum,
            capacity: 1.0, claim_threshold: 0.0, selection_seed: 1,
            selection_cell_meters: 1.0, enabled: true,
        }],
        layers: layers.iter().enumerate().map(|(index, output)| SceneVegetationAuthorLayer {
            id: format!("fixture-author-{index}"), label: format!("测试植被{index}"),
            group_id: "fixture-group".into(), output_layer_id: output.id.clone(),
            density_source_path: format!("{}/vegetation/author-{index}.r8", draft.folder),
            resolution: [2, 2], enabled: true, locked: false, opacity: 1.0,
            priority: 0, weight: 1.0,
        }).collect(),
    };
    draft.vegetation = Some(SceneVegetationAsset {
        model_surfaces: Vec::new().into(),
        schema_version: SCENE_SCHEMA_VERSION, scene_id: map.clone(), layers,
        authoring: Some(authoring),
    });

    // 六十个独立来源超过固定四十行槽，末尾滚动仍须按真实文档索引找到最后一个。
    draft.domain = Some(fixtures::document("fixture-domain-v1"));
    app.insert_resource(fixtures::state("fixture-domain-v1", 60));
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft = Some(draft);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_scene_id = Some(map);
}

/// 当前地形有完整植被，场景根有静水；两类行必须跨来源选择而不改变布局。
pub(crate) fn add_scene_root_water_fixture(app: &mut App) -> feathrise_scene_content::TerrainId {
    use crate::authoring::terrain_documents::RootWaterCanvas;
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    let mut root = crate::authoring::default_water_asset(&draft.scene_id);
    let mut water = crate::authoring::default_water_layer(&draft.folder);
    water.id = "fixture-root-water".into();
    water.label = "测试场景静水".into();
    root.layers.push(water);
    draft.terrain_documents.root_water = Some(RootWaterCanvas { water: Some(root), ..default() });
    draft.terrain_documents.active.as_ref().unwrap().definition.id.clone()
}

fn section_type_icon(world: &World, arrow_button: Entity) -> Entity {
    let row = world.get::<ChildOf>(arrow_button).unwrap().parent();
    world.get::<Children>(row).unwrap().iter()
        .filter_map(|child| world.get::<Children>(child)).flat_map(|children| children.iter())
        .find(|&child| world.get::<crate::components::SceneEditorTreeGlyph>(child).is_some()).unwrap()
}

#[test]
fn scene_tree_mesh_section_and_branch_columns_keep_actual_layout_aligned() {
    use crate::components::SceneEditorGltfBrowserRowGlyph;
    use crate::ui::gltf_browser::*;
    use crate::ui::scene_inspector::{InspectorTree, tests::{page, settle}};
    use feathrise_scene_content::ScenePlacementAssetTarget;

    let (mut app, _) = page(2);
    let mut placements = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap()
        .placements.placements.clone();
    let child = &mut placements[1];
    std::sync::Arc::make_mut(&mut child.editor).group = "摆放组".into();
    child.asset_path.clear();
    child.asset_target = ScenePlacementAssetTarget::Cuboid {
        size_mm: [1000; 3], color_srgb: [160; 3], roughness_milli: 600, collision: false,
    };
    let child_id = child.id.clone();
    load_author_layers_fixture(&mut app);
    {
        let world = app.world_mut();
        let mut session = world.resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().placements.placements = placements;
        session.placement_structure_revision += 1;
        let tree = world.query_filtered::<Entity, With<InspectorTree>>().single(world).unwrap();
        let mut host = tree;
        while let Some(parent) = world.get::<ChildOf>(host) { host = parent.parent(); }
        *world.get_mut::<Node>(host).unwrap() = Node {
            width: px(300), height: px(1450), flex_direction: FlexDirection::Column, ..default()
        };
    }
    app.init_asset::<Image>()
        .init_resource::<crate::ui::scene_objects::SceneObjectState>()
        .init_resource::<resources::LayerTreeFolds>()
        .add_systems(Update, (systems::sync, systems::sync_section_arrows, systems::sync_vegetation_toggles).chain())
        .add_systems(Update, sync_gltf_row_toggles.after(sync_gltf_browser_rows));
    settle(&mut app);
    {
        let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
        let group = browser.entries.iter().position(|entry| matches!(&entry.kind, EditorGltfBrowserEntryKind::Group { .. })).unwrap();
        browser.toggle_tree_branch(group);
    }
    for scale in [1.0, 1.5] {
        app.world_mut().resource_mut::<UiScale>().0 = scale;
        settle(&mut app);
        let world = app.world_mut();
        assert!(!world.query::<&Text>().iter(world).any(|text| text.as_str() == "网格对象"));
        let display_arrow = world.query_filtered::<Entity, With<SceneBaseGltfArrow>>().single(world).unwrap();
        let layer_buttons: Vec<_> = world.query::<(Entity, &components::SectionArrow)>().iter(world)
            .map(|(entity, _)| entity).collect();
        assert_eq!(layer_buttons.len(), 2);
        let rows: Vec<_> = world.query::<(Entity, &SceneEditorGltfBrowserRow)>().iter(world)
            .filter_map(|(entity, row)| row.entry_index.map(|index| (entity, index))).collect();
        let layer_rows: Vec<_> = world.query::<(Entity, &components::Row)>().iter(world)
            .filter_map(|(entity, row)| (row.slot == 0
                && matches!(row.list, types::LayerList::Water | types::LayerList::Vegetation)).then_some(entity)).collect();
        assert_eq!(layer_rows.len(), 2);
        let browser = world.resource::<EditorGltfBrowserState>();
        let branch_rows: Vec<_> = rows.iter().filter_map(|&(row, index)| {
            (browser.entries[index].depth == 0).then_some(row)
        }).collect();
        assert_eq!(branch_rows.len(), 2);
        let leaf = rows.iter().find_map(|&(row, index)| matches!(&browser.entries[index].kind,
            EditorGltfBrowserEntryKind::Placement { placement_id, .. } if placement_id == &child_id).then_some(row)).unwrap();
        let arrow = |row| world.get::<Children>(row).unwrap().iter()
            .find(|&child| world.get::<PlacementTreeExpander>(child).is_some()).unwrap();
        let object_icon = |row| world.get::<Children>(row).unwrap().iter()
            .filter_map(|child| world.get::<Children>(child)).flat_map(|children| children.iter())
            .find(|&child| world.get::<SceneEditorGltfBrowserRowGlyph>(child).is_some()).unwrap();
        let center = |entity| world.get::<UiGlobalTransform>(entity).unwrap().translation;
        let size = |entity| world.get::<ComputedNode>(entity).unwrap().size();
        let arrow_icon = |button| *world.get::<Children>(button).unwrap().first().unwrap();
        let display_arrow = arrow_icon(display_arrow);
        let layer_glyphs: Vec<_> = layer_buttons.into_iter()
            .map(|button| [arrow_icon(button), section_type_icon(world, button)]).collect();
        let branch_column = center(layer_glyphs[0][0]).x;
        let icon_column = center(object_icon(branch_rows[0])).x;
        let icon_size = size(object_icon(branch_rows[0]));
        assert!(center(display_arrow).x < branch_column);
        assert!(icon_column > branch_column);
        for [arrow, icon] in layer_glyphs {
            assert!(center(display_arrow).y < center(arrow).y && center(arrow).y < center(branch_rows[0]).y);
            assert!((center(arrow).x - branch_column).abs() <= 1.1);
            assert!(size(arrow).abs_diff_eq(size(display_arrow), 1.1));
            assert!((center(icon).x - icon_column).abs() <= 1.1);
            assert!(size(icon).abs_diff_eq(icon_size, 1.1));
        }
        // 同级树行各有一列箭头和类型图标，叶子保留图标列且没有可见箭头。
        for row in branch_rows {
            assert!((center(arrow_icon(arrow(row))).x - branch_column).abs() <= 1.1);
            assert!((center(object_icon(row)).x - icon_column).abs() <= 1.1);
        }
        assert!(world.get::<bevy::ui::InteractionDisabled>(arrow(leaf)).is_some());
        assert_eq!(*world.get::<Visibility>(arrow(leaf)).unwrap(), Visibility::Hidden);
        let layer_icon = |row| world.get::<Children>(row).unwrap().iter()
            .filter_map(|child| world.get::<Children>(child)).flat_map(|children| children.iter())
            .find(|&child| world.get::<crate::components::SceneEditorTreeGlyph>(child).is_some()).unwrap();
        // 文件夹网格、水域和植被子项比正文首层多 14 个逻辑像素。
        let child_column = icon_column + 14.0 * scale;
        assert!((center(object_icon(leaf)).x - child_column).abs() <= 1.1);
        assert!(layer_rows.iter().all(|&row| (center(layer_icon(row)).x - child_column).abs() <= 1.1));
    }
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().placements.placements.clear();
    settle(&mut app);
    let world = app.world_mut();
    let list = world.query_filtered::<&ComputedNode, With<SceneEditorGltfBrowserList>>().single(world).unwrap();
    assert_eq!(list.size(), Vec2::ZERO, "没有摆放时列表不占高度");
}

#[test]
fn scene_layers_fold_scroll_and_same_id_vegetation_stay_with_their_source() {
    use bevy::{ecs::system::RunSystemOnce, window::PrimaryWindow};
    use crate::ui::{gltf_browser::EditorGltfBrowserState,
        scene_context_menu::{SceneContextMenuState, SceneMenuAction, SceneObjectRequest, select_target_for_tests}};
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    // 通用属性夹具只计算布局。真实释放按钮还读取 Bevy 传播过的祖先可见性。
    app.init_asset::<Image>()
        .init_asset::<Mesh>()
        .init_asset::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>()
        .add_plugins(bevy::camera::visibility::VisibilityPlugin);
    let world = app.world_mut();
    let tree = world.query::<(Entity, &crate::ui::sidebars::SceneEditorSidebarPage)>().iter(world)
        .find(|(_, page)| page.0 == crate::ui::sidebars::EditorSidebarTab::Navigator(
            crate::ui::sidebars::EditorNavigatorTab::SceneTree)).unwrap().0;
    let host = world.get::<ChildOf>(tree).unwrap().parent();
    world.get_mut::<Node>(host).unwrap().display = Display::Flex;
    load_author_layers_fixture(&mut app);
    let terrain = add_scene_root_water_fixture(&mut app);
    {
        use crate::authoring::terrain_documents::TerrainCanvas;
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let vegetation = draft.vegetation.as_mut().unwrap();
        let mut root = vegetation.clone();
        root.authoring.as_mut().unwrap().layers.truncate(1);
        draft.terrain_documents.root_vegetation = Some(std::sync::Arc::new(TerrainCanvas {
            vegetation: Some(root), ..default()
        }));
        let layers = &mut vegetation.authoring.as_mut().unwrap().layers;
        let template = layers[0].clone();
        layers.extend((3..16).map(|index| feathrise_scene_content::SceneVegetationAuthorLayer {
            id: format!("fixture-author-{index}"), label: format!("测试植被{index}"), ..template.clone()
        }));
    }
    app.init_resource::<SceneContextMenuState>()
        .init_resource::<SceneLayerSelection>()
        .init_resource::<LayerCategoryVisibility>()
        .init_resource::<resources::LayerTreeFolds>()
        .init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>()
        .init_resource::<crate::authoring::VegetationBrushStroke>()
        .add_systems(Update, (systems::input, select_target_for_tests, systems::fold_sections, systems::sync,
            systems::sync_vegetation_toggles, systems::sync_section_arrows, systems::sync_category_toggles).chain());
    let window = app.world_mut().spawn((Window { focused: true, ..default() }, PrimaryWindow)).id();
    crate::ui::scene_inspector::tests::settle(&mut app);
    let category = |world: &mut World, kind| {
        let list = world.query::<(Entity, &components::List)>().iter(world)
            .find(|(_, list)| list.0 == kind).unwrap().0;
        let button = world.query::<(Entity, &components::SectionArrow)>().iter(world)
            .find(|(_, arrow)| arrow.list == kind).unwrap().0;
        let header = world.get::<ChildOf>(button).unwrap().parent();
        assert!(world.get::<components::SectionSelection>(header).is_some());
        assert!(world.get::<SceneObjectRow>(header).is_some());
        let title = world.query::<(Entity, &components::Title)>().iter(world)
            .find(|(_, title)| title.0 == kind).unwrap().0;
        let horizontal_edges = |entity| {
            let center = world.get::<UiGlobalTransform>(entity).unwrap().translation.x;
            let half_width = world.get::<ComputedNode>(entity).unwrap().size().x / 2.0;
            (center - half_width, center + half_width)
        };
        let arrow = *world.get::<Children>(button).unwrap().first().unwrap();
        let icon = section_type_icon(world, button);
        let icon_path = world.get::<crate::components::SceneEditorTreeGlyph>(icon).unwrap().0;
        assert_eq!(icon_path, if kind == types::LayerList::Water { "icons/editor/tree_water.png" } else { "icons/editor/tree_leaf.png" });
        let (arrow_left, arrow_right) = horizontal_edges(arrow);
        let (icon_left, icon_right) = horizontal_edges(icon);
        let (button_left, button_right) = horizontal_edges(button);
        let (title_left, title_right) = horizontal_edges(title);
        assert!(button_left <= arrow_left && arrow_right <= icon_left && icon_right <= button_right
            && (button_right - title_left).abs() <= 1.0 && title_right > title_left,
            "开合命中区须连贯覆盖箭头和图标，止于正文");
        let controls: Vec<_> = world.get::<Children>(header).unwrap().iter()
            .filter_map(|child| world.get::<Children>(child)).flat_map(|children| children.iter())
            .filter(|&child| world.get::<components::SectionToggle>(child).is_some()).collect();
        assert_eq!(controls.len(), 3);
        for control in controls {
            let (left, right) = horizontal_edges(control);
            assert!(right > left && left >= title_right - 1.0, "右侧三项有独立命中区");
        }
        assert_eq!(world.get::<TextLayout>(title).unwrap().linebreak, TextLayout::no_wrap().linebreak);
        (list, header, button, [arrow, icon], title)
    };
    let (list, vegetation_header, vegetation_button, [vegetation_arrow, vegetation_icon], vegetation_title) =
        category(app.world_mut(), types::LayerList::Vegetation);
    let (water_list, water_header, water_button, [water_arrow, water_icon], water_title) =
        category(app.world_mut(), types::LayerList::Water);
    let selected_category = |app: &App| {
        let world = app.world();
        let scene = &world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id;
        world.resource::<EditorGltfBrowserState>().selected_author_category(scene)
    };
    app.world_mut().get_mut::<ScrollPosition>(list).unwrap().y = 81.0;
    let select_row = |app: &mut App, list, index| {
        let target = test_object(app.world().resource::<EditorAuthoringSession>(), list, index).unwrap().0;
        app.world_mut().resource_mut::<SceneLayerSelection>().0 = Some(target);
        app.world_mut().run_system_once(select_target_for_tests).unwrap();
        app.update();
    };
    let project = |world: &mut World, button, target, interaction| {
        use feathrise_editor_ui::UiInteraction;
        let center = |entity| world.get::<UiGlobalTransform>(entity).unwrap().translation;
        let size = world.get::<ComputedNode>(button).unwrap().size();
        let point = center(target);
        let normalized = (point - center(button)) / size;
        let cursor_over = normalized.abs().max_element() < 0.5;
        assert!(world.get::<InheritedVisibility>(button).unwrap().get(), "分类须通过实际可见性传播");
        world.get_mut::<Window>(window).unwrap().set_physical_cursor_position(Some(point.as_dvec2()));
        world.entity_mut(button).insert((if cursor_over { interaction } else { UiInteraction::None },
            bevy::ui::RelativeCursorPosition { cursor_over, normalized: Some(normalized) }));
        cursor_over
    };
    let click = |app: &mut App, button, pressed_at, released_at| {
        use feathrise_editor_ui::UiInteraction;
        let world = app.world_mut();
        assert!(project(world, button, pressed_at, UiInteraction::Pressed));
        world.resource_mut::<ButtonInput<MouseButton>>().clear();
        world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        app.update();
        let world = app.world_mut();
        world.resource_mut::<ButtonInput<MouseButton>>().clear_just_pressed(MouseButton::Left);
        world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        let same_button = project(world, button, released_at, UiInteraction::Hovered);
        app.update();
        assert_eq!(app.world().get::<feathrise_editor_ui::EditorButtonClick>(button).unwrap().is_clicked(), same_button,
            "仅在按下来源的实际命中范围内释放才产生点击");
    };
    click(&mut app, water_button, water_icon, water_title);
    assert_eq!(app.world().get::<Node>(water_list).unwrap().display, Display::Flex);
    assert_eq!(selected_category(&app), None);
    click(&mut app, water_header, water_title, water_title);
    assert_eq!(selected_category(&app), Some(AuthorLayerCategory::Water));
    assert_eq!(app.world().get::<Node>(water_list).unwrap().display, Display::Flex);
    click(&mut app, water_button, water_arrow, water_arrow);
    assert_eq!(app.world().get::<Node>(water_list).unwrap().display, Display::None);
    assert_eq!(selected_category(&app), Some(AuthorLayerCategory::Water));
    click(&mut app, water_button, water_icon, water_icon);
    assert_eq!(app.world().get::<Node>(water_list).unwrap().display, Display::Flex);
    click(&mut app, vegetation_button, vegetation_icon, vegetation_title);
    assert_eq!(app.world().get::<Node>(list).unwrap().display, Display::Flex);
    assert_eq!(selected_category(&app), Some(AuthorLayerCategory::Water));
    click(&mut app, vegetation_header, vegetation_title, vegetation_title);
    assert_eq!(selected_category(&app), Some(AuthorLayerCategory::Vegetation));
    assert_eq!(app.world().get::<Node>(list).unwrap().display, Display::Flex);
    click(&mut app, vegetation_button, vegetation_icon, vegetation_icon);
    assert_eq!(app.world().get::<Node>(list).unwrap().display, Display::None);
    assert_eq!(selected_category(&app), Some(AuthorLayerCategory::Vegetation));
    select_row(&mut app, types::LayerList::Water, 0);
    assert_eq!(app.world().get::<Node>(list).unwrap().display, Display::None);
    click(&mut app, vegetation_button, vegetation_arrow, vegetation_arrow);
    assert_eq!(app.world().get::<Node>(list).unwrap().display, Display::Flex);
    assert_eq!(app.world().get::<ScrollPosition>(list).unwrap().y, 81.0);

    let (target, _) = test_object(app.world().resource::<EditorAuthoringSession>(), types::LayerList::Vegetation, 1).unwrap();
    let id = match &target.key {
        SceneObjectKey::Vegetation { id, terrain: Some(owner), .. } if owner == &terrain => id.clone(),
        key => panic!("私有植被归属错误：{key:?}"),
    };
    let scene = target.scene_id.clone();
    app.world_mut().resource_mut::<SceneContextMenuState>().ready = Some(SceneObjectRequest {
        target, action: SceneMenuAction::Visibility, name: String::new(),
    });
    app.world_mut().run_system_once(crate::ui::vegetation_layers::handle_scene_menu).unwrap();
    app.world_mut().resource_mut::<SceneContextMenuState>().ready = None;
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.vegetation_visibility.hidden_in(&scene, Some(&terrain)).is_some_and(|hidden| hidden.contains(&id)));
    assert!(editor.vegetation_visibility.hidden_in(&scene, None).is_none());
    assert!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_documents.active.is_none());

    select_row(&mut app, types::LayerList::Vegetation, 1);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(draft.terrain_documents.active.as_ref().map(|canvas| &canvas.definition.id), Some(&terrain));
    assert_eq!(app.world().get::<ScrollPosition>(list).unwrap().y, 81.0);
    assert!(session.undo.is_empty());
}

#[test]
fn scene_layers_domain_bounds_use_the_external_snapshot_and_reject_old_hash() {
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    load_author_layers_fixture(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let (target, _) = test_object(session, types::LayerList::DomainLayers, 0).unwrap();
    let snapshot = app.world().resource::<SceneInteractionState>().snapshot.as_ref().unwrap();
    let bounds = (snapshot.bounds)(DomainItemKind::Layer, "fixture-domain-layer").unwrap();
    assert_eq!(bounds, (Vec3::new(-2., 0., -2.), Vec3::new(11., 8., 7.)));
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().domain = Some(fixtures::document("fixture-domain-v2"));
    assert!(!target.current(app.world().resource::<SceneEditorState>(), app.world().resource::<EditorAuthoringSession>()));
}

#[test]
fn scene_layers_water_and_vegetation_menus_use_the_frozen_row_and_original_history() {
    use crate::ui::scene_context_menu::{
        SceneContextMenuState, SceneMenuAction, SceneObjectRequest,
    };
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    load_author_layers_fixture(&mut app);
    app.init_resource::<SceneContextMenuState>()
        .init_resource::<crate::authoring::WaterBrushStroke>()
        .init_resource::<crate::authoring::WaterNetworkEditState>()
        .init_resource::<crate::authoring::VegetationBrushStroke>()
        .init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>()
        .add_systems(
            Update,
            (
                crate::ui::water_layers::handle_scene_menu,
                crate::ui::vegetation_layers::handle_scene_menu,
            ),
        );
    let terrain = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        // 此夹具已直接载入primary的作者文件，补齐正式加载器采用后的画布身份，不能绕过来源门。
        let object = draft.manifest.terrains.as_ref().unwrap()[0].clone();
        let identity = crate::authoring::terrain_documents::TerrainDocumentIdentity {
            manifest: object.source_manifest(&draft.manifest),
            folder: draft.folder.clone(),
            definition: std::sync::Arc::new(object.clone()),
        };
        draft.terrain_documents.active = Some(identity);
        for layer in &mut draft.water.as_mut().unwrap().layers {
            layer.resolution = [2, 2];
            draft
                .water_masks
                .insert(layer.id.clone(), vec![1; 4].into());
        }
        for layer in &mut draft
            .vegetation
            .as_mut()
            .unwrap()
            .authoring
            .as_mut()
            .unwrap()
            .layers
        {
            draft.vegetation_author_densities.insert(
                layer.id.clone(),
                vec![1; layer.resolution[0] as usize * layer.resolution[1] as usize].into(),
            );
        }
        object.id
    };
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .terrain_canvas_target = Some(terrain);
    let (water_target, _) = test_object(
        app.world().resource::<EditorAuthoringSession>(),
        types::LayerList::Water,
        1,
    )
    .unwrap();
    let original_level = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .water
        .as_ref()
        .unwrap()
        .layers[1]
        .water_level;
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = Some(SceneObjectRequest {
        target: water_target,
        action: SceneMenuAction::Copy,
        name: String::new(),
    });
    app.update();
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = None;
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session.undo.len(),
        1,
        "{}",
        app.world().resource::<SceneEditorState>().status
    );
    assert!(matches!(
        session.undo.last(),
        Some(crate::authoring::EditorCommand::WaterLayersChanged(_))
    ));
    let water = session.draft.as_ref().unwrap().water.as_ref().unwrap();
    assert_eq!(water.layers.len(), 4);
    assert_eq!(water.layers[2].water_level, original_level); // 不能复制原先选择的第一个水体。
    let (target, _) = test_object(session, types::LayerList::Vegetation, 1).unwrap();
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = Some(SceneObjectRequest {
        target,
        action: SceneMenuAction::Lock,
        name: String::new(),
    });
    app.update();
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = None;
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session.undo.len(),
        2,
        "{}",
        app.world().resource::<SceneEditorState>().status
    );
    assert!(matches!(
        session.undo.last(),
        Some(crate::authoring::EditorCommand::VegetationAuthoringChanged(
            _
        ))
    ));
    let layers = &session
        .draft
        .as_ref()
        .unwrap()
        .vegetation
        .as_ref()
        .unwrap()
        .authoring
        .as_ref()
        .unwrap()
        .layers;
    assert!(!layers[0].locked && layers[1].locked && !layers[2].locked);
    let (target, _) = test_object(session, types::LayerList::Vegetation, 1).unwrap();
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = Some(SceneObjectRequest {
        target,
        action: SceneMenuAction::Visibility,
        name: String::new(),
    });
    app.update();
    app.world_mut()
        .resource_mut::<SceneContextMenuState>()
        .ready = None;
    let session = app.world().resource::<EditorAuthoringSession>();
    let layer = &session
        .draft
        .as_ref()
        .unwrap()
        .vegetation
        .as_ref()
        .unwrap()
        .authoring
        .as_ref()
        .unwrap()
        .layers[1];
    assert!(layer.enabled && layer.locked); // 眼睛只影响编辑器，不占正式参与位。
    assert_eq!(session.undo.len(), 2);
    let draft = session.draft.as_ref().unwrap();
    let canvas = &draft.terrain_documents.active.as_ref().unwrap().definition.id;
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.vegetation_visibility.hidden_in(&draft.scene_id, Some(canvas)).unwrap().contains(&layer.id));
}

#[test]
fn scene_layers_author_revision_keeps_the_writer_selected_water() {
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    load_author_layers_fixture(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let old = session
        .draft
        .as_ref()
        .unwrap()
        .water
        .as_ref()
        .unwrap()
        .layers[0]
        .id
        .clone();
    let expected = session
        .draft
        .as_ref()
        .unwrap()
        .water
        .as_ref()
        .unwrap()
        .layers[1]
        .id
        .clone();
    app.world_mut().resource_scope(
        |world, mut candidates: Mut<crate::viewport::LayerCandidateState>| {
            candidates.confirm_tree_author(
                crate::viewport::LayerTarget::Water(
                    crate::types::WaterAuthoringFeatureKind::StillWater,
                    old,
                ),
                Some(0),
                world.resource::<EditorAuthoringSession>(),
                0,
                Default::default(),
            );
        },
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_water_layer = 1;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .revision += 1;
    app.add_systems(Update, systems::refresh_selection);
    app.update();
    let candidates = app
        .world()
        .resource::<crate::viewport::LayerCandidateState>();
    assert_eq!(
        candidates.selected,
        Some(crate::viewport::LayerTarget::Water(
            crate::types::WaterAuthoringFeatureKind::StillWater,
            expected
        ))
    );
    assert_eq!(candidates.selected_author_index, Some(1));
}

#[test]
fn water_selection_refresh_keeps_same_source_edits_and_rejects_restored_other_sources() {
    use bevy::ecs::system::RunSystemOnce;
    use crate::types::{EditorTool, WaterAuthoringFeatureKind};
    use crate::viewport::{LayerCandidateState, LayerTarget};
    for completed_canvas in [false, true] {
        let (mut session, private, id) = crate::ui::water_feature_list::source_selection_tests::same_id_sources();
        let map = session.draft.as_ref().unwrap().scene_id.clone();
        let mut editor = SceneEditorState { open: true, selected_scene_id: Some(map), tool: EditorTool::Inspect, ..default() };
        if !completed_canvas {
            crate::ui::water_feature_list::activate_listed_water_feature(&mut editor, &mut session,
                Some(&private), false, WaterAuthoringFeatureKind::StillWater, &id, None, None).unwrap();
        }
        editor.tool = EditorTool::Inspect;
        let selected = LayerTarget::Water(WaterAuthoringFeatureKind::StillWater, id.clone());
        let mut state = LayerCandidateState::default();
        state.confirm_tree_author(selected.clone(), Some(0), &session, 0, default());
        let mut app = App::new();
        app.insert_resource(editor).insert_resource(session).insert_resource(state);
        // 同一画布的普通参数修订仍走生产刷新，必须保留正式水选择和可编辑的当前作者身份。
        app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1;
        app.world_mut().run_system_once(systems::refresh_selection).unwrap();
        assert_eq!(app.world().resource::<LayerCandidateState>().selected.as_ref(), Some(&selected));
        assert!(app.world().resource::<LayerCandidateState>().selected_matches_session(app.world().resource::<EditorAuthoringSession>()));
        let before = app.world().resource::<EditorAuthoringSession>().revision;
        app.world_mut().resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
            let mut editor = world.resource_mut::<SceneEditorState>();
            crate::ui::water_feature_list::activate_listed_water_feature(&mut editor, &mut session,
                completed_canvas.then_some(&private), false, WaterAuthoringFeatureKind::StillWater, &id, None, None).unwrap();
            editor.tool = EditorTool::Inspect;
        }); // 通过真实来源恢复桥换槽；同ID的另一个文档没有继承旧选择的权限。
        if completed_canvas {
            let revision = app.world().resource::<EditorAuthoringSession>().revision;
            app.world_mut().resource_mut::<LayerCandidateState>().complete_terrain_canvas(&private, 0, before, revision);
            assert!(app.world().resource::<LayerCandidateState>().selected.is_none(), "画布完成不能保留另一来源的旧水选择");
        }
        app.world_mut().run_system_once(systems::refresh_selection).unwrap();
        assert!(app.world().resource::<LayerCandidateState>().selected.is_none(), "修订刷新不能把旧水索引套到恢复后的画布");
    }
}

#[test]
fn scene_layers_domain_focus_adopts_complete_bounds_and_rejects_late_camera_changes() {
    use crate::resources::SceneEditorViewportState;
    use crate::ui::scene_context_menu::{
        SceneContextMenuState, SceneMenuAction, SceneObjectRequest,
    };
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    load_author_layers_fixture(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let (target, _) = test_object(session, types::LayerList::DomainLayers, 0).unwrap();
    let snapshot = app.world().resource::<SceneInteractionState>().snapshot.as_ref().unwrap();
    let bounds = (snapshot.bounds)(DomainItemKind::Layer, "fixture-domain-layer").unwrap();
    let id = "fixture-domain-layer".to_owned();
    let map = target.scene_id.clone();
    app.init_resource::<SceneContextMenuState>()
        .init_resource::<resources::LayerFocusState>()
        .init_resource::<crate::game_authoring::scene_interaction::SceneInteractionState>()
        .insert_resource(SceneEditorViewportState {
            _image: Handle::default(),
            camera: Entity::PLACEHOLDER,
            framed_scene_id: Some(map),
            center: Vec3::splat(999.0),
            scene_center: Vec3::ZERO,
            scene_half_extents: Vec3::ONE,
            extent: 100.0,
            yaw: 0.8,
            pitch: 0.4,
            render_size: UVec2::splat(900),
        })
        .add_systems(Update, focus::update);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        bevy::window::PrimaryWindow,
    ));
    app.world_mut()
        .resource_mut::<crate::game_authoring::scene_interaction::SceneInteractionState>()
        .selected_layer = Some(id.clone());
    app.world_mut()
        .resource_mut::<crate::viewport::LayerCandidateState>()
        .selected = Some(crate::viewport::LayerTarget::Domain(id));
    let request = SceneObjectRequest {
        target,
        action: SceneMenuAction::Focus,
        name: String::new(),
    };
    for late in [false, true] {
        app.world_mut()
            .resource_mut::<SceneContextMenuState>()
            .ready = Some(request.clone());
        app.update();
        app.world_mut()
            .resource_mut::<SceneContextMenuState>()
            .ready = None;
        let moved = Vec3::new(123.0, 456.0, 789.0);
        if late {
            app.world_mut()
                .resource_mut::<SceneEditorViewportState>()
                .center = moved;
        }
        for _ in 0..100 {
            app.update();
            if app
                .world()
                .resource::<resources::LayerFocusState>()
                .pending
                .is_none()
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(
            app.world()
                .resource::<resources::LayerFocusState>()
                .pending
                .is_none()
        );
        let center = app.world().resource::<SceneEditorViewportState>().center;
        assert_eq!(
            center,
            if late {
                moved
            } else {
                bounds.0 * 0.5 + bounds.1 * 0.5
            }
        );
    }
}

#[test]
fn scene_a_scene_tree_contains_water_vegetation_and_domain_and_scrolls_to_last_source() {
    let (mut app, _) = crate::ui::scene_inspector::tests::page(0);
    load_author_layers_fixture(&mut app);
    app.add_systems(Update, systems::sync);
    crate::ui::scene_inspector::tests::settle(&mut app);
    let world = app.world_mut();
    let bindings: Vec<_> = world
        .query_filtered::<&SceneObjectRow, With<components::Row>>()
        .iter(world)
        .filter_map(|row| row.0.as_ref())
        .collect();
    assert!(
        bindings
            .iter()
            .any(|target| matches!(target.key, SceneObjectKey::Water { .. }))
    );
    assert!(
        bindings
            .iter()
            .any(|target| matches!(target.key, SceneObjectKey::Vegetation { output: false, .. }))
    );
    assert!(
        bindings
            .iter()
            .any(|target| matches!(target.key, SceneObjectKey::Domain { kind: DomainItemKind::Layer, .. }))
    );
    assert!(
        bindings
            .iter()
            .any(|target| matches!(target.key, SceneObjectKey::Domain { kind: DomainItemKind::Object, .. }))
    );
    let last_id = "fixture-source-59".to_owned();
    let list = world
        .query::<(Entity, &components::List)>()
        .iter(world)
        .find(|(_, list)| list.0 == types::LayerList::DomainObjects)
        .unwrap()
        .0;
    world.get_mut::<ScrollPosition>(list).unwrap().y = 1_000_000.0;
    crate::ui::scene_inspector::tests::settle(&mut app);
    let world = app.world_mut();
    assert!(world.query_filtered::<&SceneObjectRow, With<components::Row>>().iter(world)
        .filter_map(|row| row.0.as_ref()).any(|target| matches!(&target.key, SceneObjectKey::Domain { kind: DomainItemKind::Object, id, .. } if id == &last_id)));
    let saved = world
        .query_filtered::<&SceneObjectRow, With<components::Row>>()
        .iter(world)
        .find_map(|row| row.0.clone())
        .unwrap();
    world.resource_mut::<SceneEditorState>().selected_scene_id =
        Some(feathrise_scene_content::SceneId::parse("OTHER").unwrap());
    crate::ui::scene_inspector::tests::settle(&mut app);
    let world = app.world_mut();
    assert!(
        world
            .query_filtered::<&SceneObjectRow, With<components::Row>>()
            .iter(world)
            .all(|row| row.0.is_none())
    );
    assert!(!saved.current(
        world.resource::<SceneEditorState>(),
        world.resource::<EditorAuthoringSession>()
    ));
}
