//! 历史来源、缺页等待、切图释放与保存失败的生命周期回归。
use super::*;

#[test]
fn sparse_foreign_candidate_and_waiting_history_never_take_author_or_write_save_files() {
    let (mut app, root, record, changes) = fixture();
    let map_id = app
        .world()
        .resource::<SparseTerrainAuthoringState>()
        .author
        .as_ref()
        .unwrap()
        .map_id()
        .clone();
    let mut foreign = SparseTerrainAuthorSession::load(&root, &map_id).unwrap();
    let edit = foreign
        .prepare_changes_loaded(changes.clone(), true)
        .unwrap();
    let placement = BuildingPlacementChange::Added {
        record_index: 0,
        record,
    };
    assert!(apply(&mut app, placement.clone(), edit).is_err());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.revision, 0);
    assert!(
        session
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .is_empty()
    );
    assert!(
        !app.world()
            .resource::<SparseTerrainAuthoringState>()
            .author
            .as_ref()
            .unwrap()
            .is_dirty()
    );
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .push(EditorCommand::BuildingSparseTerrainChanged(Box::new(
            BuildingSparseTerrainChange {
                placement,
                heights: changes,
            },
        )));
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .undo_requested = true;
    app.world_mut()
        .run_system_once(apply_sparse_terrain_undo_redo)
        .unwrap();
    assert!(app.world().resource::<MapEditorState>().undo_requested); // 缺同代正式来源保留请求，不释放历史起点。
    assert!(
        app.world()
            .resource::<SparseTerrainAuthoringState>()
            .task
            .is_none()
    );
    assert!(!app.world().resource::<EditorAuthoringSession>().saving);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sparse_history_waits_for_content_and_map_cancel_releases_input_gate() {
    let (mut app, root, record, changes) = fixture();
    let placement = BuildingPlacementChange::Added {
        record_index: 0,
        record,
    };
    let edit = prepare(&mut app, changes.clone(), true);
    apply(&mut app, placement.clone(), edit).unwrap();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .push(EditorCommand::BuildingSparseTerrainChanged(Box::new(
            BuildingSparseTerrainChange {
                placement,
                heights: changes.clone(),
            },
        )));
    let mut request = request(&mut app, true);
    request.ground_content = Some([1; 7]);
    let edit = prepare(&mut app, changes, false);
    app.world_mut()
        .resource_mut::<SparseTerrainAuthoringState>()
        .building_history_ready = Some((request, Ok(edit)));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    app.world_mut()
        .run_system_once(poll_sparse_building_history)
        .unwrap();
    assert!(
        app.world()
            .resource::<SparseTerrainAuthoringState>()
            .building_history_ready
            .is_some()
    );
    assert!(
        app.world_mut()
            .resource_mut::<SparseTerrainAuthoringState>()
            .take_building_author()
            .is_none()
    );
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft = None;
    app.world_mut()
        .run_system_once(sync_sparse_terrain_authoring)
        .unwrap();
    assert!(!app.world().resource::<EditorAuthoringSession>().saving);
    assert!(
        app.world()
            .resource::<SparseTerrainAuthoringState>()
            .building_history_ready
            .is_none()
    );
    assert!(
        app.world()
            .resource::<SparseHeightLiveOverride>()
            .map_id()
            .is_none()
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sparse_failed_building_audit_blocks_even_the_first_height_file_write() {
    let (mut app, root, record, changes) = fixture();
    let path = root.join(feathrise_map_content::sparse_height_manifest_asset_path(
        app.world()
            .resource::<SparseTerrainAuthoringState>()
            .author
            .as_ref()
            .unwrap()
            .map_id(),
    ));
    let disk_before = std::fs::read(&path).unwrap();
    let placement = BuildingPlacementChange::Added {
        record_index: 0,
        record,
    };
    let edit = prepare(&mut app, changes, true);
    apply(&mut app, placement, edit).unwrap();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .save_requested = true;
    app.world_mut()
        .run_system_once(start_sparse_terrain_save)
        .unwrap(); // 无建筑目录时正式审计明确失败。
    assert!(!app.world().resource::<MapEditorState>().save_requested);
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .status
            .contains("保存未开始")
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .sparse_terrain_dirty
    );
    assert!(!app.world().resource::<EditorAuthoringSession>().saving);
    assert!(
        app.world()
            .resource::<SparseTerrainAuthoringState>()
            .task
            .is_none()
    );
    assert_eq!(std::fs::read(&path).unwrap(), disk_before);
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sparse_history_io_failure_returns_original_dirty_author_and_retains_top() {
    let (mut app, root, record, changes) = fixture();
    let placement = BuildingPlacementChange::Added {
        record_index: 0,
        record,
    };
    let edit = prepare(&mut app, changes.clone(), true);
    apply(&mut app, placement.clone(), edit).unwrap();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .push(EditorCommand::BuildingSparseTerrainChanged(Box::new(
            BuildingSparseTerrainChange {
                placement,
                heights: changes,
            },
        )));
    let map_id = app
        .world()
        .resource::<SparseTerrainAuthoringState>()
        .author
        .as_ref()
        .unwrap()
        .map_id()
        .clone();
    let path = root.join(feathrise_map_content::sparse_height_manifest_asset_path(&map_id));
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.push(b' ');
    std::fs::write(&path, bytes).unwrap(); // 模拟外部工具替换清单基线。
    replay_in_background(&mut app, true);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(session.redo.is_empty());
    assert_eq!(session.revision, 1);
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements.len(),
        1
    );
    let author = app
        .world()
        .resource::<SparseTerrainAuthoringState>()
        .author
        .as_ref()
        .unwrap();
    assert!(author.is_dirty());
    assert!(author.sample_world_height(0., 0.).unwrap().unwrap() > 4.99);
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .status
            .contains("保留原栈顶与作者源")
    );
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
