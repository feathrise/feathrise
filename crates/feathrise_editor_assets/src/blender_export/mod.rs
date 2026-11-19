//! 单对象建模和 Blender 绑骨共用的保存导出脚本。

use crate::glb_metadata::ordinary;
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

pub const EXPORTER_FILENAME: &str = "Blender保存时自动导出GLB.py";
const EXPORTER: &[u8] = include_bytes!("Blender保存时自动导出GLB.py");

/// 在后台准备本次 Blender 进程读取的脚本。调用方负责独占目录及其清理。
pub fn prepare(directory: &Path, override_directory: Option<OsString>) -> Result<PathBuf, String> {
    if let Some(override_directory) = override_directory {
        let path = PathBuf::from(override_directory).join(EXPORTER_FILENAME);
        if path.is_absolute()
            && ordinary(&path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0)
        {
            return Ok(path);
        }
        return Err(format!("Blender 导出器覆盖需提供绝对目录和非空脚本 {}", path.display()));
    }

    // 编译时携带同一份源码，会话释放时保持脚本原字节和 Blender 用户配置。
    let path = directory.join(EXPORTER_FILENAME);
    fs::write(&path, EXPORTER).map_err(|error| format!("无法写入内置 Blender 导出器（{error}）"))?;
    Ok(path)
}
