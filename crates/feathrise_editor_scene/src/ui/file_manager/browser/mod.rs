//! 多窗口复用工程资产浏览的目录、属性、片段、缩略图与导航服务，路径与历史仍归文件窗口。

mod components;
mod control;
mod presentation;
pub(super) mod resources;
mod scene;
mod services;
mod types;

pub(crate) use components::{BrowserButton, BrowserNavigate};
#[cfg(feature = "validation")]
pub(crate) use components::{BrowserChoice, BrowserOverlay, ClipExpander};
#[cfg(feature = "validation")]
pub(crate) use types::BrowserAction;
pub(crate) use resources::BrowserSessions;
pub(super) use scene::spawn_toolbar;

use bevy::prelude::*;

#[derive(SystemSet, Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub(crate) struct BrowserInput;

pub(super) fn install(app: &mut App) {
    app.init_resource::<BrowserSessions>().add_message::<BrowserNavigate>()
        .add_systems(Update, (control::control, control::select, services::scroll).chain()
            .after(super::FileManagerInput)
            .before(crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard)
            .in_set(BrowserInput).in_set(crate::SceneEditorSystemSet::Input))
        .add_systems(Update, presentation::demand
            .before(crate::ui::asset_thumbnails::SharedDemandSet)
            .in_set(crate::SceneEditorSystemSet::Tasks))
        .add_systems(Update, services::mount.before(super::scene::mount).in_set(crate::SceneEditorSystemSet::Presentation))
        .add_systems(Update, (scene::row_extras, presentation::sync_rows, presentation::sync_toolbar, presentation::sync_popup).chain()
            .after(super::presentation::sync).in_set(crate::SceneEditorSystemSet::Presentation));
}
