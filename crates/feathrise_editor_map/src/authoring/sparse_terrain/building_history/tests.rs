//! 使用独占临时作者地图验证建筑与四页高度的完整历史事务，不启动GPU。
use super::*;
use crate::authoring::{BuildingPlacementChange, commands::BuildingSparseTerrainChange};
use bevy::ecs::system::RunSystemOnce;
use feathrise_map_content::{MapPlacementHeightMode, MapPlacementRecord, SparseTerrainPreparedEdit};

#[path = "tests/support.rs"]
mod support; // 独占临时作者源与真实后台任务的共用夹具。
use support::*;
#[path = "tests/lifecycle.rs"]
mod lifecycle; // 来源失败、等待、切图和保存门回归独立维护。
#[path = "tests/deletion_reader.rs"]
mod deletion_reader; // 完整高度候选等待删除表读侧，释放后才共同采用。

#[test]
fn scoped_building_sparse_history_keeps_its_object_and_rejects_equal_pixels_under_another_id() {
    let (mut app, root, record, changes) = fixture();
    let placement = BuildingPlacementChange::Added {
        record_index: 0,
        record: record.clone(),
    };
    let edit = prepare(&mut app, changes.clone(), true);
    let applied = apply(&mut app, placement.clone(), edit).unwrap();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .push(EditorCommand::TerrainCanvas {
            id: feathrise_map_content::TerrainId::legacy(),
            command: Box::new(EditorCommand::BuildingSparseTerrainChanged(Box::new(
                BuildingSparseTerrainChange {
                    placement,
                    heights: applied,
                },
            ))),
        });
    replay_in_background(&mut app, true);
    assert!(
        matches!(app.world().resource::<EditorAuthoringSession>().redo.last(),
        Some(EditorCommand::TerrainCanvas { id, .. }) if *id == feathrise_map_content::TerrainId::legacy())
    );
    replay_in_background(&mut app, false);
    assert!(
        matches!(app.world().resource::<EditorAuthoringSession>().undo.last(),
        Some(EditorCommand::TerrainCanvas { id, .. }) if *id == feathrise_map_content::TerrainId::legacy())
    );
    let request = request(&mut app, true);
    let edit = prepare(&mut app, changes.clone(), false);
    // 保留同一 Arc 与建筑差分，只改变对象ID；完成门必须拒绝整笔，不得先改任何一侧。
    if let EditorCommand::TerrainCanvas { id, .. } = app
        .world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .last_mut()
        .unwrap()
    {
        *id = feathrise_map_content::TerrainId::parse("other-canvas").unwrap();
    }
    app.world_mut()
        .resource_mut::<SparseTerrainAuthoringState>()
        .building_history_ready = Some((request, Ok(edit)));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    app.world_mut()
        .run_system_once(poll_sparse_building_history)
        .unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements,
        [record]
    );
    assert_eq!(session.undo.len(), 1);
    assert!(session.redo.is_empty());
    assert!(!session.saving);
    for change in changes.iter() {
        assert_eq!(
            app.world()
                .resource::<SparseTerrainAuthoringState>()
                .author
                .as_ref()
                .unwrap()
                .sample_grid_encoded(change.x, change.z)
                .unwrap(),
            Some(change.after)
        );
    }
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sparse_added_building_and_four_pages_undo_redo_together_and_keep_placement_save_responsibility()
{
    let (mut app, root, record, changes) = fixture();
    let placement = BuildingPlacementChange::Added {
        record_index: 0,
        record: record.clone(),
    };
    let edit = prepare(&mut app, changes.clone(), true);
    let applied = apply(&mut app, placement.clone(), edit).unwrap();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .push(EditorCommand::BuildingSparseTerrainChanged(Box::new(
            BuildingSparseTerrainChange {
                placement,
                heights: applied,
            },
        )));
    assert_eq!(
        app.world()
            .resource::<SparseHeightLiveOverride>()
            .tile_count(),
        4
    );
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 1); // 建筑与地形不是两次作者操作。
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .has_non_sparse_map_changes()
    ); // 高度成功落盘后仍需保存placements JSON。
    replay_in_background(&mut app, true);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(
        session
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .is_empty()
    );
    assert!(session.placement_previews.is_empty());
    assert_eq!(session.redo.len(), 1);
    assert!(session.undo.is_empty());
    assert_eq!(
        app.world()
            .resource::<SparseTerrainAuthoringState>()
            .sample_loaded_world_height(0., 0.),
        Some(0.)
    );
    replay_in_background(&mut app, false);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements,
        [record]
    );
    assert_eq!(
        (
            session.revision,
            session.terrain_revision,
            session.placement_structure_revision
        ),
        (3, 3, 3)
    );
    assert!(!session.draft.as_ref().unwrap().terrain_dirty);
    assert!(session.sparse_terrain_dirty);
    assert_eq!(session.placement_previews.len(), 1);
    assert_eq!(session.undo.len(), 1);
    assert!(session.redo.is_empty());
    let state = app.world().resource::<SparseTerrainAuthoringState>();
    for change in changes.iter() {
        assert_eq!(
            state
                .author
                .as_ref()
                .unwrap()
                .sample_grid_encoded(change.x, change.z)
                .unwrap(),
            Some(change.after)
        );
    }
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sparse_move_history_restores_complete_record_and_rejects_stale_world_without_partial_height() {
    let (mut app, root, mut before, changes) = fixture();
    before.translation = [4., 0., 4.];
    before.yaw_degrees = 90.;
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements
        .push(before.clone());
    let mut after = before.clone();
    after.translation = [0., 5., 0.];
    after.yaw_degrees = 0.;
    let placement = BuildingPlacementChange::Changed {
        record_index: 0,
        before: before.clone(),
        after: after.clone(),
    };
    let edit = prepare(&mut app, changes.clone(), true);
    let applied = apply(&mut app, placement.clone(), edit).unwrap();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .push(EditorCommand::BuildingSparseTerrainChanged(Box::new(
            BuildingSparseTerrainChange {
                placement,
                heights: applied,
            },
        )));
    replay_in_background(&mut app, true);
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements,
        [before]
    );
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .placement_structure_revision,
        0
    );
    replay_in_background(&mut app, false);
    assert_eq!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements,
        [after]
    );
    // 完成值就绪后记录被外部修改但修订未推进，也必须按完整起点拒绝整笔恢复。
    let request = request(&mut app, true);
    let edit = prepare(&mut app, changes.clone(), false);
    app.world_mut()
        .resource_mut::<SparseTerrainAuthoringState>()
        .building_history_ready = Some((request, Ok(edit)));
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements[0]
        .translation[0] = 1.;
    app.world_mut()
        .run_system_once(poll_sparse_building_history)
        .unwrap();
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(session.redo.is_empty());
    assert_eq!(session.revision, 3);
    assert!(
        app.world()
            .resource::<MapEditorState>()
            .status
            .contains("保留原栈顶")
    );
    for change in changes.iter() {
        assert_eq!(
            app.world()
                .resource::<SparseTerrainAuthoringState>()
                .author
                .as_ref()
                .unwrap()
                .sample_grid_encoded(change.x, change.z)
                .unwrap(),
            Some(change.after)
        );
    }
    drop(app);
    std::fs::remove_dir_all(root).unwrap();
}
