//! 显式 validation 只查找生产诊断行实体；验收驱动仍须通过原按钮协议发布合法点击。

use super::{components::{DiagnosticRow, DiagnosticsOverlay, DiagnosticsPane}, resources::DiagnosticsOverlayState, systems};
use super::super::resources::CodeIterationState;
use bevy::{prelude::*, ui::InteractionDisabled};

/// 只有浮窗、实际行布局、结果修订及滚动身份都一致时才返回原 Button，绝不伪造定位请求。
pub(crate) fn diagnostic_row_entity(world: &mut World, index: usize) -> Option<Entity> {
    let mut overlays = world.query_filtered::<(&Node, &ComputedNode, &InheritedVisibility), With<DiagnosticsOverlay>>();
    if !overlays.iter(world).any(|(node, computed, visible)| node.display != Display::None && !computed.is_empty() && visible.get()) { return None; }
    let mut panes = world.query_filtered::<&ScrollPosition, With<DiagnosticsPane>>();
    let scroll = panes.iter(world).next()?.0;
    let mut rows = world.query::<(Entity, &DiagnosticRow, &ComputedNode, &InheritedVisibility, Has<InteractionDisabled>)>();
    let state = world.get_resource::<CodeIterationState>()?;
    let view = world.get_resource::<DiagnosticsOverlayState>()?;
    if !state.diagnostics_open { return None; }
    rows.iter(world).find_map(|(entity, row, computed, visible, disabled)| {
        (row.index == Some(index) && systems::current(row, state, view, scroll)
            && !disabled && !computed.is_empty() && visible.get()).then_some(entity)
    })
}
