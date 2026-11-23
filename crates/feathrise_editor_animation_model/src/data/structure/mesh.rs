//! 源 GLB 网格的几何摘要及变换应用。仅在后台重写受影响的几何访问器。

use super::{
    GlbStructure, check_static_ancestors, check_unbound, checked_trs, index_field, index_value,
    invert, list, object_at, preserved_local, push, write_trs,
};
use crate::data::{
    reader::{float_array, read_accessor},
    writer::append_accessor,
};
use bevy::prelude::{Mat4, Transform, Vec3, Vec4};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshGeometry {
    pub vertices: usize,
    pub triangles: usize,
    pub size: Option<Vec3>,
}

impl GlbStructure {
    /// 直接读取访问器元数据，不为显示属性解码整份网格。
    pub fn mesh_geometry(&self, index: usize) -> Option<MeshGeometry> {
        let mesh = self.node(index).ok()?.mesh?;
        let primitives = list(object_at(&self.json, "meshes", mesh).ok()?, "primitives").ok()?;
        let mut geometry = MeshGeometry {
            vertices: 0,
            triangles: 0,
            size: None,
        };
        let mut seen = HashSet::new();
        let mut bounds = Some((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)));
        for primitive in primitives {
            let position = index_field(&primitive["attributes"], "POSITION").ok()?;
            let accessor = object_at(&self.json, "accessors", position).ok()?;
            let count = index_field(accessor, "count").ok()?;
            if seen.insert(position) {
                geometry.vertices = geometry.vertices.checked_add(count)?;
                bounds = bounds.and_then(|(low, high)| {
                    let (min, max) = (accessor.get("min")?, accessor.get("max")?);
                    let min = Vec3::from_array(float_array(Some(min), [0.0; 3]).ok()?);
                    let max = Vec3::from_array(float_array(Some(max), [0.0; 3]).ok()?);
                    Some((low.min(min), high.max(max)))
                });
            }
            let corners = primitive.get("indices").map_or(Some(count), |indices| {
                let accessor =
                    object_at(&self.json, "accessors", index_value(indices).ok()?).ok()?;
                index_field(accessor, "count").ok()
            })?;
            geometry.triangles =
                geometry
                    .triangles
                    .checked_add(match primitive["mode"].as_u64().unwrap_or(4) {
                        4 => corners / 3,
                        5 | 6 => corners.saturating_sub(2),
                        _ => 0,
                    })?;
        }
        geometry.size = bounds
            .map(|(low, high)| high - low)
            .filter(|size| size.is_finite());
        (!primitives.is_empty()).then_some(geometry)
    }

    /// 归零指定分量时，把差额折入几何并补偿子节点，保留编辑前的世界形状。
    pub fn bake_mesh_transform(
        &mut self,
        index: usize,
        remaining: Transform,
    ) -> Result<bool, String> {
        let original = self.node_transform(index)?;
        let remaining = checked_trs(remaining)?;
        if original == remaining {
            return Ok(false);
        }
        let node = self.node(index)?;
        let mesh_index = node.mesh.ok_or("请选择源 GLB 内的网格")?;
        let source = object_at(&self.json, "nodes", index)?;
        if source.get("camera").is_some()
            || source.pointer("/extensions/KHR_lights_punctual").is_some()
            || source
                .pointer("/extensions/EXT_mesh_gpu_instancing")
                .is_some()
        {
            return Err("附带相机、灯光或 GPU 实例的网格请在 Blender 中应用变换".into());
        }
        check_unbound(&self.json, &HashSet::from([index]))?;
        check_static_ancestors(&self.json, [index])
            .map_err(|_| "该网格含变换动画，无法直接应用变换")?;
        let bake = invert(remaining.to_matrix())? * original.to_matrix();
        let normals = invert(bake)?.transpose();
        let mirrored = bake.determinant() < 0.0;
        let linear_changed = [bake.x_axis, bake.y_axis, bake.z_axis] != [Vec4::X, Vec4::Y, Vec4::Z];
        let children = node
            .children
            .into_iter()
            .map(|child| Ok((child, preserved_local(&self.json, child, bake)?)))
            .collect::<Result<Vec<_>, String>>()?;

        let mut mesh = object_at(&self.json, "meshes", mesh_index)?.clone();
        let mut draft = self.json.clone();
        let mut binary = self.binary.to_vec();
        let mut remapped = HashMap::new();
        for primitive in mesh["primitives"]
            .as_array_mut()
            .ok_or("GLB 网格缺少图元")?
        {
            if primitive.get("targets").is_some()
                || primitive
                    .pointer("/extensions/KHR_draco_mesh_compression")
                    .is_some()
            {
                return Err("含形态目标或压缩几何的网格请在 Blender 中应用变换".into());
            }
            let position = index_field(&primitive["attributes"], "POSITION")?;
            let vertex_count = index_field(object_at(&self.json, "accessors", position)?, "count")?;
            for (name, dimensions) in [("POSITION", 3), ("NORMAL", 3), ("TANGENT", 4)] {
                if name != "POSITION" && !linear_changed {
                    continue;
                }
                let Some(accessor) = primitive["attributes"].get_mut(name) else {
                    continue;
                };
                let source = index_value(accessor)?;
                let key = (name, source);
                let replacement = if let Some(&replacement) = remapped.get(&key) {
                    replacement
                } else {
                    let info = object_at(&self.json, "accessors", source)?;
                    self.buffer_view(info)?;
                    let mut values = read_accessor(
                        &self.json,
                        std::slice::from_ref(&self.binary),
                        source,
                        dimensions,
                    )
                    .map_err(|error| format!("无法读取网格 {name}：{error}"))?;
                    let (mut min, mut max) =
                        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
                    for vertex in values.chunks_exact_mut(dimensions) {
                        let vector = Vec3::new(vertex[0], vertex[1], vertex[2]);
                        let value = match name {
                            "POSITION" => bake.transform_point3(vector),
                            "NORMAL" => normalized(normals, vector)?,
                            _ => normalized(bake, vector)?,
                        };
                        if !value.is_finite() {
                            return Err("应用后顶点超出浮点范围".into());
                        }
                        vertex[..3].copy_from_slice(&value.to_array());
                        min = min.min(value);
                        max = max.max(value);
                        if dimensions == 4 && mirrored {
                            vertex[3] = -vertex[3];
                        }
                    }
                    let replacement =
                        append_accessor(&mut draft, &mut binary, &values, dimensions, false)?;
                    let info_out = &mut draft["accessors"][replacement];
                    if name == "POSITION" {
                        info_out["min"] = json!(min.to_array());
                        info_out["max"] = json!(max.to_array());
                    }
                    for key in ["name", "extras"] {
                        if let Some(value) = info.get(key) {
                            info_out[key] = value.clone();
                        }
                    }
                    remapped.insert(key, replacement);
                    replacement
                };
                *accessor = json!(replacement);
            }
            if mirrored && let mode @ 4..=6 = primitive["mode"].as_u64().unwrap_or(4) {
                let indices = self.triangle_indices(primitive, vertex_count, mode)?;
                primitive["indices"] = json!(append_indices(&mut draft, &mut binary, &indices)?);
                primitive["mode"] = json!(4);
            }
        }
        let shared = list(&self.json, "nodes")?
            .iter()
            .filter(|node| node["mesh"].as_u64() == Some(mesh_index as u64))
            .take(2)
            .count()
            == 2;
        if shared {
            let new_index = list(&draft, "meshes")?.len();
            push(&mut draft, "meshes", mesh);
            draft["nodes"][index]["mesh"] = json!(new_index);
        } else {
            draft["meshes"][mesh_index] = mesh;
        }
        write_trs(&mut draft["nodes"][index], remaining);
        for (child, pose) in children {
            if let Some(pose) = pose {
                write_trs(&mut draft["nodes"][child], pose);
            }
        }
        draft["buffers"][0]["byteLength"] = json!(binary.len());
        self.json = draft;
        self.binary = binary.into();
        Ok(true)
    }

    fn buffer_view<'a>(&'a self, accessor: &Value) -> Result<&'a Value, String> {
        let view = object_at(
            &self.json,
            "bufferViews",
            index_field(accessor, "bufferView")?,
        )?;
        if view
            .pointer("/extensions/EXT_meshopt_compression")
            .is_some()
        {
            return Err("压缩网格请在 Blender 中应用变换".into());
        }
        Ok(view)
    }

    fn triangle_indices(
        &self,
        primitive: &Value,
        vertices: usize,
        mode: u64,
    ) -> Result<Vec<u32>, String> {
        let indices = if let Some(index) = primitive.get("indices") {
            let accessor = object_at(&self.json, "accessors", index_value(index)?)?;
            let view = self.buffer_view(accessor)?;
            let step = match index_field(accessor, "componentType")? {
                5121 => 1,
                5123 => 2,
                5125 => 4,
                _ => return Err("GLB 三角索引类型无效".into()),
            };
            if accessor.get("sparse").is_some()
                || accessor["type"] != "SCALAR"
                || view.get("byteStride").is_some()
            {
                return Err("此三角索引格式无法应用镜像缩放".into());
            }
            let offset = accessor
                .get("byteOffset")
                .map(index_value)
                .transpose()?
                .unwrap_or(0);
            let base = view
                .get("byteOffset")
                .map(index_value)
                .transpose()?
                .unwrap_or(0);
            let length = index_field(accessor, "count")?
                .checked_mul(step)
                .ok_or("GLB 三角索引过大")?;
            let bytes = self
                .binary
                .get(base..)
                .and_then(|bytes| bytes.get(..index_field(view, "byteLength").ok()?))
                .and_then(|bytes| bytes.get(offset..))
                .and_then(|bytes| bytes.get(..length))
                .ok_or("GLB 三角索引超出缓冲")?;
            bytes
                .chunks_exact(step)
                .map(|chunk| {
                    let mut value = [0; 4];
                    value[..step].copy_from_slice(chunk);
                    u32::from_le_bytes(value)
                })
                .collect::<Vec<_>>()
        } else {
            (0..u32::try_from(vertices).map_err(|_| "GLB 顶点索引越界")?).collect()
        };
        if indices.iter().any(|&index| index as usize >= vertices) || indices.len() < 3 {
            return Err("GLB 三角索引越界或不完整".into());
        }
        // 镜像消去节点行列式的负号，同时翻转绕序和切线手性以保留剔除与法线贴图。
        Ok(match mode {
            4 if indices.len() % 3 == 0 => indices
                .chunks_exact(3)
                .flat_map(|i| [i[0], i[2], i[1]])
                .collect(),
            5 => indices
                .windows(3)
                .enumerate()
                .flat_map(|(step, i)| {
                    if step % 2 == 0 {
                        [i[0], i[2], i[1]]
                    } else {
                        [i[1], i[2], i[0]]
                    }
                })
                .collect(),
            6 => indices[1..]
                .windows(2)
                .flat_map(|i| [indices[0], i[1], i[0]])
                .collect(),
            _ => return Err("GLB 三角索引数量不完整".into()),
        })
    }
}

fn normalized(matrix: Mat4, vector: Vec3) -> Result<Vec3, String> {
    matrix
        .transform_vector3(vector)
        .try_normalize()
        .ok_or_else(|| "应用后网格法线或切线无效".into())
}

fn append_indices(
    json: &mut Value,
    binary: &mut Vec<u8>,
    indices: &[u32],
) -> Result<usize, String> {
    let start = binary.len(); // 位置先写入四字节对齐的 FLOAT 数据。
    let length = indices.len().checked_mul(4).ok_or("GLB 三角索引过大")?;
    start
        .checked_add(length)
        .and_then(|length| u32::try_from(length).ok())
        .ok_or("GLB 缓冲超过 4 GiB")?;
    binary
        .try_reserve(length)
        .map_err(|_| "无法分配 GLB 三角索引缓冲")?;
    for index in indices {
        binary.extend(index.to_le_bytes());
    }
    let view = list(json, "bufferViews")?.len();
    push(
        json,
        "bufferViews",
        json!({"buffer":0,"byteOffset":start,"byteLength":length,"target":34963}),
    );
    let accessor = list(json, "accessors")?.len();
    push(
        json,
        "accessors",
        json!({"bufferView":view,"componentType":5125,"count":indices.len(),"type":"SCALAR"}),
    );
    Ok(accessor)
}
