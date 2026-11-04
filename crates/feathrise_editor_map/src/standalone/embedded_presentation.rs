//! 原生预览只发布实际模式；作者工作台和独立启动一样保持原界面与编辑现场。

use super::StandaloneEditorMode;
use bevy::prelude::*;

/// 宿主在收到当前会话的 Playing / Editing 回执后调用；故障退出传 false。
/// 预览窗口自己拥有玩法焦点和窗口控制，不能再用 editor.open=false 隐藏整套作者界面。
/// 返回也不恢复旧 open/焦点快照，避免覆盖作者在另一个窗口中继续操作的现场。
pub fn set_embedded_playing(world: &mut World, playing: bool) {
    let Some(mut mode) = world.get_resource_mut::<StandaloneEditorMode>() else {
        return;
    };
    if mode.embedded_playing != playing {
        mode.embedded_playing = playing;
    }
}

#[cfg(test)]
#[path = "embedded_presentation_tests.rs"]
mod tests;
