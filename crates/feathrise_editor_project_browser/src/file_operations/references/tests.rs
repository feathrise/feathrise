//! 独占资产根内通过同一个后台事务读写实际场景、预设和文件；另行解析保存结果。

use super::*;
use crate::file_operations::tests::{Fixture, execute as run};
use serde_json::json;

fn make_scene(asset_path: &str, scene_id: &str) -> Vec<u8> {
    let node = json!({"name":"实例", "extras":{"feathrise_id":"i",
        "feathrise_placement": json!({"asset_path":asset_path,"water_surfaces":[{"node_path":[asset_path]}]}).to_string()}});
    let mut text = serde_json::to_vec(&json!({"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0,1]}],
        "nodes":[{"name":"Feathrise","extras":{"feathrise_scene_id":scene_id,"feathrise_scene_version":2}},node]})).unwrap();
    text.resize(text.len().next_multiple_of(4), b' ');
    let total = 20 + text.len() + 12;
    let mut bytes = b"glTF".to_vec();
    for n in [2, total as u32, text.len() as u32] { bytes.extend_from_slice(&n.to_le_bytes()); }
    bytes.extend_from_slice(b"JSON"); bytes.extend(text);
    bytes.extend_from_slice(&4u32.to_le_bytes()); bytes.extend_from_slice(b"TESTsame");
    bytes
}

fn request(f: &Fixture, op: FileOperation) -> FileOperationRequest {
    let mut r = f.request(op);
    r.references = Some(ReferenceUpdate::Inspect(f.path("assets")));
    r
}

fn inspect(r: &FileOperationRequest) -> Arc<ReferencePreview> {
    let (receipt, result) = run(r.clone(), false);
    result.unwrap();
    assert!(receipt.completed.is_empty());
    receipt.reference_preview.as_ref().expect("操作没有产生引用预览").clone()
}

fn prepare(f: &Fixture) {
    for dir in ["assets/models", "assets/scenes/a", "assets/scenes/b", "assets/config", "assets/out"] {
        fs::create_dir_all(f.path(dir)).unwrap();
    }
    fs::write(f.path("assets/models/门.glb"), b"model").unwrap();
    fs::write(f.path("assets/scenes/a/scene.glb"), make_scene("models/门.glb", "SCENE-a")).unwrap();
    fs::write(f.path("assets/scenes/b/placements.scene_placements.json"), r#"{"placements":[{"asset_path":"models/门.glb"}]}"#).unwrap();
    fs::write(f.path("assets/config/example.building_presets.json"), "\u{feff}{ \"asset_path\" : \"models/门.glb#Scene0\", \"label\": \"models/门.glb\" }\r\n").unwrap();
}

#[test]
fn reference_move_previews_and_repairs_two_scenes_and_a_preset_without_touching_other_bytes() {
    let f = Fixture::new();
    prepare(&f);
    let mut r = request(&f, FileOperation::Rename { source: f.path("assets/models/门.glb"), stamp: FileEntryStamp::Untracked, name: "门楼.glb".into() });
    let p = inspect(&r);
    assert_eq!(p.documents.len(), 3);
    assert!(f.path("assets/models/门.glb").exists());
    r.references = Some(ReferenceUpdate::Confirm(p));
    let (receipt, result) = run(r, false);
    result.unwrap();
    assert_eq!(receipt.repaired_documents, 3);
    let glb = fs::read(f.path("assets/scenes/a/scene.glb")).unwrap();
    assert!(glb.ends_with(b"TESTsame"));
    let (raw, ..) = feathrise_editor_assets::glb_metadata::json_prefix(&f.path("assets/scenes/a/scene.glb"), u64::MAX, None).unwrap();
    let outer: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    let fields: serde_json::Value = serde_json::from_str(outer["nodes"][1]["extras"]["feathrise_placement"].as_str().unwrap()).unwrap();
    assert_eq!(fields["asset_path"], "models/门楼.glb");
    assert_eq!(fields["water_surfaces"][0]["node_path"][0], "models/门.glb");
    assert_eq!(fs::read_to_string(f.path("assets/scenes/b/placements.scene_placements.json")).unwrap(), r#"{"placements":[{"asset_path":"models/门楼.glb"}]}"#);
    assert_eq!(fs::read_to_string(f.path("assets/config/example.building_presets.json")).unwrap(),
        "\u{feff}{ \"asset_path\" : \"models/门楼.glb#Scene0\", \"label\": \"models/门.glb\" }\r\n");
    assert_eq!(fs::read(f.path("assets/models/门楼.glb")).unwrap(), b"model");
}

#[test]
fn reference_move_directory_uses_actual_conflict_name_and_preserves_external_changes() {
    let f = Fixture::new();
    prepare(&f);
    fs::create_dir(f.path("assets/out/models")).unwrap();
    let mut r = request(&f, FileOperation::Move { sources: vec![f.path("assets/models")].into(), destination: f.path("assets/out"), conflict: ConflictPolicy::KeepBoth });
    let p = inspect(&r);
    let old = f.path("assets/config/example.building_presets.json");
    let changed = "{\"asset_path\":\"models/门.glb\",\"label\":\"作者外部修改\"}";
    fs::write(&old, changed).unwrap();
    r.references = Some(ReferenceUpdate::Confirm(p));
    assert!(run(r.clone(), false).1.is_err());
    assert_eq!(fs::read_to_string(&old).unwrap(), changed);
    assert!(f.path("assets/models/门.glb").exists());
    r.references = Some(ReferenceUpdate::Inspect(f.path("assets")));
    r.references = Some(ReferenceUpdate::Confirm(inspect(&r)));
    run(r, false).1.unwrap();
    assert!(f.path("assets/out/models (2)/门.glb").exists());
    assert!(fs::read_to_string(old).unwrap().contains("out/models (2)/门.glb"));
}

#[test]
fn reference_move_rejects_replaced_source_and_requires_a_separate_files_only_choice_for_unrepresentable_paths() {
    let f = Fixture::new();
    prepare(&f);
    let source = f.path("assets/models/门.glb");
    let mut r = request(&f, FileOperation::Rename { source: source.clone(), stamp: FileEntryStamp::Untracked, name: "门#楼.glb".into() });
    let plan = inspect(&r);
    assert!(plan.unrepresentable);
    r.references = Some(ReferenceUpdate::Confirm(plan.clone()));
    assert!(run(r.clone(), false).1.is_err());
    fs::write(&source, b"replacement model").unwrap();
    r.references = Some(ReferenceUpdate::FilesOnly(plan));
    assert!(run(r.clone(), false).1.is_err());
    r.references = Some(ReferenceUpdate::Inspect(f.path("assets")));
    r.references = Some(ReferenceUpdate::FilesOnly(inspect(&r)));
    let (receipt, result) = run(r, false);
    result.unwrap();
    assert_eq!(receipt.repaired_documents, 0);
    assert!(f.path("assets/models/门#楼.glb").is_file());
    assert!(fs::read_to_string(f.path("assets/scenes/b/placements.scene_placements.json")).unwrap().contains("models/门.glb"));
}
