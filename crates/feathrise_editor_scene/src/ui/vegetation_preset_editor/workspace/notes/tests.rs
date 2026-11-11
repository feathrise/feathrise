//! 备注控件的有限高度、独立滚动和查询边界回归。

use super::*; // 直接锁定生产布局函数与光标滚动系统。

#[test] // 长备注必须留在六行高的内部滚动框，不能撑开中央表单。
fn notes_control_has_a_fixed_scrollable_viewport() {
    // 窗口与全屏共享同一逻辑像素结构。
    let node = notes_control_node(); // 读取生产控件布局，不复制常量。
    assert_eq!(node.height, Val::Px(NOTES_VIEW_HEIGHT)); // 视区高度只由固定行高决定。
    assert_eq!(node.width, Val::Percent(100.0)); // 标签在上，正文独占阅读宽度。
    assert_eq!(node.min_width, Val::Px(0.0)); // 长内容不能反向撑宽窄窗。
    assert_eq!(node.overflow, Overflow::scroll_y()); // 全部正文保留在独立Y轴滚动区。
} // 备注布局回归完成。
