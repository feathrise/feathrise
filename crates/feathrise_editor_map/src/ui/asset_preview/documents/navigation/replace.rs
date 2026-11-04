//! 后台字面替换与完整结果采用；原始字节、选区方向和独立一步历史成对更新。

use super::super::{
    components::DocumentSource,
    constants::MAX_FILE_BYTES,
    history::{EditContext, Selection},
    resources::DocumentState,
    types::ViewMode,
};
use super::{control::busy, index, resources::DocumentNavigation, types::*};
use crate::resources::{
    EditorTextInputGuard, MapEditorNumericScrubMouseGuard, MapEditorSettingsState, MapEditorState,
};
use bevy::{
    input_focus::InputFocus,
    prelude::*,
    text::{EditableText, FontCx, LayoutCx},
};
use std::{ops::Range, sync::Arc};

fn too_large() -> String {
    "替换后的完整文件超过256 KiB；草稿保持，请缩小范围或减少替换内容".into()
}

/// 只重写已匹配区间；跳过区间之间的原始字节，混合CRLF/LF不因全部替换而被统一。
pub(super) fn prepare(
    source: &str,
    pages: &[Arc<str>],
    crlf: bool,
    bom: bool,
    request: &ReplacementRequest,
) -> Result<Option<ReplacedContent>, String> {
    let ranges: &[Range<usize>] = match &request.target {
        Some(range) => std::slice::from_ref(range),
        None => &request.index.matches,
    };
    prepare_ranges(
        source,
        pages,
        crlf,
        bom,
        ranges,
        &request.replacement,
        request.selection,
        request.index.key.scope.as_ref(),
        request.target.is_some(),
    )
}

/// 两种替换入口复用同一保真算法；调用者只提交已受检、按UTF-8顺序排列的非重叠范围。
pub(in super::super) fn prepare_ranges(
    source: &str,
    pages: &[Arc<str>],
    crlf: bool,
    bom: bool,
    ranges: &[Range<usize>],
    replacement: &str,
    selection: Selection,
    scope: Option<&Range<usize>>,
    single: bool,
) -> Result<Option<ReplacedContent>, String> {
    let mut previous = 0;
    if ranges.iter().any(|range| {
        let invalid = range.start < previous
            || range.start >= range.end
            || source.get(range.clone()).is_none();
        previous = range.end;
        invalid
    }) {
        return Err("替换范围已失效；草稿保持".into());
    }
    if ranges.is_empty()
        || ranges
            .iter()
            .all(|range| &source[range.clone()] == replacement)
    {
        return Ok(None); // 同文替换不增加修订、脏位、历史或原生重排。
    }
    let limit = MAX_FILE_BYTES - usize::from(bom) * 3;
    let removed: usize = ranges.iter().map(|range| range.len()).sum();
    let size = replacement
        .len()
        .checked_mul(ranges.len())
        .and_then(|added| (source.len() - removed).checked_add(added))
        .filter(|size| *size <= limit)
        .ok_or_else(too_large)?;
    let raw: String = pages.iter().map(AsRef::as_ref).collect();
    let raw_replacement = if crlf {
        replacement.replace('\n', "\r\n")
    } else {
        replacement.to_owned()
    };
    let mut output = String::with_capacity(size);
    let mut bytes = String::with_capacity(limit.min(raw.len().saturating_add(size)));
    let positions = [
        selection.anchor,
        selection.focus,
        scope.map_or(0, |range| range.start),
        scope.map_or(0, |range| range.end),
    ];
    let mut mapped = [None; 4];
    let (mut source_end, mut raw_end, mut norm_cursor, mut raw_cursor, mut count) = (0, 0, 0, 0, 0);
    for range in ranges {
        let start = raw_offset(&raw, range.start, &mut norm_cursor, &mut raw_cursor);
        let end = raw_offset(&raw, range.end, &mut norm_cursor, &mut raw_cursor);
        output.push_str(&source[source_end..range.start]);
        if bytes.len() + start - raw_end > limit {
            return Err(too_large());
        }
        bytes.push_str(&raw[raw_end..start]);
        let changed = &source[range.clone()] != replacement;
        let insertion = if changed {
            raw_replacement.as_str()
        } else {
            &raw[start..end]
        };
        if bytes.len() + insertion.len() > limit {
            return Err(too_large());
        }
        bytes.push_str(insertion);
        for (mapped, position) in mapped.iter_mut().zip(positions) {
            if mapped.is_none() && position <= range.end {
                *mapped = Some(if position <= range.start {
                    output.len() - (range.start - position)
                } else if changed {
                    output.len() + replacement.len() // 区间内端点归到插入末端，不能落在中文UTF-8中间。
                } else {
                    output.len() + position - range.start
                });
            }
        }
        output.push_str(replacement);
        count += usize::from(changed);
        source_end = range.end;
        raw_end = end;
    }
    output.push_str(&source[source_end..]);
    if bytes.len() + raw.len() - raw_end > limit {
        return Err(too_large());
    }
    bytes.push_str(&raw[raw_end..]);
    for (mapped, position) in mapped.iter_mut().zip(positions) {
        if mapped.is_none() {
            *mapped = Some(output.len() - (source.len() - position));
        }
    }
    let selection = if single {
        let range = &ranges[0];
        Selection {
            anchor: range.start,
            focus: range.start + replacement.len(),
        }
    } else {
        Selection {
            anchor: mapped[0].unwrap(),
            focus: mapped[1].unwrap(),
        }
    };
    Ok(Some(ReplacedContent {
        source: Arc::from(output),
        pages: vec![Arc::from(bytes)],
        selection,
        scope: scope.map(|_| mapped[2].unwrap()..mapped[3].unwrap()),
        count,
    }))
}

fn raw_offset(raw: &str, target: usize, normalized: &mut usize, cursor: &mut usize) -> usize {
    while *normalized < target {
        *cursor += if raw.as_bytes()[*cursor..].starts_with(b"\r\n") {
            2
        } else {
            1
        };
        *normalized += 1;
    }
    *cursor
}

pub(super) fn apply(
    editor: Res<MapEditorState>,
    settings: Option<Res<MapEditorSettingsState>>,
    guard: Res<EditorTextInputGuard>,
    mouse: Res<MapEditorNumericScrubMouseGuard>,
    focus: Res<InputFocus>,
    mut documents: ResMut<DocumentState>,
    mut state: ResMut<DocumentNavigation>,
    fields: Query<
        &EditableText,
        (
            With<super::components::NavigationField>,
            Without<DocumentSource>,
        ),
    >,
    mut sources: Query<(Entity, &DocumentSource, &mut EditableText)>,
    mut font: ResMut<FontCx>,
    mut layout: ResMut<LayoutCx>,
) {
    if !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || settings.is_some_and(|settings| settings.open)
        || guard.block_shortcuts
        || mouse.blocked()
        || !documents.visible
        || documents.pending_load.is_some()
        || (documents.io.is_some() && documents.saving.is_none())
        || fields.iter().any(busy)
    {
        state.cancel_replace();
        return;
    }
    let Some(result) = state.ready.take() else {
        return;
    };
    state.replace = None;
    let request = result.request;
    let Some(doc) = documents.current().filter(|doc| {
        doc.mode == ViewMode::Edit
            && state.mode == NavigationMode::Replace
            && index::current(&request.index.key, doc, &state)
    }) else {
        return;
    };
    let Ok((entity, binding, mut text)) = sources.single_mut() else {
        return;
    };
    if entity != request.source || binding.key.as_ref() != Some(&doc.source_key()) {
        return;
    }
    if busy(&text) || Selection::read(&text) != request.selection || focus.get() != request.focus {
        state.hint = "输入、选区或焦点已变化，替换已取消；草稿保持".into();
        return;
    }
    match result.content {
        Ok(Some(content)) => {
            let selection = content.selection;
            let doc = documents.current_mut().unwrap();
            doc.replace_content(
                content.source,
                content.pages,
                EditContext {
                    before: request.selection,
                    after: selection,
                    ..default()
                },
            );
            doc.status = format!("已替换{}处；可一次撤销，保存后写入文件", content.count);
            if state.in_selection {
                state.scope = content.scope.map(|range| SelectionScope {
                    document: doc.source_key(),
                    revision: doc.revision,
                    range,
                }); // 仅本次受检替换可更新冻结范围；其他编辑或撤销使范围失效。
            }
            state.invalidate();
            state.hint = doc.status.clone();
            text.editor.set_text(doc.source_text());
            text.editor
                .driver(&mut font.context, &mut layout.0)
                .select_byte_range(selection.anchor, selection.focus);
            state.reveal = Some(RevealTarget {
                source: entity,
                key: index::key_for(doc, &state),
                selection: text.editor.raw_selection().text_range(),
            });
            if request.target.is_some() {
                state.pending = Some(NavigationRequest::Next(false));
            }
        }
        Ok(None) => state.hint = "匹配内容已相同；草稿保持".into(),
        Err(error) => state.hint = error,
    }
}
