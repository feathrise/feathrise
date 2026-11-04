//! 显式导航一次性露出完整原生选区；同一匹配的光标代次不变时也能返回其位置。

use super::super::{
    components::{DocumentScrollState, DocumentSource},
    resources::DocumentState,
    types::ViewMode,
};
use super::{index, resources::DocumentNavigation};
use bevy::{
    prelude::*,
    text::{EditableText, TextLayoutInfo},
    ui::widget::TextScroll,
};

pub(super) fn sync(
    documents: Res<DocumentState>,
    mut navigation: ResMut<DocumentNavigation>,
    mut sources: Query<(
        Entity,
        &DocumentSource,
        &EditableText,
        &ComputedNode,
        &TextLayoutInfo,
        &mut TextScroll,
        &mut DocumentScrollState,
    )>,
) {
    let Some(target) = navigation.reveal.take() else {
        return;
    };
    let Some(doc) = documents
        .current()
        .filter(|doc| documents.visible && doc.mode == ViewMode::Edit)
    else {
        return;
    };
    if !index::current(&target.key, doc, &navigation) {
        return;
    }
    let Ok((entity, binding, native, node, layout, mut scroll, mut preservation)) =
        sources.single_mut()
    else {
        return;
    };
    if entity != target.source
        || binding.key.as_ref() != Some(&target.key.document)
        || native.is_composing()
        || native.editor.raw_selection().text_range() != target.selection
    {
        return; // 新文字、候选或手工选区已接管时不能用迟到定位覆盖当前视区。
    }
    reveal_selection(node, layout, &mut scroll, &mut preservation);
}

/// 查找、跳行与JSON错误定位共用真实选区边界，保持手动滚动及4逻辑像素边距口径。
pub(in super::super) fn reveal_selection(
    node: &ComputedNode, layout: &TextLayoutInfo, scroll: &mut TextScroll,
    preservation: &mut DocumentScrollState,
) {
    let mut bounds = layout
        .selection_rects
        .iter()
        .copied()
        .reduce(|a, b| Rect {
            min: a.min.min(b.min),
            max: a.max.max(b.max),
        })
        .or_else(|| layout.cursor.map(|(_, rect)| rect));
    let Some(ref mut bounds) = bounds else {
        return;
    };
    let view = node.content_box().size();
    if !view.is_finite() || view.min_element() <= 0.0 {
        return;
    }
    // 原生caret跟随不保证完整选区可见；保留4逻辑像素边距，并以真实全文边界夹取。
    let margin = 4.0 / node.inverse_scale_factor();
    bounds.min -= Vec2::splat(margin);
    bounds.max += Vec2::splat(margin);
    let mut offset = scroll.0;
    for axis in 0..2 {
        if bounds.min[axis] < offset[axis] {
            offset[axis] = bounds.min[axis];
        } else if bounds.max[axis] > offset[axis] + view[axis] {
            offset[axis] = bounds.max[axis] - view[axis];
        }
    }
    let maximum = (layout.size - view).max(Vec2::ZERO);
    let offset = offset.clamp(Vec2::ZERO, maximum);
    if scroll.0 != offset { scroll.0 = offset; }
    preservation.preserved = None; // 后续稳定帧继续保存这里的滚动，不改变光标、选区或文字。
}
