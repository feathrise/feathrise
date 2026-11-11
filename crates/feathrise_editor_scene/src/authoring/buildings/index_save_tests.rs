//! 保存请求跨越新增网格的真实地面快照空窗；使用生产索引调度与Collider五点审计。

use super::*;
use crate::authoring::terrain_ground::TerrainGroundSnapshot;
use feathrise_scene_content::{
    BUILDING_PRESET_SCHEMA_VERSION, BuildingPresetCatalogAsset, BuildingPresetDescriptor,
    SceneCollisionLod, ScenePlacementHeightMode, ScenePlacementRecord, SceneTerrainCollection,
    SceneTerrainDescriptor, SceneTerrainKind, SceneTerrainObject, ResolvedBuildingPresetCatalog,
    SparseHeightSpec, TerrainId, TerrainObjectTransform,
};
use std::time::{Duration, Instant};

fn app() -> App {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.terrain = SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Default::default());
    draft.manifest.world_max = [100.; 2]; // 完整房屋占地与门前空间均位于作者容器内。
    draft.terrain_mesh = None;
    draft.terrain_editing_index = None;
    draft.heights = None;
    draft.terrain_ground = Some(TerrainGroundSnapshot::empty(draft.scene_id.clone(), 1));
    let preset = BuildingPresetDescriptor::from_asset(
        "house".into(),
        "地基民居".into(),
        "gltf/house.glb".into(),
        Default::default(),
    );
    let registry = BuildingPresetRegistry {
        catalog: Handle::default(),
        revision: 1,
        resolved: ResolvedBuildingPresetCatalog::build(&BuildingPresetCatalogAsset {
            schema_version: BUILDING_PRESET_SCHEMA_VERSION,
            presets: vec![preset],
        })
        .unwrap(),
        last_error: None,
    };
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(SceneEditorState {
            open: true,
            save_requested: true,
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 7,
            terrain_revision: 1,
            ..default()
        })
        .insert_resource(registry)
        .init_resource::<BuildingAuthoringState>()
        .init_resource::<BuildingPresetEditorState>()
        .init_resource::<CourtyardUiState>()
        .add_systems(Update, (poll_index_build, advance_index_gather).chain());
    app
}

fn readiness(app: &App) -> super::super::BuildingSaveReadiness {
    super::super::save_readiness(
        app.world().resource::<EditorAuthoringSession>(),
        Some(app.world().resource::<BuildingPresetRegistry>()),
        Some(app.world().resource::<BuildingAuthoringState>()),
        None,
    )
}

fn finish_index(app: &mut App, count: usize) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        let expected = current_identity(
            app.world().resource::<EditorAuthoringSession>(),
            app.world().resource::<BuildingPresetRegistry>(),
        )
        .unwrap();
        let state = app.world().resource::<BuildingAuthoringState>();
        assert!(
            state.index_error.is_none(),
            "载入空窗不能固化索引失败：{:?}",
            state.index_error
        );
        if state.active_identity.as_ref() == Some(&expected)
            && state.index_task.is_none()
            && state.gather.is_none()
        {
            assert_eq!(state.active_index.as_ref().unwrap().len(), count);
            return;
        }
        assert!(
            Instant::now() < deadline,
            "当前真实索引必须在地面就绪后恢复"
        );
        std::thread::sleep(Duration::from_millis(1)); // 仅测试让出CPU，生产主线程始终非阻塞轮询。
    }
}

fn add_clipmap(app: &mut App, with_house: bool) {
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    session.revision += 1;
    session.terrain_revision += 1;
    if with_house {
        session.placement_structure_revision += 1;
    }
    let draft = session.draft.as_mut().unwrap();
    let object = SceneTerrainObject {clipmap_stitches: Default::default(),
        id: TerrainId::parse("new-clipmap").unwrap(),
        display_name: "新增Clipmap".into(),
        enabled: true,
        mesh_lod: None,
        transform: TerrainObjectTransform {
            translation: [20., 4., 20.],
            ..default()
        },
        world_min: [-32.; 2],
        world_max: [32.; 2],
        terrain: SceneTerrainDescriptor {resolved_stitches: Default::default(),
            kind: SceneTerrainKind::SparseClipmap,
            resolution: [257; 2],
            height_path: Some(format!(
                "scenes/{}/terrains/new-clipmap/height_tiles.scene_height_manifest.json",
                draft.scene_id
            )),
            base_height: 0.,
            height_scale: 8.,
            collision_lods: vec![SceneCollisionLod {
                max_distance_meters: 16.,
                spacing_multiplier: 1,
            }],
            ..SceneTerrainDescriptor::empty()
        },
        voxel: None,
        surface_path: None,
        vegetation_path: None,
        water_path: None,
        puddles: None,
    };
    let spec = SparseHeightSpec::new(object.world_min, object.world_max, 0.25, 0., 8.).unwrap();
    assert_eq!(
        spec.cells.map(|cells| cells as u32 + 1),
        object.terrain.resolution
    );
    SceneTerrainCollection {
        legacy: SceneTerrainDescriptor::empty(),
        objects: Some(vec![object].into()),
    }
    .apply(&mut draft.manifest);
    feathrise_scene_content::validate_terrain_objects(&draft.manifest).unwrap();
    draft.terrain_ground = None; // 对齐正式新增采用：定义已完整写入，新查询源尚未原子采用。
    if with_house {
        draft.placements.placements.push(ScenePlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: default(),
            terrain_patch: None,
            id: "house-1".into(),
            asset_path: "gltf/house.glb".into(),
            asset_target: default(),
            building_preset_id: Some("house".into()),
            courtyard: None,
            translation: [20., 4., 20.],
            height_mode: ScenePlacementHeightMode::FixedHeight,
            terrain_offset: 0.,
            yaw_degrees: 0.,
            scale: [1.; 3],
            water_surfaces: Vec::new(),
        });
    }
}

fn restore_ground(app: &mut App, generation: u64) {
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let revision = session.terrain_revision;
    let draft = session.draft.as_mut().unwrap();
    let object = &draft.manifest.terrains.as_ref().unwrap()[0];
    let collider = avian3d::prelude::Collider::trimesh(
        vec![
            Vec3::new(-32., 0., -32.),
            Vec3::new(32., 0., -32.),
            Vec3::new(32., 0., 32.),
            Vec3::new(-32., 0., 32.),
        ],
        vec![[0, 2, 1], [0, 3, 2]],
    );
    let mut ground = TerrainGroundSnapshot::test_mesh(
        draft.scene_id.clone(),
        object.id.clone(),
        object.transform,
        collider,
    );
    let writable = Arc::get_mut(&mut ground).unwrap();
    writable.revision = revision;
    writable.generation = generation;
    draft.terrain_ground = Some(ground); // 完整Collider查询到达不改变作者身份，使同修订恢复可被验证。
}

#[test]
fn empty_building_index_resumes_after_new_clipmap_ground_gap_without_poisoning_save() {
    let mut app = app();
    finish_index(&mut app, 0); // 原空图也曾建立携带旧地形来源的真实空索引。
    assert!(
        app.world()
            .resource::<BuildingAuthoringState>()
            .ground_content
            .is_some()
    );
    add_clipmap(&mut app, false);
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    for _ in 0..3 {
        app.update();
        let state = app.world().resource::<BuildingAuthoringState>();
        assert!(state.index_error.is_none() && state.ground_content.is_none());
        assert_eq!(readiness(&app), super::super::BuildingSaveReadiness::Ready);
    }
    restore_ground(&mut app, 2);
    finish_index(&mut app, 0);
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().revision,
        revision
    );
    assert_eq!(readiness(&app), super::super::BuildingSaveReadiness::Ready);
    assert_eq!(
        app.world()
            .resource::<BuildingAuthoringState>()
            .ground_content,
        Some([2, 2, 0, 0, 0, 0, 0])
    );
    assert!(app.world().resource::<SceneEditorState>().save_requested);
}

#[test]
fn real_buildings_wait_through_ground_gap_and_reaudit_changed_content_for_the_same_author() {
    let mut app = app();
    finish_index(&mut app, 0);
    add_clipmap(&mut app, true);
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    for _ in 0..3 {
        app.update();
        assert!(matches!(
            readiness(&app),
            super::super::BuildingSaveReadiness::Waiting(_)
        ));
        assert!(
            app.world()
                .resource::<BuildingAuthoringState>()
                .index_error
                .is_none()
        );
    }
    restore_ground(&mut app, 2);
    finish_index(&mut app, 1);
    assert_eq!(readiness(&app), super::super::BuildingSaveReadiness::Ready);
    restore_ground(&mut app, 3); // 内容换代而作者修订不变，也不能借旧审计信用保存。
    assert!(
        matches!(readiness(&app), super::super::BuildingSaveReadiness::Waiting(detail) if detail.contains("来源已换代"))
    );
    finish_index(&mut app, 1);
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().revision,
        revision
    );
    assert_eq!(readiness(&app), super::super::BuildingSaveReadiness::Ready);
}

#[test]
fn real_building_save_rejects_another_scenes_ground_with_equal_revision_and_generation() {
    let mut app = app();
    finish_index(&mut app, 0);
    add_clipmap(&mut app, true);
    restore_ground(&mut app, 2);
    finish_index(&mut app, 1);
    assert_eq!(readiness(&app), super::super::BuildingSaveReadiness::Ready);
    let original_scene = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let ground = session
            .draft
            .as_mut()
            .unwrap()
            .terrain_ground
            .as_mut()
            .unwrap();
        let ground = Arc::get_mut(ground).expect("已完成真实五点审计不持有地面采样器租约");
        let original_scene = ground.scene_id.clone();
        ground.scene_id = SceneId::parse("MAP-other-ground").unwrap(); // 只换场景；保留地面几何、修订、代次与作者索引身份。
        original_scene
    };
    assert!(
        matches!(readiness(&app), super::super::BuildingSaveReadiness::Waiting(detail) if detail.contains("来源已换代"))
    );
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let ground = session
            .draft
            .as_mut()
            .unwrap()
            .terrain_ground
            .as_mut()
            .unwrap();
        Arc::get_mut(ground).unwrap().scene_id = original_scene;
    }
    assert_eq!(readiness(&app), super::super::BuildingSaveReadiness::Ready);
}

#[test]
fn invalid_real_building_audit_is_failed_even_when_its_ground_source_later_changes() {
    let mut app = app();
    finish_index(&mut app, 0);
    add_clipmap(&mut app, true);
    restore_ground(&mut app, 2);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.draft.as_mut().unwrap().placements.placements[0].translation[1] = 99.;
    }
    finish_index(&mut app, 1); // 真实五点Collider审计拒绝悬空建筑，没有伪造审计错误。
    assert!(
        matches!(readiness(&app), super::super::BuildingSaveReadiness::Failed(detail) if detail.contains("根高度"))
    );
    restore_ground(&mut app, 3);
    assert!(
        matches!(readiness(&app), super::super::BuildingSaveReadiness::Failed(detail) if detail.contains("根高度"))
    );
}

#[test]
fn stale_empty_audit_and_index_errors_do_not_claim_building_save_responsibility() {
    let mut app = app();
    finish_index(&mut app, 0);
    add_clipmap(&mut app, false);
    let identity = current_identity(
        app.world().resource::<EditorAuthoringSession>(),
        app.world().resource::<BuildingPresetRegistry>(),
    )
    .unwrap();
    {
        let mut state = app.world_mut().resource_mut::<BuildingAuthoringState>();
        state.index_error = Some((identity, "旧空索引来源错误".into()));
        assert!(state.ground_content.is_some()); // 刻意保留旧版已经发生的混合状态，保存入口仍不得死等。
    }
    assert_eq!(readiness(&app), super::super::BuildingSaveReadiness::Ready);
}
