//! 检索与可见标记的独立预算；不限制文档正文或匹配总数。

pub const QUERY_CHARS: usize = 128;
pub const REPLACEMENT_CHARS: usize = 512;
pub const REGEX_BYTES: usize = 2 * 1024 * 1024; // 后台字面引擎及DFA各2 MiB，编译失败明确提示。
pub const ROW_WIDTH: usize = 1024;
pub const MAX_VISIBLE_ROWS: usize = 512;
pub const ROWS_PER_FRAME: usize = 32;
pub const CLUSTERS_PER_FRAME: usize = 8192;
pub const OVERVIEW_PER_FRAME: usize = 1024;
pub const OVERVIEW_HEIGHT: usize = 1024;
pub const GUTTER_WIDTH: f32 = 48.0;
