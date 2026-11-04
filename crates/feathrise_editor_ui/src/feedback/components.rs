//! 各编辑器复用的反馈标记；领域系统仍拥有文字和真实阶段，外壳负责统一呈现位置。

use bevy::prelude::*;
use super::types::PanelStatusPhase;

/// 保留原反馈文字实体，正文不占高度，由底栏详情收集完整说明。
#[derive(Component, Clone, Copy, Default)]
pub struct PanelStatusText(pub &'static str);

/// 文档、媒体和预设具有独立身份，切地图不能清掉它们仍有效的诊断。
/// 仅由这些领域的原表现系统清空或替换文字；地图作者反馈不使用此标记。
#[derive(Component, Clone, Copy, Default)]
pub struct GlobalPanelStatus;

/// 固定反馈源的当前简述；空简述解除投影，完整原因仍由同实体的 PanelStatusText 保存。
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PanelStatusState {
    pub phase: PanelStatusPhase,
    pub summary: &'static str,
}
