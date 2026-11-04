//! 验证稀疏建筑历史的完整页候选不会在删除表共享时提前改高度、记录或栈顶。

use super::*;

#[test]
fn prepared_sparse_building_history_waits_for_deletion_reader_then_replays_atomically() {
    for undoing in [true, false] {
        let (mut app, root, record, changes) = fixture();
        let placement = BuildingPlacementChange::Added { record_index: 0, record: record.clone() };
        let edit = prepare(&mut app, changes.clone(), true);
        let heights = apply(&mut app, placement.clone(), edit).unwrap();
        app.world_mut().resource_mut::<EditorAuthoringSession>().undo.push(
            EditorCommand::BuildingSparseTerrainChanged(Box::new(BuildingSparseTerrainChange {
                placement, heights,
            })),
        );
        if !undoing { replay_in_background(&mut app, true); }
        let request = request(&mut app, undoing);
        let edit = prepare(&mut app, changes.clone(), !undoing);
        let (reader, revisions, deletion_revision, records, previews) = {
            let session = app.world().resource::<EditorAuthoringSession>();
            (session.placement_deletions.source(),
                (session.revision, session.terrain_revision, session.placement_structure_revision),
                session.placement_deletions.revision(),
                session.draft.as_ref().unwrap().placements.placements.clone(),
                session.placement_previews.clone())
        };
        app.world_mut().resource_mut::<SparseTerrainAuthoringState>().building_history_ready =
            Some((request, Ok(edit)));
        app.world_mut().resource_mut::<EditorAuthoringSession>().saving = true;
        app.world_mut().run_system_once(poll_sparse_building_history).unwrap();
        {
            let session = app.world().resource::<EditorAuthoringSession>();
            assert!(session.saving, "等待期间保留联合事务串行门");
            assert_eq!((session.revision, session.terrain_revision, session.placement_structure_revision), revisions);
            assert_eq!(session.placement_deletions.revision(), deletion_revision);
            assert_eq!(session.draft.as_ref().unwrap().placements.placements, records);
            assert_eq!(session.placement_previews, previews);
            assert_eq!((session.undo.len(), session.redo.len()), if undoing { (1, 0) } else { (0, 1) });
            let sparse = app.world().resource::<SparseTerrainAuthoringState>();
            assert!(sparse.building_history_ready.is_some(), "不能消耗或丢弃已准备的完整页");
            for change in changes.iter() {
                assert_eq!(sparse.author.as_ref().unwrap().sample_grid_encoded(change.x, change.z).unwrap(),
                    Some(if undoing { change.after } else { change.before }));
            }
        }
        drop(reader);
        app.world_mut().run_system_once(poll_sparse_building_history).unwrap();
        let session = app.world().resource::<EditorAuthoringSession>();
        assert!(!session.saving);
        assert_eq!((session.undo.len(), session.redo.len()), if undoing { (0, 1) } else { (1, 0) });
        assert_eq!(session.draft.as_ref().unwrap().placements.placements,
            if undoing { Vec::new() } else { vec![record] });
        assert_eq!((session.revision, session.terrain_revision, session.placement_structure_revision),
            (revisions.0 + 1, revisions.1 + 1, revisions.2 + 1));
        let sparse = app.world().resource::<SparseTerrainAuthoringState>();
        assert!(sparse.building_history_ready.is_none());
        for change in changes.iter() {
            assert_eq!(sparse.author.as_ref().unwrap().sample_grid_encoded(change.x, change.z).unwrap(),
                Some(if undoing { change.before } else { change.after }));
        }
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }
}
