//! 同 MapId 的多个作者源仍必须保持各自的历史身份，不启动 GPU。
use super::*;
use bevy::{ecs::system::RunSystemOnce, tasks::TaskPool};
use feathrise_map_content::{SparseHeightDocument, TerrainId};

fn world() -> (World, std::path::PathBuf, TerrainId) {
    IoTaskPool::get_or_init(TaskPool::new);
    AsyncComputeTaskPool::get_or_init(TaskPool::new);
    let (root, map) = crate::authoring::sparse_terrain::tests::test_map();
    let legacy_author = SparseTerrainAuthorSession::load(&root, &map).unwrap();
    let id = TerrainId::parse("canvas-b").unwrap();
    let mut document = SparseHeightDocument::create_for_terrain(
        &root,
        map.clone(),
        Some(id.clone()),
        legacy_author.spec().clone(),
    )
    .unwrap();
    document.save_dirty().unwrap();
    let author = SparseTerrainAuthorSession::load_for_terrain(&root, &map, Some(&id)).unwrap();
    let path = format!(
        "maps/{map}/terrains/{}/height_tiles.map_height_manifest.json",
        id.as_str()
    );
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.map_id = map.clone();
    draft.manifest.id = map.clone();
    draft.manifest.runtime_scene = "DynamicMapScene".into();
    draft.manifest.terrain.kind = MapTerrainKind::SparseClipmap;
    draft.manifest.terrain.glb_path.clear();
    draft.manifest.terrain.height_path =
        Some(feathrise_map_content::sparse_height_manifest_asset_path(&map));
    let primary = draft.manifest.terrain_objects_for_edit()[0].clone();
    let mut object = primary.clone();
    object.id = id.clone();
    object.terrain.height_path = Some(path.clone());
    draft.manifest.terrains = Some(Arc::from([primary, object.clone()]));
    draft.manifest.terrain = feathrise_map_content::MapTerrainDescriptor::empty();
    draft.terrain_documents.active = Some(
        crate::authoring::terrain_documents::TerrainDocumentIdentity {
            manifest: object.source_manifest(&draft.manifest),
            folder: object.private_folder(&map),
            definition: Arc::new(object),
        },
    );
    let mut live = SparseHeightLiveOverride::default();
    live.retarget(Some(map.clone()), 0);
    let mut world = World::new();
    world.init_resource::<crate::authoring::AuthoringRetirement>();
    world.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    });
    world.insert_resource(MapEditorState {
        open: true,
        selected_map_id: Some(map.clone()),
        terrain_canvas_target: Some(id.clone()),
        ..default()
    });
    world.insert_resource(SparseTerrainAuthoringState {
        desired_map_id: Some(map),
        desired_source: Some((Some(id.clone()), path)),
        author: Some(author),
        ..default()
    });
    world.insert_resource(GlobalEnvironmentAuthoringState::default());
    world.insert_resource(live);
    (world, root, id)
}

fn finish_task(world: &mut World) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while world
        .resource::<SparseTerrainAuthoringState>()
        .task
        .is_some()
    {
        assert!(std::time::Instant::now() < deadline);
        world.run_system_once(poll_sparse_terrain_task).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn same_source_transform_and_lod_revisions_keep_real_raise_and_lower_available() {
    let (mut world, root, _) = world();
    for (index, kind) in [SparseTerrainBrushKind::Raise, SparseTerrainBrushKind::Lower].into_iter().enumerate() {
        {
            let mut session = world.resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            let object = {
                let object = &mut Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[1];
                if index == 0 {
                    object.transform.translation[0] = 27.17;
                } else {
                    object.terrain.display_lods = Some(vec![
                        feathrise_map_content::MapTerrainDisplayLod { outer_half_extent_meters: Some(8.), spacing_multiplier: 1 },
                        feathrise_map_content::MapTerrainDisplayLod { outer_half_extent_meters: None, spacing_multiplier: 4 },
                    ]);
                }
                object.clone()
            };
            let manifest = object.source_manifest(&draft.manifest);
            let active = draft.terrain_documents.active.as_mut().unwrap();
            active.definition = Arc::new(object);
            active.manifest = manifest;
            session.revision = session.revision.wrapping_add(1);
            session.terrain_revision = session.terrain_revision.wrapping_add(1);
        }
        let live = world.resource::<SparseHeightLiveOverride>();
        let session = world.resource::<EditorAuthoringSession>();
        assert_ne!(live.revision(), session.terrain_revision);
        assert_eq!(world.resource::<SparseTerrainAuthoringState>().current_live_height_revision(session, live), Some(live.revision()),
            "仅几何换代不能要求尚未发生的高度发布");
        start_brush_task(&mut world.resource_mut::<SparseTerrainAuthoringState>(), SparseTerrainBrushInput {
            kind, center_xz: [0.; 2], radius_meters: 0.5, strength: 1., speed_meters_per_second: 20.,
            delta_seconds: 0.05, flatten_target_meters: None,
        }, [1.; 2]);
        finish_task(&mut world);
        let height = world.resource::<SparseTerrainAuthoringState>().author.as_ref().unwrap()
            .sample_world_height(0., 0.).unwrap().unwrap();
        if index == 0 { assert!(height > 0.95); } else { assert!(height.abs() < 0.01); }
        assert_eq!(world.resource::<SparseHeightLiveOverride>().revision(), world.resource::<EditorAuthoringSession>().terrain_revision);
        assert!(!world.resource::<SparseTerrainAuthoringState>().live_publish_failed);
    }
    drop(world);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn smoothing_cross_page_peak_publishes_real_heights_and_replays_one_stroke() {
    let (mut world, root, _) = world();
    let input = SparseTerrainBrushInput {
        kind: SparseTerrainBrushKind::Raise, center_xz: [0.; 2], radius_meters: 0.1,
        strength: 1., speed_meters_per_second: 200., delta_seconds: 0.05,
        flatten_target_meters: None,
    };
    // 原点位于四页共享边；先由正式后台抬高建立10米尖峰，不能用手改编码冒充平滑输入。
    start_brush_task(&mut world.resource_mut::<SparseTerrainAuthoringState>(), input, [1.; 2]);
    finish_task(&mut world);
    let spec = world.resource::<SparseTerrainAuthoringState>().author.as_ref().unwrap().spec().clone();
    let original = world.resource::<SparseHeightLiveOverride>().clone();
    {
        let mut state = world.resource_mut::<SparseTerrainAuthoringState>();
        state.stroke = SparseTerrainStroke { active: true, tool: Some(EditorTool::Smooth), ..default() };
    }
    start_brush_task(&mut world.resource_mut::<SparseTerrainAuthoringState>(), SparseTerrainBrushInput {
        kind: SparseTerrainBrushKind::Smooth, radius_meters: 0.5, speed_meters_per_second: 24., ..input
    }, [1.; 2]);
    finish_task(&mut world);
    let changes = world.resource::<SparseTerrainAuthoringState>().stroke.changes.values().copied().collect::<Vec<_>>();
    assert_eq!(changes.len(), 5, "冻结五点平均只降低尖峰并抬起四邻点，不按写入顺序继续扩散");
    let author = world.resource::<SparseTerrainAuthoringState>().author.as_ref().unwrap();
    // 24米/秒积分0.05秒：中心降低1.2米，四邻点按0.75衰减抬升0.9米；不再使用旧收敛系数。
    assert!((author.sample_world_height(0., 0.).unwrap().unwrap() - 8.8).abs() < 0.0001);
    for [x, z] in [[-0.125, 0.], [0.125, 0.], [0., -0.125], [0., 0.125]] {
        assert!((author.sample_world_height(x, z).unwrap().unwrap() - 0.9).abs() < 0.0001);
    }
    let live = world.resource::<SparseHeightLiveOverride>();
    assert_eq!(live.revision(), world.resource::<EditorAuthoringSession>().terrain_revision);
    for change in &changes {
        let (key, local) = spec.tile_for_grid_point(change.x, change.z).unwrap();
        assert_eq!(live.tile(key).unwrap().get_height(&spec, local), Some(change.after));
        assert_eq!(original.tile(key).map(|tile| tile.get_height(&spec, local)).unwrap_or(Some(0)), Some(change.before),
            "新平滑页完整发布，旧显示快照仍保留原尖峰");
    }
    world.resource_scope(|world, mut state: Mut<SparseTerrainAuthoringState>| {
        state.stroke.release_requested = true;
        finish_sparse_stroke_if_ready(&mut state, &mut world.resource_mut::<EditorAuthoringSession>());
    });
    assert_eq!(world.resource::<EditorAuthoringSession>().undo.len(), 1, "平滑整笔只产生一条历史");
    for undoing in [true, false] {
        {
            let mut editor = world.resource_mut::<MapEditorState>();
            editor.undo_requested = undoing;
            editor.redo_requested = !undoing;
        }
        world.run_system_once(apply_sparse_terrain_undo_redo).unwrap();
        finish_task(&mut world);
        let state = world.resource::<SparseTerrainAuthoringState>();
        let live = world.resource::<SparseHeightLiveOverride>();
        for change in &changes {
            let expected = Some(if undoing { change.before } else { change.after });
            let (key, local) = spec.tile_for_grid_point(change.x, change.z).unwrap();
            assert_eq!(state.author.as_ref().unwrap().sample_grid_encoded(change.x, change.z).unwrap(), expected);
            assert_eq!(live.tile(key).unwrap().get_height(&spec, local), expected);
        }
    }
    drop(world);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn current_live_height_rejects_wrong_source_namespace_loading_and_map() {
    let (mut world, root, _) = world();
    let original = world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap().terrain_documents.active.clone().unwrap();
    for mismatch in 0..4 {
        {
            let mut session = world.resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            draft.terrain_documents.active = Some(original.clone());
            draft.terrain_documents.loading = false;
            match mismatch {
                0 => {
                    draft.terrain_documents.active.as_mut().unwrap().manifest.terrain.height_path =
                        Some(feathrise_map_content::sparse_height_manifest_asset_path(&draft.map_id));
                }
                1 => Arc::make_mut(&mut draft.terrain_documents.active.as_mut().unwrap().definition).id = TerrainId::legacy(),
                2 => draft.terrain_documents.loading = true,
                _ => draft.map_id = MapId::parse("MAP-other-live-owner").unwrap(),
            }
        }
        assert!(world.resource::<SparseTerrainAuthoringState>().current_live_height_revision(
            world.resource::<EditorAuthoringSession>(), world.resource::<SparseHeightLiveOverride>()).is_none());
    }
    drop(world);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_live_publication_does_not_reuse_an_old_matching_live_version() {
    let (mut world, root, _) = world();
    start_brush_task(&mut world.resource_mut::<SparseTerrainAuthoringState>(), SparseTerrainBrushInput {
        kind: SparseTerrainBrushKind::Raise, center_xz: [0.; 2], radius_meters: 0.5, strength: 1.,
        speed_meters_per_second: 20., delta_seconds: 0.05, flatten_target_meters: None,
    }, [1.; 2]);
    let map = world.resource::<SparseHeightLiveOverride>().map_id().cloned();
    world.resource_mut::<SparseHeightLiveOverride>().retarget(map, 99);
    finish_task(&mut world);
    assert!(world.resource::<SparseTerrainAuthoringState>().live_publish_failed);
    assert!(world.resource::<MapEditorState>().status.contains("实时覆盖发布失败"));
    world.resource_mut::<EditorAuthoringSession>().terrain_revision = 99;
    assert!(world.resource::<SparseTerrainAuthoringState>().current_live_height_revision(
        world.resource::<EditorAuthoringSession>(), world.resource::<SparseHeightLiveOverride>()).is_none(),
        "会话号偶然一致不能掩盖尚未正式发布的作者改动");
    drop(world);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn independent_sparse_canvas_undo_redo_keeps_wrapper_and_real_source_pixels() {
    let (mut world, root, id) = world();
    let changes = Arc::new(
        world
            .resource_mut::<SparseTerrainAuthoringState>()
            .author
            .as_mut()
            .unwrap()
            .apply_brush(SparseTerrainBrushInput {
                kind: SparseTerrainBrushKind::Raise,
                center_xz: [0.; 2],
                radius_meters: 0.5,
                strength: 1.,
                speed_meters_per_second: 20.,
                delta_seconds: 0.05,
                flatten_target_meters: None,
            })
            .unwrap()
            .changes,
    );
    world
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .push(EditorCommand::TerrainCanvas {
            id: id.clone(),
            command: Box::new(EditorCommand::SparseTerrainChanged(changes.clone())),
        });
    for undoing in [true, false] {
        {
            let mut editor = world.resource_mut::<MapEditorState>();
            editor.undo_requested = undoing;
            editor.redo_requested = !undoing;
        }
        world
            .run_system_once(apply_sparse_terrain_undo_redo)
            .unwrap();
        assert!(world.resource::<EditorAuthoringSession>().saving);
        finish_task(&mut world);
        let session = world.resource::<EditorAuthoringSession>();
        let top = if undoing {
            session.redo.last()
        } else {
            session.undo.last()
        }
        .unwrap();
        assert!(
            matches!(top, EditorCommand::TerrainCanvas { id: scoped, command }
            if scoped == &id && matches!(command.as_ref(), EditorCommand::SparseTerrainChanged(current) if Arc::ptr_eq(current, &changes)))
        );
        let author = world
            .resource::<SparseTerrainAuthoringState>()
            .author
            .as_ref()
            .unwrap();
        for change in changes.iter() {
            assert_eq!(
                author.sample_grid_encoded(change.x, change.z).unwrap(),
                Some(if undoing { change.before } else { change.after })
            );
        }
    }
    assert!(!world.resource::<EditorAuthoringSession>().saving);
    drop(world);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn cancelled_or_stale_sparse_replay_restores_clean_and_dirty_pages_before_releasing_slot() {
    for clean_before in [false, true] {
        for cancelled in [false, true] {
            let (mut world, root, id) = world();
            let changes;
            {
                let mut state = world.resource_mut::<SparseTerrainAuthoringState>();
                let author = state.author.as_mut().unwrap();
                changes = Arc::new(
                    author
                        .apply_brush(SparseTerrainBrushInput {
                            kind: SparseTerrainBrushKind::Raise,
                            center_xz: [0.; 2],
                            radius_meters: 0.5,
                            strength: 1.,
                            speed_meters_per_second: 20.,
                            delta_seconds: 0.05,
                            flatten_target_meters: None,
                        })
                        .unwrap()
                        .changes,
                );
                if clean_before {
                    author.save_dirty().unwrap();
                }
                assert_eq!(author.is_dirty(), !clean_before);
            }
            world.resource_mut::<EditorAuthoringSession>().undo.push(
                EditorCommand::TerrainCanvas {
                    id,
                    command: Box::new(EditorCommand::SparseTerrainChanged(changes.clone())),
                },
            );
            world
                .resource_mut::<EditorAuthoringSession>()
                .sparse_terrain_dirty = !clean_before;
            world.resource_mut::<MapEditorState>().undo_requested = true;
            world
                .run_system_once(apply_sparse_terrain_undo_redo)
                .unwrap();
            assert!(world.resource::<EditorAuthoringSession>().saving);
            assert!(
                world
                    .resource::<SparseTerrainAuthoringState>()
                    .author
                    .is_none()
            );
            if cancelled {
                world.resource_mut::<MapEditorState>().close_requested = true;
            } else {
                // 外部作者身份换代可以发生于UI冻结之外；完成门仍必须保留原页。
                world.resource_mut::<EditorAuthoringSession>().revision += 1;
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !world
                .resource::<MapEditorState>()
                .status
                .contains("正在后台恢复原作者页")
            {
                assert!(std::time::Instant::now() < deadline);
                world.run_system_once(poll_sparse_terrain_task).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            assert!(
                world
                    .resource::<SparseTerrainAuthoringState>()
                    .author
                    .is_none(),
                "回滚结束前不能暴露已修改候选或让另一作者操作取得原槽"
            );
            assert!(world.resource::<EditorAuthoringSession>().saving);
            finish_task(&mut world);
            let state = world.resource::<SparseTerrainAuthoringState>();
            let author = state.author.as_ref().unwrap();
            assert_eq!(
                author.is_dirty(),
                !clean_before,
                "取消不能给干净源制造脏责任，也不能丢弃已有脏页"
            );
            for change in changes.iter() {
                assert_eq!(
                    author.sample_grid_encoded(change.x, change.z).unwrap(),
                    Some(change.after)
                );
            }
            let session = world.resource::<EditorAuthoringSession>();
            assert!(!session.saving);
            assert_eq!(session.sparse_terrain_dirty, !clean_before);
            assert_eq!(session.undo.len(), 1);
            assert!(session.redo.is_empty());
            assert_eq!(session.terrain_revision, 0);
            assert_eq!(world.resource::<SparseHeightLiveOverride>().tile_count(), 0);
            drop(world);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn foreign_sparse_history_leaves_request_and_current_canvas_untouched() {
    let (mut world, root, _) = world();
    let changes = Arc::new(vec![SparseTerrainHeightChange {
        x: 256,
        z: 256,
        before: 0,
        after: 1,
    }]);
    world
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .push(EditorCommand::TerrainCanvas {
            id: TerrainId::legacy(),
            command: Box::new(EditorCommand::SparseTerrainChanged(changes)),
        });
    world.resource_mut::<MapEditorState>().undo_requested = true;
    world
        .run_system_once(apply_sparse_terrain_undo_redo)
        .unwrap();
    assert!(
        world.resource::<MapEditorState>().undo_requested,
        "异对象记录必须交回统一历史请求恢复所属画布"
    );
    assert!(
        world
            .resource::<SparseTerrainAuthoringState>()
            .task
            .is_none()
    );
    assert!(
        !world
            .resource::<SparseTerrainAuthoringState>()
            .author
            .as_ref()
            .unwrap()
            .is_dirty()
    );
    assert!(!world.resource::<EditorAuthoringSession>().saving);
    assert_eq!(world.resource::<EditorAuthoringSession>().undo.len(), 1);
    drop(world);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn same_canvas_history_waits_until_author_source_path_matches_current_version() {
    let (mut world, root, id) = world();
    world
        .resource_mut::<EditorAuthoringSession>()
        .undo
        .push(EditorCommand::TerrainCanvas {
            id,
            command: Box::new(EditorCommand::SparseTerrainChanged(Arc::new(Vec::new()))),
        });
    {
        let mut state = world.resource_mut::<SparseTerrainAuthoringState>();
        let path = &mut state.desired_source.as_mut().unwrap().1;
        *path = path.replace(
            "height_tiles.map_height_manifest.json",
            "versions/new-source.map_height_manifest.json",
        );
    }
    world.resource_mut::<MapEditorState>().undo_requested = true;
    world
        .run_system_once(apply_sparse_terrain_undo_redo)
        .unwrap();
    assert!(world.resource::<MapEditorState>().undo_requested);
    assert!(
        world
            .resource::<SparseTerrainAuthoringState>()
            .task
            .is_none()
    );
    assert!(
        world
            .resource::<SparseTerrainAuthoringState>()
            .author
            .is_some()
    );
    drop(world);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn canonical_sparse_canvas_does_not_also_start_the_legacy_world_brush() {
    let (mut world, root, _) = world();
    let mut mouse = ButtonInput::<MouseButton>::default();
    mouse.press(MouseButton::Left);
    world.insert_resource(mouse);
    world.insert_resource(Time::<Real>::default());
    world.insert_resource(VegetationGroundCursor::default());
    world.resource_mut::<MapEditorState>().tool = EditorTool::Raise;
    world.spawn((
        MapEditorViewport,
        RelativeCursorPosition {
            cursor_over: true,
            normalized: Some(Vec2::splat(0.5)),
        },
    ));
    world.run_system_once(apply_sparse_terrain_brush).unwrap();
    let state = world.resource::<SparseTerrainAuthoringState>();
    assert!(
        !state.stroke.active,
        "显式对象已由版本化地形编辑入口接管，旧世界坐标笔刷不能形成第二笔"
    );
    assert!(state.task.is_none());
    assert!(!state.author.as_ref().unwrap().is_dirty());
    drop(world);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_primary_migration_and_restore_keep_dirty_author_pages_and_live_tiles() {
    let (mut world, root, _) = world();
    let map = world
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .map_id
        .clone();
    let mut author = SparseTerrainAuthorSession::load(&root, &map).unwrap();
    let changes = Arc::new(
        author
            .apply_brush(SparseTerrainBrushInput {
                kind: SparseTerrainBrushKind::Raise,
                center_xz: [0.; 2],
                radius_meters: 0.5,
                strength: 1.,
                speed_meters_per_second: 20.,
                delta_seconds: 0.05,
                flatten_target_meters: None,
            })
            .unwrap()
            .changes,
    );
    let tiles = author.tile_snapshots_for_changes(&changes).unwrap();
    {
        let mut state = world.resource_mut::<SparseTerrainAuthoringState>();
        state.desired_source = None;
        state.author = Some(author);
    }
    let before;
    {
        let mut session = world.resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let primary = draft.manifest.terrains.as_ref().unwrap()[0].clone();
        draft.manifest.terrain = primary.terrain.clone();
        draft.manifest.terrains = None;
        draft.terrain_documents.active = None;
        before = feathrise_map_content::MapTerrainCollection::capture(&draft.manifest);
        let mut renamed = primary;
        renamed.display_name = "改名仍保留未保存高度".into();
        crate::authoring::terrain_documents::ensure_primary_canvas(draft);
        feathrise_map_content::MapTerrainCollection {
            legacy: feathrise_map_content::MapTerrainDescriptor::empty(),
            objects: Some(Arc::from([renamed])),
        }
        .apply(&mut draft.manifest);
        session.sparse_terrain_dirty = true;
    }
    world
        .resource_mut::<SparseHeightLiveOverride>()
        .publish_tiles(&map, 0, tiles)
        .unwrap();
    let count = world.resource::<SparseHeightLiveOverride>().tile_count();
    assert!(count > 0);
    for restore_legacy in [false, true] {
        if restore_legacy {
            let mut session = world.resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            before.apply(&mut draft.manifest);
            crate::authoring::terrain_documents::restore_legacy(draft).unwrap();
        }
        world
            .run_system_once(sync_sparse_terrain_authoring)
            .unwrap();
        let state = world.resource::<SparseTerrainAuthoringState>();
        assert!(state.task.is_none(), "同一主源不能重读磁盘替换脏作者");
        assert!(state.author.as_ref().unwrap().is_dirty());
        assert_eq!(state.ticket, 0);
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
        assert_eq!(
            world.resource::<SparseHeightLiveOverride>().tile_count(),
            count
        );
        assert!(
            world
                .resource::<EditorAuthoringSession>()
                .sparse_terrain_dirty
        );
        if !restore_legacy {
            assert!(matches!(state.desired_source.as_ref(), Some((None, path))
                if path == &feathrise_map_content::sparse_height_manifest_asset_path(&map)));
        }
    }
    drop(world);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn sparse_save_completion_resets_history_scope_before_same_frame_canvas_stroke() {
    let (mut world, root, id) = world();
    let changes = Arc::new(
        world
            .resource_mut::<SparseTerrainAuthoringState>()
            .author
            .as_mut()
            .unwrap()
            .apply_brush(SparseTerrainBrushInput {
                kind: SparseTerrainBrushKind::Raise,
                center_xz: [0.; 2],
                radius_meters: 0.5,
                strength: 1.,
                speed_meters_per_second: 20.,
                delta_seconds: 0.05,
                flatten_target_meters: None,
            })
            .unwrap()
            .changes,
    );
    {
        let mut session = world.resource_mut::<EditorAuthoringSession>();
        session.undo.push(EditorCommand::TerrainCanvas {
            id: id.clone(),
            command: Box::new(EditorCommand::SparseTerrainChanged(changes)),
        });
        session
            .draft
            .as_mut()
            .unwrap()
            .terrain_documents
            .scoped_through = 1;
        session.sparse_terrain_dirty = true;
        session.saving = true;
    }
    world
        .resource_mut::<SparseTerrainAuthoringState>()
        .pending_save = true;
    world.run_system_once(start_sparse_terrain_save).unwrap(); // 真实稀疏作者页和清单在隔离测试根写盘。
    finish_task(&mut world);
    {
        let mut session = world.resource_mut::<EditorAuthoringSession>();
        assert!(!session.saving && !session.sparse_terrain_dirty);
        assert_eq!(session.successful_save_sequence, 1);
        assert!(session.undo.is_empty() && session.redo.is_empty());
        assert_eq!(
            session
                .draft
                .as_ref()
                .unwrap()
                .terrain_documents
                .scoped_through,
            0
        );
        session.undo.push(EditorCommand::SurfaceChanged {
            weight_changes: vec![crate::authoring::resources::ByteChange {
                index: 0,
                before: 0,
                after: 255,
            }],
            index_changes: Vec::new(),
        });
        crate::authoring::terrain_documents::scope_history(&mut session);
        assert!(
            matches!(session.undo.last(), Some(EditorCommand::TerrainCanvas { id: target, .. }) if target == &id)
        );
    }
    drop(world);
    std::fs::remove_dir_all(root).unwrap();
}
