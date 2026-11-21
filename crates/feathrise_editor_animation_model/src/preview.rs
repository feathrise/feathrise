//! 已发布姿态、当前展示容器和双缓冲身份；蒙皮问题帧在原资源上同步冻结。

use bevy::{prelude::*, tasks::Task};
use std::{path::{Path, PathBuf}, sync::{Arc, Weak}};
use super::{data::{AnimationDocument, PreviewJoint}, gizmo::GizmoCamera, media::{AssetPreviewState, AssetPreviewModelBounds},
    resources::{AnimationEditorState, BindingState, Session, WeightFrame}, types::PoseDraft};

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
    pub draft: Option<PoseDraft>,
    pub time: f32,
    pub local: Vec<Transform>,
    /// 只保存已匹配的目标关节变换，以源节点索引定位容器中的实体。
    pub model_local: Option<Vec<(usize, Transform)>>,
    pub world: Vec<Mat4>,
    pub skeleton: Arc<[PreviewJoint]>,
    pub bounds: AssetPreviewModelBounds,
}

impl Pose {
    pub fn matches_session(&self, session: &Session) -> bool {
        self.clip == session.clip
            && std::ptr::eq(self.document.as_ptr(), Arc::as_ptr(&session.document))
    }

    pub fn can_start_edit(&self, editor: &AnimationEditorState) -> bool {
        editor.current().is_some_and(|session| {
            self.matches_session(session)
                && self.draft == session.pose
                // 播放中从实际显示帧起手；暂停定位或还原姿势后必须等当前完整结果，不能跳回旧帧。
                && (editor.playing || self.time == session.position)
        })
    }

    pub fn can_draw_tools(&self, editor: &AnimationEditorState, dragging: bool) -> bool {
        editor.current().is_some_and(|session| self.matches_session(session))
            && (dragging || self.can_start_edit(editor))
    }
}

pub struct PendingPose {
    pub identity: Option<SourceIdentity>,
    pub revision: u64,
    pub binding_revision: u64,
    pub clip: usize,
    pub time: u32,
    pub view_only: bool,
    pub task: Task<Option<PreparedPose>>,
}

#[derive(Clone, Copy, PartialEq)]
pub struct SkeletonView {
    pub camera: GizmoCamera,
    pub selected: Option<usize>,
}

pub struct SkeletonFrame {
    pub view: SkeletonView,
    /// 按关节预算分批登记，每批包含普通与高亮两组线带。
    pub batches: std::collections::VecDeque<[GizmoAsset; 2]>,
}

pub struct PreparedPose {
    pub pose: Arc<Pose>,
    pub skeleton: Option<SkeletonFrame>,
}

#[derive(Default)]
pub struct RetiredData {
    pub poses: Vec<Arc<Pose>>,
    pub skeletons: Vec<SkeletonFrame>,
}

impl RetiredData {
    pub fn occupied(&self) -> bool {
        !self.poses.is_empty() || !self.skeletons.is_empty()
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
    pub candidate_skeleton: Option<SkeletonFrame>,
    pub published_skeleton_view: Option<SkeletonView>,
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
    pub fn discard_candidate_skeleton(&mut self) {
        // 换源或隐藏时，剩余未登记的线带随原单槽任务释放。
        self.retired.skeletons.extend(self.candidate_skeleton.take());
    }

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
            && binding.identity.as_ref().is_some_and(|identity|
                (identity.retarget || binding.embedded()) && identity.model == path)
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
            && (!(editor.in_place_rig_preview() || editor.embedded_motion)
                || self.in_place_hidden_scene.is_some())
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
            && self.published_pose.is_some()
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
            && preview.render_visible
            && preview.model.as_ref().is_some_and(|model|
                model.target == super::media::AssetPreviewModelTarget::Scene0)
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
