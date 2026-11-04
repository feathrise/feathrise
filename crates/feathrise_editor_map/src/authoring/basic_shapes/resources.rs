//! 统一网格面板提交的基础形状意图、冻结的点放规格和当前帧明确工作平面。

use bevy::prelude::*;
use feathrise_map_content::{MapId, MapPlacementAssetTarget};

/// 坐标为世界中心；None才进入视口点放，面板本身不改变中央文档。
#[derive(Clone, Copy, Debug)]
pub(crate) struct CuboidCreationRequest {
    pub(crate) target: MapPlacementAssetTarget,
    pub(crate) position: Option<Vec3>,
}

#[derive(Resource, Default)]
pub(crate) struct BasicShapeState {
    /// 由已有快捷键总入口在完整文本/模态门通过后发出，打开同一网格工具面板。
    pub(crate) request_menu: bool,
    pub(crate) creation_request: Option<CuboidCreationRequest>,
    pub(crate) creation_identity: Option<(MapId, u64)>,
    pub(crate) cancel_requested: bool,
    pub(super) placement_target: Option<MapPlacementAssetTarget>,
    pub(super) placement_identity: Option<(MapId, u64)>,
    pub(super) captured_this_frame: bool,
    pub(super) work_plane: Option<(MapId, u64, Vec3)>,
}

impl BasicShapeState {
    pub(crate) fn owns_mouse(&self) -> bool {
        self.captured_this_frame
    }

    pub(crate) fn placement_target(&self, session: &super::super::EditorAuthoringSession) -> Option<MapPlacementAssetTarget> {
        let draft = session.draft.as_ref()?;
        let (map, revision) = self.placement_identity.as_ref()?;
        (map == &draft.map_id && *revision == session.revision).then_some(self.placement_target?).filter(|target| target.is_builtin())
    }

    /// 该点只为无地形地图中的基础形状放置服务，不写入公共真实地面光标。
    pub(crate) fn work_plane_point(
        &self,
        session: &super::super::EditorAuthoringSession,
    ) -> Option<Vec3> {
        let (map, revision, point) = self.work_plane.as_ref()?;
        let draft = session.draft.as_ref()?;
        (map == &draft.map_id
            && *revision == session.revision
            && super::placement::work_plane_is_available(session))
        .then_some(*point)
    }
}
