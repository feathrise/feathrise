//! 根应用注册私有实现；地图宿主仅链接稳定函数表，常用表单修改不传播回宿主。
use super::{host::SurfaceEditorHost, resources::SurfacePresetEditorState, system_params::*};
use bevy::prelude::*;
use feathrise_editor_core::{PresetWorkspaceState, PresetWorkspaceTab};
#[derive(Resource, Clone, Copy)]
pub struct SurfaceEditorExtension {
    pub buttons: fn(&mut dyn SurfaceEditorHost, &mut PresetWorkspaceState, ButtonsParams),
    pub text: fn(bool, bool, &mut PresetWorkspaceState, TextParams),
    pub persistence: fn(&mut dyn SurfaceEditorHost, &mut PresetWorkspaceState, PersistenceParams),
    pub panel: fn(Option<&str>, PanelParams),
    pub badge: fn(&PresetWorkspaceState, BadgeParams),
    pub styles: fn(Option<&str>, &PresetWorkspaceState, StylesParams),
    pub spawn: fn(
        &mut Commands,
        Entity,
        &Handle<Font>,
        &SurfacePresetEditorState,
        PresetWorkspaceTab,
    ) -> bool,
}
