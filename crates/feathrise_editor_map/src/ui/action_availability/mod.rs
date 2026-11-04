//! 顶栏与视口命令共享可用资格；表现、悬停说明和输入分派不各自维护一套规则。

mod inputs;
mod types;

pub(crate) use inputs::EditorActionInputs;
#[cfg(test)]
pub(in crate::ui) use inputs::dynamic_terrain_save_is_pending;
pub(in crate::ui) use types::EditorActionAvailability;

#[cfg(test)]
mod tests;
