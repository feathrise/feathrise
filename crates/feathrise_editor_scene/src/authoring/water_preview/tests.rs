//! 水域预览允许逐点编辑中间态的回归测试。

use feathrise_scene_content::{
    DEFAULT_SCENE_WATER_PERCEPTUAL_ROUGHNESS, SCENE_SCHEMA_VERSION, SceneId, SceneWaterAsset,
    SceneWaterRiver, SceneWaterSegmentKind, SceneWaterSplinePoint,
};

use super::super::water_network::validate_water_network_edit;
use super::build::append_completed_spline_builds;

#[test]
fn incomplete_spline_is_a_valid_preview_intermediate_without_mesh() {
    let scene_id = SceneId::parse("MAP-preview-water").expect("测试场景ID合法");
    let water = SceneWaterAsset {
        schema_version: SCENE_SCHEMA_VERSION,
        scene_id: scene_id.clone(),
        layers: Vec::new(),
        rivers: vec![SceneWaterRiver {
            id: "river-editing".to_owned(),
            label: "正在绘制的河流".to_owned(),
            source_layer_id: None,
            target_layer_id: None,
            points: vec![SceneWaterSplinePoint {
                id: "point-1".to_owned(),
                position: [0.0, 2.0, 0.0],
                width: 6.0,
                depth: 1.5,
                flow_speed: 2.0,
                outgoing: SceneWaterSegmentKind::River,
            }],
            base_color_srgba: [0.055, 0.24, 0.29, 0.72],
            perceptual_roughness: DEFAULT_SCENE_WATER_PERCEPTUAL_ROUGHNESS, // 编辑中河流沿用清晰反射默认值。
        }],
        waterfalls: Vec::new(),
    };
    let mut builds = Vec::new();

    validate_water_network_edit(&water, &scene_id).expect("单点路径应允许继续编辑");
    append_completed_spline_builds(&water, &mut builds).expect("单点路径应被预览安全跳过");

    assert!(builds.is_empty());
}

fn clipmap_fixture(live: bool) -> bevy::prelude::App {
    use bevy::prelude::*;
    use super::super::resources::{EditorAuthoringSession, WaterBrushStroke, WaterPreviewState};
    let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
    draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
    let map = draft.scene_id.clone();
    let definition = feathrise_scene_content::SceneWaterAsset { scene_id: map.clone(), ..default() };
    draft.water = Some(definition.clone());
    let mut published = feathrise_scene::DynamicClipmapWaterDraft::default();
    published.publish(feathrise_scene::DynamicClipmapWaterDraftSnapshot {
        source_revision: 8, definition, sources: default(), masks: default(),
    });
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .insert_resource(crate::resources::SceneEditorState { open: true, selected_scene_id: Some(map.clone()), ..default() })
        .insert_resource(EditorAuthoringSession { draft: Some(draft), water_revision: 8, ..default() })
        .insert_resource(feathrise_scene_content::ActiveDynamicScene { scene_id: Some(map.clone()) })
        .insert_resource(published)
        .insert_resource(feathrise_scene::DynamicWaterStatus { scene_id: Some(map), input_draft_revision: Some(1),
            phase: feathrise_scene::DynamicWaterPhase::Ready, desired_generation: 2, applied_generation: 2, ..default() })
        .init_resource::<WaterBrushStroke>().init_resource::<WaterPreviewState>()
        .init_resource::<Assets<Image>>();
    if live {
        app.init_resource::<super::super::live_water_dynamic::LiveClipmapWaterState>()
            .add_systems(Update, (super::super::live_water_dynamic::sync_live_clipmap_water,
                super::build::start_water_preview_rebuild).chain());
    } else { app.add_systems(Update, super::build::start_water_preview_rebuild); }
    app
}

#[test]
fn clipmap_preview_accepts_current_ready_and_reports_failed_new_generation_once() {
    use super::super::resources::WaterPreviewState;
    use crate::resources::SceneEditorState;
    let mut app = clipmap_fixture(false);
    app.update();
    let map = app.world().resource::<feathrise_scene_content::ActiveDynamicScene>().scene_id.clone().unwrap();
    assert!(app.world().resource::<WaterPreviewState>().ready_for(&map, 8, 0));
    {
        let mut status = app.world_mut().resource_mut::<feathrise_scene::DynamicWaterStatus>();
        status.desired_generation = 3;
        status.phase = feathrise_scene::DynamicWaterPhase::Failed;
        status.detail = "测试GPU上传失败".into();
    }
    app.update();
    assert!(app.world().resource::<WaterPreviewState>().failed_for(&map, 8, 0),
        "失败的新generation不能等待它永远不会拥有的applied_generation");
    assert!(!app.world().resource::<WaterPreviewState>().ready_for(&map, 8, 0));
    assert!(app.world().resource::<SceneEditorState>().status.contains("测试GPU上传失败"));
    app.world_mut().resource_mut::<SceneEditorState>().status = "另一项操作".into();
    app.update();
    assert_eq!(app.world().resource::<SceneEditorState>().status, "另一项操作");
    {
        let mut status = app.world_mut().resource_mut::<feathrise_scene::DynamicWaterStatus>();
        status.phase = feathrise_scene::DynamicWaterPhase::Ready;
        status.applied_generation = 3;
    }
    app.update();
    assert!(!app.world().resource::<WaterPreviewState>().failed_for(&map, 8, 0));
}

#[test]
fn mapped_root_and_clipmap_ready_never_hide_added_formal_water() {
    use bevy::prelude::*;
    use super::super::resources::{EditorAuthoringSession, WaterPreviewState};
    for independent_root in [false, true] {
        let mut app = clipmap_fixture(false);
        let map = app.world().resource::<feathrise_scene_content::ActiveDynamicScene>().scene_id.clone().unwrap();
        if independent_root {
            app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
                .manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
            app.world_mut().resource_mut::<feathrise_scene::DynamicClipmapWaterDraft>()
                .publish_root(feathrise_scene::DynamicClipmapWaterDraftSnapshot {
                    source_revision: 8, definition: feathrise_scene_content::SceneWaterAsset { scene_id: map.clone(), ..default() },
                    sources: default(), masks: default(),
                });
        }
        app.add_systems(Update, super::suppress_new_runtime_water_surfaces.after(super::build::start_water_preview_rebuild));
        let surface = app.world_mut().spawn((feathrise_scene::PaintedWaterSurface {
            scene_id: map.clone(), layer_id: "formal-lake".into(), kind: feathrise_scene::WaterSurfaceKind::Still,
            purpose: feathrise_scene::PaintedWaterPurpose::Runtime,
        }, Visibility::Inherited)).id();
        app.update();
        let preview = app.world().resource::<WaterPreviewState>();
        assert!(preview.ready_for(&map, 8, 0), "测试必须先经过真实正式Ready映射");
        assert!(preview.replacement_source.is_none());
        assert!(preview.hidden_runtime_entities.is_empty());
        assert_eq!(app.world().get::<Visibility>(surface), Some(&Visibility::Inherited),
            "Ready映射没有EditorWaterPreview替代物，不能隐藏刚采用的正式水");
    }
}

#[test]
fn object_clipmap_preview_does_not_use_the_ready_scene_container_before_author_handoff() {
    use super::super::resources::{EditorAuthoringSession, WaterPreviewState};
    let mut app = clipmap_fixture(false);
    let map = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
        object.id = feathrise_scene_content::TerrainId::parse("preview-object").unwrap();
        let source = object.source_manifest(&draft.manifest);
        draft.terrain_documents.active = Some(super::super::terrain_documents::TerrainDocumentIdentity {
            definition: std::sync::Arc::new(object.clone()), manifest: source, folder: draft.folder.clone(),
        });
        draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
        draft.manifest.terrains = Some(std::sync::Arc::from([object]));
        draft.scene_id.clone()
    };
    app.init_resource::<feathrise_scene::TerrainObjectAuthoringTarget>();
    app.update();
    assert!(!app.world().resource::<WaterPreviewState>().ready_for(&map, 8, 0),
        "真实TerrainObjectResource在无已应用对象时不能退回同图根Ready");
}

#[test]
fn clipmap_prepublication_failure_marks_exact_author_revision_without_clearing_old_output() {
    use super::super::resources::{EditorAuthoringSession, WaterPreviewState};
    let mut app = clipmap_fixture(true);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.water_revision = 9;
        let draft = session.draft.as_mut().unwrap();
        draft.water = Some(crate::authoring::water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder));
        // 故意缺R8，使用正式单飞准备worker产生确定失败，上一份已发布版本仍可显示。
        draft.water_masks.clear();
    }
    let map = app.world().resource::<feathrise_scene_content::ActiveDynamicScene>().scene_id.clone().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !app.world().resource::<WaterPreviewState>().failed_for(&map, 9, 0) {
        assert!(std::time::Instant::now() < deadline, "发布前失败不能永久停留生成中");
        app.update(); std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert_eq!(app.world().resource::<feathrise_scene::DynamicClipmapWaterDraft>().for_scene(&map).unwrap().source_revision, 8);
    assert!(app.world().resource::<super::super::live_water_dynamic::LiveClipmapWaterState>().failure_for(&map, 8).is_none());
}

#[test]
fn changed_water_level_save_reports_the_exact_wait_and_resumes_only_after_current_adoption() {
    use bevy::prelude::*;
    use super::super::resources::*;
    use crate::resources::{SceneDeletionTask, SceneEditorState};
    let mut app = clipmap_fixture(false);
    app.update();
    let (map, definition) = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.water_revision = 9; // 模拟水位输入已经推进到新修订，旧运行时Ready不能放行。
        let draft = session.draft.as_mut().unwrap();
        draft.water = Some(crate::authoring::water_asset_with_layer_for_tests(&draft.scene_id, &draft.folder));
        (draft.scene_id.clone(), draft.water.clone().unwrap())
    };
    app.world_mut().resource_mut::<feathrise_scene::DynamicClipmapWaterDraft>()
        .publish(feathrise_scene::DynamicClipmapWaterDraftSnapshot {
            source_revision: 9, definition, sources: default(), masks: default(),
        });
    app.init_asset::<bevy::world_serialization::WorldAsset>()
        .init_resource::<DraftSaveTask>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<GlobalEnvironmentSaveTask>()
        .init_resource::<super::super::vegetation_compile::VegetationCompileState>()
        .init_resource::<SceneDeletionTask>()
        .init_resource::<feathrise_scene::ScenePlacementRuntimeStatus>()
        .init_resource::<TerrainPreviewState>()
        .init_resource::<TerrainLiveApplyState>()
        .init_resource::<super::super::PlacementTerrainFollowState>()
        .init_resource::<crate::PlacementInstanceIndex>()
        .init_resource::<super::super::TerrainBrushStroke>()
        .init_resource::<super::super::water_handoff::WaterFileHandoff>()
        .init_resource::<PlacementWaterEvidenceRegistry>()
        .add_systems(Update, (
            super::super::reconcile_following_placements,
            super::super::start_draft_save,
        ).chain().after(super::build::start_water_preview_rebuild));
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.save_requested = true;
        editor.close_requested = true;
        // 用干净标记截在正式IO前；本测试只验真实保存入口的预检，不触碰工作区场景文件。
        editor.dirty = false;
    }
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.save_requested && editor.close_requested);
    assert!(editor.status.contains("发布代次Some(2)，运行时代次Some(1)"), "{}", editor.status);
    assert!(app.world().resource::<DraftSaveTask>().task.is_none());
    {
        let mut status = app.world_mut().resource_mut::<feathrise_scene::DynamicWaterStatus>();
        status.input_draft_revision = Some(2);
        status.phase = feathrise_scene::DynamicWaterPhase::Building;
        status.desired_generation = 3;
        status.detail = "正在等待静水GPU材质预热回执".into();
    }
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.save_requested);
    assert!(editor.status.contains("静水GPU材质预热回执"), "{}", editor.status);
    assert!(!editor.status.contains("GLB")); // Clipmap不能再冒报成等待不存在的GLB构建。
    let preview = app.world().resource::<WaterPreviewState>();
    assert!(preview.waiting_detail_for(&map, 8, 0).is_none()); // 旧水位不借当前进度信用。
    {
        let mut status = app.world_mut().resource_mut::<feathrise_scene::DynamicWaterStatus>();
        status.phase = feathrise_scene::DynamicWaterPhase::Ready;
        status.applied_generation = 3;
    }
    app.update();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(!editor.save_requested, "{}", editor.status);
    assert_eq!(editor.status, "当前场景没有未保存变更");
    assert!(app.world().resource::<WaterPreviewState>().ready_for(&map, 9, 0));
    assert!(app.world().resource::<DraftSaveTask>().task.is_none());
}
