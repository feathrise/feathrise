//! 原生LOD连续列表的有界视窗计算；作者条目数量不改变UI实体数，也不生成分页。

/// 六十四行包含上下过扫；池大小仅限制同屏工作量，不截断作者LOD配置。
pub(super) const ROW_POOL_SIZE: usize = 64;
pub(super) const ROW_HEIGHT: f32 = 25.0;
pub(super) const ROW_GAP: f32 = 3.0;

/// 极高逻辑视区适当增加行距，保证可见条目仍完整落在固定池内；普通窗口保持原25像素行高。
pub(super) fn row_pitch(viewport_height: f32) -> f32 {
    (ROW_HEIGHT + ROW_GAP).max((viewport_height / (ROW_POOL_SIZE - 2) as f32).ceil())
}

pub(super) fn content_height(count: usize, pitch: f32) -> f32 {
    (count as f32 * pitch - ROW_GAP).max(0.0)
}

pub(super) fn first_row(count: usize, offset: f32, pitch: f32) -> usize {
    ((offset.max(0.0) / pitch).floor() as usize)
        .saturating_sub(1)
        .min(count.saturating_sub(1))
}

/// 只在真实选层或列表结构改变时调用；作者普通滚动不会被旧选择拉回。
pub(super) fn reveal_scroll(
    current: f32,
    list_origin: f32,
    index: usize,
    pitch: f32,
    viewport_height: f32,
) -> f32 {
    let top = list_origin + index as f32 * pitch;
    let bottom = top + ROW_HEIGHT;
    if top < current {
        top.max(0.0)
    } else if bottom > current + viewport_height {
        (bottom - viewport_height).max(0.0)
    } else {
        current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_lod_window_keeps_normal_rows_compact_and_covers_extreme_viewports() {
        for height in [200.0, 700.0, 1700.0] {
            assert_eq!(row_pitch(height), 28.0);
        }
        for height in [200.0, 700.0, 1700.0, 4096.0, 32768.0] {
            let pitch = row_pitch(height);
            for offset in [0.0, 123.5, 14000.0] {
                let start = first_row(10000, offset, pitch);
                assert!(start as f32 * pitch <= offset);
                assert!((start + ROW_POOL_SIZE) as f32 * pitch >= offset + height);
            }
        }
        assert_eq!(content_height(0, 28.0), 0.0);
        assert_eq!(content_height(1, 28.0), 25.0);
        assert_eq!(content_height(6, 28.0), 165.0);
        assert_eq!(content_height(1000, 28.0), 27997.0);
    }

    #[test]
    fn continuous_lod_reveal_only_moves_by_the_needed_distance() {
        assert_eq!(reveal_scroll(0.0, 120.0, 0, 28.0, 500.0), 0.0);
        assert_eq!(reveal_scroll(0.0, 120.0, 999, 28.0, 500.0), 27617.0);
        assert_eq!(reveal_scroll(27617.0, 120.0, 0, 28.0, 500.0), 120.0);
        assert_eq!(first_row(2, 100000.0, 28.0), 1);
    }
}
