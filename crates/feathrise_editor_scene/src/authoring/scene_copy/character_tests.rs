//! 通过正式复制事务验证 RPG 角色文档归属，以及两次复制对共享预设的保留。

use super::copy_scene;
use feathrise_scene_content::{
    SCENE_CATALOG_ASSET_PATH, SCENE_SCHEMA_VERSION, SceneCatalogAsset, SceneCatalogEntry,
    SceneId, ScenePlacementsAsset, ScenePortalsAsset,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    source: SceneCatalogEntry,
    characters_file: &'static str,
}

impl Fixture {
    fn new(registered: bool) -> Self {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("feathrise-copy-roles-{}-{nonce}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let folder = "scenes/守卫区";
        let characters_file = if registered { "配置/守卫.map_characters.json" } else { "characters.map_characters.json" };
        let source = SceneCatalogEntry {
            id: SceneId::parse("SCENE-role-source").unwrap(),
            display_name: "守卫区".to_owned(),
            folder: folder.to_owned(),
            published: true,
            placements_glb: false,
            surface_path: None,
            vegetation_path: None,
            water_path: None,
            boundary_path: None,
            characters_path: registered.then(|| format!("{folder}/{characters_file}")),
        };
        let fixture = Self { root, source, characters_file };
        let source = &fixture.source;
        fs::create_dir_all(fixture.role_path().parent().unwrap()).unwrap();
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
            "terrains": [],
            "asset_roots": [&source.folder],
        }));
        fixture.write_json(&source.placements_path(), &ScenePlacementsAsset {
            scene_id: source.id.clone(),
            ..Default::default()
        });
        fixture.write_json(&source.portals_path(), &ScenePortalsAsset {
            scene_id: source.id.clone(),
            ..Default::default()
        });
        fixture
    }

    fn write_json(&self, relative: &str, value: &impl serde::Serialize) {
        fs::write(self.root.join(relative), serde_json::to_vec(value).unwrap()).unwrap();
    }

    fn role_path(&self) -> PathBuf {
        self.root.join(&self.source.folder).join(self.characters_file)
    }

    fn roles(&self) -> Value {
        json!({
            "schema_version": 1,
            "map_id": &self.source.id,
            "load_distance": 120.0,
            "unload_distance": 160.0,
            "points": [{
                "id": "guard-point",
                "label": self.source.id.as_str(),
                "preset_id": "shared-gate-guard",
                "position": [0.0, 0.0, 0.0],
                "patrol_route": {"mode": "ping_pong", "points": [
                    {"id": "watch-gate", "position": [1.0, 0.0, 0.0], "wait_seconds": 2.0},
                    {"id": "watch-road", "position": [2.0, 0.0, 0.0]}
                ]}
            }],
            "regions": [],
            "layers": [{
                "id": "guard-layer", "label": "守卫", "visible": true,
                "preset_id": "shared-gate-guard", "sources": ["guard-point"]
            }]
        })
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root); // 仅回收本例成功独占创建的目录。
    }
}

fn read_roles(path: &Path) -> Value {
    let bytes = fs::read(path).unwrap();
    serde_json::from_slice(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes)).unwrap()
}

#[test]
fn scene_copy_rekeys_registered_and_legacy_character_documents() {
    for registered in [true, false] {
        let fixture = Fixture::new(registered);
        let original = fixture.roles();
        let mut bytes = b"\xef\xbb\xbf".to_vec(); // 角色的正式读端接受 Windows UTF-8 BOM。
        bytes.extend(serde_json::to_vec(&original).unwrap());
        fs::write(fixture.role_path(), &bytes).unwrap();

        let cancelled = AtomicBool::new(false);
        let first = copy_scene(&fixture.root, &fixture.source.id, "守卫区 副本", &cancelled, |_| {}).unwrap();
        let second = copy_scene(&fixture.root, &fixture.source.id, "守卫区 变体", &cancelled, |_| {}).unwrap();
        assert_ne!(first.scene_id, second.scene_id);
        assert_eq!(fs::read(fixture.role_path()).unwrap(), bytes);

        let catalog: SceneCatalogAsset = serde_json::from_slice(
            &fs::read(fixture.root.join(SCENE_CATALOG_ASSET_PATH)).unwrap(),
        ).unwrap();
        assert_eq!(catalog.scenes.len(), 3);
        assert_eq!(catalog.scenes[0].characters_path, fixture.source.characters_path);
        for copied in [first, second] {
            assert_ne!(copied.scene_id, fixture.source.id);
            let entry = catalog.scenes.iter().find(|entry| entry.id == copied.scene_id).unwrap();
            let relative = format!("{}/{}", entry.folder, fixture.characters_file);
            assert_eq!(entry.characters_path.as_deref(), registered.then_some(relative.as_str()));
            let mut expected = original.clone();
            expected["map_id"] = json!(copied.scene_id);
            // 点位名故意等于旧场景ID，防止误改显示名；公用预设和巡逻路线也应原样保留。
            assert_eq!(read_roles(&fixture.root.join(relative)), expected);
        }
    }
}

#[test]
fn scene_copy_rejects_a_character_document_owned_by_another_scene() {
    let fixture = Fixture::new(true);
    let mut roles = fixture.roles();
    roles["map_id"] = json!("SCENE-other-owner");
    let bytes = serde_json::to_vec(&roles).unwrap();
    fs::write(fixture.role_path(), &bytes).unwrap();
    let catalog = fixture.root.join(SCENE_CATALOG_ASSET_PATH);
    let original_catalog = fs::read(&catalog).unwrap();

    assert!(copy_scene(
        &fixture.root, &fixture.source.id, "归属错误副本", &AtomicBool::new(false), |_| {},
    ).is_err());
    assert_eq!(fs::read(catalog).unwrap(), original_catalog);
    assert_eq!(fs::read(fixture.role_path()).unwrap(), bytes);
    assert!(!fixture.root.join("scenes/归属错误副本").exists());
}
