//! 跨 GLB 移植完整节点子树及其标准资源引用，源文件编号保持稳定。

use super::super::{reader::read_accessor, writer::append_accessor};
use super::*;
use std::collections::{BTreeSet, HashMap};
mod selection;

impl GlbStructure {
    pub fn transfer_node_to(
        &mut self,
        index: usize,
        destination: &mut Self,
        parent: GlbParent,
    ) -> Result<usize, String> {
        self.transfer_node_to_in_space(index, destination, parent, Mat4::IDENTITY)
    }

    /// 变换将源 GLB 的场景坐标映射到目标 GLB 的场景坐标，保留发起操作的可见摆放位置。
    pub fn transfer_node_to_in_space(
        &mut self,
        index: usize,
        destination: &mut Self,
        parent: GlbParent,
        source_to_destination: Mat4,
    ) -> Result<usize, String> {
        child_indices(&destination.json, parent)?;
        let source_hierarchy = Hierarchy::read(&self.json)?;
        let source_parent = source_hierarchy.parent(index)?;
        let source_lineage = source_hierarchy.lineage(source_parent)?;
        let destination_lineage = Hierarchy::read(&destination.json)?.lineage(parent)?;
        check_static_ancestors(&self.json, source_lineage.iter().copied())?;
        check_static_ancestors(&destination.json, destination_lineage.iter().copied())?;
        let basis = invert(accumulate(&destination.json, &destination_lineage)?)?
            * source_to_destination
            * accumulate(&self.json, &source_lineage)?;
        let translation = basis.w_axis.truncate();
        let (replacement, animation_offset) = if basis.abs_diff_eq(Mat4::IDENTITY, 1.0e-6) {
            (None, None)
        } else if basis.abs_diff_eq(Mat4::from_translation(translation), 1.0e-6) {
            // 换父只带来固定平移时，根静置值与平移键一起补偿；旋转和缩放作用在同一组轴。
            let mut pose = self.node_transform(index)?;
            pose.translation += translation;
            (Some(checked_trs(pose)?), Some(translation))
        } else {
            (preserved_local(&self.json, index, basis)?, None)
        };
        let subtree = subtree_indices(&self.json, index)?;
        check_unbound(&self.json, &subtree)?;

        // 依赖检查、重新编号和编码准备发生在独立候选上。出错时两份草稿保持原状。
        let mut draft = destination.json.clone();
        let mut binary = destination.binary.to_vec();
        let mut ordered: Vec<_> = subtree.into_iter().collect();
        ordered.sort_unstable();
        let first_index = list(&draft, "nodes")?.len();
        let node_indices: HashMap<_, _> = ordered
            .iter()
            .enumerate()
            .map(|(offset, &source)| (source, first_index + offset))
            .collect();
        let mut copier = Copier {
            source: &self.json,
            binary: &self.binary,
            draft: &mut draft,
            output: &mut binary,
            indices: HashMap::new(),
            extensions: BTreeSet::new(),
            external_images: None,
        };
        for node in ordered {
            let mut value = object_at(&self.json, "nodes", node)?.clone();
            copier.check_extensions(&value, &[])?;
            copier.remap_field(&mut value, "mesh", "meshes")?;
            copier.remap_field(&mut value, "camera", "cameras")?;
            if let Some(children) = value.get_mut("children").and_then(Value::as_array_mut) {
                for child in children {
                    *child = json!(node_indices[&index_value(child)?]);
                }
            }
            if node == index
                && let Some(transform) = replacement
            {
                write_trs(&mut value, transform);
            }
            push(copier.draft, "nodes", value);
        }
        copier.copy_animations(list(&self.json, "animations")?, &node_indices, index, animation_offset)?;
        copier.publish_extensions()?;
        if !binary.is_empty() {
            if list(&draft, "buffers")?.is_empty() {
                push(&mut draft, "buffers", json!({ "byteLength": binary.len() }));
            } else {
                draft["buffers"][0]["byteLength"] = json!(binary.len());
            }
        }
        let inserted = node_indices[&index];
        attach(&mut draft, parent, inserted);
        // 源子树从可见层级脱离，保留原索引和未使用的标准资源，避免重新编号破坏其他引用。
        detach(&mut self.json, source_parent, index);
        destination.json = draft;
        destination.binary = binary.into();
        Ok(inserted)
    }
}

struct Copier<'a> {
    source: &'a Value,
    binary: &'a Arc<[u8]>,
    draft: &'a mut Value,
    output: &'a mut Vec<u8>,
    indices: HashMap<(&'static str, usize), usize>,
    extensions: BTreeSet<String>,
    external_images: Option<(&'a std::path::Path, &'a std::path::Path)>,
}

impl Copier<'_> {
    fn copy(&mut self, kind: &'static str, index: usize) -> Result<usize, String> {
        if let Some(&copied) = self.indices.get(&(kind, index)) {
            return Ok(copied);
        }
        let mut value = object_at(self.source, kind, index)?.clone();
        if kind != "materials" && kind != "textures" {
            self.check_extensions(&value, &[])?;
        }
        match kind {
            "meshes" => self.copy_mesh(&mut value)?,
            "materials" => self.copy_material(&mut value)?,
            "textures" => self.copy_texture(&mut value)?,
            "images" => {
                if let Some(uri) = value.get("uri").and_then(Value::as_str)
                    && !uri.starts_with("data:")
                {
                    let (root, source) = self.external_images.ok_or("跨 GLB 移动需要将所用贴图嵌入原 GLB")?;
                    let (bytes, mime) = selection::read_image(root,source,uri)?;
                    let start = self.output.len().next_multiple_of(4);
                    let length = bytes.len();
                    start.checked_add(length).and_then(|s| u32::try_from(s).ok()).ok_or("图像超过 GLB 缓冲容量")?;
                    self.output.resize(start,0);
                    self.output.extend_from_slice(&bytes);
                    let view = list(self.draft,"bufferViews")?.len();
                    push(self.draft,"bufferViews",json!({"buffer":0,"byteOffset":start,"byteLength":length}));
                    value.as_object_mut().ok_or("图像定义无效")?.remove("uri");
                    value["bufferView"] = json!(view);
                    value["mimeType"] = json!(mime);
                } else {
                    self.remap_field(&mut value, "bufferView", "bufferViews")?;
                }
            }
            "accessors" => {
                self.remap_field(&mut value, "bufferView", "bufferViews")?;
                if let Some(sparse) = value.get_mut("sparse") {
                    self.check_extensions(sparse, &[])?;
                    for key in ["indices", "values"] {
                        let accessor = sparse.get_mut(key).ok_or("GLB sparse accessor 不完整")?;
                        self.check_extensions(accessor, &[])?;
                        self.remap_field(accessor, "bufferView", "bufferViews")?;
                    }
                }
            }
            "bufferViews" => self.copy_buffer_view(&mut value)?,
            "cameras" | "samplers" => {}
            _ => return Err(format!("暂不支持跨 GLB 移动资源 {kind}")),
        }
        let next = list(self.draft, kind)?.len();
        push(self.draft, kind, value);
        self.indices.insert((kind, index), next);
        Ok(next)
    }

    fn remap_field(
        &mut self,
        value: &mut Value,
        field: &str,
        kind: &'static str,
    ) -> Result<(), String> {
        if let Some(reference) = value.get_mut(field) {
            *reference = json!(self.copy(kind, index_value(reference)?)?);
        }
        Ok(())
    }

    fn copy_mesh(&mut self, mesh: &mut Value) -> Result<(), String> {
        if declares(self.source, "extensionsUsed", "KHR_mesh_quantization")? {
            self.extensions.insert("KHR_mesh_quantization".into());
        }
        let primitives = mesh["primitives"]
            .as_array_mut()
            .ok_or("GLB 网格缺少 primitive")?;
        for primitive in primitives {
            if !primitive.is_object() {
                return Err("GLB primitive 结构无效".into());
            }
            self.check_extensions(primitive, &[])?;
            self.remap_field(primitive, "indices", "accessors")?;
            self.remap_field(primitive, "material", "materials")?;
            self.copy_attributes(&mut primitive["attributes"])?;
            if let Some(targets) = primitive.get_mut("targets") {
                for target in targets.as_array_mut().ok_or("GLB 形态目标列表无效")? {
                    self.copy_attributes(target)?;
                }
            }
        }
        Ok(())
    }

    fn copy_attributes(&mut self, attributes: &mut Value) -> Result<(), String> {
        let attributes = attributes.as_object_mut().ok_or("GLB 网格属性无效")?;
        for index in attributes.values_mut() {
            *index = json!(self.copy("accessors", index_value(index)?)?);
        }
        Ok(())
    }

    fn copy_material(&mut self, material: &mut Value) -> Result<(), String> {
        // 受理材质标准扩展的明确纹理字段。未知扩展可能引用其他文件全局索引，须拒绝。
        const EXTENSIONS: [(&str, &[&str]); 13] = [
            ("KHR_materials_anisotropy", &["anisotropyTexture"]),
            (
                "KHR_materials_clearcoat",
                &[
                    "clearcoatTexture",
                    "clearcoatRoughnessTexture",
                    "clearcoatNormalTexture",
                ],
            ),
            (
                "KHR_materials_diffuse_transmission",
                &[
                    "diffuseTransmissionTexture",
                    "diffuseTransmissionColorTexture",
                ],
            ),
            ("KHR_materials_dispersion", &[]),
            ("KHR_materials_emissive_strength", &[]),
            ("KHR_materials_ior", &[]),
            (
                "KHR_materials_iridescence",
                &["iridescenceTexture", "iridescenceThicknessTexture"],
            ),
            (
                "KHR_materials_pbrSpecularGlossiness",
                &["diffuseTexture", "specularGlossinessTexture"],
            ),
            (
                "KHR_materials_sheen",
                &["sheenColorTexture", "sheenRoughnessTexture"],
            ),
            (
                "KHR_materials_specular",
                &["specularTexture", "specularColorTexture"],
            ),
            ("KHR_materials_transmission", &["transmissionTexture"]),
            ("KHR_materials_unlit", &[]),
            ("KHR_materials_volume", &["thicknessTexture"]),
        ];
        let supported = EXTENSIONS.map(|(name, _)| name);
        self.check_extensions(material, &supported)?;
        if let Some(pbr) = material.get_mut("pbrMetallicRoughness") {
            self.check_extensions(pbr, &[])?;
            self.remap_texture_info(pbr, &["baseColorTexture", "metallicRoughnessTexture"])?;
        }
        self.remap_texture_info(
            material,
            &["normalTexture", "occlusionTexture", "emissiveTexture"],
        )?;
        if let Some(extensions) = material
            .get_mut("extensions")
            .and_then(Value::as_object_mut)
        {
            for (name, fields) in EXTENSIONS {
                if let Some(extension) = extensions.get_mut(name) {
                    self.remap_texture_info(extension, fields)?;
                }
            }
        }
        Ok(())
    }

    fn remap_texture_info(&mut self, value: &mut Value, fields: &[&str]) -> Result<(), String> {
        for &field in fields {
            if let Some(texture) = value.get_mut(field) {
                self.check_extensions(texture, &["KHR_texture_transform"])?;
                self.remap_field(texture, "index", "textures")?;
            }
        }
        Ok(())
    }

    fn copy_texture(&mut self, texture: &mut Value) -> Result<(), String> {
        self.check_extensions(texture, &["KHR_texture_basisu", "EXT_texture_webp"])?;
        self.remap_field(texture, "sampler", "samplers")?;
        self.remap_field(texture, "source", "images")?;
        if let Some(extensions) = texture.get_mut("extensions").and_then(Value::as_object_mut) {
            for extension in extensions.values_mut() {
                self.remap_field(extension, "source", "images")?;
            }
        }
        Ok(())
    }

    fn copy_buffer_view(&mut self, view: &mut Value) -> Result<(), String> {
        if index_field(view, "buffer")? != 0 {
            return Err("跨 GLB 移动只支持内嵌二进制资源".into());
        }
        let offset = view
            .get("byteOffset")
            .map(index_value)
            .transpose()?
            .unwrap_or(0);
        let length = index_field(view, "byteLength")?;
        let end = offset
            .checked_add(length)
            .ok_or("GLB BufferView 大小溢出")?;
        let declared = index_field(&self.source["buffers"][0], "byteLength")?;
        let bytes = self
            .binary
            .get(offset..end)
            .filter(|_| end <= declared)
            .ok_or("GLB BufferView 超出原缓冲范围")?;
        // 保持 bufferView 原有四字节余数，使共享/交错 accessor 的相对偏移继续对齐。
        let padding = (offset % 4 + 4 - self.output.len() % 4) % 4;
        let start = self
            .output
            .len()
            .checked_add(padding)
            .ok_or("GLB 缓冲偏移溢出")?;
        let size = start
            .checked_add(bytes.len())
            .and_then(|size| u32::try_from(size).ok());
        if size.is_none() {
            return Err("追加资源后 GLB 缓冲超过 4 GiB".into());
        }
        self.output
            .try_reserve(padding + bytes.len())
            .map_err(|_| "无法为跨 GLB 资源分配缓冲")?;
        self.output.resize(start, 0);
        self.output.extend_from_slice(bytes);
        view["byteOffset"] = json!(start);
        Ok(())
    }

    fn check_extensions(&mut self, element: &Value, supported: &[&str]) -> Result<(), String> {
        let Some(extensions) = element.get("extensions") else {
            return Ok(());
        };
        for name in extensions.as_object().ok_or("GLB 扩展字段无效")?.keys() {
            if !supported.contains(&name.as_str()) {
                return Err(format!("暂不支持跨 GLB 移动使用扩展 {name} 的资源"));
            }
            self.extensions.insert(name.clone());
        }
        Ok(())
    }

    fn publish_extensions(&mut self) -> Result<(), String> {
        for name in &self.extensions {
            if !declares(self.draft, "extensionsUsed", name)? {
                push(self.draft, "extensionsUsed", json!(name));
            }
            if declares(self.source, "extensionsRequired", name)?
                && !declares(self.draft, "extensionsRequired", name)?
            {
                push(self.draft, "extensionsRequired", json!(name));
            }
        }
        Ok(())
    }

    fn copy_animations(
        &mut self,
        animations: &[Value],
        nodes: &HashMap<usize, usize>,
        root: usize,
        translation: Option<Vec3>,
    ) -> Result<(), String> {
        for animation in animations {
            let mut channels = Vec::new();
            let mut samplers = Vec::new();
            let mut sampler_indices = HashMap::new();
            for channel in list(animation, "channels")? {
                let target = &channel["target"];
                if target
                    .pointer("/extensions/KHR_animation_pointer")
                    .is_some()
                {
                    return Err("该 GLB 使用跨属性动画指针，无法安全重映射节点".into());
                }
                let Some(original) = target.get("node") else {
                    continue;
                };
                let original = index_value(original)?;
                let Some(&node) = nodes.get(&original) else {
                    continue;
                };
                self.check_extensions(channel, &[])?;
                self.check_extensions(target, &[])?;
                let original_sampler = index_field(channel, "sampler")?;
                let offset = translation
                    .filter(|_| original == root && target["path"].as_str() == Some("translation"));
                // 旧 sampler 可被移动根、其子节点与其他对象共享。补偿和原值分别使用各自的目标 sampler。
                let key = (original_sampler, offset.is_some());
                let sampler = if let Some(&sampler) = sampler_indices.get(&key) {
                    sampler
                } else {
                    let mut value = object_at(animation, "samplers", original_sampler)?.clone();
                    self.check_extensions(&value, &[])?;
                    if let Some(offset) = offset {
                        value["output"] = json!(self.offset_animation_output(&value, offset)?);
                    } else {
                        self.remap_field(&mut value, "output", "accessors")?;
                    }
                    self.remap_field(&mut value, "input", "accessors")?;
                    let sampler = samplers.len();
                    samplers.push(value);
                    sampler_indices.insert(key, sampler);
                    sampler
                };
                let mut channel = channel.clone();
                channel["target"]["node"] = json!(node);
                channel["sampler"] = json!(sampler);
                channels.push(channel);
            }
            if !channels.is_empty() {
                self.check_extensions(animation, &[])?;
                let mut value = animation.clone();
                value["channels"] = json!(channels);
                value["samplers"] = json!(samplers);
                list(self.draft, "animations")?;
                push(self.draft, "animations", value);
            }
        }
        Ok(())
    }

    fn offset_animation_output(&mut self, sampler: &Value, offset: Vec3) -> Result<usize, String> {
        let (stride, value_start) = match sampler["interpolation"].as_str().unwrap_or("LINEAR") {
            "STEP" | "LINEAR" => (3, 0),
            "CUBICSPLINE" => (9, 3),
            _ => return Err("该平移动画的插值方式无法补偿换父偏移".into()),
        };
        let output = index_field(sampler, "output")?;
        let accessor = object_at(self.source, "accessors", output)?;
        self.check_extensions(accessor, &[])?;
        let view = object_at(
            self.source,
            "bufferViews",
            index_field(accessor, "bufferView")?,
        )?;
        self.check_extensions(view, &[])?;
        let count = index_field(
            object_at(self.source, "accessors", index_field(sampler, "input")?)?,
            "count",
        )?;
        let mut values = read_accessor(self.source, std::slice::from_ref(self.binary), output, 3)?;
        if count.checked_mul(stride) != Some(values.len()) {
            return Err("关键帧时间与平移输出数量不一致".into());
        }
        for key in values.chunks_exact_mut(stride) {
            // 三次曲线的切线是每秒变化率，加常量只应改中间的值。
            let value = &mut key[value_start..value_start + 3];
            let shifted = Vec3::from_slice(value) + offset;
            if !shifted.is_finite() {
                return Err("补偿后的动画位置超出有限浮点范围".into());
            }
            value.copy_from_slice(&shifted.to_array());
        }
        append_accessor(self.draft, self.output, &values, 3, false)
    }
}

fn declares(json: &Value, field: &str, name: &str) -> Result<bool, String> {
    Ok(list(json, field)?
        .iter()
        .any(|entry| entry.as_str() == Some(name)))
}
