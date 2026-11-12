//! 资产目录与作者工具共享的有界 GLB 首块读取，只读取头和 JSON。

use serde_json::Value;
use std::{
    fs::{self, File, Metadata},
    io::Read,
    path::Path,
};

pub const MAX_MODEL_BYTES: u64 = 256 * 1024 * 1024;
const MAX_JSON: usize = 16 * 1024 * 1024;

pub fn ordinary(path: &Path) -> Result<Metadata, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if linked {
        return Err(format!("绑骨资产路径不能经过链接：{}", path.display()));
    }
    Ok(metadata)
}

pub fn open_glb(path: &Path, max_bytes: u64) -> Result<File, String> {
    let metadata = ordinary(path)?;
    if !metadata.is_file() || !(20..=max_bytes).contains(&metadata.len()) {
        let limit = if max_bytes == MAX_MODEL_BYTES {
            "256 MiB 内的"
        } else {
            "GLB v2 长度范围内的"
        };
        return Err(format!("GLB 必须是{limit}有效普通文件：{}", path.display()));
    }
    File::open(path).map_err(|error| format!("打开 GLB {} 失败：{error}", path.display()))
}

pub fn read_directory(path: &Path) -> Result<Value, String> {
    // 目录只读头和 JSON，BIN 块不需要沿用绑骨的整文件处理预算。
    read_json(path, u64::from(u32::MAX))
}

pub fn read_json(path: &Path, max_bytes: u64) -> Result<Value, String> {
    let mut file = open_glb(path, max_bytes)?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    let mut header = [0; 20];
    file.read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    let field = |offset| u32::from_le_bytes(header[offset..offset + 4].try_into().unwrap());
    let size = field(12) as usize;
    if &header[..4] != b"glTF"
        || field(4) != 2
        || u64::from(field(8)) != length
        || length > max_bytes
        || &header[16..] != b"JSON"
        || size == 0
        || size > MAX_JSON
        || 20 + size as u64 > length
    {
        return Err(format!("无效或超出元数据预算的 GLB：{}", path.display()));
    }
    // 动作目录只读第一个 JSON chunk，网格、权重和纹理二进制不进入 Rust 内存。
    let mut bytes = vec![0; size];
    file.read_exact(&mut bytes)
        .map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| format!("解析 GLB 目录失败：{error}"))
}
