//! 原资产正文的唯一外层滚动视口；列表和缩略图共用连续窗口几何，不扫描完整目录。

use bevy::{ecs::system::SystemParam, prelude::*};
use crate::{components::{SceneEditorAssetList, SceneEditorLeftPanelScroll}, ui::{continuous_list, project_browser::{ProjectBrowserContent, ProjectBrowserView}, sidebars::{EditorNavigatorTab, EditorSidebarTab, SceneEditorSidebarPage}}};

#[derive(SystemParam)]
pub(crate) struct AssetViewport<'w, 's> {
    lists: Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<SceneEditorAssetList>>,
    pages: Query<'w, 's, (Option<&'static ProjectBrowserContent>, Option<&'static SceneEditorSidebarPage>,
        &'static ComputedNode, &'static UiGlobalTransform, &'static ScrollPosition), With<SceneEditorLeftPanelScroll>>,
    settings: Option<Res<'w, feathrise_editor_core::EditorSettings>>,
}

pub(crate) struct AssetWindow {
    pub(crate) start: usize,
    pub(crate) capacity: usize,
    pub(crate) columns: usize,
    pub(crate) width: f32,
    pub(crate) row_height: f32,
    pub(crate) gap: f32,
}

impl AssetViewport<'_, '_> {
    pub(crate) fn geometry(&self, total: usize) -> AssetWindow {
        let list = self.lists.single().ok();
        let width = list.map_or(260., |(list, _)| (list.size().x * list.inverse_scale_factor()).max(96.));
        let thumbnails = self.settings.as_ref().is_some_and(|settings| settings.asset_browser_view_mode == feathrise_editor_core::AssetBrowserViewMode::Thumbnails);
        let (columns, width, row_height, gap) = if thumbnails {
            let grid = crate::ui::asset_thumbnails::grid_layout(width, total);
            (grid.columns, grid.width, crate::ui::asset_thumbnails::TILE_HEIGHT, crate::ui::asset_thumbnails::TILE_GAP)
        } else { (1, width, feathrise_editor_ui::metrics::ROW_HEIGHT, 2.) };
        let page = self.pages.iter().find(|(content, marker, ..)|
            content.is_some_and(|content| content.0 == ProjectBrowserView::Assets)
                || content.is_none() && marker.is_some_and(|marker| marker.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)));
        let (start, capacity) = match (list, page) {
            (Some((list, list_transform)), Some((_, _, viewport, transform, scroll))) if viewport.size().y > 0. => {
                (continuous_list::first_visible(list, list_transform, viewport, transform, scroll, columns, row_height + gap, total),
                    continuous_list::pool_capacity(viewport.size().y * viewport.inverse_scale_factor(), columns, row_height + gap).max(super::ASSET_ROW_COUNT))
            }
            _ => (0, super::ASSET_ROW_COUNT),
        };
        AssetWindow { start, capacity, columns, width, row_height, gap }
    }

    pub(crate) fn is_current(&self, catalog: &crate::asset_catalog::EditorAssetCatalog) -> bool {
        self.geometry(catalog.entries.len()).start == catalog.visible_start
    }
}
