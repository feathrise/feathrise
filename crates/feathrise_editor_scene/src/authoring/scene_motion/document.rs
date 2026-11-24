//! 主场景多动画目录、独立草稿和原子 GLB 保存。

use super::*;
use bevy::gltf::gltf::binary::Glb;
use feathrise_editor_animation_model::data::new_scene_clip_id;

const CLIP_ID: &str = "feathrise_clip_id";
const DRAFTS: &str = "feathrise_scene_animation_drafts";
// 轨道按摆放 ID 排序且至少有一个键。总览按下标读取，草稿与片段历史共享未改的轨道。
type Tracks = Arc<Vec<Arc<SceneMotionTrack>>>;

#[derive(Clone)]
struct SceneClip {
    id: String,
    name: String,
    timing: SceneMotionTiming,
    tracks: Tracks,
    /// 复制片段时仍可从原片段保留采样器和未进入作者投影的通道。
    source_id: Option<String>,
    keys_changed: bool,
}

impl SceneClip {
    fn empty(id: String, name: String, timing: SceneMotionTiming) -> Self {
        Self {
            id,
            name,
            timing,
            tracks: Tracks::default(),
            source_id: None,
            keys_changed: false,
        }
    }

    fn find_track(&self, placement: &str) -> Result<usize, usize> {
        self.tracks
            .binary_search_by(|track| track.placement_id.as_str().cmp(placement))
    }

    fn track(&self, placement: &str) -> Option<&SceneMotionTrack> {
        Some(&self.tracks[self.find_track(placement).ok()?])
    }
}

/// 历史共享轨道，不复制整份 GLB；插入和删除保留目录位置。
#[derive(Clone)]
pub struct SceneMotionClipChange {
    clip: SceneClip,
    index: usize,
    added: bool,
}

#[derive(Clone, Default)]
pub(crate) struct SceneMotionDocument {
    clips: Vec<SceneClip>,
    active: Option<usize>,
    foreign_names: Arc<[String]>,
    /// 已读取的文件提供索引和再绑定依据，删除草稿仍须移除对应的原片段。
    original: Arc<BTreeMap<String, EditableClip>>,
    has_saved_animation: bool,
    available: bool,
    pub(crate) dirty: bool,
}

impl SceneMotionDocument {
    pub(crate) fn from_glb(
        path: &Path,
        bytes: Vec<u8>,
        placements: &ScenePlacementsAsset,
    ) -> Result<Self, String> {
        let glb = AnimationDocument::from_glb_bytes(path, bytes)?;
        let ids = placements
            .placements
            .iter()
            .map(|record| record.id.as_str())
            .collect();
        let nodes = placement_nodes(&glb, &ids);
        let mut clips = Vec::new();
        let mut original = BTreeMap::new();
        let owned: Vec<_> = glb
            .clips
            .iter()
            .filter(|clip| {
                glb.clip_extra(clip.source_index, OWNER)
                    .and_then(Value::as_bool)
                    == Some(true)
            })
            .collect();
        // 旧文件保持原来的首个可编辑片段选择。新保存明确标记全部归属片段。
        let candidates = if owned.is_empty() {
            active_clip(&glb, &nodes).into_iter().collect()
        } else {
            owned
        };
        let editable_slots: HashSet<_> = candidates.iter().map(|clip| clip.source_index).collect();
        let foreign_names = glb.inspect_glb(|json, _| {
            foreign_names(&json["animations"], |index, _| {
                editable_slots.contains(&index)
            })
        });
        for clip in candidates {
            let index = clip.source_index;
            let id = glb
                .clip_extra(index, CLIP_ID)
                .and_then(Value::as_str)
                .filter(|id| valid_text(id))
                .map_or_else(|| format!("scene-clip-{index}"), str::to_owned);
            let duration_seconds = glb
                .clip_extra(index, DURATION)
                .and_then(Value::as_f64)
                .map(|value| value as f32)
                .filter(|value| value.is_finite() && *value > 0.0)
                .unwrap_or(0.0)
                .max(clip.duration())
                .max(1.0 / 30.0);
            let playback = glb
                .clip_extra(index, PLAYBACK)
                .cloned()
                .and_then(|value| serde_json::from_value(value).ok())
                .unwrap_or_default();
            let auto_play = glb
                .clip_extra(index, AUTO_PLAY)
                .and_then(Value::as_bool)
                .unwrap_or_default();
            let timing = SceneMotionTiming {
                duration_seconds,
                playback,
                auto_play,
            };
            let mut item = SceneClip::empty(id.clone(), clip.name.clone(), timing);
            item.source_id = Some(id.clone());
            item.tracks = Arc::new(read_tracks(&glb, clip, &nodes, timing));
            let binding = glb.inspect_glb(|json, _| {
                EditableClip::read(json, index, |node| nodes.get(&node).map(String::as_str))
            });
            if original.insert(id, binding).is_some() {
                return Err("主场景 GLB 的动画片段 ID 重复".into());
            }
            clips.push(item);
        }
        let empty = glb.inspect_glb(|json, _| load_drafts(json, placements.scene_id.as_str()));
        for (index, empty) in empty.into_iter().enumerate() {
            let id = empty
                .id
                .filter(|id| valid_text(id))
                .unwrap_or_else(|| format!("scene-draft-{index}"));
            if clips.iter().any(|clip| clip.id == id) {
                return Err("主场景的空动画片段 ID 重复".into());
            }
            clips.push(SceneClip::empty(id, empty.name, empty.timing));
        }
        let active = (!clips.is_empty()).then_some(0);
        let has_saved_animation = !editable_slots.is_empty() || !foreign_names.is_empty();
        Ok(Self {
            clips,
            active,
            foreign_names,
            original: Arc::new(original),
            has_saved_animation,
            available: true,
            dirty: false,
        })
    }

    pub(crate) fn available(&self) -> bool {
        self.available
    }
    pub(crate) fn has_saved_animation(&self) -> bool {
        self.has_saved_animation
    }
    pub(crate) fn clip_count(&self) -> usize {
        self.clips.len()
    }
    pub(crate) fn foreign_clip_count(&self) -> usize {
        self.foreign_names.len()
    }
    pub(crate) fn foreign_clip_at(&self, index: usize) -> Option<&str> {
        self.foreign_names.get(index).map(String::as_str)
    }
    pub(crate) fn clip_at(&self, index: usize) -> Option<(&str, &str)> {
        let clip = self.clips.get(index)?;
        Some((&clip.id, &clip.name))
    }
    pub(crate) fn clips(&self) -> impl Iterator<Item = (&str, &str)> {
        self.clips
            .iter()
            .map(|clip| (clip.id.as_str(), clip.name.as_str()))
    }
    fn index(&self, id: &str) -> Option<usize> {
        self.clips.iter().position(|clip| clip.id == id)
    }
    fn active(&self) -> Option<&SceneClip> {
        self.clips.get(self.active?)
    }
    pub(crate) fn active_clip_id(&self) -> Option<&str> {
        self.active().map(|clip| clip.id.as_str())
    }
    pub(crate) fn clip_name(&self) -> Option<&str> {
        self.active().map(|clip| clip.name.as_str())
    }
    pub(crate) fn clip_name_for(&self, id: &str) -> Option<&str> {
        self.clip_at(self.index(id)?).map(|(_, name)| name)
    }

    pub(crate) fn select_clip(&mut self, id: &str) -> bool {
        let Some(index) = self.index(id) else {
            return false;
        };
        self.active = Some(index);
        true
    }

    pub(crate) fn timing(&self) -> SceneMotionTiming {
        self.active()
            .map_or_else(SceneMotionTiming::default, |clip| clip.timing)
    }
    pub(crate) fn timing_for(&self, id: &str) -> Option<SceneMotionTiming> {
        Some(self.clips[self.index(id)?].timing)
    }
    pub(crate) fn set_timing(&mut self, timing: SceneMotionTiming) {
        if let Some(index) = self.active {
            self.clips[index].timing = timing;
            self.dirty = true;
        }
    }
    pub(crate) fn set_timing_for(
        &mut self,
        id: &str,
        timing: SceneMotionTiming,
    ) -> Result<(), String> {
        let index = self.require_index(id)?;
        self.clips[index].timing = timing;
        self.dirty = true;
        Ok(())
    }

    pub(crate) fn get(&self, placement: &str) -> Option<&SceneMotionTrack> {
        self.active()?.track(placement)
    }
    pub(crate) fn get_for(&self, id: &str, placement: &str) -> Option<&SceneMotionTrack> {
        self.clips.get(self.index(id)?)?.track(placement)
    }
    pub(crate) fn keyed_track_count(&self) -> usize {
        self.active().map_or(0, |clip| clip.tracks.len())
    }
    pub(crate) fn keyed_track_at(&self, index: usize) -> Option<&SceneMotionTrack> {
        self.active()?.tracks.get(index).map(Arc::as_ref)
    }
    pub(crate) fn tracks(&self) -> impl Iterator<Item = &SceneMotionTrack> {
        self.active()
            .into_iter()
            .flat_map(|clip| clip.tracks.iter().map(Arc::as_ref))
    }
    /// 烘焙摆放变换必须避开其他尚未保存的动画，不受当前时间轴选择影响。
    pub(crate) fn has_track(&self, placement: &str) -> bool {
        self.clips
            .iter()
            .any(|clip| clip.find_track(placement).is_ok())
    }
    pub(crate) fn replace(&mut self, placement: &str, track: Option<SceneMotionTrack>) {
        if let Some(index) = self.active {
            self.replace_at(index, placement, track);
        }
    }
    pub(crate) fn replace_for(
        &mut self,
        id: &str,
        placement: &str,
        track: Option<SceneMotionTrack>,
    ) -> Result<(), String> {
        let index = self.require_index(id)?;
        self.replace_at(index, placement, track);
        Ok(())
    }
    fn replace_at(&mut self, index: usize, placement: &str, track: Option<SceneMotionTrack>) {
        let clip = &mut self.clips[index];
        let slot = clip.find_track(placement);
        if let Some(mut track) = track.filter(|track| !track.is_empty()) {
            // 摆放参数仍是编辑和历史的正式查找身份，写入时让轨道本身持有该身份。
            if track.placement_id != placement {
                track.placement_id = placement.to_owned();
            }
            let track = Arc::new(track);
            let tracks = Arc::make_mut(&mut clip.tracks);
            match slot {
                Ok(index) => tracks[index] = track,
                Err(index) => tracks.insert(index, track),
            }
        } else if let Ok(index) = slot {
            Arc::make_mut(&mut clip.tracks).remove(index);
        }
        clip.keys_changed = true;
        self.dirty = true;
    }

    fn require_index(&self, id: &str) -> Result<usize, String> {
        self.index(id).ok_or_else(|| "场景动画片段已变化".into())
    }

    fn add(&mut self, clip: SceneClip) -> SceneMotionClipChange {
        let index = self.clips.len();
        self.clips.push(clip.clone());
        self.active = Some(index);
        self.dirty = true;
        SceneMotionClipChange {
            clip,
            index,
            added: true,
        }
    }
    pub(super) fn create(&mut self, name: &str) -> Result<SceneMotionClipChange, String> {
        if !self.available {
            return Err("当前场景没有可编辑的主 GLB".into());
        }
        let name = name.trim();
        if !valid_text(name) {
            return Err("请输入有效的动画名称".into());
        }
        let name = self.unique_name(name);
        Ok(self.add(SceneClip::empty(
            new_scene_clip_id(),
            name,
            SceneMotionTiming::default(),
        )))
    }
    pub(super) fn duplicate(&mut self, id: &str) -> Result<SceneMotionClipChange, String> {
        let mut clip = self.clips[self.require_index(id)?].clone();
        clip.id = new_scene_clip_id();
        clip.name = self.unique_name(&format!("{} 副本", clip.name));
        // 新动作默认由作者明确触发，避免复制自动片段时干扰当前机关。
        clip.timing.auto_play = false;
        Ok(self.add(clip))
    }
    fn unique_name(&self, base: &str) -> String {
        let used = self.clips().map(|(_, name)| name).collect::<HashSet<_>>();
        if !used.contains(base) {
            return base.into();
        }
        (2..)
            .map(|index| format!("{base} {index}"))
            .find(|name| !used.contains(name.as_str()))
            .unwrap()
    }
    pub(crate) fn rename(
        &mut self,
        id: &str,
        name: &str,
    ) -> Result<Option<(String, String)>, String> {
        let index = self.require_index(id)?;
        let name = name.trim();
        if !valid_text(name) {
            return Err("请输入有效的动画名称".into());
        }
        let old = &mut self.clips[index].name;
        if *old == name {
            return Ok(None);
        }
        self.dirty = true;
        Ok(Some((
            std::mem::replace(old, name.to_owned()),
            name.to_owned(),
        )))
    }
    pub(super) fn remove(&mut self, id: &str) -> Result<SceneMotionClipChange, String> {
        let index = self.require_index(id)?;
        let clip = self.remove_at(index);
        Ok(SceneMotionClipChange {
            clip,
            index,
            added: false,
        })
    }
    fn remove_at(&mut self, index: usize) -> SceneClip {
        self.active = self.active.and_then(|active| {
            if active == index {
                None
            } else {
                Some(active - usize::from(active > index))
            }
        });
        self.dirty = true;
        self.clips.remove(index)
    }
    pub(crate) fn apply_clip_history(
        &mut self,
        change: &SceneMotionClipChange,
        undoing: bool,
    ) -> Result<(), String> {
        if change.added != undoing {
            if self.index(&change.clip.id).is_some() || change.index > self.clips.len() {
                return Err("场景动画列表已变化，无法恢复该片段".into());
            }
            if let Some(active) = &mut self.active {
                *active += usize::from(*active >= change.index);
            }
            self.clips.insert(change.index, change.clip.clone());
            self.active = self.active.or(Some(change.index));
            self.dirty = true;
        } else {
            let index = self.require_index(&change.clip.id)?;
            self.remove_at(index);
        }
        Ok(())
    }

    /// 整份场景保存只复制和重写一次 GLB，其他文件动画和原始缓冲按原样带回。
    pub(crate) fn encode_glb(
        &self,
        path: &Path,
        bytes: Vec<u8>,
        placements: &ScenePlacementsAsset,
        placements_changed: bool,
    ) -> Result<(Vec<u8>, Self), String> {
        let glb = AnimationDocument::from_glb_bytes(path, bytes)?;
        let ids: HashSet<_> = placements
            .placements
            .iter()
            .map(|record| record.id.as_str())
            .collect();
        let nodes = placement_nodes(&glb, &ids);
        let target_nodes: BTreeMap<_, _> = nodes
            .iter()
            .map(|(&node, id)| (id.as_str(), node))
            .collect();
        let scene_id = placements.scene_id.as_str();
        let marker = glb
            .inspect_glb(|json, _| scene_marker(json, scene_id))
            .ok_or("场景 GLB 根身份已变化")?;
        let retain_geometry = glb.inspect_glb(|json, bin| {
            json["nodes"][marker]["extras"]["feathrise_geometry_hash"]
                .as_str()
                .is_some_and(|hash| geometry_hash(json, bin) == hash)
        });
        let bindings = glb.inspect_glb(|json, _| self.bindings(json, &nodes, placements_changed));
        let original_slots: BTreeMap<_, _> =
            bindings.iter().map(|(&id, &index)| (index, id)).collect();
        let glb_clips: BTreeMap<_, _> = glb
            .clips
            .iter()
            .map(|clip| (clip.source_index, clip))
            .collect();

        let mut saved = self.clone();
        let mut encoded = Vec::new();
        let mut plan = Vec::with_capacity(saved.clips.len());
        let mut colliders = BTreeMap::new();
        for item in &mut saved.clips {
            let source = item
                .source_id
                .as_deref()
                .and_then(|id| bindings.get(id).copied());
            let source_clip = source.and_then(|index| glb_clips.get(&index)).copied();
            let slot = bindings.get(item.id.as_str()).copied();
            let rebuilt = if item.keys_changed || source.is_none() {
                let mut clip = Clip {
                    name: item.name.clone(),
                    source_index: 0,
                    channels: Vec::new(),
                };
                for track in item.tracks.iter() {
                    if let Some(&node) = target_nodes.get(track.placement_id.as_str()) {
                        append_track(&mut clip, source_clip, &glb, node, track)?;
                    }
                }
                let duration = item.timing.duration_seconds;
                if !duration.is_finite() || duration <= 0.0 || duration + 1e-6 < clip.duration() {
                    return Err("场景动画结束帧必须容纳所有关键帧".into());
                }
                if !clip.channels.is_empty() {
                    encoded.push(clip);
                    true
                } else {
                    false
                }
            } else {
                false
            };
            for track in item.tracks.iter() {
                if let Some(&node) = target_nodes.get(track.placement_id.as_str()) {
                    let collider = colliders.entry(node).or_insert(None);
                    if collider.is_none() {
                        *collider = track.collider;
                    }
                }
            }
            plan.push((slot, source, item.keys_changed, rebuilt));
            if item.tracks.iter().any(|track| !ids.contains(track.placement_id.as_str())) {
                Arc::make_mut(&mut item.tracks).retain(|track| ids.contains(track.placement_id.as_str()));
            }
            item.keys_changed = false;
        }

        let bytes = glb.encode_scene_clips(
            encoded.iter().map(|clip| (clip, json!({}))),
            |json, bin, encoded| {
                let root = json.as_object_mut().ok_or("场景 GLB 结构无效")?;
                let original = root
                    .remove("animations")
                    .and_then(|value| match value {
                        Value::Array(entries) => Some(entries),
                        _ => None,
                    })
                    .unwrap_or_default();
                let mut replacements = BTreeMap::new();
                let mut appended = Vec::new();
                let mut empty = Vec::new();
                let mut encoded = encoded.into_iter();
                for (item, (slot, source, edited, rebuilt)) in saved.clips.iter().zip(plan) {
                    let raw = source.and_then(|index| original.get(index)).cloned();
                    let entry = if edited || source.is_none() {
                        merge_channels(
                            raw,
                            rebuilt.then(|| encoded.next().expect("已编码片段顺序匹配")),
                            &nodes,
                        )?
                    } else {
                        raw
                    };
                    if let Some(mut entry) = entry {
                        update_entry(&mut entry, item)?;
                        if let Some(slot) = slot {
                            replacements.insert(slot, entry);
                        } else {
                            appended.push(entry);
                        }
                    } else {
                        empty.push(EmptySceneAnimation {
                            id: Some(item.id.clone()),
                            name: item.name.clone(),
                            timing: item.timing,
                        });
                    }
                }
                let animations: Vec<_> = original
                    .into_iter()
                    .enumerate()
                    .filter_map(|(slot, entry)| {
                        replacements
                            .remove(&slot)
                            .or_else(|| (!original_slots.contains_key(&slot)).then_some(entry))
                    })
                    .chain(appended)
                    .collect();
                saved.has_saved_animation = !animations.is_empty();
                if saved.has_saved_animation {
                    root.insert("animations".into(), Value::Array(animations));
                }
                saved.foreign_names = root.get("animations").map_or_else(Arc::default, |entries| {
                    foreign_names(entries, |_, entry| {
                        entry["extras"][OWNER].as_bool() == Some(true)
                    })
                });

                for (node, collider) in colliders {
                    if let Some(extras) = json["nodes"][node]["extras"].as_object_mut() {
                        if let Some(collider) = collider {
                            extras.insert(COLLIDER.into(), json!(collider));
                        } else {
                            extras.remove(COLLIDER);
                        }
                    }
                }
                if retain_geometry {
                    refresh_geometry_hash(json, bin, marker);
                }
                let extras = json["nodes"][marker]["extras"]
                    .as_object_mut()
                    .ok_or("场景 GLB 根属性无效")?;
                extras.insert(OWNER.into(), json!(true));
                extras.remove(SCENE_ANIMATION_DRAFT_EXTRA);
                extras.remove(DRAFTS);
                if empty.len() == 1 {
                    extras.insert(SCENE_ANIMATION_DRAFT_EXTRA.into(), json!(&empty[0]));
                } else if !empty.is_empty() {
                    extras.insert(DRAFTS.into(), json!(empty));
                }

                let original: BTreeMap<_, _> = json["animations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .filter(|(_, entry)| entry["extras"][OWNER].as_bool() == Some(true))
                    .filter_map(|(index, entry)| {
                        Some((
                            entry["extras"][CLIP_ID].as_str()?.to_owned(),
                            EditableClip::read(json, index, |node| {
                                nodes.get(&node).map(String::as_str)
                            }),
                        ))
                    })
                    .collect();
                for item in &mut saved.clips {
                    item.source_id = original.contains_key(&item.id).then(|| item.id.clone());
                }
                saved.original = Arc::new(original);
                saved.dirty = false;
                Ok(())
            },
        )?;
        Ok((bytes, saved))
    }

    /// 没有作者片段时，删除摆放仍可能让只读动画失去所有通道。
    pub(crate) fn refresh_foreign_clips(&self, bytes: &[u8]) -> Result<Self, String> {
        let glb = Glb::from_slice(bytes).map_err(|error| error.to_string())?;
        let json: Value = serde_json::from_slice(&glb.json).map_err(|error| error.to_string())?;
        let mut saved = self.clone();
        saved.foreign_names = foreign_names(&json["animations"], |_, _| false);
        saved.has_saved_animation = !saved.foreign_names.is_empty();
        Ok(saved)
    }

    fn bindings<'a>(
        &'a self,
        json: &Value,
        nodes: &BTreeMap<usize, String>,
        placements_changed: bool,
    ) -> BTreeMap<&'a str, usize> {
        let explicit: BTreeMap<_, _> = json["animations"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter(|(_, clip)| clip["extras"][OWNER].as_bool() == Some(true))
            .filter_map(|(index, clip)| Some((clip["extras"][CLIP_ID].as_str()?, index)))
            .collect();
        let ids = nodes.values().map(String::as_str).collect();
        self.original
            .iter()
            .filter_map(|(id, old)| {
                let index = explicit.get(id.as_str()).copied().or_else(|| {
                    if !placements_changed {
                        return Some(old.source_index);
                    }
                    old.rebind(json, |node| nodes.get(&node).map(String::as_str), &ids)
                        .map(|clip| clip.source_index)
                })?;
                Some((id.as_str(), index))
            })
            .collect()
    }
}

fn foreign_names(animations: &Value, editable: impl Fn(usize, &Value) -> bool) -> Arc<[String]> {
    animations
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter(|(index, entry)| !editable(*index, entry))
        .map(|(index, entry)| {
            entry["name"]
                .as_str()
                .filter(|name| !name.trim().is_empty())
                .map_or_else(|| format!("动画 {}", index + 1), str::to_owned)
        })
        .collect()
}

fn valid_text(text: &str) -> bool {
    !text.trim().is_empty() && !text.chars().any(char::is_control)
}

fn load_drafts(json: &Value, scene_id: &str) -> Vec<EmptySceneAnimation> {
    let Some(marker) = scene_marker(json, scene_id) else {
        return Vec::new();
    };
    if let Some(entries) = json["nodes"][marker]["extras"][DRAFTS].as_array() {
        return entries
            .iter()
            .filter_map(|value| EmptySceneAnimation::deserialize(value).ok())
            .filter(|entry| {
                valid_text(&entry.name)
                    && entry.timing.duration_seconds.is_finite()
                    && entry.timing.duration_seconds > 0.0
            })
            .collect();
    }
    read_empty_clip(json, scene_id).into_iter().collect()
}

fn read_tracks(
    glb: &AnimationDocument,
    clip: &Clip,
    nodes: &BTreeMap<usize, String>,
    timing: SceneMotionTiming,
) -> Vec<Arc<SceneMotionTrack>> {
    let mut tracks = BTreeMap::new();
    for channel in &clip.channels {
        let Some(id) = nodes
            .get(&channel.node)
            .filter(|_| matches!(channel.path.as_str(), "translation" | "rotation" | "scale"))
        else {
            continue;
        };
        let track = tracks
            .entry(id.clone())
            .or_insert_with(|| SceneMotionTrack {
                placement_id: id.clone(),
                duration_seconds: timing.duration_seconds,
                playback: timing.playback,
                position: Vec::new(),
                rotation: Vec::new(),
                scale: Vec::new(),
                collider: glb
                    .node_extra(channel.node, COLLIDER)
                    .and_then(|value| SceneMotionCollider::deserialize(value).ok()),
            });
        let keys = match channel.path.as_str() {
            "rotation" => &mut track.rotation,
            "scale" => &mut track.scale,
            _ => &mut track.position,
        };
        *keys = project_channel(channel);
    }
    tracks
        .into_values()
        .filter(|track| !track.is_empty())
        .map(Arc::new)
        .collect()
}

fn append_track(
    clip: &mut Clip,
    source: Option<&Clip>,
    glb: &AnimationDocument,
    node: usize,
    track: &SceneMotionTrack,
) -> Result<(), String> {
    track.validate()?;
    for (path, keys) in [
        ("translation", &track.position),
        ("rotation", &track.rotation),
        ("scale", &track.scale),
    ] {
        if keys.is_empty() {
            continue;
        }
        if let Some(channel) = source.and_then(|clip| {
            clip.channels
                .iter()
                .find(|c| c.node == node && c.path == path)
        }) && project_channel(channel) == *keys
        {
            clip.push_existing_channel(channel.clone())?;
        } else {
            let encoded = keys
                .iter()
                .map(|key| Keyframe {
                    time: key.time,
                    value: if path == "rotation" {
                        quat(key.value).to_array().to_vec()
                    } else {
                        key.value.to_vec()
                    },
                    in_tangent: Vec::new(),
                    out_tangent: Vec::new(),
                })
                .collect();
            clip.push_transform_channel(node, &glb.nodes[node].name, path, encoded)?;
        }
    }
    Ok(())
}

/// 合并作者轨道时只替换摆放的 TRS，原片段的骨骼、形态键及扩展通道仍指向原采样器。
fn merge_channels(
    mut raw: Option<Value>,
    encoded: Option<Value>,
    nodes: &BTreeMap<usize, String>,
) -> Result<Option<Value>, String> {
    let Some(mut original) = raw.take() else {
        return Ok(encoded);
    };
    let fields = original.as_object_mut().ok_or("场景动画源片段无效")?;
    let mut channels = fields
        .remove("channels")
        .and_then(|value| match value {
            Value::Array(entries) => Some(entries),
            _ => None,
        })
        .unwrap_or_default();
    channels.retain(|channel| {
        let target = &channel["target"];
        let node = target["node"]
            .as_u64()
            .and_then(|node| usize::try_from(node).ok());
        !node.is_some_and(|node| nodes.contains_key(&node))
            || !matches!(
                target["path"].as_str(),
                Some("translation" | "rotation" | "scale")
            )
    });
    // 只保留未改轨道实际使用的原采样器，连续保存不会重复累积失去通道的条目。
    let samplers = fields
        .get_mut("samplers")
        .and_then(Value::as_array_mut)
        .ok_or("场景动画原采样器无效")?;
    let previous = std::mem::take(samplers);
    let mut remap = BTreeMap::new();
    for channel in &mut channels {
        let index = channel["sampler"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|&index| index < previous.len())
            .ok_or("场景动画原采样器索引无效")?;
        let mapped = remap.entry(index).or_insert_with(|| {
            let next = samplers.len();
            samplers.push(previous[index].clone());
            next
        });
        channel["sampler"] = json!(*mapped);
    }
    if let Some(encoded) = encoded {
        let offset = samplers.len();
        samplers.extend(
            encoded["samplers"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned(),
        );
        for mut channel in encoded["channels"]
            .as_array()
            .into_iter()
            .flatten()
            .cloned()
        {
            let index = channel["sampler"].as_u64().ok_or("动画采样器索引无效")?;
            channel["sampler"] = json!(index + offset as u64);
            channels.push(channel);
        }
    }
    if channels.is_empty() {
        return Ok(None);
    }
    fields.insert("channels".into(), Value::Array(channels));
    Ok(Some(original))
}

fn update_entry(entry: &mut Value, item: &SceneClip) -> Result<(), String> {
    let fields = entry.as_object_mut().ok_or("场景动画条目无效")?;
    fields.insert("name".into(), json!(item.name));
    let previous = fields.entry("extras").or_insert_with(|| json!({}));
    if !previous.is_object() {
        let preserved = std::mem::take(previous);
        *previous = json!({"feathrise_source_extras": preserved});
    }
    let SceneMotionTiming {
        duration_seconds,
        playback,
        auto_play,
    } = item.timing;
    let extras = previous
        .as_object_mut()
        .expect("已确保动画属性是 JSON 对象");
    for (key, value) in [
        (OWNER, json!(true)),
        (CLIP_ID, json!(item.id)),
        (DURATION, json!(duration_seconds)),
        (PLAYBACK, json!(playback)),
        (AUTO_PLAY, json!(auto_play)),
    ] {
        extras.insert(key.into(), value);
    }
    Ok(())
}

impl super::super::EditorAuthoringSession {
    fn scene_motion(&mut self) -> Result<&mut SceneMotionDocument, String> {
        self.draft
            .as_mut()
            .map(|draft| &mut draft.motion)
            .ok_or_else(|| "当前未加载场景".into())
    }

    fn record_clip_change(&mut self, change: SceneMotionClipChange) -> String {
        let id = change.clip.id.clone();
        self.undo
            .push(super::super::EditorCommand::SceneMotionClipChanged(
                Box::new(change),
            ));
        // 由原输入系统统一将已作废的重做历史移交后台释放。
        id
    }

    pub(crate) fn new_scene_motion_clip(&mut self, name: &str) -> Result<String, String> {
        let change = self.scene_motion()?.create(name)?;
        Ok(self.record_clip_change(change))
    }

    pub(crate) fn duplicate_scene_motion_clip(&mut self, id: &str) -> Result<String, String> {
        let change = self.scene_motion()?.duplicate(id)?;
        Ok(self.record_clip_change(change))
    }

    pub(crate) fn remove_scene_motion_clip(&mut self, id: &str) -> Result<(), String> {
        let change = self.scene_motion()?.remove(id)?;
        self.record_clip_change(change);
        Ok(())
    }

    pub(crate) fn rename_scene_motion_clip(&mut self, id: &str, name: &str) -> Result<(), String> {
        if let Some((before, after)) = self.scene_motion()?.rename(id, name)? {
            self.undo
                .push(super::super::EditorCommand::SceneMotionNameChanged {
                    clip_id: id.into(),
                    before,
                    after,
                });
        }
        Ok(())
    }
}
