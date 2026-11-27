//! 验证独立场景的正式文件、引用、动画、取消和回滚契约。

use super::{CopiedScene, SCENE_FOLDER_ASSET_PATH, copy_scene};
use crate::authoring::scene_motion::{
    MotionRegion, MotionRegionAction, SceneMotionDocument, SceneMotionKey, SceneMotionPlayback, SceneMotionTrack,
};
use bevy::{gltf::gltf::binary::Glb, math::Vec3};
use feathrise_scene_content::{
    SCENE_CATALOG_ASSET_PATH, SCENE_SCHEMA_VERSION, SceneCatalogAsset, SceneCatalogEntry, SceneId,
    ModelSurfaceAnchor, ModelSurfaceVegetation, ModelSurfaceVegetationSample, ScenePlacementsAsset,
    SceneVegetationAsset, SparseHeightManifest, SparseHeightSpec, load_model_vegetation_blocks,
    prepare_model_vegetation_blocks, read_scene_placements, sparse_height_manifest_asset_path,
    write_scene_placements_glb,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    source: SceneCatalogEntry,
}

impl Fixture {
    fn new(stable_folder: bool, glb: bool) -> Self {
        let unix_nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("feathrise-copy-{}-{unix_nanos}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let id = SceneId::parse("SCENE-copy-source").unwrap();
        let folder = if stable_folder { format!("scenes/{id}") } else { "scenes/机关房".into() };
        let source = SceneCatalogEntry {
            id,
            display_name: "机关房".into(),
            folder,
            published: true,
            placements_glb: glb,
            surface_path: None,
            vegetation_path: None,
            water_path: None,
            boundary_path: None,
            characters_path: None,
        };
        let fixture = Self { root, source };
        let source = &fixture.source;
        let height = sparse_height_manifest_asset_path(&source.id);
        fixture.write_json(SCENE_CATALOG_ASSET_PATH, &SceneCatalogAsset {
            schema_version: SCENE_SCHEMA_VERSION,
            scenes: vec![source.clone()],
        });
        fixture.write_json(&source.manifest_path(), &json!({
            "schema_version": SCENE_SCHEMA_VERSION,
            "id": &source.id,
            "display_name": &source.display_name,
            "runtime_scene": "data_driven",
            "world_min": [-4.0, -4.0],
            "world_max": [4.0, 4.0],
            "default_spawn": [0.0, 0.0, 0.0],
            "terrain": {
                "kind": "sparse_clipmap",
                "glb_path": "",
                "height_path": &height,
                "resolution": [9, 9],
                "base_height": -8.0,
                "height_scale": 16.0,
                "player_collision_preload_radius_meters": 16.0,
            },
            "asset_roots": [&source.folder, format!("scenes/{}", source.id), "models"],
            "notes": &source.folder,
        }));
        let spec = SparseHeightSpec::new([-4.0; 2], [4.0; 2], 1.0, -8.0, 16.0).unwrap();
        fixture.write_json(&height, &SparseHeightManifest::empty(source.id.clone(), spec).unwrap());

        let portal = |id: &str, destination: &str| json!({
            "id": id, "label": id, "position": [0.0, 0.0, 0.0], "arrival": [1.0, 0.0, 0.0],
            "target": {"scene_id": destination, "portal_id": "return"},
            "color_srgba": [1.0, 1.0, 1.0, 1.0], "arrow_degrees": 0.0,
        });
        fixture.write_json(&source.portals_path(), &json!({
            "schema_version": SCENE_SCHEMA_VERSION, "scene_id": &source.id,
            "portals": [portal("return", source.id.as_str()), portal("exit", "OTHER")],
        }));
        fixture
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn write_json(&self, relative: &str, value: &impl serde::Serialize) {
        self.write(relative, &serde_json::to_vec(value).unwrap());
    }

    fn read_json(&self, relative: &str) -> Value {
        serde_json::from_slice(&fs::read(self.root.join(relative)).unwrap()).unwrap()
    }

    fn copy(&self, name: &str) -> CopiedScene {
        copy_scene(&self.root, &self.source.id, name, &AtomicBool::new(false), |_| {}).unwrap()
    }

    fn scene_names(&self) -> Vec<String> {
        let mut entries = fs::read_dir(self.root.join("scenes")).unwrap().map(|entry| {
            entry.unwrap().file_name().into_string().unwrap()
        }).collect::<Vec<_>>();
        entries.sort();
        entries
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn full_scene_copy_keeps_motion_binary_and_reuses_only_project_assets() {
    let fixture = Fixture::new(false, true);
    let source = &fixture.source;
    let shared = "models/公共门.glb";
    let private = format!("{}/机关.glb", source.folder);
    fixture.write(shared, b"shared model asset");
    fixture.write(&private, b"scene-owned model");
    let texture = format!("scenes/{}/terrain/采样.png", source.id);
    fixture.write(&texture, b"local texture data");
    fixture.write("models/公共贴图.png", b"shared texture data");
    fixture.write_json(&format!("{}/机关.gltf", source.folder), &json!({
        "asset": {"version": "2.0"},
        "images": [
            {"uri": format!("../{}/terrain/采样.png", source.id)},
            {"uri": "../../models/公共贴图.png"}
        ],
    }));

    let mut door = crate::viewport::placement_move::interaction_tests::record();
    door.id = "door".into();
    door.asset_path = shared.into();
    door.water_surfaces.clear();
    let mut mechanism = door.clone();
    mechanism.id = "mechanism".into();
    mechanism.asset_path.clone_from(&private);
    let placements = ScenePlacementsAsset {
        scene_id: source.id.clone(), placements: vec![door.clone(), mechanism], ..Default::default()
    };
    let scene_file = source.placements_path();
    let path = Path::new(&scene_file);
    let bytes = write_scene_placements_glb(&placements, &scene_file, None).unwrap();
    let mut motion = SceneMotionDocument::from_glb(path, bytes.clone(), &placements).unwrap();
    motion.create("开门").unwrap();
    let clip = motion.active_clip_id().unwrap().to_owned();
    let area = MotionRegion {
        center: Vec3::new(3.0, 1.0, -2.0), half_extents: Vec3::new(2.0, 1.0, 0.5),
        enter: MotionRegionAction::Play, exit: MotionRegionAction::Reverse,
    };
    motion.change_region(&clip, 0, None, Some(area)).unwrap();
    motion.replace(&door.id, Some(SceneMotionTrack {
        placement_id: door.id.clone(), duration_seconds: 2.0, playback: SceneMotionPlayback::Once,
        position: vec![
            SceneMotionKey { time: 0.0, value: [0.0; 3] },
            SceneMotionKey { time: 1.0, value: [1.0, 0.0, 0.0] },
        ],
        rotation: Vec::new(), scale: Vec::new(), collider: None,
    }));
    let (bytes, _) = motion.encode_glb(path, bytes, &placements, false).unwrap();
    fixture.write(&scene_file, &bytes);
    let target = fixture.copy("机关房 变体");
    assert_ne!(source.id, target.scene_id);
    assert_eq!(target.display_name, "机关房 变体");
    assert_eq!(target.folder, "scenes/机关房 变体");

    let copy_file = format!("{}/scene.glb", target.folder);
    let copy_bytes = fs::read(fixture.root.join(&copy_file)).unwrap();
    let entries = read_scene_placements(&copy_bytes, &copy_file).unwrap();
    assert_eq!(entries.scene_id, target.scene_id);
    assert_eq!(entries.placements.iter().find(|entry| entry.id == "door").unwrap().asset_path, shared);
    assert_eq!(entries.placements.iter().find(|entry| entry.id == "mechanism").unwrap().asset_path, format!("{}/机关.glb", target.folder));
    assert_eq!(fs::read(fixture.root.join(&target.folder).join("机关.glb")).unwrap(), b"scene-owned model");
    assert_eq!(fs::read(fixture.root.join(shared)).unwrap(), b"shared model asset");
    let local_model = fixture.read_json(&format!("{}/机关.gltf", target.folder));
    assert_eq!(local_model["images"][0]["uri"], format!("../{}/terrain/%E9%87%87%E6%A0%B7.png", target.scene_id));
    assert_eq!(local_model["images"][1]["uri"], "../../models/公共贴图.png");
    let old_glb = Glb::from_slice(&bytes).unwrap();
    let new_glb = Glb::from_slice(&copy_bytes).unwrap();
    assert_eq!(old_glb.bin, new_glb.bin);

    let target_path = Path::new(&copy_file);
    let mut motion = SceneMotionDocument::from_glb(target_path, copy_bytes.clone(), &entries).unwrap();
    assert_eq!(motion.regions(&clip), Some([area].as_slice()));
    let wider = MotionRegion { half_extents: Vec3::new(5.0, 1.0, 0.5), ..area };
    motion.change_region(&clip, 0, Some(area), Some(wider)).unwrap();
    let (changed, _) = motion.encode_glb(target_path, copy_bytes, &entries, false).unwrap();
    fixture.write(&copy_file, &changed);
    assert_eq!(fs::read(fixture.root.join(scene_file)).unwrap(), bytes);

    let destination_height = sparse_height_manifest_asset_path(&target.scene_id);
    let manifest = fixture.read_json(&format!("{}/scene.scene_manifest.json", target.folder));
    assert_eq!(manifest["id"], json!(target.scene_id));
    assert_eq!(manifest["terrain"]["height_path"], destination_height);
    assert_eq!(manifest["notes"], source.folder);
    assert_eq!(fixture.read_json(&destination_height)["scene_id"], json!(target.scene_id));
    let portals = fixture.read_json(&format!("{}/portals.scene_portals.json", target.folder));
    assert_eq!(portals["portals"][0]["target"]["scene_id"], json!(target.scene_id));
    assert_eq!(portals["portals"][0]["target"]["portal_id"], "return");
    assert_eq!(portals["portals"][1]["target"]["scene_id"], "OTHER");
}

#[test]
fn legacy_scene_and_stable_height_paths_can_share_one_new_root() {
    let fixture = Fixture::new(true, false);
    let source = &fixture.source;
    fixture.write_json(&source.placements_path(), &ScenePlacementsAsset {
        scene_id: source.id.clone(), ..Default::default()
    });
    let mut folders = crate::types::SceneFolderDocument::default();
    folders.folders.push(crate::types::SceneFolderRecord {
        id: "FOLDER-mechanism".into(), display_name: "机关".into(), parent_id: None,
    });
    folders.scene_folders.insert(source.id.to_string(), "FOLDER-mechanism".into());
    fixture.write_json(SCENE_FOLDER_ASSET_PATH, &folders);

    let target = fixture.copy("旧场景副本");
    assert_eq!(target.folder, format!("scenes/{}", target.scene_id));
    assert_eq!(target.folder_snapshot.unwrap().document.scene_folders.get(target.scene_id.as_str()).map(String::as_str), Some("FOLDER-mechanism"));
    let copy_file = format!("{}/placements.scene_placements.json", target.folder);
    assert_eq!(fixture.read_json(&copy_file)["scene_id"], json!(target.scene_id));
    assert_eq!(fixture.read_json(&sparse_height_manifest_asset_path(&target.scene_id))["scene_id"], json!(target.scene_id));
}

#[test]
fn cancellation_and_malformed_input_leave_no_registered_or_private_output() {
    let fixture = Fixture::new(false, true);
    let source = &fixture.source;
    let placements = ScenePlacementsAsset { scene_id: source.id.clone(), ..Default::default() };
    let path = source.placements_path();
    let good = write_scene_placements_glb(&placements, &path, None).unwrap();
    fixture.write(&path, &good);
    let existing = fixture.scene_names();
    let catalog = fs::read(fixture.root.join(SCENE_CATALOG_ASSET_PATH)).unwrap();
    let cancelled = AtomicBool::new(false);
    let result = copy_scene(&fixture.root, &source.id, "取消副本", &cancelled, |stage| {
        if stage == "正在校验场景副本…" { cancelled.store(true, Ordering::Release); }
    });
    assert!(result.unwrap_err().contains("取消"));
    assert_eq!(fixture.scene_names(), existing);

    fixture.write(&path, b"invalid glb");
    assert!(copy_scene(&fixture.root, &source.id, "失败副本", &AtomicBool::new(false), |_| {}).is_err());
    assert_eq!(fixture.scene_names(), existing);
    assert_eq!(fs::read(fixture.root.join(SCENE_CATALOG_ASSET_PATH)).unwrap(), catalog);
    assert_eq!(fs::read(fixture.root.join(&path)).unwrap(), b"invalid glb");

    fixture.write(&path, &good);
    fixture.write_json(&source.portals_path(), &json!({
        "schema_version": SCENE_SCHEMA_VERSION, "scene_id": &source.id, "portals": [42],
    }));
    assert!(copy_scene(&fixture.root, &source.id, "损坏传送点", &AtomicBool::new(false), |_| {})
        .unwrap_err().contains("场景传送点"));
    assert_eq!(fixture.scene_names(), existing);
    assert_eq!(fs::read(fixture.root.join(SCENE_CATALOG_ASSET_PATH)).unwrap(), catalog);
}

#[test]
fn model_vegetation_blocks_keep_their_samples_and_acquire_a_new_owner() {
    let fixture = Fixture::new(false, false);
    let source = &fixture.source;
    let mut surface = ModelSurfaceVegetation::empty("floor", [3; 32], 0.25);
    surface.samples.push(ModelSurfaceVegetationSample {
        anchor: ModelSurfaceAnchor { primitive: 0, triangle: 1, barycentric: [0.2, 0.3] },
        layers: vec![("grass".into(), 200)],
    });
    let original = SceneVegetationAsset {
        scene_id: source.id.clone(), model_surfaces: vec![Arc::new(surface)].into(),
        ..Default::default()
    };
    let (metadata, blocks) = prepare_model_vegetation_blocks(&source.folder, &original).unwrap();
    fixture.write_json(&format!("{}/vegetation.scene_vegetation.json", source.folder), &metadata);
    for (path, bytes) in &blocks { fixture.write(path, bytes); }
    let target = fixture.copy("植被副本");
    let path = format!("{}/vegetation.scene_vegetation.json", target.folder);
    let mut copied: SceneVegetationAsset = serde_json::from_value(fixture.read_json(&path)).unwrap();
    let new_block = &copied.model_surfaces[0].sample_blocks[0];
    assert_ne!(new_block.rsplit('/').next(), blocks[0].0.rsplit('/').next());
    assert_eq!(fixture.read_json(new_block)["scene_id"], json!(target.scene_id));
    load_model_vegetation_blocks(&fixture.root, &target.folder, &mut copied).unwrap();
    assert_eq!(copied.model_surfaces, original.model_surfaces);
    assert_eq!(fs::read(fixture.root.join(&blocks[0].0)).unwrap(), blocks[0].1);

    let mut legacy_metadata = metadata;
    let custom_name = "vegetation/custom/旧采样.json";
    let custom_path = format!("{}/{custom_name}", source.folder);
    fixture.write(&custom_path, &blocks[0].1);
    Arc::make_mut(&mut Arc::make_mut(&mut legacy_metadata.model_surfaces)[0]).sample_blocks[0] = custom_path;
    fixture.write_json(&format!("{}/vegetation.scene_vegetation.json", source.folder), &legacy_metadata);
    let legacy_target = fixture.copy("自定义采样副本");
    let legacy_path = format!("{}/vegetation.scene_vegetation.json", legacy_target.folder);
    let mut legacy_copy: SceneVegetationAsset = serde_json::from_value(fixture.read_json(&legacy_path)).unwrap();
    assert_eq!(legacy_copy.model_surfaces[0].sample_blocks[0], format!("{}/{custom_name}", legacy_target.folder));
    load_model_vegetation_blocks(&fixture.root, &legacy_target.folder, &mut legacy_copy).unwrap();
    assert_eq!(legacy_copy.model_surfaces, original.model_surfaces);
}

#[test]
fn directory_claimed_by_another_writer_is_left_intact() {
    let fixture = Fixture::new(false, false);
    fixture.write("scenes/已被占用/保留.txt", b"preexisting");
    let source = &fixture.source;
    let result = copy_scene(&fixture.root, &source.id, "已被占用", &AtomicBool::new(false), |_| {});
    assert!(result.is_err());
    assert_eq!(fs::read(fixture.root.join("scenes/已被占用/保留.txt")).unwrap(), b"preexisting");
}
