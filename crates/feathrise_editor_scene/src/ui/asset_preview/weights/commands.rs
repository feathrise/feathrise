//! 顶栏保存和历史命令按当前中央 GLB 标签分派，继续使用原权重任务与回执。

use feathrise_editor_glb_model::weights::Operation;
use super::{
    super::{AssetPreviewState, AssetPreviewWorkspace, DocumentState},
    resources::{Key, Reason, Session, WeightEditor},
    systems::{Gate, Target, begin_save, matches_key, queue},
};
use crate::{
    resources::{SceneEditorLayoutState, SceneEditorState},
    types::EditorAction,
    ui::{
        sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::{ecs::system::SystemParam, prelude::*};

#[derive(Clone, Copy)]
pub(in crate::ui) struct WeightCommandStatus {
    ready: bool,
    busy: bool,
    stale: bool,
    dirty: bool,
    undo: bool,
    redo: bool,
}

impl WeightCommandStatus {
    pub(in crate::ui) fn dirty(self) -> bool {
        self.dirty
    }

    pub(in crate::ui) fn reason(self, action: EditorAction) -> Option<&'static str> {
        if !is_weight_command(action) {
            return None;
        }
        if self.stale {
            return Some("源 GLB 已更新，请重新打开权重");
        }
        if self.busy {
            return Some("当前权重操作尚未完成");
        }
        if !self.ready {
            return Some("当前权重预览尚未就绪");
        }
        match action {
            EditorAction::Save if !self.dirty => Some("当前没有待保存的权重修改"),
            EditorAction::Undo if !self.undo => Some("当前没有可撤销的权重修改"),
            EditorAction::Redo if !self.redo => Some("当前没有可重做的权重修改"),
            _ => None,
        }
    }
}

fn is_weight_command(action: EditorAction) -> bool {
    matches!(
        action,
        EditorAction::Save | EditorAction::Undo | EditorAction::Redo
    )
}

#[derive(Message)]
pub(in crate::ui) struct RequestWeightCommand {
    action: EditorAction,
    key: Key,
}

/// 场景命令面板可以独立使用，未安装资产预览时保留原场景资格。
#[derive(SystemParam)]
pub(crate) struct WeightCommands<'w> {
    weights: Option<Res<'w, WeightEditor>>,
    sidebar: Option<Res<'w, SceneEditorSidebarState>>,
    layout: Option<Res<'w, SceneEditorLayoutState>>,
    media: Option<Res<'w, AssetPreviewWorkspace>>,
    workspace: Option<Res<'w, VegetationPresetEditorState>>,
    documents: Option<Res<'w, DocumentState>>,
    preview: Option<Res<'w, AssetPreviewState>>,
}

impl WeightCommands<'_> {
    fn matches(&self, key: &Key) -> bool {
        self.sidebar
            .as_ref()
            .is_some_and(|sidebar| sidebar.right == EditorRightSidebarTab::Weights)
            && self
                .media
                .as_deref()
                .zip(self.workspace.as_deref())
                .zip(self.documents.as_deref())
                .is_some_and(|((media, workspace), documents)| {
                    matches_key(key, media, workspace, documents)
                })
    }

    fn session(&self) -> Option<&Session> {
        self.weights
            .as_deref()?
            .session
            .as_ref()
            .filter(|session| self.matches(&session.key))
    }

    pub(in crate::ui) fn status(&self) -> Option<WeightCommandStatus> {
        let state = self.weights.as_deref()?;
        let session = self.session();
        if session.is_none()
            && !state
                .work
                .as_ref()
                .is_some_and(|work| self.matches(&work.key))
        {
            return None;
        }
        Some(WeightCommandStatus {
            ready: session.is_some_and(|session| {
                self.preview.as_ref().is_some_and(|preview| {
                    preview.render_visible
                        && preview.external_preview.is_none()
                        && preview.selected_path.as_deref() == Some(session.key.path.as_str())
                })
            }) && self
                .layout
                .as_ref()
                .is_none_or(|layout| layout.inspector_shown()),
            busy: state.busy(),
            stale: session.is_some_and(|session| session.stale),
            dirty: session.is_some_and(Session::dirty),
            undo: session.is_some_and(|session| !session.undo.is_empty()),
            redo: session.is_some_and(|session| !session.redo.is_empty()),
        })
    }

    pub(in crate::ui) fn request(&self, action: EditorAction) -> Option<RequestWeightCommand> {
        if !is_weight_command(action) || self.status()?.reason(action).is_some() {
            return None;
        }
        Some(RequestWeightCommand {
            action,
            key: self.session()?.key.clone(),
        })
    }
}

pub(super) fn receive(
    mut requests: MessageReader<RequestWeightCommand>,
    mut state: ResMut<WeightEditor>,
    target: Target,
    gate: Gate,
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
) {
    let Some(request) = requests.read().last() else {
        return;
    };
    if state.busy()
        || !gate.can_press(&editor)
        || sidebar.right != EditorRightSidebarTab::Weights
        || !target.matches(&request.key)
        || !target.preview.render_visible
        || target.preview.external_preview.is_some()
        || target.preview.selected_path.as_deref() != Some(request.key.path.as_str())
    {
        return;
    }
    let Some(session) = state
        .session
        .as_mut()
        .filter(|session| session.key == request.key && !session.stale)
    else {
        return;
    };
    match request.action {
        EditorAction::Save => {
            if let Err(error) = begin_save(&mut state) {
                state.notice = error;
            }
        }
        EditorAction::Undo | EditorAction::Redo => {
            let undo = request.action == EditorAction::Undo;
            let entry = if undo {
                session.undo.back()
            } else {
                session.redo.last()
            }
            .cloned();
            if let Some(entry) = entry {
                let old_skin = session.doc.surface(session.surface).skin.index;
                session.surface = entry.surface;
                if session.doc.surface(session.surface).skin.index != old_skin {
                    session.joint = 0;
                }
                let (weights, reason) = if undo {
                    (entry.before.clone(), Reason::Undo(entry))
                } else {
                    (entry.after.clone(), Reason::Redo(entry))
                };
                queue(&mut state, Operation::History(weights), reason);
            }
        }
        _ => return,
    }
    state.bump();
}
