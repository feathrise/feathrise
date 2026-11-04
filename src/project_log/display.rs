//! 终端的富色字形层：复用可见页和原生字体，不复制命中、选区或完整日志布局。

use super::{components::{LogDisplay, LogRun}, resources::LINE_HEIGHT, scene::TERMINAL_TEXT, types::{LogSpan, LogStyle}};
use bevy::prelude::*;
use bevy::text::LineHeight;
use std::hash::{Hash, Hasher};

pub(super) fn sync(
    commands: &mut Commands,
    entity: Entity,
    marker: &mut LogDisplay,
    children: Option<&Children>,
    existing: &mut Query<(&mut TextSpan, &mut TextColor, &mut TextFont), (With<LogRun>, Without<LogDisplay>)>,
    font: &TextFont,
    body: &str,
    spans: &[LogSpan],
) {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    body.hash(&mut hash);
    for span in spans {
        (span.start, span.end, span.style.fg, span.style.bold).hash(&mut hash);
    }
    let fingerprint = hash.finish();
    if marker.fingerprint == Some(fingerprint) { return; }
    marker.fingerprint = Some(fingerprint);

    // 只处理当前至多64行，缓存中的颜色区间另受256KiB总预算约束。
    // 复用已有TextSpan，连续进度回写不反复销毁整棵可见字形树。
    let mut runs = Vec::new();
    let mut cursor = 0;
    for span in spans {
        if span.start < cursor || span.end > body.len() || span.start >= span.end
            || !body.is_char_boundary(span.start) || !body.is_char_boundary(span.end) { continue; }
        if cursor < span.start { runs.push((&body[cursor..span.start], LogStyle::default())); }
        runs.push((&body[span.start..span.end], span.style));
        cursor = span.end;
    }
    if cursor < body.len() { runs.push((&body[cursor..], LogStyle::default())); }
    let children = children.map_or(&[][..], |children| children.as_ref());
    for (index, (value, style)) in runs.iter().enumerate() {
        let color = terminal_color(*style);
        if let Some(child) = children.get(index) {
            if let Ok((mut text, mut text_color, mut text_font)) = existing.get_mut(*child) {
                if text.0 != *value { text.0.clear(); text.0.push_str(value); }
                if text_color.0 != color { text_color.0 = color; }
                if *text_font != *font { *text_font = font.clone(); }
            }
        } else {
            // TextSpan不继承父段的字体与行高；必须显式与命中层相同，不能落回20px默认值。
            commands.entity(entity).with_child((LogRun, TextSpan::new(*value), TextColor(color),
                font.clone(), LineHeight::Px(LINE_HEIGHT), Pickable::IGNORE));
        }
    }
    for child in children.iter().skip(runs.len()) { commands.entity(*child).despawn(); }
}

fn terminal_color(style: LogStyle) -> Color {
    let mut rgb = style.fg.unwrap_or([204; 3]);
    if style.bold {
        // SGR粗体采用终端常见的高亮强度，不改变字宽；绘制字形与原生选区始终逐字对齐。
        for value in &mut rgb { *value = (*value as f32 * 1.12).min(255.) as u8; }
    }
    if style == LogStyle::default() { TERMINAL_TEXT } else { Color::srgb_u8(rgb[0], rgb[1], rgb[2]) }
}
