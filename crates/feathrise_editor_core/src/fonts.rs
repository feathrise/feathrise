//! 作者字体统一入口：独立编辑器共享 SDK 默认字体，原一体作者和自定义字体保留原路径语义。

use bevy::{asset::HandleTemplate, prelude::*};

/// 动态控件使用同一默认句柄，避免重新从路径加载并解析第二份中文字体。
pub fn editor_font(assets: &AssetServer, path: &'static str) -> Handle<Font> {
    if uses_builtin(path, super::project_paths::has_bound_editor_resources()) {
        Handle::default()
    } else {
        assets.load(path)
    }
}

/// BSN 模板与动态控件共用选择规则；自定义路径仍交给 Bevy 正常异步加载。
pub fn editor_font_template(path: &'static str) -> HandleTemplate<Font> {
    if uses_builtin(path, super::project_paths::has_bound_editor_resources()) {
        HandleTemplate::Handle(Handle::default())
    } else {
        HandleTemplate::Path(path.into())
    }
}

fn uses_builtin(path: &str, bound: bool) -> bool {
    bound && path == super::FONT_PATH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bound_editor_standard_font_uses_builtin_asset() {
        assert!(uses_builtin(crate::FONT_PATH, true));
        assert!(!uses_builtin(crate::FONT_PATH, false));
        assert!(!uses_builtin("fonts/application.otf", true));
    }
}
