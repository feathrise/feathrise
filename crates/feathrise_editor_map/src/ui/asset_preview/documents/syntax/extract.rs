//! 着色只改Bevy已经提取的原生字形颜色；原有图集、裁剪、选区和光标继续由官方系统绘制。

use super::{constants::EXTRACT_NODES, resources::*};
use bevy::{
    prelude::*,
    render::Extract,
    ui_render::{ExtractedUiItem, ExtractedUiNodes},
};

pub(super) fn begin(nodes: Res<ExtractedUiNodes>, mut state: ResMut<SyntaxExtraction>) {
    // 同一实体内，阴影先提取，正文批次随后按原字形索引追加。
    state.text_start = state.source.as_ref().and_then(|identity| {
        let main = bevy::render::sync_world::MainEntity::from(identity.key.source);
        nodes.uinodes.get(&main).map(|(_, draws)| draws.len())
    }).unwrap_or(0);
}

pub(super) fn invalidate(
    mut commands: Commands,
    mut nodes: ResMut<ExtractedUiNodes>,
    paint: Extract<Res<SyntaxPaint>>,
    palette: Extract<Res<feathrise_editor_ui::EditorPalette>>,
    mut state: ResMut<SyntaxExtraction>,
) {
    let next = paint.visible.as_ref().map(|visible| PaintIdentity {
        key: visible.key.clone(),
        selection: paint.selection.clone(),
        selected_color: paint.selected_color,
        palette: palette.name(),
    });
    if state.source == next {
        return;
    }
    // 异步分词发布、选区变化和切换文档都会使已有正文失效，从原生颜色重新开始。
    for identity in state.source.iter().chain(next.iter()) {
        let main = identity.key.source.into();
        if nodes.changed.insert(main)
            && let Some((_, mut draws)) = nodes.uinodes.remove(&main)
        {
            for (render_entity, _) in draws.drain(..) {
                commands.entity(render_entity).despawn();
            }
        }
    }
    state.source = next;
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
        return;
    };
    let main = bevy::render::sync_world::MainEntity::from(visible.key.source);
    if !nodes.changed.contains(&main) {
        return;
    }
    let Some((_, draws)) = nodes.uinodes.get_mut(&main) else {
        return;
    };
    let palette = palette.map(|color| color.to_linear());
    let mut base = 0;
    let mut colors = visible.colors.iter().peekable();
    for (_, draw) in draws.iter_mut().skip(state.text_start).take(EXTRACT_NODES) {
        if colors.peek().is_none() {
            break;
        }
        let ExtractedUiItem::Glyphs { glyphs } = &mut draw.item else {
            continue;
        };
        let end = base + glyphs.len();
        // 图集换批只拆分绘制项，字形仍按原排版顺序。仅处理可见映射中的索引。
        while colors.peek().is_some_and(|color| (color.glyph as usize) < end) {
            let color = colors.next().unwrap();
            if !paint.selected_color
                || (color.start as usize) >= paint.selection.end
                || (color.end as usize) <= paint.selection.start
            {
                glyphs[color.glyph as usize - base].color = palette[color.kind as usize];
            }
        }
        base = end;
    }
}
