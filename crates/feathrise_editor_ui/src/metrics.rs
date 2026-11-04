//! F12各作者页共享的排版尺度；密度来自连续布局与信息分层，不能依靠每页缩小文字。

pub const FONT_BODY: f32 = 13.0;
pub const FONT_CONTROL: f32 = 12.0;
pub const FONT_HINT: f32 = 11.0;
pub const CONTROL_HEIGHT: f32 = 26.0;
pub const ROW_HEIGHT: f32 = 24.0;
pub const SECTION_HEIGHT: f32 = 28.0;
pub const GAP: f32 = 4.0;
pub const RADIUS: f32 = 2.0;

/// 固定列标签提供明确行框，避免原生文字的最小内容高度把同行表单撑成数十像素空白。
/// 中英文混合单位按保守字宽预算换行，正式中文字体仍负责每行的实际字形布局。
pub fn label_height(label: &str, width: f32) -> f32 {
    let lines = label
        .lines()
        .map(|line| {
            let advance = line
                .chars()
                .map(|character| if character.is_ascii() { 0.7 } else { 1.0 })
                .sum::<f32>()
                * FONT_CONTROL;
            (advance / width.max(FONT_CONTROL)).ceil().max(1.0)
        })
        .sum::<f32>()
        .max(1.0);
    lines * 18.0
}

/// 动态预设表单也遵守正文下限，避免各分库继续沿用9～10像素的旧字号。
pub fn font_size(requested: f32) -> f32 {
    requested.clamp(FONT_CONTROL, FONT_BODY)
}
