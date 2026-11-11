//! 作者侧完整段、线性历史、唯一源预算及重开真实源验证。
use super::*;
use crate::authoring::volume_sculpt::{jobs, types::Stroke};
use feathrise_terrain::volume::sparse::{
    VolumeSource, build_author_volume_mesh, encode_author_volume_glb,
};
use feathrise_terrain::volume::*;

#[path = "sparse_tests.rs"]
mod sparse_tests;
#[path = "spatial_tests.rs"]
mod spatial_tests;

fn snapshot(source: VolumeData) -> VolumeSnapshot {
    let record = serde_json::from_value(serde_json::json!({
        "id":"volume-test","asset_path":"scenes/EDGE-TEST/volume_sculpt/first.glb","translation":[0,0,0],"yaw_degrees":0,"scale":[1,1,1],
        "terrain_patch":{"sculpted_volume":true,"cutout":{"min":[0,0],"max":[8,8]}}
    })).unwrap();
    VolumeSnapshot {
        record,
        source: Arc::new(source.into()),
    }
}
fn fixture() -> (EditorAuthoringSession, VolumeAuthoringState, SceneEditorState) {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap;
    let map = draft.scene_id.clone();
    let session = EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    };
    let state = VolumeAuthoringState {
        scope: Some(map),
        stroke: Some(Stroke {
            field: None,
            spatial: None,
            shape: default(),
            half_height: 1.,
            lock_height: false,
            flatten_height: None,
            tool: crate::types::EditorTool::VolumeDig,
            revision: 0,
            before: None,
            index: 0,
            released: false,
            last: None,
            tail: None,
            first_click_done: true,
            seed: None,
            new_region: true,
            history_slot: None,
            started: 0.,
        }),
        ..default()
    };
    (session, state, SceneEditorState::default())
}
fn pending(
    session: &EditorAuthoringSession,
    value: Option<VolumeSnapshot>,
    root: Entity,
) -> Adoption {
    Adoption {
        root,
        revision: session.revision,
        index: 0,
        next: value,
        bytes: Some(Arc::new(vec![1, 2, 3])),
        previous: None,
        history: None,
        spatial: None,
        started: 0.,
        expected: session
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .first()
            .cloned(),
    }
}
fn flat() -> VolumeData {
    volume_from_heights(16, 0.5, vec![4.; 17 * 17]).unwrap()
}
fn dense(snapshot: &VolumeSnapshot) -> &VolumeData {
    let VolumeSource::Dense(source) = snapshot.source.as_ref() else {
        panic!("旧区域兼容测试需要v1作者源")
    };
    source
}

#[test]
fn volume_accepted_segments_update_one_history_command_and_release_does_not_duplicate_it() {
    let (mut session, mut state, mut editor) = fixture();
    let root = World::new().spawn_empty().id();
    let first = snapshot(flat());
    let adoption = pending(&session, Some(first.clone()), root);
    adopt(&mut state, &mut session, &mut editor, adoption).unwrap();
    assert_eq!(session.undo.len(), 1);
    let mut next = snapshot(
        apply_volume_brush(
            dense(&first),
            VolumeBrush {
                kind: VolumeBrushKind::Dig,
                from: [4., 4., 4.],
                to: [4., 3., 4.],
                radius: 1.,
                strength: 1.,
                ..Default::default()
            },
        )
        .unwrap(),
    );
    next.record.asset_path = "scenes/EDGE-TEST/volume_sculpt/second.glb".into();
    let adoption = pending(&session, Some(next.clone()), root);
    adopt(&mut state, &mut session, &mut editor, adoption).unwrap();
    assert_eq!(session.undo.len(), 1);
    let EditorCommand::VolumeSculptChanged(change) = &session.undo[0] else {
        panic!("必须是完整体积历史")
    };
    assert!(change.before.is_none());
    assert!(Arc::ptr_eq(&change.after.source, &next.source));
    state.stroke.as_mut().unwrap().released = true;
    finish_stroke(&mut state, &mut session);
    assert!(state.stroke.is_none());
    assert_eq!(session.undo.len(), 1);
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements[0],
        next.record
    );
}

#[test]
fn volume_stale_revision_or_history_stack_cannot_partially_publish_author_record() {
    let (mut session, mut state, mut editor) = fixture();
    let root = World::new().spawn_empty().id();
    let first = snapshot(flat());
    let mut stale = pending(&session, Some(first), root);
    stale.revision = 1;
    assert!(adopt(&mut state, &mut session, &mut editor, stale).is_err());
    assert!(session.undo.is_empty());
    assert!(
        session
            .draft
            .as_ref()
            .unwrap()
            .placements
            .placements
            .is_empty()
    );
    let first = snapshot(flat());
    let p = pending(&session, Some(first.clone()), root);
    adopt(&mut state, &mut session, &mut editor, p).unwrap();
    state.stroke = None;
    let mut stale = pending(&session, None, root);
    let EditorCommand::VolumeSculptChanged(change) = session.undo[0].clone() else {
        panic!()
    };
    stale.history = Some((true, change));
    session.undo.push(EditorCommand::TerrainChanged(Vec::new()));
    assert!(adopt(&mut state, &mut session, &mut editor, stale).is_err());
    assert_eq!(session.undo.len(), 2);
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements[0],
        first.record
    );
}

#[test]
fn volume_confirmed_line_carves_middle_and_records_one_complete_undo() {
    let (mut session, mut state, mut editor) = fixture();
    let before = snapshot(flat());
    session
        .draft
        .as_mut()
        .unwrap()
        .placements
        .placements
        .push(before.record.clone());
    let brush = VolumeBrush {
        kind: VolumeBrushKind::Dig,
        from: [2., 3.5, 4.],
        to: [6., 3.5, 4.],
        radius: 0.5,
        half_height: 0.5,
        shape: VolumeBrushShape::Box,
        strength: 1.,
        ..default()
    };
    let after = snapshot(apply_volume_brush(dense(&before), brush).unwrap());
    let density = dense(&after);
    let side = density.cells as usize + 1;
    for x in 4..=12 {
        assert!(
            density.density[x + side * (7 + side * 8)] > 0,
            "整段中部也应开挖，不能只挖两个端点"
        );
    }
    state.stroke.as_mut().unwrap().new_region = false;
    state.stroke.as_mut().unwrap().before = Some(before.clone());
    state.stroke.as_mut().unwrap().released = true;
    let root = World::new().spawn_empty().id();
    let adoption = pending(&session, Some(after.clone()), root);
    adopt(&mut state, &mut session, &mut editor, adoption).unwrap();
    finish_stroke(&mut state, &mut session);
    assert!(state.stroke.is_none());
    assert_eq!(session.undo.len(), 1);
    let EditorCommand::VolumeSculptChanged(change) = &session.undo[0] else {
        panic!("直线必须记录完整体积源历史");
    };
    assert!(Arc::ptr_eq(
        &change.before.as_ref().unwrap().source,
        &before.source
    ));
    assert!(Arc::ptr_eq(&change.after.source, &after.source));
}

#[test]
fn volume_adoption_preflight_cancels_changed_record_revision_history_and_timeout() {
    for reason in 0..4 {
        let (mut session, mut state, editor) = fixture();
        let first = snapshot(flat());
        let mut app = App::new();
        let root = app.world_mut().spawn_empty().id();
        let mut pending = pending(&session, Some(first.clone()), root);
        match reason {
            0 => session.revision += 1,
            1 => session
                .draft
                .as_mut()
                .unwrap()
                .placements
                .placements
                .push(first.record.clone()),
            2 => {
                pending.history = Some((
                    true,
                    Box::new(VolumeSculptChange {
                        scene_id: session.draft.as_ref().unwrap().scene_id.clone(),
                        index: 0,
                        before: None,
                        after: first.clone(),
                    }),
                ));
                session.undo.push(EditorCommand::TerrainChanged(Vec::new()));
            }
            _ => (),
        }
        let author_count = session.draft.as_ref().unwrap().placements.placements.len();
        let undo_count = session.undo.len();
        let mut request = TerrainVolumePreviewRequest::default();
        request.target = Some(feathrise_scene::TerrainVolumePreviewTarget {
            scene_id: session.draft.as_ref().unwrap().scene_id.clone(),
            entity: root,
            record: first.record.clone(),
            world_cutout: first.record.terrain_patch.as_ref().unwrap().cutout.unwrap(),
            removal: false,
        });
        state.register_source(&first.source);
        state.adoption = Some(pending);
        let mut time = Time::<Real>::default();
        if reason == 3 {
            time.advance_by(std::time::Duration::from_secs(31));
        }
        app.insert_resource(session)
            .insert_resource(state)
            .insert_resource(editor)
            .insert_resource(request)
            .insert_resource(time)
            .add_systems(Update, validate);
        app.update();
        let state = app.world().resource::<VolumeAuthoringState>();
        assert!(state.adoption.is_none());
        assert!(state.stroke.as_ref().unwrap().released);
        assert!(
            app.world()
                .resource::<TerrainVolumePreviewRequest>()
                .target
                .is_none()
        );
        assert!(app.world().get_entity(root).is_err());
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.undo.len(), undo_count);
        assert_eq!(
            session.draft.as_ref().unwrap().placements.placements.len(),
            author_count
        );
    }
}

#[test]
fn volume_streamed_candidate_waits_for_complete_upload_without_a_thirty_second_total_gate() {
    let (session, mut state, editor) = fixture();
    let mut first = snapshot(flat());
    first.record.terrain_patch.as_mut().unwrap().streamed_volume = true;
    let mut app = App::new();
    let root = app.world_mut().spawn_empty().id();
    state.adoption = Some(pending(&session, Some(first), root));
    let mut time = Time::<Real>::default();
    time.advance_by(std::time::Duration::from_secs(120));
    app.insert_resource(session)
        .insert_resource(state)
        .insert_resource(editor)
        .insert_resource(TerrainVolumePreviewRequest::default())
        .insert_resource(time)
        .add_systems(Update, validate);
    app.update();
    assert!(
        app.world()
            .resource::<VolumeAuthoringState>()
            .adoption
            .is_some()
    );
    assert!(app.world().get_entity(root).is_ok());
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}

#[test]
fn volume_unique_density_budget_counts_shared_arc_once_and_rejects_more_than_64_mib() {
    let source = Arc::new(VolumeSource::Dense(
        volume_from_heights(64, 0.5, vec![16.; 65 * 65]).unwrap(),
    ));
    let mut state = VolumeAuthoringState::default();
    state.register_source(&source);
    state.register_source(&source);
    assert_eq!(state.holds.len(), 1);
    let mut retained = vec![source.clone()];
    while state.reserve_source().is_ok() {
        let next = Arc::new((*source).clone());
        state.register_source(&next);
        retained.push(next);
    }
    assert!(retained.len() > 100 && retained.len() < 128);
    assert!(state.holds.iter().map(|s| s.memory_bytes()).sum::<usize>() <= HISTORY_BYTES);
    assert!(state.reserve_source().is_err());
    state.reserve_restore().expect("额度满时仍可恢复既有历史源");
    let mut current = snapshot(flat());
    current.source = source.clone();
    current.record.terrain_patch.as_mut().unwrap().cutout =
        Some(feathrise_scene_content::SceneTerrainCutout {
            min: [0.; 2],
            max: [32.; 2],
        });
    let JobOutput::Prepared(restored) = jobs::restore(current, "scenes/EDGE-TEST".into()).unwrap()
    else {
        panic!()
    };
    assert!(Arc::ptr_eq(&restored.snapshot.source, &source));
}

#[test]
fn volume_author_jobs_accept_clipped_edits_and_report_boundary_without_an_error() {
    let mut current = snapshot(volume_from_heights(64, 0.125, vec![4.; 65 * 65]).unwrap());
    let mut prepared_bytes = None;
    let mut changed_after_clipping = false;
    for _ in 0..10 {
        let mesh = build_author_volume_mesh(&current.source).unwrap();
        let index = feathrise_terrain::build_terrain_editing_index(&mesh).unwrap();
        let floor = index
            .ray_intersection(&mesh.positions, &mesh.indices, [4., 8., 4.], [0., -1., 0.])
            .unwrap();
        let brush = VolumeBrush {
            kind: VolumeBrushKind::Dig,
            from: floor.position,
            to: floor.position,
            radius: 2.,
            strength: 1.,
            ..Default::default()
        };
        let clipped = brush.is_clipped_by_boundary(dense(&current));
        match jobs::edit(current.clone(), brush, "scenes/EDGE-TEST".into()).unwrap() {
            JobOutput::Prepared(prepared) => {
                changed_after_clipping |= clipped;
                assert_ne!(*prepared.snapshot.source, *current.source);
                current = prepared.snapshot;
                prepared_bytes = Some(prepared.bytes);
            }
            JobOutput::Unchanged { at_boundary } => {
                assert!(at_boundary);
                assert!(changed_after_clipping);
                assert_eq!(
                    decode_volume_glb(&prepared_bytes.unwrap()).unwrap(),
                    *dense(&current)
                );
                return;
            }
            _ => panic!("已有源编辑不能变为重开或缺页等待"),
        }
    }
    panic!("真正到达固定区域底部时必须返回边界无改动，而不是无限生成新源");
}

#[test]
fn volume_saved_source_reopens_and_rejects_external_mesh_density_mismatch() {
    let temporary = std::env::temp_dir().join(format!(
        "bevy-volume-source-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&temporary).unwrap();
    let mut snapshot = snapshot(flat());
    let mesh = build_author_volume_mesh(&snapshot.source).unwrap();
    let bytes = encode_author_volume_glb(&snapshot.source, &mesh).unwrap();
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    snapshot.record.asset_path = format!("v-{hash}.glb");
    std::fs::write(temporary.join(&snapshot.record.asset_path), &bytes).unwrap();
    let JobOutput::Opened(opened) = jobs::open_at(&temporary, snapshot.record.clone()).unwrap()
    else {
        panic!()
    };
    assert_eq!(*opened.source, *snapshot.source);
    let mut mismatched = mesh;
    mismatched.positions[0][1] += 0.5;
    let bytes = encode_author_volume_glb(&snapshot.source, &mismatched).unwrap();
    let hash: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    snapshot.record.asset_path = format!("v-{hash}.glb");
    std::fs::write(temporary.join(&snapshot.record.asset_path), bytes).unwrap();
    assert!(jobs::open_at(&temporary, snapshot.record).is_err());
    assert_eq!(temporary.parent(), Some(std::env::temp_dir().as_path()));
    std::fs::remove_dir_all(temporary).unwrap(); // 只删除本测试独占临时根。
}
