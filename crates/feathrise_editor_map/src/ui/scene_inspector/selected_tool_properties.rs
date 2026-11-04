//! 地表、传送与边界共用原作者表单；检查器只移挂唯一实体树，不复制数值根或选择状态。

use bevy::prelude::*;

use super::{components::{SelectedToolProperties, SelectedToolPropertiesHost}, types::SelectedToolPropertiesKind};
use crate::{
    authoring::EditorAuthoringSession,
    resources::MapEditorState,
    ui::sidebars::{EditorRightSidebarTab, MapEditorSidebarState},
    viewport::{LayerCandidateState, LayerTarget},
};

/// 只核对已采用选择的单一下标与稳定ID，不为显隐扫描地图或把旧画布身份借给新地图。
fn selected_kind(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    selection: &LayerCandidateState,
) -> Option<SelectedToolPropertiesKind> {
    let draft = session.draft.as_ref().filter(|draft| {
        editor.open && !editor.map_transition_in_progress
            && editor.selected_map_id.as_ref() == Some(&draft.map_id)
    })?;
    let index = selection.selected_author_index?;
    if !selection.selected_matches_session(session) { return None; }
    match selection.selected.as_ref()? {
        LayerTarget::Surface(id) if selection.author_canvas_matches(draft)
            && draft.surface.as_ref()?.layers.get(index)?.id == *id =>
                Some(SelectedToolPropertiesKind::Surface),
        LayerTarget::Portal(id) if draft.portals.portals.get(index)?.id == *id =>
            Some(SelectedToolPropertiesKind::Portal),
        LayerTarget::Boundary(id) if draft.boundaries.as_ref()?.paths.get(index)?.id == *id =>
            Some(SelectedToolPropertiesKind::Boundary),
        _ => None,
    }
}

/// 在正式点选/作者采用之后运行；工具页重新打开、清选或切图时，原表单回到各自工具宿主。
pub(crate) fn sync_selected_tool_properties(
    mut commands: Commands,
    editor: Option<Res<MapEditorState>>,
    session: Option<Res<EditorAuthoringSession>>,
    selection: Option<Res<LayerCandidateState>>,
    sidebar: Option<Res<MapEditorSidebarState>>,
    mut hosts: Query<(Entity, &mut Node), (With<SelectedToolPropertiesHost>, Without<SelectedToolProperties>)>,
    mut forms: Query<(Entity, &mut SelectedToolProperties, &ChildOf, &mut Node), Without<SelectedToolPropertiesHost>>,
) {
    let Ok((host, mut host_node)) = hosts.single_mut() else { return; };
    let kind = editor.as_deref().zip(session.as_deref()).zip(selection.as_deref())
        .zip(sidebar.as_deref()).and_then(|(((editor, session), selection), sidebar)| {
            (sidebar.right == EditorRightSidebarTab::Scene).then(|| selected_kind(editor, session, selection)).flatten()
        });
    let mut mounted = false;
    for (entity, mut form, parent, mut node) in &mut forms {
        let home = if let Some(home) = form.home { home } else {
            let home = parent.parent(); form.home = Some(home); home
        };
        let show = kind == Some(form.kind);
        if form.in_inspector != show { form.in_inspector = show; }
        let target = if show { host } else { home };
        if parent.parent() != target {
            commands.entity(entity).insert(ChildOf(target));
        }
        if show {
            mounted = true;
            if node.display != Display::Flex { node.display = Display::Flex; }
            // 原工具页的折叠祖先不再决定所选对象属性的显隐。
        }
    }
    let display = if mounted { Display::Flex } else { Display::None };
    if host_node.display != display { host_node.display = display; }
}

#[cfg(test)]
#[path = "selected_tool_properties_tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) use tests::{set_selected_tool_properties_fixture, install_selected_tool_properties_snapshot_presentation};
