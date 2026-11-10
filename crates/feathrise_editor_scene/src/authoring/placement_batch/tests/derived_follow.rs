//! 派生贴地在分帧捕获或后台删除期间采用时，旧批量快照不得发布或变成错误撤销起点。

use super::*;

fn follow_app(count: usize) -> (App, Entity) {
    let (mut app, node) = app(count);
    app.init_resource::<crate::authoring::TerrainBrushStroke>()
        .init_resource::<crate::authoring::WaterBrushStroke>()
        .init_resource::<crate::authoring::PlacementTerrainFollowState>();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        for (index, record) in session.draft.as_mut().unwrap().placements.placements.iter_mut().enumerate() {
            record.height_mode = ScenePlacementHeightMode::FollowTerrain;
            record.terrain_offset = 0.5 + index as f32 * 0.25;
        }
    }
    (app, node)
}

fn follow(app: &mut App) -> Vec<feathrise_scene_content::ScenePlacementRecord> {
    let before = {
        let session = app.world().resource::<EditorAuthoringSession>();
        (session.revision, session.placement_structure_revision, session.terrain_revision)
    };
    // 真实生产入口直接采用同地形修订的高度结果，不以手动修改记录或revision模拟派生。
    app.world_mut().run_system_once(crate::authoring::reconcile_following_placements).unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!((session.revision, session.placement_structure_revision, session.terrain_revision), before);
    let records = session.draft.as_ref().unwrap().placements.placements.clone();
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record.translation[1], 2.5 + index as f32 * 0.25);
    }
    assert!(session.undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    records
}

#[test]
fn derived_follow_rejects_copy_captured_across_two_pose_versions() {
    let (mut app, node) = follow_app(17);
    key(&mut app, node, KeyCode::KeyC, Vec2::ZERO);
    step(&mut app, node, Vec2::ZERO);
    assert_eq!(app.world().resource::<PlacementBatchState>().capture.as_ref().unwrap().records.len(), 16);
    let derived = follow(&mut app);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    assert!(app.world().resource::<PlacementBatchState>().clipboard.is_none(),
        "前16项旧姿态与第17项新姿态不能拼成完整剪贴板");
    assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements, derived);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}

#[test]
fn derived_follow_rejects_late_delete_without_stale_undo_snapshot() {
    let (mut app, node) = follow_app(2);
    key(&mut app, node, KeyCode::Delete, Vec2::ZERO);
    step(&mut app, node, Vec2::ZERO);
    assert!(app.world().resource::<PlacementBatchState>().job.is_some(),
        "先让真实删除工作进入唯一后台槽，再采用派生结果");
    let derived = follow(&mut app);
    until(&mut app, node, Vec2::ZERO, |state| !state.busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, derived,
        "旧删除快照必须取消，不能形成会恢复贴地前姿态的撤销记录");
    assert!(session.undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}
