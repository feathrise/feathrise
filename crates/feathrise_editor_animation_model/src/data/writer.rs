//! 追加式 GLB 编码与精确版本原子保存；原 buffer、未知字段/块及未编辑通道保留。

use super::{
    reader::{BIN_CHUNK, JSON_CHUNK, array},
    types::*,
};
use feathrise_scene_content::authoring_transaction::{PreparedWrite, commit_prepared_writes_checked};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};

impl AnimationDocument {
    /// 场景保存事务已经持有源文件字节。沿用模型动画的解析器，避免从磁盘重新打开并取得另一份修订。
    pub fn from_glb_bytes(path: &Path, bytes: Vec<u8>) -> Result<Self, String> {
        let document = Self::from_bytes(path, bytes)?;
        if document.read_only {
            return Err("动画需要内嵌缓冲的 GLB".into());
        }
        Ok(document)
    }

    pub fn node_extra(&self, node: usize, key: &str) -> Option<&Value> {
        array(&self.source.json, "nodes").get(node)?.get("extras")?.get(key)
    }

    pub fn clip_extra(&self, clip: usize, key: &str) -> Option<&Value> {
        array(&self.source.json, "animations").get(clip)?.get("extras")?.get(key)
    }

    /// 在原缓冲上核验场景的几何来源，动画写入前不改变任何节点或附件。
    pub fn inspect_glb<T>(&self, inspect: impl FnOnce(&Value, &[u8]) -> T) -> T {
        let binary = self.source.chunks.iter().find(|chunk| chunk.kind == BIN_CHUNK)
            .map_or(&[][..], |chunk| chunk.bytes.as_ref());
        inspect(&self.source.json, binary)
    }

    /// 将场景编辑器拥有的片段合进同一主 GLB。采样器和关键帧沿用普通动画编码，其他片段继续引用原缓冲。
    pub fn encode_owned_clip(
        &self,
        previous: Option<usize>,
        clip: &Clip,
        extras: Value,
        finish: impl FnOnce(&mut Value, &[u8]) -> Result<(), String>,
    ) -> Result<Vec<u8>, String> {
        if self.read_only {
            return Err("仅支持保存独立 GLB 文件".into());
        }
        let mut json = self.source.json.clone();
        let mut binary = self.source.chunks.iter().find(|chunk| chunk.kind == BIN_CHUNK)
            .map_or_else(Vec::new, |chunk| chunk.bytes.to_vec());
        let entry = if clip.channels.is_empty() {
            None
        } else {
            let mut encoded = encode_new_clip(&mut json, &mut binary, clip)?;
            encoded["extras"] = extras;
            Some(encoded)
        };
        if let Some(index) = previous {
            let animations = json["animations"].as_array_mut().ok_or("原动画列表无效")?;
            if index >= animations.len() {
                return Err("原场景动画片段已换代".into());
            }
            if let Some(entry) = entry {
                animations[index] = entry;
            } else {
                animations.remove(index);
            }
            if animations.is_empty() {
                json.as_object_mut().ok_or("GLB 结构无效")?.remove("animations");
            }
        } else if let Some(entry) = entry {
            json.as_object_mut().ok_or("GLB 结构无效")?.entry("animations")
                .or_insert_with(|| json!([])).as_array_mut().ok_or("动画列表无效")?.push(entry);
        }
        if !binary.is_empty() {
            if self.source.buffers.is_empty() {
                json["buffers"] = json!([{"byteLength": binary.len()}]);
            } else {
                json["buffers"][0]["byteLength"] = json!(binary.len());
            }
        }
        finish(&mut json, &binary)?;
        encode_chunks(&self.source.chunks, &json, &binary)
    }

    /// 后台只读比对磁盘身份；回到同一未变化文件时无需替换会话，也不丢失已保存的撤销历史。
    pub fn disk_unchanged(&self, path: &Path) -> Result<bool, String> {
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("无法校验动画文件路径：{error}"))?;
        if canonical != self.source.path {
            return Ok(false);
        }
        // 外置 BIN 可独立于 JSON 更新；只读来源保守重读，不能仅凭 JSON 字节冒充整份来源不变。
        if self.read_only {
            return Ok(false);
        }
        let bytes =
            std::fs::read(&canonical).map_err(|error| format!("无法校验动画文件：{error}"))?;
        Ok(bytes.as_slice() == self.source.bytes.as_ref())
    }

    /// 保存后只换磁盘身份；LINEAR/STEP 暂未使用的三次切线继续留在作者草稿中供撤销恢复。
    pub fn save_rebased(&self, path: &Path) -> Result<Self, String> {
        // 回读核验复用实际写入的编码字节，避免再编码一遍完整 GLB。
        let expected = self.write_current(path)?;
        let mut saved = Self::load(path)?;
        // 保存与重读之间也可能发生外部导出，不能把另一份文件当成本次草稿的版本基线。
        if saved.source.bytes.as_ref() != expected.as_slice() {
            return Err("动画保存后文件再次被外部修改，原草稿已保留，请重新载入后处理冲突".into());
        }
        saved.clips = self.clips.clone();
        saved.preserved_clip_count = self.preserved_clip_count;
        saved.preserved_channel_counts = self.preserved_channel_counts.clone();
        // 已核对本次编码的精确字节，节点身份未变；保存不应使同源跨动作剪贴板失效。
        saved.source_identity = self.source_identity.clone();
        // load 返回的新 Source 没有其他持有者，更新基线不会复制大 BIN 或影响原作者版本。
        std::sync::Arc::get_mut(&mut saved.source)
            .ok_or("无法建立动画保存后的独立版本基线")?
            .original_clips = self.clips.clone();
        Ok(saved)
    }

    /// 保存只作用于已加载的原文件；成功后会话重新读取以建立下一次保存的精确字节基线。
    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.write_current(path).map(drop)
    }

    fn write_current(&self, path: &Path) -> Result<Vec<u8>, String> {
        if self.read_only {
            return Err("外置 GLTF/buffer 仅可查看，请先在 Blender 导出独立 GLB".into());
        }
        let canonical = path
            .canonicalize()
            .map_err(|error| format!("动画文件已不存在：{error}"))?;
        if canonical != self.source.path {
            return Err("保存目标必须与当前加载的动画文件相同".into());
        }
        let bytes = self.encode()?;
        let verify = || -> Result<(), String> {
            if path.canonicalize().map_err(|error| error.to_string())? != self.source.path {
                return Err("动画文件路径已变化，草稿已保留".into());
            }
            let current =
                std::fs::read(&canonical).map_err(|error| format!("无法校验动画文件：{error}"))?;
            if current.as_slice() != self.source.bytes.as_ref() {
                return Err("动画文件已被外部修改，保存已停止；请先保留草稿再重新载入".into());
            }
            if canonical
                .metadata()
                .map_err(|error| error.to_string())?
                .permissions()
                .readonly()
            {
                return Err("动画文件为只读，草稿已保留".into());
            }
            Ok(())
        };
        verify()?;
        if bytes.as_slice() != self.source.bytes.as_ref() {
            commit_prepared_writes_checked(
                vec![PreparedWrite::borrowed(canonical.clone(), &bytes)],
                |_, _| verify(),
            )
            .map_err(|error| format!("动画保存失败：{}", error.message()))?;
        }
        Ok(bytes)
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, String> {
        if self.read_only {
            return Err("仅支持保存独立 GLB 文件".into());
        }
        let mut json = self.source.json.clone();
        let mut binary = self
            .source
            .chunks
            .iter()
            .find(|chunk| chunk.kind == BIN_CHUNK)
            .map_or_else(Vec::new, |chunk| chunk.bytes.to_vec());
        let mut changed = false;
        if self.clips.len() < self.preserved_clip_count {
            return Err("原 GLB 的动画片段必须保留".into());
        }
        if self.clips.len() < self.source.original_clips.len() {
            // 保存后撤销新建只移去本会话追加的尾片段，已有片段的序号始终不变。
            let animations = json["animations"].as_array_mut().ok_or("动画列表无效")?;
            animations.truncate(self.clips.len());
            if animations.is_empty() {
                json.as_object_mut().ok_or("动画 JSON 无效")?.remove("animations");
            }
            changed = true;
        }
        let mut clip_indices = HashSet::new();
        for clip in &self.clips {
            let index = clip.source_index;
            if index >= self.clips.len() {
                return Err("动画片段已不存在".into());
            }
            if !clip_indices.insert(index) {
                return Err("动画片段身份重复".into());
            }
            let Some(original) = self.source.original_clips.get(index) else {
                // 已有片段仍按原索引定位，仅允许新片段在当前尾部顺序追加。
                if array(&json, "animations").len() != index {
                    return Err("新动画片段必须按原索引顺序追加".into());
                }
                let animation = encode_new_clip(&mut json, &mut binary, clip)?;
                json.as_object_mut().ok_or("动画 JSON 无效")?
                    .entry("animations").or_insert_with(|| json!([]))
                    .as_array_mut().ok_or("动画列表无效")?.push(animation);
                changed = true;
                continue;
            };
            let source_animation = array(&self.source.json, "animations")
                .get(clip.source_index)
                .ok_or("原动画片段不存在")?;
            let preserved = self.preserved_channel_counts.get(index).copied().unwrap_or(0);
            let raw_channels = array(source_animation, "channels");
            let authored = clip.channels.partition_point(|channel| channel.source_index < preserved);
            let original_known = original.channels.partition_point(|channel| channel.source_index < preserved);
            if authored != original_known {
                return Err("原动画通道必须保留".into());
            }
            let desired_len = preserved + clip.channels.len() - authored;
            if desired_len < raw_channels.len() {
                // 保存后也能撤销追加通道；原始索引（含未知通道）不移动。
                json["animations"][index]["channels"].as_array_mut()
                    .ok_or("动画通道列表无效")?.truncate(desired_len);
                changed = true;
            }
            if clip.name != original.name {
                json["animations"][clip.source_index]["name"] = Value::String(clip.name.clone());
                changed = true;
            }
            let mut channel_indices = HashSet::new();
            let mut targets: HashSet<_> = raw_channels.iter().take(preserved).filter_map(|channel| {
                Some((usize::try_from(channel["target"]["node"].as_u64()?).ok()?, channel["target"]["path"].as_str()?))
            }).collect();
            for (position, channel) in clip.channels.iter().enumerate() {
                if !channel_indices.insert(channel.source_index) {
                    return Err("动画通道身份重复".into());
                }
                let previous = original
                    .channels
                    .binary_search_by_key(&channel.source_index, |candidate| candidate.source_index)
                    .ok().map(|i| &original.channels[i]);
                if channel.source_index < preserved {
                    if previous.is_none_or(|previous| previous.node != channel.node || previous.path != channel.path) {
                        return Err("原动画通道目标已改变".into());
                    }
                } else if position < authored || channel.source_index != preserved + position - authored
                    || !self.can_animate_node(channel.node) || !targets.insert((channel.node, channel.path.as_str()))
                {
                    return Err("新增动画通道的索引或目标无效".into());
                }
                if previous == Some(channel) {
                    continue;
                }
                let (input, output) = append_channel_accessors(&mut json, &mut binary, channel)?;
                // 新轨道可复用刚撤销后的尾部索引；只有同一目标才带上原扩展字段。
                let old_channel = previous.filter(|previous| previous.node == channel.node && previous.path == channel.path)
                    .and_then(|_| raw_channels.get(channel.source_index));
                let mut sampler = if let Some(old_channel) = old_channel {
                    let sampler_index = old_channel["sampler"].as_u64().ok_or("原 sampler 索引无效")? as usize;
                    source_animation["samplers"][sampler_index].clone()
                } else {
                    json!({})
                };
                sampler["input"] = json!(input);
                sampler["output"] = json!(output);
                sampler["interpolation"] = json!(channel.interpolation);
                let animation = &mut json["animations"][clip.source_index];
                let samplers = animation["samplers"]
                    .as_array_mut()
                    .ok_or("动画 sampler 列表无效")?;
                let next_sampler = samplers.len();
                // 共享 sampler 不能原位改动：为当前已编辑 channel 追加独立 sampler。
                samplers.push(sampler);
                let channels = animation["channels"].as_array_mut().ok_or("动画通道列表无效")?;
                if old_channel.is_some() {
                    channels[channel.source_index]["sampler"] = json!(next_sampler);
                } else {
                    let encoded = json!({
                        "sampler": next_sampler, "target": {"node": channel.node, "path": channel.path},
                    });
                    if channel.source_index == channels.len() {
                        channels.push(encoded);
                    } else {
                        *channels.get_mut(channel.source_index).ok_or("新增动画通道索引无效")? = encoded;
                    }
                }
                changed = true;
            }
        }
        if !changed {
            return Ok(self.source.bytes.to_vec());
        }
        if self.source.buffers.is_empty() {
            json["buffers"] = json!([{"byteLength": binary.len()}]);
        } else {
            json["buffers"][0]["byteLength"] = json!(binary.len());
        }
        encode_chunks(&self.source.chunks, &json, &binary)
    }
}

fn encode_new_clip(json: &mut Value, binary: &mut Vec<u8>, clip: &Clip) -> Result<Value, String> {
    if clip.channels.is_empty() {
        return Err("新动画至少需要一条通道".into());
    }
    let mut samplers = Vec::with_capacity(clip.channels.len());
    let mut channels = Vec::with_capacity(clip.channels.len());
    let mut targets = HashSet::new();
    for (index, channel) in clip.channels.iter().enumerate() {
        if channel.source_index != index
            || !targets.insert((channel.node, channel.path.as_str()))
            || json["nodes"].get(channel.node).is_none_or(|node| node.get("matrix").is_some())
        {
            return Err("新动画通道身份或目标节点无效".into());
        }
        let (input, output) = append_channel_accessors(json, binary, channel)?;
        samplers.push(json!({"input": input, "output": output, "interpolation": channel.interpolation}));
        channels.push(json!({"sampler": index, "target": {"node": channel.node, "path": channel.path}}));
    }
    Ok(json!({"name": clip.name, "samplers": samplers, "channels": channels}))
}

fn append_channel_accessors(json: &mut Value, binary: &mut Vec<u8>, channel: &Channel) -> Result<(usize, usize), String> {
    channel.validate()?;
    if channel.node >= array(json, "nodes").len() {
        return Err("通道目标节点已不存在".into());
    }
    let times: Vec<f32> = channel.keys.iter().map(|key| key.time).collect();
    let input = append_accessor(json, binary, &times, 1, true)?;
    let mut values = Vec::new();
    for key in &channel.keys {
        if channel.interpolation == "CUBICSPLINE" {
            values.extend_from_slice(&key.in_tangent);
        }
        // 旋转四元数必须在写回前归一化，避免运行时加载器拒绝整份资源。
        if channel.path == "rotation" {
            let length = key.value.iter().map(|value| value * value).sum::<f32>().sqrt();
            if (length - 1.0).abs() > 0.01 {
                return Err("旋转四元数长度必须为 1；请先归一化该关键帧".into());
            }
        }
        values.extend_from_slice(&key.value);
        if channel.interpolation == "CUBICSPLINE" {
            values.extend_from_slice(&key.out_tangent);
        }
    }
    let output = append_accessor(json, binary, &values, channel.dimensions(), false)?;
    Ok((input, output))
}

fn append_accessor(
    json: &mut Value,
    binary: &mut Vec<u8>,
    values: &[f32],
    dimensions: usize,
    time: bool,
) -> Result<usize, String> {
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let start = binary.len();
    for value in values {
        binary.extend_from_slice(&value.to_le_bytes());
    }
    if json.get("bufferViews").is_none() {
        json["bufferViews"] = json!([]);
    }
    let views = json["bufferViews"]
        .as_array_mut()
        .ok_or("bufferViews 不是数组")?;
    let view = views.len();
    views.push(json!({"buffer": 0, "byteOffset": start, "byteLength": values.len() * 4}));
    if json.get("accessors").is_none() {
        json["accessors"] = json!([]);
    }
    let accessors = json["accessors"]
        .as_array_mut()
        .ok_or("accessors 不是数组")?;
    let accessor = accessors.len();
    let mut value = json!({"bufferView": view, "componentType": 5126,
        "count": values.len() / dimensions,
        "type": match dimensions { 1 => "SCALAR", 3 => "VEC3", 4 => "VEC4", _ => return Err("不支持的通道维度".into()) }});
    if time {
        value["min"] = json!([values.first().copied().ok_or("关键帧列表为空")?]);
        value["max"] = json!([values.last().copied().ok_or("关键帧列表为空")?]);
    }
    accessors.push(value);
    Ok(accessor)
}

pub(super) fn encode_chunks(
    chunks: &[Chunk],
    json: &Value,
    binary: &[u8],
) -> Result<Vec<u8>, String> {
    let mut json_bytes =
        serde_json::to_vec(json).map_err(|error| format!("动画 JSON 编码失败：{error}"))?;
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    let mut output = Vec::new();
    output.extend_from_slice(b"glTF");
    output.extend_from_slice(&2u32.to_le_bytes());
    output.extend_from_slice(&0u32.to_le_bytes());
    let needs_bin_chunk = !binary.is_empty() && !chunks.iter().any(|chunk| chunk.kind == BIN_CHUNK);
    for chunk in chunks {
        let bytes = match chunk.kind {
            JSON_CHUNK => json_bytes.as_slice(),
            BIN_CHUNK => binary,
            _ => &chunk.bytes,
        };
        write_chunk(&mut output, chunk.kind, bytes)?;
        if needs_bin_chunk && chunk.kind == JSON_CHUNK {
            // 只有骨架的 GLB 可在创建首个动作时添加紧随 JSON 的 BIN。
            write_chunk(&mut output, BIN_CHUNK, binary)?;
        }
    }
    let length = u32::try_from(output.len()).map_err(|_| "GLB 文件超过 4 GiB")?;
    output[8..12].copy_from_slice(&length.to_le_bytes());
    Ok(output)
}

fn write_chunk(output: &mut Vec<u8>, kind: u32, bytes: &[u8]) -> Result<(), String> {
    let padded_length = bytes
        .len()
        .checked_add(3)
        .map(|length| length / 4 * 4)
        .and_then(|length| u32::try_from(length).ok())
        .ok_or("GLB 数据块超过 4 GiB")?;
    output.extend_from_slice(&padded_length.to_le_bytes());
    output.extend_from_slice(&kind.to_le_bytes());
    output.extend_from_slice(bytes);
    while output.len() % 4 != 0 {
        output.push(0);
    }
    Ok(())
}
