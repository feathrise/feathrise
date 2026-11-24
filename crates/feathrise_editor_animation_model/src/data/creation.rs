//! 从当前 GLB 的静置姿态准备可插帧的动画草稿，沿用源节点和 GLB 片段索引。

use super::types::*;
use super::reader::array;
use crate::types::PoseProperty;
use std::collections::HashSet;

impl AnimationDocument {
    pub fn can_animate_node(&self, index: usize) -> bool {
        !self.read_only && array(&self.source.json, "nodes").get(index)
            .is_some_and(|node| node.get("matrix").is_none())
    }

    /// 在后台创建本次确实编辑的缺失属性轨道。拒绝覆盖解析器保留的只读原通道。
    pub fn create_pose_tracks(
        &self, clip: &Clip, targets: &[(usize, PoseProperty)],
    ) -> Result<Vec<Channel>, String> {
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let preserved = self.preserved_channel_counts.get(clip.source_index).copied().unwrap_or(0);
        let source = self.original_clip(clip.source_index).map(|(_, animation)| animation);
        let mut occupied: HashSet<_> = source.into_iter()
            .flat_map(|animation| array(animation, "channels").iter().take(preserved))
            .filter_map(|channel| {
                let node = usize::try_from(channel["target"]["node"].as_u64()?).ok()?;
                let property = PoseProperty::from_path(channel["target"]["path"].as_str()?)?;
                Some((node, property))
            })
            .collect();
        occupied.extend(clip.channels.iter().filter_map(|channel| {
            Some((channel.node, PoseProperty::from_path(&channel.path)?))
        }));
        let start = preserved.max(clip.channels.last().map_or(0, |channel| channel.source_index + 1));
        targets.iter().enumerate().map(|(offset, &(index, property))| {
            if !self.can_animate_node(index) {
                return Err("目标骨骼的变换不可写入动画".into());
            }
            let node = &self.nodes[index];
            if !occupied.insert((index, property)) {
                return Err(format!("{}的{}通道已存在或只读，整组姿势未插入", node.name, property.path()));
            }
            let value = match property {
                PoseProperty::Translation => node.translation.to_vec(),
                PoseProperty::Rotation => node.rotation.to_vec(),
                PoseProperty::Scale => node.scale.to_vec(),
            };
            // 为新轨道保留第零帧的原静置值；在第零帧插键时由正式插帧逻辑覆盖。
            Ok(Channel {
                node: index,
                path: property.path().into(),
                node_name: node.name.clone(),
                interpolation: "LINEAR".into(),
                keys: vec![Keyframe { time: 0.0, value, in_tangent: Vec::new(), out_tangent: Vec::new() }],
                source_index: start + offset,
            })
        }).collect()
    }

    /// 候选节点已在后台解析时确定，属性面板只读取常量时间的资格。
    pub fn can_create_clip(&self) -> bool {
        !self.read_only && !self.source.animation_nodes.is_empty()
    }

    /// 在后台构造首尾静置键，留出一秒区间供播放头定位和插帧。
    pub fn create_clip(&self) -> Result<Clip, String> {
        if !self.can_create_clip() {
            return Err("当前 GLB 没有可写入动画的节点".into());
        }
        let source_index = self.next_clip_index();
        let mut channels = Vec::new();
        for &index in &self.source.animation_nodes {
            let node = &self.nodes[index];
            for (path, value) in [
                ("translation", node.translation.as_slice()),
                ("rotation", node.rotation.as_slice()),
                ("scale", node.scale.as_slice()),
            ] {
                channels.push(Channel {
                    node: index,
                    path: path.into(),
                    node_name: node.name.clone(),
                    interpolation: "LINEAR".into(),
                    keys: [0.0, 1.0].map(|time| Keyframe {
                        time,
                        value: value.to_vec(),
                        in_tangent: Vec::new(),
                        out_tangent: Vec::new(),
                    }).into(),
                    source_index: channels.len(),
                });
            }
        }
        Ok(Clip {
            name: self.unique_clip_name(&format!("动画 {}", source_index + 1)),
            channels,
            source_index,
        })
    }
}
