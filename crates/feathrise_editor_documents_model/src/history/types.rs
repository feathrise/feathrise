//! 文档输入、历史和格式化共用的UTF-8选区；不包含原生输入实例或历史栈。

use bevy::text::EditableText;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub focus: usize,
}

impl Selection {
    pub fn read(text: &EditableText) -> Self {
        let selection = text.editor.raw_selection();
        Self {
            anchor: selection.anchor().index(),
            focus: selection.focus().index(),
        }
    }
}
