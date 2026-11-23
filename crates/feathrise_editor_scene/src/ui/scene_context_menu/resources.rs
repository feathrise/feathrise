//! 单个临时菜单与至多一个明确操作；不持有整图或另建保存状态。

use super::types::{MenuTarget, SceneMenuAction, SceneObjectKey, SceneObjectRequest, SceneObjectTarget};
use bevy::prelude::*;

#[derive(Resource, Default)]
pub struct SceneContextMenuState {
    pub(super) target: Option<MenuTarget>,
    pub(super) anchor: Vec2,
    pub(super) renaming: bool,
    pub(super) name_action: Option<SceneMenuAction>,
    pub(super) captured_this_frame: bool,
    /// 关闭点击先等待原鼠标静默门，随后只发布一帧，不能在其他场景补执行。
    pub(super) queued: Option<(MenuTarget, SceneObjectRequest)>,
    pub(super) last_tree_click: Option<(MenuTarget, f64)>,
    pub(super) tree_focus_queued: bool,
    pub(crate) ready: Option<SceneObjectRequest>,
}

/// 实际外部作者只收到自己那一行的已校验命令，不依赖普通场景各类完整对象格式。
pub struct DomainSceneRequest<'a> {
    pub kind: crate::game_authoring::scene_interaction::DomainItemKind,
    pub id: &'a str,
    pub action: super::types::SceneMenuAction,
    pub name: &'a str,
    pub hidden: bool,
    pub locked: bool,
}

impl SceneContextMenuState {
    pub(crate) fn rebind_loaded_vegetation(&mut self, target: &SceneObjectTarget, revision: u64) {
        let SceneObjectKey::Vegetation { index, terrain, id, output, cold: None } = &target.key else { return; };
        let pending = |previous: &SceneObjectTarget| {
            previous.revision == revision && previous.scene_id == target.scene_id
                && matches!(&previous.key, SceneObjectKey::Vegetation {
                    index: old_index, terrain: old_terrain, id: old_id, output: old_output, cold: Some(_),
                } if (old_index, old_terrain, old_id, old_output) == (index, terrain, id, output))
        };
        if let Some((first, _)) = self.last_tree_click.as_mut() && pending(&first.object) {
            first.object = target.clone();
        }
        // 画布若恰在第二次释放和静默门之间完成，只续接同一行的聚焦，不解冻其他命令。
        if let Some((row, request)) = self.queued.as_mut() && self.tree_focus_queued
            && request.action == SceneMenuAction::Focus && pending(&row.object) && request.target == row.object
        {
            row.object = target.clone();
            request.target = target.clone();
        }
    }

    pub fn domain_request(&self, editor: &crate::resources::SceneEditorState, session: &crate::authoring::EditorAuthoringSession) -> Option<DomainSceneRequest<'_>> {
        let request = self.ready.as_ref().filter(|request| request.target.current(editor, session) && request.target.allows(request.action))?;
        let super::types::SceneObjectKey::Domain { kind, id, .. } = &request.target.key else { return None; };
        Some(DomainSceneRequest { kind: *kind, id, action: request.action, name: &request.name, hidden: request.target.hidden, locked: request.target.locked })
    }
    pub fn owns_mouse(&self) -> bool {
        self.target.is_some() || self.captured_this_frame
    }

    /// 后台采用若推进作者修订，须等菜单编辑与已冻结的点击分派完成。
    pub(crate) fn has_pending_interaction(&self) -> bool {
        self.owns_mouse() || self.queued.is_some() || self.ready.is_some()
    }
}
