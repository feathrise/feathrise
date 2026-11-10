//! 动画草稿、轻量关键帧历史和单任务背压；完整文档的复制与修改均在后台完成。

#[cfg(test)]
use feathrise_editor_ui::UiInteraction;
use super::{
    components::{CHANNEL_SLOTS, KEY_SLOTS},
    data::*,
    types::*,
};
use bevy::{prelude::*, tasks::Task, world_serialization::WorldAsset};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Resource, Default)]
pub(super) struct PoseInputState {
    pub gesture: Option<PoseGesture>,
    pub pick: Option<(PathBuf, u64, Task<Option<usize>>)>,
}

/// 历史只保存被编辑的关键帧，避免每次修改都保留整个动作文件。
#[derive(Clone)]
pub(super) enum Edit {
    /// 批量编辑在后台构造，每条受影响曲线保存修改前后值；主线程仅移动共享所有权。
    Channels {
        clip: usize,
        values: Arc<Vec<(usize, Vec<Keyframe>, Vec<Keyframe>)>>,
    },
    Key {
        clip: usize,
        channel: usize,
        before: Option<Keyframe>,
        after: Option<Keyframe>,
    },
    Interpolation {
        clip: usize,
        channel: usize,
        before: String,
        after: String,
    },
}

impl Edit {
    pub(super) fn target(&self) -> (usize, usize) {
        match self {
            Self::Channels { clip, values } => (*clip, values.first().map_or(0, |v| v.0)),
            Self::Key { clip, channel, .. } | Self::Interpolation { clip, channel, .. } => {
                (*clip, *channel)
            }
        }
    }
    pub(super) fn apply(
        &self,
        doc: &mut AnimationDocument,
        forward: bool,
    ) -> Result<Option<usize>, String> {
        match self {
            Self::Channels { clip, values } => {
                for (index, before, after) in values.iter() {
                    let channel = doc
                        .clips
                        .get_mut(*clip)
                        .and_then(|c| c.channels.get_mut(*index))
                        .ok_or("通道已改变")?;
                    channel.keys = if forward {
                        after.clone()
                    } else {
                        before.clone()
                    };
                    channel.validate()?;
                }
                Ok(None)
            }
            Self::Key {
                clip,
                channel,
                before,
                after,
            } => {
                let channel = doc
                    .clips
                    .get_mut(*clip)
                    .and_then(|clip| clip.channels.get_mut(*channel))
                    .ok_or("动画通道已经改变，请重新选择")?;
                let (remove, insert) = if forward {
                    (before, after)
                } else {
                    (after, before)
                };
                let selected = if let Some(key) = insert {
                    if !key.time.is_finite()
                        || key.time < 0.0
                        || key.value.iter().any(|v| !v.is_finite())
                    {
                        return Err("关键帧时间和数值必须有限，时间不能为负".to_owned());
                    }
                    let mut key = key.clone();
                    if forward && channel.path == "rotation" {
                        let length = key.value.iter().map(|v| v * v).sum::<f32>().sqrt();
                        if !length.is_finite() || length < 1e-6 {
                            return Err("旋转四元数长度必须有效且不能全部为零".to_owned());
                        }
                        for value in &mut key.value {
                            *value /= length;
                        }
                    }
                    let index = if let Some(previous) = remove {
                        let index = channel
                            .keys
                            .binary_search_by(|item| item.time.total_cmp(&previous.time))
                            .map_err(|_| "关键帧已经改变，请重新选择")?;
                        let index = channel.move_key(index, key.time)?;
                        channel.keys[index] = key;
                        index
                    } else {
                        // 三次曲线的中途插键使用原曲线导数，避免仅插入一帧就改变整段轨迹。
                        if channel
                            .keys
                            .binary_search_by(|item| item.time.total_cmp(&key.time))
                            .is_ok()
                        {
                            return Err("目标时间已有关键帧，请编辑该帧或选择其他时间".to_owned());
                        }
                        let index = channel.insert_key(key.time, key.value.clone())?;
                        // 恢复历史必须使用原始完整切线；只有新插键的空切线标记允许重新求导。
                        if channel.interpolation != "CUBICSPLINE"
                            || (key.in_tangent.len() == key.value.len()
                                && key.out_tangent.len() == key.value.len())
                        {
                            channel.keys[index] = key;
                        }
                        index
                    };
                    Some(index)
                } else {
                    if let Some(previous) = remove {
                        let index = channel
                            .keys
                            .binary_search_by(|item| item.time.total_cmp(&previous.time))
                            .map_err(|_| "关键帧已经改变，请重新选择")?;
                        channel.delete_key(index)?;
                    }
                    None
                };
                channel.validate()?;
                Ok(selected)
            }
            Self::Interpolation {
                clip,
                channel,
                before,
                after,
            } => {
                let channel = doc
                    .clips
                    .get_mut(*clip)
                    .and_then(|clip| clip.channels.get_mut(*channel))
                    .ok_or("动画通道已经改变")?;
                channel.set_interpolation(if forward { after } else { before })?;
                channel.validate()?;
                Ok(None)
            }
        }
    }
}

pub(super) struct Session {
    pub document: Arc<AnimationDocument>,
    pub metadata: Metadata,
    pub clip: usize,
    pub channel: Option<usize>,
    pub key: Option<usize>,
    pub position: f32,
    pub page: usize,
    pub history: Vec<Edit>,
    pub cursor: usize,
    pub saved_cursor: Option<usize>,
    pub source_changed: bool,
    pub last_used: u64,
    pub window_start: f32,
    pub window_span: f32,
    pub selected_node: Option<usize>,
    pub bone_filter: bool,
    pub selected: KeySelection,
    pub pose: Option<(usize, Transform)>,
    pub model_path: Option<String>,
    pub retarget_model: bool,
}

impl Session {
    pub fn visible_channels(&self) -> Vec<usize> {
        if self.bone_filter {
            self.selected_node
                .and_then(|node| self.metadata.channels_by_node.get(self.clip)?.get(&node))
                .map(|indices| {
                    indices
                        .iter()
                        .copied()
                        .skip(self.page * CHANNEL_SLOTS)
                        .take(CHANNEL_SLOTS)
                        .collect()
                })
                .unwrap_or_default()
        } else {
            (self.page * CHANNEL_SLOTS..self.document.clips[self.clip].channels.len())
                .take(CHANNEL_SLOTS)
                .collect()
        }
    }
    pub fn select_node(&mut self, node: usize) {
        if self.pose.is_some() {
            return;
        }
        self.selected_node = Some(node);
        self.channel = self
            .metadata
            .channels_by_node
            .get(self.clip)
            .and_then(|map| map.get(&node))
            .and_then(|v| {
                v.iter()
                    .copied()
                    .find(|i| self.document.clips[self.clip].channels[*i].path == "rotation")
                    .or_else(|| v.first().copied())
            });
        self.key = None;
        self.page = 0;
    }
    pub(super) fn select_clip(&mut self, clip: usize) {
        self.clip = clip;
        self.channel = (!self.document.clips[clip].channels.is_empty()).then_some(0);
        self.key = None;
        self.position = 0.0;
        self.page = 0;
        self.selected = KeySelection::default();
        self.window_start = 0.0;
        self.window_span = self.metadata.durations[clip].max(1.0 / 30.0);
        self.selected_node = self.metadata.bones[clip].first().copied();
    }
    pub(super) fn new(document: AnimationDocument) -> Self {
        let channel = document
            .clips
            .first()
            .filter(|clip| !clip.channels.is_empty())
            .map(|_| 0);
        let metadata = Metadata::new(&document);
        let window_span = metadata
            .durations
            .first()
            .copied()
            .unwrap_or(1.0)
            .max(1.0 / 30.0);
        let selected_node = metadata
            .bones
            .first()
            .and_then(|bones| bones.first())
            .copied();
        let channel = selected_node
            .and_then(|node| {
                metadata
                    .channels_by_node
                    .first()?
                    .get(&node)?
                    .first()
                    .copied()
            })
            .or(channel);
        Self {
            document: Arc::new(document),
            metadata,
            clip: 0,
            channel,
            key: None,
            position: 0.0,
            page: 0,
            history: Vec::new(),
            cursor: 0,
            saved_cursor: Some(0),
            source_changed: false,
            last_used: 0,
            window_start: 0.0,
            window_span,
            selected_node,
            bone_filter: true,
            selected: KeySelection::default(),
            pose: None,
            model_path: None,
            retarget_model: false,
        }
    }
    pub(super) fn dirty(&self) -> bool {
        self.pose.is_some() || self.saved_cursor != Some(self.cursor)
    }
}

pub(super) struct Metadata {
    pub durations: Vec<f32>,
    pub bones: Vec<Vec<usize>>,
    pub channels_by_node: Vec<HashMap<usize, Vec<usize>>>,
}
impl Metadata {
    pub(super) fn new(document: &AnimationDocument) -> Self {
        Self {
            bones: document
                .clips
                .iter()
                .map(|clip| {
                    let mut nodes: Vec<_> = clip.channels.iter().map(|c| c.node).collect();
                    nodes.sort_by(|a, b| document.nodes[*a].name.cmp(&document.nodes[*b].name));
                    nodes.dedup();
                    nodes
                })
                .collect(),
            channels_by_node: document
                .clips
                .iter()
                .map(|clip| {
                    let mut map: HashMap<usize, Vec<usize>> = HashMap::new();
                    for (i, c) in clip.channels.iter().enumerate() {
                        map.entry(c.node).or_default().push(i);
                    }
                    map
                })
                .collect(),
            durations: document.clips.iter().map(Clip::duration).collect(),
        }
    }
}

pub(super) enum PendingAction {
    Command(super::editing::Command),
    Edit(Edit),
    Undo,
    Redo,
    Save,
    Reload,
}
pub(super) enum TaskResult {
    Copied(Result<Arc<super::editing::Clipboard>, String>),
    Load(Result<Session, String>),
    Unchanged,
    Released,
    Edit {
        document: Result<(AnimationDocument, Metadata), String>,
        action: PendingAction,
        key: Option<usize>,
        target: (usize, usize),
        selection: Option<KeySelection>,
    },
    Save(Result<AnimationDocument, String>),
}
pub(super) struct Work {
    pub path: PathBuf,
    /// 同路径热更新也会换代；旧校验/读取不得清掉请求启动后的新变更标记。
    pub generation: Option<u64>,
    pub saving: bool, // 只读试玩门必须区分GLB写入与同槽的读取/编辑任务，不猜诊断文案。
    pub task: Task<TaskResult>,
}

/// 单份待释放所有权；不在采用帧析构旧曲线、旧历史或迟到读取的大文件二进制。
pub(super) enum Retired {
    Clipboard(Arc<super::editing::Clipboard>),
    Session(Session),
    Document {
        document: Arc<AnimationDocument>,
        metadata: Option<Metadata>,
        history: Vec<Edit>,
    },
    Result(TaskResult),
    #[cfg(test)]
    ReleaseProbe(Box<dyn FnOnce() + Send + Sync>),
}

impl Retired {
    /// 只由唯一计算任务调用；显式消费各类所有权，最后一个 Arc 也在后台归还。
    pub(super) fn release(self) {
        match self {
            Self::Clipboard(value) => drop(value),
            Self::Session(session) => drop(session),
            Self::Document {
                document,
                metadata,
                history,
            } => {
                drop(document);
                drop(metadata);
                drop(history);
            }
            Self::Result(result) => drop(result),
            #[cfg(test)]
            Self::ReleaseProbe(probe) => probe(),
        }
    }
}

#[derive(Resource)]
pub(crate) struct AnimationEditorState {
    /// 原保存结果按源路径冻结，工程收尾不能借当前可见状态栏推断隐藏源的成功。
    pub(super) project_save_receipt: Option<(PathBuf, Result<(), String>)>,
    pub(super) active: Option<PathBuf>,
    pub(super) sessions: HashMap<PathBuf, Session>,
    pub(super) work: Option<Work>,
    pub(super) retired: Option<Retired>,
    pub(super) pending: Option<PendingAction>,
    pub(super) visible: bool,
    pub(super) playing: bool,
    pub(super) looping: bool,
    pub(super) fps: u32,
    pub(super) speed: f32,
    pub(super) revision: u64,
    pub(super) source_revision: u64,
    pub(super) status: String,
    pub(super) failed: Option<PathBuf>,
    pub(super) mouse_owned: bool,
    pub(super) wheel_owned: bool,
    pub(super) drag: Option<(Entity, PathBuf, u64)>,
    pub(super) model_generation: Option<(PathBuf, u64)>,
    pub(super) gesture: Option<TimelineGesture>,
    pub(super) clipboard: Option<Arc<super::editing::Clipboard>>,
    pub(super) auto_key: bool,
    pub(super) pose_tool: PoseTool,
    pub(super) pose_mouse_owned: bool,
    pub(super) pose_revision: u64,
    pub(super) model_status: String,
    pub(super) model_binding_revision: u64,
    pub(super) rig_preview: Option<super::request::RigPreviewRequest>,
}

impl Default for AnimationEditorState {
    fn default() -> Self {
        Self {
            project_save_receipt: None,
            active: None,
            sessions: HashMap::new(),
            work: None,
            retired: None,
            pending: None,
            visible: false,
            playing: false,
            looping: true,
            fps: 30,
            speed: 1.0,
            revision: 1,
            source_revision: 1,
            status: String::new(),
            failed: None,
            mouse_owned: false,
            wheel_owned: false,
            drag: None,
            model_generation: None,
            gesture: None,
            clipboard: None,
            auto_key: true,
            pose_tool: PoseTool::Move,
            pose_mouse_owned: false,
            pose_revision: 0,
            model_status: String::new(),
            model_binding_revision: 0,
            rig_preview: None,
        }
    }
}

#[cfg(test)]
impl AnimationEditorState {
    /// 为跨工具输入回归准备真实轨道与正式输入系统，不直接伪造鼠标所有权或公共门。
    pub(crate) fn install_timeline_input_for_tests(app: &mut App) -> Entity {
        use super::super::{
            AssetPreviewWorkspace, DocumentState,
            types::{AssetPreviewContent, AssetPreviewPhase, AssetPreviewState},
        };
        use crate::{
            resources::MapEditorState,
            ui::sidebars::MapEditorSidebarState,
        };

        app.init_resource::<DocumentState>()
            .init_resource::<AssetPreviewWorkspace>()
            .init_resource::<MapEditorSidebarState>()
            .add_message::<bevy::input::mouse::MouseWheel>()
            .add_message::<feathrise_editor_ui::NumericInputCommitted>();
        let path = "animation-mesh-input.glb";
        let source = app.world().resource::<DocumentState>().asset_root().join(path);
        app.world_mut().resource_mut::<MapEditorState>().selected_asset = Some(path.into());
        app.world_mut().resource_mut::<MapEditorSidebarState>().reveal_asset_properties();
        let mut state = Self::default();
        state.active = Some(source.clone());
        state.visible = true;
        state.sessions.insert(source, Session::new(super::data::tests::test_document()));
        let binding = state.revision;
        app.insert_resource(state)
            .insert_resource(AssetPreviewState {
                selected_path: Some(path.into()),
                content: AssetPreviewContent::Model {
                    handle: Handle::default(), root: Entity::PLACEHOLDER, phase: AssetPreviewPhase::Ready,
                },
                ..default()
            })
            .add_systems(Update, (super::control::input, super::timeline::input).chain().before(crate::authoring::mesh_edit::MeshEditInput));
        app.world_mut().spawn((
            super::components::AnimationTrack { binding, row: None },
            ComputedNode { size: Vec2::new(200.0, 20.0), ..default() },
            bevy::ui::RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::ZERO) },
            UiInteraction::Hovered,
        )).id()
    }
}

impl AnimationEditorState {
    /// 动画源GLB写入仍占原工作槽；未保存姿态、读取和CPU编辑不阻止按磁盘版本试玩。
    pub(crate) fn saved_play_pending(&self) -> bool {
        matches!(self.pending, Some(PendingAction::Save)) || self.work.as_ref().is_some_and(|work| work.saving)
    }

    /// 动画源会话可在切换资产后继续留存；只读全部会话与原任务槽。
    pub(crate) fn exit_blocker(&self) -> Option<&'static str> {
        if self.busy() || self.drag.is_some() || self.gesture.is_some() || self.pose_mouse_owned {
            return Some("动画保存、编辑或手势尚未完成");
        }
        self.sessions.values().any(|session| session.dirty())
            .then_some("存在未保存的动画草稿或姿态，请先使用原动画保存或取消操作收口")
    }

    pub(super) fn current(&self) -> Option<&Session> {
        self.sessions.get(self.active.as_ref()?)
    }
    pub(super) fn current_mut(&mut self) -> Option<&mut Session> {
        self.sessions.get_mut(self.active.as_ref()?)
    }
    pub(super) fn active_document(&self) -> Option<&AnimationDocument> {
        self.current().map(|s| s.document.as_ref())
    }
    pub(super) fn document_snapshot(&self) -> Option<Arc<AnimationDocument>> {
        self.current().map(|s| s.document.clone())
    }
    pub(super) fn active_clip(&self) -> usize {
        self.current().map_or(0, |s| s.clip)
    }
    pub(super) fn position(&self) -> f32 {
        self.current().map_or(0.0, |s| s.position)
    }
    pub(super) fn revision(&self) -> u64 {
        self.revision
    }
    pub(super) fn source_revision(&self) -> u64 {
        self.source_revision
    }
    pub(super) fn source(&self) -> Option<&Path> {
        self.active.as_deref()
    }
    pub(super) fn visible(&self) -> bool {
        self.visible
            && self
                .current()
                .is_some_and(|session| !session.document.clips.is_empty())
    }
    pub(super) fn retargeting(&self) -> bool {
        self.current().is_some_and(|session| session.retarget_model && session.model_path.is_some())
    }
    pub(super) fn editable(&self) -> bool {
        !self.retargeting() && self.active_document().is_some_and(|doc| !doc.read_only)
    }
    pub(crate) fn owns_mouse(&self) -> bool {
        self.mouse_owned || self.drag.is_some() || self.gesture.is_some() || self.pose_mouse_owned
    }
    pub(crate) fn owns_wheel(&self) -> bool {
        self.visible && (self.wheel_owned || self.owns_mouse())
    }
    pub(crate) fn has_unsaved_draft(&self, root: &Path, path: &str) -> bool {
        self.sessions
            .get(&root.join(path))
            .is_some_and(Session::dirty)
    }
    pub(super) fn busy(&self) -> bool {
        self.work.is_some() || self.retired.is_some() || self.pending.is_some()
    }
    pub(super) fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1).max(1);
    }
    pub(super) fn seek(&mut self, position: f32) {
        if !position.is_finite() {
            return;
        }
        if let Some(session) = self.current_mut() {
            if session.pose.is_some() {
                return;
            } // 未插帧姿势必须显式插入或还原，避免时间切换静默丢稿。
            let duration = session
                .metadata
                .durations
                .get(session.clip)
                .copied()
                .unwrap_or(0.0);
            session.position = position.clamp(
                0.0,
                duration.max(session.window_start + session.window_span),
            );
        }
    }
    /// 固定8行、每行64个显示槽；二分定位时间窗口，密集曲线按桶显示，缩放后所有键均可访问。
    pub(super) fn view(&self) -> AnimationView {
        let mut view = AnimationView {
            visible: self.visible,
            binding: self.revision,
            status: self.status.clone(),
            playing: self.playing,
            looping: self.looping,
            fps: self.fps,
            speed: self.speed,
            busy: self.busy(),
            auto_key: self.auto_key,
            pose_tool: self.pose_tool,
            model_status: self.model_status.clone(),
            ..Default::default()
        };
        let Some(session) = self.current() else {
            return view;
        };
        view.visible &= !session.document.clips.is_empty();
        view.editable = self.editable();
        if session.source_changed {
            view.status = "源文件已更新；当前草稿保留，保存或撤销修改后可重新载入".to_owned();
        }
        if let Some(warning) = session.document.warnings.first() {
            view.status = format!(
                "{} · {}条通道提示：{warning}",
                view.status,
                session.document.warnings.len()
            );
        }
        view.dirty = session.dirty();
        view.can_undo = session.cursor > 0;
        view.can_redo = session.cursor < session.history.len();
        view.clip_index = session.clip;
        view.clip_count = session.document.clips.len();
        view.page = session.page;
        view.position = session.position;
        view.selected_channel = session.channel;
        view.selected_key = session.key;
        view.start = session.window_start;
        view.span = session.window_span;
        view.selected_bone_only = session.bone_filter;
        view.bone_name = session
            .selected_node
            .and_then(|i| session.document.nodes.get(i))
            .map_or(String::new(), |n| n.name.clone());
        view.pose_dirty = session.pose.is_some();
        view.model_path = session.model_path.clone();
        view.selected = session.selected.clone();
        if let Some(gesture) = &self.gesture {
            match &gesture.kind {
                TimelineGestureKind::Move { delta, .. } => view.move_delta = *delta,
                TimelineGestureKind::Box {
                    end_row, end_time, ..
                } => view.box_selection = Some((gesture.row, *end_row, gesture.time, *end_time)),
                _ => {}
            }
        }
        let Some(clip) = session.document.clips.get(session.clip) else {
            return view;
        };
        view.clip_name = clip.name.clone();
        view.duration = session.metadata.durations[session.clip];
        let indices = session.visible_channels();
        view.channel_count = if session.bone_filter {
            session
                .selected_node
                .and_then(|node| session.metadata.channels_by_node[session.clip].get(&node))
                .map_or(0, Vec::len)
        } else {
            clip.channels.len()
        };
        view.rows = indices
            .into_iter()
            .map(|index| {
                let channel = &clip.channels[index];
                let first = channel.keys.partition_point(|k| k.time < view.start);
                let last = channel
                    .keys
                    .partition_point(|k| k.time <= view.start + view.span);
                let count = last - first;
                let stride = count.div_ceil(KEY_SLOTS).max(1);
                AnimationChannelView {
                    index,
                    name: channel.node_name.clone(),
                    path: channel.path.clone(),
                    selected: session.channel == Some(index),
                    keys: channel
                        .keys
                        .iter()
                        .enumerate()
                        .skip(first)
                        .take(count)
                        .step_by(stride)
                        .take(KEY_SLOTS)
                        .map(|(key_index, key)| AnimationKeyView {
                            index: key_index,
                            time: key.time,
                            selected: session.selected.contains(index, key.time),
                            clustered: stride > 1,
                        })
                        .collect(),
                }
            })
            .collect();
        if let Some(channel) = session.channel.and_then(|i| clip.channels.get(i)) {
            view.interpolation = channel.interpolation.clone();
            view.key_time = session
                .key
                .and_then(|i| channel.keys.get(i))
                .map(|k| k.time);
            view.key_values = channel.sample(session.position);
            if let Some((node, pose)) = session.pose.filter(|(node, _)| *node == channel.node) {
                let _ = node;
                view.key_values = match channel.path.as_str() {
                    "rotation" => pose.rotation.to_array().to_vec(),
                    "translation" => pose.translation.to_array().to_vec(),
                    "scale" => pose.scale.to_array().to_vec(),
                    _ => view.key_values,
                };
            }
            if channel.path == "rotation" && view.key_values.len() == 4 {
                let (x, y, z) = Quat::from_array(view.key_values.as_slice().try_into().unwrap())
                    .normalize()
                    .to_euler(EulerRot::XYZ);
                view.key_values = vec![x.to_degrees(), y.to_degrees(), z.to_degrees()];
            }
        }
        view
    }
}

#[derive(Resource, Default)]
pub(super) struct BindingState {
    pub(super) identity: Option<BindingIdentity>,
    pub(super) task: Option<(
        Option<BindingIdentity>,
        Task<Result<Option<SkeletonMapping>, String>>,
    )>,
    pub(super) retired: RetiredModels,
    pub(super) mapping: Option<SkeletonMapping>,
    pub(super) banks: [ModelBank; 2],
    pub(super) handle: Option<Handle<WorldAsset>>,
    pub(super) retiring: bool,
    pub(super) failed: bool,
    pub revision: u64,
    pub active: bool,
}
