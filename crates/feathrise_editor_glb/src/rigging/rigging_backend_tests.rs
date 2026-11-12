//! 独占目录夹具核对动作索引、绑骨输入身份及无覆盖发布。

use super::*;
use serde_json::json;
use std::{fs, sync::atomic::Ordering};

fn write_glb(path: &Path, json: &serde_json::Value) {
    let mut payload = serde_json::to_vec(json).unwrap();
    payload.resize(payload.len().next_multiple_of(4), b' ');
    let mut bytes = b"glTF".to_vec();
    bytes.extend(2u32.to_le_bytes());
    bytes.extend((20 + payload.len() as u32).to_le_bytes());
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(b"JSON");
    bytes.extend(payload);
    fs::write(path, bytes).unwrap();
}

#[test]
fn catalog_keeps_glb_clip_ordinals_and_asset_relative_paths() {
    let owned = files::Stage::new(&std::env::temp_dir().join("assets")).unwrap();
    let root = owned.path("assets");
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("动作")).unwrap();
    write_glb(
        &root.join("动作/动作集.GLB"),
        &json!({
            "asset": {"version": "2.0"},
            "animations": [{"name":"待机"}, {}, {"name":"跑步"}]
        }),
    );
    fs::write(root.join("bad.glb"), b"glTF").unwrap();
    let clips = catalog(&root, &AtomicBool::new(false)).unwrap();
    assert_eq!(clips.len(), 1);
    assert_eq!(clips[0].path, "动作/动作集.GLB");
    assert_eq!(clips[0].clips, ["待机", "Animation 2", "跑步"]);
    assert!(!clips[0].can_reference);
}

#[test]
fn result_publishes_only_as_a_new_file_and_inputs_are_frozen() {
    let owned = files::Stage::new(&std::env::temp_dir().join("assets")).unwrap();
    let root = owned.path("assets");
    fs::create_dir(&root).unwrap();
    let root = files::checked_root(&root).unwrap();
    let source = root.join("角色.glb");
    let occupied = root.join("角色_已绑骨.glb");
    let cancel = AtomicBool::new(false);
    write_glb(&source, &json!({"asset":{"version":"2.0"},"meshes":[{}]}));
    fs::write(&occupied, b"existing").unwrap();
    let stage = files::Stage::new(&root).unwrap();
    let candidate = stage.path("rigged.glb");
    let snapshot = files::snapshot(&root, "角色.glb", &candidate, &cancel).unwrap();
    assert!(files::snapshot(&root, "../角色.glb", &stage.path("escape.glb"), &cancel).is_err());
    files::check_frozen(&root, &snapshot, &cancel).unwrap();
    cancel.store(true, Ordering::Release);
    assert!(files::publish(&candidate, &root, &snapshot, &cancel).is_err());
    assert!(!root.join("角色_已绑骨_2.glb").exists());
    cancel.store(false, Ordering::Release);
    let result = files::publish(&candidate, &root, &snapshot, &cancel).unwrap();
    assert_eq!(result, "角色_已绑骨_2.glb");
    assert_eq!(
        fs::read(root.join(result)).unwrap(),
        fs::read(&source).unwrap()
    );
    assert_eq!(fs::read(&occupied).unwrap(), b"existing");
    write_glb(
        &source,
        &json!({"asset":{"version":"2.0"},"meshes":[{},{}]}),
    );
    assert!(files::check_frozen(&root, &snapshot, &cancel).is_err());
}
