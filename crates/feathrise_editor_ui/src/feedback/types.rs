//! 共享底栏的只读阶段与投影顺序契约，不拥有任务或操作回执。

use bevy::prelude::SystemSet;

/// 宿主等非地图领域显式提供真实阶段，底栏不得从说明文字猜测成功或失败。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PanelStatusPhase {
    #[default]
    Info,
    Working,
    Failed,
}

/// PostUpdate 中先完成实际生命周期投影，再由统一底栏收集，避免一帧内两处状态不同。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PanelStatusProjection;
