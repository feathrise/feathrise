//! 受检解析 GLB/GLTF 动画；不支持的通道留在原 JSON 中，不因编辑 TRS 丢失权重等数据。

use super::types::*;
use bevy::math::{Mat4, Quat, Vec3};
use serde_json::Value;
use std::{path::Path, sync::Arc};

pub(super) const JSON_CHUNK: u32 = 0x4e4f534a;
pub(super) const BIN_CHUNK: u32 = 0x004e4942;

impl AnimationDocument {
    pub fn source_path(&self) -> &Path {
        &self.source.path
    }

    /// 只采纳 Scene0 实际蒙皮网格的关节，防止同名空节点或其他场景的骨架被当作模型。
    pub fn scene_skin(
        &self,
    ) -> (Vec<bool>, Vec<(usize, Vec3, Vec3)>) {
        let json = &self.source.json;
        let mut joints = vec![false; self.nodes.len()];
        let mut bounds = Vec::new();
        let index = |value: &Value| value.as_u64().and_then(|value| usize::try_from(value).ok());
        let nodes = array(json, "nodes");
        let scenes = array(json, "scenes");
        let mut stack: Vec<_> = scenes
            .first()
            .into_iter()
            .flat_map(|scene| array(scene, "nodes").iter().filter_map(index))
            .collect();
        let mut seen = vec![false; nodes.len()];
        while let Some(node_index) = stack.pop() {
            let Some(node) = nodes.get(node_index) else {
                continue;
            };
            if std::mem::replace(&mut seen[node_index], true) {
                continue;
            }
            stack.extend(array(node, "children").iter().filter_map(index));
            let (Some(skin), Some(mesh)) = (
                index(&node["skin"]).and_then(|i| array(json, "skins").get(i)),
                index(&node["mesh"]).and_then(|i| array(json, "meshes").get(i)),
            ) else {
                continue;
            };
            for joint in array(skin, "joints").iter().filter_map(index) {
                if let Some(joint) = joints.get_mut(joint) {
                    *joint = true;
                }
            }
            for primitive in array(mesh, "primitives") {
                let Some(position) = index(&primitive["attributes"]["POSITION"])
                    .and_then(|i| array(json, "accessors").get(i))
                else {
                    continue;
                };
                let (Some(minimum), Some(maximum)) = (position.get("min"), position.get("max")) else {
                    continue;
                };
                // 取景只读取 accessor 元数据，网格顶点仍交给原 Bevy 资源加载链。
                if let (Ok(minimum), Ok(maximum)) = (
                    float_array(Some(minimum), [0.0; 3]),
                    float_array(Some(maximum), [0.0; 3]),
                ) {
                    bounds.push((node_index, Vec3::from_array(minimum), Vec3::from_array(maximum)));
                }
            }
        }
        (joints, bounds)
    }

    /// 文件 IO 与完整解析随资源规模增长，调用方必须从唯一后台 IO 槽进入。
    pub fn load(path: &Path) -> Result<Self, String> {
        let path = path
            .canonicalize()
            .map_err(|error| format!("无法打开动画文件：{error}"))?;
        let bytes = std::fs::read(&path).map_err(|error| format!("读取动画文件失败：{error}"))?;
        Self::from_bytes(&path, bytes)
    }

    pub(super) fn from_bytes(path: &Path, bytes: Vec<u8>) -> Result<Self, String> {
        let glb = bytes.starts_with(b"glTF");
        let chunks = if glb {
            read_chunks(&bytes)?
        } else {
            Vec::new()
        };
        let json_bytes = if glb {
            chunks
                .iter()
                .find(|chunk| chunk.kind == JSON_CHUNK)
                .map(|chunk| chunk.bytes.as_ref())
                .ok_or("GLB 缺少 JSON 数据块")?
        } else {
            bytes.as_slice()
        };
        let json: Value = serde_json::from_slice(json_bytes)
            .map_err(|error| format!("动画 JSON 无效：{error}"))?;
        if json.pointer("/asset/version").and_then(Value::as_str) != Some("2.0") {
            return Err("动画编辑器只支持 glTF 2.0".into());
        }
        let mut buffers = Vec::new();
        for (index, buffer) in array(&json, "buffers").iter().enumerate() {
            let data = match buffer.get("uri").and_then(Value::as_str) {
                Some(uri) => read_external_buffer(path, uri)?,
                None if glb && index == 0 => chunks
                    .iter()
                    .find(|chunk| chunk.kind == BIN_CHUNK)
                    .map(|chunk| chunk.bytes.clone())
                    .ok_or("GLB 缺少 BIN 数据块")?,
                None => return Err(format!("buffer {index} 缺少有效数据源")),
            };
            let length = index_field(buffer, "byteLength")?;
            if length > data.len() {
                return Err(format!("buffer {index} 数据短于声明长度"));
            }
            buffers.push(data);
        }
        let nodes = read_nodes(&json)?;
        let mut clips = Vec::new();
        let mut warnings = Vec::new();
        for (clip_index, animation) in array(&json, "animations").iter().enumerate() {
            let name = animation
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("动画 {}", clip_index + 1));
            let mut channels = Vec::new();
            for (channel_index, channel) in array(animation, "channels").iter().enumerate() {
                match read_channel(&json, &buffers, &nodes, animation, channel, channel_index) {
                    Ok(channel) => channels.push(channel),
                    Err(error) => warnings.push(format!(
                        "{name} · 通道 {}只读保留：{error}",
                        channel_index + 1
                    )),
                }
            }
            clips.push(Clip {
                name,
                channels,
                source_index: clip_index,
            });
        }
        let read_only =
            !glb || array(&json, "buffers").len() != 1 || json.pointer("/buffers/0/uri").is_some();
        if read_only {
            warnings.push(
                "此文件含外置 GLTF/buffer，仅支持查看；可在 Blender 导出独立 GLB 后编辑保存".into(),
            );
        }
        Ok(Self {
            nodes,
            warnings,
            read_only,
            source_identity: Arc::new(()),
            source: Arc::new(Source {
                path: path.to_owned(),
                bytes: bytes.into(),
                json,
                buffers,
                chunks,
                original_clips: clips.clone(),
            }),
            clips,
        })
    }
}

pub(super) fn array<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[])
}

pub(super) fn index_field(value: &Value, name: &str) -> Result<usize, String> {
    value
        .get(name)
        .and_then(Value::as_u64)
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| format!("缺少合法 {name}"))
}

fn read_chunks(bytes: &[u8]) -> Result<Vec<Chunk>, String> {
    if bytes.len() < 12 || read_u32(bytes, 4)? != 2 || read_u32(bytes, 8)? as usize != bytes.len() {
        return Err("GLB 文件头、版本或总长度无效".into());
    }
    let mut chunks = Vec::new();
    let mut offset = 12usize;
    while offset < bytes.len() {
        let length = read_u32(bytes, offset)? as usize;
        let kind = read_u32(bytes, offset + 4)?;
        if length % 4 != 0 {
            return Err("GLB 数据块未按四字节对齐".into());
        }
        let start = offset.checked_add(8).ok_or("GLB 数据块偏移溢出")?;
        let end = start.checked_add(length).ok_or("GLB 数据块长度溢出")?;
        let data = bytes.get(start..end).ok_or("GLB 数据块超出文件边界")?;
        if chunks.is_empty() && kind != JSON_CHUNK {
            return Err("GLB 第一块必须为 JSON".into());
        }
        if chunks.iter().any(|chunk: &Chunk| chunk.kind == kind)
            && matches!(kind, JSON_CHUNK | BIN_CHUNK)
        {
            return Err("GLB 存在重复 JSON/BIN 数据块".into());
        }
        chunks.push(Chunk {
            kind,
            bytes: Arc::from(data),
        });
        offset = end;
    }
    Ok(chunks)
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset.checked_add(4).ok_or("GLB 偏移溢出")?;
    let data = bytes.get(offset..end).ok_or("GLB 数据块头不完整")?;
    Ok(u32::from_le_bytes(
        data.try_into().map_err(|_| "GLB 字段长度无效")?,
    ))
}

fn read_external_buffer(path: &Path, uri: &str) -> Result<Arc<[u8]>, String> {
    // 不访问网络，也不把 URI 的查询、片段或 data 内容误当成本地文件名。
    if uri.contains(':') || uri.contains('?') || uri.contains('#') || uri.contains('\\') {
        return Err(
            "仅支持 GLB 内嵌 buffer 或 GLTF 的本地相对 BIN；data/网络 URI 请先转为 GLB".into(),
        );
    }
    let mut decoded = Vec::new();
    let mut bytes = uri.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = bytes
                .next()
                .and_then(|byte| (byte as char).to_digit(16))
                .ok_or("buffer URI 转义无效")?;
            let low = bytes
                .next()
                .and_then(|byte| (byte as char).to_digit(16))
                .ok_or("buffer URI 转义无效")?;
            decoded.push((high * 16 + low) as u8);
        } else {
            decoded.push(byte);
        }
    }
    let relative = std::str::from_utf8(&decoded).map_err(|_| "buffer URI 不是 UTF-8")?;
    let relative = Path::new(relative);
    if relative.is_absolute()
        || relative.components().any(|part| {
            matches!(
                part,
                std::path::Component::Prefix(_) | std::path::Component::RootDir
            )
        })
    {
        return Err("GLTF buffer 必须使用本地相对路径".into());
    }
    std::fs::read(path.parent().unwrap_or(Path::new(".")).join(relative))
        .map(Arc::from)
        .map_err(|error| format!("读取 GLTF 外置 BIN 失败：{error}"))
}

fn float_array<const N: usize>(
    value: Option<&Value>,
    fallback: [f32; N],
) -> Result<[f32; N], String> {
    let Some(value) = value else {
        return Ok(fallback);
    };
    let values = value
        .as_array()
        .filter(|values| values.len() == N)
        .ok_or("节点变换维度无效")?;
    let mut output = [0.0; N];
    for (slot, value) in output.iter_mut().zip(values) {
        *slot = value
            .as_f64()
            .map(|value| value as f32)
            .filter(|value| value.is_finite())
            .ok_or("节点变换包含无效数字")?;
    }
    Ok(output)
}

fn read_nodes(json: &Value) -> Result<Vec<Node>, String> {
    let source_nodes = array(json, "nodes");
    let mut nodes = Vec::with_capacity(source_nodes.len());
    for (index, node) in source_nodes.iter().enumerate() {
        let (scale, rotation, translation) = if node.get("matrix").is_some() {
            let matrix = Mat4::from_cols_array(&float_array(
                node.get("matrix"),
                Mat4::IDENTITY.to_cols_array(),
            )?);
            // 矩阵节点的作者源仍保留原 matrix；此分解仅用于骨架可视化。
            if matrix.determinant().abs() <= f32::EPSILON {
                return Err(format!("节点 {index} 的矩阵不可分解"));
            }
            matrix.to_scale_rotation_translation()
        } else {
            (
                Vec3::from_array(float_array(node.get("scale"), [1.0; 3])?),
                Quat::from_array(float_array(node.get("rotation"), [0.0, 0.0, 0.0, 1.0])?),
                Vec3::from_array(float_array(node.get("translation"), [0.0; 3])?),
            )
        };
        if !translation.is_finite()
            || !rotation.is_finite()
            || !scale.is_finite()
            || !rotation.length_squared().is_finite()
            || rotation.length_squared() <= f32::EPSILON
        {
            return Err(format!("节点 {index} 包含无效变换"));
        }
        nodes.push(Node {
            name: node
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("节点 {index}")),
            parent: None,
            translation: translation.to_array(),
            rotation: rotation.normalize().to_array(),
            scale: scale.to_array(),
        });
    }
    for (parent, node) in source_nodes.iter().enumerate() {
        for child in array(node, "children") {
            let child = child
                .as_u64()
                .and_then(|index| usize::try_from(index).ok())
                .ok_or("节点 children 索引无效")?;
            let node = nodes.get_mut(child).ok_or("节点 children 超出范围")?;
            if node.parent.replace(parent).is_some() || child == parent {
                return Err("节点层级含重复父级或循环".into());
            }
        }
    }
    // 线性三色校验避免深骨架递归栈溢出，也避免逐节点追父导致 O(n²)。
    let mut colors = vec![0u8; nodes.len()];
    for start in 0..nodes.len() {
        let mut path = Vec::new();
        let mut cursor = Some(start);
        while let Some(index) = cursor {
            match colors[index] {
                1 => return Err("节点层级包含循环".into()),
                2 => break,
                _ => {
                    colors[index] = 1;
                    path.push(index);
                    cursor = nodes[index].parent;
                }
            }
        }
        for index in path {
            colors[index] = 2;
        }
    }
    Ok(nodes)
}

fn read_channel(
    json: &Value,
    buffers: &[Arc<[u8]>],
    nodes: &[Node],
    animation: &Value,
    channel: &Value,
    source_index: usize,
) -> Result<Channel, String> {
    let target = channel.get("target").ok_or("缺少 target")?;
    let path = target
        .get("path")
        .and_then(Value::as_str)
        .ok_or("缺少目标属性")?;
    let dimensions = match path {
        "translation" | "scale" => 3,
        "rotation" => 4,
        _ => return Err(format!("不支持 {path} 属性")),
    };
    let node = index_field(target, "node")?;
    let node_name = nodes.get(node).ok_or("目标节点不存在")?.name.clone();
    let sampler = array(animation, "samplers")
        .get(index_field(channel, "sampler")?)
        .ok_or("sampler 不存在")?;
    let interpolation = sampler
        .get("interpolation")
        .and_then(Value::as_str)
        .unwrap_or("LINEAR");
    if !matches!(interpolation, "STEP" | "LINEAR" | "CUBICSPLINE") {
        return Err(format!("不支持插值 {interpolation}"));
    }
    let times = read_accessor(json, buffers, index_field(sampler, "input")?, 1)?;
    let values = read_accessor(json, buffers, index_field(sampler, "output")?, dimensions)?;
    let multiplier = if interpolation == "CUBICSPLINE" { 3 } else { 1 };
    if times.len().checked_mul(dimensions * multiplier) != Some(values.len()) {
        return Err("关键帧时间与输出数量不一致".into());
    }
    let mut keys = Vec::with_capacity(times.len());
    for (index, time) in times.into_iter().enumerate() {
        let start = index * dimensions * multiplier;
        let value_start = start + if multiplier == 3 { dimensions } else { 0 };
        keys.push(Keyframe {
            time,
            value: values[value_start..value_start + dimensions].to_vec(),
            in_tangent: if multiplier == 3 {
                values[start..start + dimensions].to_vec()
            } else {
                Vec::new()
            },
            out_tangent: if multiplier == 3 {
                values[start + 2 * dimensions..start + 3 * dimensions].to_vec()
            } else {
                Vec::new()
            },
        });
    }
    let result = Channel {
        node,
        path: path.into(),
        node_name,
        interpolation: interpolation.into(),
        keys,
        source_index,
    };
    result.validate()?;
    Ok(result)
}

fn read_accessor(
    json: &Value,
    buffers: &[Arc<[u8]>],
    index: usize,
    dimensions: usize,
) -> Result<Vec<f32>, String> {
    let accessor = array(json, "accessors")
        .get(index)
        .ok_or("accessor 不存在")?;
    if accessor.get("sparse").is_some()
        || accessor.get("normalized").and_then(Value::as_bool) == Some(true)
        || accessor.get("componentType").and_then(Value::as_u64) != Some(5126)
    {
        return Err("仅可编辑非 sparse、非 normalized 的 FLOAT accessor".into());
    }
    let expected_type = match dimensions {
        1 => "SCALAR",
        3 => "VEC3",
        4 => "VEC4",
        _ => unreachable!(),
    };
    if accessor.get("type").and_then(Value::as_str) != Some(expected_type) {
        return Err("accessor 维度与目标属性不一致".into());
    }
    let count = index_field(accessor, "count")?;
    if count == 0 {
        return Err("关键帧数量为零".into());
    }
    let view = array(json, "bufferViews")
        .get(index_field(accessor, "bufferView")?)
        .ok_or("bufferView 不存在")?;
    let buffer_index = index_field(view, "buffer")?;
    let buffer = buffers.get(buffer_index).ok_or("buffer 不存在")?;
    let declared_length = array(json, "buffers")
        .get(buffer_index)
        .map(|buffer| index_field(buffer, "byteLength"))
        .transpose()?
        .ok_or("buffer 声明不存在")?;
    let view_offset = view
        .get("byteOffset")
        .map(|_| index_field(view, "byteOffset"))
        .transpose()?
        .unwrap_or(0);
    let accessor_offset = accessor
        .get("byteOffset")
        .map(|_| index_field(accessor, "byteOffset"))
        .transpose()?
        .unwrap_or(0);
    let view_length = index_field(view, "byteLength")?;
    let packed = dimensions * 4;
    let stride = view
        .get("byteStride")
        .map(|_| index_field(view, "byteStride"))
        .transpose()?
        .unwrap_or(packed);
    if stride < packed || stride % 4 != 0 || accessor_offset % 4 != 0 || view_offset % 4 != 0 {
        return Err("FLOAT accessor 对齐或步长无效".into());
    }
    let span = (count - 1)
        .checked_mul(stride)
        .and_then(|span| span.checked_add(packed))
        .ok_or("accessor 数量溢出")?;
    let local_end = accessor_offset
        .checked_add(span)
        .ok_or("accessor 偏移溢出")?;
    let view_end = view_offset
        .checked_add(view_length)
        .ok_or("bufferView 偏移溢出")?;
    if local_end > view_length || view_end > declared_length || view_end > buffer.len() {
        return Err("accessor 超出 bufferView/buffer 边界".into());
    }
    let total = count.checked_mul(dimensions).ok_or("accessor 数量溢出")?;
    let mut result = Vec::with_capacity(total);
    for element in 0..count {
        let start = view_offset + accessor_offset + element * stride;
        for component in 0..dimensions {
            let offset = start + component * 4;
            let value = f32::from_le_bytes(
                buffer[offset..offset + 4]
                    .try_into()
                    .map_err(|_| "FLOAT 数据不完整")?,
            );
            if !value.is_finite() {
                return Err("关键帧包含非有限数字".into());
            }
            result.push(value);
        }
    }
    Ok(result)
}
