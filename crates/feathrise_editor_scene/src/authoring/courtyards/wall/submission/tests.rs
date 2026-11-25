//! 从正式围墙确认运行真实 GLB 地基任务，检查捕获、迟到、退役与一次交换。
use super::*;
use super::super::{Module, WallTool, submit};
use crate::authoring::{buildings::{BuildingAuthoringState, move_test_support::install_existing_edit_validator},
    courtyards::operations::poll_operation, test_heightfield_draft};
use crate::types::EditorTool;
use crate::viewport::placement_move::interaction_tests::{app as base_app, record};
use bevy::{ecs::system::RunSystemOnce, prelude::*};
use feathrise_scene_content::{BuildingConnectionPointDescriptor, BuildingPresetCategory, BuildingPresetDescriptor, BuildingPresetRegistry};
use std::time::{Duration, Instant};

const PREFIX: usize = 257;
const SEGMENTS: usize = 1025;

fn fixture() -> (App, Vec<ScenePlacementRecord>) {
    let (mut app, _, _) = base_app();
    let mut draft = test_heightfield_draft(Arc::new(vec![1; 121]));
    let originals: Vec<_> = (0..PREFIX).map(|i| {
        let mut item = record();
        item.id = format!("existing-{i}");
        if i > 0 { item.water_surfaces.clear(); }
        item
    }).collect();
    draft.placements.placements = originals.clone();
    draft.placement_water_surface_count = 1;
    let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
    editor.tool = EditorTool::WallLine;
    editor.selected_scene_id = Some(draft.scene_id.clone());
    drop(editor);
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft = Some(draft);
    // 统一测量单位使 1,025 个真实连接模块落在夹具的 10×10 地形范围内。
    let half = 1.0 / 256.0;
    let mut preset = BuildingPresetDescriptor::from_asset("wall-test".into(), "wall".into(), "models/wall.glb".into(), default());
    preset.category = BuildingPresetCategory::Wall;
    preset.footprint.half_extents = [half * 0.8, 0.1];
    preset.entrance.position = [0.0, 0.0, 0.1];
    preset.entrance.clearance_half_extents = [half * 0.5, 0.05];
    preset.connections = [("west", -half, 270.0), ("east", half, 90.0)].map(|(id, x, yaw)| BuildingConnectionPointDescriptor {
        id: id.into(), kind: "wall".into(), position: [x, 0.], outward_yaw_degrees: yaw,
    }).into();
    let module = Module::resolve(&preset, None).unwrap();
    let points = vec![Vec3::new(1., 1., 5.), Vec3::new(1. + SEGMENTS as f32 * 2. * half, 1., 5.)];
    let plan = Arc::new(Plan::build(&points, &module, None).unwrap());
    assert_eq!(plan.count, SEGMENTS);
    install_existing_edit_validator(&mut app, preset);
    app.init_resource::<CourtyardAuthoringState>().insert_resource(WallTool { epoch: 8, plan, points, ..default() });
    request(&mut app).unwrap();
    (app, originals)
}

fn request(app: &mut App) -> Result<(), String> {
    app.world_mut().resource_scope(|world, mut state: Mut<CourtyardAuthoringState>| {
        world.resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
            submit(world.resource::<WallTool>(), &mut session, world.resource::<BuildingPresetRegistry>(),
                world.resource::<BuildingAuthoringState>(), &mut state, None)
        })
    })
}

fn tick(app: &mut App) { app.world_mut().run_system_once(poll_operation).unwrap(); }

fn wait_for_ground(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !app.world().resource::<CourtyardAuthoringState>().operation_task.as_ref().unwrap().is_finished() {
        assert!(Instant::now() < deadline, "正式地基 worker 应完成");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn initial_large_wall_publishes_one_complete_array_and_retires_previous_allocation() {
    let (mut app, originals) = fixture();
    let original_allocation = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.as_ptr();
    for count in [128, 256] {
        tick(&mut app);
        let state = app.world().resource::<CourtyardAuthoringState>();
        assert_eq!(state.operation_waiting.as_ref().unwrap().wall.as_ref().unwrap().prefix.len(), count);
        assert!(state.operation_task.is_none());
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, originals);
        assert!(session.undo.is_empty());
    }
    tick(&mut app);
    wait_for_ground(&mut app);
    tick(&mut app);
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    assert_eq!(draft.placements.placements.len(), PREFIX + SEGMENTS, "{}", app.world().resource::<SceneEditorState>().status);
    assert_ne!(draft.placements.placements.as_ptr(), original_allocation);
    assert_eq!(draft.placements.placements[..PREFIX], originals);
    let [EditorCommand::CourtyardBatch(change)] = session.undo.as_slice() else { panic!("整墙必须只占一笔历史"); };
    assert_eq!(draft.placements.placements[PREFIX..], change.added);
    assert_eq!(change.wall_base, Some(PREFIX));
    assert_eq!(draft.placement_water_surface_count, 1);
    let state = app.world().resource::<CourtyardAuthoringState>();
    assert!(state.visual_sync_pending() && state.operation_pending());
    assert!(request(&mut app).is_err(), "旧数组和本次可视结果均保留原单飞背压");
}

#[test]
fn initial_wall_cancelled_capture_and_stale_ground_result_retire_without_publishing() {
    for completed in [false, true] {
        let (mut app, originals) = fixture();
        tick(&mut app);
        if completed {
            tick(&mut app); tick(&mut app);
            wait_for_ground(&mut app);
            app.world_mut().resource_mut::<EditorAuthoringSession>().placement_derived_pose_revision += 1;
        } else {
            app.world_mut().resource_mut::<WallTool>().clear_route();
        }
        tick(&mut app);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.draft.as_ref().unwrap().placements.placements, originals);
        assert!(session.undo.is_empty() && session.redo.is_empty());
        let state = app.world().resource::<CourtyardAuthoringState>();
        assert!(!state.visual_sync_pending());
        assert!(state.operation_task.is_some() && state.operation_waiting.is_none());
        assert!(request(&mut app).is_err(), "迟到候选完成后台退役前不能复用槽");
        wait_for_ground(&mut app);
        tick(&mut app);
        assert!(!app.world().resource::<CourtyardAuthoringState>().operation_pending());
    }
}
