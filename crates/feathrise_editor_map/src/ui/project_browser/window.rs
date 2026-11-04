//! 工程文件的完整滚动高度与固定可见窗口；布局只访问一个容器与少量指标。

use super::{components::*, resources::{ProjectBrowserState, ProjectFileViewport}, types::ProjectBrowserView};
use crate::{asset_catalog::project_files::EditorProjectFileCatalog, ui::continuous_list};
use bevy::prelude::*;

pub(super) const ROW_GAP: f32 = 2.;
pub(super) const ROW_PITCH: f32 = feathrise_editor_ui::metrics::ROW_HEIGHT + ROW_GAP;
pub(super) const MIN_POOL: usize = 12;

pub(super) fn prepare(
    catalog: Res<EditorProjectFileCatalog>,
    browser: Res<ProjectBrowserState>,
    mut window: ResMut<ProjectFileViewport>,
    mut pages: Query<(&ProjectBrowserContent, &ComputedNode, &UiGlobalTransform, &mut ScrollPosition)>,
    mut lists: Query<(&mut Node, &ComputedNode, &UiGlobalTransform), With<ProjectFileList>>,
) {
    if browser.view != ProjectBrowserView::ProjectFiles { return; }
    let Some((_, page, page_transform, mut scroll)) = pages.iter_mut().find(|(view, ..)| view.0 == ProjectBrowserView::ProjectFiles) else { return; };
    let Ok((mut style, list, list_transform)) = lists.single_mut() else { return; };
    let revision = (catalog.requested_revision, catalog.applied_revision);
    let reset = window.catalog_revision != Some(revision);
    if reset {
        if scroll.0 != Vec2::ZERO { scroll.0 = Vec2::ZERO; } // 刷新、切目录、筛选及新结果采用都从顶部开始。
        window.catalog_revision = Some(revision);
    }
    let height = continuous_list::content_height(catalog.entries.len(), 1, feathrise_editor_ui::metrics::ROW_HEIGHT, ROW_GAP);
    if style.height != px(height) { style.height = px(height); }
    let inverse = page.inverse_scale_factor;
    if page.size().y > 0. && inverse.is_finite() && inverse > 0. {
        // 整体内容包括上方目录和筛选、下方打开动作；这里只替换列表旧高度，不增加第二层滚动。
        let maximum = ((page.content_size().y - page.size().y + page.scrollbar_size.y - list.size().y) * inverse + height).max(0.);
        let clamped = scroll.y.clamp(0., maximum);
        if scroll.y != clamped { scroll.y = clamped; }
    }
    let capacity = if page.size().y > 0. { continuous_list::pool_capacity(page.size().y * inverse, 1, ROW_PITCH).max(MIN_POOL) }
        else { window.capacity.max(MIN_POOL) }; // 暂时隐藏的同一容器保留已测池，不在重新显示第一帧退回十二槽。
    let first = continuous_list::first_visible(list, list_transform, page, page_transform, &scroll, 1, ROW_PITCH, catalog.entries.len());
    if reset || window.first != first || window.capacity != capacity || window.scroll != scroll.0 {
        window.revision = window.revision.wrapping_add(1);
        window.first = first;
        window.capacity = capacity;
        window.scroll = scroll.0;
    }
}

/// Input早于新布局时仍核对当前ScrollPosition，不能把滚动前的释放派给新条目。
pub(super) fn current(window: &ProjectFileViewport, row: &ProjectFileRow, scroll: &ScrollPosition) -> bool {
    row.viewport_revision == window.revision && window.scroll == scroll.0
        && row.entry_index.is_some_and(|index| index >= window.first && index < window.first.saturating_add(window.capacity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EditorAssetEntry;
    use feathrise_map_content::MapAssetKind;

    #[test]
    fn project_continuous_window_keeps_full_height_and_rejects_stale_capture_after_scroll() {
        let mut app = App::new();
        let mut catalog = EditorProjectFileCatalog::default();
        catalog.entries = (0..192).map(|index| EditorAssetEntry { asset_path: format!("src/file{index:03}.rs"),
            display_name: format!("file{index:03}.rs"), is_directory: false, kind: MapAssetKind::Data }).collect();
        app.insert_resource(catalog).init_resource::<ProjectBrowserState>().init_resource::<ProjectFileViewport>()
            .add_systems(Update, prepare);
        let page = app.world_mut().spawn((ProjectBrowserContent(ProjectBrowserView::ProjectFiles),
            ComputedNode { size: Vec2::new(300., 400.), content_size: Vec2::new(300., 5150.), inverse_scale_factor: 1., ..default() },
            UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::new(150., 200.))), ScrollPosition::default())).id();
        let list = app.world_mut().spawn((ProjectFileList::default(), Node::default(),
            ComputedNode { size: Vec2::new(300., 4990.), inverse_scale_factor: 1., ..default() },
            UiGlobalTransform::from(bevy::math::Affine2::from_translation(Vec2::new(150., 80. + 2495.))))).id();
        app.update();
        let capacity = app.world().resource::<ProjectFileViewport>().capacity;
        assert_eq!(app.world().get::<Node>(list).unwrap().height, px(4990.));
        let window = app.world().resource::<ProjectFileViewport>();
        let old = ProjectFileRow { entry_index: Some(0), viewport_revision: window.revision, ..default() };
        assert!(current(window, &old, app.world().get::<ScrollPosition>(page).unwrap()));
        app.world_mut().get_mut::<ScrollPosition>(page).unwrap().y = 4700.;
        assert!(!current(app.world().resource::<ProjectFileViewport>(), &old, app.world().get::<ScrollPosition>(page).unwrap()));
        app.update();
        let window = app.world().resource::<ProjectFileViewport>();
        assert_eq!(window.capacity, capacity, "文件量和滚动不扩展实体池");
        assert!(window.first > 12 && window.first + window.capacity >= 192, "原十二槽以外末项必须连续可达");
        assert!(!current(window, &old, app.world().get::<ScrollPosition>(page).unwrap()));
        app.world_mut().resource_mut::<EditorProjectFileCatalog>().set_filter("191");
        app.update();
        assert_eq!(app.world().get::<ScrollPosition>(page).unwrap().y, 0.);
    }
}
