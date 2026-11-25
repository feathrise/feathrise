//! 通过原 GLB 编码器验收多个动画片段、草稿、历史和未知片段。

use super::*;
use bevy::gltf::gltf::binary::Glb;
use feathrise_scene_content::write_scene_placements_glb;

const PATH: &str = "scenes/TEST/scene.glb";

#[test]
fn region_survives_empty_and_keyed_clip_saves_and_stays_on_its_original_clip() {
    let path = Path::new(PATH);
    let (placements, bytes) = scene();
    let mut motion = SceneMotionDocument::from_glb(path, bytes.clone(), &placements).unwrap();
    motion.create("城门").unwrap();
    let clip = motion.active_clip_id().unwrap().to_owned();
    let area = MotionRegion { center: Vec3::new(3.0, 1.0, -2.0), half_extents: Vec3::new(2.0, 1.0, 0.5),
        enter: MotionRegionAction::Play, exit: MotionRegionAction::Reverse };
    motion.change_region(&clip, 0, None, Some(area)).unwrap();
    let (saved, _) = motion.encode_glb(path, bytes, &placements, false).unwrap();
    let mut motion = SceneMotionDocument::from_glb(path, saved.clone(), &placements).unwrap();
    assert_eq!(motion.regions(&clip), Some([area].as_slice()));
    let target = &placements.placements[0].id;
    motion.select_clip(&clip);
    motion.replace(target,Some(SceneMotionTrack { placement_id: target.clone(), duration_seconds: 2.0,
        playback: SceneMotionPlayback::Once, position: vec![SceneMotionKey { time: 0.0, value: [0.0;3] }],
        rotation: Vec::new(),scale: Vec::new(),collider: None }));
    motion.duplicate(&clip).unwrap();
    let copy = motion.active_clip_id().unwrap().to_owned();
    let (saved, _) = motion.encode_glb(path, saved, &placements, false).unwrap();
    let mut motion = SceneMotionDocument::from_glb(path, saved, &placements).unwrap();
    assert_eq!(motion.regions(&copy),Some([].as_slice()));
    assert_eq!(motion.regions(&clip),Some([area].as_slice()));
    motion.change_region(&clip,0,Some(area),None).unwrap();
    motion.change_region(&clip,0,None,Some(area)).unwrap();
    assert_eq!(motion.regions(&clip),Some([area].as_slice()));
}

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
    motion.create("空草稿").unwrap();
    let clip_id = motion.active_clip_id().unwrap().to_owned();
    motion.set_timing(SceneMotionTiming { auto_play: true, ..Default::default() });
    let (bytes, mut motion) = motion.encode_glb(path, bytes, &placements, false).unwrap();

    let removed = motion.remove(&clip_id).unwrap();
    assert!(motion.clip_name().is_none());
    motion.apply_clip_history(&removed, true).unwrap();
    assert_eq!(motion.clip_name(), Some("空草稿"));
    assert!(motion.timing().auto_play);
    motion.apply_clip_history(&removed, false).unwrap();
    let (bytes, _) = motion.encode_glb(path, bytes, &placements, false).unwrap();
    let reopened = SceneMotionDocument::from_glb(path, bytes, &placements).unwrap();
    assert!(reopened.clip_name().is_none());
    assert_eq!(reopened.clip_count(), 0);
    assert_eq!(reopened.timing(), SceneMotionTiming::default());
}

#[test]
fn deleting_saved_scene_motion_preserves_the_other_glb_animation() {
    let path = Path::new(PATH);
    let (placements, bytes) = scene();
    let id = &placements.placements[0].id;
    let mut motion = SceneMotionDocument::from_glb(path, bytes.clone(), &placements).unwrap();
    motion.create("待删除片段").unwrap();
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
    let clip_id = motion.active_clip_id().unwrap().to_owned();
    let removed = motion.remove(&clip_id).unwrap();
    assert_eq!(motion.clip_count(), 0);
    assert!(motion.get(id).is_none());
    motion.apply_clip_history(&removed, true).unwrap();
    assert_eq!(motion.active_clip_id(), Some(clip_id.as_str()));
    assert_eq!(motion.get(id), Some(&track));
    motion.apply_clip_history(&removed, false).unwrap();
    let (saved, published) = motion.encode_glb(path, bytes.clone(), &placements, false).unwrap();
    assert_eq!(published.foreign_clip_count(), 1);
    assert_eq!(published.foreign_clip_at(0), Some("保留片段"));

    let saved_glb = Glb::from_slice(&saved).unwrap();
    let original_glb = Glb::from_slice(&bytes).unwrap();
    let saved_json: Value = serde_json::from_slice(&saved_glb.json).unwrap();
    let original_json: Value = serde_json::from_slice(&original_glb.json).unwrap();
    assert_eq!(saved_glb.bin, original_glb.bin);
    assert_eq!(saved_json["animations"], json!([original_json["animations"][1]]));
    let reopened = SceneMotionDocument::from_glb(path, saved, &placements).unwrap();
    assert!(reopened.has_saved_animation());
    assert!(reopened.clip_name().is_none());
    assert_eq!(reopened.foreign_clip_at(0), Some("保留片段"));
}

#[test]
fn multiple_scene_clips_keep_identity_tracks_and_empty_drafts_through_save_and_rename() {
    let path = Path::new(PATH);
    let (placements, bytes) = scene();
    let target = &placements.placements[0].id;
    let track = |y| SceneMotionTrack {
        placement_id: target.clone(), duration_seconds: 2.0, playback: SceneMotionPlayback::Once,
        position: vec![SceneMotionKey { time: 0.0, value: [0.0, y, 0.0] }],
        rotation: Vec::new(), scale: Vec::new(), collider: None,
    };
    let mut motion = SceneMotionDocument::from_glb(path, bytes.clone(), &placements).unwrap();
    motion.create("开门").unwrap();
    let opening = motion.active_clip_id().unwrap().to_owned();
    motion.replace(target, Some(track(5.0)));
    motion.create("关门").unwrap();
    let closing = motion.active_clip_id().unwrap().to_owned();
    motion.replace(target, Some(track(0.0)));
    motion.create("待机").unwrap();
    let idle = motion.active_clip_id().unwrap().to_owned();
    motion.create("停靠").unwrap();
    let stopped = motion.active_clip_id().unwrap().to_owned();
    assert!(motion.get(target).is_none());
    assert!(motion.has_track(target));
    let (bytes, _) = motion.encode_glb(path, bytes, &placements, false).unwrap();

    let mut reopened = SceneMotionDocument::from_glb(path, bytes.clone(), &placements).unwrap();
    assert_eq!(reopened.clip_count(), 4);
    assert!(reopened.select_clip(&closing));
    assert!(!reopened.dirty, "切换当前时间轴无需重写 GLB");
    assert_eq!(reopened.get(target), Some(&track(0.0)));
    reopened.rename(&opening, "开启城门").unwrap();
    assert_eq!(reopened.active_clip_id(), Some(closing.as_str()));
    reopened.duplicate(&opening).unwrap();
    let copied = reopened.active_clip_id().unwrap().to_owned();
    assert_ne!(opening, copied);
    let (saved, _) = reopened.encode_glb(path, bytes.clone(), &placements, false).unwrap();
    let source = Glb::from_slice(&bytes).unwrap();
    let output = Glb::from_slice(&saved).unwrap();
    assert_eq!(source.bin, output.bin, "改名与复制可复用原曲线缓冲");
    let result = SceneMotionDocument::from_glb(path, saved, &placements).unwrap();
    assert_eq!(result.clip_name_for(&opening), Some("开启城门"));
    assert_eq!(result.clip_name_for(&copied), Some("开启城门 副本"));
    assert_eq!(result.get_for(&opening, target), Some(&track(5.0)));
    assert_eq!(result.get_for(&copied, target), Some(&track(5.0)));
    assert_eq!(result.get_for(&closing, target), Some(&track(0.0)));
    assert_eq!(result.clip_name_for(&idle), Some("待机"));
    assert_eq!(result.clip_name_for(&stopped), Some("停靠"));
}

#[test]
fn scene_clip_overview_indexes_only_keyed_tracks_across_history_and_reopen() {
    let path = Path::new(PATH);
    let (mut placements, _) = scene();
    let template = placements.placements[0].clone();
    placements.placements = ["overview-z", "overview-a", "overview-m"].map(|id| {
        let mut record = template.clone();
        record.id = id.into();
        record
    }).into();
    let bytes = write_scene_placements_glb(&placements, PATH, None).unwrap();
    let mut motion = SceneMotionDocument::from_glb(path, bytes.clone(), &placements).unwrap();
    let keyed = |motion: &SceneMotionDocument| {
        (0..motion.keyed_track_count())
            .map(|index| motion.keyed_track_at(index).unwrap().placement_id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(motion.keyed_track_count(), 0);
    assert!(motion.keyed_track_at(0).is_none());
    motion.create("总览").unwrap();
    let first = motion.active_clip_id().unwrap().to_owned();

    let track = |id: &str| SceneMotionTrack {
        placement_id: id.into(), duration_seconds: 2.0, playback: SceneMotionPlayback::Once,
        position: vec![SceneMotionKey { time: 0.0, value: [1.0, 2.0, 3.0] }],
        rotation: Vec::new(), scale: Vec::new(), collider: None,
    };
    motion.replace("overview-z", Some(track("overview-z")));
    motion.replace("overview-a", Some(track("overview-a")));
    motion.replace("overview-m", Some(SceneMotionTrack { position: Vec::new(), ..track("overview-m") }));
    assert_eq!(keyed(&motion), ["overview-a", "overview-z"]);
    assert!(motion.keyed_track_at(2).is_none());

    motion.duplicate(&first).unwrap();
    let copy = motion.active_clip_id().unwrap().to_owned();
    motion.replace("overview-a", None);
    assert_eq!(keyed(&motion), ["overview-z"]);
    assert!(motion.get_for(&first, "overview-a").is_some());
    assert!(std::ptr::eq(
        motion.get_for(&first, "overview-z").unwrap(),
        motion.keyed_track_at(0).unwrap(),
    ), "片段历史共享未改的轨道");
    let removed = motion.remove(&copy).unwrap();
    motion.apply_clip_history(&removed, true).unwrap();
    assert!(motion.select_clip(&copy));
    assert_eq!(keyed(&motion), ["overview-z"]);

    let (saved, _) = motion.encode_glb(path, bytes, &placements, false).unwrap();
    let mut reopened = SceneMotionDocument::from_glb(path, saved, &placements).unwrap();
    assert!(reopened.select_clip(&first));
    assert_eq!(keyed(&reopened), ["overview-a", "overview-z"]);
    assert!(reopened.select_clip(&copy));
    assert_eq!(keyed(&reopened), ["overview-z"]);
}
