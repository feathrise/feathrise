//! 剪贴板完整快照、独立副本与整批失败的回归检查；使用正式记录校验。
use super::{types::*, worker};
use bevy::prelude::*;
use feathrise_map_content::{
    MapId, MapPlacementCourtyardMembership, MapPlacementHeightMode, MapPlacementRecord,
};
use std::sync::Arc;

mod interaction;
mod cuboid_worker;
mod terrain_patch_paste;
mod terrain_patch_semantics;
mod automatic_volume;

fn record(id: &str, x: f32) -> MapPlacementRecord {
    MapPlacementRecord {
        mesh_lod: None,
        terrain_binding: None,
        editor: Default::default(),
        terrain_patch: None, // 剪贴板普通资产夹具不携带体积地形。
        id: id.into(),
        asset_path: "gltf/test.glb".into(),
        asset_target: default(),
        building_preset_id: None,
        courtyard: None,
        translation: [x, 3.0, 0.0],
        height_mode: MapPlacementHeightMode::FixedHeight,
        terrain_offset: 0.0,
        yaw_degrees: 37.0,
        scale: [1.1, 1.2, 1.3],
        water_surfaces: Vec::new(),
    }
}
fn map() -> MapId {
    MapId::parse("MAP-clipboard-test").unwrap()
}
fn bounds() -> Rect {
    Rect::from_corners(Vec2::splat(-20.0), Vec2::splat(20.0))
}

#[test]
fn copies_have_independent_record_and_courtyard_ids_without_changing_source() {
    let mut first = record("source-1", -1.0);
    first.courtyard = Some(MapPlacementCourtyardMembership {
        instance_id: "source-group".into(),
        preset_id: "template".into(),
        member_id: "member-1".into(),
        origin: [0.0; 3],
        yaw_degrees: 0.0,
    });
    let mut second = first.clone();
    second.id = "source-2".into();
    second.translation[0] = 1.0;
    second.courtyard.as_mut().unwrap().member_id = "member-2".into();
    let records = vec![first, second];
    let clip = ClipboardData {
        records: records.clone(),
        patch_frames: Arc::default(),
        origin: Vec3::new(0.0, 3.0, 0.0),
        source_map: map(),
    };
    let a = worker::ghost_records(&clip, "paste-a").unwrap();
    let b = worker::ghost_records(&clip, "paste-b").unwrap();
    assert_eq!(clip.records, records);
    for i in 0..2 {
        assert_ne!(a[i].id, records[i].id);
        assert_ne!(a[i].id, b[i].id);
        assert_eq!(a[i].translation, records[i].translation);
        assert_eq!(a[i].scale, records[i].scale);
        assert_eq!(a[i].yaw_degrees, records[i].yaw_degrees);
        assert_eq!(a[i].water_surfaces, records[i].water_surfaces);
    }
    assert_eq!(
        a[0].courtyard.as_ref().unwrap().instance_id,
        a[1].courtyard.as_ref().unwrap().instance_id
    );
    assert_ne!(
        a[0].courtyard.as_ref().unwrap().instance_id,
        b[0].courtyard.as_ref().unwrap().instance_id
    );
}

#[test]
fn paste_prepares_complete_translations_and_one_history_for_target_map() {
    let source = Arc::new(vec![record("copy-1", -1.0), record("copy-2", 1.0)]);
    let work = PasteWork {
        source: source.clone(),
        delta: Vec3::new(4.0, -3.0, 5.0),
        records: Vec::new(),
        validator: None,
        presets: Arc::new(vec![None, None]),
        sampler: None,
        work_plane: false,
        patch: None,
        query: None,
        bounds: bounds(),
        map: map(),
        building_revision: 0,
    };
    let BatchOutput::Paste(Ok(prepared)) = worker::run_paste(work, 7) else {
        panic!("合法整组应完成");
    };
    assert_eq!(prepared.apply[0].translation, [3.0, 0.0, 5.0]);
    assert_eq!(prepared.apply[1].translation, [5.0, 0.0, 5.0]);
    assert_eq!(prepared.history.added, prepared.apply);
    assert_eq!(prepared.members[0].index, 7);
    assert_eq!(prepared.members[1].index, 8);
    assert_eq!(source[0].translation, [-1.0, 3.0, 0.0]);
}

#[test]
fn invalid_member_rejects_paste_as_a_whole() {
    let mut invalid = record("copy-2", 1.0);
    invalid.scale[1] = f32::NAN;
    assert!(
        worker::prepare_paste_records(vec![record("copy-1", -1.0), invalid], 0, &map(), bounds())
            .is_err()
    );
    assert!(
        worker::prepare_paste_records(
            vec![record("same", -1.0), record("same", 1.0)],
            0,
            &map(),
            bounds()
        )
        .is_err()
    );
}

#[test]
fn transformed_object_paste_snaps_world_surface_and_rebinds_target_identity() {
    use crate::authoring::{terrain_ground::TerrainGroundSnapshot, EditorAuthoringSession, PlacementTerrainSampler};
    use feathrise_map_content::{MapTerrainBinding, MapTerrainDescriptor, TerrainId, TerrainObjectTransform};
    use feathrise_scene::ClipmapGroundHit;

    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let target_id = TerrainId::parse("target-island").unwrap();
    let pose = TerrainObjectTransform {
        translation: [5.0, 4.0, 5.0], yaw_degrees: 90.0, scale: [2.0, 3.0, 2.0],
        ..Default::default()
    };
    let collider = avian3d::prelude::Collider::trimesh(
        vec![Vec3::new(-1.0, -0.5, -1.0), Vec3::new(1.0, 0.5, -1.0),
            Vec3::new(1.0, 0.5, 1.0), Vec3::new(-1.0, -0.5, 1.0)],
        vec![[0, 2, 1], [0, 3, 2]],
    ); // 真正斜坡与非单位TRS揭示世界射线误查源坐标、吸附后只保留旧Y的错误。
    let ground = TerrainGroundSnapshot::test_mesh(draft.map_id.clone(), target_id.clone(), pose, collider.clone());
    let mut objects = draft.manifest.terrain_objects_for_edit().to_vec();
    objects[0].id = target_id.clone();
    objects[0].transform = pose;
    draft.manifest.terrains = Some(objects.into());
    draft.manifest.terrain = MapTerrainDescriptor::empty();
    draft.terrain_mesh = None;
    draft.terrain_editing_index = None;
    draft.heights = None; // 目标只有独立对象，不能依赖旧容器网格或0×0采样缓存。
    draft.terrain_ground = Some(ground.clone());
    let matrix = pose.as_transform().to_matrix();
    let original = matrix.transform_point3(Vec3::new(0.37, 0.185, 0.18));
    let snap = feathrise_editor_core::EditorSnapSettings { enabled: true, translation_meters: 0.5, ..default() }
        .translation_snap(false, [0.0; 3]);
    let snapped = Vec3::from_array(snap.horizontal(original.to_array()).unwrap());
    let local = matrix.inverse().transform_point3(snapped);
    let expected = matrix.transform_point3(Vec3::new(local.x, local.x * 0.5, local.z));
    let hit = super::paste::object_hit(&ground, &Default::default(), original + Vec3::Y * 10.0, Vec3::NEG_Y, snap);
    let ClipmapGroundHit::Hit(point) = hit else { panic!("应命中真实对象斜坡：{hit:?}"); };
    assert!(point.abs_diff_eq(expected, 0.002));
    assert!((point.y - original.y).abs() > 0.05); // 吸附后必须重新核验真实Y。
    let hidden = std::collections::BTreeSet::from([target_id.clone()]);
    assert_eq!(super::paste::object_hit(&ground, &hidden, original + Vec3::Y * 10.0, Vec3::NEG_Y, snap), ClipmapGroundHit::Miss);
    assert_eq!(super::paste::object_hit(&TerrainGroundSnapshot::empty(draft.map_id.clone(), 0), &Default::default(), original + Vec3::Y * 10.0, Vec3::NEG_Y, snap), ClipmapGroundHit::Miss);

    let mut source = record("source-record", 1.0);
    source.height_mode = MapPlacementHeightMode::FollowTerrain;
    source.terrain_binding = Some(MapTerrainBinding::capture(TerrainId::parse("source-terrain").unwrap(),
        TerrainObjectTransform::default(), TerrainObjectTransform {
            translation: source.translation, yaw_degrees: source.yaw_degrees, scale: source.scale,
            ..Default::default()
        }).unwrap());
    let clipboard = ClipboardData { records: vec![source.clone()], patch_frames: Arc::default(), origin: Vec3::from_array(source.translation), source_map: map() };
    let ghosts = worker::ghost_records(&clipboard, "new-copy").unwrap();
    let sampler = PlacementTerrainSampler::from_sources(&draft, None, 0).unwrap();
    let BatchOutput::Paste(Ok(prepared)) = worker::run_paste(PasteWork {
        source: ghosts, delta: point - clipboard.origin, records: Vec::new(), validator: None,
        presets: Arc::new(vec![None]), sampler: Some(sampler), work_plane: false, patch: None, query: None,
        bounds: draft.manifest.world_bounds().unwrap(), map: draft.map_id.clone(), building_revision: 0,
    }, 0) else { panic!("新目标地形须完整采用，不能沿用源TerrainId或丢失对象采样器"); };
    let placed = &prepared.apply[0];
    let binding = placed.terrain_binding.as_ref().expect("副本重新绑定真实目标岛");
    assert_eq!(binding.terrain_id, target_id);
    assert!(Vec3::from_array(placed.translation).abs_diff_eq(point, 0.002));
    assert!(Vec3::from_array(ground.bindings.resolve(binding).unwrap().translation).abs_diff_eq(point, 0.002));
    assert_eq!(clipboard.records, vec![source]); // 目标采用不能污染源剪贴板及其完整绑定。
    let mut session = EditorAuthoringSession { draft: Some(draft), ..default() };
    let frozen = super::systems::object_content(&session).unwrap();
    session.draft.as_mut().unwrap().terrain_ground = Some(TerrainGroundSnapshot::test_mesh(
        session.draft.as_ref().unwrap().map_id.clone(), target_id, pose, collider));
    assert_ne!(super::systems::object_content(&session), Some(frozen)); // 同修订重建的快照也拒收旧粘贴结果。
}

#[test]
fn object_paste_recomputes_source_follow_offsets_on_the_target_surface() {
    use crate::authoring::{terrain_ground::TerrainGroundSnapshot, PlacementTerrainSampler};
    use feathrise_map_content::{MapTerrainBinding, MapTerrainDescriptor, TerrainId, TerrainObjectTransform};

    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let id = TerrainId::parse("target-island").unwrap();
    let pose = TerrainObjectTransform { translation: [5.0, 10.0, 5.0], ..default() };
    let collider = avian3d::prelude::Collider::trimesh(
        vec![Vec3::new(-1.0, 0.0, -1.0), Vec3::new(1.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, 1.0), Vec3::new(-1.0, 0.0, 1.0)],
        vec![[0, 2, 1], [0, 3, 2]],
    );
    draft.terrain_ground = Some(TerrainGroundSnapshot::test_mesh(draft.map_id.clone(), id.clone(), pose, collider));
    let mut objects = draft.manifest.terrain_objects_for_edit().to_vec();
    objects[0].id = id.clone();
    objects[0].transform = pose;
    draft.manifest.terrains = Some(objects.into());
    draft.manifest.terrain = MapTerrainDescriptor::empty();
    let source: Vec<_> = [30.0, 33.0].into_iter().enumerate().map(|(index, offset)| {
        let mut source = record(&format!("source-{index}"), 5.0 + index as f32 * 0.25);
        source.translation = [source.translation[0], 10.0 + offset, 5.0];
        source.height_mode = MapPlacementHeightMode::FollowTerrain;
        source.terrain_offset = offset;
        source.terrain_binding = Some(MapTerrainBinding::capture(TerrainId::parse("source-island").unwrap(),
            TerrainObjectTransform::default(), TerrainObjectTransform {
                translation: source.translation, yaw_degrees: source.yaw_degrees, scale: source.scale,
                ..Default::default()
            }).unwrap());
        source
    }).collect();
    let clipboard = ClipboardData { records: source.clone(), patch_frames: Arc::default(), origin: Vec3::new(5.125, 40.0, 5.0), source_map: map() };
    let ghosts = worker::ghost_records(&clipboard, "new-copy").unwrap();
    let BatchOutput::Paste(result) = worker::run_paste(PasteWork {
        source: ghosts, delta: Vec3::new(0.0, -30.0, 0.0), records: Vec::new(), validator: None,
        presets: Arc::new(vec![None, None]), sampler: Some(PlacementTerrainSampler::from_sources(&draft, None, 0).unwrap()), work_plane: false, patch: None,
        query: None, bounds: draft.manifest.world_bounds().unwrap(), map: draft.map_id.clone(), building_revision: 0,
    }, 0) else { panic!("真实目标地面必须完成完整粘贴"); };
    let pasted = result.expect("旧离地偏移不能使查询从目标地面下方开始而误拒合法粘贴");
    for (index, placed) in pasted.apply.iter().enumerate() {
        assert_eq!(placed.translation[1], 10.0 + index as f32 * 3.0);
        assert_eq!(placed.terrain_offset, index as f32 * 3.0);
        let binding = placed.terrain_binding.as_ref().unwrap();
        assert_eq!(binding.terrain_id, id);
        assert_eq!(binding.local_height_offset, Some(index as f32 * 3.0));
    }
    assert_eq!(clipboard.records, source); // 目标共同位移与偏移重算不能污染源剪贴板。
}
