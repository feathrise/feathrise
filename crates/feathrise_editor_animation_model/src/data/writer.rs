//! 追加式 GLB 编码与精确版本原子保存；原 buffer、未知字段/块及未编辑通道保留。

use super::{
    reader::{BIN_CHUNK, JSON_CHUNK, array},
    types::*,
};
use feathrise_scene_content::authoring_transaction::{PreparedWrite, commit_prepared_writes_checked};
use serde_json::{Value, json};
use std::{collections::HashSet, path::Path};

impl AnimationDocument {
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
        let expected = self.encode()?;
        self.save(path)?;
        let mut saved = Self::load(path)?;
        // 保存与重读之间也可能发生外部导出，不能把另一份文件当成本次草稿的版本基线。
        if saved.source.bytes.as_ref() != expected.as_slice() {
            return Err("动画保存后文件再次被外部修改，原草稿已保留，请重新载入后处理冲突".into());
        }
        saved.clips = self.clips.clone();
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
        if bytes.as_slice() == self.source.bytes.as_ref() {
            return Ok(());
        }
        commit_prepared_writes_checked(
            vec![PreparedWrite::new(canonical.clone(), bytes)],
            |_, _| verify(),
        )
        .map_err(|error| format!("动画保存失败：{}", error.message()))
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, String> {
        if self.read_only {
            return Err("仅支持保存独立 GLB 文件".into());
        }
        let mut json = self.source.json.clone();
        let mut binary = self
            .source
            .buffers
            .first()
            .ok_or("动画缺少 BIN 数据")?
            .to_vec();
        let mut changed = false;
        let mut clip_indices = HashSet::new();
        if self.clips.len() != self.source.original_clips.len() {
            return Err("首版编辑器不支持添加或删除动画片段".into());
        }
        for clip in &self.clips {
            if !clip_indices.insert(clip.source_index) {
                return Err("动画片段身份重复".into());
            }
            let original = self
                .source
                .original_clips
                .get(clip.source_index)
                .ok_or("动画片段已不存在")?;
            let source_animation = array(&self.source.json, "animations")
                .get(clip.source_index)
                .ok_or("原动画片段不存在")?;
            if clip.channels.len() != original.channels.len() {
                return Err("首版编辑器不支持添加或删除通道；未知通道始终原样保留".into());
            }
            if clip.name != original.name {
                json["animations"][clip.source_index]["name"] = Value::String(clip.name.clone());
                changed = true;
            }
            let mut channel_indices = HashSet::new();
            for channel in &clip.channels {
                if !channel_indices.insert(channel.source_index) {
                    return Err("动画通道身份重复".into());
                }
                let original_index = original
                    .channels
                    .binary_search_by_key(&channel.source_index, |candidate| candidate.source_index)
                    .map_err(|_| "原动画通道不存在")?;
                let original = &original.channels[original_index];
                if channel == original {
                    continue;
                }
                channel.validate()?;
                if channel.node >= self.nodes.len() {
                    return Err("通道目标节点已不存在".into());
                }
                let times: Vec<f32> = channel.keys.iter().map(|key| key.time).collect();
                let input = append_accessor(&mut json, &mut binary, &times, 1, true)?;
                let mut values = Vec::new();
                for key in &channel.keys {
                    if channel.interpolation == "CUBICSPLINE" {
                        values.extend_from_slice(&key.in_tangent);
                    }
                    // 四元数是一个整体，修改单分量后保存前要求合法长度，避免运行时加载器断言。
                    if channel.path == "rotation" {
                        let length = key
                            .value
                            .iter()
                            .map(|value| value * value)
                            .sum::<f32>()
                            .sqrt();
                        if (length - 1.0).abs() > 0.01 {
                            return Err("旋转四元数长度必须为 1；请先归一化该关键帧".into());
                        }
                    }
                    values.extend_from_slice(&key.value);
                    if channel.interpolation == "CUBICSPLINE" {
                        values.extend_from_slice(&key.out_tangent);
                    }
                }
                let output =
                    append_accessor(&mut json, &mut binary, &values, channel.dimensions(), false)?;
                let source_channel = &source_animation["channels"][channel.source_index];
                let sampler_index = source_channel
                    .get("sampler")
                    .and_then(Value::as_u64)
                    .ok_or("原 sampler 索引无效")? as usize;
                let mut sampler = source_animation["samplers"][sampler_index].clone();
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
                animation["channels"][channel.source_index]["sampler"] = json!(next_sampler);
                animation["channels"][channel.source_index]["target"]["node"] = json!(channel.node);
                animation["channels"][channel.source_index]["target"]["path"] = json!(channel.path);
                changed = true;
            }
        }
        if !changed {
            return Ok(self.source.bytes.to_vec());
        }
        json["buffers"][0]["byteLength"] = json!(binary.len());
        encode_chunks(&self.source.chunks, &json, &binary)
    }
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
    for chunk in chunks {
        let bytes = match chunk.kind {
            JSON_CHUNK => json_bytes.as_slice(),
            BIN_CHUNK => binary,
            _ => &chunk.bytes,
        };
        let padded_length = bytes
            .len()
            .checked_add(3)
            .map(|length| length / 4 * 4)
            .and_then(|length| u32::try_from(length).ok())
            .ok_or("GLB 数据块超过 4 GiB")?;
        output.extend_from_slice(&padded_length.to_le_bytes());
        output.extend_from_slice(&chunk.kind.to_le_bytes());
        output.extend_from_slice(bytes);
        while output.len() % 4 != 0 {
            output.push(0);
        }
    }
    let length = u32::try_from(output.len()).map_err(|_| "GLB 文件超过 4 GiB")?;
    output[8..12].copy_from_slice(&length.to_le_bytes());
    Ok(output)
}
