//! 真实GLB后台派生状态机回归：空容器不能冒充对象规格，同一失败不得逐帧重算。

use std::time::{Duration, Instant};

use bevy::ecs::system::RunSystemOnce;
use feathrise_scene_content::{SceneTerrainObject, TerrainId};

use super::*;
use crate::authoring::terrain_documents::TerrainDocumentIdentity;

/// 作者网格仍来自完整11×11夹具，根清单改为空容器，只让所选对象提供采样合同。
fn object_draft() -> super::super::super::resources::EditorSceneDraft {
    let mut draft = test_heightfield_draft(Arc::new(vec![1_000; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse("glb-derive-a").unwrap();
    // 容器边界有意与对象不同；只改分辨率却仍读取根范围也必须被真实采样发现。
    draft.manifest.world_min = [-80.0, -120.0];
    draft.manifest.world_max = [180.0, 220.0];
    draft.manifest.terrain = feathrise_scene_content::SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        manifest: object.source_manifest(&draft.manifest),
        folder: object.private_folder(&draft.scene_id),
        definition: Arc::new(object),
    });
    draft.terrain_dirty = true; // 使用未保存候选路径，验证失败与恢复的实际状态发布。
    draft
}

fn app_with_draft(draft: super::super::super::resources::EditorSceneDraft) -> App {
    let scene_id = draft.scene_id.clone();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(SceneEditorState {
            open: true,
            selected_scene_id: Some(scene_id),
            save_requested: true,
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            terrain_revision: 2,
            ..default()
        })
        .init_resource::<TerrainBrushStroke>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<TerrainPreviewState>()
        .init_resource::<TerrainLiveApplyState>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<Image>>();
    app
}

/// 只在测试中等待worker；生产系统仍按真实非阻塞check_ready推进。
fn advance_until(app: &mut App, done: impl Fn(&World) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !done(app.world()) {
        app.update();
        assert!(
            Instant::now() < deadline,
            "真实地形派生任务未在测试期限内收口"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn install_rebuild_systems(app: &mut App) {
    app.add_systems(
        Update,
        (start_terrain_preview_rebuild, poll_terrain_preview_rebuild).chain(),
    );
}

fn ready(world: &World) -> bool {
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    world
        .resource::<TerrainPreviewState>()
        .ready_for(&draft.scene_id, session.terrain_revision)
}

#[test]
fn adding_glb_to_empty_container_derives_selected_object_spec_and_stays_stable() {
    let draft = object_draft();
    let map = draft.scene_id.clone();
    assert_eq!(draft.manifest.terrain.resolution, [0, 0]);
    assert_eq!(draft.terrain_manifest().terrain.resolution, [11, 11]);
    let mut app = app_with_draft(draft);
    install_rebuild_systems(&mut app);
    advance_until(&mut app, ready);

    let preview = app.world().resource::<TerrainPreviewState>();
    let mesh_handle = preview.mesh.clone().unwrap();
    let image_handle = preview.height_image.clone().unwrap();
    let image = app
        .world()
        .resource::<Assets<Image>>()
        .get(&image_handle)
        .unwrap();
    assert_eq!(image.texture_descriptor.size.width, 10);
    assert_eq!(image.texture_descriptor.size.height, 10);
    let heights = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .heights
        .clone()
        .unwrap();
    assert_eq!(heights.len(), 121);
    let mesh_count = app.world().resource::<Assets<Mesh>>().len();
    let image_count = app.world().resource::<Assets<Image>>().len();
    for _ in 0..24 {
        app.update();
        let preview = app.world().resource::<TerrainPreviewState>();
        assert!(preview.task.is_none(), "已采用输入不能在稳定帧再次派生");
        assert_eq!(preview.mesh.as_ref(), Some(&mesh_handle));
        assert_eq!(preview.height_image.as_ref(), Some(&image_handle));
    }
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), mesh_count);
    assert_eq!(app.world().resource::<Assets<Image>>().len(), image_count);
    assert!(Arc::ptr_eq(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .heights
            .as_ref()
            .unwrap(),
        &heights
    ));
    assert!(
        app.world().resource::<SceneEditorState>().save_requested,
        "派生系统不能消费保存意图"
    );
    assert!(
        !app.world()
            .resource::<TerrainLiveApplyState>()
            .has_candidate(&map, 2),
        "独立对象预览不能接管旧单地形运行链"
    );
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let mesh = session
        .draft
        .as_mut()
        .unwrap()
        .terrain_mesh
        .as_mut()
        .unwrap();
    assert!(
        Arc::get_mut(mesh).is_some(),
        "稳定身份不能持有Arc或Weak阻塞下一笔作者写侧"
    );
}

#[test]
fn invalid_glb_spec_records_one_failure_and_does_not_restart_stable_frames() {
    let mut draft = object_draft();
    draft.terrain_manifest_mut().terrain.resolution = [0, 0];
    let map = draft.scene_id.clone();
    let heights = draft.heights.clone().unwrap();
    let mut app = app_with_draft(draft);
    install_rebuild_systems(&mut app);
    advance_until(&mut app, |world| {
        world
            .resource::<TerrainLiveApplyState>()
            .failed_for(&map, 2)
    });
    let status = app.world().resource::<SceneEditorState>().status.clone();
    assert!(status.contains("高度分辨率"));
    for _ in 0..24 {
        app.update();
        assert!(
            app.world().resource::<TerrainPreviewState>().task.is_none(),
            "同一失败输入不能再次占用CPU任务槽"
        );
        assert_eq!(app.world().resource::<SceneEditorState>().status, status);
    }
    assert!(!ready(app.world()));
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
    assert!(
        Arc::ptr_eq(
            app.world()
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .unwrap()
                .heights
                .as_ref()
                .unwrap(),
            &heights
        ),
        "失败不得替换上一完整查询高度"
    );
}

#[test]
fn changing_failed_object_spec_retries_and_recovers_even_at_the_same_author_revision() {
    let mut draft = object_draft();
    draft.terrain_manifest_mut().terrain.resolution = [0, 0];
    let map = draft.scene_id.clone();
    let mut app = app_with_draft(draft);
    install_rebuild_systems(&mut app);
    advance_until(&mut app, |world| {
        world
            .resource::<TerrainLiveApplyState>()
            .failed_for(&map, 2)
    });
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .terrain_manifest_mut()
        .terrain
        .resolution = [11, 11];
    // 清单热更或恢复画布可以只改变输入规格；失败缓存必须比较真实输入而非仅修订数字。
    advance_until(&mut app, ready);
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .terrain_revision,
        2
    );
    assert!(
        !app.world()
            .resource::<TerrainLiveApplyState>()
            .failed_for(&map, 2)
    );
    assert!(
        !app.world()
            .resource::<TerrainLiveApplyState>()
            .has_candidate(&map, 2)
    );
}

#[test]
fn nan_sampling_parameter_records_one_stable_failure_without_restarting() {
    let mut draft = object_draft();
    draft.terrain_manifest_mut().terrain.height_scale = f32::from_bits(0x7fc0_0042);
    let map = draft.scene_id.clone();
    let mut app = app_with_draft(draft);
    install_rebuild_systems(&mut app);
    advance_until(&mut app, |world| {
        world
            .resource::<TerrainLiveApplyState>()
            .failed_for(&map, 2)
    });
    let status = app.world().resource::<SceneEditorState>().status.clone();
    assert!(status.contains("地形GLB派生失败"));
    // NaN不能直接用浮点相等判定身份；保持相同位模式的坏输入必须命中同一失败缓存。
    for _ in 0..24 {
        app.update();
        assert!(app.world().resource::<TerrainPreviewState>().task.is_none());
        assert!(
            app.world()
                .resource::<TerrainPreviewState>()
                .failed_identity
                .is_some()
        );
        assert_eq!(app.world().resource::<SceneEditorState>().status, status);
    }
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
}

fn replace_active_object(draft: &mut super::super::super::resources::EditorSceneDraft, id: &str) {
    let mut object: SceneTerrainObject =
        (*draft.terrain_documents.active.as_ref().unwrap().definition).clone();
    object.id = TerrainId::parse(id).unwrap();
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        manifest: object.source_manifest(&draft.manifest),
        folder: object.private_folder(&draft.scene_id),
        definition: Arc::new(object),
    });
}

#[test]
fn gltf_object_with_stale_heightfield_buffers_does_not_enter_height_derivation() {
    let mut draft = object_draft();
    draft.terrain_manifest_mut().terrain.kind = SceneTerrainKind::Gltf;
    draft.terrain_manifest_mut().terrain.resolution = [0, 0];
    let heights = draft.heights.clone().unwrap();
    let mut app = app_with_draft(draft);
    install_rebuild_systems(&mut app);
    for _ in 0..24 {
        app.update();
        assert!(app.world().resource::<TerrainPreviewState>().task.is_none());
    }
    assert!(
        app.world()
            .resource::<TerrainLiveApplyState>()
            .failed_scene_id
            .is_none()
    );
    assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
    assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
    assert!(Arc::ptr_eq(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .heights
            .as_ref()
            .unwrap(),
        &heights
    ));
}

#[test]
fn late_success_and_failure_from_another_terrain_id_cannot_publish_into_current_canvas() {
    for old_result_fails in [false, true] {
        let mut draft = object_draft();
        if old_result_fails {
            draft.terrain_manifest_mut().terrain.resolution = [0, 0];
        }
        let mut app = app_with_draft(draft);
        app.add_systems(Update, poll_terrain_preview_rebuild);
        app.world_mut()
            .run_system_once(start_terrain_preview_rebuild)
            .unwrap();
        assert!(app.world().resource::<TerrainPreviewState>().task.is_some());
        let sentinel = Arc::new(vec![12_345; 121]);
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            replace_active_object(draft, "glb-derive-b");
            draft.heights = Some(sentinel.clone());
            // 同场景、同作者修订且源Mesh相同，只有稳定对象ID发生变化。
        }
        let status = app.world().resource::<SceneEditorState>().status.clone();
        advance_until(&mut app, |world| {
            world.resource::<TerrainPreviewState>().task.is_none()
        });
        assert!(Arc::ptr_eq(
            app.world()
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .unwrap()
                .heights
                .as_ref()
                .unwrap(),
            &sentinel
        ));
        assert_eq!(app.world().resource::<Assets<Mesh>>().len(), 0);
        assert_eq!(app.world().resource::<Assets<Image>>().len(), 0);
        assert!(
            app.world()
                .resource::<TerrainPreviewState>()
                .entity
                .is_none()
        );
        assert!(
            app.world()
                .resource::<TerrainLiveApplyState>()
                .failed_scene_id
                .is_none(),
            "旧对象的错误不得阻止当前对象"
        );
        assert_eq!(app.world().resource::<SceneEditorState>().status, status);
        // 迟到结果的单槽回收也由start轮询；恢复正式每帧调度，不能只尝试启动一次。
        app.add_systems(
            Update,
            start_terrain_preview_rebuild.before(poll_terrain_preview_rebuild),
        );
        advance_until(&mut app, ready);
        assert!(app.world().resource::<SceneEditorState>().save_requested);
    }
}
