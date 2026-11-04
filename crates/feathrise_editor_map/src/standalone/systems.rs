//! 原保存任务收口后才发试玩消息，失败保留编辑器；不把作者World切成游戏World。

use super::{RunSavedProject, SavedPlayFeedback, StandaloneEditorMode};
use crate::{authoring::EditorAuthoringSession, resources::MapEditorState};
use bevy::prelude::*;

pub(super) fn publish_saved_play(
    mut mode: ResMut<StandaloneEditorMode>,
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    launch: Option<Res<feathrise_editor_core::ProjectLaunchSettings>>,
    mut play: MessageWriter<RunSavedProject>,
) {
    if !mode.pending {
        return;
    }
    if mode.external_play_active {
        // 原游戏/构建已经在运行时再次F12只负责停止，不能被另一笔作者保存等待吞掉。
        mode.pending = false;
        mode.waiting_save_sequence = None;
        play.write(RunSavedProject {
            profile: launch.as_ref().map_or(default(), |settings| settings.profile),
            map_id: None,
            has_unsaved_changes: editor.dirty,
            stop_only: true,
        });
        return;
    }
    if editor.save_requested || session.saving {
        mode.waiting_save_sequence
            .get_or_insert(session.successful_save_sequence);
        editor.status = if launch.as_ref().is_some_and(|settings| settings.preview) {
            "正在等待显式保存完成，完成后预览当前工程；再次点击“预览”取消等待".into()
        } else {
            "正在等待保存完成，随后启动工程".into()
        };
        return;
    }
    if editor.map_transition_in_progress {
        editor.status = "请等待地图切换完成，再运行已保存工程".into();
        return;
    }
    mode.pending = false;
    if mode
        .waiting_save_sequence
        .take()
        .is_some_and(|before| before == session.successful_save_sequence)
    {
        editor.status = "保存尚未成功，本次运行已取消；作者草稿及历史保留，请确认保存结果".into();
        return;
    }
    play.write(RunSavedProject {
        profile: launch.as_ref().map_or(default(), |settings| settings.profile),
        map_id: editor.selected_map_id.as_ref().map(|id| id.0.clone()),
        has_unsaved_changes: editor.dirty,
        stop_only: false,
    });
}

pub(super) fn receive_feedback(
    mut feedback: MessageReader<SavedPlayFeedback>,
    mut editor: ResMut<MapEditorState>,
) {
    // 后台通道与每帧发布都受宿主固定槽限制，状态回显不复制大文档。
    if let Some(message) = feedback.read().last() {
        editor.status.clone_from(&message.0);
    }
}
