//! 保存等待期间的关联内容后台采用回归：空文档完成后放行，地形换代拒绝旧结果。

use super::*;
use crate::resources::MapEditorState;
use std::time::{Duration, Instant};

const TERRAIN_REVISION: u64 = 3;

/// 通过正式空索引构造合法修订；测试不直接构造或放宽地形索引的内部字段。
fn empty_ground(map: &MapId, generation: u64) -> Arc<TerrainGroundSnapshot> {
    let mut ground = TerrainGroundSnapshot::empty(map.clone(), TERRAIN_REVISION);
    Arc::get_mut(&mut ground).unwrap().generation = generation;
    ground
}

fn save_waiting_empty_map() -> App {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    // 复用完整草稿结构后移除旧单地形几何；Some([])必须保留真正空容器语义。
    draft.manifest.terrain = feathrise_map_content::MapTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Default::default());
    draft.terrain_mesh = None;
    draft.terrain_editing_index = None;
    draft.terrain_source_sha256 = None;
    draft.heights = None;
    draft.terrain_ground = Some(empty_ground(&draft.map_id, 1));
    assert!(draft.domain.is_none());
    assert!(draft.portals.portals.is_empty());
    let map = draft.map_id.clone();

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(MapEditorState {
            open: true,
            save_requested: true,
            selected_map_id: Some(map),
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 7,
            terrain_revision: TERRAIN_REVISION,
            portal_revision: 2,
            ..default()
        })
        .init_resource::<ContentTerrainFollowState>()
        .add_systems(Update, reconcile);
    app
}

fn ready(app: &App) -> bool {
    app.world()
        .resource::<ContentTerrainFollowState>()
        .ready(app.world().resource::<EditorAuthoringSession>())
}

fn finish_current_follow(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !ready(app) {
        app.update();
        let state = app.world().resource::<ContentTerrainFollowState>();
        assert!(
            state.failure.is_none(),
            "空关联文档不应失败：{:?}",
            state.failure
        );
        assert!(
            Instant::now() < deadline,
            "保存请求不能阻止当前关联后台结果完成"
        );
        std::thread::sleep(Duration::from_millis(1)); // 仅测试让出CPU等待真实任务，不改变产品轮询路径。
    }
}

fn replace_ground_generation(app: &mut App, generation: u64) {
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    draft.terrain_ground = Some(empty_ground(&draft.map_id, generation));
}

fn assert_stable_ready(app: &mut App, generation: u64) {
    for _ in 0..16 {
        app.update();
        let session = app.world().resource::<EditorAuthoringSession>();
        let state = app.world().resource::<ContentTerrainFollowState>();
        assert!(state.ready(session));
        assert!(
            state.capture.is_none() && state.work.is_none(),
            "稳定帧不得重启关联批次"
        );
        assert!(state.waiting.is_none() && state.failure.is_none());
        assert_eq!(state.accepted.as_ref().unwrap().generation, generation);
        assert!(app.world().resource::<MapEditorState>().save_requested);
        assert!(!session.saving); // 本夹具只验证保存前置门，不启动真实磁盘保存事务。
    }
}

#[test]
fn empty_content_finishes_terrain_follow_while_save_is_requested() {
    let mut app = save_waiting_empty_map();
    assert!(!ready(&app));
    app.update();
    assert!(
        app.world()
            .resource::<ContentTerrainFollowState>()
            .work
            .is_some()
    );
    // 保存意图已存在时仍必须推进；否则保存门与后台工作会互相等待。
    finish_current_follow(&mut app);
    assert_stable_ready(&mut app, 1);
}

#[test]
fn ground_generation_rejects_accepted_and_in_flight_results_while_save_waits() {
    let mut app = save_waiting_empty_map();
    finish_current_follow(&mut app);
    assert_stable_ready(&mut app, 1);

    replace_ground_generation(&mut app, 2);
    assert!(!ready(&app), "旧accepted不能给新地形代次保存信用");
    app.update();
    {
        let state = app.world().resource::<ContentTerrainFollowState>();
        assert_eq!(state.accepted.as_ref().unwrap().generation, 1);
        assert_eq!(state.work.as_ref().unwrap().identity.generation, 2);
    }
    // 在正式轮询接收前再换代，使真实后台任务即便已经结束也只能作为迟到结果丢弃。
    replace_ground_generation(&mut app, 3);
    assert!(!ready(&app));
    assert!(app.world().resource::<MapEditorState>().save_requested);

    let deadline = Instant::now() + Duration::from_secs(5);
    while !app
        .world()
        .resource::<ContentTerrainFollowState>()
        .work
        .as_ref()
        .unwrap()
        .task
        .is_finished()
    {
        assert!(
            Instant::now() < deadline,
            "旧关联任务应在测试等待预算内完成"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    app.update(); // 明确消费已完成的旧代次，不能仅凭最终ready掩盖错误采用旧结果。
    {
        let state = app.world().resource::<ContentTerrainFollowState>();
        assert_eq!(state.accepted.as_ref().unwrap().generation, 1);
        assert_eq!(state.work.as_ref().unwrap().identity.generation, 3);
    }
    assert!(!ready(&app));
    finish_current_follow(&mut app);
    assert_stable_ready(&mut app, 3);
}

#[test]
fn late_domain_follow_cannot_replace_a_new_author_document() {
    use crate::game_authoring::map_interaction::fixtures;
    let mut app = save_waiting_empty_map();
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().domain =
        Some(fixtures::document("old-body"));
    app.update();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.world().resource::<ContentTerrainFollowState>().work.as_ref().unwrap().task.is_finished() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    // 让真正后台结果先完成，再推进作者身份与正文；采用系统必须明确丢弃已完成的旧文件候选。
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.revision += 1;
        session.draft.as_mut().unwrap().domain = Some(fixtures::document("current-body"));
    }
    app.update();
    assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().adopted_domain_hash(), "current-body");
    assert!(!ready(&app));
    finish_current_follow(&mut app);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().adopted_domain_hash(), "current-body");
}
