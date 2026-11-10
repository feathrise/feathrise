//! 静态三维地形复制的源局部坐标、目标归属和容量边界；自动雕刻源不走普通剪贴板。

use super::*;
use feathrise_scene_content::{
    ScenePlacementTerrainPatch, SceneTerrainCutout, SceneTerrainDescriptor, SceneTerrainKind,
    SceneTerrainObject, SparseHeightSpec, TerrainId, TerrainObjectTransform,
};

fn target_scene() -> SceneId { SceneId::parse("MAP-patch-semantics-target").unwrap() }

fn object(map: &SceneId, id: &str, min: f32, transform: TerrainObjectTransform) -> SceneTerrainObject {
    let spec = SparseHeightSpec::new([min; 2], [min + 64.0; 2], 1.0, -8.0, 16.0).unwrap();
    SceneTerrainObject {
        clipmap_stitches: default(), mesh_lod: None,
        id: TerrainId::parse(id).unwrap(), display_name: "复制归属夹具".into(), enabled: true,
        transform, world_min: [min; 2], world_max: [min + 64.0; 2],
        terrain: SceneTerrainDescriptor {
            kind: SceneTerrainKind::SparseClipmap,
            height_path: Some(format!("scenes/{map}/terrains/{id}/height_tiles.scene_height_manifest.json")),
            resolution: [65; 2], base_height: spec.base_height, height_scale: spec.height_scale,
            collision_lods: feathrise_scene_content::default_collision_lods_for_resolution([65; 2]),
            display_lods: Some(feathrise_scene_content::default_display_lods_for_spec(&spec).unwrap()),
            ..SceneTerrainDescriptor::empty()
        },
        voxel: None, surface_path: None, vegetation_path: None, water_path: None, puddles: None,
    }
}

fn static_patch(source: &SceneTerrainObject, cutout: bool) -> ScenePlacementRecord {
    let mut item = record("static-source", source.world_min[0] + 12.0);
    item.translation[2] = source.world_min[1] + 20.0;
    item.yaw_degrees = 90.0;
    item.terrain_patch = Some(ScenePlacementTerrainPatch {
        terrain_id: Some(source.id.clone()),
        cutout: cutout.then_some(SceneTerrainCutout { min: [-1.0; 2], max: [1.0; 2] }),
        ..default()
    });
    item.validated_transform(&map(), Rect::from_corners(source.world_min.into(), source.world_max.into())).unwrap();
    item
}

fn capture(records: &[ScenePlacementRecord], objects: &[SceneTerrainObject]) -> Result<Arc<ClipboardData>, String> {
    let members = records.iter().enumerate().map(|(index, item)| {
        crate::viewport::multi_select::SelectedMember {
            id: item.id.clone(), index, position: Vec3::from_array(item.translation),
        }
    }).collect();
    let output = worker::capture(Capture {
        identity: BatchIdentity { scene_id: map(), author: 0, structure: 0, terrain: 0, content: None, placement_derived_pose_revision: 0,
            document: crate::authoring::placement_deletion::PlacementDeletionIntents::default().identity() },
        action: PlacementBatchAction::Copy,
        records: records.iter().cloned().enumerate().collect(),
        members: Arc::new(members), source_objects: objects.to_vec().into(),
        // 故意与独立来源范围不同，捕获不能再用场景容器范围裁掉合法的源局部洞口。
        bounds: bounds(), length: records.len(), selection_revision: 0, selected_id: None,
    });
    match output {
        BatchOutput::Captured(result, false) => result,
        _ => panic!("普通复制必须返回完整剪贴板结果"),
    }
}

fn paste(clipboard: &Arc<ClipboardData>, target: SceneTerrainObject, legacy: bool,
    delta: Vec3, existing: PatchBudget) -> Result<PreparedPaste, String> {
    let ghosts = worker::ghost_records(clipboard, "patch-semantics-copy")?;
    match worker::run_paste(PasteWork {
        source: ghosts, delta, records: Vec::new(), validator: None,
        presets: Arc::new(vec![None; clipboard.records.len()]), sampler: None, work_plane: false,
        patch: Some(PatchPasteContext {
            frames: clipboard.patch_frames.clone(), target: PatchTarget { object: Arc::new(target), legacy }, existing,
        }),
        query: None, bounds: bounds(), scene_id: target_scene(), building_revision: 0,
    }, 0) {
        BatchOutput::Paste(result) => result,
        _ => panic!("固定高度静态patch使用冻结的完整对象姿态，不应进入建筑或高度页等待"),
    }
}

fn matrix(item: &ScenePlacementRecord) -> Mat4 {
    TerrainObjectTransform {
        translation: item.translation, yaw_degrees: item.yaw_degrees, scale: item.scale, ..default()
    }.as_transform().to_matrix()
}

#[test]
fn static_patch_capture_and_paste_preserve_world_geometry_between_different_local_bounds_and_trs() {
    let source = object(&map(), "source-transformed", 100.0, TerrainObjectTransform {
        translation: [80.0, 7.0, -40.0], yaw_degrees: 90.0, scale: [2.0, 3.0, 4.0], ..default()
    });
    let target = object(&target_scene(), "target-transformed", 1000.0, TerrainObjectTransform {
        translation: [-50.0, 12.0, 25.0], yaw_degrees: -90.0, scale: [0.5, 2.0, 3.0], ..default()
    });
    let before = static_patch(&source, true);
    let clipboard = capture(&[before.clone()], &[source.clone()]).expect("合法源局部洞口不受容器范围限制");
    let source_frame = source.transform.as_transform().to_matrix();
    let source_world_origin = source_frame.transform_point3(Vec3::from_array(before.translation));
    assert!(clipboard.origin.abs_diff_eq(source_world_origin, 0.002));
    assert_eq!(clipboard.patch_frames.as_ref(), &vec![Some(source.transform)]);
    let target_frame = target.transform.as_transform().to_matrix();
    let clicked = target_frame.transform_point3(Vec3::new(1012.0, 2.0, 1020.0));
    let delta = clicked - clipboard.origin;
    let prepared = paste(&clipboard, target.clone(), false, delta, default())
        .expect("可表示的非均匀缩放和90度朝向必须完成完整局部重绑");
    let after = &prepared.apply[0];
    assert!(after.terrain_binding.is_none());
    assert_eq!(after.terrain_patch.as_ref().unwrap().terrain_id.as_ref(), Some(&target.id));
    after.validated_transform(&target_scene(), Rect::from_corners(target.world_min.into(), target.world_max.into())).unwrap();
    let expected_world = Mat4::from_translation(delta) * source_frame * matrix(&before);
    let actual_world = target_frame * matrix(after);
    // 比较完整局部盒八角，能同时发现中心位置正确但旋转、轴交换或三轴比例丢失。
    for x in [-1.0, 1.0] { for y in [-1.0, 1.0] { for z in [-1.0, 1.0] {
        let point = Vec3::new(x, y, z);
        assert!(actual_world.transform_point3(point).abs_diff_eq(expected_world.transform_point3(point), 0.004));
    } } }
    let reopened: ScenePlacementRecord = serde_json::from_slice(&serde_json::to_vec(after).unwrap()).unwrap();
    assert_eq!(&reopened, after);
    assert_eq!(prepared.history.added, prepared.apply);
    assert_eq!(clipboard.records, vec![before]);
}

#[test]
fn static_patch_paste_to_legacy_clipmap_clears_source_object_identity() {
    let source = object(&map(), "old-source-id", -32.0, TerrainObjectTransform {
        translation: [3.0, 2.0, -4.0], yaw_degrees: 90.0, scale: [2.0, 3.0, 4.0], ..default()
    });
    let mut target = object(&target_scene(), TerrainId::legacy().as_str(), -32.0, default());
    target.terrain.height_path = Some(format!("scenes/{}/terrain/height_tiles.scene_height_manifest.json", target_scene()));
    let before = static_patch(&source, false);
    let clipboard = capture(&[before.clone()], &[source.clone()]).unwrap();
    // 选择目标内部世界点，源局部范围与世界位置不影响旧主地形的独立归属。
    let delta = Vec3::new(5.0, 2.0, 5.0) - clipboard.origin;
    let prepared = paste(&clipboard, target, true, delta, default()).unwrap();
    let after = &prepared.apply[0];
    assert!(after.terrain_binding.is_none());
    assert!(after.terrain_patch.as_ref().unwrap().terrain_id.is_none(), "旧主地形保留兼容缺省身份，不能残留其他场景ID");
    assert!(Vec3::from_array(after.translation).abs_diff_eq(Vec3::new(5.0, 2.0, 5.0), 0.002));
    assert_eq!(clipboard.records, vec![before]);
}

#[test]
fn static_patch_and_ordinary_mesh_share_world_delta_without_reframing_the_ordinary_record() {
    let source = object(&map(), "source-mixed", -32.0, TerrainObjectTransform {
        translation: [10.0, 4.0, 5.0], yaw_degrees: 90.0, scale: [2.0, 3.0, 4.0], ..default()
    });
    let target = object(&target_scene(), "target-mixed", -32.0, TerrainObjectTransform {
        translation: [-40.0, 6.0, 20.0], yaw_degrees: 90.0, scale: [2.0; 3], ..default()
    });
    let patch = static_patch(&source, false);
    let source_frame = source.transform.as_transform().to_matrix();
    let patch_world_position = source_frame.transform_point3(Vec3::from_array(patch.translation));
    let mut ordinary = record("ordinary-world-mesh", 0.0);
    ordinary.translation = (patch_world_position + Vec3::new(5.0, 2.0, -3.0)).to_array();
    let records = vec![patch.clone(), ordinary.clone()];
    let clipboard = capture(&records, &[source.clone()]).unwrap();
    assert_eq!(clipboard.patch_frames.as_ref(), &vec![Some(source.transform), None]);
    assert!(clipboard.origin.abs_diff_eq(patch_world_position + Vec3::new(2.5, 0.0, -1.5), 0.002));
    let delta = Vec3::new(3.0, 1.0, -4.0);
    let prepared = paste(&clipboard, target.clone(), false, delta, default()).unwrap();
    assert!(Vec3::from_array(prepared.apply[1].translation)
        .abs_diff_eq(Vec3::from_array(ordinary.translation) + delta, 0.002));
    assert_eq!(prepared.apply[1].yaw_degrees, ordinary.yaw_degrees);
    assert_eq!(prepared.apply[1].scale, ordinary.scale);
    assert!(prepared.apply[1].terrain_patch.is_none());
    let after_patch_world = target.transform.as_transform().to_matrix() * matrix(&prepared.apply[0]);
    let expected_patch_world = Mat4::from_translation(delta) * source_frame * matrix(&patch);
    for point in [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z] {
        assert!(after_patch_world.transform_point3(point).abs_diff_eq(expected_patch_world.transform_point3(point), 0.002));
    }
    assert_eq!(clipboard.records, records);
}

#[test]
fn static_patch_mixed_with_building_keeps_source_bounds_and_original_order_during_validation() {
    use crate::authoring::buildings::{
        BuildingAuthoringState, courtyard_validator_snapshot,
        move_test_support::install_existing_edit_validator,
    };
    use crate::authoring::{EditorAuthoringSession, PlacementTerrainSampler, terrain_ground::TerrainGroundSnapshot};
    use feathrise_scene_content::{BuildingPresetDescriptor, BuildingPresetRegistry};

    let frame = TerrainObjectTransform { translation: [-1000.0, 0.0, -1000.0], ..default() };
    let source = object(&map(), "source-with-building", 1000.0, frame);
    let target = object(&target_scene(), "target-with-building", 1000.0, frame);
    let patch = static_patch(&source, true);
    let mut second_patch = patch.clone();
    second_patch.id = "second-static-source".into();
    second_patch.translation[0] += 20.0;
    let mut building = record("building-between-patches", 5.0);
    building.translation = [5.0, 9.0, 5.0];
    building.yaw_degrees = 0.0;
    building.scale = [1.0; 3];
    let preset = BuildingPresetDescriptor::from_asset(
        "patch-neighbor-house".into(), "混合粘贴建筑".into(), building.asset_path.clone(), building.asset_target,
    );
    building.building_preset_id = Some(preset.id.clone());
    let records = vec![patch.clone(), building, second_patch.clone()];
    let clipboard = capture(&records, &[source]).unwrap();
    let ghosts = worker::ghost_records(&clipboard, "mixed-building-copy").unwrap();

    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![2; 121]));
    draft.scene_id = target_scene();
    draft.manifest.id = draft.scene_id.clone();
    draft.placements.scene_id = draft.scene_id.clone();
    draft.manifest.world_min = bounds().min.to_array();
    draft.manifest.world_max = bounds().max.to_array();
    draft.manifest.terrains = Some(Arc::from([target.clone()]));
    draft.manifest.terrain = SceneTerrainDescriptor::empty();
    // 真实目标三角面供建筑五点和普通绑定共同查询；补丁洞口仍位于1000起的目标源局部范围。
    let collider = avian3d::prelude::Collider::trimesh(
        vec![Vec3::new(1000.0, 2.0, 1000.0), Vec3::new(1064.0, 2.0, 1000.0),
            Vec3::new(1064.0, 2.0, 1064.0), Vec3::new(1000.0, 2.0, 1064.0)],
        vec![[0, 2, 1], [0, 3, 2]],
    );
    draft.terrain_ground = Some(TerrainGroundSnapshot::test_meshes(draft.scene_id.clone(), &[target.clone()], collider));
    let sampler = PlacementTerrainSampler::from_sources(&draft, None, 0).unwrap();
    let mut app = App::new();
    app.insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() });
    install_existing_edit_validator(&mut app, preset.clone());
    let validator = courtyard_validator_snapshot(
        app.world().resource::<EditorAuthoringSession>(),
        app.world().resource::<BuildingPresetRegistry>(),
        app.world().resource::<BuildingAuthoringState>(), None,
    ).unwrap();
    let expected_ids = ghosts.iter().map(|record| record.id.clone()).collect::<Vec<_>>();
    let BatchOutput::Paste(result) = worker::run_paste(PasteWork {
        source: ghosts, delta: Vec3::ZERO, records: Vec::new(), validator: Some(validator),
        presets: Arc::new(vec![None, Some(preset), None]), sampler: Some(sampler), work_plane: false,
        patch: Some(PatchPasteContext {
            frames: clipboard.patch_frames.clone(), target: PatchTarget { object: Arc::new(target.clone()), legacy: false },
            existing: default(),
        }), query: None, bounds: bounds(), scene_id: target_scene(), building_revision: 1,
    }, 0) else { panic!("真实对象承托与已采用建筑索引应直接完成混合粘贴"); };
    let prepared = result.expect("建筑校验不能用容器范围拒绝已通过独立源范围校验的三维地形洞口");
    assert_eq!(prepared.apply.iter().map(|record| record.id.clone()).collect::<Vec<_>>(), expected_ids);
    for (index, before) in [(0, patch), (2, second_patch)] {
        let after = &prepared.apply[index];
        assert_eq!(after.translation, before.translation);
        assert_eq!(after.terrain_patch.as_ref().unwrap().terrain_id.as_ref(), Some(&target.id));
        assert!(after.terrain_binding.is_none());
        after.validated_transform(&target_scene(), Rect::from_corners(target.world_min.into(), target.world_max.into())).unwrap();
        assert!(after.validated_transform(&target_scene(), bounds()).is_err(), "用容器范围重复校验会稳定重现原缺陷");
    }
    let placed_building = &prepared.apply[1];
    assert_eq!(placed_building.translation, [5.0, 2.0, 5.0], "建筑仍必须通过五点地基校验，不能随补丁一起跳过validator");
    assert_eq!(placed_building.height_mode, ScenePlacementHeightMode::FixedHeight);
    assert_eq!(placed_building.terrain_binding.as_ref().unwrap().terrain_id, target.id);
    assert_eq!(prepared.history.added, prepared.apply);
    assert_eq!(clipboard.records, records);
}

#[test]
fn static_patch_paste_checks_root_and_hole_budgets_without_counting_empty_cutouts() {
    let source = object(&map(), "source-budget", -32.0, default());
    let target = object(&target_scene(), "target-budget", -32.0, default());
    for (hole, existing, accepted) in [
        (false, PatchBudget { roots: 31, holes: 32 }, true),
        (true, PatchBudget { roots: 31, holes: 31 }, true),
        (false, PatchBudget { roots: 32, holes: 0 }, false),
        (true, PatchBudget { roots: 0, holes: 32 }, false),
    ] {
        let before = static_patch(&source, hole);
        let clipboard = capture(&[before.clone()], &[source.clone()]).unwrap();
        let result = paste(&clipboard, target.clone(), false, Vec3::ZERO, existing);
        assert_eq!(result.is_ok(), accepted, "三维根和真实洞口分别受32项预算约束：hole={hole}, roots={}, holes={}", existing.roots, existing.holes);
        assert_eq!(clipboard.records, vec![before]);
    }
}

#[test]
fn static_patch_capture_rejects_automatic_sculpted_volume_even_when_record_is_valid() {
    let source = object(&map(), "source-sculpted", -32.0, default());
    let mut before = static_patch(&source, true);
    before.scale = [1.0; 3];
    before.yaw_degrees = 0.0;
    before.terrain_patch.as_mut().unwrap().sculpted_volume = true;
    before.validated_transform(&map(), Rect::from_corners(source.world_min.into(), source.world_max.into())).unwrap();
    assert!(capture(&[before], &[source]).is_err(), "自动密度作者包不能伪装成可独立复制的普通静态GLB");
}

#[test]
fn static_patch_paste_rejects_non_clipmap_destination() {
    let source = object(&map(), "source-type", -32.0, default());
    let before = static_patch(&source, false);
    let clipboard = capture(&[before], &[source]).unwrap();
    let mut target = object(&target_scene(), "target-type", -32.0, default());
    target.terrain.kind = SceneTerrainKind::Heightfield;
    target.terrain.glb_path = "gltf/test.glb".into();
    target.terrain.height_path = None;
    assert!(paste(&clipboard, target, false, Vec3::ZERO, default()).is_err());
}

#[test]
fn static_patch_paste_rejects_relative_pitch_that_the_placement_record_cannot_store() {
    let source = object(&map(), "source-pitch", -32.0, default());
    let before = static_patch(&source, false);
    let clipboard = capture(&[before], &[source]).unwrap();
    let mut target = object(&target_scene(), "target-pitch", -32.0, default());
    assert!(paste(&clipboard, target.clone(), false, Vec3::ZERO, default()).is_ok());
    target.transform.pitch_degrees = 15.0;
    assert!(paste(&clipboard, target, false, Vec3::ZERO, default()).is_err(), "不能把无法保存的相对X旋转静默展平成Y朝向");
}

#[test]
fn static_patch_paste_rejects_shear_instead_of_approximating_world_geometry() {
    let source = object(&map(), "source-shear", -32.0, TerrainObjectTransform { scale: [2.0, 3.0, 4.0], ..default() });
    let mut before = static_patch(&source, false);
    before.yaw_degrees = 45.0;
    let clipboard = capture(&[before], &[source]).unwrap();
    let target = object(&target_scene(), "target-shear", -32.0, default());
    assert!(paste(&clipboard, target, false, Vec3::ZERO, default()).is_err(), "父级非均匀缩放乘任意子旋转产生剪切，不能近似分解后发布");
}
