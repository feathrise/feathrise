//! 替换表单、中央差异及文件勾选的固定UI契约；所有操作带预览票据。

use bevy::prelude::*;

#[derive(Component, Clone, Default)]
pub struct ReplaceField;
#[derive(Component, Clone, Default)]
pub struct ReplacePanel;
#[derive(Component, Clone, Default)]
pub struct ReplaceFiles;
#[derive(Component, Clone, Default)]
pub struct ReplaceDiff;
#[derive(Component, Clone, Default)]
pub struct ReplaceFileContent;
#[derive(Component, Clone, Default)]
pub struct ReplaceDiffContent;
#[derive(Component)]
pub struct ReplaceFileRow(pub usize);
#[derive(Component)]
pub struct ReplaceDiffRow(pub usize);
#[derive(Component, Clone, Default)]
pub struct ReplaceTab;

#[derive(Component)]
pub enum ReplaceButtonSlot {
    File(usize),
    Check(usize),
    Hit(usize),
}

#[derive(Component, Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum ReplaceAction {
    Open,
    Activate,
    Close,
    Generate,
    SelectAll,
    SelectNone,
    ApplyHit,
    ApplyFile,
    ApplyChecked,
    OpenFile,
    Undo,
    Redo,
    SaveBatch,
    File {
        ticket: u64,
        index: usize,
    },
    Check {
        ticket: u64,
        index: usize,
    },
    Hit {
        ticket: u64,
        index: usize,
    },
    #[default]
    Inactive,
}

#[derive(Component, Clone, Default)]
pub enum ReplaceLabel {
    #[default]
    Status,
    Path,
    Summary,
    File(usize),
    Check(usize),
    Diff(usize, bool),
    Button(ReplaceAction), // 结果只列出部分匹配时，按钮同步说明实际替换范围。
}
