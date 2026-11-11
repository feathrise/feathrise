//! 缓存安装身份与菜单尺寸的常数计算；UI路径不探测磁盘或执行程序。
use super::{constants::EXTERNAL_MENU_ITEMS, resources::ExternalEditors, types::{ExternalEditor, ExternalFileAction}};
use bevy::prelude::Vec2;

impl ExternalEditors {
    pub(crate) fn installed(&self, editor: ExternalEditor) -> bool {
        self.detected.launchers[editor as usize].is_some()
    }
    pub(crate) fn visible(&self, action: ExternalFileAction, file: bool) -> bool {
        // 目录也能打开当前项目；只有明确指定文件行列的动作需要真实文件。
        action.editor().is_none_or(|editor| self.installed(editor)) && (action.location().is_none() || file)
    }
    pub(crate) fn menu_size(&self, file: bool, base_rows: usize, compact_width: f32) -> Vec2 {
        let rows = base_rows + EXTERNAL_MENU_ITEMS.iter().filter(|(_, action)| self.visible(*action, file)).count();
        let projects = ExternalEditor::ALL.into_iter().any(|editor| self.installed(editor));
        Vec2::new(if projects { 240.0 } else { compact_width }, 10.0 + rows as f32 * 24.0 + rows.saturating_sub(1) as f32 * 2.0)
    }
    #[cfg(test)]
    pub(crate) fn for_tests(editors: &[ExternalEditor]) -> Self {
        let mut available = Self::default();
        for editor in editors {
            available.detected.launchers[*editor as usize] = Some(super::types::EditorLauncher {
                executable: std::path::PathBuf::from(editor.name()), prefix_args: Vec::new(),
            });
        }
        available
    }
}
