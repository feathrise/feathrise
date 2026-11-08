//! 拖拽期间的悬停展开和树内边缘滚动；只维护一份计时身份并检查固定行池。

use bevy::prelude::*;
use bevy::time::Real;
use bevy::ui::{CalculatedClip, RelativeCursorPosition};
use bevy::window::PrimaryWindow;
use feathrise_map_content::MapContentRegistry;

use super::drag::row_is_hovered;
use crate::components::{MapEditorMapRow, MapEditorMapTreeScroll};
use crate::constants::{
    MAP_BROWSER_DRAG_EXPAND_SECONDS, MAP_BROWSER_DRAG_SCROLL_EDGE, MAP_BROWSER_DRAG_SCROLL_SPEED,
};
use crate::map_browser::helpers::folder_by_id;
use crate::resources::{
    MapBrowserDragState, MapBrowserIndexState, MapBrowserState, MapEditorNumericScrubMouseGuard,
    MapEditorSettingsState, MapEditorState,
};
use crate::types::{MapBrowserHoverExpand, MapBrowserItem};
use crate::ui::sidebars::MapEditorSidebarState;

/// 时钟使用真实时间，F12暂停玩法时仍可展开；帧卡顿最多补50ms滚动，避免焦点恢复后跳跃。
fn edge_scroll_delta(cursor: Vec2, bounds: Rect, inverse: f32, seconds: f32) -> f32 {
    if !cursor.is_finite()
        || !bounds.min.is_finite()
        || !bounds.max.is_finite()
        || bounds.size().min_element() <= 0.0
        || !bounds.contains(cursor)
        || !inverse.is_finite()
        || inverse <= 0.0
        || !seconds.is_finite()
    {
        return 0.0;
    }
    let height = bounds.height() * inverse;
    let edge = MAP_BROWSER_DRAG_SCROLL_EDGE.min(height * 0.5);
    let y = (cursor.y - bounds.min.y) * inverse;
    let strength = if y < edge {
        -(1.0 - y / edge)
    } else if height - y < edge {
        1.0 - (height - y) / edge
    } else {
        0.0
    };
    strength * MAP_BROWSER_DRAG_SCROLL_SPEED * seconds.clamp(0.0, 0.05)
}

/// 必须在原拖拽状态机之后执行：释放/取消帧不能再展开、滚动或更换原事务目标。
#[allow(clippy::too_many_arguments, clippy::type_complexity)] // 输入、投影身份及唯一滚动宿主在同一边界复核。
pub(crate) fn navigate_map_browser_drag(
    time: Res<Time<Real>>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    editor: Res<MapEditorState>,
    mut drag: ResMut<MapBrowserDragState>,
    mut browser: ResMut<MapBrowserState>,
    index: Res<MapBrowserIndexState>,
    registry: Option<Res<MapContentRegistry>>,
    sidebar: Option<Res<MapEditorSidebarState>>,
    blockers: (
        Option<Res<MapEditorSettingsState>>,
        Option<Res<crate::authoring::TerrainExternalChangeState>>,
        Option<Res<MapEditorNumericScrubMouseGuard>>,
        Option<Res<feathrise_editor_ui::ActiveNumericInputScrub>>,
    ),
    mut trees: Query<
        (
            &Node,
            &ComputedNode,
            &UiGlobalTransform,
            Option<&CalculatedClip>,
            &InheritedVisibility,
            &RelativeCursorPosition,
            &mut ScrollPosition,
        ),
        With<MapEditorMapTreeScroll>,
    >,
    rows: Query<(
        &Interaction,
        &MapEditorMapRow,
        Option<&RelativeCursorPosition>,
    )>,
    mut hover: Local<Option<MapBrowserHoverExpand>>,
) {
    let source_current = drag
        .press
        .as_ref()
        .is_some_and(|press| press.document_revision == browser.document_revision);
    let projection_current = !index.rebuilding
        && index
            .applied_projection
            .as_ref()
            .is_some_and(|(source, expansion)| {
                source.folder_revision == browser.document_revision
                    && source.catalog_revision
                        == registry.as_ref().map_or(0, |registry| registry.revision)
                    && *expansion == browser.expansion_revision
                    && index.applied_signature.as_ref() == Some(source)
            });
    if !editor.open
        || editor.close_requested
        || !drag.active
        || !source_current
        || !mouse.pressed(MouseButton::Left)
        || mouse.just_released(MouseButton::Left)
        || !projection_current
        || sidebar
            .as_ref()
            .is_some_and(|sidebar| !sidebar.map_browser_visible())
        || blockers
            .0
            .as_ref()
            .is_some_and(|settings| settings.open || settings.owns_mouse_this_frame)
        || blockers
            .1
            .as_ref()
            .is_some_and(|conflict| conflict.modal_open())
        || (blockers.2.as_ref().is_some_and(|guard| guard.blocked())
            && (!drag.blocks_other_mouse()
                || blockers
                    .3
                    .as_ref()
                    .is_some_and(|scrub| scrub.entity.is_some())))
    // 当前拖拽自身也写入共享鼠标门；只有外部数值所有者可以借该门阻止拖拽导航。
    {
        *hover = None;
        return;
    }
    let Ok(window) = windows.single() else {
        *hover = None;
        return;
    };
    let Some(cursor) = window.physical_cursor_position().filter(|_| window.focused) else {
        *hover = None;
        return;
    };
    let Ok((node, computed, transform, clip, visible, relative, mut scroll)) = trees.single_mut()
    else {
        *hover = None;
        return;
    };
    let mut bounds = Rect::from_center_size(transform.translation, computed.size());
    if let Some(clip) = clip {
        let Some(clipped) = feathrise_editor_ui::clipped_ui_bounds(clip, bounds) else {
            *hover = None;
            return;
        };
        bounds = clipped; // 低高度外栏裁切后，按实际可见的列表边缘滚动。
    }
    bounds = bounds.intersect(Rect::from_corners(
        Vec2::ZERO,
        window.physical_size().as_vec2(),
    ));
    if node.display == Display::None
        || !visible.get()
        || !relative.cursor_over()
        || bounds.size().min_element() <= 0.0
        || !bounds.contains(cursor)
    {
        *hover = None;
        return;
    }
    let inverse = computed.inverse_scale_factor();
    let maximum = ((computed.content_size().y - computed.size().y) * inverse).max(0.0);
    let delta = edge_scroll_delta(cursor, bounds, inverse, time.delta_secs());
    let next = (scroll.y + delta).clamp(0.0, maximum);
    if next != scroll.y {
        scroll.y = next; // 只改树内Y轴，不合成滚轮，也不向外侧栏传递。
        drag.drop_target = None;
        drag.drop_error = None; // 行池和真实命中下一帧重新绑定，不能保留滚动前的目标高亮。
        *hover = None; // 行在鼠标下移动时不累计悬停，避免飞过一排目录就全部展开。
        return;
    }
    let folder = rows.iter().find_map(|(interaction, row, cursor)| {
        if row_is_hovered(interaction, cursor)
            && let Some(MapBrowserItem::Folder(id)) = &row.item
        {
            return Some(id);
        }
        None
    });
    let Some(folder_id) = folder.filter(|id| !browser.expanded_folder_ids.contains(*id)) else {
        *hover = None;
        return;
    };
    let press = drag.press.as_ref().unwrap(); // 活动来源已经在入口与原状态机共同复核。
    let now = time.elapsed_secs_f64();
    let same = hover.as_ref().is_some_and(|hover| {
        hover.source == press.item
            && hover.document_revision == press.document_revision
            && &hover.folder_id == folder_id
    });
    if !same {
        *hover = Some(MapBrowserHoverExpand {
            source: press.item.clone(),
            document_revision: press.document_revision,
            folder_id: folder_id.clone(),
            since: now,
        });
        return;
    }
    if now - hover.as_ref().unwrap().since < MAP_BROWSER_DRAG_EXPAND_SECONDS {
        return;
    }
    if folder_by_id(&browser.document, folder_id).is_some()
        && browser.expanded_folder_ids.insert(folder_id.clone())
    {
        browser.expansion_revision = browser.expansion_revision.wrapping_add(1);
        // 只改变会话展开集合，沿用原single-flight后台投影；无效放置目录也允许展开以寻找其合法子目录。
        // 不切换选择、不改归类或地图dirty，也不触发保存；松开仍由原完整落点校验决定是否移动。
    }
    *hover = None;
}

#[cfg(test)]
#[path = "navigation_tests.rs"]
mod tests;
