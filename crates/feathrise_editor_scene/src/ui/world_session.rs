//! 按真实世界就绪状态显示场景专属控件；空编辑器只保留全局内容工作区。

use bevy::prelude::*;

use crate::components::{SceneEditorActionButton, SceneEditorEmptyWorkspace};
use crate::resources::{SceneEditorState, SceneEditorWorldSession};
use crate::types::EditorAction;
use crate::ui::vegetation_preset_editor::{
    PresetEditorAction, PresetEditorButton, PresetWorkspaceTab, VegetationPresetEditorState,
};

pub(crate) fn sync_editor_world_controls(
    world: SceneEditorWorldSession,
    editor: Res<SceneEditorState>,
    workspace: Res<VegetationPresetEditorState>,
    mut previous_ready: Local<Option<(bool, bool, PresetWorkspaceTab)>>,
    mut actions: Query<
        (&SceneEditorActionButton, &mut Node),
        Without<SceneEditorEmptyWorkspace>,
    >,
    mut tabs: Query<
        (&PresetEditorButton, &mut Node),
        (
            Without<SceneEditorActionButton>,
            Without<SceneEditorEmptyWorkspace>,
        ),
    >,
    mut empty_pages: Query<
        &mut Node,
        (
            With<SceneEditorEmptyWorkspace>,
            Without<SceneEditorActionButton>,
            Without<PresetEditorButton>,
        ),
    >,
) {
    let ready = world.ready();
    for mut page in &mut empty_pages {
        page.display = if !ready
            && !editor.scene_transition_in_progress
            && workspace.active_tab == PresetWorkspaceTab::Scene
        {
            Display::Flex
        } else {
            Display::None
        };
    }
    let signature = (ready, editor.scene_transition_in_progress, workspace.active_tab);
    if *previous_ready == Some(signature) {
        return; // 场景可用性稳定时不扫描按钮；预设标签切换只更新固定中央提示节点。
    }
    *previous_ready = Some(signature);
    let display = if ready && workspace.active_tab == PresetWorkspaceTab::Scene {
        Display::Flex
    } else {
        Display::None
    };
    for (action, mut node) in &mut actions {
        if action.0 == EditorAction::Close {
            // 有场景返回玩法，空工作台收起后保留空场景；按钮和快捷键使用同一关闭语义。
            node.display = Display::Flex;
            // “启动”及绿色播放图标由静态顶栏负责；世界就绪变化只影响可见性。
        }
        if matches!(
            action.0,
            EditorAction::Undo | EditorAction::Redo | EditorAction::Save
        ) {
            node.display = display;
        }
    }
    for (action, mut node) in &mut tabs {
        if action.0 == PresetEditorAction::ActivateSceneTab {
            // 编辑预设时仍保留返回场景的入口，只有空世界才隐藏场景标签。
            node.display = if ready || editor.scene_transition_in_progress {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
}

#[cfg(test)]
#[path = "world_session_tests.rs"]
mod tests;
