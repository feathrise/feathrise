//! 草稿采用时只暂存选区元数据；重新打开文件并完成原生绑定后才恢复，不排版后台文件。

use super::super::super::{components::DocumentSource, resources::DocumentState};
use super::super::control::busy;
use super::resources::AssetReplaceState;
use bevy::{
    input_focus::InputFocus,
    prelude::*,
    text::{EditableText, FontCx, LayoutCx},
};

pub(super) fn select(
    documents: Res<DocumentState>,
    mut state: ResMut<AssetReplaceState>,
    focus: Res<InputFocus>,
    mut sources: Query<(Entity, &DocumentSource, &mut EditableText)>,
    mut font: ResMut<FontCx>,
    mut layout: ResMut<LayoutCx>,
) {
    if state.restores.is_empty() {
        return;
    }
    let stale = state.restores.iter().any(|(stamp, _)| {
        !documents.documents.iter().any(|doc| {
            doc.path == stamp.key.path
                && doc.generation == stamp.key.generation
                && doc.revision == stamp.revision
        })
    });
    if stale {
        state.restores.retain(|(stamp, _)| {
            documents.documents.iter().any(|doc| {
                doc.path == stamp.key.path
                    && doc.generation == stamp.key.generation
                    && doc.revision == stamp.revision
            })
        });
    }
    let Some(doc) = documents.current().filter(|_| documents.visible) else {
        return;
    };
    let Some(index) = state.restores.iter().position(|(stamp, _)| {
        stamp.key.path == doc.path
            && stamp.key.generation == doc.generation
            && stamp.revision == doc.revision
    }) else {
        return;
    };
    let Ok((entity, binding, mut text)) = sources.single_mut() else {
        return;
    };
    if binding
        .key
        .as_ref()
        .is_none_or(|key| key.path != doc.path || key.generation != doc.generation)
    {
        return;
    }
    if busy(&text) {
        return;
    } // 初始化TextStart先由官方队列完成，不能提前把恢复的选区又覆盖为零。
    let (_, selection) = state.restores.remove(index);
    if focus.get() == Some(entity) {
        return;
    } // 作者已经点击接管源码焦点时，不覆盖新的光标。
    text.editor
        .driver(&mut font.context, &mut layout.0)
        .select_byte_range(selection.anchor, selection.focus);
}
