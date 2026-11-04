//! 地图、Scene0、资产、植被和水域固定行池共享的动态高度布局契约。

use bevy::prelude::*; // Node、Display、UiRect与像素Val。

/// 场景网格与子地图的连续滚动入口，滚轮先交给列表，到达首尾才交还场景树。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneObjectListScroll;

/// 八行视区加首尾缓冲共十个实体；池大小不是作者数量上限。
pub(crate) const SCENE_OBJECT_LIST_ROWS: usize = 10;
pub(crate) const SCENE_OBJECT_ROW_PITCH: f32 = 27.0;

/// 只按数量与滚动位置计算可见窗口，不复制或遍历完整作者集合。
pub(crate) fn fit_scene_object_list(
    count: usize, reset: bool, reveal: Option<usize>,
    viewport: &mut Node, content: &mut Node, scroll: &mut ScrollPosition,
) -> usize {
    let height = |rows: usize| (rows as f32 * SCENE_OBJECT_ROW_PITCH - 3.0).max(0.0);
    let viewport_height = height(count.min(SCENE_OBJECT_LIST_ROWS - 2));
    viewport.display = if count == 0 { Display::None } else { Display::Flex };
    viewport.height = px(viewport_height);
    content.height = px(height(count));
    if reset { scroll.y = 0.0; }
    if let Some(index) = reveal.filter(|index| *index < count) {
        let top = index as f32 * SCENE_OBJECT_ROW_PITCH;
        if top < scroll.y { scroll.y = top; }
        else if top + 24.0 > scroll.y + viewport_height {
            scroll.y = top + 24.0 - viewport_height;
        }
    }
    scroll.y = scroll.y.clamp(0.0, (height(count) - viewport_height).max(0.0));
    ((scroll.y / SCENE_OBJECT_ROW_PITCH).floor() as usize).saturating_sub(1)
}

/// 固定实体池的视觉尺寸；容量限制运行时高度但不改变实体池本身。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FixedRowPoolLayout {
    pub(crate) capacity: usize, // 单页固定槽位上限。
    pub(crate) row_height: f32, // 每个可点击行的明确像素高度。
    pub(crate) row_gap: f32,    // 相邻真实行之间的像素间距。
    pub(crate) padding_y: f32,  // 非空列表顶部与底部各自的像素内边距。
}

impl FixedRowPoolLayout {
    /// 钳制当前页真实可见行数到实体池容量。
    pub(crate) const fn visible_rows(self, requested: usize) -> usize {
        if requested < self.capacity {
            requested
        } else {
            self.capacity
        }
    }

    /// 返回零项为零、非空按行数线性增长且满页封顶的高度。
    pub(crate) fn height(self, requested: usize) -> f32 {
        let rows = self.visible_rows(requested);
        if rows == 0 {
            return 0.0; // 空列表不保留行距或内边距。
        }
        rows as f32 * self.row_height
            + rows.saturating_sub(1) as f32 * self.row_gap
            + self.padding_y * 2.0
    }

    /// 把固定行池容器原位调整到当前页真实高度，零项完全退出父布局。
    pub(crate) fn fit(self, node: &mut Node, requested: usize) {
        let rows = self.visible_rows(requested);
        node.display = if rows == 0 {
            Display::None // 零项不制造父级row_gap或大块空白。
        } else {
            Display::Flex
        };
        node.height = px(self.height(rows)); // 末页紧跟最后一条真实对象行。
        node.padding = if rows == 0 {
            UiRect::all(px(0)) // 隐藏时清掉历史非空内边距。
        } else {
            UiRect::vertical(px(self.padding_y))
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 用真实Node锁定五类列表共同的布局行为。

    #[test]
    fn continuous_scene_lists_scroll_past_eight_and_reveal_selection_without_truncation() {
        let (mut viewport, mut content, mut scroll) = (Node::default(), Node::default(), ScrollPosition::default());
        assert_eq!(fit_scene_object_list(1000, true, None, &mut viewport, &mut content, &mut scroll), 0);
        assert_eq!(content.height, px(26997.0));
        assert_eq!(viewport.height, px(213.0));
        scroll.y = 27.0 * 500.0 + 9.0;
        assert_eq!(fit_scene_object_list(1000, false, None, &mut viewport, &mut content, &mut scroll), 499);
        assert_eq!(scroll.y, 13509.0); // 普通滚动不能被原先的选择拉回。
        let start = fit_scene_object_list(1000, false, Some(999), &mut viewport, &mut content, &mut scroll);
        assert!(start <= 999 && start + SCENE_OBJECT_LIST_ROWS > 999);
        assert_eq!(scroll.y, 26784.0);
        assert_eq!(fit_scene_object_list(2, false, None, &mut viewport, &mut content, &mut scroll), 0);
        assert_eq!(scroll.y, 0.0); // 删除/撤销缩短列表后不会留在空白尾部。
        assert_eq!(fit_scene_object_list(0, true, None, &mut viewport, &mut content, &mut scroll), 0);
        assert_eq!(viewport.display, Display::None);
    }

    const LAYOUT: FixedRowPoolLayout = FixedRowPoolLayout {
        capacity: 6,
        row_height: 20.0,
        row_gap: 2.0,
        padding_y: 2.0,
    };

    #[test]
    fn fixed_row_pool_height_covers_zero_one_last_page_full_and_overflow() {
        assert_eq!(LAYOUT.height(0), 0.0); // 零项无任何高度。
        assert_eq!(LAYOUT.height(1), 24.0); // 一项包含行本体与上下内边距。
        assert_eq!(LAYOUT.height(2), 46.0); // 末页只增加一行与一个行距。
        assert_eq!(LAYOUT.height(6), 134.0); // 满页达到固定最大高度。
        assert_eq!(LAYOUT.height(usize::MAX), 134.0); // 越界输入安全钳制到容量。
    }

    #[test]
    fn fixed_row_pool_leaves_layout_only_when_empty() {
        let mut node = Node::default();
        LAYOUT.fit(&mut node, 0);
        assert_eq!(node.display, Display::None);
        assert_eq!(node.height, px(0));
        LAYOUT.fit(&mut node, 2);
        assert_eq!(node.display, Display::Flex);
        assert_eq!(node.height, px(46));
    }
}
