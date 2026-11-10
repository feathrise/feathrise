//! 动画文档的真实姿态预览：后台采样 TRS，专层骨架复用资产浏览唯一相机与 RTT。
//!
//! 纯动画不必伪造可显示的 Scene0 网格。骨节和连杆在独立临时容器中完整准备后才显示，
//! 两个骨架容器交替承接姿态，候选与绑定角色按固定预算共同写完后原子切换。

use super::super::types::*;
use super::{
    data::{AnimationDocument, Channel, Node},
    resources::{AnimationEditorState, Session},
};
use crate::{MapEditorSystemSet, components::MapEditorOwnedEntity};
use bevy::asset::{DependencyLoadState, LoadState, RecursiveDependencyLoadState};
use bevy::{
    camera::visibility::{NoFrustumCulling, RenderLayers},
    prelude::*,
    tasks::{AsyncComputeTaskPool, Task, futures::check_ready},
};
use feathrise_editor_core::constants::render_layers::MAP_EDITOR_ASSET_PREVIEW_LAYER;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Weak},
};

const NODES_PER_FRAME: usize = 64;

#[derive(Clone, PartialEq, Eq)]
struct SourceIdentity {
    path: PathBuf,
    revision: u64,
    scene: Entity,
}

pub(super) struct Pose {
    /// 仅保留精确文档的弱身份；旧完整画面可以继续显示，但不能按新文档或动作开始编辑。
    document: Weak<AnimationDocument>,
    clip: usize,
    draft: Option<(usize, Transform)>,
    pub time: f32,
    pub local: Vec<Transform>,
    model_local: Option<Vec<Option<Transform>>>,
    pub world: Vec<Mat4>,
    pub joints: Vec<Transform>,
    bones: Vec<Transform>,
    pub animated: Vec<bool>,
    bounds: AssetPreviewModelBounds,
}

impl Pose {
    fn matches_session(&self, session: &Session) -> bool {
        self.clip == session.clip
            && std::ptr::eq(self.document.as_ptr(), Arc::as_ptr(&session.document))
    }

    pub(super) fn can_start_edit(&self, editor: &AnimationEditorState) -> bool {
        !editor.retargeting() && editor.current().is_some_and(|session| {
            self.matches_session(session)
                && self.draft == session.pose
                // 播放中从实际显示帧起手；暂停定位或还原姿势后必须等当前完整结果，不能跳回旧帧。
                && (editor.playing || self.time == session.position)
        })
    }

    pub(super) fn can_draw_tools(&self, editor: &AnimationEditorState, dragging: bool) -> bool {
        !editor.retargeting() && editor.current().is_some_and(|session| self.matches_session(session))
            && (dragging || self.can_start_edit(editor))
    }
}

struct PendingPose {
    identity: Option<SourceIdentity>,
    revision: u64,
    binding_revision: u64,
    clip: usize,
    time: u32,
    task: Task<Option<Arc<Pose>>>,
}

#[derive(Clone, Copy)]
struct BoneEntities {
    joint: [Entity; 2],
    bone: [Entity; 2],
}

struct Retiring {
    roots: [Entity; 2],
    entities: std::vec::IntoIter<BoneEntities>,
}

#[derive(Default)]
struct RetiredData {
    poses: Vec<Arc<Pose>>,
}

impl RetiredData {
    fn occupied(&self) -> bool {
        !self.poses.is_empty()
    }
}

#[derive(Resource, Default)]
pub(in crate::ui::asset_preview) struct PreviewRuntime {
    binding_revision: u64,
    identity: Option<SourceIdentity>,
    pending: Option<PendingPose>,
    pose: Option<Arc<Pose>>,
    pub(super) published_pose: Option<Arc<Pose>>,
    applied: Option<(u64, usize, u32)>,
    roots: Option<[Entity; 2]>,
    entities: Vec<BoneEntities>,
    retiring: Option<Retiring>,
    mesh: Option<Handle<Mesh>>,
    joint_material: Option<Handle<StandardMaterial>>,
    animated_material: Option<Handle<StandardMaterial>>,
    front: usize,
    applying: bool,
    cursor: usize,
    published_bounds: Option<AssetPreviewModelBounds>,
    using_retarget_bounds: bool,
    complete: bool,
    retired: RetiredData,
}

impl PreviewRuntime {
    #[cfg(test)]
    pub(super) fn for_input_tests(document: Arc<AnimationDocument>) -> Self {
        Self::for_input_tests_at(document, 0, 0.0, None)
    }
    #[cfg(test)]
    pub(super) fn for_input_tests_at(
        document: Arc<AnimationDocument>, clip: usize, time: f32, draft: Option<(usize, Transform)>,
    ) -> Self {
        Self {
            published_pose: Some(Arc::new(prepare_pose_draft(document, clip, time, draft))),
            ..default()
        }
    }
    fn skeleton_ready(&self, source: &Path, scene: Entity) -> bool {
        self.complete
            && !self.entities.is_empty()
            && self
                .identity
                .as_ref()
                .is_some_and(|identity| identity.path == source && identity.scene == scene)
    }

    pub(in crate::ui::asset_preview) fn document_ready(
        &self,
        editor: &AnimationEditorState,
        preview: &AssetPreviewState,
    ) -> bool {
        editor.visible()
            && editor
                .active_document()
                .is_some_and(|document| !document.clips.is_empty())
            && editor
                .source()
                .zip(preview.scene_root())
                .is_some_and(|(source, scene)| {
                    preview
                        .selected_path
                        .as_ref()
                        .is_some_and(|path| source.ends_with(Path::new(path)))
                        && self.skeleton_ready(source, scene)
                })
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<PreviewRuntime>().add_systems(
        Update,
        drive
            .after(super::super::loading::poll_asset_preview_loading)
            .before(super::super::navigation::sync_asset_preview_view)
            .in_set(MapEditorSystemSet::Tasks),
    );
}

#[cfg(feature = "validation")]
pub(crate) struct RigAnimationProbe {
    pub model_path: String,
    pub source: PathBuf,
    pub clip_index: usize,
    pub clip_name: String,
    pub time: f32,
    pub root: Entity,
    pub joints: Vec<(String, Quat)>,
}

/// 读取真实已发布模型容器的骨骼变换；测试不能用待应用的后台姿态代替实际蒙皮。
#[cfg(feature = "validation")]
pub(crate) fn rig_animation_probe(world: &World) -> Option<RigAnimationProbe> {
    let editor = world.get_resource::<AnimationEditorState>()?;
    let runtime = world.get_resource::<PreviewRuntime>()?;
    let binding = world.get_resource::<super::binding::BindingState>()?;
    let identity = binding.identity.as_ref()?;
    let session = editor.current()?;
    let pose = runtime.published_pose.as_ref()?;
    if !editor.visible() || !binding.active || !binding.ready() || !identity.retarget
        || !world.get_resource::<AssetPreviewState>()?.render_visible
        || editor.source() != Some(identity.source.as_path())
        || editor.source_revision != identity.revision
        || editor.model_binding_revision != identity.request
        || session.model_path.as_ref() != Some(&identity.model)
        || pose.model_local.is_none()
        || !pose.matches_session(session)
    {
        return None;
    }
    let bank = &binding.banks[runtime.front];
    let root = bank.root?;
    if world.get::<Visibility>(root) != Some(&Visibility::Inherited) {
        return None;
    }
    let mut joints = binding.mapping.as_ref()?.names.iter().map(|(name, source)| {
        let entity = bank.nodes.get(source)?;
        let rotation = world.get::<Transform>(*entity)?.rotation;
        Some((name.clone(), rotation))
    }).collect::<Option<Vec<_>>>()?;
    joints.sort_by(|a, b| a.0.cmp(&b.0));
    Some(RigAnimationProbe {
        model_path: identity.model.clone(),
        source: identity.source.clone(),
        clip_index: session.clip,
        clip_name: session.document.clips[session.clip].name.clone(),
        time: pose.time,
        root,
        joints,
    })
}

#[cfg(feature = "validation")]
pub(crate) fn rig_animation_diagnostic(world: &World) -> Option<&str> {
    let editor = world.get_resource::<AnimationEditorState>()?;
    Some(if editor.model_status.is_empty() { &editor.status } else { &editor.model_status })
}

fn rotation(value: [f32; 4]) -> Quat {
    let quaternion = Quat::from_array(value);
    if quaternion.is_finite() && quaternion.length_squared() > 1e-12 {
        quaternion.normalize()
    } else {
        Quat::IDENTITY
    }
}

/// 通道插值与 glTF 作者面板共用同一实现，防止保存值和预览值使用不同曲线规则。
fn sampled_local(nodes: &[Node], channels: &[Channel], time: f32) -> Vec<Transform> {
    let mut local: Vec<_> = nodes
        .iter()
        .map(|node| Transform {
            translation: Vec3::from_array(node.translation),
            rotation: rotation(node.rotation),
            scale: Vec3::from_array(node.scale),
        })
        .collect();
    for channel in channels {
        let Some(transform) = local.get_mut(channel.node) else {
            continue;
        };
        let value = channel.sample(time);
        if value.iter().any(|value| !value.is_finite()) {
            continue;
        }
        match (channel.path.as_str(), value.as_slice()) {
            ("translation", [x, y, z]) => transform.translation = Vec3::new(*x, *y, *z),
            ("scale", [x, y, z]) => transform.scale = Vec3::new(*x, *y, *z),
            ("rotation", [x, y, z, w]) => transform.rotation = rotation([*x, *y, *z, *w]),
            _ => {} // 不支持的权重等通道由文档诊断说明，不能误当作 TRS。
        }
    }
    local
}

/// 作者节点无需按父子顺序排列；显式栈避免深骨架递归导致后台线程栈溢出。
pub(super) fn world_matrices(nodes: &[Node], local: &[Transform]) -> Vec<Mat4> {
    let mut world = vec![Mat4::IDENTITY; nodes.len()];
    let mut state = vec![0u8; nodes.len()];
    let mut chain = Vec::new();
    for start in 0..nodes.len() {
        if state[start] == 2 {
            continue;
        }
        let mut cursor = start;
        loop {
            if state[cursor] != 0 {
                break;
            }
            state[cursor] = 1;
            chain.push(cursor);
            match nodes[cursor].parent.filter(|parent| *parent < nodes.len()) {
                Some(parent) if state[parent] == 0 => cursor = parent,
                _ => break,
            }
        }
        while let Some(index) = chain.pop() {
            let parent = nodes[index]
                .parent
                .filter(|parent| *parent < nodes.len() && state[*parent] == 2);
            world[index] =
                parent.map_or(Mat4::IDENTITY, |parent| world[parent]) * local[index].to_matrix();
            state[index] = 2; // 非法环的首个节点安全当作根，读入器仍负责向作者报告源问题。
        }
    }
    world
}

#[cfg(test)]
fn prepare_pose(document: Arc<AnimationDocument>, clip: usize, time: f32) -> Pose {
    prepare_pose_draft(document, clip, time, None)
}
fn prepare_pose_draft(
    document: Arc<AnimationDocument>,
    clip: usize,
    time: f32,
    draft: Option<(usize, Transform)>,
) -> Pose {
    let channels = document
        .clips
        .get(clip)
        .map_or(&[][..], |clip| clip.channels.as_slice());
    let mut local = sampled_local(&document.nodes, channels, time);
    if let Some((node, value)) = draft
        && let Some(local) = local.get_mut(node)
    {
        *local = value;
    }
    let matrices = world_matrices(&document.nodes, &local);
    let positions: Vec<_> = matrices
        .iter()
        .map(|matrix| matrix.transform_point3(Vec3::ZERO))
        .collect();
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for position in positions.iter().filter(|position| position.is_finite()) {
        minimum = minimum.min(*position);
        maximum = maximum.max(*position);
    }
    if !minimum.is_finite() || !maximum.is_finite() {
        minimum = Vec3::splat(-0.5);
        maximum = Vec3::splat(0.5);
    }
    let thickness = ((maximum - minimum).max_element() * 0.0075).max(0.005);
    let mut animated = vec![false; document.nodes.len()];
    for channel in channels {
        if let Some(animated) = animated.get_mut(channel.node) {
            *animated = true;
        }
    }
    let mut joints = Vec::with_capacity(positions.len());
    let mut bones = Vec::with_capacity(positions.len());
    for (index, node) in document.nodes.iter().enumerate() {
        let position = positions[index];
        joints.push(Transform::from_translation(position).with_scale(Vec3::splat(thickness * 2.4)));
        let parent = node
            .parent
            .and_then(|parent| positions.get(parent))
            .copied()
            .unwrap_or(position);
        let delta = position - parent;
        let length = delta.length();
        bones.push(if length.is_finite() && length > 1e-6 {
            Transform {
                translation: (position + parent) * 0.5,
                rotation: Quat::from_rotation_arc(Vec3::Y, delta / length),
                scale: Vec3::new(thickness, length, thickness),
            }
        } else {
            Transform::from_translation(position).with_scale(Vec3::ZERO)
        });
    }
    Pose {
        document: Arc::downgrade(&document),
        clip,
        draft,
        time,
        local,
        model_local: None,
        world: matrices,
        joints,
        bones,
        animated,
        bounds: AssetPreviewModelBounds {
            minimum: minimum - Vec3::splat(thickness * 4.0),
            maximum: maximum + Vec3::splat(thickness * 4.0),
        },
    }
}

fn retire(runtime: &mut PreviewRuntime, commands: &mut Commands) {
    if let Some(roots) = runtime.roots.take() {
        for root in roots {
            commands.entity(root).try_insert(Visibility::Hidden);
        }
        runtime.retiring = Some(Retiring {
            roots,
            entities: std::mem::take(&mut runtime.entities).into_iter(),
        });
    }
    if let Some(old) = runtime.published_pose.take() {
        runtime.retired.poses.push(old);
    }
    runtime.complete = false;
    runtime.applying = false;
    runtime.cursor = 0;
    runtime.front = 0;
    runtime.published_bounds = None;
    runtime.applied = None;
    if let Some(pose) = runtime.pose.take() {
        runtime.retired.poses.push(pose);
    }
}

fn advance_retirement(runtime: &mut PreviewRuntime, commands: &mut Commands) {
    let Some(retiring) = runtime.retiring.as_mut() else {
        return;
    };
    for _ in 0..NODES_PER_FRAME / 4 {
        let Some(entities) = retiring.entities.next() else {
            for root in retiring.roots {
                commands.entity(root).try_despawn();
            }
            runtime.retiring = None;
            return;
        };
        for entity in entities.joint.into_iter().chain(entities.bone) {
            commands.entity(entity).try_despawn();
        }
    }
}

/// 根或依赖失败时立即撤下旧画面；恢复后仍须把当前候选完整写完才能重新显示。
fn hide_failed_source(runtime: &mut PreviewRuntime, commands: &mut Commands) {
    if let Some(roots) = runtime.roots {
        for root in roots {
            commands.entity(root).try_insert(Visibility::Hidden);
        }
    }
    runtime.complete = false;
    runtime.cursor = 0;
    // 文档已换代并撤销的候选不能因源恢复而重新显示，必须等唯一worker返回有效结果。
    runtime.applying = runtime.pose.is_some() && runtime.applied.is_some();
}

/// 分帧只写不可见容器；可见容器的完整姿态在最后一次定额写入后才被替换。
fn apply_candidate(
    runtime: &mut PreviewRuntime,
    commands: &mut Commands,
    transforms: &mut Query<&mut Transform>,
    mesh_materials: &mut Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let roots = runtime.roots.expect("候选应用前已建立两个专层容器");
    let total = runtime.entities.len();
    let target = 1 - runtime.front;
    let end = runtime.cursor.saturating_add(NODES_PER_FRAME).min(total);
    let pose = runtime.pose.as_ref().expect("候选来自完整后台姿态");
    for index in runtime.cursor..end {
        let entities = runtime.entities[index];
        let material = if pose.animated[index] {
            runtime.animated_material.as_ref().unwrap()
        } else {
            runtime.joint_material.as_ref().unwrap()
        };
        for (entity, value) in [
            (entities.joint[target], pose.joints[index]),
            (entities.bone[target], pose.bones[index]),
        ] {
            if let Ok(mut transform) = transforms.get_mut(entity)
                && *transform != value
            {
                *transform = value;
            }
            if let Ok(mut applied) = mesh_materials.get_mut(entity)
                && applied.0 != *material
            {
                applied.0 = material.clone();
            }
        }
    }
    runtime.cursor = end;
    if end == total {
        commands
            .entity(roots[runtime.front])
            .try_insert(Visibility::Hidden);
        commands
            .entity(roots[target])
            .try_insert(if pose.model_local.is_some() {
                // 角色和动作的关节位置可能不同，重定向时只显示实际蒙皮结果。
                Visibility::Hidden
            } else {
                Visibility::Inherited
            });
        if let Some(old) = runtime.published_pose.replace(pose.clone()) {
            runtime.retired.poses.push(old);
        }
        runtime.front = target;
        runtime.complete = true;
        runtime.applying = false;
        if runtime.published_bounds.is_none() {
            runtime.published_bounds = Some(pose.bounds);
        }
    }
}

/// 同时最多一个姿态 worker；换源后旧任务仍保留并发信用，完成结果必须再次验证身份。
pub(super) fn drive(
    mut commands: Commands,
    editor: Res<AnimationEditorState>,
    mut preview: ResMut<AssetPreviewState>,
    mut runtime: ResMut<PreviewRuntime>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut transforms: Query<&mut Transform>,
    mut mesh_materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
    mut binding: Option<ResMut<super::binding::BindingState>>,
) {
    let binding_revision = binding.as_ref().map_or(0, |b| b.revision);
    if runtime.binding_revision != binding_revision {
        runtime.binding_revision = binding_revision;
        runtime.applied = None;
        runtime.applying = false;
        runtime.cursor = 0;
    }
    advance_retirement(&mut runtime, &mut commands);
    let eligible = editor.visible()
        && editor
            .active_document()
            .is_some_and(|document| !document.clips.is_empty());
    let wanted = eligible
        .then(|| editor.source().zip(preview.scene_root()))
        .flatten()
        .map(|(path, scene)| SourceIdentity {
            path: path.to_owned(),
            revision: editor.source_revision(),
            scene,
        });
    if runtime.identity != wanted {
        // 上一代退役尚未结束时不产生第三套骨架，连续切换只更新最新期望身份。
        if runtime.retiring.is_some() {
            return;
        }
        retire(&mut runtime, &mut commands);
        runtime.identity = wanted.clone();
    }
    if runtime.applying
        && runtime.applied.is_some_and(|(revision, clip, _)| {
            revision != editor.revision() || clip != editor.active_clip()
        })
    {
        // 关键帧、插值或剪辑在候选填充期间变化，立即撤销候选，旧完整姿态继续可见。
        runtime.applying = false;
        runtime.applied = None;
        runtime.cursor = 0;
    }
    if !runtime.applying
        && let Some(result) = runtime
            .pending
            .as_mut()
            .and_then(|pending| check_ready(&mut pending.task))
    {
        let pending = runtime.pending.take().expect("刚完成的唯一姿态任务");
        if let Some(pose) = result {
            if wanted == pending.identity
                && pending.revision == editor.revision()
                && pending.binding_revision == binding_revision
                && pending.clip == editor.active_clip()
                && (editor.playing
                    || editor.current().is_some_and(|s| s.pose.is_some())
                    || pending.time
                        == editor
                            .position()
                            .to_bits()
                            .wrapping_add(editor.pose_revision as u32))
            {
                if let Some(previous) = runtime.pose.replace(pose) {
                    runtime.retired.poses.push(previous);
                }
                runtime.applied = Some((pending.revision, pending.clip, pending.time));
                runtime.applying = true;
                runtime.cursor = 0;
            } else {
                runtime.retired.poses.push(pose);
            }
        }
    }
    let request = (
        editor.revision(),
        editor.active_clip(),
        editor
            .position()
            .to_bits()
            .wrapping_add(editor.pose_revision as u32),
    );
    if runtime.pending.is_none() {
        let snapshot = wanted
            .as_ref()
            .filter(|_| runtime.applied != Some(request))
            .and_then(|_| editor.document_snapshot());
        if snapshot.is_some() || runtime.retired.occupied() {
            // 候选分帧写入时可重叠准备下一姿态；仍只保留一个worker，且写完前不会取代当前候选。
            // 数据析构共用唯一 worker，不能为连续播放每帧 detach 一个释放任务。
            let retired = std::mem::take(&mut runtime.retired);
            let clip = editor.active_clip();
            let position = editor.position();
            let draft = editor.current().and_then(|s| s.pose);
            let retarget = binding.as_ref().filter(|binding| binding.ready())
                .and_then(|binding| binding.mapping.as_ref()?.retarget.as_ref())
                .filter(|rig| rig.supports(clip))
                .cloned();
            runtime.pending = Some(PendingPose {
                identity: wanted.clone(),
                revision: request.0,
                binding_revision,
                clip,
                time: request.2,
                task: AsyncComputeTaskPool::get().spawn(async move {
                    drop(retired);
                    snapshot.map(|document| {
                        let mut pose = prepare_pose_draft(document, clip, position, draft);
                        if let Some(rig) = retarget {
                            pose.model_local = Some(rig.sample(&pose.local));
                        }
                        Arc::new(pose)
                    })
                }),
            });
        }
    }
    if wanted.is_none() {
        return;
    }
    if runtime.retiring.is_some() {
        return;
    }
    let Some(source) = preview.model.as_ref().map(|model| model.catalog.gltf.id()) else {
        return;
    };
    let source_failed =
        assets
            .get_load_states(source)
            .is_some_and(|(direct, dependencies, recursive)| {
                matches!(direct, LoadState::Failed(_))
                    || matches!(dependencies, DependencyLoadState::Failed(_))
                    || matches!(recursive, RecursiveDependencyLoadState::Failed(_))
            });
    if source_failed {
        if let Some(binding) = binding.as_mut() {
            binding.hide(&mut commands);
        }
        hide_failed_source(&mut runtime, &mut commands);
        return;
    } // 自建骨架不能把损坏 GLTF、缺失缓冲或纹理依赖伪装成成功模型。
    if !assets.is_loaded_with_dependencies(source) {
        return;
    } // 缓存Gltf存在不代表当前加载成功；只检查真实根，不依赖合法纯动画可能没有的Scene0。
    if runtime.pose.is_none() {
        return;
    }
    if runtime.mesh.is_none() {
        runtime.mesh = Some(meshes.add(Cuboid::new(1.0, 1.0, 1.0)));
        runtime.joint_material = Some(materials.add(StandardMaterial {
            base_color: Color::srgb(0.32, 0.74, 0.88),
            unlit: true,
            ..default()
        }));
        runtime.animated_material = Some(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.65, 0.22),
            unlit: true,
            ..default()
        }));
    }
    if runtime.roots.is_none() {
        runtime.roots = Some(std::array::from_fn(|bank| {
            commands
                .spawn((
                    Name::new(format!("动画骨架预览_{bank}")),
                    MapEditorOwnedEntity,
                    Transform::default(),
                    Visibility::Hidden,
                    RenderLayers::layer(MAP_EDITOR_ASSET_PREVIEW_LAYER),
                ))
                .id()
        }));
    }
    let roots = runtime.roots.unwrap();
    let total = runtime.pose.as_ref().unwrap().joints.len();
    // 一个作者节点包含两份骨节/连杆；每帧最多创建64份骨架节点，完整前两容器都隐藏。
    for index in runtime.entities.len()..(runtime.entities.len() + NODES_PER_FRAME / 2).min(total) {
        let pose = runtime.pose.as_ref().unwrap();
        let material = if pose.animated[index] {
            runtime.animated_material.as_ref().unwrap()
        } else {
            runtime.joint_material.as_ref().unwrap()
        }
        .clone();
        let joint = std::array::from_fn(|bank| {
            commands
                .spawn((
                    Name::new("动画骨节"),
                    MapEditorOwnedEntity,
                    Mesh3d(runtime.mesh.as_ref().unwrap().clone()),
                    MeshMaterial3d(material.clone()),
                    pose.joints[index],
                    NoFrustumCulling,
                    ChildOf(roots[bank]),
                    RenderLayers::layer(MAP_EDITOR_ASSET_PREVIEW_LAYER),
                ))
                .id()
        });
        let bone = std::array::from_fn(|bank| {
            commands
                .spawn((
                    Name::new("动画骨骼连杆"),
                    MapEditorOwnedEntity,
                    Mesh3d(runtime.mesh.as_ref().unwrap().clone()),
                    MeshMaterial3d(material.clone()),
                    pose.bones[index],
                    NoFrustumCulling,
                    ChildOf(roots[bank]),
                    RenderLayers::layer(MAP_EDITOR_ASSET_PREVIEW_LAYER),
                ))
                .id()
        });
        runtime.entities.push(BoneEntities { joint, bone });
    }
    if runtime.entities.len() != total {
        return;
    }

    if runtime.applying {
        // 角色与骨架使用相同候选及预算；双方写完同一姿态后才同时切换前后容器。
        if let Some(binding) = binding.as_ref().filter(|b| b.ready()) {
            let bank = 1 - runtime.front;
            let pose = runtime.pose.as_ref().unwrap();
            let retargeting = binding.mapping.as_ref().is_some_and(|mapping| mapping.retarget.is_some());
            for index in
                runtime.cursor..(runtime.cursor + NODES_PER_FRAME).min(runtime.entities.len())
            {
                if let Some(entity) = binding.node(bank, index)
                    && let Ok(mut transform) = transforms.get_mut(entity)
                {
                    if retargeting {
                        if let Some(value) = pose.model_local.as_ref().and_then(|model| model[index]) {
                            *transform = value;
                        }
                    } else {
                        *transform = pose.local[index];
                    }
                }
            }
        }
        apply_candidate(
            &mut runtime,
            &mut commands,
            &mut transforms,
            &mut mesh_materials,
        );
        if !runtime.applying
            && let Some(binding) = binding.as_mut()
        {
            let retargeting = binding.mapping.as_ref().is_some_and(|mapping| mapping.retarget.is_some());
            if !retargeting || runtime.pose.as_ref().is_some_and(|pose| pose.model_local.is_some()) {
                binding.publish(&mut commands, runtime.front);
            }
        }
    }
    if !runtime.complete {
        return;
    }
    let bounds = runtime.published_bounds.unwrap();
    if let Some(binding) = binding.as_ref()
        && let Some(root) = preview.scene_root()
        && preview
            .model
            .as_ref()
            .is_some_and(|m| m.scene_mesh_count > 0)
    {
        commands.entity(root).try_insert(if binding.active {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
    // Scene0继续使用原完整静态预览，不对真实蒙皮分批写入半套姿态；骨架是本版明确的动作显示。
    if preview.bounds_scan.is_none() {
        let rig_bounds = binding.as_ref().filter(|binding| binding.active)
            .and_then(|binding| binding.mapping.as_ref()?.retarget.as_ref())
            .map(|rig| rig.bounds);
        let using_retarget_bounds = rig_bounds.is_some();
        if preview.model_bounds.is_none() || runtime.using_retarget_bounds != using_retarget_bounds {
            preview.model_bounds = Some(rig_bounds.unwrap_or_else(|| {
                preview.model.as_ref().and_then(|model| model.scene_bounds).unwrap_or(bounds)
            }));
            runtime.using_retarget_bounds = using_retarget_bounds;
        }
        let update_model = preview
            .model
            .as_ref()
            .is_some_and(|model| !model.scene_ready || model.scene_bounds.is_none());
        if update_model && let Some(model) = preview.model.as_mut() {
            model.scene_ready = true;
            if model.scene_bounds.is_none() {
                model.scene_bounds = Some(bounds);
            }
        }
        let real_meshes = preview
            .model
            .as_ref()
            .map_or(0, |model| model.scene_mesh_count);
        let preserve_reveal = real_meshes > 0
            && matches!(
                preview.content,
                AssetPreviewContent::Model {
                    phase: AssetPreviewPhase::RevealPending,
                    ..
                }
            );
        if !preserve_reveal {
            if matches!(preview.content, AssetPreviewContent::Model { phase, .. } if phase != AssetPreviewPhase::Ready)
                && let AssetPreviewContent::Model { phase, .. } = &mut preview.content
            {
                *phase = AssetPreviewPhase::Ready;
            }
            let status = if using_retarget_bounds {
                "已绑骨角色 · 外部 GLB 动作预览".to_owned()
            } else if binding.as_ref().is_some_and(|b| b.active) {
                format!("角色动画预览 · {total}个节点 · 点击骨节可编辑姿势")
            } else if real_meshes > 0 {
                format!("动画骨架预览 · {total}个节点 · 原模型保留静态姿态")
            } else {
                format!("动画骨架预览 · {total}个节点 · 橙色为动画通道")
            };
            if preview.status != status {
                preview.status = status;
            }
        }
    }
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
