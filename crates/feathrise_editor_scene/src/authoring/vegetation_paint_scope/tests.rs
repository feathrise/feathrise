//! 跨网格刷草的真实表面、历史与持久化回归。
use super::{model, resources::*, systems, types::*, worker};
use crate::authoring::{
    EditorAuthoringSession, EditorCommand,
    terrain_documents::{TerrainCanvas, TerrainDocumentIdentity},
    terrain_ground::TerrainGroundSnapshot,
};
use crate::resources::SceneEditorState;
use bevy::prelude::*;
use feathrise_scene_content::*;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

fn plane() -> avian3d::prelude::Collider {
    avian3d::prelude::Collider::trimesh(
        vec![
            Vec3::new(-2., 0., -2.),
            Vec3::new(2., 0., -2.),
            Vec3::new(2., 0., 2.),
            Vec3::new(-2., 0., 2.),
        ],
        vec![[0, 2, 1], [0, 3, 2]],
    )
}
fn fixture() -> (crate::authoring::EditorSceneDraft, Transaction) {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut a = draft.manifest.terrain_objects_for_edit()[0].clone();
    a.id = TerrainId::parse("island-a").unwrap();
    a.world_min = [-2.; 2];
    a.world_max = [2.; 2];
    a.terrain.kind = SceneTerrainKind::Gltf;
    a.terrain.glb_path = "gltf/test.glb".into();
    a.transform = TerrainObjectTransform::default();
    a.surface_path = None;
    a.water_path = None;
    a.vegetation_path = None;
    let mut b = a.clone();
    b.id = TerrainId::parse("island-b").unwrap();
    b.transform.translation = [3., 0., 0.];
    b.transform.yaw_degrees = 90.;
    b.transform.scale = [2., 1., 0.5];
    let objects: Arc<[_]> = vec![a, b].into();
    draft.manifest.terrain = SceneTerrainDescriptor::empty();
    draft.manifest.terrains = Some(objects.clone());
    let template = SceneVegetationLayer {
        id: "grass".into(),
        label: "草".into(),
        kind: SceneVegetationKind::Blade,
        runtime_profile: Some("ordinary_grass".into()),
        runtime_layer_id: Some(1),
        prototype_id: None,
        asset_path: "grass.glb".into(),
        density_source_path: "unused.r8".into(),
        density_texture_path: "unused.ktx2".into(),
        resolution: [33; 2],
        spacing_meters: 0.25,
        scale_range: [0.5, 1.2],
        max_instances: u32::MAX,
        seed: 12345,
    };
    let group = SceneVegetationPaintGroup {
        id: "draw-test".into(),
        label: "草地绘制组".into(),
        template,
        targets: objects.iter().map(|o| o.id.clone()).collect(),
        members: BTreeMap::new(),
    };
    let groups: Arc<[_]> = vec![group.clone()].into();
    draft.manifest.vegetation_paint_groups = groups.clone();
    let ground = TerrainGroundSnapshot::test_meshes(draft.scene_id.clone(), &objects, plane());
    draft.terrain_ground = Some(ground.clone());
    let documents = objects
        .iter()
        .map(|object| {
            let identity = TerrainDocumentIdentity {
                definition: Arc::new(object.clone()),
                manifest: object.source_manifest(&draft.manifest),
                folder: object.private_folder(&draft.scene_id),
            };
            (
                object.id.clone(),
                Arc::new(TerrainCanvas {
                    identity: Some(identity),
                    vegetation: Some(SceneVegetationAsset {
                        scene_id: draft.scene_id.clone(),
                        schema_version: SCENE_SCHEMA_VERSION,
                        authoring: Some(
                            crate::authoring::vegetation_defaults::empty_vegetation_authoring(),
                        ),
                        layers: Vec::new(),
                    }),
                    ..default()
                }),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut active = (*documents[&objects[0].id]).clone();
    active.swap(&mut draft);
    draft
        .terrain_documents
        .values
        .insert(objects[1].id.clone(), documents[&objects[1].id].clone());
    let transaction = Transaction {
        scene_id: draft.scene_id.clone(),
        group,
        groups_before: groups,
        documents,
        before: BTreeMap::new(),
        changed: BTreeSet::new(),
        ground,
        hidden: Arc::default(),
    };
    (draft, transaction)
}
fn dab(point: Vec3, radius: f32) -> Dab {
    Dab {
        from: point,
        to: point,
        radius,
        amount: 0.7,
        exposure: 0.,
        erase: false,
        camera: Transform::from_translation(point + Vec3::new(0., 12., 0.2))
            .looking_at(point, Vec3::Z)
            .to_matrix(),
        orthographic: false,
    }
}
fn byte_sum(canvas: &TerrainCanvas) -> u64 {
    canvas
        .vegetation_author_densities
        .values()
        .flat_map(|p| p.iter())
        .map(|v| u64::from(*v))
        .sum()
}

#[test]
fn cross_mesh_one_dab_paints_both_rotated_scaled_local_canvases_without_merging_layers() {
    let (_, transaction) = fixture();
    let batch = worker::paint(
        transaction,
        VecDeque::from([dab(Vec3::new(2., 0., 0.3), 1.1)]),
    )
    .unwrap();
    assert_eq!(batch.transaction.changed.len(), 2);
    assert_eq!(batch.prepared.len(), 2);
    assert_eq!(batch.transaction.group.members.len(), 2);
    for (id, canvas) in &batch.transaction.documents {
        assert!(byte_sum(canvas) > 0);
        let v = canvas.vegetation.as_ref().unwrap();
        assert_eq!(v.layers[0].seed, 12345);
        assert!(
            v.layers[0]
                .density_source_path
                .starts_with(&id.private_test_folder(&batch.transaction.scene_id))
        );
        let object = &canvas.identity.as_ref().unwrap().definition;
        for (index, value) in canvas
            .vegetation_author_densities
            .values()
            .next()
            .unwrap()
            .iter()
            .enumerate()
        {
            if *value == 0 {
                continue;
            }
            let point = Vec3::new(
                -2. + 4. * (index % 33) as f32 / 32.,
                0.,
                -2. + 4. * (index / 33) as f32 / 32.,
            );
            let world = object
                .transform
                .as_transform()
                .to_matrix()
                .transform_point3(point);
            assert!(
                world.xz().distance(Vec2::new(2., 0.3)) <= 1.1001,
                "世界圆刷必须在源旋转、非等比缩放后仍保持圆形"
            );
        }
    }
}

#[test]
fn continuous_cross_grass_long_thin_stroke_drains_every_prefix_without_losing_local_ownership() {
    let (_, mut transaction) = fixture();
    let id = TerrainId::parse("island-a").unwrap();
    transaction.group.targets.retain(|target| *target == id);
    transaction.documents.retain(|target, _| *target == id);
    transaction.group.template.resolution = [129, 33];
    let mut line = dab(Vec3::ZERO, 0.05);
    line.from = Vec3::new(-1.7, 0.0, 0.0);
    line.to = Vec3::new(1.7, 0.0, 0.0);
    line.orthographic = true;
    line.camera = Transform::from_xyz(0.0, 12.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z).to_matrix();
    let mut remaining = VecDeque::from([line]);
    let mut batches = 0;
    while !remaining.is_empty() {
        let batch = worker::paint(transaction, remaining).unwrap();
        assert!(batch.waiting.is_empty());
        if batches == 0 { assert!(batch.remaining.front().unwrap().from.x < -1.0,
            "第一批最多推进32枚相连刷印，不能跨全段稀释"); }
        remaining = batch.remaining;
        transaction = batch.transaction;
        batches += 1;
        assert!(batches < 16);
    }
    assert!(batches > 1);
    assert_eq!(transaction.group.members.len(), 1);
    let density = &transaction.documents[&id].vegetation_author_densities[&transaction.group.members[&id]];
    for x in 11..=117 { assert!(density[16 * 129 + x] > 0, "跨网格细刷第{x}列不得漏刷"); }
}

#[test]
fn cross_mesh_occlusion_and_empty_erase_do_not_create_hidden_or_lower_members() {
    let (_, mut transaction) = fixture();
    let ids = transaction.documents.keys().cloned().collect::<Vec<_>>();
    let mut objects = transaction
        .documents
        .values()
        .map(|c| (*c.identity.as_ref().unwrap().definition).clone())
        .collect::<Vec<_>>();
    objects[1].transform = TerrainObjectTransform {
        translation: [0., 0.4, 0.],
        ..default()
    };
    for object in &objects {
        let c = Arc::make_mut(transaction.documents.get_mut(&object.id).unwrap());
        c.identity.as_mut().unwrap().definition = Arc::new(object.clone());
    }
    transaction.ground =
        TerrainGroundSnapshot::test_meshes(transaction.scene_id.clone(), &objects, plane());
    let batch = worker::paint(
        transaction.clone(),
        VecDeque::from([dab(Vec3::new(0., 0.4, 0.), 2.)]),
    )
    .unwrap();
    assert_eq!(batch.transaction.changed, BTreeSet::from([ids[1].clone()]));
    assert!(!batch.transaction.group.members.contains_key(&ids[0]));
    let mut erase = dab(Vec3::ZERO, 2.);
    erase.erase = true;
    let erased = worker::paint(transaction, VecDeque::from([erase])).unwrap();
    assert!(erased.transaction.changed.is_empty());
    assert!(erased.transaction.group.members.is_empty());
}

#[test]
fn cross_mesh_no_hit_and_failed_last_member_leave_every_source_unchanged() {
    let (_, mut transaction) = fixture();
    let original = transaction.documents.clone();
    let batch = worker::paint(
        transaction.clone(),
        VecDeque::from([dab(Vec3::new(100., 0., 100.), 1.)]),
    )
    .unwrap();
    assert!(batch.transaction.changed.is_empty());
    assert!(batch.transaction.group.members.is_empty());
    let last = transaction.documents.keys().next_back().unwrap().clone();
    transaction
        .group
        .members
        .insert(last, "missing-author".into());
    assert!(
        worker::paint(
            transaction,
            VecDeque::from([dab(Vec3::new(2., 0., 0.), 1.)])
        )
        .is_err()
    );
    for canvas in original.values() {
        assert_eq!(byte_sum(canvas), 0);
        assert!(canvas.vegetation.as_ref().unwrap().layers.is_empty());
    }
}

#[test]
fn cross_mesh_history_restores_all_members_and_keeps_newer_mesh_pose() {
    let (draft, transaction) = fixture();
    let mut session = EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    };
    let mut editor = SceneEditorState::default();
    let mut state = VegetationPaintState::default();
    let batch = worker::paint(
        transaction,
        VecDeque::from([dab(Vec3::new(2., 0., 0.), 1.2)]),
    )
    .unwrap();
    systems::adopt(batch, &mut state, &mut session, &mut editor);
    let transaction = state.transaction.as_ref().unwrap();
    let change = VegetationPaintChange {
        before_groups: transaction.groups_before.clone(),
        after_groups: session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .vegetation_paint_groups
            .clone(),
        canvases: transaction
            .changed
            .iter()
            .map(|id| {
                (
                    id.clone(),
                    (
                        transaction.before[id].clone(),
                        VegetationValues::capture(&transaction.documents[id]),
                    ),
                )
            })
            .collect(),
    };
    crate::authoring::terrain_documents::scope_history(&mut session);
    assert_eq!(session.undo.len(), 1);
    assert!(matches!(
        session.undo[0],
        EditorCommand::VegetationPaintChanged(_)
    ));
    let draft = session.draft.as_mut().unwrap();
    let objects = Arc::make_mut(draft.manifest.terrains.as_mut().unwrap());
    objects[0].transform.translation = [100., 7., -20.];
    objects[0].transform.yaw_degrees = 37.;
    model::apply_history(draft, &change, true).unwrap();
    assert!(draft.vegetation_author_densities.is_empty());
    assert!(draft.manifest.vegetation_paint_groups[0].members.is_empty());
    assert!(
        draft
            .terrain_documents
            .values
            .values()
            .all(|v| v.vegetation_author_densities.is_empty())
    );
    model::apply_history(draft, &change, false).unwrap();
    assert_eq!(draft.manifest.vegetation_paint_groups[0].members.len(), 2);
    assert_eq!(
        draft.manifest.terrains.as_ref().unwrap()[0]
            .transform
            .translation,
        [100., 7., -20.]
    );
    assert_eq!(
        draft.manifest.terrains.as_ref().unwrap()[0]
            .transform
            .yaw_degrees,
        37.
    );
    for (id, (_, after)) in &change.canvases {
        let values = if draft
            .terrain_documents
            .active
            .as_ref()
            .unwrap()
            .definition
            .id
            == *id
        {
            draft.vegetation_author_densities.clone()
        } else {
            draft.terrain_documents.values[id]
                .vegetation_author_densities
                .clone()
        };
        for (key, pixels) in &after.authors {
            assert!(Arc::ptr_eq(&values[key], pixels));
        }
    }
}

#[test]
fn cross_mesh_group_registry_round_trip_keeps_local_members_and_no_runtime_global_canvas() {
    let (mut draft, transaction) = fixture();
    let batch = worker::paint(
        transaction,
        VecDeque::from([dab(Vec3::new(2., 0., 0.), 1.2)]),
    )
    .unwrap();
    draft.manifest.vegetation_paint_groups = vec![batch.transaction.group].into();
    let bytes = serde_json::to_vec(&draft.manifest).unwrap();
    let reopened: SceneManifestAsset = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reopened.vegetation_paint_groups[0].members.len(), 2);
    validate_terrain_objects(&reopened).unwrap();
    assert!(reopened.terrains.as_ref().unwrap().iter().all(|o| {
        o.source_manifest(&reopened)
            .vegetation_paint_groups
            .is_empty()
    }));
}

trait TestFolder {
    fn private_test_folder(&self, map: &SceneId) -> String;
}
impl TestFolder for TerrainId {
    fn private_test_folder(&self, map: &SceneId) -> String {
        format!("scenes/{map}/terrains/{}/", self.as_str())
    }
}

pub(crate) fn painted_draft() -> crate::authoring::EditorSceneDraft {
    let (draft, transaction) = fixture();
    let mut session = EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    };
    let batch = worker::paint(
        transaction,
        VecDeque::from([dab(Vec3::new(2., 0., 0.), 1.2)]),
    )
    .unwrap();
    systems::adopt(
        batch,
        &mut VegetationPaintState::default(),
        &mut session,
        &mut SceneEditorState::default(),
    );
    session.draft.unwrap()
}

fn input_app() -> App {
    let mut app = crate::authoring::vegetation_paint::object_tests::fixture();
    let (draft, _) = fixture();
    let map = draft.scene_id.clone();
    let group = draft.manifest.vegetation_paint_groups[0].id.clone();
    app.insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .init_asset::<Image>()
    .init_resource::<ActiveDynamicScene>()
    .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(33),
    ));
    super::install(&mut app);
    app.configure_sets(
        Update,
        (
            crate::SceneEditorSystemSet::Input,
            crate::SceneEditorSystemSet::Tasks,
            crate::SceneEditorSystemSet::Cursor,
            crate::SceneEditorSystemSet::Presentation,
        )
            .chain(),
    );
    *app.world_mut().resource_mut::<VegetationPaintSettings>() = VegetationPaintSettings {
        scene_id: Some(map.clone()),
        scope: PaintScope::Cross,
        group: Some(group),
    };
    let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
    editor.selected_scene_id = Some(map);
    editor.brush_radius = 1.2;
    editor.brush_strength = 0.7;
    editor.terrain_canvas_target = Some(TerrainId::parse("island-a").unwrap());
    app.world_mut()
        .resource_mut::<crate::resources::SceneEditorViewportState>()
        .center = Vec3::new(2., 0., 0.);
    // 起笔射线落在 A 的真实表面内，笔刷半径仍覆盖与 B 的接缝。
    move_cross_pointer(&mut app, -0.02);
    app
}
fn step(app: &mut App) {
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    std::thread::sleep(std::time::Duration::from_millis(1));
}
fn until(app: &mut App, done: impl Fn(&App) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while !done(app) {
        assert!(
            std::time::Instant::now() < deadline,
            "跨网格后台未收口：{}",
            app.world().resource::<SceneEditorState>().status
        );
        step(app);
    }
}

#[test]
fn cross_mesh_real_camera_hold_release_records_one_complete_history_and_keeps_panel_identity() {
    let mut app = input_app();
    until(&mut app, |a| {
        a.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .is_some()
    });
    let objects = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .manifest
        .terrains
        .clone()
        .unwrap();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    until(&mut app, |a| {
        a.world()
            .resource::<EditorAuthoringSession>()
            .vegetation_revision
            >= 2
    });
    let group_identity = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap()
        .manifest
        .vegetation_paint_groups
        .clone();
    let revision = app
        .world()
        .resource::<EditorAuthoringSession>()
        .vegetation_revision;
    until(&mut app, |a| {
        a.world()
            .resource::<EditorAuthoringSession>()
            .vegetation_revision
            > revision
    });
    assert!(Arc::ptr_eq(
        &group_identity,
        &app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .vegetation_paint_groups
    ));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    until(&mut app, |a| {
        !a.world().resource::<VegetationPaintState>().busy()
    });
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(!session.vegetation_cross_busy);
    let EditorCommand::VegetationPaintChanged(change) = &session.undo[0] else {
        panic!("整笔必须是场景级命令")
    };
    assert_eq!(change.canvases.len(), 2);
    assert_eq!(change.after_groups[0].members.len(), 2);
    assert!(Arc::ptr_eq(
        &objects,
        session
            .draft
            .as_ref()
            .unwrap()
            .manifest
            .terrains
            .as_ref()
            .unwrap()
    ));
    assert_eq!(
        app.world()
            .resource::<SceneEditorState>()
            .terrain_canvas_target
            .as_ref(),
        Some(&TerrainId::parse("island-a").unwrap())
    );
    install_history(&mut app);
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .undo_requested = true;
    step(&mut app);
    until(&mut app, |a| {
        !a.world().resource::<VegetationPaintState>().busy()
    });
    let draft = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap();
    assert!(draft.vegetation_author_densities.is_empty());
    assert!(
        draft
            .terrain_documents
            .values
            .values()
            .all(|c| c.vegetation_author_densities.is_empty())
    );
    assert!(draft.manifest.vegetation_paint_groups[0].members.is_empty());
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .redo_requested = true;
    step(&mut app);
    until(&mut app, |a| {
        !a.world().resource::<VegetationPaintState>().busy()
    });
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
    let draft = app
        .world()
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap();
    assert_eq!(draft.manifest.vegetation_paint_groups[0].members.len(), 2);
    assert!(
        draft
            .vegetation_author_densities
            .values()
            .any(|p| p.iter().any(|v| *v > 0))
    );
    assert!(
        draft
            .terrain_documents
            .values
            .values()
            .any(|c| byte_sum(c) > 0)
    );
}

#[test]
fn cross_mesh_locked_target_is_excluded_and_late_source_generation_is_discarded() {
    let mut app = input_app();
    let other = TerrainId::parse("island-b").unwrap();
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .terrain_locked
        .insert(other.clone());
    until(&mut app, |a| {
        a.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .is_some()
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    until(&mut app, |a| {
        a.world()
            .resource::<EditorAuthoringSession>()
            .vegetation_revision
            > 0
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    until(&mut app, |a| {
        !a.world().resource::<VegetationPaintState>().busy()
    });
    let session = app.world().resource::<EditorAuthoringSession>();
    let EditorCommand::VegetationPaintChanged(change) = &session.undo[0] else {
        panic!()
    };
    assert_eq!(change.canvases.len(), 1);
    assert!(!change.canvases.contains_key(&other));

    let mut app = input_app();
    until(&mut app, |a| {
        a.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .is_some()
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app);
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .terrain_revision = 1; // 结果在后台完成也不能借新源回写。
    until(&mut app, |a| {
        !a.world().resource::<VegetationPaintState>().busy()
    });
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .unwrap()
            .vegetation_author_densities
            .is_empty()
    );
}

fn install_history(app: &mut App) {
    use crate::authoring::*;
    app.insert_resource(EditorPreviewAssets {
        portal_base_mesh: default(),
        portal_arrow_head_mesh: default(),
        portal_arrow_shaft_mesh: default(),
        portal_edge_beam_mesh: default(),
        portal_edge_ring_mesh: default(),
        portal_base_material: default(),
        portal_arrow_material: default(),
        portal_edge_beam_material: default(),
        portal_edge_ring_material: default(),
        portal_label_font: default(),
    })
    .init_resource::<crate::authoring::courtyards::CourtyardAuthoringState>()
    .init_resource::<GlobalEnvironmentAuthoringState>()
    .init_resource::<WaterMaskMaterializeTask>()
    .init_resource::<crate::authoring::surface_layer_edit::SurfaceLayerEditTask>()
    .init_resource::<SurfaceBrushStroke>()
    .init_resource::<WaterBrushStroke>()
    .init_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
    .init_resource::<WaterFileHandoff>()
    .add_systems(
        Update,
        apply_undo_redo.in_set(crate::SceneEditorSystemSet::Input),
    );
}

#[test]
fn cross_mesh_budget_refuses_oversized_targets_before_any_layer_is_adopted() {
    let (_, mut transaction) = fixture();
    transaction.group.template.resolution = [4096; 2];
    let before = transaction.documents.clone();
    match worker::paint(
        transaction,
        VecDeque::from([dab(Vec3::new(2., 0., 0.), 1.)]),
    ) {
        Err(error) => assert!(error.contains("256MiB")),
        Ok(_) => panic!("超预算不能采用"),
    };
    for canvas in before.values() {
        assert_eq!(byte_sum(canvas), 0);
    }
}

#[test]
fn cross_mesh_same_content_page_snapshot_refresh_does_not_end_the_frozen_stroke() {
    let mut app = input_app();
    until(&mut app, |a| {
        a.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .is_some()
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    until(&mut app, |a| {
        a.world()
            .resource::<EditorAuthoringSession>()
            .vegetation_revision
            > 0
    });
    let revision = app
        .world()
        .resource::<EditorAuthoringSession>()
        .vegetation_revision;
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let objects = draft.manifest.terrains.as_ref().unwrap();
        draft.terrain_ground = Some(TerrainGroundSnapshot::test_meshes(
            draft.scene_id.clone(),
            objects,
            plane(),
        ));
    }
    until(&mut app, |a| {
        a.world()
            .resource::<EditorAuthoringSession>()
            .vegetation_revision
            > revision
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    until(&mut app, |a| {
        !a.world().resource::<VegetationPaintState>().busy()
    });
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn cross_mesh_missing_pages_keep_brush_segments_and_do_not_spin_same_snapshot() {
    let mut app = input_app();
    until(&mut app, |a| {
        a.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .is_some()
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app);
    // 精确模拟worker的缺页完成协议；真实表面/遮挡算法由上面的Collider回归单独核对。
    until(&mut app, |a| {
        a.world().resource::<VegetationPaintState>().work.is_none()
    });
    let mut state = app.world_mut().resource_mut::<VegetationPaintState>();
    let transaction = state.transaction.as_ref().unwrap().clone();
    let ground = transaction.ground.clone();
    state.staging = None;
    state.queue = VecDeque::from([dab(Vec3::new(2., 0., 0.), 1.)]);
    state.waiting_ground = Some(ground);
    drop(state);
    step(&mut app);
    assert!(
        app.world()
            .resource::<VegetationPaintState>()
            .work
            .is_none()
    );
    assert!(
        !app.world()
            .resource::<VegetationPaintState>()
            .queue
            .is_empty()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.terrain_ground = Some(TerrainGroundSnapshot::test_meshes(
            draft.scene_id.clone(),
            draft.manifest.terrains.as_ref().unwrap(),
            plane(),
        ));
    }
    until(&mut app, |a| {
        !a.world().resource::<VegetationPaintState>().busy()
    });
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn cross_mesh_selection_mode_and_held_mode_switch_cannot_start_painting() {
    let mut app = input_app();
    app.init_resource::<crate::viewport::modes::ViewportModeState>(); // 默认选择模式，即使记忆工具仍为植被也不能写。
    until(&mut app, |a| {
        a.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .is_some()
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    for _ in 0..4 {
        step(&mut app);
    }
    assert!(!app.world().resource::<VegetationPaintState>().busy());
    app.world_mut()
        .resource_mut::<crate::viewport::modes::ViewportModeState>()
        .mode = crate::viewport::modes::ViewportMode::Edit;
    for _ in 0..4 {
        step(&mut app);
    }
    assert!(
        !app.world().resource::<VegetationPaintState>().busy(),
        "切模式不能复活同次旧按压"
    );
}

#[test]
fn cross_mesh_waits_for_previous_author_compile_before_freezing_history_baseline() {
    let mut app = input_app();
    until(&mut app, |a| {
        a.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .is_some()
    });
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .vegetation_revision = 1;
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    step(&mut app);
    assert!(
        app.world()
            .resource::<VegetationPaintState>()
            .press
            .is_some()
    );
    assert!(
        app.world()
            .resource::<VegetationPaintState>()
            .work
            .is_none()
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    ); // 首帧射线可能仍在等待；未取得完整基线不能提前占历史。
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .vegetation_compiled_revision = 1;
    until(&mut app, |a| {
        a.world()
            .resource::<EditorAuthoringSession>()
            .vegetation_revision
            > 1
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    until(&mut app, |a| {
        !a.world().resource::<VegetationPaintState>().busy()
    });
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn cross_mesh_new_members_use_enabled_local_groups_without_inheriting_hidden_first_group() {
    let (_, mut transaction) = fixture();
    for canvas in transaction.documents.values_mut() {
        Arc::make_mut(canvas)
            .vegetation
            .as_mut()
            .unwrap()
            .authoring
            .as_mut()
            .unwrap()
            .groups[0]
            .enabled = false;
    }
    let batch = worker::paint(
        transaction,
        VecDeque::from([dab(Vec3::new(2., 0., 0.), 1.2)]),
    )
    .unwrap();
    for canvas in batch.transaction.documents.values() {
        assert!(
            !canvas
                .vegetation
                .as_ref()
                .unwrap()
                .authoring
                .as_ref()
                .unwrap()
                .groups[0]
                .enabled
        );
        assert!(
            canvas
                .vegetation_densities
                .values()
                .any(|p| p.iter().any(|v| *v > 0))
        );
    }
}

#[test]
fn cross_mesh_cancelled_old_worker_cannot_append_history_after_a_new_author_operation() {
    let mut app = input_app();
    until(&mut app, |a| {
        a.world()
            .resource::<SceneEditorState>()
            .brush_cursor_world
            .is_some()
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    until(&mut app, |a| {
        a.world()
            .resource::<EditorAuthoringSession>()
            .vegetation_revision
            > 0
    });
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1,
        "第一份采用就固定本笔的历史位置"
    );
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let before = SceneTerrainCollection::capture(&draft.manifest);
        Arc::make_mut(draft.manifest.terrains.as_mut().unwrap())[0].display_name =
            "外部新名称".into();
        let after = SceneTerrainCollection::capture(&draft.manifest);
        session
            .undo
            .push(EditorCommand::TerrainObjectsChanged { before, after });
        session.revision = session.revision.wrapping_add(1);
    }
    until(&mut app, |a| {
        !a.world().resource::<VegetationPaintState>().busy()
    });
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 2);
    assert!(matches!(
        session.undo[0],
        EditorCommand::VegetationPaintChanged(_)
    ));
    assert!(matches!(
        session.undo[1],
        EditorCommand::TerrainObjectsChanged { .. }
    ));
}

fn move_cross_pointer(app: &mut App, x: f32) {
    let world = app.world_mut();
    let mut viewport = world.query_filtered::<
        &mut bevy::ui::RelativeCursorPosition,
        With<crate::components::SceneEditorViewport>,
    >();
    viewport.single_mut(world).unwrap().normalized = Some(Vec2::new(x, 0.0));
}

fn continuous_cross_app() -> App {
    let mut app = input_app();
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let groups = &mut session.draft.as_mut().unwrap().manifest.vegetation_paint_groups;
    Arc::make_mut(groups)[0].template.resolution = [129; 2];
    drop(session);
    let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
    editor.brush_radius = 0.05;
    editor.brush_strength = 1.0;
    drop(editor);
    // 从A内部起笔，向右拖入旋转且非等比缩放的B；5厘米圆刷使两米长段必须分批。
    move_cross_pointer(&mut app, -0.10);
    app
}

fn start_cross_hold(app: &mut App) -> Vec3 {
    use crate::authoring::terrain_ground::cursor_running_for_tests;
    until(app, |app| {
        app.world().resource::<SceneEditorState>().brush_cursor_world.is_some()
            && !cursor_running_for_tests(app.world())
    });
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    until(app, |app| {
        let state = app.world().resource::<VegetationPaintState>();
        state.applied && state.last.is_some() && !cursor_running_for_tests(app.world())
    });
    let start = app.world().resource::<VegetationPaintState>().last.unwrap();
    assert!(start.x > 0.0 && start.x < 1.0, "真实相机起点必须位于A内部：{start:?}");
    start
}

fn cross_member_pixels(app: &App) -> BTreeMap<TerrainId, Arc<Vec<u8>>> {
    let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
    draft.manifest.vegetation_paint_groups[0].members.iter().map(|(id, layer)| {
        let pixels = if draft.terrain_documents.active.as_ref().is_some_and(|active| active.definition.id == *id) {
            &draft.vegetation_author_densities[layer]
        } else {
            &draft.terrain_documents.values[id].vegetation_author_densities[layer]
        };
        (id.clone(), pixels.clone())
    }).collect()
}

fn assert_cross_line_has_no_gaps(app: &App, start: Vec3, end: Vec3) {
    let pixels = cross_member_pixels(app);
    assert_eq!(pixels.len(), 2, "同次拖动应分别创建A、B的本地作者层");
    let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
    let mut checked = BTreeMap::new();
    for object in draft.manifest.terrains.as_ref().unwrap().iter() {
        let transform = object.transform.as_transform().to_matrix();
        let mut count = 0;
        for z in 0..129 {
            for x in 0..129 {
                let point = transform.transform_point3(Vec3::new(
                    object.world_min[0] + (object.world_max[0] - object.world_min[0]) * x as f32 / 128.0,
                    0.0,
                    object.world_min[1] + (object.world_max[1] - object.world_min[1]) * z as f32 / 128.0,
                ));
                // 共同边的完全重合采样仅归射线实际可见的一侧；逐格检查两侧内部的真实轨迹。
                if point.z.abs() < 0.0001 && point.x >= start.x + 0.025
                    && point.x <= end.x - 0.025 && (point.x - 2.0).abs() > 0.025 {
                    assert!(pixels[&object.id][z * 129 + x] > 0,
                        "{}在世界轨迹{point:?}出现未排空断口", object.id.as_str());
                    count += 1;
                }
            }
        }
        checked.insert(object.id.clone(), count);
    }
    assert!(checked[&TerrainId::parse("island-a").unwrap()] >= 32);
    assert!(checked[&TerrainId::parse("island-b").unwrap()] >= 64);
}

fn undo_redo_cross_hold_once(app: &mut App, painted: &BTreeMap<TerrainId, Arc<Vec<u8>>>) {
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().reset_all();
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().reset_all();
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
        editor.save_requested = false;
        editor.close_requested = false;
        editor.undo_requested = true;
    }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
    install_history(app);
    step(app);
    until(app, |app| !app.world().resource::<VegetationPaintState>().busy());
    let session = app.world().resource::<EditorAuthoringSession>();
    assert!(session.undo.is_empty());
    assert_eq!(session.redo.len(), 1);
    let draft = session.draft.as_ref().unwrap();
    assert!(draft.manifest.vegetation_paint_groups[0].members.is_empty());
    assert!(draft.vegetation_author_densities.is_empty());
    assert!(draft.terrain_documents.values.values().all(|canvas| canvas.vegetation_author_densities.is_empty()),
        "一次生产撤销必须同时移除两个网格的惰性成员和全部已受理尾段");
    app.world_mut().resource_mut::<SceneEditorState>().redo_requested = true;
    step(app);
    until(app, |app| !app.world().resource::<VegetationPaintState>().busy());
    assert_eq!(cross_member_pixels(app), *painted, "一次重做应准确恢复两个本地画布的整笔密度");
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn continuous_cross_grass_delayed_real_camera_points_continue_while_strict_hover_is_empty() {
    use crate::authoring::terrain_ground::{TerrainGroundCursor, hold_cursor_result_for_tests};
    let mut app = continuous_cross_app();
    let start = start_cross_hold(&mut app);
    move_cross_pointer(&mut app, 0.02);
    step(&mut app);
    let release = hold_cursor_result_for_tests(app.world_mut());
    move_cross_pointer(&mut app, 0.10);
    for _ in 0..5 { step(&mut app); }
    let state = app.world().resource::<VegetationPaintState>();
    assert_eq!(state.last, Some(start), "真实射线在途不能把跨网格路径清成孤立圆印");
    assert!(state.waiting_exposure > 0.0, "等待同代查询也必须保留按住时间");
    assert!(app.world().resource::<SceneEditorState>().brush_cursor_world.is_none());
    release.send(()).unwrap();
    until(&mut app, |app| {
        app.world().resource::<VegetationPaintState>().last.is_some_and(|point| point.x > 2.0)
    });
    let accepted = app.world().resource::<VegetationPaintState>().last.unwrap();
    let cursor = app.world().resource::<TerrainGroundCursor>();
    assert!(cursor.paint_hit().is_some_and(|hit| hit.point.distance(accepted) < 0.0001));
    assert!(app.world().resource::<SceneEditorState>().brush_cursor_world.is_none(),
        "较早的真实完成点可以继续刷草，但不能冒充最新严格悬停");
    assert!(accepted.x < 3.0, "本帧必须消费受控延迟的中间真点，不能跳到鼠标终点");
    until(&mut app, |app| app.world().resource::<VegetationPaintState>().last.is_some_and(|point| point.x > 3.0));
    let end = app.world().resource::<VegetationPaintState>().last.unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<VegetationPaintState>().busy());
    assert_cross_line_has_no_gaps(&app, start, end);
    let painted = cross_member_pixels(&app);
    undo_redo_cross_hold_once(&mut app, &painted);
}

#[test]
fn continuous_cross_grass_accepted_long_drag_drains_on_save_and_close_with_one_undo() {
    use crate::authoring::terrain_ground::TerrainGroundCursor;
    for closing in [false, true] {
        let mut app = continuous_cross_app();
        let start = start_cross_hold(&mut app);
        move_cross_pointer(&mut app, 0.10);
        // 观察正式worker的真实余段，不手工注入命中或队列；第一批仍在完整图片发布门内。
        until(&mut app, |app| app.world().resource::<VegetationPaintState>().staging.as_ref()
            .and_then(|batch| batch.remaining.front())
            .is_some_and(|dab| dab.to.x > 3.0 && dab.to.x - dab.from.x > 1.0));
        let state = app.world().resource::<VegetationPaintState>();
        let remaining = state.staging.as_ref().unwrap().remaining.front().unwrap();
        let end = remaining.to;
        assert!(state.in_flight_inputs > 0, "待采用余段仍占原输入信用");
        assert!(remaining.from.x < 2.0, "保存/关闭前B侧尾段尚未处理完");
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
            if closing { editor.close_requested = true; } else { editor.save_requested = true; }
        }
        step(&mut app);
        assert!(app.world().resource::<TerrainGroundCursor>().input_authority().is_none(),
            "保存/关闭应撤销接收新点的光标权限");
        let state = app.world().resource::<VegetationPaintState>();
        assert!(state.busy() && state.ending && !state.cancelled,
            "正常收尾必须排空已受理长段，不能把光标作废当成异常取消");
        until(&mut app, |app| !app.world().resource::<VegetationPaintState>().busy());
        assert_cross_line_has_no_gaps(&app, start, end);
        let painted = cross_member_pixels(&app);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
        assert_eq!(app.world().resource::<SceneEditorState>().terrain_canvas_target.as_ref(),
            Some(&TerrainId::parse("island-a").unwrap()), "跨网格排空不能切走当前检查器画布");
        for _ in 0..4 { step(&mut app); }
        assert_eq!(cross_member_pixels(&app), painted, "请求仍保持且左键仍按住时不能追加新点");
        undo_redo_cross_hold_once(&mut app, &painted);
    }
}
