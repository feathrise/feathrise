//! 水位中心只接受当前画布和完整预览；来源门不得借同名兄弟实体，也不因数值拖动逐帧扫描。

use super::*;
use super::source::applied_instance_scope;
use feathrise_map_content::{MapInstancePath, TerrainId};
use feathrise_scene::{PaintedWaterPurpose, SubMapInstance};
use feathrise_scene_core::residency::{MapInstanceOwner, MapSourceFrame};
use std::sync::Arc;

fn session() -> EditorAuthoringSession {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut water = crate::authoring::water_asset_with_layer_for_tests(&draft.map_id, &draft.folder);
    water.layers[0].resolution = [2, 2];
    draft.water = Some(water);
    draft.water_masks.insert("lake".into(), Arc::new(vec![255; 4]));
    EditorAuthoringSession { draft: Some(draft), water_revision: 7, terrain_revision: 11, ..default() }
}

fn activate_canvas(session: &mut EditorAuthoringSession, id: &str) {
    let draft = session.draft.as_mut().unwrap();
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.id = TerrainId::parse(id).unwrap();
    object.water_path = Some(draft.water_asset_path.clone());
    let manifest = object.source_manifest(&draft.manifest);
    draft.manifest.terrains = Some(Arc::from([object.clone()]));
    draft.terrain_documents.active = Some(crate::authoring::terrain_documents::TerrainDocumentIdentity {
        definition: Arc::new(object), manifest, folder: draft.folder.clone(),
    });
}

#[test]
fn still_water_center_requires_current_source_and_revisions() {
    let mut session = session();
    let mut selection = SelectedWaterViewportState::default();
    let center = Vec3::new(3.0, 1.0, 4.0);
    selection.set_still_water_center_for_tests(&session, "lake", center);
    assert_eq!(selection.still_water_center(&session, "lake"), Some(center));
    assert_eq!(selection.still_water_center(&session, "other"), None);
    session.water_revision += 1;
    assert_eq!(selection.still_water_center(&session, "lake"), None);
    session.water_revision -= 1;
    session.terrain_revision += 1;
    assert_eq!(selection.still_water_center(&session, "lake"), None);
}

#[test]
fn sibling_canvas_with_same_map_layer_and_r8_cannot_reuse_center() {
    let mut session = session();
    activate_canvas(&mut session, "canvas-a");
    let mut selection = SelectedWaterViewportState::default();
    selection.set_still_water_center_for_tests(&session, "lake", Vec3::ONE);
    assert_eq!(selection.still_water_center(&session, "lake"), Some(Vec3::ONE));
    // 故意共享文件路径、R8和数值修订，只改变画布；同MapId和同层名仍不能授权旧中心。
    activate_canvas(&mut session, "canvas-b");
    assert_eq!(selection.still_water_center(&session, "lake"), None);
}

#[test]
fn replaced_r8_path_hash_or_loading_invalidates_water_center() {
    let mut session = session();
    let mut selection = SelectedWaterViewportState::default();
    selection.set_still_water_center_for_tests(&session, "lake", Vec3::ONE);
    session.draft.as_mut().unwrap().water_masks.insert("lake".into(), Arc::new(vec![255; 4]));
    assert_eq!(selection.still_water_center(&session, "lake"), None); // 相同像素的新R8也属于新来源。
    selection.set_still_water_center_for_tests(&session, "lake", Vec3::ONE);
    session.draft.as_mut().unwrap().water_asset_path.push_str(".new");
    assert_eq!(selection.still_water_center(&session, "lake"), None);
    selection.set_still_water_center_for_tests(&session, "lake", Vec3::ONE);
    session.draft.as_mut().unwrap().terrain_source_sha256 = Some([9; 32]);
    assert_eq!(selection.still_water_center(&session, "lake"), None);
    selection.set_still_water_center_for_tests(&session, "lake", Vec3::ONE);
    session.draft.as_mut().unwrap().terrain_documents.loading = true;
    assert_eq!(selection.still_water_center(&session, "lake"), None);
}

#[test]
fn water_lookup_signature_ignores_pending_numeric_revisions() {
    let mut session = session();
    let identity = SelectedWaterIdentity {
        map_id: session.draft.as_ref().unwrap().map_id.clone(),
        kind: WaterAuthoringFeatureKind::StillWater, feature_id: "lake".into(),
    };
    let mut preview = WaterPreviewState::default();
    preview.applied_map_id = Some(identity.map_id.clone());
    let before = source_lookup_signature(&identity, &preview, WaterSurfaceScope::Root,
        WaterSourceIdentity::capture(&session, "lake"));
    session.water_revision += 1;
    session.terrain_revision += 1;
    let pending = source_lookup_signature(&identity, &preview, WaterSurfaceScope::Root,
        WaterSourceIdentity::capture(&session, "lake"));
    assert_eq!(before, pending); // 旧完整水面保留，拖动每帧只做定点缓存验证。
    preview.applied_water_revision += 1;
    assert_ne!(before, source_lookup_signature(&identity, &preview, WaterSurfaceScope::Root,
        WaterSourceIdentity::capture(&session, "lake"))); // 真正采用新预览才重查。
    preview.applied_water_revision -= 1;
    session.draft.as_mut().unwrap().water_masks.insert("lake".into(), Arc::new(vec![255; 4]));
    assert_eq!(before, source_lookup_signature(&identity, &preview, WaterSurfaceScope::Root,
        WaterSourceIdentity::capture(&session, "lake"))); // 连续绘制的R8 copy-on-write不能逐帧扫描水面。
    session.draft.as_mut().unwrap().water_asset_path.push_str(".new");
    assert_ne!(before, source_lookup_signature(&identity, &preview, WaterSurfaceScope::Root,
        WaterSourceIdentity::capture(&session, "lake")));
}

#[test]
fn water_scope_rejects_sibling_owner_and_applies_complete_source_transform() {
    let mut world = World::new();
    let owner = world.spawn_empty().id();
    let sibling = world.spawn_empty().id();
    let transform = Transform::from_xyz(13.0, 9.0, -5.0)
        .with_rotation(Quat::from_euler(EulerRot::YXZ, 0.4, 0.2, -0.3))
        .with_scale(Vec3::new(2.0, 3.0, 0.5));
    let scope = WaterSurfaceScope::Instance(owner, transform.to_matrix());
    assert!(scope.accepts(Some(&MapInstanceOwner(owner))));
    assert!(!scope.accepts(Some(&MapInstanceOwner(sibling))));
    assert!(!scope.accepts(None));
    assert!(WaterSurfaceScope::Root.accepts(None));
    assert!(!WaterSurfaceScope::Root.accepts(Some(&MapInstanceOwner(owner))));
    let bounds = Aabb::from_min_max(Vec3::new(1.0, 2.0, 3.0), Vec3::new(5.0, 2.0, 9.0));
    let frame = transformed_water_frame(&Transform::from_matrix(scope.matrix()), &bounds);
    assert!(frame.center.distance(transform.transform_point(Vec3::new(3.0, 2.0, 6.0))) < 0.00001);
}

#[test]
fn applied_water_instance_rejects_changed_source_and_inactive_batch() {
    let mut session = session();
    activate_canvas(&mut session, "canvas-a");
    let draft = session.draft.as_ref().unwrap();
    let current = &draft.terrain_documents.active.as_ref().unwrap().definition;
    let mut world = World::new();
    let entity = world.spawn_empty().id();
    let batch = world.spawn_empty().id();
    let instance = SubMapInstance { path: MapInstancePath::root(draft.map_id.clone()).terrain(current.id.clone()),
        source_map_id: draft.map_id.clone(), generation: 1, batch };
    let frame = MapSourceFrame::from_transform(current.transform.as_transform());
    assert_eq!(applied_instance_scope(&draft.map_id, current, entity, &instance, current, &frame, true),
        WaterSurfaceScope::Instance(entity, frame.source_to_world));
    assert_eq!(applied_instance_scope(&draft.map_id, current, entity, &instance, current, &frame, false),
        WaterSurfaceScope::Unavailable);
    let mut previous = current.as_ref().clone();
    previous.terrain.height_path = Some("maps/old/height.r16".into());
    assert_eq!(applied_instance_scope(&draft.map_id, current, entity, &instance, &previous, &frame, true),
        WaterSurfaceScope::Unavailable);
    previous = current.as_ref().clone();
    previous.id = TerrainId::parse("canvas-b").unwrap();
    assert_eq!(applied_instance_scope(&draft.map_id, current, entity, &instance, &previous, &frame, true),
        WaterSurfaceScope::Unavailable);
}

#[test]
fn pending_water_preview_keeps_highlight_frame_without_authorizing_center() {
    let mut app = App::new();
    let session = session();
    let map = session.draft.as_ref().unwrap().map_id.clone();
    app.insert_resource(MapEditorState { open: true, tool: EditorTool::Water, ..default() })
        .insert_resource(session).init_resource::<WaterPreviewState>()
        .init_resource::<SelectedWaterViewportState>()
        .init_resource::<Assets<DynamicWaterMaterial>>().init_resource::<Assets<ClipmapWaterMaterial>>()
        .add_systems(Update, systems::sync_selected_water_viewport);
    let marker = PaintedWaterSurface { map_id: map.clone(), layer_id: "lake".into(),
        kind: WaterSurfaceKind::Still, purpose: PaintedWaterPurpose::Runtime };
    let bounds = Aabb::from_min_max(Vec3::new(1.0, 1.0, 1.0), Vec3::new(5.0, 1.0, 7.0));
    let owner = app.world_mut().spawn_empty().id();
    app.world_mut().spawn((PaintedWaterSurface { map_id: map.clone(), layer_id: "lake".into(),
        kind: WaterSurfaceKind::Still, purpose: PaintedWaterPurpose::Runtime }, Mesh3d::default(),
        Transform::from_xyz(100.0, 0.0, 100.0), Visibility::Inherited, bounds, MapInstanceOwner(owner)));
    let source = app.world_mut().spawn((marker, Mesh3d::default(), Transform::IDENTITY, Visibility::Inherited, bounds)).id();
    app.update();
    let center = Vec3::new(3.0, 1.0, 4.0);
    let selected = app.world().resource::<SelectedWaterViewportState>();
    assert_eq!(selected.source_entity, Some(source)); // 真实同步选择当前无owner根，拒绝同图同层的子实例。
    assert_eq!(selected.frame.unwrap().center, center);
    assert_eq!(selected.still_water_center(app.world().resource::<EditorAuthoringSession>(), "lake"), None);
    {
        let mut preview = app.world_mut().resource_mut::<WaterPreviewState>();
        preview.applied_map_id = Some(map);
        preview.applied_water_revision = 7;
        preview.applied_terrain_revision = 11;
    }
    app.update();
    assert_eq!(app.world().resource::<SelectedWaterViewportState>().still_water_center(
        app.world().resource::<EditorAuthoringSession>(), "lake"), Some(center));
    app.world_mut().resource_mut::<EditorAuthoringSession>().water_revision += 1;
    app.update();
    let selected = app.world().resource::<SelectedWaterViewportState>();
    assert_eq!(selected.frame.unwrap().center, center); // 水位新预览等待期间继续保留上一完整几何。
    assert_eq!(selected.still_water_center(app.world().resource::<EditorAuthoringSession>(), "lake"), None);
}

#[test]
fn glb_author_preview_requires_ready_and_cannot_cross_canvas() {
    let mut app = App::new();
    let mut session = session();
    activate_canvas(&mut session, "canvas-a");
    let active = session.draft.as_mut().unwrap().terrain_documents.active.as_mut().unwrap();
    Arc::make_mut(&mut active.definition).transform.translation = [10.0, 0.0, 0.0];
    let map = session.draft.as_ref().unwrap().map_id.clone();
    app.insert_resource(MapEditorState { open: true, tool: EditorTool::Water, ..default() })
        .insert_resource(session).init_resource::<WaterPreviewState>()
        .init_resource::<SelectedWaterViewportState>()
        .init_resource::<Assets<DynamicWaterMaterial>>().init_resource::<Assets<ClipmapWaterMaterial>>()
        .add_systems(Update, systems::sync_selected_water_viewport);
    // 真实GLB作者面在父域spawn，不带MapInstanceOwner；marker和已采用联合修订共同证明归属。
    let source = app.world_mut().spawn((crate::components::EditorWaterPreview,
        PaintedWaterSurface { map_id: map.clone(), layer_id: "lake".into(),
            kind: WaterSurfaceKind::Still, purpose: PaintedWaterPurpose::EditorPreview },
        Mesh3d::default(), Transform::IDENTITY, Visibility::Inherited,
        Aabb::from_min_max(Vec3::new(1.0, 1.0, 1.0), Vec3::new(5.0, 1.0, 7.0)))).id();
    app.update();
    assert!(app.world().resource::<SelectedWaterViewportState>().frame.is_none());
    {
        let mut preview = app.world_mut().resource_mut::<WaterPreviewState>();
        preview.applied_map_id = Some(map);
        preview.applied_water_revision = 7;
        preview.applied_terrain_revision = 11;
    }
    app.update();
    let selected = app.world().resource::<SelectedWaterViewportState>();
    assert_eq!(selected.source_entity, Some(source));
    let center = Vec3::new(13.0, 1.0, 4.0);
    assert_eq!(selected.still_water_center(app.world().resource::<EditorAuthoringSession>(), "lake"), Some(center));
    app.world_mut().resource_mut::<EditorAuthoringSession>().water_revision += 1;
    app.update();
    let selected = app.world().resource::<SelectedWaterViewportState>();
    assert_eq!(selected.frame.unwrap().center, center);
    assert_eq!(selected.still_water_center(app.world().resource::<EditorAuthoringSession>(), "lake"), None);
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        activate_canvas(&mut session, "canvas-b");
        session.water_revision += 1; // 与真实画布采用/切换路径一致，旧Ready不能授权新画布同名层。
    }
    app.update();
    let selected = app.world().resource::<SelectedWaterViewportState>();
    assert!(selected.source_entity.is_none() && selected.frame.is_none());
    assert_eq!(selected.still_water_center(app.world().resource::<EditorAuthoringSession>(), "lake"), None);
}
