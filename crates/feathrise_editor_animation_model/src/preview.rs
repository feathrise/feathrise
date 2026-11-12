//! 已发布姿态、当前展示容器和双缓冲身份；蒙皮问题帧在原资源上同步冻结。

use bevy::{prelude::*, tasks::Task};
use std::{path::{Path, PathBuf}, sync::{Arc, Weak}};
use super::{data::AnimationDocument, media::{AssetPreviewState, AssetPreviewModelBounds},
    resources::{AnimationEditorState, BindingState, Session, WeightFrame}};

#[derive(Clone, PartialEq, Eq)]
pub struct SourceIdentity {
    pub path: PathBuf,
    pub revision: u64,
    pub scene: Entity,
}

pub struct Pose {
    /// 仅保留精确文档的弱身份；旧完整画面可以继续显示，但不能按新文档或动作开始编辑。
    pub document: Weak<AnimationDocument>,
    pub clip: usize,
    pub draft: Option<(usize, Transform)>,
    pub time: f32,
    pub local: Vec<Transform>,
    pub model_local: Option<Vec<Option<Transform>>>,
    pub world: Vec<Mat4>,
    pub joints: Vec<Transform>,
    pub bones: Vec<Transform>,
    pub animated: Vec<bool>,
    pub bounds: AssetPreviewModelBounds,
}

impl Pose {
    pub fn matches_session(&self, session: &Session) -> bool {
        self.clip == session.clip
            && std::ptr::eq(self.document.as_ptr(), Arc::as_ptr(&session.document))
    }

    pub fn can_start_edit(&self, editor: &AnimationEditorState) -> bool {
        !editor.retargeting() && editor.current().is_some_and(|session| {
            self.matches_session(session)
                && self.draft == session.pose
                // 播放中从实际显示帧起手；暂停定位或还原姿势后必须等当前完整结果，不能跳回旧帧。
                && (editor.playing || self.time == session.position)
        })
    }

    pub fn can_draw_tools(&self, editor: &AnimationEditorState, dragging: bool) -> bool {
        !editor.retargeting() && editor.current().is_some_and(|session| self.matches_session(session))
            && (dragging || self.can_start_edit(editor))
    }
}

pub struct PendingPose {
    pub identity: Option<SourceIdentity>,
    pub revision: u64,
    pub binding_revision: u64,
    pub clip: usize,
    pub time: u32,
    pub task: Task<Option<Arc<Pose>>>,
}

#[derive(Clone, Copy)]
pub struct BoneEntities {
    pub joint: [Entity; 2],
    pub bone: [Entity; 2],
}

pub struct Retiring {
    pub roots: [Entity; 2],
    pub entities: std::vec::IntoIter<BoneEntities>,
}

#[derive(Default)]
pub struct RetiredData {
    pub poses: Vec<Arc<Pose>>,
}

impl RetiredData {
    pub fn occupied(&self) -> bool {
        !self.poses.is_empty()
    }
}

#[derive(Resource, Default)]
pub struct PreviewRuntime {
    pub binding_revision: u64,
    pub identity: Option<SourceIdentity>,
    pub pending: Option<PendingPose>,
    pub pose: Option<Arc<Pose>>,
    pub published_pose: Option<Arc<Pose>>,
    pub applied: Option<(u64, usize, u32)>,
    pub roots: Option<[Entity; 2]>,
    pub entities: Vec<BoneEntities>,
    pub retiring: Option<Retiring>,
    pub mesh: Option<Handle<Mesh>>,
    pub joint_material: Option<Handle<StandardMaterial>>,
    pub animated_material: Option<Handle<StandardMaterial>>,
    pub front: usize,
    pub applying: bool,
    pub cursor: usize,
    pub published_bounds: Option<AssetPreviewModelBounds>,
    pub using_retarget_bounds: bool,
    /// 只恢复本系统确实隐藏过的中央 Scene0，单体预览仍由原目标系统控制。
    pub in_place_hidden_scene: Option<Entity>,
    pub complete: bool,
    pub retired: RetiredData,
}

impl PreviewRuntime {
    /// 框选从 GPU 已显示的容器起手，冻结该容器并废弃还在准备的下一姿态。
    pub fn freeze_weight_frame(
        &mut self,
        editor: &mut AnimationEditorState,
        binding: &BindingState,
        path: &str,
    ) -> Option<(WeightFrame, Entity)> {
        let time = self.published_weight_time(editor)?;
        if !editor.playing && editor.position() != time {
            return None;
        }
        let frame = editor.weight_frame(time)?;
        let root = self.weight_frame_root(binding, path)?;
        editor.pause_weight_comparison(Some(time));
        self.applying = false;
        self.applied = Some((editor.revision(), editor.active_clip(), pose_request_time(editor)));
        Some((frame, root))
    }

    pub fn weight_frame_root(
        &self, binding: &BindingState, path: &str,
    ) -> Option<Entity> {
        if self.in_place_hidden_scene.is_some()
            && binding.active
            && binding.ready()
            && self.binding_revision == binding.revision
            && binding.identity.as_ref().is_some_and(|identity| identity.retarget && identity.model == path)
        {
            binding.banks[self.front].root
        } else {
            None
        }
    }

    /// 权重对照冻结已显示的姿态。时间轴的请求帧可能还在后台采样或填充另一套容器。
    pub fn published_weight_time(
        &self,
        editor: &AnimationEditorState,
    ) -> Option<f32> {
        let pose = self.published_pose.as_ref()?;
        let session = editor.current()?;
        (editor.visible()
            && (!editor.in_place_rig_preview() || self.in_place_hidden_scene.is_some())
            && pose.matches_session(session)
            && pose.draft == session.pose)
            .then_some(pose.time)
    }

    pub fn rig_pose_published(&self, editor: &AnimationEditorState) -> bool {
        self.in_place_hidden_scene.is_some()
            && self.published_pose.as_ref().is_some_and(|pose|
                editor.current().is_some_and(|session| pose.matches_session(session)))
    }

    fn skeleton_ready(&self, source: &Path, scene: Entity) -> bool {
        self.complete
            && !self.entities.is_empty()
            && self
                .identity
                .as_ref()
                .is_some_and(|identity| identity.path == source && identity.scene == scene)
    }

    pub fn document_ready(
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

pub fn pose_request_time(editor: &AnimationEditorState) -> u32 {
    editor.position().to_bits().wrapping_add(editor.pose_revision as u32)
}
