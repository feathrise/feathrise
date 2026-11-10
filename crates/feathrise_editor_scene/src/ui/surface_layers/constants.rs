//! 地表作者层沿用植被六槽分页和紧凑行高。

use crate::ui::list_layout::FixedRowPoolLayout; // 复用所有固定行池的真实内容高度规则。

pub(super) const ROW_COUNT: usize = 6; // 单页容量只限制UI实体数，不限制地表层总数。
pub(super) const LAYOUT: FixedRowPoolLayout = FixedRowPoolLayout {
    capacity: ROW_COUNT, // 分页步长与可见槽数保持一致。
    row_height: 20.0,    // 与植被作者层采用同一行高。
    row_gap: 2.0,        // 真实条目间留两像素间距。
    padding_y: 2.0,      // 非空列表上下各保留两像素。
}; // 零项容器自动收缩为零。
