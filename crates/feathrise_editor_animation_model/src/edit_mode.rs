//! 预览、已选片段与显式动画编辑分开记录，编辑权限绑定当前作者来源和工作区。

use crate::resources::AnimationEditorState;
use feathrise_editor_core::PresetWorkspaceTab;

#[derive(Default)]
pub struct AnimationEditMode {
    tab: Option<PresetWorkspaceTab>,
    selected: Option<(u64, usize)>,
    editing_source: Option<u64>,
}

impl AnimationEditorState {
    pub fn sync_animation_scope(&mut self, tab: PresetWorkspaceTab, open: bool, weights: bool) {
        let switched = self.edit_mode.tab.replace(tab).is_some_and(|old| old != tab);
        if switched || !open {
            self.return_to_animation_preview();
        } else if weights {
            self.exit_animation_edit();
        }
    }

    pub fn return_to_animation_preview(&mut self) {
        self.edit_mode.selected = None;
        self.exit_animation_edit();
    }

    fn writable_source(&self) -> bool {
        // 工程动作已经校验并复用源 GLB 的作者会话。角色文件只提供预览目标。
        (self.in_place_rig_preview() || !self.retargeting())
            && self.active_document().is_some_and(|doc| !doc.read_only)
    }

    pub fn can_select_current_clip(&self) -> bool {
        self.writable_source() && self.current().is_some_and(|session|
            session.document.clips.get(session.clip).is_some())
    }

    pub fn select_current_clip(&mut self) -> bool {
        if !self.can_select_current_clip() {
            return false;
        }
        self.edit_mode.selected = Some((self.source_revision, self.active_clip()));
        self.changed();
        true
    }

    pub fn can_create_clip(&self) -> bool {
        self.active.as_ref().and_then(|path| self.sessions.get(path)).is_some_and(|session|
            session.pose.is_none() && session.document.can_create_clip())
    }

    pub fn can_edit_selected(&self) -> bool {
        self.can_select_current_clip()
            && self.edit_mode.selected == Some((self.source_revision, self.active_clip()))
    }

    pub fn enter_animation_edit(&mut self) -> bool {
        if !self.can_edit_selected() {
            return false;
        }
        self.edit_mode.editing_source = Some(self.source_revision);
        self.playing = false;
        self.changed();
        true
    }

    pub fn exit_animation_edit(&mut self) {
        if self.edit_mode.editing_source.take().is_some() {
            // 失效旧按钮和数值绑定，画布输入复验模式后还原未提交手势。
            self.changed();
        }
    }

    pub fn editing_animation(&self) -> bool {
        // 权限随当前来源保留。撤销唯一片段后，仍可在同一编辑态重做。
        self.edit_mode.editing_source == Some(self.source_revision) && self.writable_source()
    }
}
