//! 工程目录的内部呈现来源；进入登记资产目录时借用原资产正文，不提供模式切换入口。

use super::types::ProjectBrowserView;
use crate::asset_catalog::{AssetTypeFilter, EditorAssetCatalog, project_files::EditorProjectFileCatalog};
use bevy::prelude::{Resource, Vec2};
use feathrise_editor_ui::localization::EditorLocale;

/// 可见窗口身份与完整目录分离；滚动/尺寸改变使旧按钮捕获失效，不复制文件表。
#[derive(Resource, Default)]
pub(crate) struct ProjectFileViewport {
    pub(crate) first: usize,
    pub(crate) capacity: usize,
    pub(crate) revision: u64,
    pub(crate) scroll: Vec2,
    pub(super) catalog_revision: Option<(u64, u64)>,
}

#[derive(Resource, Default)]
pub(crate) struct ProjectBrowserState {
    pub(crate) view: ProjectBrowserView,
    /// 两种正文共用当前类型；目录导航只切内部来源，不重置类型选择。
    pub(crate) type_filter: AssetTypeFilter,
    /// 登记资产根相对工程根的真实前缀；绑定来源变化时计算，不改AssetServer资源URI。
    pub(crate) asset_prefix: Option<String>,
}

impl ProjectBrowserState {
    pub(crate) fn show_assets(&mut self) {
        self.view = ProjectBrowserView::Assets;
    }

    pub(crate) fn assets_visible(&self) -> bool {
        self.view == ProjectBrowserView::Assets
    }

    pub(super) fn set_type_filter(
        &mut self, filter: AssetTypeFilter, files: &mut EditorProjectFileCatalog, assets: &mut EditorAssetCatalog,
    ) {
        if self.type_filter != filter {
            self.type_filter = filter;
            files.set_type_filter(filter);
            assets.set_type_filter(filter);
        }
    }

    pub(crate) fn asset_folder_label(&self, folder: &str, locale: EditorLocale) -> String {
        let root = self.asset_prefix.as_deref().map_or_else(
            || "assets".to_owned(),
            |prefix| format!("{}{prefix}", locale.text("label.project")),
        );
        if folder.is_empty() { format!("{root}/") } else { format!("{root}/{folder}/") }
    }
}
