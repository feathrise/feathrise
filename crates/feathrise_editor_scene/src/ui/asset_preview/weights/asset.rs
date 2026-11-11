//! 后台读取 Scene0 蒙皮，保留 GLB 原始载荷并准备可独立渲染的草稿网格。

use super::{
    influences::{VertexWeights, opposite_name},
    spatial::{MirrorPlane, Occlusion, Points},
};
use crate::ui::asset_preview::rigging::files::{FrozenAsset, active};
use bevy::{
    asset::RenderAssetUsages,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
    prelude::*,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    ops::Range,
    sync::{Arc, atomic::AtomicBool},
};

pub(super) const MAX_VERTICES: usize = 500_000;
const MAX_TRIANGLES: usize = 1_000_000;
const MAX_NODES: usize = 8_192;
const MAX_SURFACES: usize = 128;
const MAX_JOINTS: usize = 256;
pub(super) const MAX_FILE: usize = 256 * 1024 * 1024;
const MAX_JSON: usize = 16 * 1024 * 1024;

pub(super) struct Joint {
    pub node: usize,
    pub name: String,
    pub opposite: Option<u16>,
    pub rest: Vec3,
}

pub(super) struct Skin {
    pub index: usize,
    pub joints: Vec<Joint>,
    pub mirror: Option<MirrorPlane>,
    matrices: Vec<Mat4>,
}

pub(super) struct Surface {
    pub mesh: usize,
    pub primitive: usize,
    pub name: String,
    pub skin: Arc<Skin>,
    pub points: Points,
    pub original: Arc<[VertexWeights]>,
    base: Mesh,
}

impl Surface {
    pub fn geometry(&self) -> (&[[f32; 3]], &[u32]) {
        let Some(VertexAttributeValues::Float32x3(positions)) =
            self.base.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            unreachable!("基础网格保留了原图元顶点次序")
        };
        let Some(Indices::U32(indices)) = self.base.indices() else {
            unreachable!("基础网格保留了三角形索引")
        };
        (positions, indices)
    }

    pub fn mesh(&self, weights: &[VertexWeights], colors: Option<Vec<[f32; 4]>>) -> Mesh {
        let mut mesh = self.base.clone();
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(weights.iter().map(|vertex| vertex.joints).collect()),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_WEIGHT,
            VertexAttributeValues::Float32x4(weights.iter().map(|vertex| vertex.weights).collect()),
        );
        if let Some(colors) = colors {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        }
        mesh
    }
}

/// 刚性附件的局部几何与静置遮挡分开保留，动作帧按实际实例变换重建遮挡。
pub(super) struct RigidSurface {
    pub mesh: usize,
    pub primitive: usize,
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Geometry {
    Skin(usize),
    Rigid(usize),
}

pub(super) struct Document {
    pub identity: FrozenAsset,
    pub source: Vec<u8>,
    pub json: Value,
    pub binary: Range<usize>,
    pub surfaces: Vec<Surface>,
    pub rigid: Vec<RigidSurface>,
    pub occlusion: Occlusion,
    pub extent: f32,
}

pub(super) fn chunks(bytes: &[u8]) -> Result<(Range<usize>, Range<usize>), String> {
    let word = |offset: usize| -> Option<usize> {
        Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?) as usize)
    };
    if bytes.len() > MAX_FILE
        || bytes.get(..4) != Some(b"glTF")
        || word(4) != Some(2)
        || word(8) != Some(bytes.len())
        || bytes.get(16..20) != Some(b"JSON")
    {
        return Err("请选择 256 MiB 内的 GLB 2.0 模型".into());
    }
    let json_len = word(12)
        .filter(|&len| len <= MAX_JSON && len % 4 == 0)
        .ok_or("GLB JSON 块长度无效")?;
    let next = 20 + json_len;
    if bytes.get(next + 4..next + 8) != Some(b"BIN\0") {
        return Err("权重编辑需要内嵌的 GLB 二进制数据".into());
    }
    let binary_start = next + 8;
    let len = word(next)
        .filter(|&len| len % 4 == 0 && len <= bytes.len() - binary_start)
        .ok_or("GLB 二进制块长度无效")?;
    Ok((20..next, binary_start..binary_start + len))
}

fn worlds(doc: &gltf::Document) -> Result<Vec<Option<Mat4>>, String> {
    let scene = doc.scenes().next().ok_or("GLB 缺少 Scene0")?;
    let mut matrices = vec![None; doc.nodes().len()];
    if matrices.len() > MAX_NODES {
        return Err("GLB 节点数超过权重编辑预算".into());
    }
    let mut queue: Vec<_> = scene.nodes().map(|node| (node, Mat4::IDENTITY)).collect();
    while let Some((node, parent)) = queue.pop() {
        let matrix = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        let slot = &mut matrices[node.index()];
        if slot.replace(matrix).is_some() || !matrix.is_finite() {
            return Err("Scene0 节点层级或变换无效".into());
        }
        queue.extend(node.children().map(|child| (child, matrix)));
    }
    Ok(matrices)
}

fn make_skin(source: gltf::Skin<'_>, world: &[Option<Mat4>], bin: &[u8]) -> Result<Skin, String> {
    let nodes: Vec<_> = source.joints().collect();
    if nodes.is_empty() || nodes.len() > MAX_JOINTS {
        return Err("单套蒙皮需要 1–256 个关节".into());
    }
    let bindings = source
        .reader(|buffer| (buffer.index() == 0).then_some(bin))
        .read_inverse_bind_matrices()
        .map(|values| {
            values
                .map(|matrix| Mat4::from_cols_array_2d(&matrix))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![Mat4::IDENTITY; nodes.len()]);
    if bindings.len() != nodes.len() {
        return Err("逆绑定矩阵与关节数量不一致".into());
    }
    let mut joints: Vec<_> = nodes
        .iter()
        .map(|node| Joint {
            node: node.index(),
            name: node
                .name()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Joint {}", node.index())),
            opposite: None,
            rest: Vec3::ZERO,
        })
        .collect();
    let mut names = BTreeMap::new();
    for (index, joint) in joints.iter().enumerate() {
        names
            .entry(joint.name.to_lowercase())
            .and_modify(|entry| *entry = None)
            .or_insert(Some(index as u16));
    }
    for joint in &mut joints {
        joint.opposite =
            opposite_name(&joint.name).and_then(|name| names.get(&name).copied().flatten());
    }
    let positions: Vec<_> = nodes
        .iter()
        .map(|node| {
            world[node.index()]
                .map(|matrix| matrix.transform_point3(Vec3::ZERO))
                .ok_or("蒙皮关节不在 Scene0 内")
        })
        .collect::<Result<_, _>>()?;
    for (joint, &position) in joints.iter_mut().zip(&positions) {
        joint.rest = position;
    }
    let matrices: Vec<_> = nodes
        .iter()
        .zip(bindings)
        .map(|(node, bind)| world[node.index()].expect("已核对 Scene0 关节") * bind)
        .collect();
    if matrices.iter().any(|matrix| !matrix.is_finite()) {
        return Err("蒙皮矩阵包含无效数值".into());
    }
    // 优先用双臂或双肩恢复角色自身的中面，避免把 GLB 世界 X 轴误认成人物左右。
    let pair = joints
        .iter()
        .enumerate()
        .filter_map(|(index, joint)| {
            let other = joint.opposite? as usize;
            if index >= other || joints[other].opposite != Some(index as u16) {
                return None;
            }
            let a = positions[index];
            let b = positions[other];
            let name = joint.name.to_lowercase();
            let priority = if name.ends_with("leftarm")
                || name.ends_with("rightarm")
                || name.contains("shoulder")
            {
                1
            } else {
                0
            };
            Some((priority, a.distance_squared(b), a, b))
        })
        .max_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mirror = pair.and_then(|(_, _, a, b)| {
        (a - b).with_y(0.0).try_normalize().map(|axis| MirrorPlane {
            center: a.midpoint(b),
            axis,
        })
    });
    Ok(Skin {
        index: source.index(),
        joints,
        mirror,
        matrices,
    })
}

pub(super) fn decode(
    identity: FrozenAsset,
    source: Vec<u8>,
    cancel: &AtomicBool,
) -> Result<Arc<Document>, String> {
    active(cancel)?;
    let (json_chunk, binary) = chunks(&source)?;
    let json: Value =
        serde_json::from_slice(&source[json_chunk]).map_err(|error| error.to_string())?;
    let gltf = gltf::Gltf::from_slice(&source).map_err(|error| error.to_string())?;
    let doc = &gltf.document;
    if doc.buffers().len() != 1
        || !doc
            .buffers()
            .all(|buffer| matches!(buffer.source(), gltf::buffer::Source::Bin))
    {
        return Err("权重编辑需要全部几何位于 GLB 的 BIN 块".into());
    }
    let bin = &source[binary.clone()];
    let world = worlds(doc)?;
    let mut surfaces = Vec::new();
    let mut rigid = Vec::new();
    let mut rigid_meshes = HashSet::new();
    let mut skins = BTreeMap::new();
    let mut triangles = Vec::new();
    let mut vertices = 0;
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = -minimum;
    let mut shared = HashSet::new();
    let mut used = HashSet::new();
    for node in doc.nodes().filter(|node| world[node.index()].is_some()) {
        if let Some(mesh) = node.mesh()
            && !used.insert(mesh.index())
        {
            shared.insert(mesh.index());
        }
    }
    for node in doc.nodes() {
        let (Some(matrix), Some(mesh)) = (world[node.index()], node.mesh()) else {
            continue;
        };
        let skin = if let Some(skin) = node.skin() {
            if shared.contains(&mesh.index()) {
                return Err("同一网格被多个 Scene0 节点复用，请先拆分蒙皮网格".into());
            }
            let index = skin.index();
            if !skins.contains_key(&index) {
                skins.insert(index, Arc::new(make_skin(skin, &world, bin)?));
            }
            Some(skins[&index].clone())
        } else {
            None
        };
        for primitive in mesh.primitives() {
            active(cancel)?;
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                if skin.is_some() {
                    return Err("权重编辑只支持三角形蒙皮图元".into());
                }
                continue;
            }
            let position_count = primitive
                .get(&gltf::Semantic::Positions)
                .map_or(0, |accessor| accessor.count());
            vertices += position_count;
            if position_count == 0 || vertices > MAX_VERTICES || surfaces.len() >= MAX_SURFACES {
                return Err("Scene0 顶点或蒙皮图元超出权重编辑预算".into());
            }
            let reader = primitive.reader(|buffer| (buffer.index() == 0).then_some(bin));
            let positions: Vec<_> = reader
                .read_positions()
                .ok_or("GLB 位置数据不可读")?
                .collect();
            if positions.len() != position_count
                || positions
                    .iter()
                    .any(|point| !Vec3::from_array(*point).is_finite())
            {
                return Err("GLB 顶点位置无效".into());
            }
            let index_count = primitive
                .indices()
                .map_or(position_count, |indices| indices.count());
            if index_count % 3 != 0
                || index_count == 0
                || triangles.len() + index_count / 3 > MAX_TRIANGLES
            {
                return Err("Scene0 三角形超出预算或索引不完整".into());
            }
            let indices: Vec<u32> = reader.read_indices().map_or_else(
                || (0..position_count as u32).collect(),
                |indices| indices.into_u32().collect(),
            );
            if indices.len() != index_count
                || indices
                    .iter()
                    .any(|&index| index as usize >= position_count)
            {
                return Err("GLB 图元索引越界".into());
            }
            if let Some(skin) = &skin {
                if primitive.attributes().any(|(kind, _)| {
                    matches!(
                        kind,
                        gltf::Semantic::Joints(1..) | gltf::Semantic::Weights(1..)
                    )
                }) || primitive.morph_targets().len() > 0
                {
                    return Err("此模型含超过四组顶点影响或形态键，当前只可预览".into());
                }
                let joints = reader.read_joints(0).ok_or("缺少 JOINTS_0")?.into_u16();
                let weights = reader.read_weights(0).ok_or("缺少 WEIGHTS_0")?.into_f32();
                let weights: Arc<[VertexWeights]> = joints
                    .zip(weights)
                    .map(|(joints, weights)| {
                        VertexWeights::checked(joints, weights, skin.joints.len())
                            .ok_or("顶点蒙皮权重或关节索引无效")
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .into();
                if weights.len() != positions.len() {
                    return Err("顶点位置与蒙皮权重数量不同".into());
                }
                // 绑定姿势拾取与 glTF 蒙皮共用关节世界变换和逆绑定矩阵。
                let points: Vec<_> = positions
                    .iter()
                    .zip(weights.iter())
                    .map(|(&position, row)| {
                        let point = Vec3::from_array(position);
                        row.joints
                            .into_iter()
                            .zip(row.weights)
                            .filter(|(_, weight)| *weight > 0.0)
                            .map(|(joint, weight)| {
                                skin.matrices[joint as usize].transform_point3(point) * weight
                            })
                            .sum::<Vec3>()
                    })
                    .collect();
                add_triangles(
                    &mut triangles,
                    &points,
                    &indices,
                    &mut minimum,
                    &mut maximum,
                )?;
                let spatial = Points::new(points, &indices);
                let mut base = Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::RENDER_WORLD,
                );
                base.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
                base.insert_indices(Indices::U32(indices));
                if let Some(normals) = reader.read_normals() {
                    base.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals.collect::<Vec<_>>());
                } else {
                    base.compute_normals();
                }
                if let Some(tangents) = reader.read_tangents() {
                    base.insert_attribute(Mesh::ATTRIBUTE_TANGENT, tangents.collect::<Vec<_>>());
                }
                for (slot, attribute) in [(0, Mesh::ATTRIBUTE_UV_0), (1, Mesh::ATTRIBUTE_UV_1)] {
                    if let Some(uv) = reader.read_tex_coords(slot) {
                        base.insert_attribute(attribute, uv.into_f32().collect::<Vec<_>>());
                    }
                }
                if base.attribute(Mesh::ATTRIBUTE_TANGENT).is_none()
                    && base.attribute(Mesh::ATTRIBUTE_UV_0).is_some()
                {
                    let _ = base.generate_tangents();
                }
                if let Some(colors) = reader.read_colors(0) {
                    base.insert_attribute(
                        Mesh::ATTRIBUTE_COLOR,
                        colors.into_rgba_f32().collect::<Vec<_>>(),
                    );
                }
                surfaces.push(Surface {
                    mesh: mesh.index(),
                    primitive: primitive.index(),
                    name: mesh
                        .name()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("Mesh {}", mesh.index())),
                    skin: skin.clone(),
                    points: spatial,
                    original: weights,
                    base,
                });
            } else {
                let points: Vec<_> = positions
                    .iter()
                    .map(|&point| matrix.transform_point3(Vec3::from_array(point)))
                    .collect();
                add_triangles(
                    &mut triangles,
                    &points,
                    &indices,
                    &mut minimum,
                    &mut maximum,
                )?;
                if rigid_meshes.insert((mesh.index(), primitive.index())) {
                    rigid.push(RigidSurface {
                        mesh: mesh.index(),
                        primitive: primitive.index(),
                        positions,
                        indices,
                    });
                }
            }
        }
    }
    if surfaces.is_empty() {
        return Err("Scene0 没有可编辑的蒙皮网格".into());
    }
    active(cancel)?;
    let occlusion = Occlusion::new(triangles);
    Ok(Arc::new(Document {
        identity,
        source,
        json,
        binary,
        surfaces,
        rigid,
        occlusion,
        extent: (maximum - minimum).max_element().max(0.0001),
    }))
}

fn add_triangles(
    triangles: &mut Vec<[Vec3; 3]>,
    points: &[Vec3],
    indices: &[u32],
    minimum: &mut Vec3,
    maximum: &mut Vec3,
) -> Result<(), String> {
    for &point in points {
        if !point.is_finite() {
            return Err("蒙皮静置位置无效".into());
        }
        *minimum = minimum.min(point);
        *maximum = maximum.max(point);
    }
    triangles.extend(indices.chunks_exact(3).map(|face| {
        [
            points[face[0] as usize],
            points[face[1] as usize],
            points[face[2] as usize],
        ]
    }));
    Ok(())
}
