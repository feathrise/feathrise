//! 跨文件草稿预览、虚拟行与单飞任务的固定预算。

pub const REPLACEMENT_CHARS: usize = 512;
pub const FILE_SLOTS: usize = 16;
pub const DIFF_SLOTS: usize = 32;
pub const FILE_HEIGHT: f32 = 28.0;
pub const DIFF_HEIGHT: f32 = 30.0;
