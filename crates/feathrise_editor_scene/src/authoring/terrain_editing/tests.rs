//! 三种独立作者源的真实文件版本与回读回归；测试只使用独占临时目录。
use super::*;
use crate::types::EditorTool;
use bevy::prelude::*;
use feathrise_scene_content::*;
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        // Windows时钟可能让并行测试取得相同纳秒值；独占创建保留，并用进程序号避免串用作者目录。
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rpg-object-source-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        if self.0.parent() == Some(std::env::temp_dir().as_path()) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
fn object(kind: SceneTerrainKind) -> (SceneId, SceneTerrainObject) {
    let owner = SceneId::parse("MAP-source-test").unwrap();
    let id = TerrainId::parse("terrain-a").unwrap();
    let folder = format!("scenes/{owner}/terrains/{}", id.as_str());
    let object = SceneTerrainObject {clipmap_stitches: Default::default(),
        mesh_lod: None,
        id,
        display_name: "独立源".into(),
        enabled: true,
        transform: Default::default(),
        world_min: [-32.; 2],
        world_max: [32.; 2],
        terrain: SceneTerrainDescriptor {resolved_stitches: Default::default(),
            kind,
            resolution: [129; 2],
            base_height: -8.,
            height_scale: 16.,
            glb_path: format!("{folder}/terrain.glb"),
            height_path: Some(format!("{folder}/height_tiles.scene_height_manifest.json")),
            ..SceneTerrainDescriptor::empty()
        },
        voxel: None,
        surface_path: None,
        vegetation_path: None,
        water_path: None,
        puddles: None,
    };
    (owner, object)
}
fn brush(tool: EditorTool) -> ObjectBrushInput {
    ObjectBrushInput {
        stroke: 1,
        tool,
        center: Vec3::ZERO,
        radius: 2.,
        strength: 1.,
        speed: 24.,
        delta_seconds: 0.05,
        plane_point: Vec3::ZERO,
        shape: Default::default(),
        half_height: 1.,
    }
}
fn write(root: &std::path::Path, path: &str, bytes: &[u8]) {
    let target = root.join(path);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(target, bytes).unwrap();
}

#[test]
fn clipmap_object_sculpt_publishes_a_reopenable_private_version_and_leaves_original_flat() {
    let _app = App::new().add_plugins(MinimalPlugins);
    let temporary = Temp::new();
    let (owner, object) = object(SceneTerrainKind::SparseClipmap);
    let spec = SparseHeightSpec::new(object.world_min, object.world_max, 0.5, -8., 16.).unwrap();
    let mut document = SparseHeightDocument::create_for_terrain(
        &temporary.0,
        owner.clone(),
        Some(object.id.clone()),
        spec,
    )
    .unwrap();
    document.save_dirty().unwrap();
    let path = object.terrain.height_path.as_ref().unwrap();
    let baseline = std::fs::read(temporary.0.join(path)).unwrap();
    let edited = bevy::tasks::block_on(edit(
        temporary.0.clone(),
        owner.clone(),
        object.clone(),
        brush(EditorTool::Raise),
        "edit-test".into(),
    ))
    .unwrap();
    assert_eq!(std::fs::read(temporary.0.join(path)).unwrap(), baseline);
    let mut reopened = SparseHeightDocument::load_source(
        &temporary.0,
        &owner,
        Some(&object.id),
        edited.terrain.height_path.as_ref().unwrap(),
    )
    .unwrap();
    reopened
        .load_tile(SparseHeightTileKey { x: 0, z: 0 })
        .unwrap();
    assert!((reopened.sample_grid(64, 64).unwrap().unwrap() - 1.2).abs() < 0.01);
}

#[test]
fn voxel_object_dig_writes_hashed_density_without_mutating_the_original_source() {
    let _app = App::new().add_plugins(MinimalPlugins);
    let temporary = Temp::new();
    let (owner, mut object) = object(SceneTerrainKind::Voxel);
    object.world_min = [-2.; 2];
    object.world_max = [2.; 2];
    object.terrain.glb_path.clear();
    object.terrain.height_path = None;
    let source = SceneVoxelTerrainSource {
        schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
        scene_id: owner.clone(),
        terrain_id: object.id.clone(),
        cell_size_meters: 0.5,
        min: [-2., -2., -2.],
        max: [2., 0., 2.],
        blocks: Default::default(),
        surface_fields_path: None,
    };
    let path = format!(
        "{}/source.scene_voxel_source.json",
        object.private_folder(&owner)
    );
    object.voxel = Some(SceneVoxelTerrainDescriptor {
        cell_size_meters: source.cell_size_meters,
        min: source.min,
        max: source.max,
        source_path: path.clone(),
        display_lods: default_collision_lods(),
        collision_lods: default_collision_lods(),
    });
    let baseline = serde_json::to_vec_pretty(&source).unwrap();
    write(&temporary.0, &path, &baseline);
    let mut input = brush(EditorTool::VolumeDig);
    input.radius = 1.;
    let edited = bevy::tasks::block_on(edit(
        temporary.0.clone(),
        owner,
        object,
        input,
        "edit-test".into(),
    ))
    .unwrap();
    assert_eq!(std::fs::read(temporary.0.join(&path)).unwrap(), baseline);
    let reopened: SceneVoxelTerrainSource = serde_json::from_slice(
        &std::fs::read(temporary.0.join(&edited.voxel.unwrap().source_path)).unwrap(),
    )
    .unwrap();
    reopened.validate().unwrap();
    assert!(!reopened.blocks.is_empty());
    let reference = reopened
        .blocks
        .iter()
        .find(|reference| reference.key == [0; 3])
        .unwrap();
    let bytes = std::fs::read(temporary.0.join(reopened.block_path(&reference.sha256))).unwrap();
    assert_eq!(authoring_io::bytes_hash(&bytes), reference.sha256);
    assert!(i16::from_le_bytes([bytes[0], bytes[1]]) > 0); // 球心已从岩土/边界变为空气。
}

#[test]
fn native_surface_fields_reopen_without_a_clipmap_and_preserve_the_previous_source() {
    use feathrise_terrain::volume::sparse::paged::*;
    let _app = App::new().add_plugins(MinimalPlugins);
    let temporary = Temp::new();
    let (owner, mut object) = object(SceneTerrainKind::Voxel);
    object.world_min = [-2.; 2];
    object.world_max = [2.; 2];
    object.terrain.glb_path.clear();
    object.terrain.height_path = None;
    object.transform.translation = [20., 3., -10.];
    object.transform.scale = [2., 1., 0.5];
    object.transform.yaw_degrees = 35.;
    let source = SceneVoxelTerrainSource {
        schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
        scene_id: owner.clone(),
        terrain_id: object.id.clone(),
        cell_size_meters: 0.5,
        min: [-2., -2., -2.],
        max: [2., 0., 2.],
        blocks: Default::default(),
        surface_fields_path: None,
    };
    let path = format!(
        "{}/source.scene_voxel_source.json",
        object.private_folder(&owner)
    );
    object.voxel = Some(SceneVoxelTerrainDescriptor {
        cell_size_meters: source.cell_size_meters,
        min: source.min,
        max: source.max,
        source_path: path.clone(),
        display_lods: default_collision_lods(),
        collision_lods: default_collision_lods(),
    });
    let original = serde_json::to_vec_pretty(&source).unwrap();
    write(&temporary.0, &path, &original);
    let frame =
        feathrise_scene_core::residency::SceneSourceFrame::from_transform(object.transform.as_transform());
    let collider = avian3d::prelude::Collider::trimesh(
        vec![
            Vec3::new(-2., 0., -2.),
            Vec3::new(2., 0., -2.),
            Vec3::new(2., 0., 2.),
            Vec3::new(-2., 0., 2.),
        ],
        vec![[0, 2, 1], [0, 3, 2]],
    );
    let point = frame.to_world(Vec3::ZERO);
    let input = NativeFieldInput {
        stroke: 1,
        query: feathrise_scene::paint_surface::PaintSurfaceSnapshot {
            target: SurfaceTarget::Terrain { terrain_id: object.id.clone() },
            instance_path: SceneInstancePath::root(owner.clone()).terrain(object.id.clone()),
            geometry_revision: [1; 32],
            geometry: feathrise_scene::paint_surface::PaintSurfaceGeometry::Terrain(
                feathrise_scene::TerrainObjectRaySnapshot::Mesh(collider, frame)),
        },
        ray: (point + Vec3::Y * 10., Vec3::NEG_Y),
        brush: VolumeFieldBrush {
            channel: VolumeFieldChannel::Surface,
            id: "stone".into(),
            from: point.to_array(),
            to: point.to_array(),
            normal: Vec3::Y.to_array(),
            radius: 1.,
            strength: 1.,
            flow_exposure: 0.,
            erase: false,
            source_to_world: Mat3::IDENTITY,
        },
    };
    let next = bevy::tasks::block_on(edit_fields(
        temporary.0.clone(),
        owner,
        object.clone(),
        input,
        "field-test".into(),
    ))
    .unwrap();
    assert_eq!(std::fs::read(temporary.0.join(&path)).unwrap(), original);
    let next_source: SceneVoxelTerrainSource = serde_json::from_slice(
        &std::fs::read(temporary.0.join(&next.voxel.unwrap().source_path)).unwrap(),
    )
    .unwrap();
    next_source.validate().unwrap();
    assert!(next_source.blocks.is_empty() && source.blocks.is_empty());
    let field_path = temporary.0.join(next_source.surface_fields_path.unwrap());
    let field = decode_paged_volume(&std::fs::read(&field_path).unwrap())
        .unwrap()
        .unwrap();
    let fields = load_surface_fields(
        &field_path.parent().unwrap().join("objects"),
        &field,
        &[[0.; 3]],
    )
    .unwrap();
    assert_eq!(
        fields.samples[&[0, 0, 0]].surface,
        vec![("stone".into(), 255)]
    );
    assert!(fields.samples.values().all(|sample| {
        frame
            .to_world(Vec3::from_array(sample.position))
            .distance(point)
            <= 1.001
    }));
    assert_eq!(object.voxel.unwrap().source_path, path); // 历史持有旧路径即可撤销，不需要恢复Clipmap。
}
