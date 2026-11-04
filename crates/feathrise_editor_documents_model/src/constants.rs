//! 文本作者工具的固定内存、文字排版和可见节点预算。

pub const MAX_FILE_BYTES: usize = 256 * 1024;
pub const MAX_DOCUMENTS: usize = 8; // 满槽时只淘汰已保存文档，绝不默默丢掉草稿。
pub const SOURCE_PAGE_CHARS: usize = 4096; // 仅后台存储分块，界面始终绑定完整文档。
pub const MAX_EDIT_CHARS: usize = MAX_FILE_BYTES; // 完整原生编辑缓冲；UTF-8保存仍按256 KiB字节检查。
pub const BLOCK_CHARS: usize = 1024;
pub const BLOCK_SPANS: usize = 32;
pub const PREVIEW_PAGE_CHARS: usize = 4096;
pub const PREVIEW_PAGE_BLOCKS: usize = 12;
pub const MAX_PREVIEW_CHARS: usize = 512 * 1024; // 引用链接复用长地址可能膨胀，派生结果也必须独立有界。
pub const MAX_PREVIEW_SPANS: usize = 32 * 1024; // 密集格式的后台结构也有上限，不仅限制可见UI。
pub const PREVIEW_BUILD_BLOCKS: usize = 4; // 每帧最多4个块/132个实体，全文预览不一次构造全部节点。
pub const PREVIEW_RETIRE_NODES: usize = 128; // 旧预览按叶子到根退役，不递归删除完整正文。

pub const MINIMAP_WIDTH: u32 = 96;
pub const MINIMAP_HEIGHT: u32 = 1024; // 每张RGBA轮廓固定384 KiB，不随文档长度扩张纹理。
pub const MINIMAP_GLYPHS_PER_FRAME: usize = 4096; // 只分批读取原生排版缓存，不在主线程遍历全文。
pub const MINIMAP_MAX_GLYPHS: usize = MAX_EDIT_CHARS * 4; // 最多8 MiB轻量矩形快照，覆盖组合字形的余量。
pub const MINIMAP_PANEL_WIDTH: f32 = 108.0;
pub const MINIMAP_MIN_EDITOR_WIDTH: f32 = 360.0; // 低宽度时自动让出正文空间。
pub const MINIMAP_MIN_THUMB_HEIGHT: f32 = 12.0; // 长文档视区很小时仍可抓取。
