//! 蒙皮权重右栏与资产入口的布局，由编辑器宿主和场景插件并列安装。

use bevy::prelude::*;
use feathrise_editor_weights_model::WeightPanelScenes;

mod scene;

pub struct WeightPanelPlugin;

impl Plugin for WeightPanelPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(scenes());
    }
}

/// 正式插件和布局夹具使用同一套实体构造和字体依赖。
pub fn scenes() -> WeightPanelScenes {
    WeightPanelScenes {
        panel: || Box::new(scene::sidebar_page()),
        entry: || Box::new(scene::entry()),
    }
}
