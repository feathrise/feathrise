//! 单个临时菜单与至多一个明确操作；不持有整图或另建保存状态。

use super::types::{MenuTarget, SceneObjectRequest};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct SceneContextMenuState {
    pub(super) target: Option<MenuTarget>,
    pub(super) anchor: Vec2,
    pub(super) renaming: bool,
    pub(super) captured_this_frame: bool,
    /// 关闭点击先等待原鼠标静默门，随后只发布一帧，不能在其他地图补执行。
    pub(super) queued: Option<(MenuTarget, SceneObjectRequest)>,
    pub(crate) ready: Option<SceneObjectRequest>,
}

/// 实际外部作者只收到自己那一行的已校验命令，不依赖普通地图各类完整对象格式。
pub struct DomainSceneRequest<'a> {
    pub kind: crate::game_authoring::map_interaction::DomainItemKind,
    pub id: &'a str,
    pub action: super::types::SceneMenuAction,
    pub name: &'a str,
    pub hidden: bool,
    pub locked: bool,
}

impl SceneContextMenuState {
    pub fn domain_request(&self, editor: &crate::resources::MapEditorState, session: &crate::authoring::EditorAuthoringSession) -> Option<DomainSceneRequest<'_>> {
        let request = self.ready.as_ref().filter(|request| request.target.current(editor, session) && request.target.allows(request.action))?;
        let super::types::SceneObjectKey::Domain { kind, id, .. } = &request.target.key else { return None; };
        Some(DomainSceneRequest { kind: *kind, id, action: request.action, name: &request.name, hidden: request.target.hidden, locked: request.target.locked })
    }
    pub fn owns_mouse(&self) -> bool {
        self.target.is_some() || self.captured_this_frame
    }
}
