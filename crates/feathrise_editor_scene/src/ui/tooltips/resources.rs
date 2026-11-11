//! 唯一悬停会话只记一个按钮和真实时钟，不持有作者草稿或保存请求。

use bevy::prelude::*;
use feathrise_scene_content::SceneId;

#[derive(Resource, Default)]
pub(super) struct EditorTooltipState {
    pub(super) target: Option<Entity>,
    pub(super) since: f64,
    pub(super) scene_id: Option<SceneId>,
    pub(super) reason: Option<&'static str>,
}
