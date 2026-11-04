//! 跨文件搜索的后台、结果内存和固定行池预算，不改变原文档读取上限。

pub const QUERY_CHARS: usize = 128;
pub const FILTER_CHARS: usize = 256;
pub const MAX_FILTERS: usize = 16;
pub const MAX_HITS: usize = 1024;
pub const MAX_ENTRIES: usize = 100_000;
pub const MAX_DEPTH: usize = 32;
pub const MAX_PATH_BYTES: usize = 1024;
pub const ROW_HEIGHT: f32 = 22.0;
pub const VIEW_HEIGHT: f32 = 242.0; // 结果视区最大高度；少量结果按实际行数收缩。
pub const ROW_SLOTS: usize = 16; // 视区最多11行，额外槽覆盖边缘和布局舍入。
