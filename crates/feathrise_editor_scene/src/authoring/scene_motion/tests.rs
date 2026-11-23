//! 删除主场景动画通过原 GLB 编码器验收草稿、历史和标准片段。

use super::*;
use feathrise_scene_content::write_scene_placements_glb;

const PATH: &str = "scenes/TEST/scene.glb";

fn scene() -> (ScenePlacementsAsset, Vec<u8>) {
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    let mut record = crate::viewport::placement_move::interaction_tests::record();
    record.water_surfaces.clear();
    draft.placements.placements.push(record);
    let bytes = write_scene_placements_glb(&draft.placements, PATH, None).unwrap();
    (draft.placements, bytes)
}

#[test]
fn deleting_named_scene_motion_draft_removes_its_saved_metadata() {
    let path = Path::new(PATH);
    let (placements, bytes) = scene();
    let mut motion = SceneMotionDocument::from_glb(path, bytes.clone(), &placements).unwrap();
    motion.set_clip_name(Some("空草稿".into()));
    motion.set_timing(SceneMotionTiming { auto_play: true, ..Default::default() });
    let (bytes, mut motion) = motion.encode_glb(path, bytes, &placements, false).unwrap();

    let removed = motion.remove_clip().unwrap();
    assert!(motion.clip_name().is_none());
    motion.undo_clip_removal(&removed).unwrap();
    assert_eq!(motion.clip_name(), Some("空草稿"));
    assert!(motion.timing().auto_play);
    motion.redo_clip_removal(&removed).unwrap();
    let (bytes, _) = motion.encode_glb(path, bytes, &placements, false).unwrap();
    let reopened = SceneMotionDocument::from_glb(path, bytes, &placements).unwrap();
    assert!(reopened.clip_name().is_none());
    assert_eq!(reopened.visible_clip_count(), 0);
    assert_eq!(reopened.timing(), SceneMotionTiming::default());
}

#[test]
fn deleting_saved_scene_motion_preserves_the_other_glb_animation() {
    let path = Path::new(PATH);
    let (placements, bytes) = scene();
    let id = &placements.placements[0].id;
    let mut motion = SceneMotionDocument::from_glb(path, bytes.clone(), &placements).unwrap();
    motion.set_clip_name(Some("待删除片段".into()));
    let track = SceneMotionTrack {
        placement_id: id.clone(),
        duration_seconds: 2.0,
        playback: SceneMotionPlayback::Once,
        position: vec![SceneMotionKey { time: 0.0, value: [1.0, 2.0, 3.0] }],
        rotation: Vec::new(),
        scale: Vec::new(),
        collider: None,
    };
    motion.replace(id, Some(track.clone()));
    let (bytes, _) = motion.encode_glb(path, bytes, &placements, false).unwrap();
    let standard = AnimationDocument::from_glb_bytes(path, bytes).unwrap();
    let empty = Clip { name: String::new(), source_index: 0, channels: Vec::new() };
    let bytes = standard.encode_owned_clip(None, &empty, Value::Null, |json, _| {
        // 没有所有权标记的旧场景打开第一个可编辑片段，第二个虽共用访问器仍需完整保留。
        json["nodes"][0]["extras"].as_object_mut().unwrap().remove(OWNER);
        json["animations"][0]["extras"].as_object_mut().unwrap().remove(OWNER);
        let mut other = json["animations"][0].clone();
        other["name"] = json!("保留片段");
        json["animations"].as_array_mut().unwrap().push(other);
        Ok(())
    }).unwrap();
    let mut motion = SceneMotionDocument::from_glb(path, bytes.clone(), &placements).unwrap();
    assert_eq!(motion.editable_clip_index(), Some(0));
    let removed = motion.remove_clip().unwrap();
    assert_eq!(motion.visible_clip_count(), 1);
    assert_eq!(motion.visible_clip(0), Some((1, "保留片段")));
    assert!(motion.get(id).is_none());
    motion.undo_clip_removal(&removed).unwrap();
    assert_eq!(motion.editable_clip_index(), Some(0));
    assert_eq!(motion.get(id), Some(&track));
    motion.redo_clip_removal(&removed).unwrap();
    let (saved, _) = motion.encode_glb(path, bytes.clone(), &placements, false).unwrap();

    let saved_glb = Glb::from_slice(&saved).unwrap();
    let original_glb = Glb::from_slice(&bytes).unwrap();
    let saved_json: Value = serde_json::from_slice(&saved_glb.json).unwrap();
    let original_json: Value = serde_json::from_slice(&original_glb.json).unwrap();
    assert_eq!(saved_glb.bin, original_glb.bin);
    assert_eq!(saved_json["animations"], json!([original_json["animations"][1]]));
    let reopened = SceneMotionDocument::from_glb(path, saved, &placements).unwrap();
    assert_eq!(reopened.clip_names(), ["保留片段"]);
    assert!(reopened.clip_name().is_none());
}
