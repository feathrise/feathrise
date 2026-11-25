//! 普通 GLB 动作片段的稳定身份、元数据及可撤销复制和删除。

use super::{reader::array, types::*};
use serde_json::Value;
use std::{
    sync::{Arc, atomic::{AtomicU64, Ordering}},
    time::{SystemTime, UNIX_EPOCH},
};

/// 场景树和资产动画工作台用同一身份格式，避免在同一 GLB 内复制出相同运行时身份。
pub fn new_scene_clip_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let instant = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("scene-clip-{instant:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}

impl AnimationDocument {
    /// 历史与磁盘都按稳定片段身份编号。调用方只调整展示顺序时，编码保留既有动画槽位。
    pub(super) fn clips_in_file_order(&self) -> Vec<&Clip> {
        let mut ordered: Vec<_> = self.clips.iter().collect();
        ordered.sort_unstable_by_key(|clip| clip.source_index);
        ordered
    }

    /// 目录使用磁盘中的 GLB 数组序号。已打开会话再映射成其稳定片段身份。
    pub fn clip_id_from_disk_index(&self, index: usize) -> Option<usize> {
        self.source.original_clips.get(index).map(|clip| clip.source_index)
    }

    /// 已保存目录名称用于校验行身份，作者可能已改名或持有删除前的无名片段占位名称。
    pub fn disk_clip_name_matches(&self, index: usize, name: &str) -> bool {
        array(&self.source.json, "animations").get(index).is_some_and(|clip| {
            clip["name"].as_str().filter(|name| !name.trim().is_empty())
                .map_or_else(|| name == format!("动画 {}", index + 1), |disk_name| name == disk_name)
        })
    }

    pub(super) fn disk_clip_index(&self, id: usize) -> Option<usize> {
        self.source.original_clips.binary_search_by_key(&id, |clip| clip.source_index).ok()
    }

    pub(super) fn original_clip(&self, id: usize) -> Option<(&Clip, &Value)> {
        if let Some(index) = self.disk_clip_index(id) {
            let source = array(&self.source.json, "animations").get(index)?;
            Some((&self.source.original_clips[index], source))
        } else {
            self.retained_clips.get(&id).map(|clip| (&clip.original, &clip.animation))
        }
    }

    pub(super) fn next_clip_index(&self) -> usize {
        // 稀疏索引也能继续新建。删除或撤销后不可把旧目录序号误投递到另一份片段。
        self.clips.last().map_or(0, |last| last.source_index + 1)
            .max(self.preserved_channel_counts.len())
    }

    pub(super) fn unique_clip_name(&self, base: &str) -> String {
        let in_use = |name: &str| self.clips.iter().any(|clip| clip.name == name);
        if !in_use(base) {
            return base.to_owned();
        }
        (2..).map(|index| format!("{base} {index}")).find(|name| !in_use(name)).unwrap()
    }

    pub fn renamed_clip_name(&self, id: usize, name: &str) -> Result<String, String> {
        if self.read_only {
            return Err("外置 GLTF/buffer 仅可查看，请先导出独立 GLB".into());
        }
        let existing = self.clips.iter().find(|clip| clip.source_index == id).ok_or("动画片段已改变")?;
        let name = name.trim();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err("请输入可显示的动画名称".into());
        }
        if existing.name != name && self.clips.iter().any(|clip| clip.name == name) {
            return Err("已有同名动画，请使用不同的名称".into());
        }
        Ok(name.to_owned())
    }

    pub fn snapshot_clip(&self, position: usize) -> Result<ClipSnapshot, String> {
        if self.read_only {
            return Err("外置 GLTF/buffer 仅可查看，请先导出独立 GLB".into());
        }
        let clip = self.clips.get(position).ok_or("动画片段已改变")?;
        let retained = if let Some(index) = self.disk_clip_index(clip.source_index) {
            let animation = array(&self.source.json, "animations").get(index).ok_or("原动画片段不存在")?;
            Some(Arc::new(RetainedClip { original: self.source.original_clips[index].clone(), animation: animation.clone() }))
        } else {
            self.retained_clips.get(&clip.source_index).cloned()
        };
        Ok(ClipSnapshot {
            clip: Arc::new(clip.clone()),
            retained,
            preserved_channels: self.preserved_channel_counts.get(clip.source_index).copied().unwrap_or(0),
        })
    }

    pub fn duplicate_clip(&self, position: usize) -> Result<ClipSnapshot, String> {
        let mut copy = self.snapshot_clip(position)?;
        let clip = Arc::make_mut(&mut copy.clip);
        clip.name = self.unique_clip_name(&format!("{} 副本", clip.name));
        clip.source_index = self.next_clip_index();
        if let Some(retained) = &mut copy.retained
            && retained.animation["extras"]["feathrise_scene_animation"].as_bool() == Some(true)
        {
            // 直接从资产打开场景 GLB 时也须保留场景身份契约，副本默认由作者手动触发。
            let extras = &mut Arc::make_mut(retained).animation["extras"];
            extras["feathrise_clip_id"] = Value::String(new_scene_clip_id());
            extras["feathrise_autoplay"] = Value::Bool(false);
        }
        Ok(copy)
    }

    pub(crate) fn insert_clip(&mut self, position: usize, clip: &Clip, snapshot: Option<&ClipSnapshot>) -> Result<(), String> {
        let id = clip.source_index;
        if position > self.clips.len()
            || position.checked_sub(1).is_some_and(|previous| self.clips[previous].source_index >= id)
            || self.clips.get(position).is_some_and(|next| next.source_index <= id)
        {
            return Err("动画片段已改变，请重新选择".into());
        }
        if self.preserved_channel_counts.len() <= id {
            self.preserved_channel_counts.resize(id + 1, 0);
        }
        if let Some(snapshot) = snapshot {
            self.preserved_channel_counts[id] = snapshot.preserved_channels;
            if let Some(source) = &snapshot.retained {
                self.retained_clips.insert(id, source.clone());
            }
        }
        self.removed_clips.remove(&id);
        self.clips.insert(position, clip.clone());
        Ok(())
    }

    pub(crate) fn remove_clip(&mut self, position: usize, id: usize) -> Result<(), String> {
        if self.clips.get(position).is_none_or(|clip| clip.source_index != id) {
            return Err("动画片段已改变，请重新选择".into());
        }
        self.retained_clips.remove(&id);
        self.removed_clips.insert(id);
        self.clips.remove(position);
        Ok(())
    }
}
