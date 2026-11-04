//! 从现有Parley布局定位可见cluster，再对齐原生图集字形；不创建输入或第二份排版。

use super::super::{components::DocumentSource, resources::DocumentState, types::ViewMode};
use super::{components::SyntaxPreview, constants::VISIBLE_WORK, resources::*, types::*};
use bevy::{
    prelude::*,
    text::{
        ComputedTextBlock, EditableText, EditableTextGeneration, TextCursorStyle, TextLayoutInfo,
    },
    ui::widget::TextScroll,
};

fn lower_line(count: usize, mut before: impl FnMut(usize) -> bool) -> usize {
    let (mut start, mut end) = (0, count);
    while start < end {
        let mid = (start + end) / 2;
        if before(mid) {
            start = mid + 1;
        } else {
            end = mid;
        }
    }
    start
}

pub(super) fn token_at(tokens: &[Token], byte: usize) -> Option<Kind> {
    tokens
        .get(tokens.partition_point(|token| token.end as usize <= byte))
        .filter(|token| token.start as usize <= byte)
        .map(|token| token.kind)
}

pub(super) fn sync(
    documents: Res<DocumentState>,
    index: Res<SyntaxIndex>,
    mut paint: ResMut<SyntaxPaint>,
    sources: Query<
        (
            Entity,
            &ComputedNode,
            &InheritedVisibility,
            &UiGlobalTransform,
            Option<&CalculatedClip>,
            &ComputedTextBlock,
            Ref<TextLayoutInfo>,
            &TextFont,
            Option<&TextCursorStyle>,
            Option<(
                &DocumentSource,
                &EditableText,
                &EditableTextGeneration,
                &TextScroll,
            )>,
            Option<&SyntaxPreview>,
        ),
        Or<(With<DocumentSource>, With<SyntaxPreview>)>,
    >,
) {
    let current = documents
        .current()
        .filter(|_| documents.visible)
        .and_then(|doc| {
            let index = index
                .result
                .as_ref()
                .filter(|result| result.key.current(doc))?;
            let source = sources.iter().find(|source| {
                source.2.get()
                    && !source.1.is_empty()
                    && match doc.mode {
                        ViewMode::Edit => source.9.is_some_and(|(key, input, _, _)| {
                            key.key.as_ref() == Some(&index.key.document) && !input.is_composing()
                        }),
                        ViewMode::Preview => source.10.is_some_and(|key| key.0 == index.key),
                    }
            })?;
            Some((source, index))
        });
    let Some(((entity, node, _, transform, clip, computed, info, font, cursor, native, _), index)) =
        current
    else {
        paint.visible = None;
        return;
    };
    let layout = if let Some((_, input, _, _)) = native {
        let Some(layout) = input.editor.try_layout() else {
            paint.visible = None;
            return;
        };
        paint.selection = input.editor.raw_selection().text_range();
        paint.selected_color = cursor.is_some_and(|cursor| cursor.selected_text_color.is_some());
        layout
    } else {
        paint.selection = 0..0;
        paint.selected_color = false;
        computed.buffer()
    };
    let (top, bottom) = if let Some((_, _, _, scroll)) = native {
        (scroll.0.y, scroll.0.y + node.content_box().height())
    } else {
        // 预览长Text随ScrollPosition平移；用真实祖先裁剪反推其局部可见范围。
        let origin = transform.affine().transform_point2(node.content_box().min);
        let Some(clip) = clip else {
            paint.visible = None;
            return;
        };
        (
            (clip.clip.min.y - origin.y).max(0.0),
            (clip.clip.max.y - origin.y).min(info.size.y),
        )
    };
    let first = lower_line(layout.len(), |i| {
        layout.get(i).unwrap().metrics().block_max_coord <= top
    });
    let end = lower_line(layout.len(), |i| {
        layout.get(i).unwrap().metrics().block_min_coord < bottom
    });
    let key = GlyphKey {
        source: entity,
        syntax: index.key.clone(),
        generation: native.map(|(_, _, generation, _)| *generation),
        preview_tick: if native.is_none() {
            info.last_changed().get()
        } else {
            0
        },
        lines: first..end,
        extent: info.size,
        width: node.content_box().width(),
        scale: node.inverse_scale_factor(),
        glyphs: info.glyphs.len(),
    };
    if paint
        .visible
        .as_ref()
        .is_some_and(|visible| visible.key == key)
    {
        return;
    }
    paint.visible = None;
    let mut colors = Vec::new();
    let mut budget = VISIBLE_WORK;
    'visible: for line_index in first..end {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let line = layout.get(line_index).unwrap();
        let mut glyph_index = info
            .glyphs
            .partition_point(|glyph| glyph.line_index < line_index);
        let stop = info
            .glyphs
            .partition_point(|glyph| glyph.line_index <= line_index);
        let mut pen = line.metrics().offset + line.metrics().inline_min_coord;
        for run in line.runs() {
            if budget == 0 {
                break 'visible;
            }
            budget -= 1;
            for cluster in run.visual_clusters() {
                if budget == 0 {
                    break 'visible;
                }
                budget -= 1;
                let bytes = cluster.text_range();
                let kind = token_at(&index.tokens, bytes.start);
                for glyph in cluster.glyphs() {
                    if budget == 0 {
                        break 'visible;
                    }
                    budget -= 1;
                    let mut expected = Vec2::new(pen + glyph.x, line.metrics().baseline + glyph.y);
                    pen += glyph.advance;
                    if native.is_none() && font.font_smoothing == bevy::text::FontSmoothing::None {
                        expected = expected.floor();
                    }
                    if glyph_index >= stop {
                        continue;
                    }
                    let positioned = &info.glyphs[glyph_index];
                    let origin = positioned.position
                        - positioned.atlas_info.rect.size() / 2.0
                        - positioned.atlas_info.offset;
                    // 空白和缺失位图可能被Bevy跳过，不能用“第N个字符=第N个字形”猜索引。
                    // 只有与原生笔尖坐标一致才着色；失配退回正文色，每行重新对齐。
                    if (origin - expected).abs().max_element() <= 0.05 {
                        if positioned.atlas_info.is_alpha_mask
                            && let Some(kind) = kind
                        {
                            colors.push(GlyphColor {
                                glyph: glyph_index as u32,
                                start: bytes.start as u32,
                                end: bytes.end as u32,
                                kind,
                            });
                        }
                        glyph_index += 1;
                    }
                }
            }
        }
    }
    paint.visible = Some(VisibleColors { key, colors }); // 当前身份的可见映射一次发布；超预算字形继续用原生正文色。
}
