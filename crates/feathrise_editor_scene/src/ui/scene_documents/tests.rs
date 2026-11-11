//! 真实文档暂停、后台交接与既有加载采用的往返回归；不写场景文件。

use super::*;
use crate::authoring::AuthoringRetirement;
use crate::authoring::{
    self, DraftLoadTask, EditorAuthoringSession, EditorCommand, GlobalEnvironmentAuthoringState,
    SparseTerrainAuthoringState, SurfacePreviewState, VegetationPreviewState, WaterBrushStroke,
    WaterPreviewState,
};
use crate::resources::{EditorViewportSelectionState, SceneEditorState};
use crate::ui::gltf_browser::EditorGltfBrowserState;
use bevy::{asset::AssetPlugin, ecs::system::RunSystemOnce, tasks::AsyncComputeTaskPool};
use feathrise_scene_content::sparse_height::{SparseHeightSpec, SparseHeightTile, SparseHeightTileKey};
use feathrise_scene_content::{
    SceneAuthorPreview, SceneBoundariesAsset, ScenePlacementRecord, ScenePlacementsAsset, ScenePortalRecord,
    ScenePortalTarget, ScenePortalsAsset, SparseHeightLiveOverride,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Resource)]
struct Target(SceneId);

fn placement(id: &str, x: f32) -> ScenePlacementRecord {
    serde_json::from_value(serde_json::json!({
        "id": id, "asset_path": "gltf/test.glb", "translation": [x, 0.0, 3.0],
        "yaw_degrees": 0.0, "scale": [1.0, 1.0, 1.0]
    }))
    .unwrap()
}

#[test]
fn project_close_lists_active_parked_and_hidden_scene_drafts_without_retiring_their_history() {
    use crate::standalone::{cancel_project_save, project_close_snapshot, request_save_all};
    use super::resources::SceneDocumentSnapshot;

    let a = SceneId::parse("MAP-close-active").unwrap();
    let b = SceneId::parse("MAP-close-parked-b").unwrap();
    let c = SceneId::parse("MAP-close-parked-c").unwrap();
    let hidden = SceneId::parse("MAP-close-hidden").unwrap();
    let editor = SceneEditorState { open: true, selected_scene_id: Some(a.clone()), dirty: true, ..default() };
    let browser = EditorGltfBrowserState::default();
    let snapshot = |map: &SceneId| SceneDocumentSnapshot {
        session: EditorAuthoringSession {
            draft: Some(draft(map)), revision: 17,
            undo: vec![EditorCommand::PlacementAdded(placement("kept-undo", 3.0))],
            redo: vec![EditorCommand::PlacementAdded(placement("kept-redo", 4.0))],
            ..default()
        },
        sparse: default(), heights: default(), selection: default(),
        ui: super::types::SceneDocumentUi::capture(&editor, &browser), dirty: true, history_order: default(),
    };
    let mut documents = SceneDocuments::default();
    documents.upsert(a.clone(), "当前".into(), true);
    documents.upsert(b.clone(), "后台B".into(), true);
    documents.upsert(c.clone(), "后台C".into(), true);
    documents.parked.insert(b.clone(), snapshot(&b));
    documents.parked.insert(c.clone(), snapshot(&c));
    documents.parked.insert(hidden.clone(), snapshot(&hidden));
    let mut world = World::new();
    world.insert_resource(EditorAuthoringSession { draft: Some(draft(&a)), ..default() });
    world.insert_resource(editor);
    world.insert_resource(documents);
    let labels = project_close_snapshot(&world).unsaved;
    assert_eq!(labels.len(), 4);
    assert!(labels[0].contains(a.as_str()));
    assert!(labels[1].contains(b.as_str()));
    assert!(labels[2].contains(c.as_str()));
    assert!(labels[3].contains(hidden.as_str()));
    request_save_all(&mut world).unwrap();
    cancel_project_save(&mut world); // 尚未启动任何场景保存时取消，不得回收parked数据。
    let documents = world.resource::<SceneDocuments>();
    assert_eq!(documents.parked.len(), 3);
    for map in [&b, &c, &hidden] {
        let document = &documents.parked[map];
        assert!(document.dirty);
        assert_eq!(document.session.revision, 17);
        assert_eq!((document.session.undo.len(), document.session.redo.len()), (1, 1));
        let draft = document.session.draft.as_ref().unwrap();
        assert!(draft.domain.is_none());
        assert_eq!(draft.preserved_asset_paths.as_ref(), &[format!("scenes/{map}/private-actors.scene_characters.json")]);
    }
}

fn portal(target: &SceneId) -> ScenePortalRecord {
    ScenePortalRecord {
        terrain_binding: None,
        arrival_terrain_binding: None,
        id: "portal-unsaved".into(),
        label: "未保存出口".into(),
        label_customized: true,
        position: [2.0, 0.0, 3.0],
        arrival: [2.0, 0.0, 4.0],
        target: ScenePortalTarget::default_spawn(target.clone()),
        color_srgba: [1.0; 4],
        arrow_degrees: 0.0,
    }
}

fn draft(map: &SceneId) -> authoring::EditorSceneDraft {
    let mut draft = authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.scene_id = map.clone();
    draft.manifest.id = map.clone();
    draft.manifest.display_name = map.as_str().into();
    draft.placements.scene_id = map.clone();
    draft.portals.scene_id = map.clone();
    draft.preserved_asset_paths = Arc::from([format!("scenes/{map}/private-actors.scene_characters.json")]); // 无适配的旧游戏入口只保留，不创建或解释角色正文。
    draft
}

fn document_app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, AssetPlugin::default()))
        .init_asset::<ScenePlacementsAsset>()
        .init_asset::<ScenePortalsAsset>()
        .init_asset::<SceneBoundariesAsset>()
        .init_asset::<Mesh>()
        .init_asset::<feathrise_scene::DynamicWaterMaterial>()
        .init_resource::<SceneDocuments>()
        .init_resource::<SceneAuthorPreview>()
        .init_resource::<EditorAuthoringSession>()
        .init_resource::<DraftLoadTask>()
        .init_resource::<SceneEditorState>()
        .init_resource::<GlobalEnvironmentAuthoringState>()
        .init_resource::<SparseTerrainAuthoringState>()
        .init_resource::<SparseHeightLiveOverride>()
        .init_resource::<AuthoringRetirement>()
        .init_resource::<EditorViewportSelectionState>()
        .init_resource::<EditorGltfBrowserState>()
        .init_resource::<SurfacePreviewState>()
        .init_resource::<VegetationPreviewState>()
        .init_resource::<WaterPreviewState>()
        .init_resource::<WaterBrushStroke>()
        .add_systems(
            Update,
            (poll_scene_document_history, authoring::poll_draft_load).chain(),
        );
    app
}

fn pause_to_target(
    mut state: SceneDocumentTransition,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mut load: ResMut<DraftLoadTask>,
    target: Res<Target>,
    mut commands: Commands,
) {
    assert!(!state.busy());
    park_document(&mut state, &mut editor, &mut session, &target.0, &mut commands);
    editor.selected_scene_id = Some(target.0.clone());
    load.desired_scene_id = Some(target.0.clone());
    load.ticket += 1;
}

fn await_document(app: &mut App, map: &SceneId) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.update();
        let docs = app.world().resource::<SceneDocuments>();
        if docs.history_task.is_none()
            && docs.pending_history.is_none()
            && app
                .world()
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .is_some_and(|draft| &draft.scene_id == map)
        {
            return;
        }
        assert!(Instant::now() < deadline, "完整文档没有采用");
        std::thread::yield_now();
    }
}

#[test]
fn unsaved_scenes_round_trip_through_real_load_adoption_without_copying_author_sources() {
    let a = SceneId::parse("MAP-tab-a").unwrap();
    let b = SceneId::parse("MAP-tab-b").unwrap();
    let mut app = document_app();
    let heights = Arc::new(vec![23; 121]);
    let density = Arc::new(vec![77; 121]);
    let mut source = draft(&a);
    source.heights = Some(heights.clone());
    source.terrain_dirty = true;
    source
        .vegetation_author_densities
        .insert("grass".into(), density.clone());
    source.placements.placements.push(placement("same-id", 1.0));
    source.portals.portals.push(portal(&b));
    app.insert_resource(EditorAuthoringSession {
        draft: Some(source),
        revision: 9,
        terrain_revision: 9,
        sparse_terrain_dirty: true,
        undo: vec![EditorCommand::PlacementAdded(placement("same-id", 1.0))],
        redo: vec![EditorCommand::PlacementAdded(placement("undone", 5.0))],
        ..default()
    });
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(a.clone()),
        dirty: true,
        tool: crate::types::EditorTool::Surface,
        brush_radius: 37.0,
        selected_current_portal_id: Some("portal-unsaved".into()),
        ..default()
    });
    app.world_mut()
        .resource_mut::<EditorGltfBrowserState>()
        .selected_placement_id = Some("same-id".into());
    let spec = SparseHeightSpec::new([0.0, 0.0], [64.0, 64.0], 1.0, 0.0, 65535.0).unwrap();
    let key = SparseHeightTileKey { x: 0, z: 0 };
    let mut tile = SparseHeightTile::blank(&spec, key).unwrap();
    tile.set([0, 0], 23).unwrap();
    let tile = Arc::new(tile);
    {
        let mut live = app.world_mut().resource_mut::<SparseHeightLiveOverride>();
        live.retarget(Some(a.clone()), 9);
        live.publish_tiles(&a, 9, [tile.clone()]).unwrap();
    }
    app.insert_resource(Target(b.clone()));
    app.world_mut().run_system_once(pause_to_target).unwrap();
    let ticket = app.world().resource::<DraftLoadTask>().ticket;
    let fresh = draft(&b);
    app.world_mut().resource_mut::<DraftLoadTask>().task =
        Some(AsyncComputeTaskPool::get().spawn(async move { (ticket, Ok(fresh)) }));
    await_document(&mut app, &b);
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    assert_eq!(
        app.world().resource::<SceneDocuments>().parked[&a]
            .session
            .undo
            .len(),
        1
    );
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session
            .draft
            .as_mut()
            .unwrap()
            .placements
            .placements
            .push(placement("same-id", 8.0));
        session
            .undo
            .push(EditorCommand::PlacementAdded(placement("same-id", 8.0)));
    }
    app.world_mut().resource_mut::<SceneEditorState>().dirty = true;
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .brush_radius = 12.0;
    app.insert_resource(Target(a.clone()));
    app.world_mut().run_system_once(pause_to_target).unwrap();
    await_document(&mut app, &a);
    let session = app.world().resource::<EditorAuthoringSession>();
    let restored = session.draft.as_ref().unwrap();
    assert!(Arc::ptr_eq(restored.heights.as_ref().unwrap(), &heights));
    assert!(Arc::ptr_eq(
        &restored.vegetation_author_densities["grass"],
        &density
    ));
    assert!(restored.terrain_dirty && session.sparse_terrain_dirty && session.revision > 9);
    assert_eq!(restored.placements.placements[0].translation[0], 1.0);
    assert_eq!(restored.portals.portals[0].label, "未保存出口");
    assert!(restored.domain.is_none());
    assert_eq!(restored.preserved_asset_paths.as_ref(), &[format!("scenes/{a}/private-actors.scene_characters.json")]);
    assert_eq!((session.undo.len(), session.redo.len()), (1, 1));
    assert!(!session.saving); // 旧加载历史任务不能迟到覆盖恢复后的栈。
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.dirty && !editor.save_requested);
    assert_eq!(editor.brush_radius, 37.0);
    assert_eq!(editor.tool, crate::types::EditorTool::Surface);
    assert!(Arc::ptr_eq(
        app.world()
            .resource::<SparseHeightLiveOverride>()
            .tile(key)
            .unwrap(),
        &tile
    ));
    let preview = app
        .world()
        .resource::<SceneAuthorPreview>()
        .for_scene(&a)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<Assets<ScenePlacementsAsset>>()
            .get(&preview.placements)
            .unwrap()
            .placements[0]
            .translation[0],
        1.0
    );
    assert_eq!(
        app.world()
            .resource::<Assets<ScenePortalsAsset>>()
            .get(&preview.portals)
            .unwrap()
            .portals[0]
            .label,
        "未保存出口"
    );
    assert_eq!(
        app.world().resource::<SceneDocuments>().parked[&b]
            .session
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements[0]
            .translation[0],
        8.0
    );
    let parked = app.world().resource::<SceneDocuments>().parked[&b].session.draft.as_ref().unwrap();
    assert!(parked.domain.is_none());
    assert_eq!(parked.preserved_asset_paths.as_ref(), &[format!("scenes/{b}/private-actors.scene_characters.json")]);
    app.update();
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn scene_loading_retry_same_document_preserves_unsaved_source_and_both_history_stacks() {
    let map = SceneId::parse("MAP-retry-same-document").unwrap();
    let mut app = document_app();
    let heights = Arc::new(vec![23; 121]);
    let mut source = draft(&map);
    source.heights = Some(heights.clone());
    source.terrain_dirty = true;
    app.insert_resource(EditorAuthoringSession {
        draft: Some(source),
        undo: vec![EditorCommand::PlacementAdded(placement("before", 1.0)),
            EditorCommand::GlobalCelestialLightingChanged { sequence: 9, before: default(), after: default() },
            EditorCommand::PlacementAdded(placement("after", 2.0))],
        redo: vec![EditorCommand::PlacementAdded(placement("undone", 3.0))],
        ..default()
    });
    app.insert_resource(SceneEditorState { open: true, selected_scene_id: None, dirty: true, ..default() });
    app.insert_resource(Target(map.clone()));
    app.world_mut().run_system_once(pause_to_target).unwrap();
    await_document(&mut app, &map);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(Arc::ptr_eq(session.draft.as_ref().unwrap().heights.as_ref().unwrap(), &heights));
    assert_eq!((session.undo.len(), session.redo.len()), (3, 1));
    assert!(matches!(&session.undo[0], EditorCommand::PlacementAdded(record) if record.id == "before"));
    assert_eq!(session.undo[1].global_history_id(), Some(9));
    assert!(matches!(&session.undo[2], EditorCommand::PlacementAdded(record) if record.id == "after"));
    assert!(matches!(&session.redo[0], EditorCommand::PlacementAdded(record) if record.id == "undone"));
    assert!(app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn opening_three_compatibility_documents_keeps_tabs_clean_until_an_actual_edit() {
    let scenes: Vec<_> = ["MAP-clean-a", "MAP-clean-b", "MAP-clean-c"]
        .into_iter()
        .map(|id| SceneId::parse(id).unwrap())
        .collect();
    let mut app = document_app();
    app.init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
    for (index, map) in scenes.iter().enumerate() {
        if index == 0 {
            app.insert_resource(SceneEditorState {
                open: true,
                selected_scene_id: Some(map.clone()),
                ..default()
            });
            app.world_mut()
                .resource_mut::<DraftLoadTask>()
                .desired_scene_id = Some(map.clone());
        } else {
            app.insert_resource(Target(map.clone()));
            app.world_mut().run_system_once(pause_to_target).unwrap();
        }
        let ticket = app.world().resource::<DraftLoadTask>().ticket;
        let mut fresh = draft(map);
        // 对应已有场景私有水文件尚未登记目录的加载补齐；继续保留显式保存责任。
        fresh.water_catalog_registration_required = true;
        fresh.water_registration_source_hash = Some("existing-water-source".into());
        app.world_mut().resource_mut::<DraftLoadTask>().task =
            Some(AsyncComputeTaskPool::get().spawn(async move { (ticket, Ok(fresh)) }));
        await_document(&mut app, map);
        app.world_mut().run_system_once(sync_scene_documents).unwrap();
        assert!(app.world().resource::<SceneEditorState>().dirty);
        let docs = app.world().resource::<SceneDocuments>();
        assert_eq!(docs.tabs.len(), index + 1);
        assert!(
            docs.tabs.iter().all(|tab| !tab.dirty),
            "仅打开场景不能出现未保存星号"
        );
    }
    app.insert_resource(Target(scenes[0].clone()));
    app.world_mut().run_system_once(pause_to_target).unwrap();
    await_document(&mut app, &scenes[0]);
    app.world_mut().run_system_once(sync_scene_documents).unwrap();
    assert!(
        app.world()
            .resource::<SceneDocuments>()
            .tabs
            .iter()
            .all(|tab| !tab.dirty)
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .water_catalog_registration_required
    );
    app.world_mut()
        .run_system_once(
            |mut session: ResMut<EditorAuthoringSession>, mut editor: ResMut<SceneEditorState>| {
                let item = placement("actual-edit", 3.0);
                session
                    .draft
                    .as_mut()
                    .unwrap()
                    .placements
                    .placements
                    .push(item.clone());
                session.undo.push(EditorCommand::PlacementAdded(item));
                session.mark_placement_structure_changed(&mut editor);
            },
        )
        .unwrap();
    app.world_mut().run_system_once(sync_scene_documents).unwrap();
    assert!(app.world().resource::<SceneDocuments>().tabs[0].dirty);
    app.insert_resource(Target(scenes[1].clone()));
    app.world_mut().run_system_once(pause_to_target).unwrap();
    await_document(&mut app, &scenes[1]);
    app.world_mut().run_system_once(sync_scene_documents).unwrap();
    let dirty_tabs: Vec<_> = app
        .world()
        .resource::<SceneDocuments>()
        .tabs
        .iter()
        .filter(|tab| tab.dirty)
        .map(|tab| tab.scene_id.clone())
        .collect();
    assert_eq!(dirty_tabs, vec![scenes[0].clone()]); // 真实场景编辑在暂停后仍有星号，其他场景保持干净。
    app.world_mut()
        .resource_mut::<GlobalEnvironmentAuthoringState>()
        .dirty = true;
    app.world_mut().run_system_once(sync_scene_documents).unwrap();
    assert!(app.world().resource::<SceneDocuments>().tabs[1].dirty); // 全局真实编辑继续在活动工作区提示保存。
    app.world_mut()
        .resource_mut::<GlobalEnvironmentAuthoringState>()
        .dirty = false;
    app.world_mut().run_system_once(sync_scene_documents).unwrap();
    assert!(!app.world().resource::<SceneDocuments>().tabs[1].dirty);
}

#[test]
fn global_history_returns_to_its_original_position_and_new_commands_follow_it() {
    use super::history::{merge_history, split_history};
    let global = |sequence| EditorCommand::GlobalCelestialLightingChanged {
        sequence,
        before: default(),
        after: default(),
    };
    let (local, mut shared, order) = split_history(vec![
        EditorCommand::PlacementAdded(placement("first", 1.0)),
        global(1),
        EditorCommand::PlacementAdded(placement("last", 2.0)),
    ]);
    shared.push(global(2));
    let restored = merge_history(local, shared, order);
    assert!(matches!(&restored[0], EditorCommand::PlacementAdded(item) if item.id == "first"));
    assert_eq!(restored[1].global_history_id(), Some(1));
    assert!(matches!(&restored[2], EditorCommand::PlacementAdded(item) if item.id == "last"));
    assert_eq!(restored[3].global_history_id(), Some(2));
}

#[test]
fn metadata_reopening_the_same_scene_updates_one_tab() {
    let a = SceneId::parse("MAP-tab-a").unwrap();
    let mut docs = SceneDocuments::default();
    assert!(docs.upsert(a.clone(), "场景甲".into(), false));
    assert!(!docs.upsert(a.clone(), "场景甲".into(), false));
    assert!(docs.upsert(a.clone(), "新名称".into(), true));
    assert_eq!(docs.tabs.len(), 1);
    assert!(docs.contains(&a) && docs.tabs[0].dirty);
}

#[test]
fn saving_shared_global_values_in_another_tab_does_not_dirty_a_clean_scene_on_return() {
    let a = SceneId::parse("MAP-global-a").unwrap();
    let b = SceneId::parse("MAP-global-b").unwrap();
    let mut app = document_app();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft(&a)),
        undo: vec![EditorCommand::GlobalCelestialLightingChanged {
            sequence: 1,
            before: default(),
            after: default(),
        }],
        ..default()
    })
    .insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(a.clone()),
        dirty: true,
        ..default()
    });
    app.world_mut()
        .resource_mut::<GlobalEnvironmentAuthoringState>()
        .dirty = true;
    app.insert_resource(Target(b.clone()));
    app.world_mut().run_system_once(pause_to_target).unwrap();
    let ticket = app.world().resource::<DraftLoadTask>().ticket;
    let fresh = draft(&b);
    app.world_mut().resource_mut::<DraftLoadTask>().task =
        Some(AsyncComputeTaskPool::get().spawn(async move { (ticket, Ok(fresh)) }));
    await_document(&mut app, &b);
    assert!(!app.world().resource::<SceneDocuments>().parked[&a].dirty);
    app.world_mut()
        .resource_mut::<GlobalEnvironmentAuthoringState>()
        .dirty = false;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .clear();
    app.world_mut().resource_mut::<SceneEditorState>().dirty = false;
    app.insert_resource(Target(a.clone()));
    app.world_mut().run_system_once(pause_to_target).unwrap();
    await_document(&mut app, &a);
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn untouched_compatibility_migration_survives_tab_restore_and_close_protects_global_edits() {
    let a = SceneId::parse("MAP-migration-a").unwrap();
    let b = SceneId::parse("MAP-migration-b").unwrap();
    let mut app = document_app();
    app.add_plugins(bevy::state::app::StatesPlugin)
        .init_state::<feathrise_scene_core::state::ScenePhase>()
        .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft(&a)),
        revision: 7,
        loaded_migration_revision: Some(7),
        ..default()
    });
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(a.clone()),
        dirty: true,
        ..default()
    });
    app.insert_resource(Target(b.clone()));
    app.world_mut().run_system_once(pause_to_target).unwrap();
    let ticket = app.world().resource::<DraftLoadTask>().ticket;
    let fresh = draft(&b);
    app.world_mut().resource_mut::<DraftLoadTask>().task =
        Some(AsyncComputeTaskPool::get().spawn(async move { (ticket, Ok(fresh)) }));
    await_document(&mut app, &b);
    app.insert_resource(Target(a.clone()));
    app.world_mut().run_system_once(pause_to_target).unwrap();
    await_document(&mut app, &a);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.revision > 7);
    assert_eq!(session.loaded_migration_revision, Some(session.revision));
    // 加载补齐仍可显式保存；全局真实修改必须提示，只有未编辑兼容补齐才能直接关闭。
    app.world_mut().resource_mut::<SceneEditorState>().dirty = true;
    app.world_mut()
        .resource_mut::<SceneDocuments>()
        .close_requested = Some(a.clone());
    app.world_mut()
        .resource_mut::<GlobalEnvironmentAuthoringState>()
        .dirty = true;
    app.world_mut()
        .run_system_once(finish_scene_document_close)
        .unwrap();
    assert!(app.world().resource::<SceneDocuments>().contains(&a));
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .scene_switch_confirmation
            .is_some()
    );
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .scene_switch_confirmation = None;
    app.world_mut().resource_mut::<SceneDocuments>().closing = None;
    app.world_mut()
        .resource_mut::<GlobalEnvironmentAuthoringState>()
        .dirty = false;
    app.world_mut()
        .run_system_once(finish_scene_document_close)
        .unwrap();
    assert!(!app.world().resource::<SceneDocuments>().contains(&a));
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .scene_switch_confirmation
            .is_none()
    );
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .pending_scene_transition,
        Some(b)
    );
}

#[test]
fn dirty_tab_close_waits_without_repeating_diagnostics_or_changing_the_document() {
    use std::{io::Write, sync::Mutex};

    // 捕获正式系统的日志，防止把每帧WARN仅改成另一级别后仍持续刷屏。
    #[derive(Clone)]
    struct LogWriter(Arc<Mutex<Vec<u8>>>);
    impl Write for LogWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let log = Arc::new(Mutex::new(Vec::new()));
    let writer = log.clone();
    let subscriber = bevy::log::tracing_subscriber::fmt()
        .with_max_level(bevy::log::Level::DEBUG)
        .without_time()
        .with_ansi(false)
        .with_writer(move || LogWriter(writer.clone()))
        .finish();
    bevy::log::tracing::subscriber::with_default(subscriber, || {
        let map = SceneId::parse("MAP-close-dirty-wait").unwrap();
        let mut app = document_app();
        app.add_plugins(bevy::state::app::StatesPlugin)
            .init_state::<feathrise_scene_core::state::ScenePhase>()
            .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
        let mut source = draft(&map);
        source.placements.placements.push(placement("unsaved", 1.0));
        app.insert_resource(EditorAuthoringSession {
            draft: Some(source),
            revision: 3,
            undo: vec![EditorCommand::PlacementAdded(placement("unsaved", 1.0))],
            ..default()
        })
        .insert_resource(SceneEditorState {
            open: true,
            selected_scene_id: Some(map.clone()),
            dirty: true,
            ..default()
        });
        {
            let mut docs = app.world_mut().resource_mut::<SceneDocuments>();
            docs.upsert(map.clone(), "未保存场景".into(), true);
            docs.close_requested = Some(map.clone());
        }
        app.world_mut().run_system_once(finish_scene_document_close).unwrap();
        for stage in [
            crate::types::SceneSwitchStage::Choosing,
            crate::types::SceneSwitchStage::Saving { success_sequence_before: 0 },
        ] {
            app.world_mut().resource_mut::<SceneEditorState>()
                .scene_switch_confirmation.as_mut().unwrap().stage = stage;
            let before = app.world().get_resource_ref::<SceneDocuments>().unwrap().last_changed();
            for _ in 0..120 {
                // 与现场一致：场景编辑可以仅由历史和editor.dirty表达，不得因领域脏位为false而放行。
                let session = app.world().resource::<EditorAuthoringSession>();
                assert!(session.needs_save_before_scene_switch(
                    app.world().resource::<SceneEditorState>(),
                    app.world().resource::<GlobalEnvironmentAuthoringState>(),
                ));
                app.world_mut().run_system_once(finish_scene_document_close).unwrap();
            }
            assert_eq!(app.world().get_resource_ref::<SceneDocuments>().unwrap().last_changed(), before);
            let docs = app.world().resource::<SceneDocuments>();
            assert!(docs.contains(&map) && !docs.close_ready);
            assert_eq!(docs.close_requested.as_ref(), Some(&map));
            let editor = app.world().resource::<SceneEditorState>();
            let request = editor.scene_switch_confirmation.as_ref().unwrap();
            assert_eq!(request.stage, stage);
            assert_eq!(request.source_revision, 3);
            assert!(editor.dirty && !editor.save_requested && editor.pending_scene_transition.is_none());
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(session.undo.len(), 1);
            assert_eq!(session.draft.as_ref().unwrap().placements.placements.len(), 1);
        }
    });
    let output = String::from_utf8(log.lock().unwrap().clone()).unwrap();
    assert_eq!(output.matches("等待确认关闭未保存场景").count(), 1, "{output}");
    assert!(!output.contains("场景进入被未保存保护拦截"), "{output}");
}

#[test]
fn closing_last_clean_scene_returns_to_empty_workspace_and_active_stroke_delays_close() {
    let map = SceneId::parse("MAP-close-last").unwrap();
    let mut app = document_app();
    app.add_plugins(bevy::state::app::StatesPlugin)
        .init_state::<feathrise_scene_core::state::ScenePhase>()
        .init_resource::<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft(&map)),
        ..default()
    });
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(map.clone()),
        ..default()
    });
    {
        let mut docs = app.world_mut().resource_mut::<SceneDocuments>();
        docs.upsert(map.clone(), "最后场景".into(), false);
        docs.close_requested = Some(map.clone());
    }
    app.world_mut().resource_mut::<WaterBrushStroke>().active = true;
    app.world_mut()
        .run_system_once(finish_scene_document_close)
        .unwrap();
    assert!(app.world().resource::<SceneDocuments>().contains(&map));
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .is_some()
    );
    app.world_mut().resource_mut::<WaterBrushStroke>().active = false;
    app.world_mut().resource_mut::<SceneEditorState>().empty_workspace_closed = true;
    app.world_mut()
        .run_system_once(finish_scene_document_close)
        .unwrap();
    assert!(app.world().resource::<SceneDocuments>().tabs.is_empty());
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .is_none()
    );
    assert!(
        app.world()
            .resource::<SceneEditorState>()
            .selected_scene_id
            .is_none()
    );
    assert!(app.world().contains_resource::<feathrise_scene_core::state::EditorStartup>());
    assert!(!app.world().resource::<SceneEditorState>().empty_workspace_closed); // 最后场景关闭建立新的空工作台入口。
}

#[test]
fn scene_tab_title_and_close_button_have_independent_sibling_hit_targets() {
    let a = SceneId::parse("MAP-label-a").unwrap();
    let b = SceneId::parse("MAP-label-b").unwrap();
    let mut app = App::new();
    let mut docs = SceneDocuments::default();
    docs.upsert(a.clone(), "开封城区".into(), true);
    docs.upsert(b.clone(), "黄河故道".into(), false);
    app.insert_resource(docs);
    app.world_mut()
        .run_system_once(|mut commands: Commands, docs: Res<SceneDocuments>| {
            commands.spawn(Node::default()).with_children(|parent| {
                spawn_scene_tabs(
                    parent,
                    &Handle::default(),
                    &docs,
                    None,
                    PresetWorkspaceTab::Scene,
                );
            });
        })
        .unwrap();
    let mut targets = app.world_mut().query::<(&SceneDocumentButton, &ChildOf)>();
    let mut activate = std::collections::BTreeMap::new();
    let mut close = std::collections::BTreeMap::new();
    for (button, parent) in targets.iter(app.world()) {
        match button {
            SceneDocumentButton::Activate(map) => {
                activate.insert(map.clone(), parent.parent());
            }
            SceneDocumentButton::Close(map) => {
                close.insert(map.clone(), parent.parent());
            }
        }
    }
    assert_eq!(activate.len(), 2);
    assert_eq!(activate, close);
    let labels: Vec<_> = app
        .world_mut()
        .query::<&Text>()
        .iter(app.world())
        .map(|text| text.0.clone())
        .collect();
    assert!(labels.iter().any(|label| label == "开封城区*"));
    assert!(labels.iter().any(|label| label == "黄河故道"));
}
