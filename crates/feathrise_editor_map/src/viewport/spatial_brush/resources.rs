//! 空间笔刷只保存固定坐标、两个端点和一个待执行请求，关闭或换图清除。
use super::{super::placement_move::MoveHandle, types::*};
use crate::{authoring::EditorAuthoringSession, resources::MapEditorState};
use bevy::prelude::*;
use feathrise_map_content::MapId;

#[derive(Resource, Default)]
pub(crate) struct SpatialBrushState {
    pub(super) scope: Option<(MapId, String)>,
    pub(super) enabled: bool,
    pub(super) mode: ApplyMode,
    pub(super) path: super::path::PathState,
    pub(super) center: Option<Vec3>,
    pub(super) start: Option<Vec3>,
    pub(super) end: Option<Vec3>,
    pub(super) armed: bool,
    pub(super) click_pending: bool,
    pub(super) click_identity: Option<Identity>,
    pub(super) click_started: f64,
    pub(super) drag: Option<SpatialDrag>,
    pub(super) hover: Option<MoveHandle>,
    pub(super) owns_mouse: bool,
    pub(super) pending: Option<SpatialBrushRequest>,
    pub(super) authoring_available: bool, // Cursor阶段复验模态/模式门，尚未受理的后台段不能迟到执行。
}
impl SpatialBrushState {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }
    pub(crate) fn path_active(&self) -> bool {
        self.enabled && self.mode == ApplyMode::Path
    }
    pub(crate) fn owns_mouse(&self) -> bool {
        self.owns_mouse
    }
    pub(crate) fn authoring_available(&self) -> bool {
        self.authoring_available
    }
    pub(crate) fn follows_cursor(&self) -> bool {
        !self.enabled || self.armed && !self.click_pending
    }
    pub(crate) fn take_request(
        &mut self,
        editor: &MapEditorState,
        session: &EditorAuthoringSession,
    ) -> Result<Option<SpatialBrushRequest>, String> {
        let Some(request) = self.pending.take() else {
            return Ok(None);
        };
        if !self.enabled || !request.matches(editor, session) {
            return Err("空间雕刻请求已过期，当前地形保持不变".into());
        }
        Ok(Some(request))
    }
    pub(super) fn sync_scope(&mut self, editor: &MapEditorState, session: &EditorAuthoringSession) {
        let scope = editor
            .open
            .then(|| {
                session
                    .draft
                    .as_ref()
                    .map(|d| (d.map_id.clone(), d.terrain_folder().to_owned()))
            })
            .flatten();
        if self.scope != scope {
            let held = self.owns_mouse;
            let mut path = std::mem::take(&mut self.path);
            path.reset();
            *self = Self {
                scope,
                owns_mouse: held,
                path,
                ..default()
            };
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|p| !p.matches(editor, session))
        {
            self.pending = None;
        }
    }
    pub(super) fn clear(&mut self) {
        self.path.reset();
        self.center = None;
        self.start = None;
        self.end = None;
        self.armed = false;
        self.click_pending = false;
        self.click_identity = None;
        self.pending = None;
        self.drag = None;
    }
}
