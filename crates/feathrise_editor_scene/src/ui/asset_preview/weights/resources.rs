//! 当前 GLB 的蒙皮草稿、有限历史、单个后台任务及画布手势身份。

use feathrise_editor_core::view_projection::ViewProjection;
use feathrise_editor_glb_model::weights::{Brush, Prepared, Tool, VertexWeights, WeightDocument};
use super::{
    posed::{Bookmark, Capture, Gesture},
};
use bevy::{gltf::Gltf, prelude::*, tasks::Task};
use std::{
    collections::{BTreeMap, VecDeque},
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

const HISTORY_BYTES: usize = 48 * 1024 * 1024;
const HISTORY_COUNT: usize = 32;
pub(super) const STROKE_QUEUE: usize = 128;
pub(super) const BATCH_DABS: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Key {
    pub root: PathBuf,
    pub path: String,
    pub tab: u64,
}

pub(super) struct Draft {
    pub weights: Arc<[VertexWeights]>,
    pub saved: Arc<[VertexWeights]>,
    pub selection: Arc<[u32]>,
    pub shaded: Option<Handle<Mesh>>,
}

#[derive(Clone)]
pub(super) struct History {
    pub surface: usize,
    pub before: Arc<[VertexWeights]>,
    pub after: Arc<[VertexWeights]>,
}

impl History {
    fn size(&self) -> usize {
        (self.before.len() + self.after.len()) * std::mem::size_of::<VertexWeights>()
    }
}

pub(super) struct Session {
    pub key: Key,
    pub doc: Arc<dyn WeightDocument>,
    pub gltf: Handle<Gltf>,
    pub drafts: Vec<Draft>,
    pub surface: usize,
    pub joint: u16,
    pub heat_mesh: Option<(usize, Handle<Mesh>)>,
    pub original: bool,
    pub heat: bool,
    pub show_skeleton: bool,
    pub visible_only: bool,
    pub mirror: bool,
    pub mask: bool,
    pub tool: Tool,
    pub edit_tool: Tool,
    pub radius: f32,
    pub strength: f32,
    pub amount: f32,
    pub saved_path: Option<String>,
    pub stale: bool,
    pub locks: BTreeMap<usize, Arc<[bool]>>,
    pub undo: VecDeque<History>,
    pub redo: Vec<History>,
}

impl Session {
    pub fn new(key: Key, gltf: Handle<Gltf>, doc: Arc<dyn WeightDocument>) -> Self {
        let drafts = doc
            .surfaces()
            .map(|surface| Draft {
                weights: surface.original.clone(),
                saved: surface.original.clone(),
                selection: Arc::from([]),
                shaded: None,
            })
            .collect();
        let locks = doc
            .surfaces()
            .map(|surface| {
                (
                    surface.skin.index,
                    vec![false; surface.skin.joints.len()].into(),
                )
            })
            .collect();
        let joint = doc.surface(0)
            .skin
            .joints
            .iter()
            .position(|joint| {
                joint
                    .name
                    .rsplit(':')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("hips"))
            })
            .unwrap_or(0) as u16;
        Self {
            key,
            doc,
            gltf,
            drafts,
            surface: 0,
            joint,
            heat_mesh: None,
            original: false,
            heat: true,
            show_skeleton: true,
            visible_only: true,
            mirror: false,
            mask: false,
            tool: Tool::Bone,
            edit_tool: Tool::Point,
            radius: 32.0,
            strength: 0.3,
            amount: 0.5,
            saved_path: None,
            stale: false,
            locks,
            undo: VecDeque::new(),
            redo: Vec::new(),
        }
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        if tool != Tool::Bone {
            self.edit_tool = tool;
        }
    }

    pub fn dirty(&self) -> bool {
        self.drafts
            .iter()
            .any(|draft| !Arc::ptr_eq(&draft.weights, &draft.saved))
    }

    pub fn current_locks(&self) -> &Arc<[bool]> {
        &self.locks[&self.doc.surface(self.surface).skin.index]
    }

    pub fn brush(&self) -> Brush {
        Brush {
            tool: self.tool,
            joint: self.joint,
            radius: self.radius,
            strength: self.strength,
            visible_only: self.visible_only,
            mirror: self.mirror,
            restrict_selection: self.mask,
            locks: self.current_locks().clone(),
        }
    }

    pub fn record(&mut self, entry: History) {
        if Arc::ptr_eq(&entry.before, &entry.after) {
            return;
        }
        self.redo.clear();
        self.undo.push_back(entry);
        while self.undo.len() > 1
            && (self.undo.len() > HISTORY_COUNT
                || self.undo.iter().map(History::size).sum::<usize>() > HISTORY_BYTES)
        {
            self.undo.pop_front();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Message)]
pub(in crate::ui::asset_preview) enum TreePick {
    Joint(usize),
    Mesh(usize),
    Skin(usize),
}

pub(super) struct Stroke {
    pub surface: usize,
    pub before: Arc<[VertexWeights]>,
    pub brush: Brush,
    pub view: ViewProjection,
    pub start: Vec2,
    pub latest: Vec2,
    pub last_queued: Vec2,
    pub dabs: VecDeque<Vec2>,
    pub extend: bool,
    pub released: bool,
    pub cancelled: bool,
    pub problem: Option<Gesture>,
}

impl Stroke {
    pub fn advance(&mut self, position: Vec2, force_end: bool) {
        self.latest = position;
        if !self.brush.tool.paint() {
            return;
        }
        let pixels = self.view.size * self.view.inverse_scale;
        let distance = ((position - self.last_queued) * pixels).length();
        if distance < 0.01 {
            return;
        }
        let spacing = (self.brush.radius * 0.25).max(2.0);
        let count = (distance / spacing).floor() as usize;
        let count = if force_end { count.max(1) } else { count };
        if count == 0 {
            return;
        }
        let first = self.last_queued;
        let capacity = (STROKE_QUEUE - self.dabs.len()).min(count);
        for step in 1..=capacity {
            let t = if force_end {
                step as f32 / count as f32
            } else {
                (step as f32 * spacing / distance).min(1.0)
            };
            self.last_queued = first.lerp(position, t);
            self.dabs.push_back(self.last_queued);
        }
    }
}

pub(super) enum Reason {
    Display,
    Problem(Bookmark),
    Edit,
    Stroke,
    Rollback,
    Undo(History),
    Redo(History),
}

pub(super) enum Outcome {
    Open {
        key: Key,
        gltf: Handle<Gltf>,
        doc: Arc<dyn WeightDocument>,
    },
    Apply {
        result: Prepared,
        reason: Reason,
    },
    Save(String),
}

pub(super) struct Work {
    pub key: Key,
    pub saving: bool,
    pub cancel: Arc<AtomicBool>,
    pub task: Task<Result<Outcome, String>>,
}

#[derive(Resource, Default)]
pub(crate) struct WeightEditor {
    pub(super) session: Option<Session>,
    pub(super) opening: Option<(PathBuf, String)>,
    pub(super) work: Option<Work>,
    pub(super) stroke: Option<Stroke>,
    pub(super) capture: Option<Capture>,
    pub(super) problem: Option<Bookmark>,
    pub(super) notice: String,
    pub(super) refresh: bool,
    pub(super) revision: u64,
    pub(super) mouse: bool,
    pub(super) save_receipt: Option<(PathBuf, Result<String, String>)>,
}

impl WeightEditor {
    pub(super) fn busy(&self) -> bool {
        self.work.is_some() || self.stroke.is_some() || self.capture.is_some()
    }

    /// 画布与场景树共用选中骨骼和热力图刷新路径。
    pub(super) fn select_joint(&mut self, surface: usize, joint: u16) {
        if let Some(session) = self.session.as_mut()
            && (session.surface, session.joint) != (surface, joint)
            && surface < session.doc.surface_count()
            && usize::from(joint) < session.doc.surface(surface).skin.joints.len()
        {
            session.surface = surface;
            session.joint = joint;
            self.refresh = true;
            self.bump();
        }
    }

    pub(crate) fn owns_mouse(&self) -> bool {
        self.mouse
    }
    pub(crate) fn dirty(&self) -> bool {
        self.session.as_ref().is_some_and(Session::dirty)
    }

    pub(in crate::ui::asset_preview) fn selected_tree(
        &self,
        tab: Option<u64>,
    ) -> Option<[TreePick; 3]> {
        let session = self
            .session
            .as_ref()
            .filter(|session| Some(session.key.tab) == tab)?;
        let surface = session.doc.surface(session.surface);
        Some([
            TreePick::Mesh(surface.mesh),
            TreePick::Skin(surface.skin.index),
            TreePick::Joint(surface.skin.joints[session.joint as usize].node),
        ])
    }

    pub(super) fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}
