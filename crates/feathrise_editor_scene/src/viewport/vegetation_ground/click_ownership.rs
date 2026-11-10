//! 共享Clipmap查询槽的作者点击所有权；取消旧工具只能移除本域的三个等待阶段。
use super::{GroundCursorTarget, GroundRequest, VegetationGroundCursor};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClickOwner {
    SplineWater,
    Placement,
    Portal,
    Domain(&'static str),
    Boundary,
    Building,
    Courtyard,
}

fn owner(request: &GroundRequest) -> Option<ClickOwner> {
    match request.context.target.as_ref()? {
        GroundCursorTarget::River(_) | GroundCursorTarget::Waterfall(_) => {
            Some(ClickOwner::SplineWater)
        }
        GroundCursorTarget::PlaceAsset { .. } | GroundCursorTarget::RelocatePlacement(_) => {
            Some(ClickOwner::Placement)
        }
        GroundCursorTarget::Portal(_) => Some(ClickOwner::Portal),
        GroundCursorTarget::Domain(target) if target.click_qualified => Some(ClickOwner::Domain(target.domain)),
        GroundCursorTarget::BoundaryPoint { .. } => Some(ClickOwner::Boundary),
        GroundCursorTarget::Building { .. } | GroundCursorTarget::RelocateBuilding { .. } => {
            Some(ClickOwner::Building)
        }
        GroundCursorTarget::Courtyard { .. } => Some(ClickOwner::Courtyard),
        _ => None, // 圆刷和拖动沿用连续输入代次，不属于松键后仍可完成的冻结单击。
    }
}

impl VegetationGroundCursor {
    fn cancel_owned_click(&mut self, expected: ClickOwner) {
        let owned = |request: &GroundRequest| owner(request) == Some(expected);
        if self.pending_author_click.as_ref().is_some_and(owned) {
            self.pending_author_click = None;
        }
        if self
            .pending_author_surface
            .as_ref()
            .is_some_and(|completion| owned(&completion.request))
        {
            self.pending_author_surface = None;
        }
        if self
            .completed_author_click
            .as_ref()
            .is_some_and(|completion| owned(&completion.request))
        {
            self.completed_author_click = None;
        }
    } // 分别复核三个槽位；新工具已武装时旧工具收口也不能取消它的点击。

    pub(crate) fn cancel_spline_click(&mut self) {
        self.cancel_owned_click(ClickOwner::SplineWater);
    }
    pub(crate) fn cancel_placement_click(&mut self) {
        self.cancel_owned_click(ClickOwner::Placement);
    }
    pub(crate) fn cancel_portal_click(&mut self) {
        self.cancel_owned_click(ClickOwner::Portal);
    }
    pub fn cancel_domain_click(&mut self, domain: &'static str) {
        self.cancel_owned_click(ClickOwner::Domain(domain));
    } // 每种外部工具只取消自己的点击，不能清除另一个工具新冻结的意图。
    pub(crate) fn cancel_boundary_click(&mut self) {
        self.cancel_owned_click(ClickOwner::Boundary);
    }
    pub(crate) fn cancel_building_click(&mut self) {
        self.cancel_owned_click(ClickOwner::Building);
    }
    pub(crate) fn cancel_courtyard_click(&mut self) {
        self.cancel_owned_click(ClickOwner::Courtyard);
    }
}
