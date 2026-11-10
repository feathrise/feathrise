//! 高亮只处理有限可见字形；病态超长视觉行超额部分保持原生正文颜色。

// 1080p密集JSON约两万可见字符；cluster和字形各计一次，64 Ki工作覆盖正常整屏。
pub(super) const VISIBLE_WORK: usize = 65_536;
pub(super) const EXTRACT_NODES: usize = 4_096;
