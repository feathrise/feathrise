//! F12主题偏好桥；规范逻辑色兼容旧路径，最终绘制色由编辑器UI库统一拥有。

pub(crate) use feathrise_editor_ui::colors::*;

use bevy::prelude::*;
use feathrise_editor_ui::EditorPalette;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorColorTheme;

pub(crate) fn install(app: &mut App) {
    app.init_resource::<EditorSettings>()
        .init_resource::<EditorPalette>()
        .add_systems(PostUpdate, sync_editor_theme);
}

pub(super) fn sync_editor_theme(settings: Res<EditorSettings>, mut palette: ResMut<EditorPalette>) {
    // 设置在启动加载、按钮选择或外部恢复后都从同一偏好生效；其他设置变化不重建色板。
    if palette.name() == settings.editor_color_theme.as_str() { return; }
    *palette = match settings.editor_color_theme {
        EditorColorTheme::NeutralGray => EditorPalette::neutral_gray(),
        EditorColorTheme::GraphiteBlue => EditorPalette::graphite_blue(),
        EditorColorTheme::WarmCopper => EditorPalette::warm_copper(),
    };
}
