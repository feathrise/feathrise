//! 唯一悬停会话只记一个按钮和真实时钟，不持有作者草稿或保存请求。

use bevy::prelude::*;
use feathrise_map_content::MapId;

#[derive(Resource, Default)]
pub(super) struct EditorTooltipState {
    pub(super) target: Option<Entity>,
    pub(super) since: f64,
    pub(super) map_id: Option<MapId>,
    pub(super) reason: Option<&'static str>,
}
