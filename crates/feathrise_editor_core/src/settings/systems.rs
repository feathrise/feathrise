//! 作者偏好向共享运行输入发布；界面倍率只在作者工具拥有界面时生效。
use super::{EditorSettings, effective_editor_ui_scale};
use bevy::prelude::*;
use feathrise_scene_core::input::{GamePauseSystemSet, EditorInputState};
use feathrise_settings::{
    EnvironmentPreviewSettings, RenderSettings, RuntimeSettingsSyncSet, SceneDisplaySettings,
};
pub struct EditorSettingsPlugin;
impl Plugin for EditorSettingsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EditorSettings>()
            .init_resource::<RenderSettings>()
            .init_resource::<SceneDisplaySettings>()
            .init_resource::<EnvironmentPreviewSettings>()
            .add_systems(
                Update,
                (publish_editor_settings, apply_author_ui_scale)
                    .in_set(RuntimeSettingsSyncSet)
                    .after(GamePauseSystemSet::Apply),
            );
    }
}
fn publish_editor_settings(
    source: Res<EditorSettings>,
    gate: Option<Res<EditorInputState>>,
    mut render: ResMut<RenderSettings>,
    mut scene: ResMut<SceneDisplaySettings>,
    mut environment: ResMut<EnvironmentPreviewSettings>,
) {
    // 原游戏的F12作者工具只在打开时拥有这些输入；独立编辑器没有游戏菜单所有权。
    if gate.is_some_and(|gate| !gate.map_editor_open) {
        return;
    }
    render.set_if_neq(source.render);
    let mut display = source.scene;
    display.terrain_backfaces = source.editor_show_terrain_backfaces;
    scene.set_if_neq(display);
    environment.set_if_neq(source.environment);
}
fn apply_author_ui_scale(
    source: Res<EditorSettings>,
    gate: Option<Res<EditorInputState>>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut scale: Option<ResMut<UiScale>>,
) {
    if gate.is_some_and(|gate| !gate.map_editor_open) {
        return;
    }
    let Some(scale) = scale.as_deref_mut() else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };
    let desired = effective_editor_ui_scale(
        source.editor_ui_scale,
        source.editor_ui_scale_auto,
        source.editor_ui_scale_auto_offset,
        window.width(),
        window.height(),
    );
    if scale.0 != desired {
        scale.0 = desired;
    }
}
