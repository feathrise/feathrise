//! 积水笔迹纯CPU核心：仅扫描线段扩展包围盒，每像素一次，不分配逐像素撤销树。
use super::types::Sample;
use bevy::prelude::*; // 世界XZ与二维线段距离。 // 已受理尾段的冻结参数。
pub(super) fn paint(
    pixels: &mut [u8],
    resolution: [u32; 2],
    min: [f32; 2],
    max: [f32; 2],
    from: Vec3,
    sample: Sample,
) -> bool {
    // 只在有界后台任务执行。
    if !sample.radius.is_finite()
        || sample.radius <= 0.0
        || !sample.amount.is_finite()
        || sample.amount <= 0.0
    {
        return false;
    } // 非法输入保持原画布。
    let world_min = Vec2::from_array(min); // 首纹素世界坐标。
    let span = Vec2::from_array(max) - world_min; // 非正方场景两个轴分别换算。
    let from = Vec2::new(from.x, from.z); // 积水作者图使用世界XZ。
    let to = Vec2::new(sample.world.x, sample.world.z); // 高度不影响潜力分布。
    if !span.is_finite() || span.min_element() <= 0.0 || !from.is_finite() || !to.is_finite() {
        return false;
    } // 拒绝坏场景或命中。
    let step = span / Vec2::new((resolution[0] - 1) as f32, (resolution[1] - 1) as f32); // 端点对齐。
    let lower = ((from.min(to) - Vec2::splat(sample.radius) - world_min) / step)
        .floor()
        .max(Vec2::ZERO); // 裁剪局部起点。
    let upper = ((from.max(to) + Vec2::splat(sample.radius) - world_min) / step)
        .ceil()
        .min(Vec2::new(
            (resolution[0] - 1) as f32,
            (resolution[1] - 1) as f32,
        )); // 裁剪局部终点。
    if lower.x > upper.x || lower.y > upper.y {
        return false;
    } // 圆刷完全离图。
    let segment = to - from; // 两个后台块之间连续连接。
    let denominator = segment.length_squared().max(f32::EPSILON); // 单击退化圆刷，不除零。
    let mut changed = false; // 只有真实字节变化才创建历史。
    for z in lower.y as u32..=upper.y as u32 {
        // 每个候选纹素只访问一次。
        for x in lower.x as u32..=upper.x as u32 {
            // 无逐像素BTreeMap或临时分配。
            let point = world_min + Vec2::new(x as f32, z as f32) * step; // 当前纹素世界位置。
            let projection = ((point - from).dot(segment) / denominator).clamp(0.0, 1.0); // 最近线段投影。
            let distance = point.distance(from + segment * projection); // 圆头连续笔迹精确距离。
            if distance >= sample.radius {
                continue;
            } // 扩展包围盒外角不着色。
            let index = (z * resolution[0] + x) as usize; // 行优先R8地址。
            let target = if sample.erase { 0.0 } else { 255.0 }; // 擦除禁止积水，绘制增加潜力。
            let factor = (sample.amount * (1.0 - distance / sample.radius)).clamp(0.0, 1.0); // 公共线性柔边语义。
            let next =
                (pixels[index] as f32 + (target - pixels[index] as f32) * factor).round() as u8; // 保留独立R8精度。
            changed |= next != pixels[index]; // 零变化不发布新作者版本。
            pixels[index] = next; // 当前后台独占写侧。
        } // 完成一行。
    } // 完成局部矩形。
    changed // 主线程只需常数大小变化标志。
} // 结束无额外像素内存笔刷。
