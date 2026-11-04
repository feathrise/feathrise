//! 着色只改Bevy已经提取的原生字形颜色；原有图集、裁剪、选区和光标继续由官方系统绘制。

use super::{constants::EXTRACT_NODES, resources::*};
use bevy::{
    prelude::*,
    render::Extract,
    ui_render::{ExtractedUiItem, ExtractedUiNodes},
};

pub(super) fn begin(nodes: Res<ExtractedUiNodes>, mut state: ResMut<SyntaxExtraction>) {
    state.text_start = nodes.uinodes.len();
}

pub(super) fn color(
    mut nodes: ResMut<ExtractedUiNodes>,
    paint: Extract<Res<SyntaxPaint>>,
    palette: Extract<Res<feathrise_editor_ui::EditorPalette>>,
    mut state: ResMut<SyntaxExtraction>,
) {
    apply_palette(&mut nodes, &paint, &mut state, palette.syntax_colors());
}

#[cfg(test)]
pub(super) fn apply(
    nodes: &mut ExtractedUiNodes,
    paint: &SyntaxPaint,
    state: &mut SyntaxExtraction,
) {
    apply_palette(nodes, paint, state, feathrise_editor_ui::EditorPalette::warm_copper().syntax_colors());
}

fn apply_palette(nodes: &mut ExtractedUiNodes, paint: &SyntaxPaint, state: &mut SyntaxExtraction,
    palette: [Color; 6]) {
    let Some(visible) = &paint.visible else {
        state.cached = None;
        state.cursor = 0;
        return;
    };
    let start = state.text_start;
    let text = &nodes.uinodes[start..];
    let first = |i: usize| {
        text.get(i).is_some_and(|node| {
            node.main_entity.id() == visible.key.source
                && matches!(node.item, ExtractedUiItem::Glyphs { .. })
                && (i == 0 || text[i - 1].main_entity != node.main_entity)
        })
    };
    let cached = state.cached.filter(|i| first(*i));
    let found = cached.or_else(|| {
        let begin = state.cursor.min(text.len());
        let end = (begin + EXTRACT_NODES).min(text.len());
        let found = (begin..end).find(|i| first(*i));
        state.cursor = if end == text.len() { 0 } else { end };
        found
    });
    state.cached = found;
    let Some(found) = found else {
        return;
    };
    let ExtractedUiItem::Glyphs { range } = &text[found].item else {
        return;
    };
    let base = range.start;
    // extract_text_sections按单实体连续提取全部字形，图集换批只切节点，不改变字形索引。
    // 首批定位采用4096节点/帧和稳定序号缓存；不增加一次全UI或完整字形扫描。
    let palette = palette.map(|color| color.to_linear());
    for color in &visible.colors {
        if paint.selected_color
            && (color.start as usize) < paint.selection.end
            && (color.end as usize) > paint.selection.start
        {
            continue;
        }
        if let Some(glyph) = nodes.glyphs.get_mut(base + color.glyph as usize) {
            glyph.color = palette[color.kind as usize];
        }
    }
}
