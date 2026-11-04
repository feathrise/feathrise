//! 资料事务只写名称备注，并保留独立网格和LOD的真实文件回归。
use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture {
    root: PathBuf,
    entry: MapCatalogEntry,
    target: MapEditTarget,
}
impl Fixture {
    fn new() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "map-metadata-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(root.join("maps/MAP-fixture")).unwrap();
        let catalog = serde_json::json!({"schema_version":1,"maps":[
            {"id":"MAP-fixture","display_name":"原名称","folder":"maps/MAP-fixture","published":true}
        ]});
        let value = serde_json::json!({
            "schema_version":1,"id":"MAP-fixture","display_name":"原名称","runtime_scene":"DynamicMapScene",
            "world_min":[-512,-512],"world_max":[512,512],"default_spawn":[0,2,0],
            "terrains":[{
                "id":"primary","display_name":"Clipmap网格","enabled":true,
                "transform":{"translation":[0,0,0],"yaw_degrees":0,"scale":[1,1,1]},
                "world_min":[-512,-512],"world_max":[512,512],
                "terrain":{"kind":"sparse_clipmap","glb_path":"","height_path":"maps/MAP-fixture/terrain/height_tiles.map_height_manifest.json",
                    "resolution":[4097,4097],"base_height":-80,"height_scale":160,
                    "collision_lods":[{"max_distance_meters":16,"spacing_multiplier":1},{"max_distance_meters":64,"spacing_multiplier":4}],
                    "display_lods":[{"outer_half_extent_meters":16,"spacing_multiplier":1},{"outer_half_extent_meters":null,"spacing_multiplier":64}]}
            }],
            "future_author_field":[1,2,3]
        });
        let catalog: MapCatalogAsset = serde_json::from_value(catalog).unwrap();
        let entry = catalog.maps[0].clone();
        std::fs::write(
            root.join(MAP_CATALOG_ASSET_PATH),
            serde_json::to_vec(&catalog).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join(entry.manifest_path()),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let target = MapEditTarget {
            id: entry.id.clone(),
            original_name: "原名称".into(),
            original_notes: String::new(),
        };
        Self {
            root,
            entry,
            target,
        }
    }
    fn bytes(&self) -> (Vec<u8>, Vec<u8>) {
        (
            std::fs::read(self.root.join(MAP_CATALOG_ASSET_PATH)).unwrap(),
            std::fs::read(self.root.join(self.entry.manifest_path())).unwrap(),
        )
    }
    fn save(&self, name: &str, notes: &str) -> Result<EditedMap, String> {
        edit_on_disk(&self.root, &self.entry, &self.target, name, notes)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        assert!(self.root.starts_with(std::env::temp_dir()));
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn metadata_save_never_reads_height_sources_or_changes_mesh_lods() {
    let fixture = Fixture::new();
    let before = fixture.bytes();
    fixture.save("原名称", "新备注\n第二行").unwrap();
    let after = fixture.bytes();
    assert_eq!(before.0, after.0);
    let before: serde_json::Value = serde_json::from_slice(&before.1).unwrap();
    let after: serde_json::Value = serde_json::from_slice(&after.1).unwrap();
    assert_eq!(before["terrains"], after["terrains"]);
    assert_eq!(before["future_author_field"], after["future_author_field"]);
    assert_eq!(after["notes"], "新备注\n第二行");
    assert!(after.get("terrain").is_none());
    assert!(!fixture.root.join("maps/MAP-fixture/terrain").exists());
}

#[test]
fn chinese_rename_publishes_catalog_and_manifest_together() {
    let fixture = Fixture::new();
    fixture.save("  中文新名称  ", "备注").unwrap();
    let (catalog, manifest) = fixture.bytes();
    let catalog: MapCatalogAsset = serde_json::from_slice(&catalog).unwrap();
    let manifest: MapManifestAsset = serde_json::from_slice(&manifest).unwrap();
    assert_eq!(catalog.maps[0].display_name, "中文新名称");
    assert_eq!(manifest.display_name, "中文新名称");
    assert_eq!(manifest.notes, "备注");
}

#[test]
fn geometry_changed_after_open_is_preserved_by_metadata_only_save() {
    let fixture = Fixture::new();
    let path = fixture.root.join(fixture.entry.manifest_path());
    let mut current: serde_json::Value = serde_json::from_slice(&fixture.bytes().1).unwrap();
    current["terrains"][0]["terrain"]["display_lods"][0]["outer_half_extent_meters"] = 20.into();
    std::fs::write(path, serde_json::to_vec(&current).unwrap()).unwrap();
    fixture.save("原名称", "只改资料").unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&fixture.bytes().1).unwrap();
    assert_eq!(saved["terrains"], current["terrains"]);
}

#[test]
fn concurrent_metadata_changes_and_invalid_names_leave_disk_untouched() {
    for concurrent in [false, true] {
        let fixture = Fixture::new();
        if concurrent {
            let path = fixture.root.join(fixture.entry.manifest_path());
            let mut value: serde_json::Value = serde_json::from_slice(&fixture.bytes().1).unwrap();
            value["notes"] = "其他作者的新备注".into();
            std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        let before = fixture.bytes();
        assert!(
            fixture
                .save(if concurrent { "原名称" } else { "" }, "未保存备注")
                .is_err()
        );
        assert_eq!(fixture.bytes(), before);
    }
}

#[test]
fn successful_save_closes_only_the_matching_unchanged_text_dialog() {
    for pending in [false, true] {
        let fixture = Fixture::new();
        let saved = fixture.save("原名称", "新备注").unwrap();
        let mut editor = MapEditorState {
            new_map_dialog_open: true,
            new_map_edit_target: Some(fixture.target.clone()),
            new_map_name: "原名称".into(),
            new_map_notes: if pending { "后续备注" } else { "新备注" }.into(),
            ..default()
        };
        assert_eq!(finish_edit_dialog(&mut editor, &saved, false), !pending);
        assert_eq!(editor.new_map_dialog_open, pending);
        if pending {
            assert_eq!(
                editor.new_map_edit_target.as_ref().unwrap().original_notes,
                "新备注"
            );
        }
    }
}

#[test]
fn a_late_success_does_not_close_another_map_and_failure_keeps_text_drafts() {
    let fixture = Fixture::new();
    let saved = fixture.save("原名称", "新备注").unwrap();
    let mut editor = MapEditorState {
        new_map_dialog_open: true,
        new_map_edit_target: Some(MapEditTarget {
            id: MapId::parse("MAP-other").unwrap(),
            ..fixture.target.clone()
        }),
        new_map_name: "后续名称".into(),
        new_map_notes: "后续备注".into(),
        ..default()
    };
    assert!(!finish_edit_dialog(&mut editor, &saved, false));
    report_edit_failure(&mut editor, "外部修改");
    assert!(editor.new_map_dialog_open);
    assert_eq!(editor.new_map_name, "后续名称");
    assert_eq!(editor.new_map_notes, "后续备注");
    assert!(editor.status.contains("外部修改"));
}
