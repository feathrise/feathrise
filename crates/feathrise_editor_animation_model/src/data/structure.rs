//! 以源 GLB 的节点索引编辑真实层级和局部变换。文件事务由作者宿主统一处理。

use super::{
    reader::{BIN_CHUNK, JSON_CHUNK, float_array, index_field, read_chunks},
    types::Chunk,
    writer::encode_chunks,
};
use bevy::prelude::{Mat4, Quat, Transform, Vec3};
use serde_json::{Value, json};
use std::{collections::HashSet, sync::Arc};

#[cfg(test)]
mod tests;
mod mesh;
mod transfer;
pub use mesh::MeshGeometry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlbParent {
    Scene(usize),
    Node(usize),
}

impl GlbParent {
    fn location(self) -> (&'static str, usize, &'static str) {
        match self {
            Self::Scene(index) => ("scenes", index, "nodes"),
            Self::Node(index) => ("nodes", index, "children"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlbNodeInfo {
    pub index: usize,
    pub name: String,
    pub mesh: Option<usize>,
    pub children: Vec<usize>,
    pub folder: bool,
}

/// 按源文件保留二进制和未知数据块，几何写入为受影响的访问器追加数据。
pub struct GlbStructure {
    json: Value,
    chunks: Vec<Chunk>,
    binary: Arc<[u8]>,
}

impl GlbStructure {
    /// 在后台使用本次作者事务已经冻结的原始字节，避免为结构操作解释整份动画曲线。
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if !bytes.starts_with(b"glTF") {
            return Err("仅支持编辑独立 GLB 文件".into());
        }
        let chunks = read_chunks(bytes)?;
        let json_chunk = chunks
            .first()
            .filter(|chunk| chunk.kind == JSON_CHUNK)
            .ok_or("GLB 缺少 JSON 数据块")?;
        let json: Value = serde_json::from_slice(&json_chunk.bytes)
            .map_err(|error| format!("GLB JSON 无效：{error}"))?;
        if json.pointer("/asset/version").and_then(Value::as_str) != Some("2.0") {
            return Err("仅支持 glTF 2.0 的节点结构".into());
        }
        let binary = chunks
            .iter()
            .find(|chunk| chunk.kind == BIN_CHUNK)
            .map_or_else(Arc::default, |chunk| chunk.bytes.clone());
        match list(&json, "buffers")? {
            [] if binary.is_empty() => {}
            [buffer]
                if buffer.get("uri").is_none()
                    && index_field(buffer, "byteLength")? <= binary.len() => {}
            _ => return Err("编辑 GLB 结构需要单一内嵌缓冲".into()),
        }
        Ok(Self {
            json,
            chunks,
            binary,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>, String> {
        encode_chunks(&self.chunks, &self.json, &self.binary)
    }

    pub fn scene_index(&self) -> Option<usize> {
        let index = index_field(&self.json, "scene").unwrap_or(0);
        list(&self.json, "scenes").ok()?.get(index).map(|_| index)
    }

    pub fn children(&self, parent: GlbParent) -> Result<Vec<GlbNodeInfo>, String> {
        child_indices(&self.json, parent)?
            .into_iter()
            .map(|index| self.node(index))
            .collect()
    }

    pub fn node(&self, index: usize) -> Result<GlbNodeInfo, String> {
        let node = object_at(&self.json, "nodes", index)?;
        Ok(GlbNodeInfo {
            index,
            name: node
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("GltfNode{index}")),
            mesh: node.get("mesh").map(index_value).transpose()?,
            children: indices(node, "children")?,
            folder: node
                .pointer("/extras/feathrise_folder")
                .and_then(Value::as_bool)
                == Some(true),
        })
    }

    pub fn mesh_name(&self, index: usize) -> Result<Option<&str>, String> {
        Ok(object_at(&self.json, "meshes", index)?
            .get("name")
            .and_then(Value::as_str))
    }

    pub fn node_transform(&self, index: usize) -> Result<Transform, String> {
        let node = object_at(&self.json, "nodes", index)?;
        if node.get("matrix").is_some() {
            decompose(local_matrix(node)?)
        } else {
            read_trs(node)
        }
    }

    pub fn node_extra(&self, index: usize, key: &str) -> Result<Option<&Value>, String> {
        Ok(object_at(&self.json, "nodes", index)?.get("extras").and_then(|extras| extras.get(key)))
    }

    /// 把临时生成的几何导入其他文件之前，去掉源节点专用的非标准协议。
    pub fn clear_node_extras(&mut self, index: usize) -> Result<(), String> {
        object_at(&self.json, "nodes", index)?;
        self.json["nodes"][index].as_object_mut().unwrap().remove("extras");
        Ok(())
    }

    /// 只更新指定的 extras 字段，保留其他工具写入的数据。
    pub fn set_node_extra(&mut self, index: usize, key: &str, value: Option<Value>) -> Result<(), String> {
        object_at(&self.json, "nodes", index)?;
        let node = self.json["nodes"][index].as_object_mut().unwrap();
        if let Some(value) = value {
            let extras = node.entry("extras").or_insert_with(|| json!({}));
            extras.as_object_mut().ok_or("GLB 节点的 extras 不是对象，无法保留原数据并写入属性")?
                .insert(key.to_owned(), value);
        } else if let Some(extras) = node.get_mut("extras").and_then(Value::as_object_mut) {
            if extras.remove(key).is_some() && extras.is_empty() {
                node.remove("extras");
            }
        }
        Ok(())
    }

    /// 以明确场景重建节点路径，外部重导出后用来复核树行所见的真实身份。
    pub fn node_path_in_scene(
        &self,
        scene: usize,
        index: usize,
    ) -> Result<Vec<String>, String> {
        self.node_lineage_in_scene(scene, index)?.into_iter().map(|index| Ok(self.node(index)?.name)).collect()
    }

    fn node_lineage_in_scene(&self, scene: usize, mut index: usize) -> Result<Vec<usize>, String> {
        let hierarchy = Hierarchy::read(&self.json)?;
        let mut path = Vec::new();
        loop {
            let parent = hierarchy.parents.get(index).ok_or("GLB 节点索引越界")?;
            if path.len() == hierarchy.parents.len() {
                return Err("GLB 父节点形成循环".into());
            }
            path.push(index);
            let Some(parent) = parent else {
                break;
            };
            index = *parent;
        }
        if !hierarchy.roots[index].contains(&scene) {
            return Err("GLB 节点不在此场景中".into());
        }
        path.reverse();
        Ok(path)
    }

    pub fn set_node_transform(&mut self, index: usize, transform: Transform) -> Result<(), String> {
        object_at(&self.json, "nodes", index)?;
        let transform = checked_trs(transform)?;
        write_trs(&mut self.json["nodes"][index], transform);
        Ok(())
    }

    pub fn node_world_matrix(&self, index: usize) -> Result<Mat4, String> {
        let hierarchy = Hierarchy::read(&self.json)?;
        let lineage = hierarchy.lineage(GlbParent::Node(index))?;
        accumulate(&self.json, &lineage)
    }

    /// 返回直属父节点相对 GLB 根的完整矩阵。不反求所选节点，因此也支持它已有零缩放。
    pub fn node_parent_matrix(&self, scene: usize, index: usize) -> Result<Mat4, String> {
        let mut lineage = self.node_lineage_in_scene(scene, index)?;
        lineage.pop(); // 节点自身旋转和缩放由独立的局部姿态读取。
        accumulate(&self.json, &lineage)
    }

    /// 复用 GLB 局部变换的精确检查，编辑器换坐标时也不能把剪切默默丢弃。
    pub fn decompose_node_matrix(matrix: Mat4) -> Result<Transform, String> {
        decompose(matrix)
    }

    pub fn create_folder(&mut self, parent: GlbParent, name: &str) -> Result<usize, String> {
        let name = checked_name(name)?;
        child_indices(&self.json, parent)?;
        let index = list(&self.json, "nodes")?.len();
        push(
            &mut self.json,
            "nodes",
            json!({
                "name": name, "extras": { "feathrise_folder": true },
            }),
        );
        attach(&mut self.json, parent, index);
        Ok(index)
    }

    /// 从场景层级移除空作者文件夹，保留其余 glTF 节点的索引。
    pub fn remove_empty_folder(&mut self, index: usize) -> Result<(), String> {
        let folder = self.node(index)?;
        let value = object_at(&self.json, "nodes", index)?;
        if !folder.folder || folder.mesh.is_some() || !folder.children.is_empty()
            || value.get("camera").is_some() || value.get("extensions").is_some()
        {
            return Err("只能删除没有内容的源 GLB 文件夹".into());
        }
        check_unbound(&self.json, &HashSet::from([index]))?;
        check_static_ancestors(&self.json, [index])?;
        let parent = Hierarchy::read(&self.json)?.parent(index)?;
        detach(&mut self.json, parent, index);
        Ok(())
    }

    pub fn rename_node(&mut self, index: usize, name: &str) -> Result<(), String> {
        let name = checked_name(name)?;
        object_at(&self.json, "nodes", index)?;
        self.json["nodes"][index]["name"] = json!(name);
        Ok(())
    }

    pub fn rename_mesh(&mut self, index: usize, name: &str) -> Result<(), String> {
        let name = checked_name(name).map_err(|_| "请输入网格名称")?;
        object_at(&self.json, "meshes", index)?;
        self.json["meshes"][index]["name"] = json!(name);
        Ok(())
    }

    /// 文件内移动保留节点序号、其余场景和所有资源。只有可无损变换父空间时才接纳换父。
    pub fn move_node(&mut self, index: usize, parent: GlbParent) -> Result<(), String> {
        child_indices(&self.json, parent)?;
        let hierarchy = Hierarchy::read(&self.json)?;
        let previous = hierarchy.parent(index)?;
        if previous == parent {
            return Ok(());
        }
        let previous_lineage = hierarchy.lineage(previous)?;
        let next_lineage = hierarchy.lineage(parent)?;
        if next_lineage.contains(&index) {
            return Err("不能将节点移动到自身或其子节点下".into());
        }
        let subtree = subtree_indices(&self.json, index)?;
        check_unbound(&self.json, &subtree)?;
        let shared = previous_lineage
            .iter()
            .zip(&next_lineage)
            .take_while(|(a, b)| a == b)
            .count();
        let source = &previous_lineage[shared..];
        let destination = &next_lineage[shared..];
        check_static_ancestors(&self.json, source.iter().chain(destination).copied())?;
        // 同一动画祖先下改换静态父节点时，共有祖先的姿态相消；子树内的曲线可原样保留。
        let basis = invert(accumulate(&self.json, destination)?)? * accumulate(&self.json, source)?;
        let replacement = preserved_local(&self.json, index, basis)?;
        detach(&mut self.json, previous, index);
        attach(&mut self.json, parent, index);
        if let Some(transform) = replacement {
            write_trs(&mut self.json["nodes"][index], transform);
        }
        Ok(())
    }
}

struct Hierarchy {
    parents: Vec<Option<usize>>,
    roots: Vec<Vec<usize>>,
    scene_count: usize,
}

impl Hierarchy {
    fn read(json: &Value) -> Result<Self, String> {
        let nodes = list(json, "nodes")?;
        let scenes = list(json, "scenes")?;
        let mut parents = vec![None; nodes.len()];
        let mut roots = vec![Vec::new(); nodes.len()];
        for (index, node) in nodes.iter().enumerate() {
            for child in indices(node, "children")? {
                let parent = parents.get_mut(child).ok_or("GLB 子节点索引越界")?;
                if parent.replace(index).is_some() {
                    return Err("GLB 节点的父节点不唯一".into());
                }
            }
        }
        for (scene, value) in scenes.iter().enumerate() {
            for index in indices(value, "nodes")? {
                let node_roots = roots.get_mut(index).ok_or("GLB 场景根节点索引越界")?;
                if parents[index].is_some() || node_roots.last() == Some(&scene) {
                    return Err("GLB 场景根节点已经处于另一层级或在同一场景重复使用".into());
                }
                node_roots.push(scene);
            }
        }
        Ok(Self {
            parents,
            roots,
            scene_count: scenes.len(),
        })
    }

    fn parent(&self, node: usize) -> Result<GlbParent, String> {
        let parent = self.parents.get(node).ok_or("GLB 节点索引越界")?;
        match (parent, self.roots[node].as_slice()) {
            (Some(index), _) => Ok(GlbParent::Node(*index)),
            (None, [scene]) => Ok(GlbParent::Scene(*scene)),
            _ => Err("节点的所属场景不唯一".into()),
        }
    }

    fn lineage(&self, parent: GlbParent) -> Result<Vec<usize>, String> {
        let mut next = parent;
        let mut reverse = Vec::new();
        while let GlbParent::Node(node) = next {
            if reverse.len() == self.parents.len() {
                return Err("GLB 父节点形成循环".into());
            }
            reverse.push(node);
            next = self.parent(node)?;
        }
        let GlbParent::Scene(scene) = next else {
            unreachable!()
        };
        if scene >= self.scene_count {
            return Err("GLB 场景索引越界".into());
        }
        reverse.reverse();
        Ok(reverse)
    }
}

fn preserved_local(json: &Value, index: usize, basis: Mat4) -> Result<Option<Transform>, String> {
    let node = object_at(json, "nodes", index)?;
    if basis.abs_diff_eq(Mat4::IDENTITY, 1.0e-6) {
        return Ok(None);
    }
    check_static_ancestors(json, [index])?;
    decompose(basis * local_matrix(node)?).map(Some)
}

fn check_static_ancestors(
    json: &Value,
    nodes: impl IntoIterator<Item = usize>,
) -> Result<(), String> {
    let nodes: HashSet<_> = nodes.into_iter().collect();
    if nodes.is_empty() {
        return Ok(());
    }
    for clip in list(json, "animations")? {
        for channel in list(clip, "channels")? {
            let target = &channel["target"];
            if target
                .pointer("/extensions/KHR_animation_pointer")
                .is_some()
                || (matches!(
                    target["path"].as_str(),
                    Some("translation" | "rotation" | "scale")
                ) && index_field(target, "node")
                    .ok()
                    .is_some_and(|index| nodes.contains(&index)))
            {
                return Err("父空间变化会改变已有动画姿态，请将节点移动到相同变换空间".into());
            }
        }
    }
    Ok(())
}

fn check_unbound(json: &Value, subtree: &HashSet<usize>) -> Result<(), String> {
    for skin in list(json, "skins")? {
        let mut nodes = indices(skin, "joints")?;
        nodes.extend(skin.get("skeleton").map(index_value).transpose()?);
        if nodes.into_iter().any(|node| subtree.contains(&node)) {
            return Err("该子树含蒙皮关节，不能自动更改骨架层级".into());
        }
    }
    for &index in subtree {
        if object_at(json, "nodes", index)?.get("skin").is_some() {
            return Err("该子树含蒙皮网格，不能自动移动绑定关系".into());
        }
    }
    Ok(())
}

fn subtree_indices(json: &Value, root: usize) -> Result<HashSet<usize>, String> {
    let mut pending = vec![root];
    let mut seen = HashSet::new();
    while let Some(index) = pending.pop() {
        if !seen.insert(index) {
            return Err("GLB 子树包含循环或重复节点".into());
        }
        pending.extend(child_indices(json, GlbParent::Node(index))?);
    }
    Ok(seen)
}

fn checked_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("请输入节点名称".into());
    }
    Ok(name)
}

fn accumulate(json: &Value, nodes: &[usize]) -> Result<Mat4, String> {
    nodes.iter().try_fold(Mat4::IDENTITY, |matrix, &node| {
        Ok(matrix * local_matrix(object_at(json, "nodes", node)?)?)
    })
}

fn read_trs(node: &Value) -> Result<Transform, String> {
    checked_trs(Transform {
        translation: Vec3::from_array(float_array(node.get("translation"), [0.0; 3])?),
        rotation: Quat::from_array(float_array(node.get("rotation"), [0.0, 0.0, 0.0, 1.0])?),
        scale: Vec3::from_array(float_array(node.get("scale"), [1.0; 3])?),
    })
}

fn checked_trs(mut transform: Transform) -> Result<Transform, String> {
    let length = transform.rotation.length_squared();
    if !transform.translation.is_finite()
        || !transform.scale.is_finite()
        || !length.is_finite()
        || length < f32::MIN_POSITIVE
    {
        return Err("GLB 节点的局部变换无效".into());
    }
    transform.rotation = transform.rotation.normalize();
    Ok(transform)
}

fn local_matrix(node: &Value) -> Result<Mat4, String> {
    if let Some(matrix) = node.get("matrix") {
        if ["translation", "rotation", "scale"]
            .iter()
            .any(|key| node.get(key).is_some())
        {
            return Err("GLB 节点同时使用了 matrix 与 TRS".into());
        }
        return Ok(Mat4::from_cols_array(&float_array(
            Some(matrix),
            [0.0; 16],
        )?));
    }
    Ok(read_trs(node)?.to_matrix())
}

fn invert(matrix: Mat4) -> Result<Mat4, String> {
    let determinant = matrix.determinant();
    if !determinant.is_finite() || determinant == 0.0 {
        return Err("目标父节点的变换不可逆".into());
    }
    Ok(matrix.inverse())
}

fn decompose(matrix: Mat4) -> Result<Transform, String> {
    if !matrix.is_finite() {
        return Err("节点变换超出有限浮点范围".into());
    }
    if matrix.determinant() == 0.0 {
        return Err("节点含零缩放，无法重新分解局部坐标".into());
    }
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    let transform = checked_trs(Transform {
        scale,
        rotation,
        translation,
    })?;
    let restored = transform.to_matrix().to_cols_array();
    if !matrix
        .to_cols_array()
        .into_iter()
        .zip(restored)
        .all(|(input, output)| (input - output).abs() <= 1.0e-5 * input.abs().max(1.0))
    {
        return Err("当前变换包含无法用 GLB 节点表示的剪切".into());
    }
    Ok(transform)
}

fn write_trs(node: &mut Value, transform: Transform) {
    node.as_object_mut()
        .expect("已校验 GLB 节点")
        .remove("matrix");
    node["translation"] = json!(transform.translation.to_array());
    node["rotation"] = json!(transform.rotation.to_array());
    node["scale"] = json!(transform.scale.to_array());
}

fn index_value(value: &Value) -> Result<usize, String> {
    value
        .as_u64()
        .and_then(|index| usize::try_from(index).ok())
        .ok_or_else(|| "GLB 索引无效".into())
}

fn list<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], String> {
    value.get(key).map_or(Ok(&[][..]), |value| {
        value
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| format!("GLB {key} 不是数组"))
    })
}

fn indices(value: &Value, key: &str) -> Result<Vec<usize>, String> {
    list(value, key)?.iter().map(index_value).collect()
}

fn object_at<'a>(json: &'a Value, key: &str, index: usize) -> Result<&'a Value, String> {
    list(json, key)?
        .get(index)
        .filter(|value| value.is_object())
        .ok_or_else(|| format!("GLB {key}[{index}] 不存在或无效"))
}

fn child_indices(json: &Value, parent: GlbParent) -> Result<Vec<usize>, String> {
    let (key, index, children) = parent.location();
    indices(object_at(json, key, index)?, children)
}

fn push(value: &mut Value, key: &str, item: Value) {
    value
        .as_object_mut()
        .expect("已校验 GLB 对象")
        .entry(key)
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .expect("已校验 GLB 数组")
        .push(item);
}

fn attach(json: &mut Value, parent: GlbParent, node: usize) {
    let (key, index, children) = parent.location();
    push(&mut json[key][index], children, json!(node));
}

fn detach(json: &mut Value, parent: GlbParent, node: usize) {
    let (key, index, children) = parent.location();
    let entry = json[key][index].as_object_mut().expect("已校验 GLB 父级");
    let values = entry
        .get_mut(children)
        .and_then(Value::as_array_mut)
        .expect("已确认旧父级关系");
    values.retain(|value| value.as_u64() != Some(node as u64));
    // Bevy 使用的 glTF 解析器要求场景保留 nodes，节点的 children 可以省略。
    if values.is_empty() && matches!(parent, GlbParent::Node(_)) {
        entry.remove(children);
    }
}
