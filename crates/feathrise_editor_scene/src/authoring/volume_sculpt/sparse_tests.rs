//! 分块作者任务到单笔历史、扩展撤销/重做及真实文件重开的完整回归。
use super::*;
use feathrise_terrain::volume::sparse::*;

fn dig(
    snapshot: VolumeSnapshot,
    point: [f32; 3],
    budget: Arc<VolumeMemoryBudget>,
) -> Box<PreparedVolume> {
    let JobOutput::Prepared(prepared) = jobs::edit_layer(
        snapshot,
        VolumeBrush {
            kind: VolumeBrushKind::Dig,
            from: point,
            to: point,
            radius: 2.,
            strength: 1.,
            ..Default::default()
        },
        "scenes/EDGE-TEST".into(),
        Rect::from_corners(Vec2::splat(-128.), Vec2::splat(128.)),
        budget,
        Vec::new(),
        |_| Ok(0.),
    )
    .unwrap() else {
        panic!("有效扩展必须生成完整候选")
    };
    prepared
}
fn publish(
    state: &mut VolumeAuthoringState,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    prepared: Box<PreparedVolume>,
    history: Option<(bool, Box<VolumeSculptChange>)>,
    root: Entity,
) {
    let mut candidate = pending(session, Some(prepared.snapshot), root);
    candidate.bytes = Some(prepared.bytes);
    candidate.history = history;
    adopt(state, session, editor, candidate).unwrap();
}

#[test]
fn volume_sparse_multiple_positions_share_one_layer_and_expansion_undo_redo_restore_exact_source() {
    let (mut session, mut state, mut editor) = fixture();
    let draft = session.draft.as_mut().unwrap();
    draft.manifest.world_min = [-128.; 2];
    draft.manifest.world_max = [128.; 2];
    let root = World::new().spawn_empty().id();
    let mut seed = snapshot(flat());
    seed.source = Arc::new(VolumeSource::Sparse(
        new_sparse_volume(0.25, state.budget.clone()).unwrap(),
    ));
    let prepared = dig(seed, [0.; 3], state.budget.clone());
    let first = prepared.snapshot.clone();
    publish(&mut state, &mut session, &mut editor, prepared, None, root);
    state.stroke.as_mut().unwrap().released = true;
    finish_stroke(&mut state, &mut session);
    let (_, mut next_stroke, _) = fixture();
    let mut stroke = next_stroke.stroke.take().unwrap();
    stroke.revision = session.revision;
    stroke.before = Some(first.clone());
    stroke.new_region = false;
    state.stroke = Some(stroke);
    for point in [[40., 0., 0.], [80., 0., 0.]] {
        let prepared = dig(state.current.clone().unwrap(), point, state.budget.clone());
        publish(&mut state, &mut session, &mut editor, prepared, None, root);
    }
    state.stroke.as_mut().unwrap().released = true;
    finish_stroke(&mut state, &mut session);
    assert_eq!(session.undo.len(), 2, "多个后台段仍属于同一条笔划");
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements.len(),
        1
    );
    let expanded = state.current.clone().unwrap();
    assert_eq!(expanded.record.id, first.record.id);
    assert_eq!(expanded.source.footprint().len(), 3);
    assert_eq!(state.files.len(), 1, "多块不产生逐块保存文件");
    let EditorCommand::VolumeSculptChanged(change) = session.undo[1].clone() else {
        panic!()
    };
    let JobOutput::Prepared(prepared) =
        jobs::restore(change.before.clone().unwrap(), "scenes/EDGE-TEST".into()).unwrap()
    else {
        panic!()
    };
    publish(
        &mut state,
        &mut session,
        &mut editor,
        prepared,
        Some((true, change.clone())),
        root,
    );
    assert_eq!(state.current.as_ref().unwrap().record, first.record);
    assert_eq!(*state.current.as_ref().unwrap().source, *first.source);
    assert!(
        Arc::ptr_eq(&state.current.as_ref().unwrap().source, &first.source),
        "恢复不能复制已经占用额度的源"
    );
    assert_eq!(session.redo.len(), 1);
    let JobOutput::Prepared(prepared) =
        jobs::restore(change.after.clone(), "scenes/EDGE-TEST".into()).unwrap()
    else {
        panic!()
    };
    publish(
        &mut state,
        &mut session,
        &mut editor,
        prepared,
        Some((false, change)),
        root,
    );
    assert_eq!(state.current.as_ref().unwrap().record, expanded.record);
    assert_eq!(*state.current.as_ref().unwrap().source, *expanded.source);
    assert!(session.redo.is_empty());
    assert_eq!(session.undo.len(), 2);

    let temporary = std::env::temp_dir().join(format!(
        "bevy-sparse-volume-reopen-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let file = &state.files[0];
    let path = temporary.join(&file.path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, file.bytes.as_slice()).unwrap();
    let JobOutput::Opened(reopened) = jobs::open_at(&temporary, expanded.record.clone()).unwrap()
    else {
        panic!()
    };
    assert_eq!(reopened.record, expanded.record);
    assert_eq!(*reopened.source, *expanded.source);
    let mut stale_record = expanded.record;
    stale_record.terrain_patch.as_mut().unwrap().cutouts.pop();
    assert!(
        jobs::open_at(&temporary, stale_record).is_err(),
        "不能把缺少一块覆盖的记录套用到完整源"
    );
    assert_eq!(temporary.parent(), Some(std::env::temp_dir().as_path()));
    std::fs::remove_dir_all(temporary).unwrap();
}

#[test]
fn volume_sparse_foreign_overlap_and_scene_boundary_fail_without_changing_previous_candidate() {
    let budget: Arc<VolumeMemoryBudget> = Arc::default();
    let mut seed = snapshot(flat());
    seed.source = Arc::new(VolumeSource::Sparse(
        new_sparse_volume(0.25, budget.clone()).unwrap(),
    ));
    let first = dig(seed, [0.; 3], budget.clone()).snapshot;
    let before = build_author_volume_mesh(&first.source).unwrap();
    let used = budget.used_bytes();
    let brush = VolumeBrush {
        kind: VolumeBrushKind::Dig,
        from: [40., 0., 0.],
        to: [40., 0., 0.],
        radius: 2.,
        strength: 1.,
        ..Default::default()
    };
    let bounds = Rect::from_corners(Vec2::splat(-128.), Vec2::splat(128.));
    assert!(
        jobs::edit_layer(
            first.clone(),
            brush,
            "scenes/EDGE-TEST".into(),
            bounds,
            budget.clone(),
            vec![feathrise_scene_content::SceneTerrainCutout {
                min: [32., -8.],
                max: [48., 8.]
            }],
            |_| Ok(0.)
        )
        .is_err()
    );
    let brush = VolumeBrush {
        from: [128., 0., 0.],
        to: [128., 0., 0.],
        ..brush
    };
    assert!(
        jobs::edit_layer(
            first.clone(),
            brush,
            "scenes/EDGE-TEST".into(),
            bounds,
            budget.clone(),
            Vec::new(),
            |_| Ok(0.)
        )
        .is_err()
    );
    assert_eq!(build_author_volume_mesh(&first.source).unwrap(), before);
    assert_eq!(budget.used_bytes(), used, "失败候选必须归还所有新分配额度");
}
