//! 标准模态遮罩的绘制身份与固定颜色契约，不接管显隐或输入。
use bevy::prelude::*;

/// 标记真正绘制全视口标准DIALOG_BACKDROP的节点，不等同输入捕获。
/// 同时承载关闭Button时，三态底色须保持标准遮罩色，绘制不随悬停或按压增亮。
/// 普通按钮增亮与透明按钮背景补绘均排除此标记，显隐仍遵循原UI规则。
#[derive(Component, Clone, Copy, Default)]
pub struct EditorDialogBackdrop;
