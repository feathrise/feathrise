//! 主场景 GLB 变换动画的作者投影，采样与标准通道使用原 GLB 动画合同。

use bevy::{gltf::gltf::binary::Glb, math::{EulerRot, Quat, Vec3}};
use feathrise_editor_animation_model::data::{AnimationDocument, Channel, Clip, Keyframe};
use feathrise_scene_content::{ScenePlacementsAsset, authoring_io::bytes_hash};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::{BTreeMap, HashSet}, path::Path, sync::Arc};

const OWNER: &str = "feathrise_scene_animation";
const SCENE_ID: &str = "feathrise_scene_id";
const DURATION: &str = "feathrise_duration";
const PLAYBACK: &str = "feathrise_playback";
const AUTO_PLAY: &str = "feathrise_autoplay";
const COLLIDER: &str = "feathrise_motion_collider";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SceneMotionPlayback {
    #[default]
    Once,
    Loop,
    PingPong,
}

impl SceneMotionPlayback {
    pub(crate) fn time(self, elapsed: f64, duration: f32) -> f64 {
        let duration = f64::from(duration);
        match self {
            Self::Once => elapsed.clamp(0.0, duration),
            Self::Loop => elapsed.rem_euclid(duration),
            Self::PingPong => duration - (elapsed.rem_euclid(2.0 * duration) - duration).abs(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneMotionTiming {
    pub(crate) duration_seconds: f32,
    pub(crate) playback: SceneMotionPlayback,
    /// 进入工程预览或游戏后是否开始。编辑时间轴的播放始终由作者控制。
    pub(crate) auto_play: bool,
}

impl Default for SceneMotionTiming {
    fn default() -> Self {
        Self { duration_seconds: 2.0, playback: SceneMotionPlayback::Once, auto_play: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SceneMotionKey {
    pub time: f32,
    pub value: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneMotionCollider {
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SceneMotionTrack {
    pub placement_id: String,
    pub duration_seconds: f32,
    pub playback: SceneMotionPlayback,
    pub position: Vec<SceneMotionKey>,
    /// 面板使用 XYZ 角度。GLB 关键帧和旋转插值使用标准单位四元数。
    pub rotation: Vec<SceneMotionKey>,
    pub collider: Option<SceneMotionCollider>,
}

impl SceneMotionTrack {
    pub(crate) fn insert_key(&mut self, rotation: bool, key: SceneMotionKey) {
        let keys = if rotation { &mut self.rotation } else { &mut self.position };
        let index = keys.partition_point(|existing| existing.time < key.time);
        if let Some(existing) = keys.get_mut(index).filter(|existing| existing.time == key.time) {
            *existing = key;
        } else {
            keys.insert(index, key);
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.placement_id.is_empty() || !self.duration_seconds.is_finite() || self.duration_seconds <= 0.0
            || [self.position.as_slice(), self.rotation.as_slice()].into_iter().any(|keys| {
                keys.iter().any(|key| !key.time.is_finite() || key.time < 0.0
                    || !Vec3::from_array(key.value).is_finite())
                    || keys.windows(2).any(|keys| keys[0].time >= keys[1].time)
            })
            || self.collider.is_some_and(|collider| {
                !Vec3::from_array(collider.center).is_finite()
                    || !(Vec3::from_array(collider.half_extents) * 2.0).is_finite()
                    || collider.half_extents.iter().any(|&half| half <= 0.0)
            })
        {
            return Err("动画关键帧或碰撞数据无效".into());
        }
        Ok(())
    }

    pub(crate) fn sample(&self, time: f64, base_position: [f32; 3], base_rotation: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        let position = segment(&self.position, time as f32).map_or(base_position, |(left, right, t)| {
            std::array::from_fn(|i| {
                let start = f64::from(left[i]);
                (start + (f64::from(right[i]) - start) * f64::from(t)) as f32
            })
        });
        let rotation = segment(&self.rotation, time as f32).map_or(base_rotation, |(left, right, t)| {
            if t == 0.0 { left } else { degrees(quat(left).slerp(quat(right), t)) }
        });
        (position, rotation)
    }
}

fn segment(keys: &[SceneMotionKey], time: f32) -> Option<([f32; 3], [f32; 3], f32)> {
    let first = keys.first()?;
    let upper = keys.partition_point(|key| key.time <= time);
    if upper == 0 { return Some((first.value, first.value, 0.0)); }
    let previous = keys[upper - 1];
    Some(keys.get(upper).map_or((previous.value, previous.value, 0.0), |next|
        (previous.value, next.value, (time - previous.time) / (next.time - previous.time))))
}

fn quat([x, y, z]: [f32; 3]) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x.to_radians(), y.to_radians(), z.to_radians()).normalize()
}

fn degrees(rotation: Quat) -> [f32; 3] {
    let (x, y, z) = rotation.to_euler(EulerRot::XYZ);
    [x.to_degrees(), y.to_degrees(), z.to_degrees()]
}

#[derive(Clone)]
pub(crate) struct SceneMotionDocument {
    tracks: Arc<BTreeMap<String, SceneMotionTrack>>,
    // 目录跟随已打开和已保存的 GLB 修订。保存快照和 UI 只共享名称。
    clip_names: Arc<[String]>,
    timing: SceneMotionTiming,
    available: bool,
    pub(crate) dirty: bool,
}

impl Default for SceneMotionDocument {
    fn default() -> Self {
        Self {
            tracks: Default::default(),
            clip_names: Default::default(),
            timing: SceneMotionTiming::default(),
            available: false,
            dirty: false,
        }
    }
}

impl SceneMotionDocument {
    pub(crate) fn from_glb(path: &Path, bytes: Vec<u8>, placements: &ScenePlacementsAsset) -> Result<Self, String> {
        let glb = AnimationDocument::from_glb_bytes(path, bytes)?;
        let mut result = Self {
            available: true,
            clip_names: glb.inspect_glb(|json, _| read_clip_names(json)),
            ..Self::default()
        };
        let ids: HashSet<_> = placements.placements.iter().map(|record| record.id.as_str()).collect();
        let nodes = placement_nodes(&glb, &ids);
        let Some(clip) = active_clip(&glb, &nodes) else { return Ok(result); };
        let duration = glb.clip_extra(clip.source_index, DURATION).and_then(Value::as_f64)
            .map(|value| value as f32).filter(|value| value.is_finite() && *value > 0.0)
            .unwrap_or(0.0).max(clip.duration()).max(1.0 / 30.0);
        let playback = glb.clip_extra(clip.source_index, PLAYBACK).cloned()
            .and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default();
        let auto_play = glb.clip_extra(clip.source_index, AUTO_PLAY).and_then(Value::as_bool).unwrap_or_default();
        result.timing = SceneMotionTiming { duration_seconds: duration, playback, auto_play };
        let tracks = Arc::make_mut(&mut result.tracks);
        for channel in &clip.channels {
            let Some(id) = nodes.get(&channel.node).filter(|_| matches!(channel.path.as_str(), "translation" | "rotation"))
                else { continue; };
            let track = tracks.entry(id.clone()).or_insert_with(|| SceneMotionTrack {
                placement_id: id.clone(), duration_seconds: duration, playback, position: Vec::new(), rotation: Vec::new(),
                collider: glb.node_extra(channel.node, COLLIDER).cloned().and_then(|value| serde_json::from_value(value).ok()),
            });
            let keys = project_channel(channel);
            if channel.path == "rotation" { track.rotation = keys; } else { track.position = keys; }
        }
        Ok(result)
    }

    pub(crate) fn available(&self) -> bool { self.available }
    pub(crate) fn clip_names(&self) -> &[String] { &self.clip_names }
    pub(crate) fn timing(&self) -> SceneMotionTiming { self.timing }

    /// 摆放删除可能让整个片段失去通道。从正式候选的 JSON 取名，不复制 BIN 或解码关键帧。
    pub(crate) fn with_saved_clip_names(&self, bytes: &[u8]) -> Result<Option<Self>, String> {
        let glb = Glb::from_slice(bytes).map_err(|error| error.to_string())?;
        let json = serde_json::from_slice(&glb.json).map_err(|error| error.to_string())?;
        let clip_names = read_clip_names(&json);
        Ok((clip_names != self.clip_names).then(|| Self { clip_names, ..self.clone() }))
    }

    pub(crate) fn set_timing(&mut self, timing: SceneMotionTiming) {
        self.timing = timing;
        self.dirty = true;
    }

    pub(crate) fn get(&self, id: &str) -> Option<&SceneMotionTrack> { self.tracks.get(id) }
    pub(crate) fn tracks(&self) -> impl Iterator<Item = &SceneMotionTrack> { self.tracks.values() }

    pub(crate) fn replace(&mut self, id: &str, track: Option<SceneMotionTrack>) {
        let tracks = Arc::make_mut(&mut self.tracks);
        if let Some(track) = track { tracks.insert(id.to_owned(), track); }
        else { tracks.remove(id); }
        self.dirty = true;
    }

    /// 候选节点来自同一份主 GLB 的摆放身份。模型内容与动画通道在原场景事务中使用同一个输出。
    pub(crate) fn encode_glb(
        &self, path: &Path, bytes: Vec<u8>, placements: &ScenePlacementsAsset,
    ) -> Result<(Vec<u8>, Self), String> {
        let glb = AnimationDocument::from_glb_bytes(path, bytes)?;
        let ids: HashSet<_> = placements.placements.iter().map(|record| record.id.as_str()).collect();
        let nodes = placement_nodes(&glb, &ids);
        let source = active_clip(&glb, &nodes);
        let owned = owned_clip(&glb);
        let mut clip = Clip { name: source.map_or_else(|| "场景动画".into(), |clip| clip.name.clone()),
            source_index: owned.map_or(glb.clips.len(), |clip| clip.source_index), channels: Vec::new() };
        let mut colliders = Vec::new();
        for (&node, id) in &nodes {
            let Some(track) = self.get(id) else { continue; };
            track.validate()?;
            if track.position.is_empty() && track.rotation.is_empty() { continue; }
            colliders.push((node, track.collider));
            for (path, keys) in [("translation", &track.position), ("rotation", &track.rotation)] {
                if keys.is_empty() { continue; }
                if let Some(channel) = source.and_then(|clip| clip.channels.iter().find(|c| c.node == node && c.path == path))
                    && project_channel(channel) == *keys
                {
                    clip.push_existing_channel(channel.clone())?;
                } else {
                    let encoded = keys.iter().map(|key| Keyframe {
                        time: key.time,
                        value: if path == "rotation" { quat(key.value).to_array().to_vec() } else { key.value.to_vec() },
                        in_tangent: Vec::new(), out_tangent: Vec::new(),
                    }).collect();
                    clip.push_transform_channel(node, &glb.nodes[node].name, path, encoded)?;
                }
            }
        }
        let SceneMotionTiming { duration_seconds: duration, playback, auto_play } = self.timing;
        if !duration.is_finite() || duration <= 0.0 || duration + 1e-6 < clip.duration() {
            return Err("场景动画结束帧必须容纳所有关键帧".into());
        }
        // Blender 已改写的网格使用真实几何。只有源摘要本来有效，才能为追加动画后重签。
        let retain_geometry = glb.inspect_glb(|json, bin| {
            json["nodes"].as_array().and_then(|nodes| nodes.iter()
                .find(|node| node["extras"][SCENE_ID].is_string()))
                .and_then(|node| node["extras"]["feathrise_geometry_hash"].as_str())
                .is_some_and(|hash| geometry_hash(json, bin) == hash)
        });
        let mut saved = self.clone();
        let bytes = glb.encode_owned_clip(owned.map(|clip| clip.source_index), &clip,
            json!({(OWNER): true, (DURATION): duration, (PLAYBACK): playback, (AUTO_PLAY): auto_play}),
            |json, bin| {
                for (node, collider) in colliders {
                    if let Some(extras) = json["nodes"][node]["extras"].as_object_mut() {
                        if let Some(collider) = collider {
                            extras.insert(COLLIDER.into(), serde_json::to_value(collider).map_err(|error| error.to_string())?);
                        } else { extras.remove(COLLIDER); }
                    }
                }
                if retain_geometry { refresh_geometry_hash(json, bin); }
                if let Some(root) = json["nodes"].as_array_mut().and_then(|nodes| nodes.iter_mut()
                    .find(|node| node["extras"][SCENE_ID].is_string()))
                {
                    root["extras"][OWNER] = json!(true);
                }
                saved.clip_names = read_clip_names(json);
                Ok(())
            })?;
        Arc::make_mut(&mut saved.tracks).retain(|id, _| ids.contains(id.as_str()));
        saved.dirty = false;
        Ok((bytes, saved))
    }
}

fn read_clip_names(json: &Value) -> Arc<[String]> {
    let Some(clips) = json["animations"].as_array() else { return Arc::default(); };
    clips.iter().enumerate().map(|(index, clip)| {
        clip["name"]
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .map_or_else(|| format!("动画 {}", index + 1), str::to_owned)
    }).collect()
}

fn placement_nodes(glb: &AnimationDocument, ids: &HashSet<&str>) -> BTreeMap<usize, String> {
    glb.nodes.iter().enumerate().filter_map(|(index, node)| {
        let id = glb.node_extra(index, "feathrise_id").and_then(Value::as_str)
            .map(str::to_owned).unwrap_or_else(|| format!("glb-{}", bytes_hash(node.name.as_bytes())));
        ids.contains(id.as_str()).then_some((index, id))
    }).collect()
}

fn owned_clip(glb: &AnimationDocument) -> Option<&Clip> {
    glb.clips.iter().find(|clip| glb.clip_extra(clip.source_index, OWNER).and_then(Value::as_bool) == Some(true))
}

fn active_clip<'a>(glb: &'a AnimationDocument, nodes: &BTreeMap<usize, String>) -> Option<&'a Clip> {
    let owned = owned_clip(glb);
    // 删除最后一键后仍以场景根标记为准，重开时保留空的时间轴。
    if owned.is_some() || (0..glb.nodes.len()).any(|node| {
        glb.node_extra(node, SCENE_ID).and_then(Value::as_str).is_some()
            && glb.node_extra(node, OWNER).and_then(Value::as_bool) == Some(true)
    }) {
        return owned;
    }
    glb.clips.iter().find(|clip| clip.channels.iter().any(|channel|
        nodes.contains_key(&channel.node) && matches!(channel.path.as_str(), "translation" | "rotation")))
}

fn project_channel(channel: &Channel) -> Vec<SceneMotionKey> {
    channel.keys.iter().filter_map(|key| {
        let value = if channel.path == "rotation" {
            degrees(Quat::from_array(key.value.as_slice().try_into().ok()?).normalize())
        } else { key.value.as_slice().try_into().ok()? };
        Some(SceneMotionKey { time: key.time, value })
    }).collect()
}

fn geometry_hash(json: &Value, bin: &[u8]) -> String {
    let mut hash = Sha256::new();
    // SDK 用完整缓冲校验参数化网格。动画追加缓冲后同步更新同一个摘要。
    for field in ["buffers", "bufferViews", "accessors", "meshes", "materials", "textures", "images"] {
        hash.update(json[field].to_string());
    }
    hash.update(bin);
    hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

fn refresh_geometry_hash(json: &mut Value, bin: &[u8]) {
    let digest = geometry_hash(json, bin);
    if let Some(marker) = json["nodes"].as_array_mut().and_then(|nodes| nodes.iter_mut()
        .find(|node| node["extras"][SCENE_ID].is_string()))
    {
        marker["extras"]["feathrise_geometry_hash"] = json!(digest);
    }
}
