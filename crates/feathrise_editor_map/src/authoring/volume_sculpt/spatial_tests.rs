//! 长路径的私有进度、完整历史，以及失败/取消/换代后零采用回归。
use super::*;
use crate::{
    authoring::volume_sculpt::{lifecycle, types::SpatialSculptTransaction},
    viewport::spatial_brush::SpatialBrushRequest,
};
use bevy::tasks::IoTaskPool;
use feathrise_map_terrain::volume::sparse::paged::PagedVolume;

fn staged_app() -> (App, VolumeSnapshot, VolumeSnapshot) {
    let (mut session, mut state, mut editor) = fixture();
    editor.open = true;
    editor.tool = crate::types::EditorTool::VolumeDig;
    editor.selected_map_id = state.scope.clone();
    let mut before = snapshot(flat());
    before.source = Arc::new(VolumeSource::Paged(PagedVolume::new(0.25).unwrap()));
    session
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements
        .push(before.record.clone());
    let brush = VolumeBrush {
        from: [4., 0., 4.],
        to: [120., 0., 4.],
        radius: 0.5,
        ..default()
    };
    let request = SpatialBrushRequest::for_test(&editor, &session, brush);
    let stroke = state.stroke.as_mut().unwrap();
    stroke.spatial = Some(request.clone());
    stroke.before = Some(before.clone());
    stroke.new_region = false;
    stroke.released = true;
    state.current = Some(before.clone());
    let mut after = before.clone();
    let mut version = PagedVolume::new(0.25).unwrap();
    version.focus = [40., 0., 4.];
    after.source = Arc::new(VolumeSource::Paged(version));
    after.record.asset_path = "maps/EDGE-TEST/volume_sculpt/last.glb".into();
    let mut batches = VolumeBrushBatches::new(brush, 0.25).unwrap();
    batches.next_batch().unwrap().unwrap();
    assert!(!batches.finished());
    let transaction = SpatialSculptTransaction {
        request: request.clone(),
        snapshot: after.clone(),
        batches: batches.into(),
        batch: None,
        ground: [0; 7],
        changed: true,
    };
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    state.work = Some(crate::authoring::volume_sculpt::types::Work {
        task: IoTaskPool::get().spawn(async move { Ok(JobOutput::Staged(Box::new(transaction))) }),
        map: state.scope.clone().unwrap(),
        revision: 0,
        source: Some(before.source.clone()),
        index: 0,
        history: None,
        ground: None,
        opening: false,
        spatial: Some(request),
    });
    app.insert_resource(session)
        .insert_resource(state)
        .insert_resource(editor)
        .init_resource::<TerrainVolumePreviewRequest>()
        .init_resource::<feathrise_scene::TerrainVolumeStreamControl>()
        .add_systems(Update, lifecycle::poll);
    (app, before, after)
}

fn collect(app: &mut App) {
    for _ in 0..1000 {
        app.update();
        if app
            .world()
            .resource::<VolumeAuthoringState>()
            .work
            .is_none()
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("小型后台测试任务未结束");
}

#[test]
fn volume_spatial_private_batch_keeps_formal_source_until_complete_and_records_one_undo() {
    let (mut app, before, after) = staged_app();
    collect(&mut app);
    let state = app.world().resource::<VolumeAuthoringState>();
    assert!(state.spatial_transaction.is_some() && state.stroke.is_some());
    assert!(Arc::ptr_eq(
        &state.current.as_ref().unwrap().source,
        &before.source
    ));
    assert!(
        app.world()
            .resource::<TerrainVolumePreviewRequest>()
            .target
            .is_none()
    );
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements[0],
        before.record
    );
    assert!(session.undo.is_empty() && session.redo.is_empty());

    let mut state = app
        .world_mut()
        .remove_resource::<VolumeAuthoringState>()
        .unwrap();
    let mut session = app
        .world_mut()
        .remove_resource::<EditorAuthoringSession>()
        .unwrap();
    let mut editor = app.world_mut().remove_resource::<MapEditorState>().unwrap();
    let mut adoption = pending(
        &session,
        Some(after.clone()),
        app.world_mut().spawn_empty().id(),
    );
    adoption.spatial = state.stroke.as_ref().unwrap().spatial.clone();
    state.spatial_transaction = None; // 完整后台结果已取代私有进度，正式采用只执行一次。
    adopt(&mut state, &mut session, &mut editor, adoption).unwrap();
    finish_stroke(&mut state, &mut session);
    assert!(state.stroke.is_none());
    assert_eq!(session.undo.len(), 1);
    let EditorCommand::VolumeSculptChanged(change) = &session.undo[0] else {
        panic!()
    };
    assert!(Arc::ptr_eq(
        &change.before.as_ref().unwrap().source,
        &before.source
    ));
    assert!(Arc::ptr_eq(&change.after.source, &after.source));
    assert!(editor.status.contains("整条空间雕刻已采用"));
}

#[test]
fn volume_spatial_cancelled_failed_or_stale_batch_cannot_restore_progress_or_publish_history() {
    for reason in 0..6 {
        let (mut app, before, _) = staged_app();
        match reason {
            0 => {
                app.world_mut().resource_mut::<MapEditorState>().tool =
                    crate::types::EditorTool::VolumeFill
            }
            1 => {
                app.world_mut()
                    .resource_mut::<VolumeAuthoringState>()
                    .stroke
                    .as_mut()
                    .unwrap()
                    .spatial = None
            }
            2 => {
                app.world_mut()
                    .resource_mut::<EditorAuthoringSession>()
                    .terrain_revision += 1
            }
            3 => {
                let work = app
                    .world_mut()
                    .resource_mut::<VolumeAuthoringState>()
                    .work
                    .take()
                    .unwrap();
                // 原任务必须完成才能归还信用；本测试替换为已经结束的失败输出。
                bevy::tasks::block_on(work.task).unwrap();
                app.world_mut().resource_mut::<VolumeAuthoringState>().work =
                    Some(crate::authoring::volume_sculpt::types::Work {
                        task: IoTaskPool::get()
                            .spawn(async { Err("局部块写入失败".into()) }),
                        map: work.map,
                        revision: work.revision,
                        source: work.source,
                        index: work.index,
                        history: None,
                        ground: None,
                        opening: false,
                        spatial: work.spatial,
                    });
            }
            4 => app.world_mut().resource_mut::<MapEditorState>().open = false,
            _ => {
                app.init_resource::<crate::viewport::modes::ViewportModeState>();
            }
        }
        collect(&mut app);
        let state = app.world().resource::<VolumeAuthoringState>();
        assert!(state.spatial_transaction.is_none() && state.stroke.is_none());
        assert!(Arc::ptr_eq(
            &state.current.as_ref().unwrap().source,
            &before.source
        ));
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(
            session.draft.as_ref().unwrap().placements.placements[0],
            before.record
        );
        assert!(session.undo.is_empty());
        assert!(
            app.world()
                .resource::<TerrainVolumePreviewRequest>()
                .target
                .is_none()
        );
    }
}

#[test]
fn volume_spatial_final_candidate_rechecks_mode_before_scene_publication() {
    let (mut app, before, after) = staged_app();
    collect(&mut app);
    let root = app.world_mut().spawn_empty().id();
    let session = app.world().resource::<EditorAuthoringSession>();
    let mut candidate = pending(session, Some(after.clone()), root);
    candidate.spatial = app
        .world()
        .resource::<VolumeAuthoringState>()
        .stroke
        .as_ref()
        .unwrap()
        .spatial
        .clone();
    let map = session.draft.as_ref().unwrap().map_id.clone();
    let mut state = app.world_mut().resource_mut::<VolumeAuthoringState>();
    state.spatial_transaction = None;
    state.adoption = Some(candidate);
    app.world_mut()
        .resource_mut::<TerrainVolumePreviewRequest>()
        .target = Some(feathrise_scene::TerrainVolumePreviewTarget {
        map_id: map,
        entity: root,
        record: after.record.clone(),
        world_cutout: after.record.terrain_patch.as_ref().unwrap().cutout.unwrap(),
        removal: false,
    });
    // 模式输入先于Tasks，不能等Presentation取消本笔才拦下已经完成的候选。
    app.init_resource::<crate::viewport::modes::ViewportModeState>()
        .add_systems(Update, validate.after(lifecycle::poll));
    app.update();
    let state = app.world().resource::<VolumeAuthoringState>();
    assert!(state.adoption.is_none());
    assert!(Arc::ptr_eq(
        &state.current.as_ref().unwrap().source,
        &before.source
    ));
    assert!(app.world().get_entity(root).is_err());
    assert!(
        app.world()
            .resource::<TerrainVolumePreviewRequest>()
            .target
            .is_none()
    );
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.undo.is_empty());
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements[0],
        before.record
    );
}
