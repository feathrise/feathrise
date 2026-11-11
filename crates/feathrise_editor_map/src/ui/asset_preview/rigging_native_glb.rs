//! 原生绑骨的 GLB 边界。只在后台解码求解需要的三角形，发布时保留原始顶点和贴图载荷。

use super::super::files::{active, mixamo_skin};
use super::{Geometry, Joint, VertexSkin};
use bevy::prelude::{Mat3, Mat4, Quat, Vec3};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    fs::OpenOptions,
    io::{BufWriter, Write},
    ops::Range,
    path::Path,
    sync::atomic::AtomicBool,
};

const MAX_GLB: usize = 256 * 1024 * 1024;
const MAX_JSON: usize = 16 * 1024 * 1024;
const MAX_NODES: usize = 8192;
const MAX_VERTICES: usize = 500_000;
const MAX_TRIANGLES: usize = 1_000_000;
const MAX_MESH_NODES: usize = 32;
const MAX_PRIMITIVES: usize = 1024;
const MAX_JOINTS: usize = 128;
const SKIN_STRIDE: usize = 24; // 四个 u16 关节后接四个 f32 权重，保持四字节对齐。
const JSON: u32 = u32::from_le_bytes(*b"JSON");
const BIN: u32 = u32::from_le_bytes(*b"BIN\0");

struct MeshPart {
    node: usize,
    mesh: usize,
    world: Mat4,
    primitives: Vec<Range<usize>>,
}

pub(super) struct GlbModel {
    pub geometry: Geometry,
    source: Vec<u8>,
    document: Value,
    binary: Range<usize>,
    parts: Vec<MeshPart>,
}

pub(super) struct GlbOutput {
    source: Vec<u8>,
    binary: Range<usize>,
    document: Vec<u8>,
    additions: Vec<u8>,
}

impl GlbOutput {
    pub(super) fn write(&self, path: &Path, cancel: &AtomicBool) -> Result<(), String> {
        active(cancel)?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| format!("创建蒙皮 GLB 暂存失败：{error}"))?;
        let mut output = BufWriter::new(file);
        let binary_len = self.binary.len() + self.additions.len();
        let trailing = &self.source[self.binary.end..];
        let file_len = 28 + self.document.len() + binary_len + trailing.len();
        let header = |size: usize, kind: u32| {
            let mut header = [0; 8];
            header[..4].copy_from_slice(&(size as u32).to_le_bytes());
            header[4..].copy_from_slice(&kind.to_le_bytes());
            header
        };
        let glb_header = header(2, file_len as u32);
        let json_header = header(self.document.len(), JSON);
        let binary_header = header(binary_len, BIN);
        // 一个合法 BIN 块分段写出，原载荷与新权重无需合并成第二份完整文件。
        for segment in [
            b"glTF".as_slice(),
            &glb_header,
            &json_header,
            &self.document,
            &binary_header,
            &self.source[self.binary.clone()],
            &self.additions,
            trailing,
        ] {
            for chunk in segment.chunks(64 * 1024) {
                active(cancel)?;
                output
                    .write_all(chunk)
                    .map_err(|error| format!("写入蒙皮 GLB 失败：{error}"))?;
            }
        }
        active(cancel)?;
        output
            .flush()
            .map_err(|error| format!("完成蒙皮 GLB 写入失败：{error}"))
    }
}

fn list<'a>(object: &'a Value, key: &str) -> Result<&'a [Value], String> {
    match object.get(key) {
        Some(value) => value
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| format!("GLB 的 {key} 必须是数组")),
        None => Ok(&[]),
    }
}

fn index(value: &Value) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| "GLB 索引或长度无效".to_owned())
}

fn index_or(value: &Value, key: &str, default: usize) -> Result<usize, String> {
    value.get(key).map_or(Ok(default), index)
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let data = bytes.get(offset..offset + 4).ok_or("GLB 块头被截断")?;
    Ok(u32::from_le_bytes(data.try_into().unwrap()))
}

fn chunk(bytes: &[u8], offset: usize) -> Result<(u32, Range<usize>), String> {
    let len = le_u32(bytes, offset)? as usize;
    let kind = le_u32(bytes, offset + 4)?;
    let start = offset + 8;
    let end = start.checked_add(len).ok_or("GLB 块长度溢出")?;
    if len % 4 != 0 || end > bytes.len() {
        return Err("GLB 块长度超出文件或未四字节对齐".into());
    }
    Ok((kind, start..end))
}

fn container(bytes: &[u8], cancel: &AtomicBool) -> Result<(Value, Range<usize>), String> {
    if !(28..=MAX_GLB).contains(&bytes.len())
        || !bytes.starts_with(b"glTF")
        || le_u32(bytes, 4)? != 2
        || le_u32(bytes, 8)? as usize != bytes.len()
    {
        return Err("模型必须是 256 MiB 内的完整 glTF 2.0 GLB".into());
    }
    let (kind, metadata) = chunk(bytes, 12)?;
    if kind != JSON || metadata.is_empty() || metadata.len() > MAX_JSON {
        return Err("GLB 首块必须是 16 MiB 内的 JSON".into());
    }
    let (kind, binary) = chunk(bytes, metadata.end)?;
    if kind != BIN {
        return Err("模型缺少内嵌 BIN 块".into());
    }
    let mut offset = binary.end;
    while offset < bytes.len() {
        active(cancel)?;
        let (kind, extra) = chunk(bytes, offset)?;
        if matches!(kind, JSON | BIN) {
            return Err("GLB 包含重复的 JSON 或 BIN 块".into());
        }
        offset = extra.end; // 未知块不参与解码，写回时保留原字节。
    }
    let document: Value = serde_json::from_slice(&bytes[metadata])
        .map_err(|error| format!("解析模型 GLB 元数据失败：{error}"))?;
    Ok((document, binary))
}

fn components<const N: usize>(
    node: &Value,
    key: &str,
    default: [f32; N],
) -> Result<[f32; N], String> {
    let Some(value) = node.get(key) else {
        return Ok(default);
    };
    let values = value
        .as_array()
        .filter(|values| values.len() == N)
        .ok_or_else(|| format!("GLB 节点 {key} 维数无效"))?;
    let mut result = [0.0; N];
    for (output, input) in result.iter_mut().zip(values) {
        *output = input.as_f64().ok_or("GLB 节点变换不是数字")? as f32;
        if !output.is_finite() {
            return Err("GLB 节点变换含无效数字".into());
        }
    }
    Ok(result)
}

fn normalized_rotation(value: Quat) -> Result<Quat, String> {
    let length = value.length_squared();
    if !length.is_finite() || length <= 1e-12 {
        return Err("GLB 骨架或节点旋转无效".into());
    }
    Ok(value.normalize())
}

fn local_matrix(node: &Value) -> Result<Mat4, String> {
    if node.get("matrix").is_some() {
        if ["translation", "rotation", "scale"]
            .iter()
            .any(|key| node.get(*key).is_some())
        {
            return Err("GLB 节点不能同时声明矩阵和局部 TRS".into());
        }
        let matrix =
            Mat4::from_cols_array(&components(node, "matrix", Mat4::IDENTITY.to_cols_array())?);
        if [
            matrix.x_axis.w,
            matrix.y_axis.w,
            matrix.z_axis.w,
            matrix.w_axis.w,
        ] != [0.0, 0.0, 0.0, 1.0]
        {
            return Err("GLB 节点矩阵必须是仿射变换".into());
        }
        return Ok(matrix);
    }
    Ok(Mat4::from_scale_rotation_translation(
        Vec3::from_array(components(node, "scale", [1.0; 3])?),
        normalized_rotation(Quat::from_array(components(
            node,
            "rotation",
            Quat::IDENTITY.to_array(),
        )?))?,
        Vec3::from_array(components(node, "translation", [0.0; 3])?),
    ))
}

struct NodeGraph {
    parents: Vec<Option<usize>>,
    children: Vec<Vec<usize>>,
    world: Vec<Mat4>,
}

impl NodeGraph {
    fn parse(nodes: &[Value], cancel: &AtomicBool) -> Result<Self, String> {
        if nodes.is_empty() || nodes.len() > MAX_NODES {
            return Err("GLB 必须包含 1 至 8192 个节点".into());
        }
        let mut parents = vec![None; nodes.len()];
        let mut children = Vec::with_capacity(nodes.len());
        for (parent, node) in nodes.iter().enumerate() {
            active(cancel)?;
            if !node.is_object() {
                return Err("GLB 节点元数据不是对象".into());
            }
            let mut edges = Vec::new();
            for child in list(node, "children")? {
                let child = index(child)?;
                let slot = parents.get_mut(child).ok_or("GLB 子节点索引越界")?;
                if slot.replace(parent).is_some() {
                    return Err("GLB 节点包含重复子节点或多个父级".into());
                }
                edges.push(child);
            }
            children.push(edges);
        }
        let mut stack: Vec<_> = (0..nodes.len())
            .filter(|&node| parents[node].is_none())
            .collect();
        let mut world = vec![Mat4::IDENTITY; nodes.len()];
        let mut visited = 0;
        while let Some(node) = stack.pop() {
            active(cancel)?;
            let matrix = parents[node].map_or(Mat4::IDENTITY, |parent| world[parent])
                * local_matrix(&nodes[node])?;
            if !matrix.is_finite() {
                return Err("GLB 场景变换超出可表示范围".into());
            }
            world[node] = matrix;
            visited += 1;
            stack.extend(children[node].iter().copied());
        }
        if visited != nodes.len() {
            return Err("GLB 节点层级包含循环".into());
        }
        Ok(Self {
            parents,
            children,
            world,
        })
    }

    fn scene_zero(&self, scenes: &[Value], cancel: &AtomicBool) -> Result<Vec<bool>, String> {
        if scenes.is_empty() {
            return Err("模型需要一个 Scene0 场景".into());
        }
        let mut reachable = vec![false; self.parents.len()];
        for (scene, document) in scenes.iter().enumerate() {
            active(cancel)?;
            if !document.is_object() {
                return Err("GLB 场景元数据不是对象".into());
            }
            let mut roots = HashSet::new();
            for node in list(document, "nodes")? {
                let node = index(node)?;
                if self.parents.get(node) != Some(&None) || !roots.insert(node) {
                    return Err("GLB 场景包含重复或非法的根节点".into());
                }
            }
            if scene == 0 {
                let mut stack: Vec<_> = roots.into_iter().collect();
                while let Some(node) = stack.pop() {
                    reachable[node] = true;
                    stack.extend(self.children[node].iter().copied());
                }
            }
        }
        Ok(reachable)
    }
}

struct Elements<'a> {
    bytes: &'a [u8],
    count: usize,
    stride: usize,
    width: usize,
}

impl Elements<'_> {
    fn at(&self, index: usize) -> &[u8] {
        let offset = index * self.stride;
        &self.bytes[offset..offset + self.width]
    }
}

fn elements<'a>(
    doc: &Value,
    binary: &'a [u8],
    accessor: &Value,
    component: usize,
    width: usize,
) -> Result<Elements<'a>, String> {
    if accessor.get("sparse").is_some()
        || accessor
            .get("normalized")
            .is_some_and(|v| v != &Value::Bool(false))
    {
        return Err("原生绑骨暂不接受稀疏或归一化 POSITION/索引".into());
    }
    let view = list(doc, "bufferViews")?
        .get(index(&accessor["bufferView"])?)
        .ok_or("GLB 访问器的 bufferView 越界")?;
    if ["EXT_meshopt_compression", "KHR_meshopt_compression"]
        .iter()
        .any(|extension| view["extensions"].get(*extension).is_some())
    {
        return Err("原生绑骨暂不接受 Meshopt 压缩几何".into());
    }
    if index(&view["buffer"])? != 0 {
        return Err("绑骨几何必须位于 GLB 内嵌 buffer 0".into());
    }
    let start = index_or(view, "byteOffset", 0)?;
    let length = index(&view["byteLength"])?;
    let offset = index_or(accessor, "byteOffset", 0)?;
    let stride = index_or(view, "byteStride", width)?;
    let count = index(&accessor["count"])?;
    let aligned = start % component == 0 && offset % component == 0;
    if stride < width || stride > 252 || stride % component != 0 || !aligned || count == 0 {
        return Err("GLB 访问器长度、步幅或对齐无效".into());
    }
    let extent = (count - 1)
        .checked_mul(stride)
        .and_then(|length| length.checked_add(width))
        .and_then(|length| offset.checked_add(length))
        .ok_or("GLB 访问器长度溢出")?;
    let end = start.checked_add(length).ok_or("GLB bufferView 长度溢出")?;
    let bytes = binary
        .get(start..end)
        .and_then(|view| view.get(offset..extent))
        .ok_or("GLB 访问器超出内嵌数据边界")?;
    Ok(Elements {
        bytes,
        count,
        stride,
        width,
    })
}

fn primitive_geometry(
    doc: &Value,
    binary: &[u8],
    primitive: &Value,
    world: Mat4,
    geometry: &mut Geometry,
    cancel: &AtomicBool,
) -> Result<Range<usize>, String> {
    if !primitive.is_object()
        || !list(primitive, "targets")?.is_empty()
        || primitive["extensions"]
            .get("KHR_draco_mesh_compression")
            .is_some()
    {
        return Err("请选择无形态键、无 Draco 压缩的普通 GLB 网格".into());
    }
    let attributes = primitive["attributes"]
        .as_object()
        .ok_or("GLB 网格缺少顶点属性")?;
    if attributes
        .keys()
        .any(|name| name.starts_with("JOINTS_") || name.starts_with("WEIGHTS_"))
    {
        return Err("原模型已经包含顶点蒙皮属性".into());
    }
    let accessors = list(doc, "accessors")?;
    let accessor = |value: &Value| -> Result<&Value, String> {
        accessors
            .get(index(value)?)
            .ok_or_else(|| "GLB 网格访问器索引越界".to_owned())
    };
    let position = accessor(attributes.get("POSITION").ok_or("GLB 网格缺少 POSITION")?)?;
    if position["componentType"] != 5126 || position["type"] != "VEC3" {
        return Err("原生绑骨需要普通 FLOAT VEC3 顶点坐标".into());
    }
    let positions = elements(doc, binary, position, 4, 12)?;
    let start = geometry.vertices.len();
    if positions.count > MAX_VERTICES - start {
        return Err("场景网格总顶点数超过 50 万".into());
    }
    let indices = if let Some(value) = primitive.get("indices") {
        let index_accessor = accessor(value)?;
        let size = match index(&index_accessor["componentType"])? {
            5121 => 1,
            5123 => 2,
            5125 => 4,
            _ => return Err("GLB 三角索引必须是 u8、u16 或 u32".into()),
        };
        if index_accessor["type"] != "SCALAR" {
            return Err("GLB 三角索引必须是 SCALAR".into());
        }
        Some(elements(doc, binary, index_accessor, size, size)?)
    } else {
        None
    };
    let count = indices
        .as_ref()
        .map_or(positions.count, |indices| indices.count);
    let mode = index_or(primitive, "mode", 4)?;
    let triangles = match mode {
        4 if count >= 3 && count % 3 == 0 => count / 3,
        5 | 6 if count >= 3 => count - 2,
        _ => return Err("绑骨网格必须使用有效的三角形、三角带或三角扇".into()),
    };
    if triangles > MAX_TRIANGLES - geometry.triangles.len() {
        return Err("场景网格总三角形数超过 100 万".into());
    }
    let mirrored = world.determinant() < 0.0;
    for vertex in 0..positions.count {
        if vertex % 4096 == 0 {
            active(cancel)?;
        }
        let bytes = positions.at(vertex);
        let point = Vec3::from_array(std::array::from_fn(|i| {
            f32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
        }));
        let point = world.transform_point3(point);
        if !point.is_finite() {
            return Err("模型包含非有限顶点或超出范围的世界坐标".into());
        }
        geometry.vertices.push(point);
    }
    let vertex = |offset: usize| -> Result<u32, String> {
        let value = match indices.as_ref().map(|indices| indices.at(offset)) {
            Some([a]) => u32::from(*a),
            Some([a, b]) => u32::from(u16::from_le_bytes([*a, *b])),
            Some([a, b, c, d]) => u32::from_le_bytes([*a, *b, *c, *d]),
            Some(_) => unreachable!("三角索引访问器宽度已经验证"),
            None => offset as u32,
        };
        if value as usize >= positions.count {
            return Err("GLB 三角索引超出对应顶点范围".into());
        }
        Ok(start as u32 + value)
    };
    for triangle in 0..triangles {
        if triangle % 4096 == 0 {
            active(cancel)?;
        }
        let [a, b, c] = match mode {
            4 => [triangle * 3, triangle * 3 + 1, triangle * 3 + 2],
            5 if triangle % 2 == 1 => [triangle + 1, triangle, triangle + 2],
            5 => [triangle, triangle + 1, triangle + 2],
            _ => [0, triangle + 1, triangle + 2],
        };
        let mut indices = [vertex(a)?, vertex(b)?, vertex(c)?];
        if mirrored {
            indices.swap(1, 2);
        }
        geometry.triangles.push(indices);
    }
    Ok(start..geometry.vertices.len())
}

pub(super) fn decode(bytes: Vec<u8>, cancel: &AtomicBool) -> Result<GlbModel, String> {
    active(cancel)?;
    let (document, binary) = container(&bytes, cancel)?;
    if document.pointer("/asset/version").and_then(Value::as_str) != Some("2.0")
        || !list(&document, "animations")?.is_empty()
        || !list(&document, "skins")?.is_empty()
    {
        return Err("请选择无骨架、无动画的 glTF 2.0 模型".into());
    }
    let unsupported = |extension: &Value| {
        extension.as_str().is_some_and(|name| {
            matches!(
                name,
                "KHR_draco_mesh_compression"
                    | "EXT_meshopt_compression"
                    | "KHR_meshopt_compression"
                    | "EXT_mesh_gpu_instancing"
            )
        })
    };
    for field in ["extensionsUsed", "extensionsRequired"] {
        if list(&document, field)?.iter().any(unsupported) {
            return Err("原生绑骨暂不接受压缩几何或 GPU 实例网格，请先导出普通 GLB".into());
        }
    }
    let [buffer] = list(&document, "buffers")? else {
        return Err("模型必须只有一个内嵌 BIN 缓冲区".into());
    };
    let length = index(&buffer["byteLength"])?;
    if buffer.get("uri").is_some()
        || length > binary.len()
        || binary.len() - length > 3
        || list(&document, "images")?.iter().any(|image| {
            image
                .get("uri")
                .is_some_and(|uri| !uri.as_str().is_some_and(|uri| uri.starts_with("data:")))
        })
    {
        return Err("请选择贴图及几何均已打包的完整 GLB".into());
    }
    let nodes = list(&document, "nodes")?;
    let meshes = list(&document, "meshes")?;
    let graph = NodeGraph::parse(nodes, cancel)?;
    let reachable = graph.scene_zero(list(&document, "scenes")?, cancel)?;
    let mut geometry = Geometry {
        vertices: Vec::new(),
        triangles: Vec::new(),
        parts: Vec::new(),
    };
    let mut parts = Vec::new();
    let mut seen = vec![false; meshes.len()];
    let mut primitive_count = 0;
    for (node_index, node) in nodes.iter().enumerate() {
        active(cancel)?;
        if node.get("skin").is_some() || node.get("weights").is_some() {
            return Err("原模型必须没有蒙皮或形态键".into());
        }
        let Some(mesh) = node.get("mesh") else {
            continue;
        };
        if !reachable[node_index] || node["extensions"].get("EXT_mesh_gpu_instancing").is_some() {
            return Err("仅支持 Scene0 可达的普通网格节点".into());
        }
        if parts.len() >= MAX_MESH_NODES {
            return Err("Scene0 网格节点超过 32 个".into());
        }
        let mesh = index(mesh)?;
        let mesh_document = meshes
            .get(mesh)
            .filter(|mesh| mesh.is_object())
            .ok_or("GLB 网格索引越界")?;
        let primitives = list(mesh_document, "primitives")?;
        if primitives.is_empty() || mesh_document.get("weights").is_some() {
            return Err("GLB 网格缺少 primitive 或包含形态键".into());
        }
        primitive_count += primitives.len();
        if primitive_count > MAX_PRIMITIVES {
            return Err("Scene0 网格 primitive 总数超过 1024 个".into());
        }
        let world = graph.world[node_index];
        let determinant = world.determinant();
        if !determinant.is_finite() || determinant == 0.0 {
            return Err("网格节点世界变换退化，无法保持原模型姿态".into());
        }
        seen[mesh] = true;
        let start = geometry.vertices.len();
        let ranges = primitives
            .iter()
            .map(|primitive| {
                primitive_geometry(
                    &document,
                    &bytes[binary.start..binary.start + length],
                    primitive,
                    world,
                    &mut geometry,
                    cancel,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        geometry.parts.push(start..geometry.vertices.len());
        parts.push(MeshPart {
            node: node_index,
            mesh,
            world,
            primitives: ranges,
        });
    }
    if parts.is_empty() || seen.contains(&false) {
        return Err("模型必须包含 Scene0 网格，所有网格定义均须由该场景引用".into());
    }
    Ok(GlbModel {
        geometry,
        source: bytes,
        document,
        binary,
        parts,
    })
}

fn rigid_basis(world: Mat4) -> Result<Quat, String> {
    let axes = [
        world.x_axis.truncate(),
        world.y_axis.truncate(),
        world.z_axis.truncate(),
    ];
    let lengths = Vec3::new(axes[0].length(), axes[1].length(), axes[2].length());
    let maximum = lengths.max_element();
    if !lengths.is_finite()
        || lengths.min_element() <= 1e-8
        || maximum - lengths.min_element() > maximum * 0.001
    {
        return Err("参考骨架含非等比或退化缩放，请先整理参考 GLB".into());
    }
    let basis = Mat3::from_cols(
        axes[0] / lengths.x,
        axes[1] / lengths.y,
        axes[2] / lengths.z,
    );
    if (basis.determinant() - 1.0).abs() > 0.001
        || basis.x_axis.dot(basis.y_axis).abs() > 0.001
        || basis.y_axis.dot(basis.z_axis).abs() > 0.001
        || basis.z_axis.dot(basis.x_axis).abs() > 0.001
    {
        return Err("参考骨架含镜像或剪切坐标轴，请先整理参考 GLB".into());
    }
    normalized_rotation(Quat::from_mat3(&basis))
}

pub(super) fn reference(metadata: &Value, cancel: &AtomicBool) -> Result<Vec<Joint>, String> {
    active(cancel)?;
    if metadata.pointer("/asset/version").and_then(Value::as_str) != Some("2.0") {
        return Err("人形参考骨架需要 glTF 2.0 GLB".into());
    }
    let (skin, _) = mixamo_skin(metadata).ok_or("所选 GLB 缺少可用的 Mixamo 人形参考骨架")?;
    let nodes = list(metadata, "nodes")?;
    let graph = NodeGraph::parse(nodes, cancel)?;
    let joint_nodes = list(skin, "joints")?
        .iter()
        .map(index)
        .collect::<Result<Vec<_>, _>>()?;
    let mut joint_by_node = vec![None; nodes.len()];
    for (joint, &node) in joint_nodes.iter().enumerate() {
        joint_by_node[node] = Some(joint);
    }
    joint_nodes
        .into_iter()
        .map(|node| {
            active(cancel)?;
            let parent = graph.parents[node];
            let joint_parent = parent.and_then(|parent| joint_by_node[parent]);
            if joint_parent.is_none() {
                let mut ancestor = parent;
                while let Some(parent) = ancestor {
                    if joint_by_node[parent].is_some()
                        || nodes[parent]["name"]
                            .as_str()
                            .is_some_and(|name| name.starts_with("mixamorig:"))
                    {
                        return Err("参考骨架的关节之间包含未纳入蒙皮的中间节点".into());
                    }
                    ancestor = graph.parents[parent];
                }
            }
            let world = graph.world[node];
            Ok(Joint {
                name: nodes[node]["name"].as_str().unwrap().to_owned(),
                parent: joint_parent,
                position: world.w_axis.truncate(),
                rotation: rigid_basis(world)?,
            })
        })
        .collect()
}

fn push(document: &mut Value, array: &str, value: Value) -> usize {
    let values = document
        .as_object_mut()
        .expect("GLB 已验证为对象")
        .entry(array)
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .expect("GLB 数组已通过输入验证");
    let index = values.len();
    values.push(value);
    index
}

fn encode_skin(
    weights: &[VertexSkin],
    joints: usize,
    output: &mut Vec<u8>,
    cancel: &AtomicBool,
) -> Result<(), String> {
    for (vertex, skin) in weights.iter().enumerate() {
        if vertex % 4096 == 0 {
            active(cancel)?;
        }
        let mut sum = 0.0;
        for (slot, (&joint, &weight)) in skin.joints.iter().zip(&skin.weights).enumerate() {
            if usize::from(joint) >= joints
                || !weight.is_finite()
                || weight < 0.0
                || (weight > 0.0
                    && (0..slot).any(|i| skin.joints[i] == joint && skin.weights[i] > 0.0))
            {
                return Err("顶点蒙皮包含越界关节、重复影响或无效权重".into());
            }
            sum += weight;
            output.extend(joint.to_le_bytes());
        }
        if !sum.is_finite() || (sum - 1.0_f32).abs() > 0.002 {
            return Err("有顶点缺少有效的归一化权重".into());
        }
        for weight in skin.weights {
            output.extend((weight / sum).to_le_bytes());
        }
    }
    Ok(())
}

fn joint_nodes(
    document: &mut Value,
    joints: &[Joint],
) -> Result<(usize, Vec<usize>, Vec<Mat4>), String> {
    if joints.is_empty() || joints.len() > MAX_JOINTS {
        return Err("骨架必须包含 1 至 128 个关节".into());
    }
    let mut names: HashSet<&str> = list(document, "nodes")?
        .iter()
        .filter_map(|node| node["name"].as_str())
        .collect();
    let mut rotations = Vec::with_capacity(joints.len());
    for (index, joint) in joints.iter().enumerate() {
        if joint.name.is_empty()
            || !names.insert(joint.name.as_str())
            || !joint.position.is_finite()
        {
            return Err("骨架有无效位置、空名或与现有节点重复的名称".into());
        }
        let mut ancestor = joint.parent;
        for _ in 0..joints.len() {
            let Some(parent) = ancestor else {
                break;
            };
            if parent == index {
                return Err("骨架关节层级包含循环".into());
            }
            ancestor = joints.get(parent).ok_or("骨架父关节索引越界")?.parent;
        }
        if ancestor.is_some() {
            return Err("骨架关节层级包含循环".into());
        }
        rotations.push(normalized_rotation(joint.rotation)?);
    }
    drop(names);
    let start = list(document, "nodes")?.len();
    if start + joints.len() + 1 > MAX_NODES {
        return Err("加入人形骨架后将超过动作预览的 8192 节点限制".into());
    }
    let mut children = vec![Vec::new(); joints.len()];
    let mut roots = Vec::new();
    for (index, joint) in joints.iter().enumerate() {
        if let Some(parent) = joint.parent {
            children[parent].push(start + index);
        } else {
            roots.push(start + index);
        }
    }
    let mut inverse = Vec::with_capacity(joints.len());
    for (index, joint) in joints.iter().enumerate() {
        let rotation = rotations[index];
        let conjugate = rotation.conjugate();
        inverse.push(Mat4::from_rotation_translation(
            conjugate,
            conjugate * -joint.position,
        ));
        let (translation, rotation) = joint.parent.map_or((joint.position, rotation), |parent| {
            let parent_inverse = rotations[parent].conjugate();
            (
                parent_inverse * (joint.position - joints[parent].position),
                (parent_inverse * rotation).normalize(),
            )
        });
        if !translation.is_finite() {
            return Err("骨架局部位置超出可表示范围".into());
        }
        let mut node = json!({"name": joint.name, "translation": translation.to_array(), "rotation": rotation.to_array()});
        if !children[index].is_empty() {
            node["children"] = json!(children[index]);
        }
        push(document, "nodes", node);
    }
    let skeleton = push(
        document,
        "nodes",
        json!({"name":"FeathriseRig", "children":roots}),
    );
    for scene in document["scenes"].as_array_mut().unwrap() {
        if scene.get("nodes").is_none() {
            scene["nodes"] = json!([]);
        }
        scene["nodes"].as_array_mut().unwrap().push(json!(skeleton));
    }
    Ok((skeleton, (start..start + joints.len()).collect(), inverse))
}

pub(super) fn encode(
    model: GlbModel,
    joints: &[Joint],
    weights: &[VertexSkin],
    cancel: &AtomicBool,
) -> Result<GlbOutput, String> {
    active(cancel)?;
    let GlbModel {
        geometry,
        source,
        mut document,
        binary,
        parts,
    } = model;
    if weights.len() != geometry.vertices.len() {
        return Err("蒙皮回执的顶点数量与原网格不一致".into());
    }
    drop(geometry);
    let (skeleton, joint_indices, inverse) = joint_nodes(&mut document, joints)?;
    let mut additions =
        Vec::with_capacity(weights.len() * SKIN_STRIDE + parts.len() * joints.len() * 64);
    encode_skin(weights, joints.len(), &mut additions, cancel)?;
    let skin_view = push(
        &mut document,
        "bufferViews",
        json!({
            "buffer":0, "byteOffset":binary.len(), "byteLength":additions.len(),
            "byteStride":SKIN_STRIDE, "target":34962
        }),
    );
    let mut shared = vec![false; list(&document, "meshes")?.len()];
    for part in parts {
        active(cancel)?;
        let bind_offset = binary.len() + additions.len();
        for joint_inverse in &inverse {
            // glTF/Bevy 使用世界关节矩阵乘逆绑定。按网格实例乘回原世界矩阵，
            // 静止时顶点仍在原位置，平移、旋转和非等比网格变换均保留。
            let matrix = *joint_inverse * part.world;
            if !matrix.is_finite() || matrix.w_axis.w != 1.0 {
                return Err("网格逆绑定矩阵超出可表示范围".into());
            }
            for value in matrix.to_cols_array() {
                additions.extend(value.to_le_bytes());
            }
        }
        let bind_view = push(
            &mut document,
            "bufferViews",
            json!({
                "buffer":0, "byteOffset":bind_offset, "byteLength":joints.len() * 64
            }),
        );
        let bind = push(
            &mut document,
            "accessors",
            json!({
                "bufferView":bind_view, "componentType":5126, "count":joints.len(), "type":"MAT4"
            }),
        );
        let skin = push(
            &mut document,
            "skins",
            json!({
                "inverseBindMatrices":bind, "skeleton":skeleton, "joints":joint_indices
            }),
        );
        let mut mesh = document["meshes"][part.mesh].clone();
        for (primitive, range) in mesh["primitives"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(part.primitives)
        {
            let joint_accessor = push(
                &mut document,
                "accessors",
                json!({
                    "bufferView":skin_view, "byteOffset":range.start * SKIN_STRIDE,
                    "componentType":5123, "count":range.len(), "type":"VEC4"
                }),
            );
            let weight_accessor = push(
                &mut document,
                "accessors",
                json!({
                    "bufferView":skin_view, "byteOffset":range.start * SKIN_STRIDE + 8,
                    "componentType":5126, "count":range.len(), "type":"VEC4"
                }),
            );
            primitive["attributes"]["JOINTS_0"] = json!(joint_accessor);
            primitive["attributes"]["WEIGHTS_0"] = json!(weight_accessor);
        }
        // 重复引用同一网格的节点可能需要不同权重。保留第一个原索引，后续只复制 JSON，复用原顶点和材质引用。
        let mesh = if std::mem::replace(&mut shared[part.mesh], true) {
            push(&mut document, "meshes", mesh)
        } else {
            document["meshes"][part.mesh] = mesh;
            part.mesh
        };
        document["nodes"][part.node]["mesh"] = json!(mesh);
        document["nodes"][part.node]["skin"] = json!(skin);
    }
    document["buffers"][0]["byteLength"] = json!(binary.len() + additions.len());
    let mut document = serde_json::to_vec(&document)
        .map_err(|error| format!("编码蒙皮 GLB 元数据失败：{error}"))?;
    document.resize(document.len().next_multiple_of(4), b' ');
    let file_len = 28 + document.len() + binary.len() + additions.len() + source.len() - binary.end;
    if document.len() > MAX_JSON || file_len > MAX_GLB {
        return Err("加上蒙皮数据后超出 GLB 文件或元数据预算".into());
    }
    Ok(GlbOutput {
        source,
        binary,
        document,
        additions,
    })
}

#[cfg(test)]
#[path = "rigging_native_glb_tests.rs"]
mod tests;
