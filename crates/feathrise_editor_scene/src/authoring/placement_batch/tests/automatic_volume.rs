//! 自动雕刻层归专用拓扑作者链；真实框选、批删及旧选集手柄不能接管其记录或驻留子树。

use super::super::{types::*, worker, PlacementBatchState, handle_placement_batch, sync_batch_selection};
use crate::{
    authoring::{EditorAuthoringSession, courtyards::{CourtyardAuthoringState, sync_visuals}},
    components::EditorPlacedAsset,
    resources::{SceneEditorState, SceneEditorViewportState},
    ui::{gltf_browser::EditorGltfBrowserState, placement_water::{
        register_placement_instance_root, unregister_placement_instance_root,
    }},
    viewport::{
        modes::{ViewportMode, ViewportModeState},
        multi_select::{PlacementSelectionSet, SelectedMember, advance_multi_selection, handle_multi_select_pointer},
        placement_move::{handle_placement_move, interaction_tests::{app as base_app, camera, step}},
    },
};
use bevy::{ecs::system::RunSystemOnce, prelude::*, ui::RelativeCursorPosition};
use std::sync::Arc;
use feathrise_scene_content::{ScenePlacementRecord, ScenePlacementTerrainPatch, SceneTerrainCutout, SceneTerrainKind};
use feathrise_scene::{ScenePlacementInstance, StreamedVolumeResident, TerrainVolumeAuthorPreviewRoot};

fn volume_app(streamed: bool) -> (App, Entity, Entity, Entity, Entity, ScenePlacementRecord) {
    let (mut app, ordinary_root, node) = base_app();
    app.add_plugins(bevy::transform::TransformPlugin)
        .init_asset::<bevy::world_serialization::WorldAsset>()
        .init_resource::<PlacementBatchState>()
        .init_resource::<PlacementSelectionSet>()
        .init_resource::<CourtyardAuthoringState>()
        .add_observer(register_placement_instance_root)
        .add_observer(unregister_placement_instance_root)
        .add_systems(Update, (handle_placement_batch, sync_batch_selection).chain().before(handle_placement_move));
    let (map, volume, structure) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
        let mut volume = draft.placements.placements[0].clone();
        volume.id = "automatic-volume-neighbor".into();
        volume.asset_path = format!("scenes/{}/volume_sculpt/accepted.glb", draft.scene_id);
        volume.translation = [6.5, 4.0, 5.0];
        volume.yaw_degrees = 0.0;
        volume.scale = [1.0; 3];
        volume.water_surfaces.clear();
        volume.terrain_patch = Some(ScenePlacementTerrainPatch {
            sculpted_volume: true,
            streamed_volume: streamed,
            volume_cell_size: streamed.then_some(0.5),
            cutouts: vec![SceneTerrainCutout { min: [-0.25; 2], max: [0.25; 2] }],
            ..default()
        });
        // streamed协议必须同时带sculpted身份；两种夹具均先证明是真实合法记录，不能靠非法组合制造红灯。
        volume.validated_transform(&draft.scene_id, draft.manifest.world_bounds().unwrap()).unwrap();
        assert!(volume.editor.selectable());
        draft.placements.placements.push(volume.clone());
        (draft.scene_id.clone(), volume, session.placement_structure_revision)
    };
    // 夹具位于联合采用完成后的编辑边界；组件组合与生产lifecycle一致，不把普通对象硬标为Resident。
    // 这里只驱动真实选择/批量事务，不声称验证雕刻计算、GLB加载或GPU联合采用。
    let volume_root = app.world_mut().spawn((
        EditorPlacedAsset,
        ScenePlacementInstance::new(&map, &volume),
        Transform::from_translation(volume.translation.into()),
        Visibility::Inherited,
        TerrainVolumeAuthorPreviewRoot(feathrise_scene_content::SceneInstancePath::root(map)
            .terrain(feathrise_scene_content::TerrainId::legacy())),
    )).id();
    if streamed { app.world_mut().entity_mut(volume_root).insert(StreamedVolumeResident::default()); }
    let child = app.world_mut().spawn((Transform::IDENTITY, ChildOf(volume_root))).id();
    app.world_mut().resource_mut::<EditorAuthoringSession>().placement_previews.insert(volume.id.clone(), volume_root);
    {
        let mut browser = app.world_mut().resource_mut::<EditorGltfBrowserState>();
        browser.select_indexed_test_placement(&volume.id, 1, structure);
        browser.selected_placement_id = None;
    }
    app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select;
    app.world_mut().resource_mut::<SceneEditorState>().placement_transform_enabled = false;
    (app, node, ordinary_root, volume_root, child, volume)
}

fn selection_step(app: &mut App, node: Entity, cursor: Vec2) {
    app.world_mut().get_mut::<RelativeCursorPosition>(node).unwrap().normalized = Some(cursor);
    app.world_mut().run_system_once(handle_multi_select_pointer).unwrap();
    app.world_mut().run_system_once(advance_multi_selection).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().clear();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
}

fn select_rectangle(app: &mut App, node: Entity, volume: &ScenePlacementRecord) {
    let camera = camera(app);
    let ordinary = camera.project(Vec3::new(5.0, 4.0, 5.0)).unwrap();
    let volume = camera.project(volume.translation.into()).unwrap();
    let min = ordinary.min(volume) - Vec2::splat(0.025);
    let max = ordinary.max(volume) + Vec2::splat(0.025);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    selection_step(app, node, min);
    selection_step(app, node, max);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    selection_step(app, node, max);
    selection_step(app, node, max); // 静默释放鼠标归属，再进入真实Delete输入。
}

fn settle_batch(app: &mut App, node: Entity) {
    for _ in 0..256 {
        step(app, node, Vec2::ZERO);
        app.world_mut().run_system_once(sync_visuals).unwrap();
        if !app.world().resource::<PlacementBatchState>().busy()
            && !app.world().resource::<CourtyardAuthoringState>().visual_sync_pending() { return; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("批量删除未完成：{}", app.world().resource::<SceneEditorState>().status);
}

fn publish_old_members(app: &mut App) {
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        let session = world.resource::<EditorAuthoringSession>();
        let members = session.draft.as_ref().unwrap().placements.placements.iter().enumerate()
            .map(|(index, record)| SelectedMember { id: record.id.clone(), index, position: record.translation.into() }).collect();
        selection.publish(members, session);
    }); // 防御性旧选集复核：不以此注入代替上方真实框选缺陷的可达性证明。
}

#[test]
fn automatic_volume_rectangle_delete_keeps_author_record_and_resident_subtree() {
    for streamed in [false, true] {
        let (mut app, node, ordinary_root, volume_root, child, volume) = volume_app(streamed);
        select_rectangle(&mut app, node, &volume);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Delete);
        step(&mut app, node, Vec2::ZERO);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Delete);
        settle_batch(&mut app, node);
        // 不在框选后提前断言：旧实现必须走完worker、accept和视觉队列，暴露实际记录/实体销毁。
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, vec![volume.clone()],
            "普通框选删除只能处理普通成员，自动雕刻层必须继续由专用历史负责；streamed={streamed}");
        assert_eq!(session.undo.len(), 1, "普通成员仍须完成一次真实删除，不能以全部禁用掩盖缺陷");
        assert_eq!(session.placement_previews.get(&volume.id), Some(&volume_root));
        assert!(app.world().get_entity(ordinary_root).is_err());
        assert!(app.world().get::<ScenePlacementInstance>(volume_root).unwrap().matches_record(
            &session.draft.as_ref().unwrap().scene_id, &volume, &[]));
        assert_eq!(app.world().get::<Visibility>(volume_root), Some(&Visibility::Inherited));
        assert_eq!(app.world().get::<ChildOf>(child).map(ChildOf::parent), Some(volume_root));
        assert_eq!(app.world().get::<StreamedVolumeResident>(volume_root).is_some(), streamed);
    }
}

#[test]
fn automatic_volume_delete_worker_rejects_a_complete_mixed_capture_atomically() {
    for streamed in [false, true] {
        let (app, _, _, _, _, _) = volume_app(streamed);
        let session = app.world().resource::<EditorAuthoringSession>();
        let draft = session.draft.as_ref().unwrap();
        let capture = Capture { model_vegetation: Arc::default(),
            identity: BatchIdentity::from_session(session, None).unwrap(), action: PlacementBatchAction::Delete,
            records: draft.placements.placements.iter().cloned().enumerate().collect(),
            members: std::sync::Arc::new(draft.placements.placements.iter().enumerate().map(|(index, record)|
                SelectedMember { id: record.id.clone(), index, position: record.translation.into() }).collect()),
            bounds: draft.manifest.world_bounds().unwrap(), source_objects: draft.manifest.terrain_objects_for_edit(),
            length: draft.placements.placements.len(), selection_revision: 0, selected_id: None,
        };
        assert!(matches!(worker::capture(capture), BatchOutput::Deleted(Err(_))),
            "worker末端也须拒绝旧入口递交的混合自动层删除，不能只修选择表现；streamed={streamed}");
    }
}

#[test]
fn automatic_volume_browser_primary_cannot_publish_an_ordinary_edit_selection() {
    for streamed in [false, true] { for stale in [false, true] {
        let (mut app, _, _, _, _, volume) = volume_app(streamed);
        if stale {
            app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
                selection.publish(vec![SelectedMember {
                    id: volume.id.clone(), index: 1, position: volume.translation.into(),
                }], world.resource::<EditorAuthoringSession>());
            }); // ready且同ID的旧主项也必须重验归属，不能仅阻止首次加入。
        }
        app.world_mut().resource_mut::<EditorGltfBrowserState>().selected_placement_id = Some(volume.id);
        app.world_mut().run_system_once(advance_multi_selection).unwrap();
        assert!(app.world().resource::<PlacementSelectionSet>().members.is_empty(),
            "场景树可查看作者层，但主选择同步不能重新授予普通多选/手柄资格");
    } }
}

#[test]
fn automatic_volume_click_and_shift_click_keep_the_previous_ordinary_selection() {
    for streamed in [false, true] { for additive in [false, true] {
        let (mut app, node, _, _, _, volume) = volume_app(streamed);
        select_rectangle(&mut app, node, &volume);
        let result = app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
            selection.click(SelectedMember { id: volume.id.clone(), index: 1, position: volume.translation.into() },
                additive, world.resource::<EditorAuthoringSession>())
        });
        assert!(result.is_err(), "真实候选收编接口同样拒绝自动层，不能借普通单击或Shift追加恢复编辑资格");
        let selection = app.world().resource::<PlacementSelectionSet>();
        assert_eq!(selection.members.len(), 1);
        assert_eq!(selection.members[0].id, "move-test");
    } }
}

#[test]
fn automatic_volume_old_mixed_selection_delete_is_rejected_during_capture() {
    for streamed in [false, true] {
        let (mut app, node, ordinary_root, volume_root, child, _) = volume_app(streamed);
        publish_old_members(&mut app);
        let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Delete);
        step(&mut app, node, Vec2::ZERO);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().release(KeyCode::Delete);
        settle_batch(&mut app, node);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, before,
            "旧混合选集不能静默跳过自动层后部分删除；捕获阶段必须保留完整原批次");
        assert!(session.undo.is_empty());
        assert!(app.world().get_entity(ordinary_root).is_ok());
        assert!(app.world().get_entity(volume_root).is_ok());
        assert_eq!(app.world().get::<ChildOf>(child).map(ChildOf::parent), Some(volume_root));
    }
}

#[test]
fn automatic_volume_refresh_removes_old_members_before_republishing_selection() {
    for streamed in [false, true] {
        let (mut app, _, _, _, _, volume) = volume_app(streamed);
        publish_old_members(&mut app);
        app.world_mut().resource_mut::<EditorAuthoringSession>().revision += 1;
        app.world_mut().run_system_once(advance_multi_selection).unwrap();
        let selection = app.world().resource::<PlacementSelectionSet>();
        assert!(selection.ready_for(app.world().resource::<EditorAuthoringSession>()));
        assert_eq!(selection.members.len(), 1);
        assert!(!selection.contains(&volume.id), "旧集合不能在有界刷新后重新取得自动雕刻层资格");
    }
}

#[test]
fn automatic_volume_old_group_selection_does_not_reparent_resident_before_release() {
    for streamed in [false, true] {
        let (mut app, node, _, volume_root, child, _) = volume_app(streamed);
        publish_old_members(&mut app);
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Move;
        app.world_mut().resource_mut::<SceneEditorState>().placement_transform_enabled = true;
        let center = app.world().resource::<PlacementSelectionSet>().center;
        app.world_mut().resource_mut::<SceneEditorViewportState>().center = center;
        let camera = camera(&app);
        let cursor = camera.project(center + Vec3::X * camera.handle_length(center).unwrap() * 0.65).unwrap();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, cursor);
        step(&mut app, node, cursor);
        assert!(app.world().get::<ChildOf>(volume_root).is_none(),
            "旧组选集须在group_bind之前失败关闭，不能等worker校验时才归还已被重挂的Resident");
        assert_eq!(app.world().get::<ChildOf>(child).map(ChildOf::parent), Some(volume_root));
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}
