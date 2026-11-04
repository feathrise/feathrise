//! 回归：草地后台工作与地形背面显示不阻塞F12退出，草稿、任务和显示偏好继续保留。

use super::*; // 调用生产关闭系统与相同作者资源。
use crate::authoring::test_heightfield_draft; // 其他作者域均干净的独立草稿。
use feathrise_map_content::{ActiveDynamicMap, MapVegetationAsset}; // 当前动态地图身份。
use feathrise_scene::{DynamicVegetationPhase, DynamicVegetationStatus}; // 模拟截图中候选等待状态。

#[test]
fn grass_background_work_and_terrain_backfaces_never_block_f12_close_or_save_draft() {
    for compiled in [false, true] {
        // 同时覆盖编译未完成、候选已完成但等待运行时。
        let mut draft = test_heightfield_draft(std::sync::Arc::new(vec![0; 121])); // 与用户磁盘无关。
        let map = draft.map_id.clone(); // 所有资源指向同一地图。
        draft.vegetation = Some(MapVegetationAsset {
            map_id: map.clone(), // 关闭目标。
            schema_version: 1,   // 当前协议。
            layers: vec![],
            authoring: None, // 清空草地同样可能触发后台更新。
        });
        draft
            .vegetation_dirty_layers
            .insert("ordinary-grass".into()); // 草稿仍未保存。
        let mut app = App::new(); // 无窗口运行真正的关闭系统。
        app.insert_resource(EditorAuthoringSession {
            draft: Some(draft),     // 当前会话必须跨关闭保留。
            vegetation_revision: 5, // 最新作者笔划。
            vegetation_compiled_revision: if compiled { 5 } else { 4 }, // 两种后台阶段。
            ..default()             // 没有保存任务。
        })
        .insert_resource(MapEditorState {
            open: true,
            close_requested: true,
            ..default() // 用户直接按F12。
        })
        .insert_resource(ActiveDynamicMap {
            map_id: Some(map.clone()),
        }) // 当前玩法地图。
        .insert_resource(State::new(feathrise_scene_core::state::ScenePhase::Active)) // 正式动态场景。
        .insert_resource(DynamicVegetationStatus {
            map_id: Some(map),
            phase: DynamicVegetationPhase::Building, // 候选尚未采用。
            detail: "植被候选已完整就绪，正在等待同代地形".into(), // 重现截图。
            ..default()                              // 未产生正式采用回执。
        })
        .init_resource::<TerrainBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<TerrainLiveApplyState>()
        .init_resource::<WaterPreviewState>()
        .init_resource::<WaterFileHandoff>()
        .init_resource::<BoundaryEditState>()
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<MapCreationTask>()
        .init_resource::<MapDeletionTask>() // 既有地形、水域及保存门保持不变。
        .init_resource::<feathrise_scene::TerrainBackfacesPreview>()
        .add_systems(Update, finish_map_editor_close); // 正式入口，不使用替身。
        app.world_mut().resource_mut::<feathrise_scene::TerrainBackfacesPreview>().enabled = true;
        app.update(); // 单帧完成关闭，不等待植被后台阶段。
        assert!(!app.world().resource::<MapEditorState>().open); // 草地不能锁住F12。
        assert!(app.world().resource::<feathrise_scene::TerrainBackfacesPreview>().enabled); // 关闭F12不撤销显示偏好。
        let session = app.world().resource::<EditorAuthoringSession>(); // 原会话仍然可供后台读取。
        assert!(!session.saving); // 不暗中保存。
        assert!(
            session
                .draft
                .as_ref()
                .unwrap()
                .vegetation_dirty_layers
                .contains("ordinary-grass")
        ); // 未保存修改完整保留。
    }
}
