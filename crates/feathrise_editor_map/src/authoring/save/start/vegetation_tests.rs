//! 空白地图的占位植被不能阻塞保存；真实网格仍须完整编译并拒绝失败和迟到结果。

use super::*;
use crate::authoring::{
    EditorMapDraft,
    vegetation_compile::{poll_vegetation_compile, start_vegetation_compile},
};
use feathrise_map_content::{MapTerrainDescriptor, MapVegetationAsset};

fn draft_with_empty_vegetation() -> EditorMapDraft {
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    draft.vegetation = Some(MapVegetationAsset {
        schema_version: feathrise_map_content::MAP_SCHEMA_VERSION,
        map_id: draft.map_id.clone(),
        layers: Vec::new(),
        authoring: Some(crate::authoring::vegetation_defaults::empty_vegetation_authoring()),
    }); // 与新建地图相同：仅有基础组，没有正式输出或密度画布。
    draft
}

fn save_app(draft: EditorMapDraft) -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .init_asset::<WorldAsset>()
        .insert_resource(MapEditorState {
            open: true,
            selected_map_id: Some(draft.map_id.clone()),
            save_requested: true,
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 7,
            vegetation_revision: 9,
            vegetation_structure_revision: 11,
            vegetation_compiled_revision: 8, // 真实载入后会要求后台生成本次完整输出。
            ..default()
        })
        .init_resource::<DraftSaveTask>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<GlobalEnvironmentSaveTask>()
        .init_resource::<VegetationCompileState>()
        .init_resource::<MapDeletionTask>()
        .init_resource::<ActiveDynamicMap>()
        .init_resource::<DynamicWaterStatus>()
        .init_resource::<MapPlacementRuntimeStatus>()
        .init_resource::<WaterPreviewState>()
        .init_resource::<TerrainPreviewState>()
        .init_resource::<TerrainLiveApplyState>()
        .init_resource::<PlacementTerrainFollowState>()
        .init_resource::<crate::PlacementInstanceIndex>()
        .init_resource::<crate::authoring::TerrainBrushStroke>()
        .init_resource::<WaterFileHandoff>()
        .init_resource::<WaterBrushStroke>()
        .init_resource::<PlacementWaterEvidenceRegistry>()
        .add_systems(
            Update,
            (
                crate::authoring::reconcile_following_placements,
                poll_vegetation_compile,
                start_vegetation_compile,
                start_draft_save,
            )
                .chain(),
        ); // 运行真实非阻塞编译及保存入口；无未保存修改时绝不启动磁盘IO。
    app
}

#[test]
fn blank_map_save_ignores_vegetation_without_an_owned_terrain_canvas() {
    for has_objects in [false, true] {
        let mut draft = draft_with_empty_vegetation();
        let objects = if has_objects {
            draft.manifest.terrain_objects_for_edit()
        } else {
            Default::default()
        };
        draft.manifest.terrain = MapTerrainDescriptor::empty();
        draft.manifest.terrains = Some(objects);
        draft.terrain_mesh = None;
        draft.heights = None;
        let mut app = save_app(draft);
        for _ in 0..4 {
            app.update();
        }
        let editor = app.world().resource::<MapEditorState>();
        assert!(
            !editor.save_requested,
            "空图保存没有收口，实际状态：{}",
            editor.status,
        );
        assert_eq!(editor.status, "当前地图没有未保存变更");
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(!session.saving);
        assert_eq!(
            session.vegetation_compiled_revision, 8,
            "不能伪造一次编译完成回执"
        );
        assert!(
            session
                .draft
                .as_ref()
                .unwrap()
                .terrain_documents
                .active
                .is_none()
        );
        assert!(
            app.world()
                .resource::<VegetationCompileState>()
                .task
                .is_none()
        );
        assert!(app.world().resource::<DraftSaveTask>().task.is_none());
    }
}

#[test]
fn vegetation_save_still_waits_for_legacy_and_explicit_canvas_compilation() {
    for explicit in [false, true] {
        let mut draft = draft_with_empty_vegetation();
        if explicit {
            crate::authoring::terrain_documents::ensure_primary_canvas(&mut draft);
            let objects = draft.manifest.terrain_objects_for_edit();
            draft.manifest.terrain = MapTerrainDescriptor::empty();
            draft.manifest.terrains = Some(objects);
        }
        let mut app = save_app(draft);
        app.update();
        assert!(app.world().resource::<MapEditorState>().save_requested);
        assert!(
            app.world()
                .resource::<MapEditorState>()
                .status
                .contains("等待植被作者图层后台编译")
        );
        assert!(
            app.world()
                .resource::<VegetationCompileState>()
                .task
                .is_some()
        );
        assert!(app.world().resource::<DraftSaveTask>().task.is_none());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.world().resource::<MapEditorState>().save_requested {
            app.update();
            assert!(
                std::time::Instant::now() < deadline,
                "同修订编译完成后必须自动继续保存"
            );
            std::thread::sleep(std::time::Duration::from_millis(1)); // 仅测试等待真实后台完成值。
        }
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(
            session.vegetation_compiled_revision,
            session.vegetation_revision
        );
        assert_eq!(
            session.vegetation_compiled_structure_revision,
            session.vegetation_structure_revision
        );
        assert!(app.world().resource::<DraftSaveTask>().task.is_none());
    }
}

#[test]
fn vegetation_save_keeps_waiting_while_an_explicit_canvas_is_loading() {
    let mut draft = draft_with_empty_vegetation();
    let objects = draft.manifest.terrain_objects_for_edit();
    draft.manifest.terrain = MapTerrainDescriptor::empty();
    draft.manifest.terrains = Some(objects);
    draft.terrain_documents.loading = true; // 没有active不等于容器稳定，尚未完成的目标不能越过保存门。
    let mut app = save_app(draft);
    for _ in 0..4 {
        app.update();
    }
    assert!(app.world().resource::<MapEditorState>().save_requested);
    assert!(
        app.world()
            .resource::<VegetationCompileState>()
            .task
            .is_none()
    );
    assert!(app.world().resource::<DraftSaveTask>().task.is_none());
}

#[test]
fn vegetation_save_cancels_close_when_the_owned_canvas_compile_fails() {
    let mut draft = draft_with_empty_vegetation();
    crate::authoring::terrain_documents::ensure_primary_canvas(&mut draft);
    let objects = draft.manifest.terrain_objects_for_edit();
    draft.manifest.terrain = MapTerrainDescriptor::empty();
    draft.manifest.terrains = Some(objects);
    draft.vegetation.as_mut().unwrap().authoring = None; // 真正作者来源损坏，不能按空容器放过。
    let mut app = save_app(draft);
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .close_requested = true;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.world().resource::<MapEditorState>().save_requested {
        app.update();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let editor = app.world().resource::<MapEditorState>();
    assert!(editor.open);
    assert!(!editor.close_requested);
    assert!(editor.status.contains("编译失败"));
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .vegetation_compiled_revision,
        8
    );
    assert!(app.world().resource::<DraftSaveTask>().task.is_none());
}
