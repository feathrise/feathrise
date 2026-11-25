//! 主场景 GLB 变换动画的作者投影，采样与标准通道使用原 GLB 动画合同。

use bevy::math::{EulerRot, Quat, Vec3};
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
const TRIGGERS: &str = "feathrise_motion_triggers";
const COLLIDER: &str = "feathrise_motion_collider";
pub(crate) const DEFAULT_SCENE_ANIMATION_NAME: &str = "场景动画";
pub(crate) const SCENE_ANIMATION_DRAFT_EXTRA: &str = "feathrise_scene_animation_draft";

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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneMotionTiming {
    pub(crate) duration_seconds: f32,
    pub(crate) playback: SceneMotionPlayback,
    /// 进入工程预览或游戏后是否开始。编辑时间轴的播放始终由作者控制。
    pub(crate) auto_play: bool,
}

#[derive(Serialize, Deserialize)]
struct EmptySceneAnimation {
    #[serde(default)]
    id: Option<String>,
    name: String,
    #[serde(flatten)]
    timing: SceneMotionTiming,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    regions: Vec<MotionRegion>,
}

/// 作者数据以主场景的局部米保存，进入与离开分别选择运行时播放指令。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MotionRegion {
    pub(crate) center: Vec3,
    pub(crate) half_extents: Vec3,
    pub(crate) enter: MotionRegionAction,
    pub(crate) exit: MotionRegionAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotionRegionAction { None, Play, Reverse, Pause, Stop }

impl MotionRegion {
    pub(crate) fn valid(self) -> bool {
        self.half_extents.is_finite() && self.half_extents.min_element() > 0.0
            && (self.center - self.half_extents).is_finite() && (self.center + self.half_extents).is_finite()
    }
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
    pub scale: Vec<SceneMotionKey>,
    pub collider: Option<SceneMotionCollider>,
}

impl SceneMotionTrack {
    pub(crate) fn insert_key(keys: &mut Vec<SceneMotionKey>, key: SceneMotionKey) {
        let index = keys.partition_point(|existing| existing.time < key.time);
        if let Some(existing) = keys.get_mut(index).filter(|existing| existing.time == key.time) {
            *existing = key;
        } else {
            keys.insert(index, key);
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.position.is_empty() && self.rotation.is_empty() && self.scale.is_empty()
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.placement_id.is_empty() || !self.duration_seconds.is_finite() || self.duration_seconds <= 0.0
            || [&self.position, &self.rotation, &self.scale].into_iter().any(|keys| {
                keys.iter().any(|key| !key.time.is_finite() || key.time < 0.0
                    || !Vec3::from_array(key.value).is_finite())
                    || keys.windows(2).any(|keys| keys[0].time >= keys[1].time)
            })
            || self.scale.iter().any(|key| key.value.into_iter().any(|value| !(0.001..=1_000.0).contains(&value)))
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

    pub(crate) fn sample(&self, time: f64, base_position: [f32; 3], base_rotation: [f32; 3], base_scale: [f32; 3])
        -> ([f32; 3], [f32; 3], [f32; 3])
    {
        let lerp = |keys: &[SceneMotionKey], base| segment(keys, time as f32).map_or(base, |(left, right, t)| {
            std::array::from_fn(|i| {
                let start = f64::from(left[i]);
                (start + (f64::from(right[i]) - start) * f64::from(t)) as f32
            })
        });
        let rotation = segment(&self.rotation, time as f32).map_or(base_rotation, |(left, right, t)| {
            if t == 0.0 { left } else { degrees(quat(left).slerp(quat(right), t)) }
        });
        (lerp(&self.position, base_position), rotation, lerp(&self.scale, base_scale))
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

mod document;
pub(crate) use document::SceneMotionDocument;
pub use document::SceneMotionClipChange;

#[derive(Clone, PartialEq, Eq)]
struct EditableClip {
    source_index: usize,
    name: Option<Arc<str>>,
    owned: bool,
    samplers: [u8; 32],
    channels: Arc<[(Option<String>, [u8; 32])]>,
}

impl EditableClip {
    fn read<'a>(json: &Value, source_index: usize, placement: impl Fn(usize) -> Option<&'a str>) -> Self {
        let clip = &json["animations"][source_index];
        let channels = clip["channels"].as_array().into_iter().flatten().filter_map(|channel| {
            // 摆放写入只重绑节点索引并移除失效通道。用稳定 ID 和原采样器校验仍是同一片段。
            let mut normalized = channel.clone();
            let id = match normalized.get_mut("target")?.as_object_mut()?.remove("node") {
                Some(node) => Some(placement(usize::try_from(node.as_u64()?).ok()?)?.to_owned()),
                None => None,
            };
            Some((id, Sha256::digest(normalized.to_string()).into()))
        }).collect();
        Self {
            source_index,
            name: clip["name"].as_str().map(Arc::from),
            owned: clip["extras"][OWNER].as_bool() == Some(true),
            samplers: Sha256::digest(clip["samplers"].to_string()).into(),
            channels,
        }
    }

    fn rebind<'a>(
        &self, json: &Value, placement: impl Fn(usize) -> Option<&'a str> + Copy, ids: &HashSet<&str>,
    ) -> Option<Self> {
        let kept = self.channels.iter().filter(|(id, _)| id.as_deref().is_none_or(|id| ids.contains(id)));
        if kept.clone().next().is_none() { return None; }
        let clips = json["animations"].as_array()?;
        clips.iter().enumerate()
            .filter(|(_, entry)| entry["name"].as_str() == self.name.as_deref())
            .map(|(index, _)| Self::read(json, index, placement)).find(|candidate| {
                candidate.owned == self.owned && candidate.samplers == self.samplers
                    && candidate.channels.iter().eq(kept.clone())
            })
    }
}

fn scene_marker(json: &Value, scene_id: &str) -> Option<usize> {
    let default = usize::try_from(json["scene"].as_u64().unwrap_or(0)).ok()?;
    json["scenes"][default]["nodes"].as_array()?.iter()
        .filter_map(|index| usize::try_from(index.as_u64()?).ok())
        .find(|&index| json["nodes"][index]["extras"][SCENE_ID].as_str() == Some(scene_id))
}

fn read_empty_clip(json: &Value, scene_id: &str) -> Option<EmptySceneAnimation> {
    let entry = json["nodes"].get(scene_marker(json, scene_id)?)?
        .get("extras")?.get(SCENE_ANIMATION_DRAFT_EXTRA)?;
    let empty = EmptySceneAnimation::deserialize(entry).ok()?;
    let duration = empty.timing.duration_seconds;
    (!empty.name.trim().is_empty() && duration.is_finite() && duration > 0.0).then_some(empty)
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
        nodes.contains_key(&channel.node) && matches!(channel.path.as_str(), "translation" | "rotation" | "scale")))
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

fn refresh_geometry_hash(json: &mut Value, bin: &[u8], marker: usize) {
    let digest = geometry_hash(json, bin);
    json["nodes"][marker]["extras"]["feathrise_geometry_hash"] = json!(digest);
}

#[cfg(test)]
mod tests;
