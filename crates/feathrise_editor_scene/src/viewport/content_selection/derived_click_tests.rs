//! 真实贴地采用淘汰旧异步点击与轮选候选，同时保留已确认精确网格。
//! 命中从生产append入口交付，后续使用真实请求、worker和采用；不模拟窗口射线或GLB加载。

use feathrise_editor_ui::UiInteraction;
use super::*;
use bevy::ecs::system::RunSystemOnce;
use crate::authoring::{PlacementTerrainFollowState, TerrainBrushStroke, WaterBrushStroke};
use crate::ui::gltf_browser::EditorGltfMeshSelection;
use crate::ui::placement_water::{PlacementInstanceIndex, register_placement_instance_root, unregister_placement_instance_root};
use crate::viewport::content_selection::{layer_apply::apply_layer_candidate, layer_query::poll_layer_candidates,
    layer_ui::{CandidateAction, layer_candidate_interactions},
    placement::{PlacementViewportHit, append_placement_candidate}};
use feathrise_scene_content::ScenePlacementHeightMode;
use feathrise_scene::ScenePlacementInstance;
use std::time::{Duration, Instant};

fn precise(id: &str) -> EditorGltfMeshSelection {
    EditorGltfMeshSelection { placement_id: id.into(), node_path: vec!["模型".into(), "叶片".into()],
        mesh_name: "叶片网格".into(), mesh_index: Some(1), valid: true, orphaned_water_binding: false }
}

/// 冻结本次输入的稳定网格命中；任务结果由真实poll计算，不能手填已完成的Task。
fn queue_click(app: &mut App, ids: &[&str], pixel: Vec2) {
    let session = app.world().resource::<EditorAuthoringSession>();
    let records = &session.draft.as_ref().unwrap().placements.placements;
    let ray_y = ids.first().and_then(|id| records.iter().find(|record| record.id == *id))
        .map_or(9.0, |record| record.translation[1]); // 新点击随真实贴地后的姿态重新取输入，不复用旧Y=9射线。
    let mut candidates = Vec::new();
    let mut hits = std::collections::HashMap::new();
    for id in ids {
        let distance = records.iter().find(|record| record.id == *id).unwrap().translation[0];
        append_placement_candidate(PlacementViewportHit::SceneMesh(precise(id)), distance,
            &mut candidates, &mut hits);
    }
    let draft = session.draft.as_ref().unwrap();
    let ground = draft.terrain_ground.clone().filter(|ground| ground.revision == session.terrain_revision);
    let objects = draft.manifest.terrains.clone().unwrap_or_default();
    let key = Identity {
        scene_id: draft.scene_id.clone(), revision: session.revision,
        placement_derived_pose_revision: session.placement_derived_pose_revision,
        domain_hash: draft.adopted_domain_hash().to_owned(),
        generation: app.world().resource::<SceneInteractionState>().observed_generation,
        ticket: 0,
        filter: app.world().resource::<ViewportModeState>()
            .selection_filter(app.world().resource::<SceneEditorState>()),
    }; // 与生产输入入口一样冻结当前元数据，不能给旧请求重盖新派生代次。
    let mut state = app.world_mut().resource_mut::<LayerCandidateState>();
    state.request(key, Ray3d::new(Vec3::new(0.0, ray_y, 5.0), Dir3::X), None,
        pixel, Vec2::splat(128.0), 1.0, None);
    let request = state.request.as_mut().unwrap();
    request.terrain_ground = ground;
    request.terrain_objects = objects;
    request.placement_candidates = candidates;
    request.placement_hits = hits;
}

fn finish_query(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        let state = app.world().resource::<LayerCandidateState>();
        if state.task.is_none() && state.request.is_none() { break; }
        assert!(Instant::now() < deadline, "真实候选worker未在期限内归还单槽");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn fixture() -> (App, Vec<Entity>, EditorGltfMeshSelection, u64, Option<usize>) {
    fixture_with_canvas(false)
}

fn fixture_with_canvas(with_canvas: bool) -> (App, Vec<Entity>, EditorGltfMeshSelection, u64, Option<usize>) {
    let mut app = super::app();
    app.add_plugins(bevy::transform::TransformPlugin)
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_resource::<PlacementInstanceIndex>()
        .init_resource::<TerrainBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<PlacementTerrainFollowState>()
        .add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root)
        .add_systems(Update, (poll_layer_candidates, apply_layer_candidate).chain());
    if with_canvas {
        app.init_resource::<crate::ui::terrain_objects::TerrainObjectsEditorState>();
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = feathrise_scene_content::TerrainId::parse("derived-click-canvas").unwrap();
        object.transform = default();
        object.world_min = [-10.0; 2]; object.world_max = [10.0; 2];
        draft.terrain_ground = Some(crate::authoring::terrain_ground::TerrainGroundSnapshot::test_meshes(
            draft.scene_id.clone(), &[object.clone()], avian3d::prelude::Collider::trimesh(
                vec![Vec3::new(-10.0, 0.0, -10.0), Vec3::new(10.0, 0.0, -10.0),
                    Vec3::new(10.0, 0.0, 10.0), Vec3::new(-10.0, 0.0, 10.0)],
                vec![[0, 2, 1], [0, 3, 2]])));
        draft.manifest.terrains = Some(vec![object].into());
    } // 生产承托核对真实上向特征法线；明确三角绕序，不能用仅可射线命中的凸体冒充地形面。
    let mut records = Vec::new();
    for (index, id) in ["asset-a", "asset-b"].into_iter().enumerate() {
        let mut record = crate::viewport::placement_move::interaction_tests::record();
        record.id = id.into();
        record.translation = [4.0 + index as f32 * 2.0, 9.0, 5.0];
        record.height_mode = ScenePlacementHeightMode::FollowTerrain;
        record.terrain_offset = 1.0 + index as f32;
        record.water_surfaces.clear();
        records.push(record);
    }
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    let roots = records.iter().map(|record| app.world_mut().spawn((
        ScenePlacementInstance::new(&map, record),
        Transform::from_translation(record.translation.into())
            .with_rotation(Quat::from_rotation_y(record.yaw_degrees.to_radians()))
            .with_scale(record.scale.into()),
        Visibility::Inherited, // 安装真实现存作者根，贴地不得借缺根补生路径绕过本次姿态采用。
    )).id()).collect::<Vec<_>>();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().placements.placements = records.clone();
        for (record, root) in records.iter().zip(&roots) {
            session.placement_previews.insert(record.id.clone(), *root);
        }
    }
    {
        let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
        for (index, record) in records.iter().enumerate() {
            browser.select_indexed_test_placement(&record.id, index, 0);
        }
        browser.selected_placement_id = Some("asset-a".into());
    }
    queue_click(&mut app, &["asset-a", "asset-b"], Vec2::splat(20.0));
    finish_query(&mut app); // 真实worker和采用出口先确认A，不能只手填selected标签。
    let precise = precise("asset-a");
    let state = app.world().resource::<LayerCandidateState>();
    assert_eq!(state.selected, Some(LayerTarget::Placement("asset-a".into())));
    assert_eq!(state.candidates.len(), 2);
    assert_eq!(state.placement_mesh_selection("asset-a"), Some(&precise));
    assert_eq!(app.world().resource::<EditorGltfBrowserState>().selection.as_ref(), Some(&precise));
    assert_whole_instance(&app, "asset-a");
    assert!(state.selection_revision > 0);
    let confirmations = state.selection_revision;
    let author_index = state.selected_author_index;
    (app, roots, precise, confirmations, author_index)
}

fn apply_real_follow(app: &mut App, roots: &[Entity]) {
    let before = {
        let session = app.world().resource::<EditorAuthoringSession>();
        (session.revision, session.placement_structure_revision, session.terrain_revision,
            session.placement_derived_pose_revision)
    };
    {
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        let sampler = crate::authoring::PlacementTerrainSampler::from_sources(draft, None, session.terrain_revision)
            .expect("真实贴地夹具必须已采用当前修订的地形源");
        for record in &draft.placements.placements {
            assert_eq!(sampler.sample(record.translation[0], record.translation[2]).unwrap(), 0.0,
                "夹具必须提供真实Y=0承托面，不能靠等待超时掩盖坏几何");
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.world_mut().run_system_once(crate::authoring::reconcile_following_placements).unwrap();
        if app.world().resource::<EditorAuthoringSession>().placement_derived_pose_revision != before.3 { break; }
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        let readiness = app.world().resource::<PlacementTerrainFollowState>().readiness_for(
            &draft.scene_id, session.terrain_revision, session.placement_structure_revision, draft.placements.placements.len());
        assert!(!matches!(readiness, crate::authoring::PlacementGroundingReadiness::Failed(_)),
            "真实贴地夹具失败：{readiness:?}");
        assert!(Instant::now() < deadline, "真实贴地worker未在期限内采用");
        std::thread::sleep(Duration::from_millis(1));
    } // 多地形必须等待真实采样采用，期间不运行候选poll，保留待验证的旧输入窗口。
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!((session.revision, session.placement_structure_revision, session.terrain_revision),
        (before.0, before.1, before.2), "真实派生不能伪造作者、结构或地形修订");
    assert_ne!(session.placement_derived_pose_revision, before.3, "只有真实姿态采用推进派生代次，测试不手改它");
    for (index, (record, root)) in session.draft.as_ref().unwrap().placements.placements.iter().zip(roots).enumerate() {
        assert_eq!(record.translation[1], 1.0 + index as f32);
        assert_eq!(app.world().get::<Transform>(*root).unwrap().translation.y, record.translation[1]);
    }
    assert!(session.undo.is_empty() && session.redo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

fn assert_confirmed_a(app: &App, precise: &EditorGltfMeshSelection, confirmations: u64, author_index: Option<usize>) {
    let state = app.world().resource::<LayerCandidateState>();
    assert_eq!(state.selected, Some(LayerTarget::Placement("asset-a".into())), "旧点击不能抢走或清空已确认A");
    assert_eq!(state.selected_author_index, author_index);
    assert_eq!(state.selection_revision, confirmations, "派生采用不是新的明确选择");
    let browser = app.world().resource::<EditorGltfBrowserState>();
    assert_eq!(browser.selected_placement_id.as_deref(), Some("asset-a"));
    assert_eq!(browser.selection.as_ref(), Some(precise), "派生贴地不能把已确认子网格降级成整实例");
    assert_eq!(state.placement_mesh_selection("asset-a"), Some(precise), "正式精确覆盖不随旧轮选候选一同失效");
    assert_whole_instance(app, "asset-a");
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

fn assert_whole_instance(app: &App, id: &str) {
    for ignore_hover in [false, true] {
        let info = crate::viewport::placement_selection::selected_placement_mesh_info(
            app.world().resource::<SceneEditorState>(), app.world().resource::<EditorGltfBrowserState>(),
            app.world().resource::<EditorAuthoringSession>(), app.world().resource::<LayerCandidateState>(), ignore_hover)
            .expect("视口正式选择必须持续提供表现与取景身份");
        assert_eq!(info.identity.placement_id, id);
        assert!(info.identity.whole_instance, "精确叶片引用只供浏览器同步，派生清候选不能把视口整实例静默降级");
    } // 直接验收生产表现/取景入口，而非把候选列表非空误当成正式选择语义。
}

fn assert_old_candidates_invalid(app: &App) {
    let state = app.world().resource::<LayerCandidateState>();
    assert!(state.candidates.is_empty(), "旧距离顺序的候选必须失效，不能继续同点轮选");
    assert!(state.hover.is_none() && state.pixel.is_none() && state.apply.is_none() && !state.clear_objects);
}

fn assert_fresh_b_click(app: &mut App, confirmations: u64) {
    queue_click(app, &["asset-b"], Vec2::splat(80.0));
    finish_query(app);
    let state = app.world().resource::<LayerCandidateState>();
    assert_eq!(state.selected, Some(LayerTarget::Placement("asset-b".into())), "新代次独立点击仍须正常选择B");
    assert_eq!(state.selection_revision, confirmations + 1);
    assert_eq!(state.candidates.len(), 1);
    let browser = app.world().resource::<EditorGltfBrowserState>();
    assert_eq!(browser.selected_placement_id.as_deref(), Some("asset-b"));
    assert_eq!(browser.selection, Some(precise("asset-b")));
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

fn assert_old_click_is_rejected(started: bool, empty_hit: bool) {
    let (mut app, roots, precise, confirmations, author_index) = fixture();
    queue_click(&mut app, if empty_hit { &[] } else { &["asset-b"] }, Vec2::splat(60.0));
    if started {
        app.world_mut().run_system_once(poll_layer_candidates).unwrap();
        let state = app.world().resource::<LayerCandidateState>();
        assert!(state.task.is_some() && state.request.is_none(), "生产poll必须实际建立Task，尚未运行采用出口");
    } else {
        assert!(app.world().resource::<LayerCandidateState>().request.is_some());
        assert!(app.world().resource::<LayerCandidateState>().task.is_none());
    }
    apply_real_follow(&mut app, &roots); // 后台可已完成，但采用轮询一定发生在真实贴地之后。
    finish_query(&mut app);
    assert_confirmed_a(&app, &precise, confirmations, author_index);
    assert_old_candidates_invalid(&app);
    assert_fresh_b_click(&mut app, confirmations);
}

#[test]
fn derived_click_queued_hit_and_empty_requests_keep_confirmed_precise_mesh() {
    for empty_hit in [false, true] { assert_old_click_is_rejected(false, empty_hit); }
}

#[test]
fn derived_click_inflight_hit_and_empty_tasks_keep_confirmed_precise_mesh() {
    for empty_hit in [false, true] { assert_old_click_is_rejected(true, empty_hit); }
}

#[test]
fn derived_click_stable_confirmation_keeps_precise_mesh_and_invalidates_old_cycle() {
    let (mut app, roots, precise, confirmations, author_index) = fixture();
    apply_real_follow(&mut app, &roots);
    app.update();
    assert_confirmed_a(&app, &precise, confirmations, author_index);
    assert_old_candidates_invalid(&app);
    assert_fresh_b_click(&mut app, confirmations);
}

#[test]
fn derived_click_old_candidate_button_cannot_bypass_poll_adoption_guard() {
    let (mut app, roots, precise, confirmations, author_index) = fixture();
    apply_real_follow(&mut app, &roots);
    let row = app.world_mut().spawn((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered(),
        CandidateAction::Select(1), BackgroundColor::default())).id();
    // 故意先消费真实按钮和采用出口，验证其自身权限门；不能靠poll先清掉全部候选才安全。
    app.world_mut().run_system_once(layer_candidate_interactions).unwrap();
    app.world_mut().run_system_once(apply_layer_candidate).unwrap();
    assert_confirmed_a(&app, &precise, confirmations, author_index);
    app.world_mut().despawn(row);
    app.update();
    assert_old_candidates_invalid(&app);
    assert_fresh_b_click(&mut app, confirmations);
}

#[test]
fn derived_click_transform_lock_exit_preserves_confirmation_and_discards_old_cycle() {
    let (mut app, roots, precise, confirmations, author_index) = fixture();
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Move;
    app.update();
    apply_real_follow(&mut app, &roots);
    app.update(); // 变换期间保留正式目标，但不能给旧候选重新盖上当前代次。
    assert_confirmed_a(&app, &precise, confirmations, author_index);
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    app.update();
    assert_confirmed_a(&app, &precise, confirmations, author_index);
    assert_old_candidates_invalid(&app);
    assert_fresh_b_click(&mut app, confirmations);
}

#[test]
fn derived_click_current_pending_survives_stale_published_identity_in_the_same_frame() {
    let (mut app, roots, confirmed, confirmations, author_index) = fixture();
    let old = app.world().resource::<LayerCandidateState>().identity.clone().unwrap();
    apply_real_follow(&mut app, &roots);
    queue_click(&mut app, &["asset-b"], Vec2::splat(80.0));
    {
        let state = app.world().resource::<LayerCandidateState>();
        assert_eq!(state.identity.as_ref(), Some(&old), "输入尚未经过poll，旧公布身份仍在");
        assert_ne!(state.request.as_ref().unwrap().identity.placement_derived_pose_revision,
            old.placement_derived_pose_revision, "新输入必须使用真实贴地后的当前代次");
    }
    assert_confirmed_a(&app, &confirmed, confirmations, author_index);
    finish_query(&mut app);
    let state = app.world().resource::<LayerCandidateState>();
    assert_eq!(state.selected, Some(LayerTarget::Placement("asset-b".into())));
    assert_eq!(state.selection_revision, confirmations + 1, "作废旧轮选不能吞掉同帧的新独立输入");
    assert_eq!(state.placement_mesh_selection("asset-b"), Some(&precise("asset-b")));
}

fn poll_without_apply(app: &mut App, waiting_for_canvas: bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.world_mut().run_system_once(poll_layer_candidates).unwrap();
        let state = app.world().resource::<LayerCandidateState>();
        if if waiting_for_canvas { state.waiting_canvas.is_some() }
            else { state.task.is_none() && state.request.is_none() && (state.apply.is_some() || state.clear_objects) } {
            break;
        }
        assert!(Instant::now() < deadline, "真实查询未在期限内到达待采用出口");
        std::thread::sleep(Duration::from_millis(1));
    } // 刻意不运行正式apply，复现两条Tasks链之间贴地采用的真实调度窗口。
}

fn assert_polled_result_cannot_replace_confirmation(ids: &[&str], replace_mesh: bool) {
    let (mut app, roots, precise, confirmations, author_index) = fixture();
    queue_click(&mut app, ids, Vec2::splat(60.0));
    if replace_mesh {
        let mut changed = precise.clone();
        changed.node_path = vec!["模型".into(), "另一片".into()];
        changed.mesh_name = "另一片网格".into(); changed.mesh_index = Some(2);
        app.world_mut().resource_mut::<LayerCandidateState>().request.as_mut().unwrap()
            .placement_hits.insert("asset-a".into(), PlacementViewportHit::SceneMesh(changed));
    } // 同实例的另一个精确命中也属于未确认点击，不能提前覆盖正式叶片。
    poll_without_apply(&mut app, false);
    apply_real_follow(&mut app, &roots);
    app.world_mut().run_system_once(apply_layer_candidate).unwrap();
    assert_confirmed_a(&app, &precise, confirmations, author_index);
    app.update();
    assert_old_candidates_invalid(&app);
    assert_fresh_b_click(&mut app, confirmations);
}

#[test]
fn derived_click_polled_empty_result_before_follow_cannot_clear_confirmed_precise_mesh() {
    assert_polled_result_cannot_replace_confirmation(&[], false);
}

#[test]
fn derived_click_polled_b_result_before_follow_cannot_replace_confirmed_precise_mesh() {
    assert_polled_result_cannot_replace_confirmation(&["asset-b"], false);
}

#[test]
fn derived_click_polled_same_instance_other_mesh_before_follow_cannot_replace_confirmation() {
    assert_polled_result_cannot_replace_confirmation(&["asset-a"], true);
}

fn continue_canvas_author_revision(app: &mut App) {
    let (id, before, after) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let id = session.draft.as_ref().unwrap().manifest.terrains.as_ref().unwrap()[0].id.clone();
        let before = session.revision;
        session.revision = session.revision.wrapping_add(1);
        (id, before, session.revision)
    };
    app.world_mut().resource_mut::<LayerCandidateState>().complete_terrain_canvas(&id, 0, before, after);
} // 只隔离生产显式完成接口的作者修订续接合同，不声称覆盖画布文件载入器。

fn assert_canvas_continuation_does_not_revive_old_pose(stage: usize) {
    let (mut app, roots, precise, confirmations, author_index) = fixture_with_canvas(true);
    queue_click(&mut app, &["asset-b"], Vec2::splat(60.0));
    if stage == 1 {
        app.world_mut().run_system_once(poll_layer_candidates).unwrap();
        let state = app.world().resource::<LayerCandidateState>();
        assert!(state.task.is_some() && state.request.is_none(), "必须使用生产poll创建的真实worker");
    } else if stage == 2 {
        {
            let mut state = app.world_mut().resource_mut::<LayerCandidateState>();
            let request = state.request.as_mut().unwrap();
            request.ray = Ray3d::new(Vec3::new(6.0, 20.0, 5.0), Dir3::NEG_Y);
            request.placement_candidates[0].distance = 11.0; // 旧B在Y=9，真实地面在Y=0且比资产更远。
        }
        poll_without_apply(&mut app, true);
        let state = app.world().resource::<LayerCandidateState>();
        assert!(state.waiting_canvas.is_some() && state.task.is_none() && state.request.is_none(),
            "等待必须来自真实地形射线和生产poll，不能手填CanvasWait");
    }
    let old_pose = app.world().resource::<EditorAuthoringSession>().placement_derived_pose_revision;
    apply_real_follow(&mut app, &roots);
    continue_canvas_author_revision(&mut app);
    {
        let state = app.world().resource::<LayerCandidateState>();
        for key in state.request.as_ref().map(|request| &request.identity).into_iter()
            .chain(state.task_identity.as_ref())
            .chain(state.waiting_canvas.as_ref().map(|wait| &wait.request.identity)) {
            assert_eq!(key.placement_derived_pose_revision, old_pose,
                "画布完成只允许续接作者修订，不能给旧射线与距离重盖新派生代次");
        }
    }
    finish_query(&mut app);
    assert!(app.world().resource::<LayerCandidateState>().waiting_canvas.is_none());
    assert_confirmed_a(&app, &precise, confirmations, author_index);
    assert_old_candidates_invalid(&app);
    assert_fresh_b_click(&mut app, confirmations);
}

#[test]
fn derived_click_canvas_author_continuation_cannot_revive_queued_old_pose() {
    assert_canvas_continuation_does_not_revive_old_pose(0);
}

#[test]
fn derived_click_canvas_author_continuation_cannot_revive_real_worker_old_pose() {
    assert_canvas_continuation_does_not_revive_old_pose(1);
}

#[test]
fn derived_click_canvas_author_continuation_cannot_revive_production_waiting_old_pose() {
    assert_canvas_continuation_does_not_revive_old_pose(2);
}

#[test]
fn derived_click_explicit_scene_tree_takeover_keeps_precise_mesh_after_follow() {
    let (mut app, roots, precise, _, _) = fixture();
    app.world_mut().resource_mut::<LayerCandidateState>().clear_for_scene_tree();
    // 同一叶片被场景树明确重选时浏览器值相同，生产交接入口必须取消视口整实例来源。
    // 此处隔离交接合同，不声称覆盖私有行组件的UI点击。
    apply_real_follow(&mut app, &roots);
    app.update();
    for ignore_hover in [false, true] {
        let info = crate::viewport::placement_selection::selected_placement_mesh_info(
            app.world().resource::<SceneEditorState>(), app.world().resource::<EditorGltfBrowserState>(),
            app.world().resource::<EditorAuthoringSession>(), app.world().resource::<LayerCandidateState>(), ignore_hover).unwrap();
        assert!(!info.identity.whole_instance, "明确场景树网格选择不能被保留视口来源的逻辑反弹成整实例");
        assert_eq!(info.identity.placement_id, precise.placement_id);
        assert_eq!(info.identity.node_path, precise.node_path);
        assert_eq!(info.identity.mesh_name, precise.mesh_name);
    }
    assert!(!app.world().resource::<LayerCandidateState>().has_viewport_candidates());
    assert_eq!(app.world().resource::<EditorGltfBrowserState>().selection.as_ref(), Some(&precise));
}

fn assert_author_focus_dispatch_after_follow(water_target: bool) {
    let (mut app, roots, _, confirmations, _) = fixture();
    let target = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        if water_target {
            let mut water = crate::authoring::water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder);
            water.layers[0].id = "focus-water".into();
            water.layers[0].water_level = 9.0;
            draft.water = Some(water);
            LayerTarget::Water(crate::types::WaterAuthoringFeatureKind::StillWater, "focus-water".into())
        } else {
            draft.portals.portals.push(feathrise_scene_content::ScenePortalRecord {
                terrain_binding: None, arrival_terrain_binding: None, id: "focus-portal".into(),
                label: "取景传送点".into(), label_customized: true, position: [3.0, 0.0, 5.0],
                arrival: [8.0, 0.0, 5.0], target: feathrise_scene_content::ScenePortalTarget::default_spawn(draft.scene_id.clone()),
                color_srgba: [1.0; 4], arrow_degrees: 0.0,
            });
            LayerTarget::Portal("focus-portal".into())
        }
    };
    queue_click(&mut app, &[], Vec2::splat(60.0));
    if water_target {
        app.world_mut().resource_mut::<LayerCandidateState>().request.as_mut().unwrap()
            .water_candidates.push(LayerCandidate { author_index: Some(0), target: target.clone(),
                label: "静水体 · 取景水面".into(), rank: 0, distance: 3.0 });
    } // 水命中在生产输入边界交付；Portal由真实gameplay快照和worker几何查询产生。
    finish_query(&mut app);
    {
        let state = app.world().resource::<LayerCandidateState>();
        assert_eq!(state.selected.as_ref(), Some(&target));
        assert_eq!(state.selection_revision, confirmations + 1);
        assert_eq!(state.candidates.len(), 1);
    }
    apply_real_follow(&mut app, &roots);
    app.update();
    assert_old_candidates_invalid(&app);
    {
        let state = app.world().resource::<LayerCandidateState>();
        assert_eq!(state.selected.as_ref(), Some(&target));
        assert_eq!(state.selection_revision, confirmations + 1);
        assert_eq!(state.selected_author_index, Some(0));
    }
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    app.init_resource::<crate::resources::SceneEditorNumericScrubMouseGuard>()
        .init_resource::<crate::authoring::WaterNetworkEditState>()
        .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>()
        .insert_resource(crate::resources::SceneEditorViewportState {
            _image: default(), camera: Entity::PLACEHOLDER, center: Vec3::new(100.0, 200.0, 300.0),
            scene_center: Vec3::ZERO, scene_half_extents: Vec3::ONE, extent: 50.0,
            yaw: 0.8, pitch: 0.4, framed_scene_id: Some(map.clone()), render_size: UVec2::new(800, 600),
        });
    app.world_mut().resource_mut::<SceneEditorState>().viewport_framing_requested = Some(crate::types::EditorViewportFraming::Selected);
    app.world_mut().run_system_once(crate::viewport::framing::apply_viewport_framing).unwrap();
    assert_eq!(app.world().resource::<SceneEditorState>().status, "正在聚焦所选，等待当前完整范围",
        "清掉旧候选列表后仍须按正式Water/Portal分派Selected取景");
    assert!(app.world().resource::<SelectedPlacementViewportState>().focus_request.is_none(),
        "旧Placement不得接管正式作者对象的取景请求");
    assert_eq!(app.world().resource::<LayerCandidateState>().selected.as_ref(), Some(&target));
    if water_target {
        assert_eq!(app.world().resource::<SelectedWaterViewportState>().viewport_feature(&map),
            Some((crate::types::WaterAuthoringFeatureKind::StillWater, "focus-water")));
    } else {
        assert_eq!(app.world().resource::<SceneEditorState>().selected_current_portal_id.as_deref(), Some("focus-portal"));
    }
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
} // 这里只验收实际取景请求分派；未安装完整水面预览，不声称范围解析或相机定位完成。

#[test]
fn derived_click_confirmed_water_still_dispatches_selected_focus_after_old_candidates_expire() {
    assert_author_focus_dispatch_after_follow(true);
}

#[test]
fn derived_click_confirmed_portal_still_dispatches_selected_focus_after_old_candidates_expire() {
    assert_author_focus_dispatch_after_follow(false);
}

fn assert_canvas_continuation_keeps_confirmed_portal_form(started: bool) {
    let (mut app, roots, _, confirmations, _) = fixture_with_canvas(true);
    let target = LayerTarget::Portal("canvas-portal".into());
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.portals.portals.push(feathrise_scene_content::ScenePortalRecord {
            terrain_binding: None, arrival_terrain_binding: None, id: "canvas-portal".into(),
            label: "画布续接传送点".into(), label_customized: true, position: [3.0, 0.0, 0.0],
            arrival: [8.0, 0.0, 0.0], target: feathrise_scene_content::ScenePortalTarget::default_spawn(draft.scene_id.clone()),
            color_srgba: [1.0; 4], arrow_degrees: 0.0,
        });
    }
    queue_click(&mut app, &[], Vec2::splat(40.0));
    app.world_mut().resource_mut::<LayerCandidateState>().request.as_mut().unwrap().ray =
        Ray3d::new(Vec3::new(0.0, 9.0, 0.0), Dir3::X);
    // P的首次射线在Z=0，后续B射线在Z=5，超过正式传送半径；不能让P遮住B掩盖旧点击抢选。
    finish_query(&mut app);
    let portal_confirmations = confirmations + 1;
    {
        let state = app.world().resource::<LayerCandidateState>();
        assert_eq!(state.selected.as_ref(), Some(&target));
        assert_eq!(state.selected_author_index, Some(0));
        assert_eq!(state.selection_revision, portal_confirmations);
        assert!(state.selected_matches_session(app.world().resource::<EditorAuthoringSession>()));
    }
    queue_click(&mut app, &["asset-b"], Vec2::splat(60.0));
    if started {
        app.world_mut().run_system_once(poll_layer_candidates).unwrap();
        let state = app.world().resource::<LayerCandidateState>();
        assert!(state.task.is_some() && state.request.is_none(), "必须保留生产poll建立的真实worker窗口");
    } else {
        let state = app.world().resource::<LayerCandidateState>();
        assert!(state.request.is_some() && state.task.is_none());
    }
    apply_real_follow(&mut app, &roots);
    continue_canvas_author_revision(&mut app);
    finish_query(&mut app);
    for _ in 0..2 {
        let state = app.world().resource::<LayerCandidateState>();
        assert_eq!(state.selected.as_ref(), Some(&target));
        assert_eq!(state.selected_author_index, Some(0));
        assert_eq!(state.selection_revision, portal_confirmations);
        assert!(state.selected_matches_session(app.world().resource::<EditorAuthoringSession>()),
            "画布显式续接淘汰旧B时仍须保留Portal的正式作者身份，否则所选属性表单会永久收回");
        assert_eq!(app.world().resource::<SceneEditorState>().selected_current_portal_id.as_deref(), Some("canvas-portal"));
        assert_old_candidates_invalid(&app);
        app.update();
    }
    assert_fresh_b_click(&mut app, portal_confirmations);
} // 直接验收正式表单使用的身份门，不调用会重新确认目标的UI样例夹具修补状态。

#[test]
fn derived_click_canvas_completion_keeps_confirmed_portal_form_after_queued_old_b_is_rejected() {
    assert_canvas_continuation_keeps_confirmed_portal_form(false);
}

#[test]
fn derived_click_canvas_completion_keeps_confirmed_portal_form_after_real_worker_old_b_is_rejected() {
    assert_canvas_continuation_keeps_confirmed_portal_form(true);
}
