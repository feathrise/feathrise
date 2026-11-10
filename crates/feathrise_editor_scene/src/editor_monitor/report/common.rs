//! F4动态场景报告共享的场景身份、密度口径和可读格式函数。

use feathrise_scene_content::SceneVegetationLayer; // 正式层提供稳定ID、分辨率与候选格距。

use crate::authoring::{EditorAuthoringSession, EditorSceneDraft}; // 报告只读取已加载草稿和修订缓存。
use crate::resources::SceneEditorState; // F12显隐与未保存状态属于每页共同上下文。

/// 空作者会话仍返回以当前页名开头的明确状态。
pub(super) fn no_draft_report(page_title: &str) -> String {
    format!(
        "{page_title}\n当前没有已加载的编辑器场景草稿。\n进入一张动态场景后，这里会显示该页正式作者数据。"
    ) // F4可以在F12关闭后独立存在，因此不能把关闭F12误报成没有场景。
}

/// 为每页建立一致的场景身份、F12与保存状态开头。
pub(super) fn page_header(
    page_title: &str,                 // 与叶子页签一致的首行名称。
    editor: &SceneEditorState,          // 当前F12显隐和dirty状态。
    session: &EditorAuthoringSession, // 保存进行中状态来自作者会话。
    draft: &EditorSceneDraft,           // 场景名称、稳定ID和运行场景来自权威清单。
) -> Vec<String> {
    let mut lines = Vec::with_capacity(18); // 常规正文一次预留，避免逐行频繁扩容。
    lines.push(page_title.to_owned()); // 所有页面首行保持稳定，截图和测试都可直接辨认。
    lines.push(format!(
        "场景：{} [{}]  |  编辑器：{}  |  场景：{}",
        draft.manifest.display_name,
        draft.scene_id.as_str(),
        if editor.open {
            "已打开"
        } else {
            "已关闭"
        },
        draft.manifest.runtime_scene
    )); // 清楚区分场景显示名、稳定身份与内部场景适配器。
    lines.push(format!(
        "作者状态：修订 {}  |  {}  |  {}",
        session.revision,
        if editor.dirty {
            "有未保存修改"
        } else {
            "已保存"
        },
        if session.saving {
            "保存中"
        } else {
            "未在保存"
        }
    )); // F4关闭F12后也继续回显当前草稿是否安全落盘。
    lines
}

/// 返回受检场景东西宽度与南北长度。
pub(super) fn world_extent(
    world_min: [f32; 2], // 世界X/Z最小角。
    world_max: [f32; 2], // 世界X/Z最大角。
) -> Option<(f32, f32)> {
    let width = world_max[0] - world_min[0]; // X轴对应东西宽度。
    let length = world_max[1] - world_min[1]; // Z轴对应南北长度。
    (width.is_finite() && length.is_finite() && width > 0.0 && length > 0.0)
        .then_some((width, length)) // 只有严格正有限矩形可参与米制统计。
}

/// 按正式动态Mesh散布算法计算全图候选网格列、行和槽位总数。
pub(super) fn candidate_grid(
    world_min: [f32; 2], // 场景最小X/Z端点。
    world_max: [f32; 2], // 场景最大X/Z端点。
    spacing: f32,        // 作者声明的满密度候选间距。
) -> Option<(u32, u32, u64)> {
    let (width, length) = world_extent(world_min, world_max)?; // 坏场景范围不产生伪造候选数。
    if !spacing.is_finite() || spacing <= 0.0 {
        return None; // 非有限或非正间距不能进入除法和整数转换。
    }
    let columns = (width / spacing).ceil().max(1.0) as u32; // 与正式散布X轴格数保持同一转换。
    let rows = (length / spacing).ceil().max(1.0) as u32; // 与正式散布Z轴格数保持同一转换。
    let total = u64::from(columns).saturating_mul(u64::from(rows)); // 极端损坏输入也不允许整数回绕。
    Some((columns, rows, total)) // 最终命中数仍由正式R8概率与稳定随机判定。
}

/// 向植被报告追加同正式R8原子发布的覆盖和概率统计。
pub(super) fn append_density_stats(
    lines: &mut Vec<String>,    // 当前页面输出行。
    draft: &EditorSceneDraft,     // 持有后台编译随修订发布的统计表。
    layer: &SceneVegetationLayer, // 稳定正式输出ID索引对应统计。
) {
    let Some(stats) = draft.vegetation_density_stats.get(&layer.id) else {
        lines.push("密度统计：等待植被后台编译完成；F4不会在主线程扫描整张R8。".to_owned());
        return; // 首载或编译尚未完成时保持真实等待态。
    };
    if stats.total_pixels == 0 {
        lines.push("密度统计：正式R8为空，无法计算覆盖率。".to_owned());
        return; // 防御损坏完成值，正常验证路径不会产生零像素图。
    }
    let coverage = ratio_percent(stats.nonzero_pixels, stats.total_pixels); // 非零像素表达刷印覆盖面积。
    let full_mean = density_mean_percent(stats.density_sum, stats.total_pixels); // 全图平均包含零像素。
    lines.push(format!(
        "密度覆盖：{} / {} 像素 ({coverage:.3}%)  |  全图平均概率 {full_mean:.3}%",
        format_count(stats.nonzero_pixels),
        format_count(stats.total_pixels)
    ));
    let Some(minimum) = stats.min_nonzero else {
        lines.push("非零密度：无  |  当前正式密度图为全零".to_owned());
        return; // 全零图单独显示，避免零被误解为有效最小密度。
    };
    let nonzero_mean = density_mean_percent(stats.density_sum, stats.nonzero_pixels); // 已刷区域显示真实平均强度。
    lines.push(format!(
        "非零密度：平均 {nonzero_mean:.2}%  |  范围 {:.2}%..{:.2}%  |  R8 {}..{}",
        density_percent(minimum),
        density_percent(stats.max_value),
        minimum,
        stats.max_value
    ));
}

/// 把归一化笔刷强度按正式写入入口四舍五入为R8。
pub(super) fn density_byte(strength: f32) -> u8 {
    (strength.clamp(0.0, 1.0) * f32::from(u8::MAX)).round() as u8 // 与密度笔刷目标值换算一致。
}

/// 把一个R8值换算为真实百分比概率。
pub(super) fn density_percent(value: u8) -> f64 {
    f64::from(value) * 100.0 / f64::from(u8::MAX) // R8=13实际约为5.10%。
}

/// 计算若干R8单位在指定像素数上的平均概率百分比。
fn density_mean_percent(sum: u64, pixels: u64) -> f64 {
    if pixels == 0 {
        return 0.0; // 空集合没有概率贡献，避免除零。
    }
    sum as f64 * 100.0 / (pixels as f64 * f64::from(u8::MAX)) // 保留u64精确和后一次转浮点。
}

/// 计算分子占分母的普通百分比。
fn ratio_percent(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0; // 空密度图由上层单独诊断，此处仍保持安全。
    }
    numerator as f64 * 100.0 / denominator as f64 // 覆盖率与密度概率是两种独立百分比。
}

/// 使用英文逗号分隔大整数，便于读取万、百万与千万级场景数据。
pub(super) fn format_count(value: u64) -> String {
    let digits = value.to_string(); // 先取得无符号十进制表示。
    let mut output = String::with_capacity(digits.len() + digits.len() / 3); // 为千分位预留少量容量。
    for (index, character) in digits.chars().enumerate() {
        if index != 0 && (digits.len() - index).is_multiple_of(3) {
            output.push(','); // 从右侧每三位插入一个分隔符。
        }
        output.push(character); // 保持原数字顺序。
    }
    output // 返回适合诊断面板的可读计数。
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::brush_controls::BRUSH_STRENGTH_MIN; // 最低控件值必须与报告换算共测。

    #[test]
    fn four_meter_grid_reports_full_scene_capacity() {
        assert_eq!(
            candidate_grid([0.0, 0.0], [1_001.0, 801.0], 4.0),
            Some((251, 201, 50_451))
        ); // 非整除边缘仍保留最后一列与最后一行候选槽。
    }

    #[test]
    fn minimum_brush_strength_reports_quantized_r8_probability() {
        let value = density_byte(BRUSH_STRENGTH_MIN); // 使用生产最低强度常量。
        assert_eq!(value, 13); // round(255×0.05)必须得到13。
        assert!((density_percent(value) - 5.098_039_215_686_274).abs() < 1.0e-9);
    }

    #[test]
    fn count_formatter_adds_thousands_separators() {
        assert_eq!(format_count(0), "0"); // 零值不增加前导分隔符。
        assert_eq!(format_count(8_192), "8,192"); // 普通数字不带特殊容量语义。
        assert_eq!(format_count(12_345_678), "12,345,678"); // 多个分隔符保持三位分组。
    }
}
