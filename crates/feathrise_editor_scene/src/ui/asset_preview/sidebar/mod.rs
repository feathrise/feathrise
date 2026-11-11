//! 资产浏览器静态分区与只读选择信息；预览仍使用原唯一画布及加载链。

mod components;
#[path = "../asset_property_details.rs"]
mod details;
mod scene;
mod systems;
mod types;

pub(crate) use scene::{asset_properties_page, assets_page};
pub(super) use details::install as install_details;
#[cfg(feature = "validation")]
pub(crate) use details::validation_text as validation_details_text;

#[cfg(test)]
pub(crate) fn install_snapshot_presentation(app: &mut bevy::prelude::App) {
    use bevy::prelude::*;
    app.add_systems(bevy::prelude::Update, systems::sync_sidebar
        .after(crate::ui::rows::sync_asset_rows).after(crate::ui::rows::sync_dynamic_list_layouts));
}

/// 只追加选择说明和显式复制；不改变目录、模型、文档的任务或渲染阶段。
pub(super) fn install(app: &mut bevy::prelude::App) {
    crate::ui::project_browser::install(app);
    use crate::SceneEditorSystemSet;
    use bevy::prelude::*;
    app.add_systems(
        Update,
        systems::copy_selected_path
            .after(
                crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
            )
            .in_set(SceneEditorSystemSet::Input),
    )
    .add_systems(
        Update,
        systems::sync_sidebar
            .after(crate::ui::rows::sync_asset_rows)
            .after(crate::ui::rows::sync_dynamic_list_layouts)
            .in_set(SceneEditorSystemSet::Presentation),
    );
}

#[cfg(test)]
mod tests;
