//! 读取源码版可选的 DX 工具路径。
use std::{fs, io, path::{Path, PathBuf}};

pub(super) fn read_local_path(config: &Path) -> io::Result<Option<PathBuf>> {
    let contents = match fs::read_to_string(config) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io::Error::new(error.kind(), format!("无法读取 DX 配置 {}（{error}）", config.display()))),
    };
    let path = PathBuf::from(contents.trim_start_matches('\u{feff}').trim());
    if !path.is_absolute() {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
            format!("{} 必须填写 DX 可执行文件的绝对路径", config.display())));
    }
    Ok(Some(path))
}
