//! 动画文档的真实姿态预览：后台采样 TRS，专层骨架复用资产浏览唯一相机与 RTT。
//!
//! 纯动作的轮廓与姿态在同一后台任务准备。模型仍使用原双容器，
//! 候选模型与相同时间的骨架完成后才共同发布。

use feathrise_editor_animation_model::{assembly::AnimationSystems, media::*};
pub(super) use feathrise_editor_animation_model::preview::*;
use super::{
    data::{AnimationDocument, Channel, Node, preview_joint_topology},
    resources::{AnimationEditorState, BindingState},
    skeleton::{self, SkeletonPresentation},
    types::PoseDraft,
};
use feathrise_editor_core::SceneEditorSystemSet;
use bevy::asset::{DependencyLoadState, LoadState, RecursiveDependencyLoadState};
use bevy::{
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};
use std::sync::Arc;
#[cfg(any(test, feature = "validation"))]
use std::path::PathBuf;
#[cfg(test)]
use std::path::Path;

const NODES_PER_FRAME: usize = 64;

#[cfg(test)]
pub(super) fn for_input_tests(document: Arc<AnimationDocument>) -> PreviewRuntime {
    for_input_tests_at(document, 0, 0.0, None)
}
#[cfg(test)]
pub(super) fn for_input_tests_at(
    document: Arc<AnimationDocument>, clip: usize, time: f32, draft: Option<PoseDraft>,
) -> PreviewRuntime {
    PreviewRuntime {
        published_pose: Some(Arc::new(prepare_pose_draft(document, clip, time, draft))),
        ..default()
    }
}

pub(super) fn install(app: &mut App) {
    app.init_resource::<PreviewRuntime>().add_systems(
        Update,
        drive
            .in_set(AnimationSystems::Preview)
            .in_set(SceneEditorSystemSet::Tasks),
    );
}

#[cfg(feature = "validation")]
pub struct RigAnimationProbe {
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
pub fn rig_animation_probe(world: &World) -> Option<RigAnimationProbe> {
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
        || editor.preview_model_path() != Some(identity.model.as_str())
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
pub fn rig_animation_diagnostic(world: &World) -> Option<&str> {
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
    draft: Option<PoseDraft>,
) -> Pose {
    let channels = document
        .clips
        .get(clip)
        .map_or(&[][..], |clip| clip.channels.as_slice());
    let mut local = sampled_local(&document.nodes, channels, time);
    if let Some(draft) = &draft {
        // 所有局部覆写先一起采用，再沿父子层级计算一份一致的世界姿势。
        for (node, pose) in draft.iter() {
            if let Some(local) = local.get_mut(node) {
                *local = pose.transform;
            }
        }
    }
    let matrices = world_matrices(&document.nodes, &local);
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for matrix in &matrices {
        let position = matrix.transform_point3(Vec3::ZERO);
        if position.is_finite() {
            minimum = minimum.min(position);
            maximum = maximum.max(position);
        }
    }
    if !minimum.is_finite() || !maximum.is_finite() {
        minimum = Vec3::splat(-0.5);
        maximum = Vec3::splat(0.5);
    }
    let mut skeleton = document.skin_preview_topology();
    if skeleton.is_empty() {
        // 无蒙皮声明的动作仍可编辑，使用当前片段的节点关系。
        skeleton = preview_joint_topology(&document.nodes, channels.iter().map(|channel| channel.node)).into();
    }
    // 保留原取景留白，轮廓本身改为相机投影的固定像素尺寸。
    let padding = Vec3::splat(((maximum - minimum).max_element() * 0.03).max(0.02));
    Pose {
        document: Arc::downgrade(&document),
        clip,
        draft,
        time,
        local,
        model_local: None,
        world: matrices,
        skeleton,
        bounds: AssetPreviewModelBounds {
            minimum: minimum - padding,
            maximum: maximum + padding,
        },
    }
}

fn retire(runtime: &mut PreviewRuntime) {
    if let Some(old) = runtime.published_pose.take() {
        runtime.retired.poses.push(old);
    }
    runtime.complete = false;
    runtime.applying = false;
    runtime.cursor = 0;
    runtime.front = 0;
    runtime.published_bounds = None;
    runtime.applied = None;
    runtime.discard_candidate_skeleton();
    if let Some(pose) = runtime.pose.take() {
        runtime.retired.poses.push(pose);
    }
}

/// 根或依赖失败时立即撤下旧画面；恢复后仍须把当前候选完整写完才能重新显示。
fn hide_failed_source(runtime: &mut PreviewRuntime) {
    runtime.complete = false;
    runtime.cursor = 0;
    // 文档已换代并撤销的候选不能因源恢复而重新显示，必须等唯一worker返回有效结果。
    runtime.applying = runtime.pose.is_some() && runtime.applied.is_some();
}

fn clear_skeleton(runtime: &mut PreviewRuntime, skeleton: &mut SkeletonPresentation, commands: &mut Commands) {
    runtime.discard_candidate_skeleton();
    runtime.published_skeleton_view = None;
    skeleton.clear(commands);
}

fn restore_in_place_scene(
    runtime: &mut PreviewRuntime,
    preview: &AssetPreviewState,
    commands: &mut Commands,
) {
    if let Some(root) = runtime.in_place_hidden_scene.take()
        && preview.scene_root() == Some(root)
        && preview.model.as_ref().is_some_and(|model| model.target == AssetPreviewModelTarget::Scene0)
        && matches!(preview.content, AssetPreviewContent::Model { phase: AssetPreviewPhase::Ready, .. })
    {
        commands.entity(root).try_insert(Visibility::Inherited);
    }
}

fn publish_in_place_scene(
    runtime: &mut PreviewRuntime,
    binding: &mut BindingState,
    preview: &AssetPreviewState,
    commands: &mut Commands,
) {
    binding.publish(commands, runtime.front);
    if let Some(root) = preview.scene_root()
        && runtime.in_place_hidden_scene != Some(root)
    {
        commands.entity(root).try_insert(Visibility::Hidden);
        runtime.in_place_hidden_scene = Some(root);
    }
}

fn apply_bound_pose(
    binding: &super::binding::BindingState,
    bank: usize,
    pose: &Pose,
    cursor: usize,
    transforms: &mut Query<&mut Transform>,
) -> usize {
    let mut write_joint = |index, value| {
        if let Some(entity) = binding.node(bank, index)
            && let Ok(mut transform) = transforms.get_mut(entity)
            && *transform != value
        {
            *transform = value;
        }
    };
    let end = cursor + NODES_PER_FRAME;
    if let Some(joints) = &pose.model_local {
        // 源模型的辅助节点及目标缺少的指骨不占写入预算，源节点索引也可超出当前预算。
        let end = end.min(joints.len());
        for &(source, value) in joints.iter().take(end).skip(cursor) {
            write_joint(source, value);
        }
        return end;
    }
    let end = end.min(pose.local.len());
    if binding.mapping.as_ref().is_some_and(|mapping| mapping.retarget.is_none()) {
        for (source, &value) in pose.local.iter().enumerate().take(end).skip(cursor) {
            write_joint(source, value);
        }
    }
    end
}

fn publish_pose(runtime: &mut PreviewRuntime, bank: usize) {
    let pose = runtime.pose.as_ref().expect("候选来自完整后台姿态");
    if let Some(previous) = runtime.published_pose.replace(pose.clone()) {
        runtime.retired.poses.push(previous);
    }
    runtime.front = bank;
    runtime.complete = true;
    runtime.applying = false;
    runtime.published_bounds.get_or_insert(pose.bounds);
}

/// 模型候选仍按原预算写入隐藏容器，整副姿态完成才更新公开容器索引。
fn apply_model_candidate(
    runtime: &mut PreviewRuntime,
    binding: &BindingState,
    transforms: &mut Query<&mut Transform>,
    skeleton_ready: bool,
) -> bool {
    let bank = 1 - runtime.front;
    let pose = runtime.pose.as_ref().expect("已就绪的候选姿态");
    let total = pose.model_local.as_ref().map_or(pose.local.len(), Vec::len);
    runtime.cursor = apply_bound_pose(binding, bank, pose, runtime.cursor, transforms);
    if runtime.cursor != total || !skeleton_ready {
        return false;
    }
    publish_pose(runtime, bank);
    true
}

pub(super) fn drive(
    mut commands: Commands,
    mut editor: ResMut<AnimationEditorState>,
    mut preview: ResMut<AssetPreviewState>,
    mut runtime: ResMut<PreviewRuntime>,
    assets: Res<AssetServer>,
    mut transforms: Query<&mut Transform>,
    mut binding: Option<ResMut<BindingState>>,
    mut skeleton: SkeletonPresentation,
) {
    skeleton.maintain(&mut commands);
    let project_motion = editor.in_place_rig_preview();
    let embedded = binding.as_ref().is_some_and(|binding| binding.embedded());
    let in_place = project_motion || embedded;
    let in_place_ready = in_place && preview.render_visible
        && matches!(preview.content, AssetPreviewContent::Model { phase: AssetPreviewPhase::Ready, .. })
        && binding.as_ref().is_some_and(|binding| binding.ready() && (embedded ||
            binding.mapping.as_ref().and_then(|mapping| mapping.retarget.as_ref())
                .is_some_and(|rig| rig.supports(editor.active_clip()))));
    let embedded_motion = embedded && in_place_ready;
    if editor.embedded_motion != embedded_motion {
        editor.embedded_motion = embedded_motion;
    }
    let eligible = (!in_place || in_place_ready) && editor.visible()
        && editor.active_document().is_some_and(|document| !document.clips.is_empty());
    let view = (eligible && !editor.retargeting())
        .then(|| skeleton.view(&preview, editor.current().and_then(|session| session.selected_node)))
        .flatten();
    if view.is_none() {
        clear_skeleton(&mut runtime, &mut skeleton, &mut commands);
    }
    if in_place_ready && editor.weight_rest_active() {
        // 权重修正使用原静置网格，保留已发布的动作容器供同一帧对照。
        if let Some(binding) = binding.as_mut().filter(|binding| binding.active) {
            binding.hide(&mut commands);
        }
        restore_in_place_scene(&mut runtime, &preview, &mut commands);
        return;
    }
    if in_place && let Some(binding) = binding.as_mut().filter(|binding| binding.active)
        && (!in_place_ready || runtime.published_pose.as_ref().is_none_or(|pose|
            !editor.current().is_some_and(|session| pose.matches_session(session))))
    {
        binding.hide(&mut commands);
        clear_skeleton(&mut runtime, &mut skeleton, &mut commands);
    }
    if !in_place || binding.as_ref().is_none_or(|binding| !binding.active) {
        restore_in_place_scene(&mut runtime, &preview, &mut commands);
    }
    let binding_revision = binding.as_ref().map_or(0, |b| b.revision);
    if runtime.binding_revision != binding_revision {
        runtime.binding_revision = binding_revision;
        runtime.applied = None;
        runtime.applying = false;
        runtime.cursor = 0;
        clear_skeleton(&mut runtime, &mut skeleton, &mut commands);
    }
    let wanted = eligible.then(|| editor.source().zip(preview.scene_root())).flatten()
        .map(|(path, scene)| SourceIdentity {
            path: path.to_owned(),
            revision: editor.source_revision(),
            scene,
        });
    if runtime.identity != wanted {
        retire(&mut runtime);
        clear_skeleton(&mut runtime, &mut skeleton, &mut commands);
        runtime.identity = wanted.clone();
    }

    let request = (editor.revision(), editor.active_clip(), pose_request_time(&editor));
    // 播放时保留上一完整姿态；暂停定位只接受播放头指定结果。
    let accepts = |(revision, clip, time)| {
        revision == request.0 && clip == request.1
            && (time == request.2 || editor.playing || editor.current().is_some_and(|s| s.pose.is_some()))
    };
    if runtime.applying && runtime.applied.is_none_or(|candidate| !accepts(candidate)) {
        runtime.applying = false;
        runtime.applied = None;
        runtime.cursor = 0;
        runtime.discard_candidate_skeleton();
        skeleton.discard_candidate();
    }
    if !runtime.applying
        && let Some(result) = runtime.pending.as_mut().and_then(|pending| check_ready(&mut pending.task))
    {
        let pending = runtime.pending.take().expect("刚完成的唯一姿态任务");
        if let Some(mut prepared) = result {
            if wanted == pending.identity && pending.binding_revision == binding_revision
                && accepts((pending.revision, pending.clip, pending.time))
            {
                if pending.view_only {
                    if runtime.complete
                        && runtime.published_pose.as_ref().is_some_and(|pose| Arc::ptr_eq(pose, &prepared.pose))
                        && let Some(frame) = prepared.skeleton.as_ref()
                        && view.is_some_and(|view| view.selected == frame.view.selected)
                    {
                        skeleton.discard_candidate();
                        runtime.discard_candidate_skeleton();
                        runtime.candidate_skeleton = prepared.skeleton.take();
                    }
                } else {
                    if let Some(previous) = runtime.pose.replace(prepared.pose) {
                        runtime.retired.poses.push(previous);
                    }
                    skeleton.discard_candidate();
                    runtime.discard_candidate_skeleton();
                    runtime.candidate_skeleton = prepared.skeleton.take();
                    runtime.applied = Some((pending.revision, pending.clip, pending.time));
                    runtime.applying = true;
                    runtime.cursor = 0;
                }
            } else {
                runtime.retired.poses.push(prepared.pose);
            }
            runtime.retired.skeletons.extend(prepared.skeleton);
        }
    }
    if runtime.pending.is_none() {
        let snapshot = wanted.as_ref()
            .filter(|_| runtime.applied != Some(request))
            .and_then(|_| editor.document_snapshot());
        let published = (snapshot.is_none() && !runtime.applying && runtime.complete
            && runtime.candidate_skeleton.is_none()
            && view.is_some_and(|view| skeleton.needs_view(view)))
            .then(|| runtime.published_pose.clone()).flatten();
        if snapshot.is_some() || published.is_some() || runtime.retired.occupied() {
            // 相机变化只重投已发布姿态。新的播放帧与骨架复用同一工作槽和输入快照。
            let retired = std::mem::take(&mut runtime.retired);
            let clip = editor.active_clip();
            let position = editor.position();
            let draft = editor.current().and_then(|s| s.pose.clone());
            let retarget = binding.as_ref().filter(|binding| binding.ready())
                .and_then(|binding| binding.mapping.as_ref()?.retarget.as_ref())
                .filter(|rig| rig.supports(clip)).cloned();
            runtime.pending = Some(PendingPose {
                identity: wanted.clone(),
                revision: request.0,
                binding_revision,
                clip: request.1,
                time: request.2,
                view_only: published.is_some(),
                task: AsyncComputeTaskPool::get().spawn(async move {
                    drop(retired);
                    let pose = published.or_else(|| snapshot.map(|document| {
                        let mut pose = prepare_pose_draft(document, clip, position, draft);
                        if let Some(rig) = retarget {
                            pose.model_local = Some(rig.sample(&pose.local));
                        }
                        Arc::new(pose)
                    }))?;
                    let skeleton = view.map(|view| skeleton::build(&pose, view));
                    Some(PreparedPose { pose, skeleton })
                }),
            });
        }
    }
    if wanted.is_none() {
        return;
    }
    let Some(source) = preview.model.as_ref().map(|model| model.catalog.gltf.id()) else {
        return;
    };
    // 失败的依赖必须撤下已发布的骨架和模型。缓存中有旧 GLTF 也不能冒充就绪。
    let failed = matches!(assets.load_state(source), LoadState::Failed(_))
        || matches!(assets.dependency_load_state(source), DependencyLoadState::Failed(_))
        || matches!(assets.recursive_dependency_load_state(source), RecursiveDependencyLoadState::Failed(_));
    if failed {
        if let Some(binding) = binding.as_mut() {
            binding.hide(&mut commands);
        }
        clear_skeleton(&mut runtime, &mut skeleton, &mut commands);
        hide_failed_source(&mut runtime);
        restore_in_place_scene(&mut runtime, &preview, &mut commands);
        return;
    }
    if !assets.is_loaded_with_dependencies(source) || runtime.pose.is_none() {
        return;
    }

    if in_place
        && let Some(binding) = binding.as_mut().filter(|b| b.ready() && !b.active)
        && !runtime.applying && runtime.applied == Some(request)
        && runtime.published_pose.as_ref().is_some_and(|pose|
            editor.current().is_some_and(|session| pose.matches_session(session)))
    {
        // 从静置返回时立即采用已经发布的原动作容器。
        publish_in_place_scene(&mut runtime, binding, &preview, &mut commands);
    }
    if runtime.candidate_skeleton.as_ref().is_some_and(|frame|
        !view.is_some_and(|view| view.selected == frame.view.selected))
    {
        runtime.discard_candidate_skeleton();
        skeleton.discard_candidate();
    }
    let skeleton_ready = runtime.candidate_skeleton.as_mut()
        .is_none_or(|frame| skeleton.stage(&mut commands, frame));
    let publishes_pose = runtime.applying;
    if publishes_pose {
        let model = binding.as_mut().filter(|b| b.ready() && (b.mapping.as_ref()
            .is_some_and(|mapping| mapping.retarget.is_none())
            || runtime.pose.as_ref().is_some_and(|pose| pose.model_local.is_some())));
        if let Some(binding) = model {
            if !apply_model_candidate(&mut runtime, binding, &mut transforms, skeleton_ready) {
                return;
            }
            if in_place {
                publish_in_place_scene(&mut runtime, binding, &preview, &mut commands);
            } else {
                binding.publish(&mut commands, runtime.front);
            }
        } else if !in_place && skeleton_ready {
            if let Some(binding) = binding.as_mut().filter(|binding| binding.active) {
                binding.hide(&mut commands);
            }
            let bank = 1 - runtime.front;
            publish_pose(&mut runtime, bank);
        } else {
            return;
        }
    } else if !skeleton_ready {
        return;
    }
    if let Some(frame) = runtime.candidate_skeleton.take() {
        skeleton.publish(&mut commands, frame.view);
        runtime.published_skeleton_view = Some(frame.view);
    } else if publishes_pose {
        clear_skeleton(&mut runtime, &mut skeleton, &mut commands);
    }
    if in_place || !runtime.complete {
        return;
    }
    let bounds = runtime.published_bounds.unwrap();
    if let Some(binding) = binding.as_ref()
        && let Some(root) = preview.scene_root()
        && preview.model.as_ref().is_some_and(|model| model.scene_mesh_count > 0
            && model.target == AssetPreviewModelTarget::Scene0)
    {
        commands.entity(root).try_insert(if binding.active {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
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
        if preview.model.as_ref().is_some_and(|model| !model.scene_ready || model.scene_bounds.is_none())
            && let Some(model) = preview.model.as_mut()
        {
            model.scene_ready = true;
            model.scene_bounds.get_or_insert(bounds);
        }
        let real_meshes = preview.model.as_ref().map_or(0, |model| model.scene_mesh_count);
        let preserve_reveal = real_meshes > 0 && matches!(preview.content, AssetPreviewContent::Model {
            phase: AssetPreviewPhase::RevealPending, ..
        });
        if !preserve_reveal {
            if matches!(preview.content, AssetPreviewContent::Model { phase, .. } if phase != AssetPreviewPhase::Ready)
                && let AssetPreviewContent::Model { phase, .. } = &mut preview.content
            {
                *phase = AssetPreviewPhase::Ready;
            }
            let status = if using_retarget_bounds {
                "已绑骨角色 · 外部 GLB 动作预览"
            } else if binding.as_ref().is_some_and(|binding| binding.active) {
                "角色动画预览"
            } else {
                "动画预览 · 点击关节或骨段选骨"
            };
            if preview.status != status {
                preview.status = status.into();
            }
        }
    }
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
