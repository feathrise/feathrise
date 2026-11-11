//! 普通模型院落模板热采用必须先形成完整作者组，再释放正式场景保存请求；全程零资产写入。

use super::*;
use crate::authoring::{
    placement_grounding::PlacementTerrainFollowState,
    resources::{
        DraftSaveTask, GlobalEnvironmentAuthoringState, GlobalEnvironmentSaveTask,
        PlacementWaterEvidenceRegistry, TerrainLiveApplyState, TerrainPreviewState,
        WaterBrushStroke, WaterPreviewState,
    },
    sparse_terrain::SparseTerrainAuthoringState,
    start_draft_save, start_sparse_terrain_save,
    vegetation_compile::VegetationCompileState,
    water_handoff::WaterFileHandoff,
};
use crate::resources::{SceneDeletionTask, SceneEditorState};
use crate::ui::courtyard::CourtyardUiState;
use bevy::{asset::AssetPlugin, ecs::system::RunSystemOnce, world_serialization::WorldAsset};
use feathrise_scene_content::{
    ActiveDynamicScene, COURTYARD_PRESET_SCHEMA_VERSION, CourtyardPresetCatalogAsset,
    CourtyardPresetDescriptor, CourtyardPresetRegistry, ScenePlacementHeightMode, SceneTerrainKind,
    ResolvedCourtyardPresetCatalog,
};
use feathrise_scene::{DynamicWaterStatus, ScenePlacementRuntimeStatus};

#[path = "visual_sync_tests.rs"]
mod visual_sync_tests; // 整组删除历史必须通过真实有界队列消费后仍保护预览所有权和正式根。

fn member(id: &str, x: f32) -> ScenePlacementRecord {
    ScenePlacementRecord {
        id: id.into(),
        asset_path: "gltf/tables.glb".into(),
        asset_target: default(),
        translation: [x, 0.0, 5.0],
        yaw_degrees: 0.0,
        scale: [1.0; 3],
        height_mode: ScenePlacementHeightMode::FixedHeight,
        terrain_offset: 0.0,
        building_preset_id: None,
        courtyard: None,
        water_surfaces: Vec::new(),
        terrain_binding: None,
        terrain_patch: None,
        mesh_lod: None,
        editor: default(),
    }
}

fn app() -> (App, CourtyardPresetDescriptor) {
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let records = vec![member("plain-a", 3.0), member("plain-b", 7.0)];
    let (preset, origin, yaw_degrees) = super::super::geometry::preset_from_placements(
        "yard-pending".into(),
        "普通模型小院".into(),
        &records,
    )
    .unwrap();
    let adoption = CatalogAdoption {
        baseline_revision: 1,
        preset_id: preset.id.clone(),
        scene_id: draft.scene_id.clone(),
        author_revision: 7,
        structure_revision: 0,
        selected: records.iter().cloned().enumerate().collect(),
        origin,
        yaw_degrees,
    };
    draft.placements.placements = records;
    let map = draft.scene_id.clone();
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<WorldAsset>()
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 7,
            ..default()
        })
        .insert_resource(SceneEditorState {
            open: true,
            dirty: true,
            save_requested: true,
            selected_scene_id: Some(map),
            ..default()
        })
        .insert_resource(CourtyardAuthoringState {
            catalog_task: Some(CatalogTask::AwaitAdoption(adoption)),
            ..default()
        })
        .insert_resource(CourtyardPresetRegistry {
            catalog: Handle::default(),
            revision: 1,
            resolved: ResolvedCourtyardPresetCatalog::default(),
            last_error: None,
        })
        .init_resource::<CourtyardUiState>()
        .init_resource::<DraftSaveTask>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<GlobalEnvironmentSaveTask>()
        .init_resource::<VegetationCompileState>()
        .init_resource::<SceneDeletionTask>()
        .init_resource::<ActiveDynamicScene>()
        .init_resource::<DynamicWaterStatus>()
        .init_resource::<ScenePlacementRuntimeStatus>()
        .init_resource::<WaterPreviewState>()
        .init_resource::<TerrainPreviewState>()
        .init_resource::<TerrainLiveApplyState>()
        // 热采用后的保存须继续到真实下一门；故意未授予重贴地信用，确保不启动用户资产IO。
        .init_resource::<PlacementTerrainFollowState>()
        .init_resource::<WaterFileHandoff>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<PlacementWaterEvidenceRegistry>()
        .add_systems(
            Update,
            (super::super::catalog::poll_catalog_save, start_draft_save).chain(),
        );
    (app, preset)
}

fn adopt_catalog(app: &mut App, preset: CourtyardPresetDescriptor) {
    let asset = CourtyardPresetCatalogAsset {
        schema_version: COURTYARD_PRESET_SCHEMA_VERSION,
        presets: vec![preset],
    };
    let resolved = ResolvedCourtyardPresetCatalog::build(&asset).unwrap();
    let mut registry = app.world_mut().resource_mut::<CourtyardPresetRegistry>();
    registry.resolved = resolved;
    registry.revision = 2; // 模拟正式注册表已完整校验热更，而不是直接给作者记录补组元数据。
}

#[test]
fn plain_model_courtyard_awaits_catalog_adoption_before_the_real_scene_save_gate_continues() {
    let (mut app, preset) = app();
    let before = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .clone();
    assert!(
        before
            .iter()
            .all(|record| record.building_preset_id.is_none())
    );
    let state = app.world().resource::<CourtyardAuthoringState>();
    assert!(!state.operation_pending() && state.save_pending());
    for _ in 0..3 {
        app.update(); // 正式目录采用尚未完成；零建筑不能越过已受理的成员成组事务。
        let editor = app.world().resource::<SceneEditorState>();
        assert!(editor.save_requested);
        assert!(
            editor.status.contains("院落模板"),
            "保存应等待模板和成员成组：{}",
            editor.status
        );
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(!session.saving && session.undo.is_empty());
        assert_eq!(session.revision, 7);
        assert_eq!(
            session.draft.as_ref().unwrap().placements.placements,
            before
        );
        let save = app.world().resource::<DraftSaveTask>();
        assert!(save.task.is_none());
        assert_eq!(save.ticket, 0);
    }
    adopt_catalog(&mut app, preset.clone());
    app.update(); // 运行真正poll_catalog_save，原成员同时成组并写一条完整作者历史。
    let state = app.world().resource::<CourtyardAuthoringState>();
    assert!(!state.save_pending());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(!session.saving);
    assert_eq!(session.undo.len(), 1);
    assert_eq!(session.revision, 8);
    assert_eq!(session.placement_structure_revision, 1);
    for (ordinal, record) in session
        .draft
        .as_ref()
        .unwrap()
        .placements
        .placements
        .iter()
        .enumerate()
    {
        let membership = record
            .courtyard
            .as_ref()
            .expect("热采用后原位置成员须获得同一院落身份");
        assert_eq!(membership.preset_id, preset.id);
        assert_eq!(membership.instance_id, "courtyard-instance-yard-pending");
        assert_eq!(membership.member_id, format!("member-{}", ordinal + 1));
        assert_eq!(record.translation, before[ordinal].translation);
        assert_eq!(record.asset_path, before[ordinal].asset_path);
        assert!(record.building_preset_id.is_none());
    }
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.dirty && editor.save_requested);
    assert!(
        editor.status.contains("跟随型资产"),
        "院落采用后正式保存应自动进入下一完成门：{}",
        editor.status
    );
    let save = app.world().resource::<DraftSaveTask>();
    assert!(save.task.is_none());
    assert_eq!(save.ticket, 0); // 测试确证保存继续预检，同时完全不触及真实场景文件。
}

#[test]
fn plain_model_courtyard_catalog_adoption_blocks_the_real_sparse_height_save_claim() {
    let (mut app, preset) = app();
    app.init_resource::<SparseTerrainAuthoringState>();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.sparse_terrain_dirty = true;
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.terrain.kind = SceneTerrainKind::SparseClipmap;
        draft.manifest.terrain.glb_path.clear();
        draft.manifest.terrain.height_path = Some(format!(
            "scenes/{}/terrain/height_tiles.scene_height_manifest.json",
            draft.scene_id,
        ));
        draft.manifest.terrain.collision_lods =
            feathrise_scene_content::default_collision_lods_for_resolution(
                draft.manifest.terrain.resolution,
            );
    }
    for _ in 0..3 {
        app.world_mut()
            .run_system_once(start_sparse_terrain_save)
            .unwrap();
        assert!(app.world().resource::<SceneEditorState>().save_requested);
        assert!(!app.world().resource::<EditorAuthoringSession>().saving);
        // 公开停放完成门为true同时证明没有task、pending_save、源借用或笔划，不开放测试专用内部字段。
        assert!(
            app.world()
                .resource::<SparseTerrainAuthoringState>()
                .document_can_park()
        );
        assert!(app.world().resource::<DraftSaveTask>().task.is_none());
    }
    adopt_catalog(&mut app, preset);
    app.update();
    assert!(
        !app.world()
            .resource::<CourtyardAuthoringState>()
            .save_pending()
    );
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
    app.world_mut()
        .run_system_once(start_sparse_terrain_save)
        .unwrap();
    assert!(!app.world().resource::<SceneEditorState>().save_requested);
    assert!(app.world().resource::<EditorAuthoringSession>().saving);
    assert!(
        !app.world()
            .resource::<SparseTerrainAuthoringState>()
            .document_can_park()
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("稀疏高度清单尚未载入")
    );
    assert!(app.world().resource::<DraftSaveTask>().task.is_none());
    // 作者源保持默认未载入，正式保存只领取请求并等来源，不产生任何文件任务。
}

#[test]
fn a_completed_courtyard_visual_queue_does_not_reclaim_the_scene_save_gate() {
    let (mut app, preset) = app();
    adopt_catalog(&mut app, preset);
    app.update();
    let mut state = app
        .world_mut()
        .remove_resource::<CourtyardAuthoringState>()
        .unwrap();
    {
        let session = app.world().resource::<EditorAuthoringSession>();
        let record = session.draft.as_ref().unwrap().placements.placements[0].clone();
        queue_visual_sync(&mut state, session, vec![(0, record, true)]);
    }
    assert!(state.visual_sync_pending());
    assert!(!state.save_pending());
    app.insert_resource(state);
    app.update();
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .status
            .contains("跟随型资产")
    );
    assert!(app.world().resource::<DraftSaveTask>().task.is_none());
}
