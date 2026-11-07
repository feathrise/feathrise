//! 编辑器设置的唯一契约；作者偏好不会进入公共 SDK 或原游戏 settings.json。
mod editor_shortcuts;
mod editor_snap;
mod locale;
mod persistence;
mod resources;
mod systems;
mod types;
pub use editor_shortcuts::*;
pub use editor_snap::*;
pub use locale::EditorLocalePreference;
pub use persistence::*;
pub use resources::*;
pub use systems::EditorSettingsPlugin;
pub use types::*;

#[cfg(test)]
mod tests;
