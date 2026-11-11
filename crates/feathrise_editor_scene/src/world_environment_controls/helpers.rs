//! F12 世界环境滑块端点映射、草地风摆与格式化纯函数。

use feathrise_environment::vegetation::blade::GPU_GRASS_MAX_WIND_TIP_STRENGTH; // 草地风摆沿用运行时GPU安全上限。

pub(super) const WORLD_SLIDER_TRACK_WIDTH: f32 = 260.0;
pub(super) const WORLD_SLIDER_THUMB_WIDTH: f32 = 14.0;
pub(super) const WORLD_SLIDER_TRAVEL: f32 = WORLD_SLIDER_TRACK_WIDTH - WORLD_SLIDER_THUMB_WIDTH;
pub(super) const WEATHER_MODE_COUNT: usize = 6;
pub(super) const GRASS_WIND_MAX_CENTIMETERS: f32 = GPU_GRASS_MAX_WIND_TIP_STRENGTH * 100.0; // 原F1控件的零至三十五厘米范围不变。

pub(super) fn normalized_ratio(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub(super) fn centered_cursor_x_to_ratio(cursor_x: f32) -> f32 {
    let cursor_px = normalized_ratio(cursor_x + 0.5) * WORLD_SLIDER_TRACK_WIDTH;
    normalized_ratio((cursor_px - WORLD_SLIDER_THUMB_WIDTH * 0.5) / WORLD_SLIDER_TRAVEL)
}

pub(super) fn hour_from_ratio(ratio: f32) -> f32 {
    (normalized_ratio(ratio) * 24.0 * 60.0).round() / 60.0
}

pub(super) fn month_from_ratio(ratio: f32) -> u8 {
    (1.0 + normalized_ratio(ratio) * 11.0)
        .round()
        .clamp(1.0, 12.0) as u8
}

pub(super) fn weather_mode_from_ratio(ratio: f32) -> usize {
    let last = WEATHER_MODE_COUNT - 1;
    (normalized_ratio(ratio) * last as f32).round() as usize
}

pub(super) fn hour_thumb_left(hour: f32) -> f32 {
    let safe = if hour.is_finite() {
        hour.clamp(0.0, 24.0)
    } else {
        0.0
    };
    safe / 24.0 * WORLD_SLIDER_TRAVEL
}

pub(super) fn month_thumb_left(month: u8) -> f32 {
    f32::from(month.clamp(1, 12) - 1) / 11.0 * WORLD_SLIDER_TRAVEL
}

pub(super) fn weather_mode_thumb_left(index: usize) -> f32 {
    index.min(WEATHER_MODE_COUNT - 1) as f32 / (WEATHER_MODE_COUNT - 1) as f32 * WORLD_SLIDER_TRAVEL
}

pub(super) fn weather_preview_thumb_left(preview: f32) -> f32 {
    normalized_ratio(preview) * WORLD_SLIDER_TRAVEL
}

/// 晴天基准摆幅以厘米步进写入原有全局设置，实际草尖幅度由天气桥派生。
pub(super) fn grass_wind_strength_from_ratio(ratio: f32) -> f32 {
    (normalized_ratio(ratio) * GRASS_WIND_MAX_CENTIMETERS).round() / 100.0 // 保留原F1的一厘米吸附精度。
}

pub(super) fn grass_wind_thumb_left(strength: f32) -> f32 {
    let safe_strength = if strength.is_finite() {
        strength.clamp(0.0, GPU_GRASS_MAX_WIND_TIP_STRENGTH) // 损坏设置不传播到布局。
    } else {
        0.0 // 无效设置时手柄回到左端。
    };
    safe_strength / GPU_GRASS_MAX_WIND_TIP_STRENGTH * WORLD_SLIDER_TRAVEL // 复用右栏260px滑块行程。
}

pub(super) fn format_grass_wind_strength(strength: f32) -> String {
    let centimeters =
        grass_wind_thumb_left(strength) / WORLD_SLIDER_TRAVEL * GRASS_WIND_MAX_CENTIMETERS; // 标签与手柄使用相同钳制后的设置值。
    format!("{centimeters:.0} cm") // 与原F1标签保持相同的整数厘米单位。
}

pub(super) fn format_hour(hour: f32) -> String {
    let safe = if hour.is_finite() {
        hour.clamp(0.0, 24.0)
    } else {
        0.0
    };
    let minutes = (safe * 60.0).round() as u32;
    if minutes >= 24 * 60 {
        "24:00".to_owned()
    } else {
        format!("{:02}:{:02}", minutes / 60, minutes % 60)
    }
}

pub(super) fn format_game_seconds(seconds: f32) -> String {
    let total = if seconds.is_finite() {
        seconds.max(0.0).round() as u64
    } else {
        0
    };
    if total >= 3_600 {
        format!(
            "{:02}:{:02}:{:02}",
            total / 3_600,
            total % 3_600 / 60,
            total % 60
        )
    } else {
        format!("{:02}:{:02}", total / 60, total % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_month_and_weather_sliders_reach_all_endpoints() {
        assert_eq!(hour_from_ratio(0.0), 0.0);
        assert_eq!(hour_from_ratio(0.5), 12.0);
        assert_eq!(hour_from_ratio(1.0), 24.0);
        assert_eq!(month_from_ratio(0.0), 1);
        assert_eq!(month_from_ratio(1.0), 12);
        assert_eq!(weather_mode_from_ratio(0.0), 0);
        assert_eq!(weather_mode_from_ratio(1.0), 5);
        assert_eq!(centered_cursor_x_to_ratio(-0.5), 0.0);
        assert_eq!(centered_cursor_x_to_ratio(0.5), 1.0);
    }

    #[test] // 迁移后草地风摆仍在原有安全端点内按一厘米吸附。
    fn grass_wind_slider_keeps_f1_range_and_steps() {
        assert_eq!(grass_wind_strength_from_ratio(0.0), 0.0); // 左端关闭草地风摆。
        assert_eq!(grass_wind_strength_from_ratio(0.5), 0.18); // 半轨四舍五入到十八厘米。
        assert_eq!(
            grass_wind_strength_from_ratio(1.0),
            GPU_GRASS_MAX_WIND_TIP_STRENGTH
        ); // 右端达到渲染端安全上限。
        assert_eq!(grass_wind_strength_from_ratio(f32::NAN), 0.0); // 无效输入安全回退。
        assert_eq!(
            grass_wind_thumb_left(GPU_GRASS_MAX_WIND_TIP_STRENGTH),
            WORLD_SLIDER_TRAVEL
        ); // 最右端手柄完全可达。
        assert_eq!(format_grass_wind_strength(0.16), "16 cm"); // 保留原来的厘米标签。
    }
}
