//! 水体目录真实文件事务回归：空目录、规范/历史引用与拒绝覆盖均使用自建临时根。

use super::*;
use feathrise_map_content::{MapCatalogEntry, MapId, MapWaterLayer, TerrainId};
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

pub(super) fn preset() -> WaterPresetDescriptor {
    let mut value = super::super::defaults::new_descriptor();
    value.id = "water-main".into();
    value.aliases = vec!["legacy-water".into()];
    value
}

pub(super) fn catalog() -> WaterPresetCatalogAsset {
    WaterPresetCatalogAsset { schema_version: WATER_PRESET_SCHEMA_VERSION, presets: vec![preset()] }
}

pub(super) fn registry(catalog: &WaterPresetCatalogAsset) -> WaterPresetRegistry {
    let mut app = crate::ui::status_bar::feedback_tests::asset_app();
    let mut registry = app.world_mut().remove_resource::<WaterPresetRegistry>().unwrap();
    registry.resolved = ResolvedWaterPresetCatalog::build(catalog).unwrap();
    registry.revision = 1;
    registry.last_error = None;
    registry
}

pub(super) fn water(reference: &str) -> MapWaterAsset {
    MapWaterAsset {
        schema_version: MAP_SCHEMA_VERSION,
        map_id: MapId::parse("EDGE-TEST").unwrap(),
        layers: vec![MapWaterLayer {
            id: "water-layer".into(), label: "中性水层".into(),
            mask_source_path: "maps/EDGE-TEST/water.r8".into(),
            mask_texture_path: "maps/EDGE-TEST/water.ktx2".into(),
            resolution: [4, 4], canvas_bounds: None, water_level: 0.0,
            still_kind: Default::default(), preset_id: reference.into(),
            flow_direction: [1.0, 0.0], flow_speed: 0.0,
            base_color_srgba: [0.1, 0.2, 0.3, 0.7], perceptual_roughness: 0.06,
        }],
        ..Default::default()
    }
}

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("feathrise-water-catalog-{}-{}",
            std::process::id(), NEXT_ROOT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path).expect("只创建新的独占临时目录，拒绝覆盖已有路径");
        let root = Self(path);
        root.write(WATER_PRESET_CATALOG_ASSET_PATH, &catalog());
        root.write(MAP_CATALOG_ASSET_PATH, &MapCatalogAsset { schema_version: MAP_SCHEMA_VERSION, maps: vec![] });
        root
    }

    fn write(&self, relative: &str, value: &impl serde::Serialize) {
        let path = safe_asset_path(&self.0, relative).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }

    fn catalog_bytes(&self) -> Vec<u8> {
        std::fs::read(self.0.join(WATER_PRESET_CATALOG_ASSET_PATH)).unwrap()
    }

    fn delete(&self, baseline: WaterPresetCatalogAsset) -> Result<WaterCatalogWriteResult, String> {
        write_catalog_transaction(self.0.clone(), baseline,
            WaterCatalogMutation::Delete { id: "water-main".into() }, 1, None)
    }

    fn map(&self, reference: &str, private: bool) -> (MapCatalogEntry, MapManifestAsset, String) {
        let mut manifest = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])).manifest;
        let path = if private { "maps/EDGE-TEST/terrains/private/water.map_water.json" }
            else { "maps/EDGE-TEST/water.map_water.json" }.to_owned();
        let entry: MapCatalogEntry = serde_json::from_value(serde_json::json!({
            "id": "EDGE-TEST", "display_name": "中性地图", "folder": "maps/EDGE-TEST",
            "water_path": if private { None } else { Some(path.as_str()) }
        })).unwrap();
        if private {
            let mut objects = manifest.terrain_objects_for_edit().to_vec();
            objects[0].id = TerrainId::parse("private").unwrap();
            objects[0].water_path = Some(path.clone());
            manifest.terrains = Some(objects.into());
        }
        self.write(MAP_CATALOG_ASSET_PATH, &MapCatalogAsset { schema_version: MAP_SCHEMA_VERSION, maps: vec![entry.clone()] });
        self.write(&entry.manifest_path(), &manifest);
        self.write(&path, &water(reference));
        (entry, manifest, path)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        // 只删除 new 成功创建的确定临时子目录，绝不推导或触碰工作区资产根。
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn editing_preserves_stable_id_but_allows_label_changes() {
    let mut edited = preset();
    edited.label = "作者新名称".into();
    let updated = mutate_catalog(catalog(), &WaterCatalogMutation::Upsert {
        original_id: Some("water-main".into()), descriptor: edited.clone(),
    }).unwrap();
    assert_eq!(updated.presets[0].id, "water-main");
    assert_eq!(updated.presets[0].label, "作者新名称");
    edited.id = "changed-id".into();
    assert!(mutate_catalog(catalog(), &WaterCatalogMutation::Upsert {
        original_id: Some("water-main".into()), descriptor: edited,
    }).unwrap_err().contains("稳定 ID"));
}

#[test]
fn deleting_unreferenced_last_preset_publishes_empty_catalog_and_allows_recreation() {
    let root = TestRoot::new();
    let result = root.delete(catalog()).unwrap();
    assert!(result.deleted);
    assert_eq!(result.preset_id, "water-main");
    let empty: WaterPresetCatalogAsset = serde_json::from_slice(&root.catalog_bytes()).unwrap();
    assert!(empty.presets.is_empty());
    assert!(ResolvedWaterPresetCatalog::build(&empty).unwrap().presets().is_empty());
    write_catalog_transaction(root.0.clone(), empty, WaterCatalogMutation::Upsert {
        original_id: None, descriptor: preset(),
    }, 2, Some(1)).unwrap();
    assert_eq!(serde_json::from_slice::<WaterPresetCatalogAsset>(&root.catalog_bytes()).unwrap(), catalog());
}

#[test]
fn canonical_and_alias_references_in_root_or_private_water_block_the_last_preset() {
    for reference in ["water-main", "legacy-water"] {
        for private in [false, true] {
            let root = TestRoot::new();
            let (_, _, path) = root.map(reference, private);
            let before = root.catalog_bytes();
            let water_before = std::fs::read(root.0.join(&path)).unwrap();
            let error = root.delete(catalog()).unwrap_err();
            assert!(error.contains("仍被中性地图(EDGE-TEST)引用"), "{error}");
            assert_eq!(before, root.catalog_bytes());
            assert_eq!(water_before, std::fs::read(root.0.join(path)).unwrap());
        }
    }
}

#[test]
fn inline_legacy_water_does_not_implicitly_reference_the_first_preset() {
    let root = TestRoot::new();
    let (_, _, path) = root.map("", true);
    let water_before = std::fs::read(root.0.join(&path)).unwrap();
    root.delete(catalog()).unwrap();
    assert_eq!(water_before, std::fs::read(root.0.join(path)).unwrap());
    assert!(serde_json::from_slice::<WaterPresetCatalogAsset>(&root.catalog_bytes()).unwrap().presets.is_empty());
}

#[test]
fn unregistered_empty_and_clipmap_water_use_the_same_reference_rules_as_loading() {
    for kind in [MapTerrainKind::Empty, MapTerrainKind::SparseClipmap] {
        for case in 0..5 {
            let root = TestRoot::new();
            let (mut entry, mut manifest, _) = root.map("legacy-water", false);
            entry.water_path = None;
            manifest.terrain.kind = kind;
            root.write(MAP_CATALOG_ASSET_PATH, &MapCatalogAsset {
                schema_version: MAP_SCHEMA_VERSION, maps: vec![entry.clone()],
            });
            root.write(&entry.manifest_path(), &manifest);
            let relative = format!("{}/{}", entry.folder, MAP_WATER_FILE_NAME);
            root.write(&relative, &water("legacy-water"));
            match case {
                1 => std::fs::remove_file(root.0.join(&relative)).unwrap(),
                2 => { root.write(&relative, &water("")); }
                3 => std::fs::write(root.0.join(&relative), b"not json").unwrap(),
                4 => { let mut value = water(""); value.map_id = MapId::parse("OTHER").unwrap(); root.write(&relative, &value); }
                _ => (),
            }
            let before = root.catalog_bytes();
            let result = root.delete(catalog());
            if matches!(case, 1 | 2) {
                assert!(result.is_ok(), "确切缺失或内联水不引用预设：{result:?}");
            } else {
                assert!(result.is_err(), "旧私有水引用、坏文件和错图不能漏过检查");
                assert_eq!(root.catalog_bytes(), before);
            }
        }
    }
}

#[test]
fn explicit_private_water_registration_takes_priority_over_optional_legacy_path() {
    let root = TestRoot::new();
    let (entry, mut manifest, _) = root.map("", true);
    let relative = format!("{}/{}", entry.folder, MAP_WATER_FILE_NAME);
    manifest.terrain.kind = MapTerrainKind::Empty;
    let mut terrains = manifest.terrains.as_ref().unwrap().to_vec();
    terrains[0].water_path = Some(relative.clone());
    manifest.terrains = Some(terrains.into());
    root.write(&entry.manifest_path(), &manifest);
    assert!(!root.0.join(&relative).exists());
    let before = root.catalog_bytes();
    assert!(root.delete(catalog()).unwrap_err().contains("读取地图"));
    assert_eq!(root.catalog_bytes(), before, "同路径一经显式登记，缺文件就不能走可选兼容分支");
}

#[test]
fn unknown_missing_or_mismatched_map_sources_never_authorize_deletion() {
    for case in 0..8 {
        let root = TestRoot::new();
        let (entry, mut manifest, path) = root.map("unrelated-preset", true);
        match case {
            0 => { manifest.runtime_scene = "unknown-project-type".into(); root.write(&entry.manifest_path(), &manifest); }
            1 => { manifest.schema_version += 1; root.write(&entry.manifest_path(), &manifest); }
            2 => { manifest.id = MapId::parse("OTHER").unwrap(); root.write(&entry.manifest_path(), &manifest); }
            3 => { std::fs::remove_file(root.0.join(entry.manifest_path())).unwrap(); }
            4 => { std::fs::remove_file(root.0.join(&path)).unwrap(); }
            5 => { std::fs::write(root.0.join(&path), b"not json").unwrap(); }
            6 => { let mut water = water("unrelated-preset"); water.map_id = MapId::parse("OTHER").unwrap(); root.write(&path, &water); }
            _ => { let mut water = water("unrelated-preset"); water.schema_version += 1; root.write(&path, &water); }
        }
        let before = root.catalog_bytes();
        assert!(root.delete(catalog()).is_err(), "损坏来源 {case} 不得被当作无引用");
        assert_eq!(root.catalog_bytes(), before, "拒绝删除时不能改写目录");
    }
}

#[test]
fn external_catalog_change_rejects_delete_without_overwriting_author_edits() {
    let root = TestRoot::new();
    let mut external = catalog();
    external.presets[0].label = "外部修改".into();
    root.write(WATER_PRESET_CATALOG_ASSET_PATH, &external);
    let before = root.catalog_bytes();
    assert!(root.delete(catalog()).unwrap_err().contains("外部修改"));
    assert_eq!(root.catalog_bytes(), before);
}

#[test]
fn explicit_create_in_an_empty_catalog_opens_a_neutral_draft_without_installing_it() {
    let empty = WaterPresetCatalogAsset { schema_version: WATER_PRESET_SCHEMA_VERSION, presets: vec![] };
    let mut app = super::super::workbench_tests::interaction_app(WaterPresetEditorState::default());
    app.insert_resource(registry(&empty));
    super::super::workbench_tests::press(&mut app, super::super::types::WaterPresetAction::Create);
    let state = app.world().resource::<WaterPresetEditorState>();
    assert_eq!(state.tabs.len(), 1);
    let draft = &state.tabs[0].draft;
    assert!(matches!(draft.mode, WaterDraftMode::Create));
    assert!(draft.descriptor.id.starts_with("water-"));
    assert!(draft.descriptor.aliases.is_empty());
    assert!(draft.descriptor.micro_normals.iter().all(|normal| normal.texture_path.is_none()));
    assert!(ResolvedWaterPresetCatalog::build(&WaterPresetCatalogAsset {
        schema_version: WATER_PRESET_SCHEMA_VERSION, presets: vec![draft.descriptor.clone()],
    }).is_ok());
    assert!(app.world().resource::<WaterPresetRegistry>().presets().is_empty());
    assert!(state.write_task.is_none() && state.adoption.is_none());
}
