//! 场景树真实输入回归，分别验证名称选择与类型图标展开。

use super::super::{EditorGltfBrowserEntry, EditorGltfMeshSelection};
use super::*;
use crate::viewport::{LayerCandidateState, LayerTarget, modes::{ViewportMode, ViewportModeState}};

fn mesh(id: &str) -> EditorGltfMeshSelection {
    EditorGltfMeshSelection {
        placement_id: id.into(),
        node_path: vec!["对象".into()],
        mesh_name: "mesh".into(),
        mesh_index: Some(0),
        node_index: None,
        valid: true,
        orphaned_water_binding: false, empty_source_folder: false,
    }
}

#[test]
fn placement_and_group_names_select_but_only_icons_expand() {
    use crate::ui::{scene_context_menu::{SceneObjectKey, SceneObjectRow}, terrain_objects::page_tests::{click, page_without_tree_input}};
    use super::super::{SceneTreeContents, organize::sync_root_row, scene_tree_root_row};
    use bevy::world_serialization::WorldAsset;
    use feathrise_scene_content::{ScenePlacementEditorMetadata, ScenePlacementHeightMode, ScenePlacementRecord};

    let (mut app, window) = page_without_tree_input();
    let record = |id: &str, group: &str| ScenePlacementRecord {
        editor: ScenePlacementEditorMetadata { name: id.into(), group: group.into(), ..default() }.into(),
        id: id.into(), asset_path: "gltf/scene-tree-input-fixture.glb".into(), asset_target: default(),
        mesh_lod: None, terrain_binding: None, building_preset_id: None, courtyard: None, terrain_patch: None,
        translation: [0.0; 3], height_mode: ScenePlacementHeightMode::FixedHeight, terrain_offset: 0.0,
        yaw_degrees: 0.0, scale: [1.0; 3], water_surfaces: Vec::new(),
    };
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.placement_glb_hash = Some("测试主场景".into());
        draft.motion.set_clip_name(Some("测试场景动画".into()));
        draft.placements.placements = vec![record("主模型", ""), record("分组模型", "摆放组")];
    }
    let contents = {
        let world = app.world_mut();
        let root = world.query_filtered::<Entity, With<feathrise_editor_ui::EditorThemeRoot>>().single(world).unwrap();
        for child in world.get::<Children>(root).unwrap().to_vec() {
            world.get_mut::<Node>(child).unwrap().display = Display::None;
        }
        let host = world.spawn((ChildOf(root), Node { width: px(500), flex_direction: FlexDirection::Column, ..default() })).id();
        world.spawn_scene(scene_tree_root_row()).unwrap().insert(ChildOf(host));
        let contents = world.spawn((ChildOf(host), SceneTreeContents, Node::default())).id();
        let list = world.spawn((ChildOf(contents), SceneEditorGltfBrowserList, ScrollPosition::default(),
            Node { width: px(500), overflow: Overflow::scroll_y(), ..default() })).id();
        world.spawn((ChildOf(list), SceneEditorGltfBrowserListContent, Node { width: percent(100), ..default() }));
        contents
    };
    app.init_asset::<WorldAsset>()
        .init_resource::<PlacementTreeDrag>()
        .init_resource::<crate::ui::scene_objects::SceneObjectState>()
        .init_resource::<SelectedPlacementViewportState>()
        .init_resource::<EditorViewportSelectionState>()
        .add_systems(Update, handle_gltf_browser_input.in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, super::super::refresh_gltf_browser_cache.in_set(crate::SceneEditorSystemSet::Tasks))
        .add_systems(Update, (spawn_gltf_browser_rows, sync_gltf_browser_rows, sync_root_row, super::super::sync_gltf_row_toggles)
            .chain().in_set(crate::SceneEditorSystemSet::Presentation));
    for _ in 0..5 { app.update(); }

    let row_at = |app: &mut App, index| {
        let world = app.world_mut();
        world.query::<(Entity, &SceneEditorGltfBrowserRow)>().iter(world)
            .find_map(|(entity, row)| (row.entry_index == Some(index)).then_some(entity)).unwrap()
    };
    let fold_icon = |app: &App, row, path| {
        let world = app.world();
        let caption = world.get::<Children>(row).unwrap().iter()
            .find(|&child| world.get::<GltfRowCaption>(child).is_some()).unwrap();
        let button = world.get::<Children>(caption).unwrap().iter()
            .find(|&child| world.get::<PlacementTreeExpander>(child).is_some()).unwrap();
        let glyph = world.get::<Children>(button).unwrap().iter()
            .find(|&child| world.get::<crate::components::SceneEditorGltfBrowserRowGlyph>(child).is_some()).unwrap();
        // 向图标中心发送原鼠标事件，防止控件仍留在独立的空槽。
        let image = world.get::<ImageNode>(glyph).unwrap();
        assert_eq!(image.image.path().unwrap().path(), std::path::Path::new(path));
        glyph
    };
    let revision = app.world().resource::<EditorGltfBrowserState>().published_entries_revision;
    let model = row_at(&mut app, 0);
    let group = row_at(&mut app, 1);
    click(&mut app, window, model, MouseButton::Left);
    assert_eq!(app.world().resource::<EditorGltfBrowserState>().selected_placement_id.as_deref(), Some("主模型"));
    click(&mut app, window, group, MouseButton::Left);
    {
        let state = app.world().resource::<EditorGltfBrowserState>();
        assert_eq!(state.selected_group.as_deref(), Some("摆放组"));
        assert_eq!(state.published_entries_revision, revision, "两次名称选择均保留原树投影");
        assert!(state.expanded_groups.is_empty() && state.expanded_placements.is_empty());
        assert!(state.asset_demand_counts.is_empty(), "选择模型根无需请求Scene0");
    }
    let model_icon = fold_icon(&app, model, "icons/editor/tree_glb.png");
    click(&mut app, window, model_icon, MouseButton::Left);
    let group = row_at(&mut app, 1);
    let group_icon = fold_icon(&app, group, "icons/editor/tree_folder.png");
    click(&mut app, window, group_icon, MouseButton::Left);
    {
        let state = app.world().resource::<EditorGltfBrowserState>();
        assert_eq!(state.selected_group.as_deref(), Some("摆放组"), "点击展开图标保留原选择");
        assert!(state.expanded_placements.contains("主模型") && state.expanded_groups.contains("摆放组"));
        assert_eq!(state.asset_demand_counts.get("gltf/scene-tree-input-fixture.glb"), Some(&1));
        assert!(state.entries.iter().any(|entry| entry.depth == 1 && matches!(&entry.kind,
            EditorGltfBrowserEntryKind::Placement { placement_id, collapsed: true, .. } if placement_id == "分组模型")));
    }
    let main_glb = {
        let world = app.world_mut();
        world.query::<(Entity, &SceneEditorGltfBrowserRow)>().iter(world)
            .find_map(|(entity, row)| (row.main_glb == Some(MainSceneGlbRow::File)).then_some(entity)).unwrap()
    };
    assert!(matches!(app.world().get::<SceneObjectRow>(main_glb).unwrap().0.as_ref().unwrap().key, SceneObjectKey::MainSceneGlb));
    click(&mut app, window, main_glb, MouseButton::Left);
    let scene = app.world().resource::<SceneEditorState>().selected_scene_id.clone().unwrap();
    let state = app.world().resource::<EditorGltfBrowserState>();
    assert!(state.is_main_glb_selected(&scene) && state.collapsed_main_glb.is_none());
    assert!(state.selected_group.is_none() && state.selected_placement_id.is_none());
    assert!(state.expanded_placements.contains("主模型") && state.expanded_groups.contains("摆放组"));
    let root_icon = fold_icon(&app, main_glb, "icons/editor/tree_glb.png");
    click(&mut app, window, root_icon, MouseButton::Left);
    assert_eq!(app.world().get::<Node>(contents).unwrap().display, Display::None);
    assert!(app.world().resource::<EditorGltfBrowserState>().is_main_glb_selected(&scene));
    assert_eq!(app.world().get::<Node>(main_glb).unwrap().display, Display::Flex);
    click(&mut app, window, root_icon, MouseButton::Left);
    assert_eq!(app.world().get::<Node>(contents).unwrap().display, Display::Flex);
    let model = row_at(&mut app, 0);
    click(&mut app, window, model, MouseButton::Left);
    let state = app.world().resource::<EditorGltfBrowserState>();
    assert!(state.selected_main_row.is_none());
    assert_eq!(state.selected_placement_id.as_deref(), Some("主模型"));
    let animation = {
        let world = app.world_mut();
        world.query::<(Entity, &SceneEditorGltfBrowserRow)>().iter(world)
            .find_map(|(entity, row)| (row.main_glb == Some(MainSceneGlbRow::Animation(None))).then_some(entity)).unwrap()
    };
    click(&mut app, window, animation, MouseButton::Left);
    let state = app.world().resource::<EditorGltfBrowserState>();
    assert!(matches!(&state.selected_main_row, Some((id, MainSceneSelection::Animation)) if id == &scene));
    assert_eq!(state.selected_placement_id.as_deref(), Some("主模型"), "选择动画仍保留原视口姿势目标");
    click(&mut app, window, model, MouseButton::Left);
    assert!(app.world().resource::<EditorGltfBrowserState>().selected_main_row.is_none());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn scene_tree_click_switches_root_and_mesh_rows_without_changing_the_viewport_tool() {
    for (mode, kind) in [
        (ViewportMode::Move, EditorGltfBrowserEntryKind::Placement {
            placement_id: "asset-b".into(),
            collapsed: true,
            expandable: false,
            glb: true,
        }),
        (ViewportMode::Rotate, EditorGltfBrowserEntryKind::Mesh),
        (ViewportMode::Scale, EditorGltfBrowserEntryKind::MissingWaterBinding),
    ] {
        let mut editor = SceneEditorState { open: true, ..default() };
        let mut tools = ViewportModeState::default();
        crate::viewport::placement_move::set_placement_mode(&mut editor, Some(&mut tools), mode);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(editor)
            .insert_resource(tools)
            .init_resource::<SceneEditorNumericScrubMouseGuard>()
            .init_resource::<EditorAuthoringSession>()
            .init_resource::<EditorGltfBrowserState>()
            .init_resource::<PlacementMoveState>()
            .init_resource::<SelectedPlacementViewportState>()
            .init_resource::<EditorViewportSelectionState>()
            .init_resource::<LayerCandidateState>()
            .add_systems(Update, handle_gltf_browser_input);
        app.world_mut().resource_mut::<LayerCandidateState>().selected = Some(LayerTarget::Placement("asset-a".into()));
        let old_mesh = mesh("asset-a");
        {
            let mut state = app.world_mut().resource_mut::<EditorGltfBrowserState>();
            state.selected_placement_id = Some("asset-a".into());
            state.selection = Some(old_mesh.clone());
            state
                .placement_asset_paths
                .insert("asset-b".into(), "gltf/test.glb".into());
            state.entries = vec![EditorGltfBrowserEntry {
                kind: kind.clone(),
                display_name: "B".into(),
                depth: 0,
                selection: Some(mesh("asset-b")),
            }];
        }
        let row = app
            .world_mut()
            .spawn((
                EditorButtonClick::triggered(),
                SceneEditorGltfBrowserRow {
                    slot: 0,
                    entry_index: Some(0),
                    entries_revision: 0,
                    main_glb: None,
                },
            ))
            .id();
        app.world_mut().resource_mut::<PlacementMoveState>().capture_mouse();
        app.update(); // 已经由手柄拥有的拖动不能被行接管。
        *app.world_mut().resource_mut::<PlacementMoveState>() = PlacementMoveState::default();
        app.update(); // 手势归还后不重放拖动期间的边沿。
        assert_eq!(
            app.world()
                .resource::<EditorGltfBrowserState>()
                .selected_placement_id
                .as_deref(),
            Some("asset-a")
        );
        app.world_mut().entity_mut(row).insert(EditorButtonClick::triggered());
        app.update();
        let state = app.world().resource::<EditorGltfBrowserState>();
        assert_eq!(state.selected_placement_id.as_deref(), Some("asset-b"));
        let expected_mesh = (!matches!(kind, EditorGltfBrowserEntryKind::Placement { .. })).then(|| mesh("asset-b"));
        assert_eq!(state.selection, expected_mesh);
        assert_eq!(app.world().resource::<ViewportModeState>().mode, mode);
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.placement_transform_enabled);
        assert!(!editor.dirty);
    }
}

#[test]
fn static_patch_selection_scene_tree_click_publishes_world_anchor_and_rejects_disabled_source() {
    use crate::viewport::multi_select::PlacementSelectionSet;
    use bevy::ecs::system::RunSystemOnce;
    use feathrise_scene_content::{ScenePlacementTerrainPatch, SceneTerrainDescriptor, SceneTerrainKind, SceneTerrainObject, TerrainId, TerrainObjectTransform};
    for enabled in [true, false] {
        let (mut app, _, _) = crate::viewport::placement_move::interaction_tests::app();
        app.init_resource::<PlacementSelectionSet>()
            .init_resource::<SelectedPlacementViewportState>()
            .init_resource::<EditorViewportSelectionState>();
        app.world_mut().resource_mut::<SceneEditorState>().placement_transform_enabled = false;
        let source = SceneTerrainObject {
            clipmap_stitches: default(), mesh_lod: None,
            id: TerrainId::parse("tree-selection-source").unwrap(), display_name: "场景树选择夹具".into(), enabled,
            transform: TerrainObjectTransform { translation: [31.0, 8.0, -23.0], yaw_degrees: 37.0,
                pitch_degrees: 17.0, roll_degrees: -11.0, scale: [2.0, 3.0, 4.0] },
            world_min: [0.0; 2], world_max: [64.0; 2],
            terrain: SceneTerrainDescriptor { kind: SceneTerrainKind::SparseClipmap, ..SceneTerrainDescriptor::empty() },
            voxel: None, surface_path: None, vegetation_path: None, water_path: None, puddles: None,
        };
        let (id, world) = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            let record = &mut draft.placements.placements[0];
            record.water_surfaces.clear();
            record.terrain_patch = Some(ScenePlacementTerrainPatch { terrain_id: Some(source.id.clone()), ..default() });
            let world = source.transform.as_transform().transform_point(Vec3::from_array(record.translation));
            let id = record.id.clone();
            draft.manifest.terrains = Some(vec![source].into());
            (id, world)
        };
        {
            let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
            browser.placement_asset_paths.insert(id.clone(), "gltf/test.glb".into());
            browser.entries = vec![EditorGltfBrowserEntry { kind: EditorGltfBrowserEntryKind::Placement {
                placement_id: id.clone(), collapsed: true, expandable: false, glb: true,
            }, display_name: "三维地形".into(), depth: 0, selection: None }];
        }
        app.world_mut().spawn((EditorButtonClick::triggered(), SceneEditorGltfBrowserRow { slot: 0, entry_index: Some(0), entries_revision: 0, main_glb: None }));
        app.world_mut().run_system_once(handle_gltf_browser_input).unwrap();
        let selection = app.world().resource::<PlacementSelectionSet>();
        let browser = app.world().resource::<EditorGltfBrowserState>();
        if enabled {
            assert_eq!(selection.members.len(), 1);
            assert!(selection.members[0].position.abs_diff_eq(world, 0.0001));
            assert_eq!(browser.selected_placement_id.as_deref(), Some(id.as_str()));
        } else {
            assert!(selection.members.is_empty());
            assert!(browser.selected_placement_id.is_none(), "禁用来源不能通过真实场景树点击重新取得编辑资格");
        }
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}
