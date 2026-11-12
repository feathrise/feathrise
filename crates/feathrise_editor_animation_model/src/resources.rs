//! 动画草稿、轻量关键帧历史和单任务背压；完整文档的复制与修改均在后台完成。

use super::{
    constants::{CHANNEL_SLOTS, KEY_SLOTS},
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
pub struct PoseInputState {
    pub gesture: Option<PoseGesture>,
    pub pick: Option<(PathBuf, u64, Task<Option<usize>>)>,
}

/// 历史只保存被编辑的关键帧，避免每次修改都保留整个动作文件。
#[derive(Clone)]
pub enum Edit {
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
    pub fn target(&self) -> (usize, usize) {
        match self {
            Self::Channels { clip, values } => (*clip, values.first().map_or(0, |v| v.0)),
            Self::Key { clip, channel, .. } | Self::Interpolation { clip, channel, .. } => {
                (*clip, *channel)
            }
        }
    }
    pub fn apply(
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

pub struct Session {
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
    pub fn select_clip(&mut self, clip: usize) {
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
    pub fn clip_from_source(&self, index: usize) -> Option<usize> {
        // 解析器按 GLB 原顺序保留片段。目录传来的是原索引，不能用编辑器数组位置猜测身份。
        self.document.clips.binary_search_by_key(&index, |clip| clip.source_index).ok()
    }
    pub fn new(document: AnimationDocument) -> Self {
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
    pub fn dirty(&self) -> bool {
        self.pose.is_some() || self.saved_cursor != Some(self.cursor)
    }
}

pub struct Metadata {
    pub durations: Vec<f32>,
    pub bones: Vec<Vec<usize>>,
    pub channels_by_node: Vec<HashMap<usize, Vec<usize>>>,
}
impl Metadata {
    pub fn new(document: &AnimationDocument) -> Self {
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

pub enum PendingAction {
    Command(super::types::Command),
    Edit(Edit),
    Undo,
    Redo,
    Save,
    Reload,
}
pub enum TaskResult {
    Copied(Result<Arc<super::types::Clipboard>, String>),
    Load(Result<Session, String>),
    RigLoad { ticket: u64, result: Result<Session, String> },
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
pub struct Work {
    pub path: PathBuf,
    /// 同路径热更新也会换代；旧校验/读取不得清掉请求启动后的新变更标记。
    pub generation: Option<u64>,
    pub saving: bool, // 只读试玩门必须区分GLB写入与同槽的读取/编辑任务，不猜诊断文案。
    pub task: Task<TaskResult>,
}

/// 单份待释放所有权；不在采用帧析构旧曲线、旧历史或迟到读取的大文件二进制。
pub enum Retired {
    Clipboard(Arc<super::types::Clipboard>),
    Session(Session),
    Document {
        document: Arc<AnimationDocument>,
        metadata: Option<Metadata>,
        history: Vec<Edit>,
    },
    Result(TaskResult),
    #[cfg(any(test, feature = "runtime_test_support"))]
    ReleaseProbe(Box<dyn FnOnce() + Send + Sync>),
}

impl Retired {
    /// 只由唯一计算任务调用；显式消费各类所有权，最后一个 Arc 也在后台归还。
    pub fn release(self) {
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
            #[cfg(any(test, feature = "runtime_test_support"))]
            Self::ReleaseProbe(probe) => probe(),
        }
    }
}

/// 已发布的人形动作帧。播放请求或动作来源换代后，旧书签不能恢复到新容器。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeightFrame {
    source_revision: u64,
    binding_request: u64,
    clip: usize,
    pub time: f32,
}

#[derive(Resource)]
pub struct AnimationEditorState {
    /// 原保存结果按源路径冻结，工程收尾不能借当前可见状态栏推断隐藏源的成功。
    pub project_save_receipt: Option<(PathBuf, Result<(), String>)>,
    pub active: Option<PathBuf>,
    pub sessions: HashMap<PathBuf, Session>,
    pub work: Option<Work>,
    pub retired: Option<Retired>,
    pub pending: Option<PendingAction>,
    pub visible: bool,
    pub playing: bool,
    pub looping: bool,
    pub fps: u32,
    pub speed: f32,
    pub revision: u64,
    pub source_revision: u64,
    pub status: String,
    pub failed: Option<PathBuf>,
    pub mouse_owned: bool,
    pub wheel_owned: bool,
    pub drag: Option<(Entity, PathBuf, u64)>,
    pub model_generation: Option<(PathBuf, u64)>,
    pub gesture: Option<TimelineGesture>,
    pub clipboard: Option<Arc<super::types::Clipboard>>,
    pub auto_key: bool,
    pub pose_tool: PoseTool,
    pub pose_mouse_owned: bool,
    pub pose_revision: u64,
    pub model_status: String,
    pub model_binding_revision: u64,
    pub rig_preview: Option<super::request::RigPreviewRequest>,
    pub weight_rest: Option<WeightFrame>,
    /// 独立只读会话按请求票据访问，退出或换源后由原工作槽后台释放。
    pub rig_session: Option<(u64, Session)>,
    /// 普通 GLB 的显式目录动作等待原打开流程发布文件会话，连续点击仅保留末次意图。
    pub directory_pick: Option<super::request::DirectoryPick>,
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
            weight_rest: None,
            rig_session: None,
            directory_pick: None,
        }
    }
}

impl AnimationEditorState {
    /// 动画源GLB写入仍占原工作槽；未保存姿态、读取和CPU编辑不阻止按磁盘版本试玩。
    pub fn saved_play_pending(&self) -> bool {
        matches!(self.pending, Some(PendingAction::Save)) || self.work.as_ref().is_some_and(|work| work.saving)
    }

    /// 动画源会话可在切换资产后继续留存；只读全部会话与原任务槽。
    pub fn exit_blocker(&self) -> Option<&'static str> {
        if self.busy() || self.drag.is_some() || self.gesture.is_some() || self.pose_mouse_owned {
            return Some("动画保存、编辑或手势尚未完成");
        }
        self.sessions.values().any(|session| session.dirty())
            .then_some("存在未保存的动画草稿或姿态，请先使用原动画保存或取消操作收口")
    }

    pub fn current(&self) -> Option<&Session> {
        if let Some(request) = &self.rig_preview {
            return self.rig_session.as_ref()
                .filter(|(ticket, _)| *ticket == request.ticket)
                .map(|(_, session)| session);
        }
        self.sessions.get(self.active.as_ref()?)
    }
    pub fn current_mut(&mut self) -> Option<&mut Session> {
        if let Some(request) = &self.rig_preview {
            return self.rig_session.as_mut()
                .filter(|(ticket, _)| *ticket == request.ticket)
                .map(|(_, session)| session);
        }
        self.sessions.get_mut(self.active.as_ref()?)
    }
    pub fn active_document(&self) -> Option<&AnimationDocument> {
        self.current().map(|s| s.document.as_ref())
    }
    pub fn document_snapshot(&self) -> Option<Arc<AnimationDocument>> {
        self.current().map(|s| s.document.clone())
    }
    pub fn active_clip(&self) -> usize {
        self.current().map_or(0, |s| s.clip)
    }
    pub fn position(&self) -> f32 {
        self.current().map_or(0.0, |s| s.position)
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn source_revision(&self) -> u64 {
        self.source_revision
    }
    pub fn source(&self) -> Option<&Path> {
        self.rig_preview.as_ref().map(|request| request.source.as_path()).or(self.active.as_deref())
    }
    pub fn visible(&self) -> bool {
        self.visible
            && self
                .current()
                .is_some_and(|session| !session.document.clips.is_empty())
    }
    pub fn retargeting(&self) -> bool {
        self.in_place_rig_preview()
            || self.current().is_some_and(|session| session.retarget_model && session.model_path.is_some())
    }
    pub fn in_place_rig_preview(&self) -> bool {
        self.rig_preview.is_some()
    }
    pub fn reset_rig_playback(&mut self) {
        self.playing = false;
        self.weight_rest = None;
        self.drag = None;
        self.gesture = None;
        self.pending = None;
        self.status.clear();
        self.model_status.clear();
        self.source_revision = self.source_revision.wrapping_add(1).max(1);
        self.model_binding_revision = self.model_binding_revision.wrapping_add(1);
        self.changed();
    }
    pub fn stop_rig_preview(&mut self) {
        if self.rig_preview.take().is_some() {
            self.reset_rig_playback();
        }
    }

    pub fn weight_motion_active(&self) -> bool {
        !self.weight_rest_active() && (self.in_place_rig_preview() || (self.visible && self.playing))
    }

    pub fn weight_frame(&self, time: f32) -> Option<WeightFrame> {
        (self.visible && self.in_place_rig_preview() && self.current().is_some()).then(|| WeightFrame {
            source_revision: self.source_revision,
            binding_request: self.model_binding_revision,
            clip: self.active_clip(),
            time,
        })
    }

    pub fn matches_weight_frame(&self, frame: WeightFrame) -> bool {
        !self.playing && self.weight_frame(self.position()) == Some(frame)
    }

    pub fn weight_rest_active(&self) -> bool {
        self.weight_rest.is_some_and(|frame| self.matches_weight_frame(frame))
    }

    pub fn show_weight_rest(&mut self, frame: WeightFrame) -> bool {
        if !self.matches_weight_frame(frame) {
            return false;
        }
        self.weight_rest = Some(frame);
        true
    }

    pub fn resume_weight_frame(&mut self, frame: WeightFrame) -> bool {
        if self.weight_rest != Some(frame) {
            return false;
        }
        self.weight_rest = None;
        self.matches_weight_frame(frame)
    }

    pub fn weight_rest_pose(&mut self) {
        self.stop_rig_preview();
        self.pause_weight_comparison(None);
    }

    pub fn pause_weight_comparison(&mut self, published: Option<f32>) {
        if let Some(time) = published {
            self.seek(time);
        }
        if self.playing || published.is_some() {
            self.playing = false;
            self.changed(); // 失效尚未发布的候选姿态，保留此刻已显示的帧。
        }
    }
    pub fn editable(&self) -> bool {
        !self.retargeting() && self.active_document().is_some_and(|doc| !doc.read_only)
    }
    pub fn owns_mouse(&self) -> bool {
        self.mouse_owned || self.drag.is_some() || self.gesture.is_some() || self.pose_mouse_owned
    }
    pub fn owns_wheel(&self) -> bool {
        self.visible && (self.wheel_owned || self.owns_mouse())
    }
    pub fn has_unsaved_draft(&self, root: &Path, path: &str) -> bool {
        self.sessions
            .get(&root.join(path))
            .is_some_and(Session::dirty)
    }
    pub fn busy(&self) -> bool {
        self.work.is_some() || self.retired.is_some() || self.pending.is_some()
    }
    pub fn changed(&mut self) {
        self.revision = self.revision.wrapping_add(1).max(1);
    }
    pub fn seek(&mut self, position: f32) {
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
    pub fn view(&self) -> AnimationView {
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
        view.clip_source_index = clip.source_index;
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
pub struct BindingState {
    pub identity: Option<BindingIdentity>,
    pub task: Option<(
        Option<BindingIdentity>,
        Task<Result<Option<SkeletonMapping>, String>>,
    )>,
    pub retired: RetiredModels,
    pub mapping: Option<SkeletonMapping>,
    pub banks: [ModelBank; 2],
    pub handle: Option<Handle<WorldAsset>>,
    pub retiring: bool,
    pub failed: bool,
    pub revision: u64,
    pub active: bool,
}

impl BindingState {
    pub fn weight_roots(&self, path: &str) -> [Option<Entity>; 2] {
        if self.identity.as_ref().is_some_and(|id| id.model == path) {
            [self.banks[0].root, self.banks[1].root]
        } else {
            [None, None]
        }
    }
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
