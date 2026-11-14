//! 普通静态GLB三维地形跨场景粘贴回归；专属地形归属不能与普通内容绑定混用。

use super::*;
use crate::authoring::{PlacementTerrainSampler, terrain_ground::TerrainGroundSnapshot};
use feathrise_scene_content::{
    ScenePlacementTerrainPatch, SceneTerrainCutout, SceneTerrainDescriptor, SceneTerrainKind,
    SparseHeightSpec, TerrainId,
};
use feathrise_scene::ClipmapGroundHit;

fn paste_legacy_static_patch(cutout: Option<SceneTerrainCutout>) {
    let mut source_draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let spec = SparseHeightSpec::new([-32.0; 2], [32.0; 2], 1.0, -8.0, 16.0).unwrap();
    source_draft.manifest.world_min = [-32.0; 2];
    source_draft.manifest.world_max = [32.0; 2];
    source_draft.manifest.terrain = SceneTerrainDescriptor {
        kind: SceneTerrainKind::SparseClipmap,
        height_path: Some(format!("{}/terrain/height_tiles.scene_height_manifest.json", source_draft.folder)),
        resolution: [65; 2],
        base_height: spec.base_height,
        height_scale: spec.height_scale,
        collision_lods: feathrise_scene_content::default_collision_lods_for_resolution([65; 2]),
        display_lods: Some(feathrise_scene_content::default_display_lods_for_spec(&spec).unwrap()),
        ..SceneTerrainDescriptor::empty()
    };
    source_draft.terrain_mesh = None;
    source_draft.terrain_editing_index = None;
    source_draft.heights = None;
    assert!(source_draft.manifest.terrains.is_none()); // 旧主Clipmap通过缺省patch TerrainId关联，不能伪造普通terrain_binding。

    let mut source = record("source-static-terrain", 1.0);
    source.yaw_degrees = 90.0;
    source.terrain_patch = Some(ScenePlacementTerrainPatch { cutout, ..default() });
    source.validated_transform(&source_draft.scene_id, source_draft.manifest.world_bounds().unwrap()).unwrap();
    source_draft.placements.placements.push(source.clone());
    let BatchOutput::Captured(Ok(clipboard), false) = worker::capture(Capture { model_vegetation: Arc::default(),
        identity: BatchIdentity {
            document: crate::authoring::placement_deletion::PlacementDeletionIntents::default().identity(),
            scene_id: source_draft.scene_id.clone(), author: 0, structure: 0, terrain: 0, content: None,
            placement_derived_pose_revision: 0,
        },
        action: PlacementBatchAction::Copy,
        members: Arc::new(vec![crate::viewport::multi_select::SelectedMember {
            id: source.id.clone(), index: 0, position: Vec3::from_array(source.translation),
        }]),
        records: vec![(0, source.clone())],
        bounds: source_draft.manifest.world_bounds().unwrap(),
        source_objects: source_draft.manifest.terrain_objects_for_edit(),
        length: 1, selection_revision: 0, selected_id: Some(source.id.clone()),
    }) else { panic!("合法普通静态三维地形必须能复制；自动sculpted_volume不属于本夹具"); };

    let mut target = source_draft.clone();
    target.scene_id = SceneId::parse("MAP-static-patch-target").unwrap();
    target.manifest.id = target.scene_id.clone();
    target.placements.scene_id = target.scene_id.clone();
    target.placements.placements.clear();
    let target_id = TerrainId::parse("target-clipmap").unwrap();
    let mut objects = target.manifest.terrain_objects_for_edit().to_vec();
    objects[0].id = target_id.clone();
    objects[0].terrain.height_path = Some(format!(
        "{}/height_tiles.scene_height_manifest.json", objects[0].private_folder(&target.scene_id),
    ));
    target.manifest.terrains = Some(objects.clone().into());
    target.manifest.terrain = SceneTerrainDescriptor::empty();
    feathrise_scene_content::validate_terrain_objects(&target.manifest).unwrap();
    // 只用真实三角Collider替代外部高度页装载；承托查询、对象身份和粘贴worker均走正式实现。
    let collider = avian3d::prelude::Collider::trimesh(
        vec![Vec3::new(-32.0, 0.0, -32.0), Vec3::new(32.0, 0.0, -32.0),
            Vec3::new(32.0, 0.0, 32.0), Vec3::new(-32.0, 0.0, 32.0)],
        vec![[0, 2, 1], [0, 3, 2]],
    );
    let ground = TerrainGroundSnapshot::test_meshes(target.scene_id.clone(), &objects, collider);
    target.terrain_ground = Some(ground.clone());
    let snap = feathrise_editor_core::EditorSnapSettings { enabled: false, ..default() }
        .translation_snap(false, [0.0; 3]);
    let ClipmapGroundHit::Hit(point) = super::super::paste::object_hit(
        &ground, &Default::default(), Vec3::new(5.0, 10.0, 5.0), Vec3::NEG_Y, snap,
    ) else { panic!("目标必须命中已就绪的独立SparseClipmap承托面，不能因无地形提前失败"); };
    assert!(point.abs_diff_eq(Vec3::new(5.0, 0.0, 5.0), 0.0001));
    assert_ne!(clipboard.source_scene, target.scene_id);
    let sampler = PlacementTerrainSampler::from_sources(&target, None, 0).unwrap();
    assert!(sampler.is_object_surface());
    let ghosts = worker::ghost_records(&clipboard, "static-patch-copy").unwrap();
    let BatchOutput::Paste(result) = worker::run_paste(PasteWork { model_vegetation: None,
        source: ghosts, delta: point - clipboard.origin, records: Vec::new(),
        validator: None, presets: Arc::new(vec![None]), sampler: Some(sampler),
        patch: Some(PatchPasteContext {
            frames: clipboard.patch_frames.clone(),
            target: PatchTarget { object: Arc::new(objects[0].clone()), legacy: false },
            existing: default(),
        }),
        work_plane: false, query: None, bounds: target.manifest.world_bounds().unwrap(),
        scene_id: target.scene_id.clone(), building_revision: 0,
    }, 0) else { panic!("已就绪的固定高度静态patch应完成粘贴，无需建筑校验或额外高度页等待"); };
    let prepared = result.unwrap_or_else(|error| panic!(
        "普通静态三维地形须通过自身patch关联目标Clipmap，不能强加普通terrain_binding而拒绝粘贴：{error}",
    ));
    let placed = &prepared.apply[0];
    assert!(placed.terrain_binding.is_none());
    let mut expected_patch = source.terrain_patch.clone().unwrap();
    expected_patch.terrain_id = Some(target_id);
    assert_eq!(placed.terrain_patch.as_ref(), Some(&expected_patch));
    assert_ne!(placed.id, source.id);
    assert_eq!(placed.asset_path, source.asset_path);
    assert_eq!(placed.asset_target, source.asset_target);
    assert_eq!(placed.height_mode, ScenePlacementHeightMode::FixedHeight);
    assert_eq!(prepared.history.added, prepared.apply);
    assert_eq!(clipboard.records, vec![source.clone()]);
    assert_eq!(source_draft.placements.placements, vec![source]); // 目标准备不能污染旧图或源剪贴板。
}

#[test]
fn legacy_static_patch_without_cutout_pastes_to_object_clipmap() {
    paste_legacy_static_patch(None);
}

#[test]
fn legacy_static_patch_with_cutout_pastes_to_object_clipmap() {
    paste_legacy_static_patch(Some(SceneTerrainCutout { min: [-1.0; 2], max: [1.0; 2] }));
}
