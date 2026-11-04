//! 复用Bevy字形命中、拖选与选区绘制；日志只允许阅读操作，跨虚拟页选区绑定完整缓存。

use super::{components::LogBody, resources::*};
use bevy::{clipboard::Clipboard, prelude::*, text::{EditableText, TextEdit}};

/// 官方编辑器没有只读开关。两次apply_text_edits之前都过滤写操作，不能靠字符过滤器阻止删除。
pub(super) fn readonly(
    mut state: ResMut<ProjectLogState>,
    mut bodies: Query<(&mut LogBody, &mut EditableText)>,
) {
    for (mut body, mut text) in &mut bodies {
        text.pending_paste = None;
        text.pending_edits.retain(|edit| {
            if !state.open { return false; }
            match edit {
                TextEdit::Copy => { state.copy_selection = true; false }
                TextEdit::SelectAll => {
                    state.select_all = true;
                    state.follow = false;
                    state.dirty = true;
                    false
                }
                edit if is_read_action(edit) => {
                    // 重排尚未发布时，画面仍属于上一修订；不能将旧字形命中套到新的活尾行。
                    if state.wrap && !state.wrap_layout.rows.is_empty()
                        && state.wrap_layout.revision != Some(state.content_revision) { return false; }
                    let extend = matches!(edit,
                        TextEdit::ExtendSelectionToPoint(_) | TextEdit::ShiftClickExtension(_)
                        | TextEdit::Left(true) | TextEdit::Right(true) | TextEdit::Up(true) | TextEdit::Down(true)
                        | TextEdit::WordLeft(true) | TextEdit::WordRight(true)
                        | TextEdit::LineStart(true) | TextEdit::LineEnd(true)
                        | TextEdit::HardLineStart(true) | TextEdit::HardLineEnd(true)
                        | TextEdit::TextStart(true) | TextEdit::TextEnd(true));
                    body.extend_selection = extend && (!body.read_selection || body.extend_selection);
                    body.read_selection = true;
                    state.select_all = false;
                    state.follow = false;
                    state.dirty = true;
                    true
                }
                _ => false, // 输入、粘贴、剪切、删除和输入法提交均不得改写真实日志。
            }
        });
    }
}

fn is_read_action(edit: &TextEdit) -> bool {
    // 失焦时官方会折叠选区；日志必须允许先选中，再点击“复制选中”而不丢失原选区。
    matches!(edit, TextEdit::SelectAllIfCollapsed
        | TextEdit::MoveToPoint(_) | TextEdit::SelectWordAtPoint(_) | TextEdit::SelectLineAtPoint(_)
        | TextEdit::SelectedHardLineAtPoint(_) | TextEdit::ExtendSelectionToPoint(_) | TextEdit::ShiftClickExtension(_)
        | TextEdit::Left(_) | TextEdit::Right(_) | TextEdit::Up(_) | TextEdit::Down(_)
        | TextEdit::WordLeft(_) | TextEdit::WordRight(_)
        | TextEdit::LineStart(_) | TextEdit::LineEnd(_) | TextEdit::HardLineStart(_) | TextEdit::HardLineEnd(_)
        | TextEdit::TextStart(_) | TextEdit::TextEnd(_))
}

pub(super) fn remember_selection(
    mut state: ResMut<ProjectLogState>,
    mut bodies: Query<(&mut LogBody, &EditableText)>,
) {
    for (mut body, text) in &mut bodies {
        if !body.read_selection { continue; }
        let selection = text.editor.raw_selection();
        let anchor = state.position_at(body.origin + selection.anchor().index() as u64);
        let focus = state.position_at(body.origin + selection.focus().index() as u64);
        let anchor = if body.extend_selection { state.selection.map_or(anchor, |old| old.anchor) } else { anchor };
        state.selection = Some(LogSelection { anchor, focus });
        state.dirty = true;
        body.read_selection = false;
        body.extend_selection = false;
    }
}

pub(super) fn copy(mut state: ResMut<ProjectLogState>, mut clipboard: ResMut<Clipboard>) {
    let all = std::mem::take(&mut state.copy_all);
    let selected = std::mem::take(&mut state.copy_selection);
    if !state.open || (!all && !selected) { return; }
    let text = state.copy_text(all);
    state.notice = if text.is_empty() {
        if all { "当前没有可复制的日志".into() } else { "请先拖选文本，或点击复制全部".into() }
    } else {
        match clipboard.set_text(&text) {
            Ok(()) => if all { "已复制全部缓存日志".into() } else { "已复制选中文本".into() },
            Err(error) => format!("复制失败：{error}"),
        }
    };
    state.dirty = true;
}

pub(super) fn local_position(text: &str, origin: u64, position: u64) -> usize {
    let mut at = position.saturating_sub(origin).min(text.len() as u64) as usize;
    while !text.is_char_boundary(at) { at -= 1; }
    at
}
