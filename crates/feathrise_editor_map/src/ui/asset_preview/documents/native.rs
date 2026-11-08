//! 切文件时交换原生编辑状态，保留各自的光标、选区和滚动；仍只有一个ECS正文输入。

use super::{components::*, constants::MAX_EDIT_CHARS, resources::*, types::*};
use bevy::{input_focus::InputFocus, prelude::*, text::{EditableText, EditableTextGeneration, LineHeight}};

pub(super) fn bind(
    state: Res<DocumentState>,
    mut inputs: ResMut<DocumentInputs>,
    mut focus: ResMut<InputFocus>,
    mut sources: Query<(
        Entity, &mut DocumentSource, &mut EditableText, &mut EditableTextGeneration,
        &mut DocumentScrollState,
        (&mut TextFont, &mut LineHeight, &mut TextLayout, &mut ComputedNode),
    )>,
) {
    let Ok((entity, mut source, mut input, mut layout_generation, mut preservation,
        (mut font, mut line_height, mut layout, mut node))) = sources.single_mut() else { return; };
    if !state.is_changed() && !source.is_added() {
        return;
    }
    // 重新载入和会话淘汰使旧输入失效；最多8份，后台文件不参与原生排版系统。
    inputs.sessions.retain(|entry| state.documents.iter().any(|doc| doc.path == entry.key.path && doc.generation == entry.key.generation));
    let doc = state.current();
    let editing = doc.filter(|doc| state.visible && doc.mode == ViewMode::Edit && !state.is_loading(&doc.path));
    if let Some(previous_key) = source.key.as_ref().filter(|key| {
        editing.is_none_or(|doc| key.path != doc.path || key.generation != doc.generation)
    }).cloned() {
        // 在切到预览/读取中或隐藏正文之前保存；隐藏控件的零尺寸排版不能覆盖旧文件视区。
        let previous = std::mem::take(&mut *input);
        if let Some(previous_doc) = state.documents.iter().find(|doc| doc.path == previous_key.path && doc.generation == previous_key.generation)
            && !previous.is_composing() && previous.pending_paste.is_none() && previous.pending_edits.is_empty()
            && previous.value() == previous_doc.source_text() // 被重载门拒绝的残余输入不能在失败后从原生缓存回灌。
        {
            inputs.sessions.push(NativeDocumentInput { key: previous_key, revision: previous_doc.revision, scroll: previous.viewport.offset, input: previous });
        }
        source.key = None;
        *preservation = DocumentScrollState::default();
    }
    if let Some(doc) = editing {
        let key = doc.source_key();
        if source.key.as_ref() == Some(&key) {
            return;
        }
        let restored = inputs.sessions.iter().position(|entry| entry.key == key)
            .map(|index| inputs.sessions.remove(index));
        *preservation = DocumentScrollState::default();
        if let Some(restored) = restored.filter(|entry| entry.revision == doc.revision) {
            *input = restored.input;
            input.viewport.offset = restored.scroll;
            preservation.restore = Some(restored.scroll); // 首帧不能被旧caret跟随覆盖手动浏览位置。
        } else {
            input.editor.set_text(doc.source_text());
            input.max_characters = Some(MAX_EDIT_CHARS);
            input.allow_newlines = true;
            input.visible_lines = None;
            input.queue_edit(bevy::text::TextEdit::TextStart(false));
            input.viewport.offset = Vec2::ZERO;
        }
        // 不同文件的原生编辑器可能恰好同代；清除ECS排版代号，保证换回缓存后更新正文。
        *layout_generation = EditableTextGeneration::default();
        // Bevy只按组件变化同步样式和视区宽度，新建或取回的编辑器必须重新采用当前UI参数。
        font.set_changed();
        line_height.set_changed();
        layout.set_changed();
        node.set_changed();
        source.key = Some(key);
    } else if focus.get() == Some(entity) {
        focus.clear(); // 隐藏文件不能继续收中文或影响地图按键。
        if input.is_composing() {
            input.clear();
            if let Some(doc) = doc { input.editor.set_text(doc.source_text()); }
            source.key = None; // 隐藏时只保留已确认草稿，不保存候选状态。
        }
    }
}

/// 只记录当前已采用正文的滚动值，旧构造树和旧文件不得污染新标签的视区。
pub(super) fn capture_preview(
    mut state: ResMut<DocumentState>,
    rendering: Res<PreviewRenderState>,
    areas: Query<&ScrollPosition, (With<DocumentPreviewScroll>, Changed<ScrollPosition>)>,
) {
    let Some(active) = rendering.active.as_ref() else { return; };
    let Some(doc) = state.current().filter(|doc| state.visible && doc.mode == ViewMode::Preview
        && doc.path == active.identity.path && doc.generation == active.identity.generation
        && doc.revision == active.identity.revision) else { return; };
    if let Ok(scroll) = areas.single() {
        let value = Vec2::new(scroll.x, scroll.y);
        if doc.preview_scroll != value { state.current_mut().unwrap().preview_scroll = value; }
    }
}
