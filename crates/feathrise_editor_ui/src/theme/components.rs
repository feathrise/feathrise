//! 编辑器主题的显式所有权边界；游戏HUD、素材颜色和预览场景不凭颜色相似被接管。
use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default)]
pub struct EditorThemeRoot;

/// 正文画布保持工作区底色；普通精确输入框使用更亮的输入区底色。
#[derive(Component, Clone, Copy, Default)]
pub struct EditorCodeSurface;

/// 数据本身携带的颜色（时间轴、终端ANSI等）不参与工具主题语义换色。
/// 仅作用于当前绘制实体，按钮仍保留原生悬停与按下反馈。
#[derive(Component, Clone, Copy, Default)]
pub struct EditorLiteralColor;
