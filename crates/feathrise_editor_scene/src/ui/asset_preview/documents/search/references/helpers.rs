//! 固定扫描范围与路径字段约定；路径必须与assets目录下的物理文件精确一致。

use super::super::constants::MAX_PATH_BYTES;
use std::path::{Component, Path};

pub(in super::super) fn valid_target(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && !path.contains(['\\', '#', ':'])
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

pub(in super::super) fn in_scope(path: &str) -> bool {
    path == "config" || path.starts_with("config/") || path == "scenes" || path.starts_with("scenes/")
}

pub(in super::super) fn accepts(path: &str) -> bool {
    in_scope(path)
        && Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
}

pub(super) fn path_field(key: &str) -> bool {
    key == "path" || key.ends_with("_path") || key.ends_with("_paths")
}
