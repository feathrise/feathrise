//! 同一 GLB 的节点实例材质槽。共享网格只在写盘时复制描述，几何、蒙皮及贴图仍引用原资源。

use super::{AnimationDocument, reader::array};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct MaterialSlot {
    pub node: usize,
    pub primitive: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct MaterialChange {
    slot: MaterialSlot,
    before: Option<usize>,
    after: usize,
}

type Overrides = BTreeMap<MaterialSlot, Option<usize>>;

/// 仅第一次编辑时在后台冻结网格 JSON 和实例引用。跨保存撤销仍能还原原先的共享关系。
#[derive(Debug)]
struct MeshOrigin {
    meshes: Vec<Value>,
    references: Vec<Option<usize>>,
    users: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct MaterialDraft {
    origin: Option<Arc<MeshOrigin>>,
    current: Arc<Overrides>,
    written: Arc<Overrides>,
}

fn index(value: &Value) -> Option<usize> {
    usize::try_from(value.as_u64()?).ok()
}

impl MeshOrigin {
    fn read(file: &Value) -> Result<Self, String> {
        let meshes = array(file, "meshes").to_vec();
        let mut users = vec![0; meshes.len()];
        let mut references = Vec::with_capacity(array(file, "nodes").len());
        for node in array(file, "nodes") {
            let mesh = node
                .get("mesh")
                .map(|mesh| index(mesh).ok_or("节点网格索引无效"))
                .transpose()?;
            if let Some(id) = mesh {
                *users.get_mut(id).ok_or("节点引用不存在的网格")? += 1;
            }
            references.push(mesh);
        }
        Ok(Self {
            meshes,
            users,
            references,
        })
    }
}

impl AnimationDocument {
    pub fn can_replace_material(&self) -> bool {
        !self.read_only
            && !array(&self.source.json, "materials").is_empty()
            && !array(&self.source.json, "meshes").is_empty()
    }

    fn material_mesh(&self, node: usize) -> Option<&Value> {
        if let Some(origin) = &self.materials.origin {
            return origin.meshes.get((*origin.references.get(node)?)?);
        }
        let mesh = self.disk_mesh_index(node)?;
        array(&self.source.json, "meshes").get(mesh)
    }

    pub fn disk_mesh_index(&self, node: usize) -> Option<usize> {
        index(self.source.json.get("nodes")?.get(node)?.get("mesh")?)
    }

    pub fn material_slot_count(&self, node: usize) -> usize {
        self.material_mesh(node)
            .map_or(0, |mesh| array(mesh, "primitives").len())
    }

    /// 外层 None 表示槽不存在，内层 None 是 glTF 合法的默认材质。
    pub fn slot_material(&self, slot: MaterialSlot) -> Option<Option<usize>> {
        let primitive = array(self.material_mesh(slot.node)?, "primitives").get(slot.primitive)?;
        Some(
            self.materials
                .current
                .get(&slot)
                .copied()
                .unwrap_or_else(|| primitive.get("material").and_then(index)),
        )
    }

    pub fn material_changes(&self) -> &Arc<BTreeMap<MaterialSlot, Option<usize>>> {
        &self.materials.current
    }

    pub fn has_material_draft(&self) -> bool {
        self.materials.origin.is_some()
    }

    pub fn plan_material(
        &self,
        slot: MaterialSlot,
        material: usize,
    ) -> Result<MaterialChange, String> {
        if self.read_only {
            return Err("该文件只读，不能替换材质槽".into());
        }
        array(&self.source.json, "materials")
            .get(material)
            .ok_or("所选材质不存在")?;
        let before = self.slot_material(slot).ok_or("所选对象或图元槽已改变")?;
        if before == Some(material) {
            return Err("该图元已经使用所选材质".into());
        }
        Ok(MaterialChange {
            slot,
            before,
            after: material,
        })
    }

    pub(crate) fn apply_material(
        &mut self,
        edit: &MaterialChange,
        forward: bool,
    ) -> Result<(), String> {
        let (from, to) = if forward {
            (edit.before, Some(edit.after))
        } else {
            (Some(edit.after), edit.before)
        };
        if self.slot_material(edit.slot) != Some(from) {
            return Err("材质槽已经改变，请重新选择".into());
        }
        if self.materials.origin.is_none() {
            self.materials.origin = Some(Arc::new(MeshOrigin::read(&self.source.json)?));
        }
        let origin = self.material_mesh(edit.slot.node).ok_or("缺少原网格")?["primitives"]
            [edit.slot.primitive]
            .get("material")
            .and_then(index);
        let changes = Arc::make_mut(&mut self.materials.current);
        if to == origin {
            changes.remove(&edit.slot);
        } else {
            changes.insert(edit.slot, to);
        }
        Ok(())
    }
}

impl MaterialDraft {
    pub(super) fn rebase(&mut self) {
        self.written = self.current.clone();
    }

    pub(super) fn encode(&self, json: &mut Value) -> Result<bool, String> {
        if Arc::ptr_eq(&self.current, &self.written) || self.current == self.written {
            return Ok(false);
        }
        let origin = self.origin.as_ref().ok_or("缺少原材质槽布局")?;
        let mut meshes = origin.meshes.clone();
        let nodes = json["nodes"].as_array_mut().ok_or("GLB 节点列表无效")?;
        for (node, mesh) in nodes.iter_mut().zip(&origin.references) {
            if let Some(mesh) = mesh {
                node["mesh"] = json!(mesh);
            }
        }
        let mut instance = None;
        let mut mesh_id = 0;
        for (&slot, &material) in self.current.iter() {
            if instance != Some(slot.node) {
                let source = origin.references[slot.node].ok_or("图元所属节点缺少网格")?;
                mesh_id = if origin.users[source] > 1 {
                    // 只复制 mesh/primitive 描述。其他节点及所有 accessor 的原索引都保持。
                    meshes.push(origin.meshes[source].clone());
                    meshes.len() - 1
                } else {
                    source
                };
                nodes[slot.node]["mesh"] = json!(mesh_id);
                instance = Some(slot.node);
            }
            let primitive = meshes[mesh_id]["primitives"][slot.primitive]
                .as_object_mut()
                .ok_or("GLB 图元无效")?;
            if let Some(material) = material {
                primitive.insert("material".into(), json!(material));
            } else {
                primitive.remove("material");
            }
        }
        json["meshes"] = Value::Array(meshes);
        Ok(true)
    }
}
