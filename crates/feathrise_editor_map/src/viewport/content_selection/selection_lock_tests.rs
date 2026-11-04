//! 变换模式的选择锁：同帧迟到点击/空命中、后台换选及候选UI不能改变正式操作目标。

use super::*;
use crate::authoring::{
    BoundaryEditState, EditorAuthoringSession,
    test_heightfield_draft,
};
use crate::game_authoring::map_interaction::MapInteractionState;
use crate::resources::{EditorViewportSelectionState, MapEditorState};
use crate::ui::{gltf_browser::EditorGltfBrowserState, sidebars::MapEditorSidebarState};
use crate::viewport::{
    SelectedPlacementViewportState, SelectedPortalViewportState, SelectedWaterViewportState,
    modes::ViewportMode,
};
use bevy::asset::AssetPlugin;
use std::sync::Arc;

#[path = "derived_click_tests.rs"]
mod derived_click_tests; // 真实贴地采用后的旧点击失效与已确认精确网格保留。

fn app() -> App {
    let draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    let map = draft.map_id.clone();
    let hash = draft.adopted_domain_hash().to_owned();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .insert_resource(MapEditorState {
            open: true,
            selected_map_id: Some(map.clone()),
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 7,
            ..default()
        })
        .init_resource::<MapInteractionState>()
        .init_resource::<ViewportModeState>()
        .init_resource::<MapEditorSidebarState>()
        .init_resource::<EditorViewportSelectionState>()
        .init_resource::<EditorGltfBrowserState>()
        .init_resource::<SelectedPlacementViewportState>()
        .init_resource::<SelectedWaterViewportState>()
        .init_resource::<SelectedPortalViewportState>()
        .init_resource::<BoundaryEditState>()
        .insert_resource(LayerCandidateState {
            selected: Some(LayerTarget::Placement("asset-a".into())),
            selected_author_index: Some(0),
            identity: Some(Identity {
                map,
                revision: 7,
                placement_derived_pose_revision: 0,
                domain_hash: hash,
                generation: 0,
                ticket: 0,
                filter: SelectionFilter::All,
            }),
            candidates: vec![candidate("asset-a", 0), candidate("asset-b", 1)],
            ..default()
        });
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .select_indexed_test_placement("asset-a", 0, 0);
    app
}

fn candidate(id: &str, index: usize) -> LayerCandidate {
    LayerCandidate {
        target: LayerTarget::Placement(id.into()),
        author_index: Some(index),
        label: id.into(),
        rank: 0,
        distance: index as f32,
    }
}

#[test]
fn multi_shift_candidate_add_remove_empty_and_transform_lock_share_formal_selection() {
    use crate::viewport::multi_select::{PlacementSelectionSet, SelectedMember};
    use feathrise_map_content::{MapPlacementHeightMode, MapPlacementRecord};
    let mut app = app();
    app.init_resource::<PlacementSelectionSet>()
        .add_systems(Update, super::super::layer_apply::apply_layer_candidate);
    for (index, id) in ["asset-a", "asset-b"].into_iter().enumerate() {
        let record = MapPlacementRecord { mesh_lod: None, terrain_binding: None, editor: Default::default(), id: id.into(), asset_path: "gltf/test.glb".into(), asset_target: default(),
            building_preset_id: None, courtyard: None, terrain_patch: None, translation: [5.0 + index as f32, 2.0, 5.0],
            height_mode: MapPlacementHeightMode::FixedHeight, terrain_offset: 0.0, yaw_degrees: 0.0,
            scale: [1.0; 3], water_surfaces: vec![] };
        app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().placements.placements.push(record);
        app.world_mut().resource_mut::<EditorGltfBrowserState>().select_indexed_test_placement(id, index, 0);
    }
    app.world_mut().resource_mut::<EditorGltfBrowserState>().selected_placement_id = Some("asset-a".into());
    {
        let mut sidebar = app.world_mut().resource_mut::<MapEditorSidebarState>();
        sidebar.left = crate::ui::sidebars::EditorLeftSidebarTab::Environment;
        sidebar.right = crate::ui::sidebars::EditorRightSidebarTab::Tools;
    }
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        selection.publish(vec![SelectedMember { id: "asset-a".into(), index: 0, position: Vec3::new(5.0, 2.0, 5.0) }], world.resource::<EditorAuthoringSession>());
    });
    {
        let mut state = app.world_mut().resource_mut::<LayerCandidateState>();
        state.apply = Some(LayerTarget::Placement("asset-b".into())); state.additive = true;
    }
    app.update();
    assert_eq!(app.world().resource::<PlacementSelectionSet>().members.len(), 2);
    assert!(app.world().resource::<EditorGltfBrowserState>().selected_placement_id.is_none());
    assert_eq!(app.world().resource::<MapEditorSidebarState>().right, crate::ui::sidebars::EditorRightSidebarTab::Scene);
    assert_eq!(app.world().resource::<MapEditorSidebarState>().left, crate::ui::sidebars::EditorLeftSidebarTab::Environment);
    app.world_mut().resource_mut::<LayerCandidateState>().clear_objects = true;
    app.update();
    assert_eq!(app.world().resource::<PlacementSelectionSet>().members.len(), 2);
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Rotate;
    app.world_mut().resource_mut::<LayerCandidateState>().apply = Some(LayerTarget::Placement("asset-b".into()));
    app.update();
    assert_eq!(app.world().resource::<PlacementSelectionSet>().members.len(), 2);
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    app.world_mut().resource_mut::<LayerCandidateState>().apply = Some(LayerTarget::Placement("asset-b".into()));
    app.update();
    assert_eq!(app.world().resource::<PlacementSelectionSet>().members.len(), 1);
    assert_eq!(app.world().resource::<EditorGltfBrowserState>().selected_placement_id.as_deref(), Some("asset-a"));
    assert_eq!(app.world().resource::<LayerCandidateState>().selected, Some(LayerTarget::Placement("asset-a".into())));
    assert!(!app.world().resource::<MapEditorState>().dirty);
}

#[test]
fn static_patch_selection_candidate_uses_world_anchor_and_rejects_missing_source() {
    use crate::viewport::multi_select::PlacementSelectionSet;
    use feathrise_map_content::{MapPlacementTerrainPatch, MapTerrainDescriptor, MapTerrainKind, MapTerrainObject, TerrainId, TerrainObjectTransform};
    for valid_source in [true, false] {
        let mut app = app();
        app.init_resource::<PlacementSelectionSet>()
            .add_systems(Update, super::super::layer_apply::apply_layer_candidate);
        let source = MapTerrainObject {
            clipmap_stitches: default(), mesh_lod: None,
            id: TerrainId::parse("candidate-selection-source").unwrap(), display_name: "候选选择夹具".into(), enabled: true,
            transform: TerrainObjectTransform { translation: [29.0, 5.0, -13.0], yaw_degrees: 61.0,
                pitch_degrees: 19.0, roll_degrees: -7.0, scale: [2.0, 3.0, 4.0] },
            world_min: [0.0; 2], world_max: [64.0; 2],
            terrain: MapTerrainDescriptor { kind: MapTerrainKind::SparseClipmap, ..MapTerrainDescriptor::empty() },
            voxel: None, surface_path: None, vegetation_path: None, water_path: None, puddles: None,
        };
        let mut record = crate::viewport::placement_move::interaction_tests::record();
        record.id = "asset-a".into();
        record.water_surfaces.clear();
        record.terrain_patch = Some(MapPlacementTerrainPatch { terrain_id: Some(source.id.clone()), ..default() });
        let world = source.transform.as_transform().transform_point(Vec3::from_array(record.translation));
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            draft.placements.placements.push(record);
            draft.manifest.terrains = Some(if valid_source { vec![source].into() } else { Vec::new().into() });
        }
        app.world_mut().resource_mut::<LayerCandidateState>().apply = Some(LayerTarget::Placement("asset-a".into()));
        app.update();
        let selection = app.world().resource::<PlacementSelectionSet>();
        let browser = app.world().resource::<EditorGltfBrowserState>();
        if valid_source {
            assert_eq!(selection.members.len(), 1);
            assert!(selection.members[0].position.abs_diff_eq(world, 0.0001));
            assert_eq!(browser.selected_placement_id.as_deref(), Some("asset-a"));
        } else {
            assert!(selection.members.is_empty());
            assert!(browser.selected_placement_id.is_none());
            assert!(app.world().resource::<LayerCandidateState>().selected.is_none());
        }
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}

#[test]
fn static_patch_selection_additive_candidate_does_not_republish_stale_source_anchor() {
    use crate::viewport::multi_select::{PlacementSelectionSet, SelectedMember};
    use feathrise_map_content::{MapPlacementTerrainPatch, MapTerrainDescriptor, MapTerrainKind, MapTerrainObject, TerrainId, TerrainObjectTransform};
    let mut app = app();
    app.init_resource::<PlacementSelectionSet>()
        .add_systems(Update, super::super::layer_apply::apply_layer_candidate);
    let source = MapTerrainObject {
        clipmap_stitches: default(), mesh_lod: None,
        id: TerrainId::parse("additive-selection-source").unwrap(), display_name: "追加选择夹具".into(), enabled: true,
        transform: TerrainObjectTransform { translation: [29.0, 5.0, -13.0], yaw_degrees: 61.0,
            pitch_degrees: 19.0, roll_degrees: -7.0, scale: [2.0, 3.0, 4.0] },
        world_min: [0.0; 2], world_max: [64.0; 2],
        terrain: MapTerrainDescriptor { kind: MapTerrainKind::SparseClipmap, ..MapTerrainDescriptor::empty() },
        voxel: None, surface_path: None, vegetation_path: None, water_path: None, puddles: None,
    };
    let old_world = {
        let mut record = crate::viewport::placement_move::interaction_tests::record();
        record.id = "asset-a".into(); record.water_surfaces.clear();
        record.terrain_patch = Some(MapPlacementTerrainPatch { terrain_id: Some(source.id.clone()), ..default() });
        let old_world = source.transform.as_transform().transform_point(Vec3::from_array(record.translation));
        let mut regular = record.clone(); regular.id = "asset-b".into(); regular.terrain_patch = None;
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.placements.placements = vec![record, regular];
        draft.manifest.terrains = Some(vec![source].into());
        old_world
    };
    {
        let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
        browser.select_indexed_test_placement("asset-b", 1, 0);
        browser.selected_placement_id = Some("asset-a".into());
    }
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        selection.publish(vec![SelectedMember { id: "asset-a".into(), index: 0, position: old_world }], world.resource::<EditorAuthoringSession>());
    });
    let old_members = app.world().resource::<PlacementSelectionSet>().members.clone();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        std::sync::Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap())[0].transform.translation[0] += 10.0;
        session.terrain_revision += 1;
    }
    {
        let mut candidates = app.world_mut().resource_mut::<LayerCandidateState>();
        candidates.apply = Some(LayerTarget::Placement("asset-b".into()));
        candidates.additive = true;
    }
    app.update(); // 实际候选采用先于多选分帧复核，覆盖可发生的同帧输入窗口。
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert!(Arc::ptr_eq(&selection.members, &old_members), "未完成复核不能通过追加点击重新发布旧锚点");
    assert!(!selection.ready_for(app.world().resource::<EditorAuthoringSession>()));
    assert_eq!(app.world().resource::<EditorGltfBrowserState>().selected_placement_id.as_deref(), Some("asset-a"));
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}

#[test]
fn author_object_selection_reveals_properties_without_changing_the_creation_module() {
    use crate::ui::sidebars::{EditorInspectorTarget, EditorLeftSidebarTab, EditorRightSidebarTab};
    use feathrise_map_content::{MapBoundariesAsset, MapPortalRecord, MapPortalTarget, MapSurfaceAsset, MapSurfaceLayer};
    for (module, page) in [(EditorLeftSidebarTab::Environment, EditorRightSidebarTab::Environment),
        (EditorLeftSidebarTab::Water, EditorRightSidebarTab::Tools),
        (EditorLeftSidebarTab::Terrain, EditorRightSidebarTab::Assets)] {
        for kind in 0..3 {
            let mut app = app();
            let target = {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                let draft = session.draft.as_mut().unwrap();
                match kind {
                    0 => {
                        draft.surface = Some(MapSurfaceAsset { schema_version: 1, map_id: draft.map_id.clone(),
                            resolution: [1, 1], weight_source_path: "weights.rgba8".into(), weight_texture_path: "weights.ktx2".into(),
                            index_source_path: "indices.rgba32".into(), index_texture_path: "indices.ktx2".into(),
                            layers: vec![MapSurfaceLayer { id: "soil".into(), label: "地表".into(), preset_id: "soil".into() }] });
                        LayerTarget::Surface("soil".into())
                    }
                    1 => {
                        draft.portals.portals.push(MapPortalRecord { terrain_binding: None, arrival_terrain_binding: None,
                            id: "portal".into(), label: "传送点".into(), label_customized: true,
                            position: [1.0, 0.0, 1.0], arrival: [2.0, 0.0, 2.0],
                            target: MapPortalTarget::default_spawn(draft.map_id.clone()), color_srgba: [1.0; 4], arrow_degrees: 0.0 });
                        LayerTarget::Portal("portal".into())
                    }
                    _ => {
                        draft.boundaries = Some(MapBoundariesAsset::rectangular_default(&draft.map_id, [0.0; 2], [10.0; 2], 0.0, 20.0).unwrap());
                        LayerTarget::Boundary(draft.boundaries.as_ref().unwrap().paths[0].id.clone())
                    }
                }
            };
            {
                let mut sidebar = app.world_mut().resource_mut::<MapEditorSidebarState>();
                sidebar.reveal_asset_properties(); sidebar.left = module; sidebar.right = page;
            }
            app.world_mut().resource_mut::<MapEditorState>().selected_asset = Some("gltf/仍选中的文件.glb".into());
            {
                let mut state = app.world_mut().resource_mut::<LayerCandidateState>();
                state.selected_author_index = Some(0); state.apply = Some(target.clone());
            }
            app.add_systems(Update, super::super::layer_apply::apply_layer_candidate);
            app.update();
            let sidebar = app.world().resource::<MapEditorSidebarState>();
            assert_eq!(sidebar.right, EditorRightSidebarTab::Scene);
            assert_eq!(sidebar.inspector, EditorInspectorTarget::Scene);
            assert_eq!(sidebar.left, module, "选择对象不能改名顶部动态工具页签");
            assert_eq!(app.world().resource::<LayerCandidateState>().selected, Some(target.clone()));
            assert_eq!(app.world().resource::<MapEditorState>().tool, crate::types::EditorTool::Inspect);
            assert!(!app.world().resource::<MapEditorState>().dirty);
            app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1;
            app.world_mut().resource_scope(|world, mut state: Mut<LayerCandidateState>| {
                state.rebind_author_revision(world.resource::<MapEditorState>(), world.resource::<EditorAuthoringSession>(),
                    None, SelectionFilter::All);
                assert!(state.selected_matches_session(world.resource::<EditorAuthoringSession>()));
            }); // 属性提交后的作者修订仍续接同一对象，不能把表单移回隐藏工具宿主。
            assert_eq!(app.world().resource::<LayerCandidateState>().selected_author_index, Some(0));
            app.world_mut().resource_mut::<MapEditorSidebarState>().reveal_asset_properties();
            app.update(); // 稳定选中及后台作者修订没有新的用户点选，不能夺回刚打开的文件属性。
            assert_eq!(app.world().resource::<MapEditorSidebarState>().inspector, EditorInspectorTarget::Asset);
            app.world_mut().resource_mut::<LayerCandidateState>().apply = Some(target);
            app.update(); // 再点同一个对象也必须显式恢复对象属性，不能只观察选中ID是否变化。
            {
                let mut sidebar = app.world_mut().resource_mut::<MapEditorSidebarState>();
                assert_eq!(sidebar.inspector, EditorInspectorTarget::Scene);
                sidebar.right = EditorRightSidebarTab::Tools;
                sidebar.reveal_properties();
                assert_eq!(sidebar.right, EditorRightSidebarTab::Scene);
            }
            assert_eq!(app.world().resource::<MapEditorState>().selected_asset.as_deref(), Some("gltf/仍选中的文件.glb"));
        }
    }
}

#[test]
fn selection_lock_rejects_pending_selection_and_empty_hits_before_poll() {
    for mode in [
        ViewportMode::Move,
        ViewportMode::Rotate,
        ViewportMode::Scale,
    ] {
        for empty_hit in [false, true] {
            let mut app = app();
            app.add_systems(Update, super::super::layer_apply::apply_layer_candidate);
            app.world_mut().resource_mut::<ViewportModeState>().mode = mode;
            {
                let mut state = app.world_mut().resource_mut::<LayerCandidateState>();
                state.apply = Some(LayerTarget::Placement("asset-b".into()));
                state.hover = state.apply.clone();
                state.clear_objects = empty_hit;
            }
            app.update();
            let state = app.world().resource::<LayerCandidateState>();
            assert_eq!(
                state.selected,
                Some(LayerTarget::Placement("asset-a".into()))
            );
            assert!(state.apply.is_none() && state.hover.is_none() && !state.clear_objects);
            assert_eq!(
                app.world()
                    .resource::<EditorGltfBrowserState>()
                    .selected_placement_id
                    .as_deref(),
                Some("asset-a")
            );
            assert!(!app.world().resource::<MapEditorState>().dirty);
        }
    }
}

#[test]
fn selection_lock_discards_real_worker_results_even_after_returning_to_select() {
    use bevy::tasks::AsyncComputeTaskPool;
    use std::time::{Duration, Instant};
    type Result = (Identity, Request, Vec<LayerCandidate>);
    for empty_hit in [false, true] {
        let mut app = app();
        app.add_systems(Update, super::super::layer_query::poll_layer_candidates);
        let slot = Arc::new(std::sync::Mutex::new((None::<Result>, None::<std::task::Waker>)));
        let mut state = app.world_mut().resource_mut::<LayerCandidateState>();
        let key = state.identity.clone().unwrap();
        state.request(
            key,
            Ray3d::new(Vec3::Y, Dir3::NEG_Y),
            Some(Vec3::ZERO),
            Vec2::ONE,
            Vec2::splat(100.0),
            1.0,
            None,
        );
        let request = state.request.take().unwrap();
        let key = request.identity.clone();
        let work = slot.clone();
        state.task = Some(AsyncComputeTaskPool::get().spawn(std::future::poll_fn(move |cx| {
            let mut work = work.lock().unwrap();
            if let Some(result) = work.0.take() {
                std::task::Poll::Ready(result)
            } else {
                work.1 = Some(cx.waker().clone());
                std::task::Poll::Pending
            }
        })));
        drop(state);
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Move;
        app.update();
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
        app.update();
        assert!(app.world().resource::<LayerCandidateState>().task.is_some());
        // 先完成“进入变换再返回选择”，才放行真实任务；不靠固定延迟猜测线程池调度顺序。
        let wake = {
            let mut work = slot.lock().unwrap();
            work.0 = Some((key, request, if empty_hit { vec![] } else { vec![candidate("asset-b", 1)] }));
            work.1.take()
        };
        if let Some(wake) = wake {
            wake.wake();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.world().resource::<LayerCandidateState>().task.is_some() {
            app.update();
            assert!(Instant::now() < deadline, "选择锁回归的后台结果未在期限内完成");
            std::thread::sleep(Duration::from_millis(1));
        }
        let state = app.world().resource::<LayerCandidateState>();
        assert!(state.task.is_none());
        assert_eq!(
            state.selected,
            Some(LayerTarget::Placement("asset-a".into()))
        );
        assert!(state.apply.is_none() && !state.clear_objects);
        assert_eq!(state.candidates.len(), 2);
    }
}

#[test]
fn selection_lock_hides_candidate_ui_and_restores_it_without_rebuilding() {
    let mut app = app();
    app.init_asset::<Font>()
        .add_systems(Update, super::super::layer_ui::sync_layer_candidate_ui);
    let slot = app
        .world_mut()
        .spawn((super::super::layer_ui::LayerCandidateSlot, Node::default()))
        .id();
    app.world_mut()
        .resource_mut::<LayerCandidateState>()
        .ui_revision = 1;
    app.update();
    let children = app
        .world()
        .get::<Children>(slot)
        .unwrap()
        .iter()
        .collect::<Vec<_>>();
    for mode in [
        ViewportMode::Move,
        ViewportMode::Rotate,
        ViewportMode::Scale,
        ViewportMode::Select,
    ] {
        app.world_mut().resource_mut::<ViewportModeState>().mode = mode;
        app.update();
        assert_eq!(
            app.world().get::<Node>(slot).unwrap().display,
            if mode == ViewportMode::Select {
                Display::Flex
            } else {
                Display::None
            }
        );
        assert_eq!(
            app.world()
                .get::<Children>(slot)
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            children
        );
        assert_eq!(
            app.world().resource::<LayerCandidateState>().selected,
            Some(LayerTarget::Placement("asset-a".into()))
        );
    }
}

#[test]
fn selection_lock_still_invalidates_selection_when_the_map_changes() {
    let mut app = app();
    app.add_systems(Update, super::super::layer_query::poll_layer_candidates);
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Rotate;
    app.update();
    assert!(
        app.world()
            .resource::<LayerCandidateState>()
            .selected
            .is_some()
    );
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = Some(MapId::parse("MAP-another").unwrap());
    app.update();
    assert!(
        app.world()
            .resource::<LayerCandidateState>()
            .selected
            .is_none()
    );
}
