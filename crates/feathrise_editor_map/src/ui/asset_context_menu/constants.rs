//! 路径复制仅处理短资产身份，不扫描磁盘或复制资源正文。
pub(super) const MAX_COPY_PATH_BYTES: usize = 4096; // 根和相对路径各至多4KiB，输出保守8KiB。
