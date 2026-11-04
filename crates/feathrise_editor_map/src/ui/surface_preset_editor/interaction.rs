//! 输入阶段的即时宿主适配，私有逻辑由根应用插件注册。
pub(super) use super::systems::{handle_buttons, handle_text_input};
#[cfg(test)]
#[path = "interaction_tests.rs"]
mod tests;
