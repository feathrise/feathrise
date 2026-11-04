//! 真实格式与工程来源隔离回归；不启动或改写用户地图。
use super::*;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(1);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "new-editor-map-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const GAME_CONFIG_PATHS: [&str; 4] = [
    "config/character_presets.character_presets.json",
    "config/equipment_presets.equipment_presets.json",
    "config/skill_presets.skill_presets.json",
    "config/dialogue_bindings.dialogue_bindings.json",
];

#[test]
fn empty_project_opens_without_game_presets_or_sample_assets() {
    let fixture = Fixture::new();
    seed::create_empty(&fixture.0).unwrap();
    let project = ProjectMaps::load(&fixture.0).unwrap();
    assert!(project.entries.is_empty());
    assert!(project.building_preset_names.is_empty());
    assert!(project.preset_counts.values().all(|count| *count == 0));
    for path in GAME_CONFIG_PATHS {
        assert!(!fixture.0.join(path).exists(), "空工程不应生成游戏配置：{path}");
    }
    assert!(!fixture.0.join("maps/M1-SAMPLE").exists());
    assert!(!fixture.0.join("scene").exists());
    assert!(!fixture.0.join("textures").exists());
}

#[test]
fn existing_game_configs_remain_opaque_and_unchanged_when_opening_project() {
    let fixture = Fixture::new();
    seed::create_empty(&fixture.0).unwrap();
    // 游戏可自行升级、加密或替换配置格式；编辑器的公共地图加载不解析或改写它们。
    let bytes = b"opaque game-owned data, not an editor JSON schema\r\n";
    for path in GAME_CONFIG_PATHS {
        let path = fixture.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    assert!(ProjectMaps::load(&fixture.0).unwrap().entries.is_empty());
    for path in GAME_CONFIG_PATHS {
        assert_eq!(fs::read(fixture.0.join(path)).unwrap(), bytes);
    }
}

#[test]
fn official_map_files_and_same_names_stay_in_their_project() {
    let a = Fixture::new();
    let b = Fixture::new();
    seed::create(&a.0, "工程A").unwrap();
    seed::create(&b.0, "工程B").unwrap();
    let first = ProjectMaps::load(&a.0).unwrap();
    let other = ProjectMaps::load(&b.0).unwrap();
    let back = ProjectMaps::load(&a.0).unwrap();
    assert_eq!(first.entries[0].id, other.entries[0].id);
    assert_eq!(first.entries[0].placement_count, 1);
    assert_eq!(first.entries[1].placement_count, 0);
    assert_ne!(first.entries[0].name, other.entries[0].name);
    assert_ne!(first.building_presets_sha256, other.building_presets_sha256);
    assert_ne!(first.asset_root, other.asset_root);
    assert_eq!(first.catalog_sha256, back.catalog_sha256);
    assert_eq!(
        first.entries[0].manifest_sha256,
        back.entries[0].manifest_sha256
    );
}
#[test]
fn project_open_accepts_new_and_legacy_runtime_names_without_migrating_saved_files() {
    let fixture = Fixture::new();
    seed::create(&fixture.0, "运行类型").unwrap();
    let path = fixture.0.join("maps/M1-SAMPLE/map.map_manifest.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(value["runtime_scene"], "data_driven"); // 新种子不得携带游戏枚举名。
    for (name, accepted) in [("data_driven", true), ("DynamicMapScene", true),
        ("KaifengScene", false), ("FutureRuntime", false)] {
        value["runtime_scene"] = name.into();
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert_eq!(ProjectMaps::load(&fixture.0).is_ok(), accepted, "{name}");
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn corrupt_identity_and_traversal_never_adopt_a_partial_project() {
    let fixture = Fixture::new();
    seed::create(&fixture.0, "测试").unwrap();
    let path = fixture.0.join("maps/M1-SAMPLE/map.map_manifest.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["id"] = "another-map".into();
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(ProjectMaps::load(&fixture.0).unwrap_err().contains("身份"));
    let mut extra = std::collections::BTreeMap::new();
    assert!(set_selection(&mut extra, "../other").is_err());
    assert!(selection(&extra).unwrap().is_none());
    set_selection(&mut extra, "M1-SAMPLE").unwrap();
    assert_eq!(selection(&extra).unwrap(), Some("M1-SAMPLE"));
}
#[test]
fn seed_never_overwrites_an_existing_project() {
    let fixture = Fixture::new();
    seed::create(&fixture.0, "原名").unwrap();
    let before = fs::read(fixture.0.join("maps/catalog.map_catalog.json")).unwrap();
    assert!(seed::create(&fixture.0, "新名").is_err());
    assert_eq!(
        before,
        fs::read(fixture.0.join("maps/catalog.map_catalog.json")).unwrap()
    );
}

#[test]
fn background_cancel_rejects_before_publishing_context() {
    let fixture = Fixture::new();
    seed::create(&fixture.0, "取消检查").unwrap();
    let mut checks = 0;
    let error = ProjectMaps::load_with_check(&fixture.0, || {
        checks += 1;
        if checks >= 5 {
            Err("本轮已取消".into())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert!(error.contains("已取消"));
    assert!(ProjectMaps::load(&fixture.0).is_ok());
}

#[test]
fn placement_stream_does_not_add_a_prototype_byte_limit() {
    use std::io::Write;
    let fixture = Fixture::new();
    seed::create(&fixture.0, "正式文件").unwrap();
    let path = fixture
        .0
        .join("maps/M1-SAMPLE/placements.map_placements.json");
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    // 合法JSON尾部留白刻意超过旧原型读取门，生产格式允许且不改变摆放内容。
    let block = vec![b' '; 64 * 1024];
    for _ in 0..129 {
        file.write_all(&block).unwrap();
    }
    drop(file);
    assert_eq!(
        ProjectMaps::load(&fixture.0).unwrap().entries[0].placement_count,
        1
    );
}

#[test]
fn sample_has_real_standable_terrain_and_private_author_sources_while_empty_stays_empty() {
    use feathrise_map_content::{MapManifestAsset, MapSurfaceAsset, validate_terrain_objects};
    let fixture = Fixture::new();
    seed::create(&fixture.0, "正式平地").unwrap();
    let sample: MapManifestAsset = serde_json::from_slice(
        &fs::read(fixture.0.join("maps/M1-SAMPLE/map.map_manifest.json")).unwrap(),
    )
    .unwrap();
    validate_terrain_objects(&sample).expect("示例必须可由原生产对象合同接受");
    let objects = sample.terrains.as_ref().unwrap();
    assert_eq!(objects.len(), 1);
    let terrain = &objects[0];
    assert_eq!(sample.world_min, terrain.world_min);
    assert_eq!(sample.world_max, terrain.world_max);
    let bytes = fs::read(fixture.0.join(&terrain.terrain.glb_path)).unwrap();
    let mesh = feathrise_map_terrain::parse_dynamic_terrain_glb(&bytes)
        .expect("生产GLB必须能由原作者加载器回读");
    let derived = feathrise_map_terrain::derive_terrain(
        mesh,
        feathrise_map_terrain::TerrainSampleSpec {
            resolution: terrain.terrain.resolution,
            world_min: terrain.world_min,
            world_max: terrain.world_max,
            base_height: terrain.terrain.base_height,
            height_scale: terrain.terrain.height_scale,
        },
    )
    .unwrap();
    assert_eq!(derived.positions.len(), 33 * 33);
    assert_eq!(derived.indices.len(), 32 * 32 * 6);
    assert!(derived.positions.iter().all(|point| point[1] == 0.0));
    assert!(derived.normals.iter().all(|normal| normal[1] > 0.999));
    assert!(
        sample.default_spawn[0] > terrain.world_min[0]
            && sample.default_spawn[0] < terrain.world_max[0]
    );
    assert!(
        sample.default_spawn[2] > terrain.world_min[1]
            && sample.default_spawn[2] < terrain.world_max[1]
    );
    let surface: MapSurfaceAsset = serde_json::from_slice(
        &fs::read(fixture.0.join(terrain.surface_path.as_deref().unwrap())).unwrap(),
    )
    .unwrap();
    assert_eq!(surface.map_id, sample.id);
    assert!(surface.layers.is_empty());
    assert_eq!(
        fs::metadata(fixture.0.join(&surface.index_source_path))
            .unwrap()
            .len(),
        512 * 512 * 16
    );
    assert_eq!(
        fs::metadata(fixture.0.join(&surface.weight_source_path))
            .unwrap()
            .len(),
        512 * 512 * 4
    );
    for path in [
        &surface.index_source_path,
        &surface.weight_source_path,
        terrain.vegetation_path.as_ref().unwrap(),
        terrain.water_path.as_ref().unwrap(),
    ] {
        assert!(path.starts_with(seed::SAMPLE_TERRAIN_FOLDER));
        assert!(fixture.0.join(path).is_file());
    }
    let empty: MapManifestAsset = serde_json::from_slice(
        &fs::read(fixture.0.join("maps/M1-EMPTY/map.map_manifest.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(empty.terrain_count(), 0);
    assert!(empty.terrain.is_empty());
    assert!(!fixture.0.join("maps/M1-EMPTY/terrains").exists());
}
