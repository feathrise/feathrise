//! 单一外层滚动容器内的连续虚拟列表几何；只读固定布局值，不扫描目录或创建条目。

use bevy::prelude::*;

/// 完整目录决定滚动高度；实体池大小不参与高度计算，末行之后不额外保留行距。
pub(crate) fn content_height(total: usize, columns: usize, row_height: f32, gap: f32) -> f32 {
    let rows = total.div_ceil(columns.max(1));
    if rows == 0 { 0. } else { rows as f32 * row_height.max(0.) + rows.saturating_sub(1) as f32 * gap.max(0.) }
}

/// 池容量只取决于真实可见高度及两行过扫，不能因为目录有十万项就分配十万个按钮。
pub(crate) fn pool_capacity(viewport_logical_height: f32, columns: usize, pitch: f32) -> usize {
    if !viewport_logical_height.is_finite() || !pitch.is_finite() || pitch <= 0. { return columns.max(1); }
    ((viewport_logical_height.max(0.) / pitch).ceil() as usize).saturating_add(2).saturating_mul(columns.max(1))
}

/// 最后布局中的列表顶端已包含当时的滚动；补入本帧ScrollPosition差量，避免滚轮后错绑一帧。
pub(crate) fn first_visible(
    list: &ComputedNode,
    list_transform: &UiGlobalTransform,
    viewport: &ComputedNode,
    viewport_transform: &UiGlobalTransform,
    scroll: &ScrollPosition,
    columns: usize,
    pitch: f32,
    total: usize,
) -> usize {
    if total == 0 || !pitch.is_finite() || pitch <= 0. { return 0; }
    let inverse = viewport.inverse_scale_factor;
    if !inverse.is_finite() || inverse <= 0. { return 0; }
    let list_top = list_transform.translation.y - list.size().y * 0.5;
    let viewport_top = viewport_transform.translation.y - viewport.size().y * 0.5;
    let applied = viewport.scroll_position.y * inverse;
    let hidden = (viewport_top - list_top) * inverse + scroll.y - applied;
    if !hidden.is_finite() { return 0; }
    let columns = columns.max(1);
    let row = (hidden.max(0.) / pitch).floor() as usize;
    row.min(total.saturating_sub(1) / columns).saturating_mul(columns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_window_uses_full_height_and_only_viewport_controls_pool_capacity() {
        assert_eq!(content_height(192, 1, 24., 2.), 4990.);
        assert_eq!(content_height(193, 3, 24., 2.), 1688.);
        assert_eq!(content_height(0, 1, 24., 2.), 0.);
        assert_eq!(pool_capacity(400., 1, 26.), 18);
        assert_eq!(pool_capacity(400., 3, 26.), 54);
    }

    #[test]
    fn same_frame_scroll_and_dpi_select_the_same_absolute_rows() {
        for scale in [0.6, 1., 1.5, 2.] {
            let viewport = ComputedNode { size: Vec2::new(300., 400.) * scale,
                scroll_position: Vec2::new(0., 240.) * scale, inverse_scale_factor: 1. / scale, ..default() };
            let list = ComputedNode { size: Vec2::new(300., 4990.) * scale, ..default() };
            let viewport_transform = UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::new(150., 200.) * scale));
            let list_transform = UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::new(150., 80. - 240. + 2495.) * scale));
            assert_eq!(first_visible(&list, &list_transform, &viewport, &viewport_transform,
                &ScrollPosition(Vec2::new(0., 500.)), 1, 26., 192), 16);
            assert_eq!(first_visible(&list, &list_transform, &viewport, &viewport_transform,
                &ScrollPosition(Vec2::ZERO), 1, 26., 192), 0);
            assert_eq!(first_visible(&list, &list_transform, &viewport, &viewport_transform,
                &ScrollPosition(Vec2::new(0., 100_000.)), 1, 26., 192), 191);
        }
    }
}
