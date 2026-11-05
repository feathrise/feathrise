//! 资产定位的一次性可见性收口；只检查固定十二行，不逐帧追踪或抢回作者滚动。

use super::{EditorNavigatorTab, EditorSidebarTab, MapEditorSidebarPage, MapEditorSidebarState}; // 只操作左栏资产目录，不移动右侧属性滚动。
use crate::components::{MapEditorAssetRow, MapEditorAssetList}; // 固定行池的全目录索引。
use crate::resources::{EditorAssetCatalog, MapEditorState}; // 已原子采用的定位事实。
use bevy::prelude::*; // 最终布局坐标、滚动位置和轻量修订缓存。 // 采用与显示共用十二行容量。

/// 定位成功后最小幅度滚动至高亮行；普通选择、失败、切页或手动滚动均不会被追着重置。
pub(crate) fn sync_asset_reveal_scroll(
    catalog: Res<EditorAssetCatalog>,          // 只读取一条已完成意图。
    editor: Res<MapEditorState>,               // 不改变选中资产、预设或地图dirty。
    sidebar: Res<MapEditorSidebarState>,       // 作者已经离开资产页就放弃滚动。
    browser: Option<Res<crate::ui::project_browser::ProjectBrowserState>>,
    mut consumed: Local<Option<(u64, u64, usize)>>, // 同一次定位或模式锚点最多调整一次滚动。
    settings: Option<Res<feathrise_editor_core::EditorSettings>>,
    mut pages: Query<(
        Option<&MapEditorSidebarPage>,
        Option<&crate::ui::project_browser::ProjectBrowserContent>,
        &Node,
        &ComputedNode,
        &UiGlobalTransform,
        &mut ScrollPosition,
    ), Or<(With<MapEditorSidebarPage>, With<crate::ui::project_browser::ProjectBrowserContent>)>>, // 兼容旧夹具，正式浏览器使用资产视图自己的滚动容器。
    rows: Query<(&MapEditorAssetRow, &Node, &ComputedNode, &UiGlobalTransform)>, // 固定十二行，不扫描全部目录。
    lists: Query<(&ComputedNode, &UiGlobalTransform), With<MapEditorAssetList>>,
) {
    let target = catalog.scroll_anchor.map(|anchor| (anchor.ticket, anchor.revision, anchor.index, anchor.require_selection))
        .or_else(|| catalog.revealed_entry.map(|(revision, index)| (0, revision, index, true)));
    let Some((ticket, revision, index, require_selection)) = target else {
        return;
    }; // 无定位的稳定帧常数返回。
    let identity = (ticket, revision, index);
    if *consumed == Some(identity) {
        return;
    } // 作者随后自由滚动，不自动拉回。
    let still_current = editor.open // 关闭F12后不排队抢滚动。
        && !editor.close_requested // 关闭联合门也表示作者离开。
        && sidebar.assets_browser_visible() // 目录是否可见独立于右侧属性或工具。
        && browser.as_ref().is_none_or(|browser| browser.assets_visible())
        && catalog.requested_revision == revision // 刷新和后来的导航拥有优先权。
        && catalog.applied_revision == revision // 不能用旧布局响应尚未采用的目录。
        && catalog.entries.get(index).is_some_and(|entry| !require_selection || editor.selected_asset.as_deref() == Some(entry.asset_path.as_str())); // 模式锚点不依赖选中；旧定位仍让位于后续选择。
    if !still_current {
        *consumed = Some(identity); // 失效也是完成，不让它下次开页复活。
        return;
    }
    // 正式正文与旧标记外壳可同时存在；正文暂隐藏/零布局时也要等待，不能在外壳消费定位。
    let has_asset_content = pages.iter().any(|(_, view, ..)|
        view.is_some_and(|view| view.0 == crate::ui::project_browser::ProjectBrowserView::Assets));
    let Some((_, _, page_node, page, page_transform, mut scroll)) = pages
        .iter_mut()
        .find(|(marker, view, ..)| view.is_some_and(|view| view.0 == crate::ui::project_browser::ProjectBrowserView::Assets)
            || !has_asset_content && view.is_none()
                && marker.is_some_and(|marker| marker.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)))
    else {
        return;
    }; // 初次生成前等待UI挂载。
    if page_node.display == Display::None || page.size().y <= 0.0 {
        return; // 下一次真实有效布局再收口，不能用隐藏页零尺寸计算。
    }
    let viewport = physical_rect(page, page_transform); // 布局坐标与行坐标同为物理像素。
    let item = if let Ok((list, transform)) = lists.single() {
        if list.size().x <= 0.0 { return; }
        let thumbnails = settings.as_ref().is_some_and(|settings| settings.asset_browser_view_mode == feathrise_editor_core::AssetBrowserViewMode::Thumbnails);
        let (columns, height, gap) = if thumbnails {
            let grid = crate::ui::asset_thumbnails::grid_layout(list.size().x * list.inverse_scale_factor(), catalog.entries.len());
            (grid.columns, crate::ui::asset_thumbnails::TILE_HEIGHT, crate::ui::asset_thumbnails::TILE_GAP)
        } else { (1, feathrise_editor_ui::metrics::ROW_HEIGHT, 2.) };
        let top = transform.translation.y - list.size().y * 0.5;
        let inverse = list.inverse_scale_factor();
        let minimum = Vec2::new(transform.translation.x - list.size().x * 0.5,
            top + (index / columns) as f32 * (height + gap) / inverse);
        Rect::from_corners(minimum, minimum + Vec2::new(list.size().x, height / inverse))
    } else {
        let Some((_, row_node, row, transform)) = rows.iter().find(|(row, ..)| row.entry_index == Some(index)) else { return; };
        if row_node.display == Display::None || row.size().y <= 0. { return; }
        physical_rect(row, transform) // 兼容只挂实际单行的旧布局夹具；正式虚拟列表不等待远处行绑定。
    };
    let delta = visible_delta(viewport, item) * page.inverse_scale_factor(); // 最后一步才转回ScrollPosition逻辑单位，避免全屏倍率错位。
    let maximum = (page.content_size().y - page.size().y).max(0.0) * page.inverse_scale_factor(); // 真实滚动范围，不假设固定窗口高。
    scroll.y = (scroll.y + delta).clamp(0.0, maximum); // 只做必要纵向修正，不改变水平位置或预览视角。
    *consumed = Some(identity); // 完成后立即释放自动滚动控制。
}

/// 布局与可见区域使用同一物理坐标，不额外乘UI倍率。
fn physical_rect(node: &ComputedNode, transform: &UiGlobalTransform) -> Rect {
    Rect::from_center_size(transform.affine().translation, node.size()) // 右栏没有额外旋转，与实际命中矩形相同。
}

/// 返回让目标完整可见所需的最小滚动；超高目标优先露出顶部。
fn visible_delta(viewport: Rect, item: Rect) -> f32 {
    if item.min.y < viewport.min.y || item.height() > viewport.height() {
        item.min.y - viewport.min.y // 目标在上方时向上滚，不把预览一起拉回目录最顶端。
    } else if item.max.y > viewport.max.y {
        item.max.y - viewport.max.y // 低高度窗口中，最后一行也能自动露出。
    } else {
        0.0 // 已可见时完全不扰动当前滚动位置。
    }
}

#[cfg(test)]
mod tests; // 一次性滚动、缩放与过期意图的真实ECS回归。
