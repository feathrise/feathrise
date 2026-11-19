//! 场景树真实输入回归，点击根行和网格可以直接更换变换目标。

use super::super::{EditorGltfBrowserEntry, EditorGltfMeshSelection};
use super::*;
use crate::viewport::{LayerCandidateState, LayerTarget, modes::{ViewportMode, ViewportModeState}};

fn mesh(id: &str) -> EditorGltfMeshSelection {
    EditorGltfMeshSelection {
        placement_id: id.into(),
        node_path: vec!["对象".into()],
        mesh_name: "mesh".into(),
        mesh_index: Some(0),
        valid: true,
        orphaned_water_binding: false,
    }
}

#[test]
fn scene_tree_click_switches_root_and_mesh_rows_without_changing_the_viewport_tool() {
    for (mode, kind) in [
        (ViewportMode::Move, EditorGltfBrowserEntryKind::Placement {
            placement_id: "asset-b".into(),
            collapsed: true,
            expandable: false,
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
                placement_id: id.clone(), collapsed: true, expandable: false,
            }, display_name: "三维地形".into(), depth: 0, selection: None }];
        }
        app.world_mut().spawn((EditorButtonClick::triggered(), SceneEditorGltfBrowserRow { slot: 0, entry_index: Some(0), entries_revision: 0 }));
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
