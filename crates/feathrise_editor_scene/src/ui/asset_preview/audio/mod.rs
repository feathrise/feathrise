//! 资产音频试听：后台受检完整解码、单输出和原位中央多标签，不触碰玩法/作者数据。

use bevy::{
    audio::{AddAudioSource, AudioPlugin},
    prelude::*,
};
mod components;
mod context;
mod control;
mod decode;
mod drive;
#[cfg(test)]
mod gpu_tests;
mod io;
mod presentation;
mod resources;
pub(super) mod scene;
mod source;
#[cfg(test)]
mod tests;
mod types;

pub(crate) use resources::AudioPreviewState;

pub(super) fn install(app: &mut App) {
    app.init_resource::<AudioPreviewState>();
    if app.is_plugin_added::<AudioPlugin>() {
        app.add_audio_source::<source::PreviewAudio>();
    } else {
        // 最小无声测试仍安装完整状态机；没有设备不能被伪造为正在播放。
        app.init_resource::<Assets<source::PreviewAudio>>();
    }
    // 在本域注册系统，父入口只依赖install；私有PCM和控件契约不穿过模块边界。
    app.add_systems(
        Update,
        control::input
            .after(super::workspace::actions)
            .after(super::interaction::handle_asset_preview_actions)
            .before(super::navigation::control_asset_preview)
            .before(
                crate::ui::numeric_scrub_mouse_guard::update_scene_editor_numeric_scrub_mouse_guard,
            )
            .in_set(crate::SceneEditorSystemSet::Input),
    )
    .add_systems(
        Update,
        drive::drive
            .after(super::loading::sync_asset_preview_selection)
            .before(crate::game_authoring::ExternalPreviewAnimate)
            .in_set(crate::SceneEditorSystemSet::Tasks),
    )
    .add_systems(
        Update,
        presentation::sync
            .after(super::presentation::sync_asset_preview_controls)
            .before(super::subasset_presentation::sync_asset_preview_subassets)
            .in_set(crate::SceneEditorSystemSet::Presentation),
    );
}
