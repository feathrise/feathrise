//! 兼容角色的双容器蒙皮预览：后台核对骨架，隐藏扫描与骨架姿态共同完整发布。

pub(super) use super::resources::BindingState;
use super::{
    components::BoundModel, data::AnimationDocument, resources::AnimationEditorState, types::*,
};
use crate::{SceneEditorSystemSet, components::SceneEditorOwnedEntity};
use bevy::{
    camera::visibility::{NoFrustumCulling, RenderLayers},
    gltf::GltfAssetLabel,
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
    world_serialization::{WorldAsset, WorldAssetRoot, WorldInstanceReady},
};
use feathrise_editor_core::constants::render_layers::SCENE_EDITOR_ASSET_PREVIEW_LAYER;
use std::collections::HashMap;
#[cfg(test)]
use std::path::PathBuf;

pub(super) fn install(app: &mut App) {
    app.init_resource::<BindingState>()
        .add_observer(ready)
        .add_systems(
            Update,
            drive
                .after(super::super::loading::poll_asset_preview_loading)
                .before(super::preview::drive)
                .in_set(SceneEditorSystemSet::Tasks),
        );
}

fn compatible(
    source: &AnimationDocument,
    target: &AnimationDocument,
) -> Result<SkeletonMapping, String> {
    let mut target_names = HashMap::new();
    for (i, node) in target.nodes.iter().enumerate() {
        if target_names.insert(node.name.as_str(), i).is_some() {
            return Err(format!("模型有重名节点 {}，无法唯一绑定", node.name));
        }
    }
    let mut animated = vec![false; source.nodes.len()];
    for channel in source.clips.iter().flat_map(|c| &c.channels) {
        animated[channel.node] = true;
    }
    let mut names = HashMap::new();
    for (i, n) in source
        .nodes
        .iter()
        .enumerate()
        .filter(|(i, _)| animated[*i])
    {
        let target_index = *target_names
            .get(n.name.as_str())
            .ok_or_else(|| format!("模型缺少骨骼 {}", n.name))?;
        let other = &target.nodes[target_index];
        if names.insert(n.name.clone(), i).is_some() {
            return Err(format!("动画有重名骨骼 {}", n.name));
        }
        let parent_animated = n.parent.filter(|p| animated[*p]);
        if let Some(parent) = parent_animated {
            if other.parent.map(|p| target.nodes[p].name.as_str())
                != Some(source.nodes[parent].name.as_str())
            {
                return Err(format!("骨骼 {} 的父级不兼容", n.name));
            }
            let length = Vec3::from_array(n.translation).length();
            let target_length = Vec3::from_array(other.translation).length();
            if (length - target_length).abs() > 0.005_f32.max(length * 0.01) {
                return Err(format!(
                    "骨骼 {} 的骨长/单位不同，需先在Blender重定向",
                    n.name
                ));
            }
            let q = Quat::from_array(n.rotation).normalize();
            let target_q = Quat::from_array(other.rotation).normalize();
            if q.dot(target_q).abs() < 0.999 {
                return Err(format!("骨骼 {} 的局部坐标不同，需先重定向", n.name));
            }
        } else {
            // 外层包装名称可以不同，但世界轴向与单位必须一致；根骨的初始位移可来自不同动作。
            let ancestors = |doc: &AnimationDocument, mut parent: Option<usize>| {
                let mut matrix = Mat4::IDENTITY;
                while let Some(p) = parent {
                    let n = &doc.nodes[p];
                    matrix = Mat4::from_scale_rotation_translation(
                        Vec3::from_array(n.scale),
                        Quat::from_array(n.rotation).normalize(),
                        Vec3::from_array(n.translation),
                    ) * matrix;
                    parent = n.parent;
                }
                matrix
            };
            if !ancestors(source, n.parent).abs_diff_eq(ancestors(target, other.parent), 0.001) {
                return Err("骨架外层坐标或单位不兼容，需先在Blender统一".into());
            }
        }
        if !Vec3::from_array(n.scale).abs_diff_eq(Vec3::from_array(other.scale), 0.001) {
            return Err(format!("骨骼 {} 的缩放不同", n.name));
        }
    }
    if names.is_empty() {
        return Err("动画没有可绑定骨骼".into());
    }
    Ok(SkeletonMapping { names })
}

fn ready(
    event: On<WorldInstanceReady>,
    roots: Query<&BoundModel>,
    mut binding: ResMut<BindingState>,
    mut commands: Commands,
    mut editor: ResMut<AnimationEditorState>,
) {
    let Ok(marker) = roots.get(event.entity) else {
        return;
    };
    let bank = &mut binding.banks[marker.0];
    if bank.root != Some(event.entity) {
        return;
    }
    commands.entity(event.entity).try_insert(Visibility::Hidden);
    if bank.ready {
        bank.stack.clear();
        bank.stack.push((event.entity, 0));
        // 模型热更不得继续使用旧骨架映射；先退出绑定，作者可按按钮重验新文件。
        binding.failed = true;
        binding.active = false;
        editor.model_status = "模型已热更新，请重新绑定以检查新骨架".into();
        if let Some(s) = editor.current_mut() {
            s.model_path = None;
        }
        return;
    }
    bank.ready = true;
    bank.stack.push((event.entity, 0));
}

impl BindingState {
    pub fn hide(&mut self, commands: &mut Commands) {
        for bank in &self.banks {
            if let Some(root) = bank.root {
                commands.entity(root).try_insert(Visibility::Hidden);
            }
        }
        self.active = false;
    }
    pub fn ready(&self) -> bool {
        self.mapping.is_some()
            && !self.failed
            && !self.retiring
            && self.banks.iter().all(|b| b.scanned)
    }
    pub fn node(&self, bank: usize, node: usize) -> Option<Entity> {
        self.banks[bank].nodes.get(&node).copied()
    }
    pub fn publish(&mut self, commands: &mut Commands, bank: usize) {
        if !self.ready() {
            return;
        }
        for (i, b) in self.banks.iter().enumerate() {
            if let Some(root) = b.root {
                commands.entity(root).try_insert(if i == bank {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
        }
        self.active = true;
    }
}

fn drive(
    mut commands: Commands,
    mut editor: ResMut<AnimationEditorState>,
    documents: Res<super::super::DocumentState>,
    assets: Res<AssetServer>,
    children: Query<&Children>,
    names: Query<&Name>,
    meshes: Query<(), With<Mesh3d>>,
    mut cameras: Query<&mut Camera, Without<super::super::types::AssetPreviewCamera>>,
    mut binding: ResMut<BindingState>,
) {
    let desired = editor
        .visible()
        .then(|| {
            editor
                .current()?
                .model_path
                .as_ref()
                .map(|model| BindingIdentity {
                    source: editor.active.clone().unwrap(),
                    revision: editor.source_revision,
                    request: editor.model_binding_revision,
                    model: model.clone(),
                })
        })
        .flatten();
    if binding.identity != desired && !binding.retiring {
        for bank in &binding.banks {
            if let Some(root) = bank.root {
                commands.entity(root).try_insert(Visibility::Hidden);
            }
        }
        binding.active = false;
        binding.retiring = true;
        binding.failed = false;
        binding.revision = binding.revision.wrapping_add(1);
    }
    if binding.retiring {
        for bank in &mut binding.banks {
            // 切源可能发生在隔离扫描途中；先按预算补齐未访问子树，再逆序回收，禁止根递归析构大模型。
            for _ in 0..64 {
                let Some((entity, index)) = bank.stack.last_mut() else {
                    break;
                };
                if *index == 0 && Some(*entity) != bank.root {
                    bank.entities.push(*entity);
                }
                if let Some(child) = children
                    .get(*entity)
                    .ok()
                    .and_then(|c| c.get(*index))
                    .copied()
                {
                    *index += 1;
                    bank.stack.push((child, 0));
                } else {
                    bank.stack.pop();
                }
            }
            if !bank.stack.is_empty() {
                continue;
            }
            // 反向回收先子后父，单次最多64节点，避免递归删除整模型占用一帧。
            for _ in 0..32 {
                if let Some(entity) = bank.entities.pop() {
                    commands.entity(entity).try_despawn();
                } else {
                    break;
                }
            }
        }
        if binding
            .banks
            .iter()
            .any(|b| !b.entities.is_empty() || !b.stack.is_empty())
        {
            return;
        }
        // 若还在加载，根删除也取消尚未安装的实例；旧任务继续占槽直到真正结束。
        let banks = std::mem::take(&mut binding.banks);
        for bank in &banks {
            if let Some(root) = bank.root {
                commands.entity(root).try_despawn();
            }
        }
        if let Some(mapping) = binding.mapping.take() {
            binding.retired.mappings.push(mapping);
        }
        for bank in banks {
            if !bank.nodes.is_empty() {
                binding.retired.banks.push(bank);
            }
        }
        binding.handle = None;
        binding.retiring = false;
        binding.identity = desired.clone();
    }
    if let Some(result) = binding
        .task
        .as_mut()
        .and_then(|(_, task)| check_ready(task))
    {
        let (identity, _) = binding.task.take().unwrap();
        if identity == desired {
            match result {
                Ok(Some(mapping)) => {
                    binding.mapping = Some(mapping);
                    editor.model_status = "骨架匹配，正在加载角色蒙皮".into();
                }
                Ok(None) => {}
                Err(error) => {
                    binding.failed = true;
                    editor.model_status = format!("无法绑定：{error}");
                }
            }
        } else if let Ok(Some(mapping)) = result {
            binding.retired.mappings.push(mapping);
        }
    }
    let load = desired.is_some() && binding.mapping.is_none() && !binding.failed;
    if binding.task.is_none()
        && (load || !binding.retired.banks.is_empty() || !binding.retired.mappings.is_empty())
    {
        let source = load.then(|| editor.document_snapshot()).flatten();
        let path = desired
            .as_ref()
            .map(|id| documents.asset_root().join(&id.model));
        let retired = std::mem::take(&mut binding.retired);
        binding.task = Some((
            desired.clone(),
            IoTaskPool::get().spawn(async move {
                drop(retired);
                if let (Some(source), Some(path)) = (source, path) {
                    let target = AnimationDocument::load(&path)?;
                    compatible(&source, &target).map(Some)
                } else {
                    Ok(None)
                }
            }),
        ));
        if load {
            return;
        }
    }
    let Some(identity) = desired else {
        return;
    };
    if binding.failed || binding.mapping.is_none() {
        return;
    }
    if binding.handle.is_none() {
        let handle: Handle<WorldAsset> =
            assets.load(GltfAssetLabel::Scene(0).from_asset(identity.model));
        for (index, bank) in binding.banks.iter_mut().enumerate() {
            bank.root = Some(
                commands
                    .spawn((
                        Name::new("动画绑定角色"),
                        BoundModel(index),
                        SceneEditorOwnedEntity,
                        WorldAssetRoot(handle.clone()),
                        Transform::default(),
                        Visibility::Hidden,
                        RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER),
                    ))
                    .id(),
            );
        }
        binding.handle = Some(handle);
        return;
    }
    if binding
        .handle
        .as_ref()
        .is_some_and(|h| matches!(assets.load_state(h.id()), bevy::asset::LoadState::Failed(_)))
    {
        binding.failed = true;
        editor.model_status = "角色模型加载失败，请检查GLB资源".into();
        return;
    }
    let binding = &mut *binding;
    let mapping = binding.mapping.as_ref().unwrap();
    for bank in &mut binding.banks {
        if !bank.ready || bank.scanned {
            continue;
        }
        for _ in 0..128 {
            let Some((entity, child_index)) = bank.stack.last_mut() else {
                bank.scanned = true;
                if bank.nodes.len() != mapping.names.len() || bank.mesh_count == 0 {
                    binding.failed = true;
                    editor.model_status = "模型Scene0缺少匹配骨骼或蒙皮网格".into();
                }
                binding.revision = binding.revision.wrapping_add(1);
                break;
            };
            if *child_index == 0 {
                commands.entity(*entity).try_insert((
                    RenderLayers::layer(SCENE_EDITOR_ASSET_PREVIEW_LAYER),
                    NoFrustumCulling,
                ));
                if let Ok(mut camera) = cameras.get_mut(*entity) {
                    camera.is_active = false;
                }
                if meshes.contains(*entity) {
                    bank.mesh_count += 1;
                }
                if let Ok(name) = names.get(*entity)
                    && let Some(&source) = mapping.names.get(name.as_str())
                {
                    if bank.nodes.insert(source, *entity).is_some() {
                        binding.failed = true;
                        editor.model_status = "实例中出现重名骨骼，绑定已取消".into();
                    }
                }
                if Some(*entity) != bank.root {
                    bank.entities.push(*entity);
                }
            }
            if let Some(child) = children
                .get(*entity)
                .ok()
                .and_then(|c| c.get(*child_index))
                .copied()
            {
                *child_index += 1;
                bank.stack.push((child, 0));
            } else {
                bank.stack.pop();
            }
        }
    }
    if binding.ready() {
        editor.model_status = "角色已绑定 · 同骨架蒙皮预览".into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mismatched_bone_units_are_rejected() {
        let source = super::super::data::tests::test_document();
        let mut target = source.clone();
        for node in &mut target.nodes {
            node.scale = [0.01; 3];
        }
        assert!(compatible(&source, &target).is_err());
    }
    #[test]
    fn neutral_rig_accepts_matching_model_and_rejects_different_proportions() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/animation_preview");
        let source = AnimationDocument::load(&root.join("animated_rig.glb")).unwrap();
        let mut target = AnimationDocument::load(&root.join("skinned_rig.glb")).unwrap();
        assert!(compatible(&source, &target).is_ok());
        // 仍核对实际GLB节点和父级；同名骨骼的比例变化不能被直接绑定接受。
        target.nodes[2].translation[0] *= 1.2;
        assert!(compatible(&source, &target).is_err());
    }
}
