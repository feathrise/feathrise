//! 日志软换行的视觉行索引：增量复用真实字体排版，只向原生阅读层提供原文字节边界。

use super::{components::{LogBody, LogScroll}, resources::{LINE_HEIGHT, ProjectLogState}};
use bevy::{prelude::*, text::{FontCx, LayoutCx, TextBrush, resolve_font_source}};
use parley::{Layout, StyleProperty, WordBreak};
use std::{collections::{HashMap, HashSet}, ops::Range};

#[derive(Clone, Copy, Debug)]
pub(super) struct VisualRow { pub start: u64, pub end: u64 }

#[derive(Default)]
pub(super) struct LogWrapLayout {
    pub rows: Vec<VisualRow>,
    pub width: f32,
    pub revision: Option<u64>,
    /// 切换宽度或换行模式时冻结原文锚点，不能按新行数误跳到其他历史内容。
    anchor: Option<u64>,
}

#[derive(Default)]
pub(super) struct WrapCache {
    font: Option<TextFont>,
    width: f32,
    scale: f32,
    clear_revision: u64,
    published_revision: Option<u64>,
    lines: HashMap<(u64, u64), Vec<Range<usize>>>,
    layout: Layout<TextBrush>,
}

pub(super) fn toggle(state: &mut ProjectLogState) {
    let anchor = if state.wrap && !state.wrap_layout.rows.is_empty() {
        state.wrap_layout.rows.get(state.first_row()).map(|row| row.start)
    } else { state.rows().nth(state.first_row()).map(|(start, _)| start) };
    state.wrap = !state.wrap;
    state.wrap_layout.anchor = anchor;
    if !state.wrap {
        if !state.follow && let Some(anchor) = anchor {
            let first = state.rows().take_while(|(start, _)| *start <= anchor).count().saturating_sub(1);
            state.scroll_back = state.row_count().saturating_sub(first + state.visible_rows);
        }
        state.content_width = 0.;
    }
    state.dirty = true;
}

pub(super) fn sync(
    mut state: ResMut<ProjectLogState>,
    panes: Query<&ComputedNode, With<LogScroll>>,
    fonts: Res<Assets<Font>>,
    text_fonts: Query<Ref<TextFont>, With<LogBody>>,
    mut font_cx: ResMut<FontCx>,
    mut layout_cx: ResMut<LayoutCx>,
    mut cache: Local<WrapCache>,
) {
    if !state.open || !state.wrap { return; }
    let (Ok(node), Ok(font)) = (panes.single(), text_fonts.single()) else { return; };
    if node.is_empty() { return; }
    let inverse_scale = node.inverse_scale_factor();
    let width = (node.content_box().width() * inverse_scale).max(1.);
    let scale = inverse_scale.recip();
    if let bevy::text::FontSource::Handle(handle) = &font.font {
        let Some(asset) = fonts.get(handle.id()) else { return; };
        if asset.alias.is_empty() || font_cx.collection.family_id(&asset.alias).is_none() { return; }
    }
    let Ok(family) = resolve_font_source(&font, &fonts) else { return; };
    // 字体须已由官方字体登记阶段发布；中文回退变化也会标记TextFont需要重排。
    let family = family.into_owned();
    let changed = cache.font.as_ref() != Some(&*font)
        || font.is_changed() || cache.width != width || cache.scale != scale
        || cache.clear_revision != state.clear_revision;
    if changed {
        cache.font = Some((*font).clone());
        cache.width = width;
        cache.scale = scale;
        cache.clear_revision = state.clear_revision;
        cache.lines.clear();
        cache.published_revision = None;
    }
    if cache.published_revision == Some(state.content_revision) && state.wrap_layout.anchor.is_none() { return; }

    // 生产邮箱每帧最多32条，新宽度每帧最多准备64条/每条1KiB；
    // 预备预算必须大于持续新增量，才能在不断输出时也完成宽度重排，而不是永远等待静默。
    // 缓存只存行边界，不为整个256KiB历史创建UI字形或第二份正文。
    let mut budget = super::resources::VIEW_LINES;
    let mut offset = state.next_offset;
    let rows = state.lines.iter().map(|line| ((line.start, 0), line.start, line.text.as_str())).chain(
        state.current_tails().map(move |tail| {
            let start = offset;
            offset += tail.text.len() as u64 + 1;
            // 尾行顺序可能被另一来源的新行整体后移；不变的事件继续复用本地折行边界。
            ((tail.sequence, 1), start, tail.text.as_str())
        }),
    );
    let mut complete = true;
    let mut visual = Vec::new();
    let mut live = HashSet::new();
    for (key, origin, text) in rows {
        live.insert(key);
        if !cache.lines.contains_key(&key) {
            if budget == 0 { complete = false; continue; }
            budget -= 1;
            let mut builder = layout_cx.0.ranged_builder(&mut font_cx, text, scale, true);
            builder.push_default(StyleProperty::FontFamily(family.clone()));
            builder.push_default(StyleProperty::FontSize(13.));
            builder.push_default(StyleProperty::LineHeight(parley::LineHeight::Absolute(LINE_HEIGHT)));
            builder.push_default(StyleProperty::WordBreak(WordBreak::BreakAll));
            builder.push_default(StyleProperty::FontWeight(font.weight.into()));
            builder.push_default(StyleProperty::FontWidth(font.width.into()));
            builder.push_default(StyleProperty::FontStyle(font.style.into()));
            builder.push_default(StyleProperty::FontFeatures((&font.font_features).into()));
            builder.push_default(StyleProperty::FontVariations((&font.font_variations).into()));
            builder.build_into(&mut cache.layout, text);
            cache.layout.break_all_lines(Some(width * scale));
            let mut breaks = cache.layout.lines().map(|line| line.text_range()).collect::<Vec<_>>();
            if breaks.is_empty() { breaks.push(0..text.len()); }
            cache.lines.insert(key, breaks);
        }
        if let Some(breaks) = cache.lines.get(&key) {
            visual.extend(breaks.iter().map(|range| VisualRow {
                start: origin + range.start as u64, end: origin + range.end as u64,
            }));
        }
    }
    cache.lines.retain(|key, _| live.contains(key));
    if !complete { return; }

    let anchor = state.wrap_layout.anchor.take().or_else(||
        state.wrap_layout.rows.get(state.first_row()).map(|row| row.start));
    state.wrap_layout.rows = visual;
    state.wrap_layout.width = width;
    state.wrap_layout.revision = Some(state.content_revision);
    if state.follow { state.scroll_back = 0; }
    else if let Some(anchor) = anchor {
        let first = state.wrap_layout.rows.partition_point(|row| row.start <= anchor).saturating_sub(1);
        state.scroll_back = state.display_row_count().saturating_sub(first + state.visible_rows);
    }
    state.clamp_scroll();
    state.dirty = true;
    cache.published_revision = Some(state.content_revision);
}
