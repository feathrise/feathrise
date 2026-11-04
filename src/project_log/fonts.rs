//! 为内嵌等宽终端登记确定的中文回退；不扫描系统字体，也不替换用户的首选字体。

use super::components::{LogBody, LogDisplay, LogRun};
use bevy::{asset::AssetId, prelude::*, text::FontCx};
use parley::fontique::{FallbackKey, Script};

pub(super) fn fallback(
    fonts: Res<Assets<Font>>,
    mut context: ResMut<FontCx>,
    mut terminal_fonts: Query<&mut TextFont, Or<(With<LogBody>, With<LogDisplay>, With<LogRun>)>>,
) {
    if terminal_fonts.is_empty() { return; }
    // 编辑器已由SDK安装内嵌中文默认字体；只复用其已注册别名，不复制大字体资源。
    let Some(font) = fonts.get(AssetId::<Font>::default()) else { return; };
    let Some(family) = context.collection.family_id(&font.alias) else { return; };
    let mut changed = false;
    for bytes in [*b"Hani", *b"Hira", *b"Kana", *b"Hang", *b"Zyyy", *b"Zinh"] {
        let key = FallbackKey::new(Script::from_bytes(bytes), None);
        if !context.collection.fallback_families(key).any(|known| known == family) {
            changed |= context.collection.append_fallbacks(key, std::iter::once(family));
        }
    }
    if changed {
        // 字体首次登记或集合重建后，让两层一起重新排版，不能留下旧的缺字方框或旧命中位置。
        for mut font in &mut terminal_fonts { font.set_changed(); }
    }
}
