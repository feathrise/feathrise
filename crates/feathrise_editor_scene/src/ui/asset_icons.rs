//! 场景树、固定文件栏和浮动文件窗口共用 GLB 的类型图标。

use bevy::prelude::Color;
use std::path::Path;

pub(super) const GLB: &str = "icons/editor/tree_glb.png";

pub(super) fn tint(icon: &str, fallback: Color) -> Color {
    // GLB 底色和字样已画在文件角标里，保留两者的对比度。
    if icon == GLB { Color::WHITE } else { fallback }
}

pub(super) fn is_glb(path: impl AsRef<Path>) -> bool {
    path.as_ref().extension().is_some_and(|extension| extension.eq_ignore_ascii_case("glb"))
}

pub(super) fn file_row(path: impl AsRef<Path>, directory: bool) -> &'static str {
    if directory {
        "icons/editor/tree_folder.png"
    } else if is_glb(path) {
        GLB
    } else {
        "icons/editor/tree_file.png"
    }
}
