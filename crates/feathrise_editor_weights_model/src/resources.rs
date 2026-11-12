//! 权重右栏和资产入口的静态场景工厂。

use bevy::prelude::{Resource, Scene};

/// 场景宿主在原右栏实体上组合布局。具体 BSN 类型留在布局实现库。
#[derive(Resource, Clone, Copy)]
pub struct WeightPanelScenes {
    pub panel: fn() -> Box<dyn Scene>,
    pub entry: fn() -> Box<dyn Scene>,
}
