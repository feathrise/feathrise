//! 独立三维地形仍保存源局部记录；选择发布、框选和局部刷新必须使用实际世界原点。

use super::*;
use crate::authoring::EditorAuthoringSession;
use crate::ui::gltf_browser::EditorGltfBrowserState;
use crate::resources::{SceneEditorState, SceneEditorViewportState};
use crate::viewport::modes::{ViewportMode, ViewportModeState};
use crate::viewport::placement_move::interaction_tests::{app, camera};
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use feathrise_scene_content::{ScenePlacementTerrainPatch, SceneTerrainDescriptor, SceneTerrainKind, SceneTerrainObject, TerrainId, TerrainObjectTransform};

fn patch_app() -> (App, Entity, Vec3) {
    let (mut app, _, node) = app();
    let source = SceneTerrainObject {
        clipmap_stitches: default(), mesh_lod: None,
        id: TerrainId::parse("selection-source").unwrap(), display_name: "选择框架夹具".into(), enabled: true,
        transform: TerrainObjectTransform {
            translation: [23.0, 7.0, -19.0], yaw_degrees: 41.0,
            pitch_degrees: 13.0, roll_degrees: -9.0, scale: [2.0, 3.0, 4.0],
        },
        world_min: [0.0; 2], world_max: [64.0; 2],
        terrain: SceneTerrainDescriptor { kind: SceneTerrainKind::SparseClipmap, ..SceneTerrainDescriptor::empty() },
        voxel: None, surface_path: None, vegetation_path: None, water_path: None, puddles: None,
    };
    let world = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let record = &mut draft.placements.placements[0];
        record.water_surfaces.clear();
        record.terrain_patch = Some(ScenePlacementTerrainPatch { terrain_id: Some(source.id.clone()), ..default() });
        assert!(record.terrain_binding.is_none()); // 不能靠普通内容绑定掩盖三维地形的专属父空间。
        let world = source.transform.as_transform().transform_point(Vec3::from_array(record.translation));
        draft.manifest.terrains = Some(vec![source].into());
        draft.placement_water_surface_count = 0;
        world
    };
    app.init_resource::<PlacementSelectionSet>();
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    (app, node, world)
}

fn selection_step(app: &mut App, node: Entity, cursor: Vec2) {
    app.world_mut().get_mut::<bevy::ui::RelativeCursorPosition>(node).unwrap().normalized = Some(cursor);
    app.world_mut().run_system_once(handle_multi_select_pointer).unwrap();
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
}

#[test]
fn static_patch_selection_rectangle_projects_world_origin_and_mixes_regular_instances() {
    let (mut app, node, world) = patch_app();
    let regular_world = world + Vec3::new(4.0, -2.0, 0.0);
    let (regular_id, structure) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut regular = draft.placements.placements[0].clone();
        regular.id = "regular-selection-neighbor".into();
        regular.terrain_patch = None;
        regular.translation = regular_world.to_array();
        let id = regular.id.clone();
        draft.placements.placements.push(regular);
        (id, session.placement_structure_revision)
    };
    {
        let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
        browser.select_indexed_test_placement(&regular_id, 1, structure);
        browser.selected_placement_id = None;
    }
    app.world_mut().resource_mut::<SceneEditorState>().placement_transform_enabled = false;
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = (world + regular_world) * 0.5;
    let camera = camera(&app);
    let projected = camera.project(world).unwrap();
    let neighbor = camera.project(regular_world).unwrap();
    let min = projected.min(neighbor) - Vec2::splat(0.02);
    let max = projected.max(neighbor) + Vec2::splat(0.02);
    let local = camera.project(Vec3::new(5.0, 4.0, 5.0));
    assert!(local.is_none_or(|p| !p.cmpge(min).all() || !p.cmple(max).all()), "负对照局部原点不得落入真实世界框");
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    selection_step(&mut app, node, min);
    selection_step(&mut app, node, max);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    selection_step(&mut app, node, max);
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert_eq!(selection.members.len(), 2, "框住真实世界原点应同时选中独立三维地形与普通资产");
    assert!(selection.members[0].position.abs_diff_eq(world, 0.0001));
    assert!(selection.members[1].position.abs_diff_eq(regular_world, 0.0001));
    assert!(selection.center.abs_diff_eq((world + regular_world) * 0.5, 0.0001));
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}

#[test]
fn static_patch_selection_refreshes_world_anchor_when_only_terrain_revision_changes() {
    let (mut app, _, before) = patch_app();
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    let offset = Vec3::new(7.0, -3.0, 11.0);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let source = &mut std::sync::Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap())[0];
        source.transform.translation = (Vec3::from_array(source.transform.translation) + offset).to_array();
        session.terrain_revision += 1; // 单独来源身份也须使缓存失效，不能依赖碰巧同时变化的全局修订。
    }
    assert!(!app.world().resource::<PlacementSelectionSet>().ready_for(app.world().resource::<EditorAuthoringSession>()));
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert!(selection.ready_for(app.world().resource::<EditorAuthoringSession>()));
    assert!(selection.center.abs_diff_eq(before + offset, 0.0001));
}

#[test]
fn static_patch_selection_missing_or_disabled_source_revokes_stale_primary_and_membership() {
    for initially_selected in [false, true] {
        let (mut app, _, _) = patch_app();
        if initially_selected {
            app.world_mut().run_system_once(advance_multi_selection).unwrap();
        }
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            std::sync::Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap())[0].enabled = false;
            session.terrain_revision += 1;
        }
        app.world_mut().run_system_once(advance_multi_selection).unwrap();
        assert!(app.world().resource::<PlacementSelectionSet>().members.is_empty());
        assert!(app.world().resource::<EditorGltfBrowserState>().selected_placement_id.is_none(), "失效源不能留下可操作的旧主选择");
    }
}

#[test]
fn static_patch_selection_browser_sync_uses_world_origin_instead_of_source_local_record() {
    let (mut app, _, world) = patch_app();
    let selected = app.world().resource::<EditorGltfBrowserState>().selected_placement_id.clone();
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert_eq!(selection.members.len(), 1);
    assert_eq!(Some(&selection.members[0].id), selected.as_ref());
    assert!(selection.members[0].position.abs_diff_eq(world, 0.0001), "浏览器恢复选择应发布实际世界原点");
    assert!(selection.center.abs_diff_eq(world, 0.0001));
}

#[test]
fn static_patch_selection_additive_rectangle_does_not_certify_old_anchors_during_bounded_refresh() {
    let (mut app, node, old_world) = patch_app();
    let target_world = old_world + Vec3::new(-4.0, 2.0, 0.0);
    let (members, indexed, structure) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let first = draft.placements.placements[0].clone();
        let mut members = vec![SelectedMember { id: first.id.clone(), index: 0, position: old_world }];
        for index in 1..=64 {
            let mut record = first.clone();
            record.id = format!("rectangle-refresh-{index}");
            record.terrain_patch = None;
            record.translation = (if index == 64 { target_world } else { old_world + Vec3::new(1000.0 + index as f32 * 5.0, 0.0, 0.0) }).to_array();
            if index < 64 {
                members.push(SelectedMember { id: record.id.clone(), index, position: Vec3::from_array(record.translation) });
            }
            draft.placements.placements.push(record);
        }
        let indexed = draft.placements.placements.iter().enumerate().map(|(index, record)| (record.id.clone(), index)).collect::<Vec<_>>();
        (members, indexed, session.placement_structure_revision)
    };
    {
        let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
        for (id, index) in indexed { browser.select_indexed_test_placement(&id, index, structure); }
        browser.selected_placement_id = None;
    }
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        selection.publish(members, world.resource::<EditorAuthoringSession>());
    });
    let offset = Vec3::new(20.0, -3.0, 11.0);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let source = &mut std::sync::Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap())[0];
        source.transform.translation = (Vec3::from_array(source.transform.translation) + offset).to_array();
        session.terrain_revision += 1;
    }
    app.world_mut().resource_mut::<SceneEditorState>().placement_transform_enabled = false;
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = target_world;
    let point = camera(&app).project(target_world).unwrap();
    let min = point - Vec2::splat(0.025);
    let max = point + Vec2::splat(0.025);
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::ShiftLeft);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    selection_step(&mut app, node, min); // 第一帧只复核16项，不同步刷新整个64成员集合。
    selection_step(&mut app, node, max); // 第二帧拖框，旧集合仍未完整复核。
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    selection_step(&mut app, node, max);
    {
        let selection = app.world().resource::<PlacementSelectionSet>();
        assert_eq!(selection.members.len(), 64, "未完成复核时拒绝追加，不能把新目标混入旧世界锚点集合");
        assert!(!selection.ready_for(app.world().resource::<EditorAuthoringSession>()), "本次框选不能给旧锚点盖上新地形修订");
        assert!(selection.members[0].position.abs_diff_eq(old_world, 0.0001));
    }
    for _ in 0..8 {
        selection_step(&mut app, node, max);
        if app.world().resource::<PlacementSelectionSet>().ready_for(app.world().resource::<EditorAuthoringSession>()) { break; }
    }
    let selection = app.world().resource::<PlacementSelectionSet>();
    assert_eq!(selection.members.len(), 64);
    assert!(selection.ready_for(app.world().resource::<EditorAuthoringSession>()));
    assert!(selection.members[0].position.abs_diff_eq(old_world + offset, 0.0001));
}
