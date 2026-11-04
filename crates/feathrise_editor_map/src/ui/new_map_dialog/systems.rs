//! 地图资料反馈独立滚动；正文只保留名称和备注，无地形或LOD会话。

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use super::components::DialogFeedbackScroll;
use crate::resources::MapEditorState;

/// 反馈保留完整真实状态，长错误独立滚动；新反馈从顶部开始，不借用正文或LOD滚动位置。
pub(super) fn scroll_feedback(
    editor: Res<MapEditorState>,
    mut previous: Local<Option<String>>,
    mut wheels: MessageReader<MouseWheel>,
    mut feedback: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<DialogFeedbackScroll>,
    >,
) {
    let delta: f32 = wheels
        .read()
        .map(|wheel| {
            -wheel.y
                * if wheel.unit == MouseScrollUnit::Line {
                    24.0
                } else {
                    1.0
                }
        })
        .sum();
    if !editor.open || !editor.new_map_dialog_open {
        *previous = None;
        return; // 消费隐藏期间消息，重新打开不回放。
    }
    let changed = previous.is_none()
        || (editor.is_changed() && previous.as_deref() != Some(editor.status.as_str()));
    if changed {
        *previous = Some(editor.status.clone());
    }
    for (cursor, computed, mut scroll) in &mut feedback {
        let maximum = ((computed.content_size().y - computed.size().y)
            * computed.inverse_scale_factor())
        .max(0.0);
        let offset = if changed {
            0.0
        } else if cursor.cursor_over() {
            (scroll.y + delta).clamp(0.0, maximum)
        } else {
            scroll.y.clamp(0.0, maximum)
        };
        if scroll.y != offset {
            scroll.y = offset;
        }
    }
}
