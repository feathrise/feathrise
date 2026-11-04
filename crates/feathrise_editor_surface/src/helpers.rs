//! 地表实现复用唯一资产事务和官方 IME 开关；不改变原有写盘及焦点规则。
use bevy::{prelude::*, window::PrimaryWindow};
pub(crate) use feathrise_editor_assets::{
    commit_owned_files,
    files::{safe_asset_path, workspace_asset_root},
    ktx2::{Rgba8ArrayKind, encode_rgba8_array},
};
pub(crate) fn set_ime_enabled(
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
    enabled: bool,
) {
    if let Ok(mut window) = windows.single_mut() {
        if window.ime_enabled != enabled {
            window.ime_enabled = enabled;
        }
    }
}
