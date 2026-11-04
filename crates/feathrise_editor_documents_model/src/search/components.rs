//! 固定搜索表单、虚拟结果行和滚动视口标记。

use bevy::prelude::*;

#[derive(Component, Default, Clone)]
pub struct SearchBody;
#[derive(Component, Default, Clone)]
pub struct SearchResults;
#[derive(Component, Default, Clone)]
pub struct SearchContent;
#[derive(Component, Default, Clone)]
pub struct SearchTextControls;
#[derive(Component, Default, Clone)]
pub struct SearchReferenceControls;
#[derive(Component, Default, Clone)]
pub struct SearchEditorControls;
#[derive(Component, Clone)]
pub struct SearchEditorOption(pub feathrise_editor_core::ExternalEditor);
impl Default for SearchEditorOption {
    fn default() -> Self {
        Self(feathrise_editor_core::ExternalEditor::VsCode)
    }
}
#[derive(Component)]
pub struct SearchRow(pub usize);
#[derive(Component)]
pub struct SearchExternalRow(pub usize, pub bool); // 槽位与是否打开当前项目；两种入口共享所选IDE。

#[derive(Component, Default, Clone, Copy, PartialEq, Eq)]
pub enum SearchField {
    #[default]
    Query,
    Filter,
}

#[derive(Component, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchAction {
    Toggle,
    Submit,
    TextSearch,
    MissingAssets,
    Cancel,
    AllAssets,
    CurrentFolder,
    CaseSensitive,
    WholeWord,
    Row {
        ticket: u64,
        index: usize,
    },
    ChooseEditor(feathrise_editor_core::ExternalEditor),
    External {
        ticket: u64,
        index: usize,
        editor: feathrise_editor_core::ExternalEditor,
        in_project: bool,
    },
    #[default]
    Inactive,
}

#[derive(Component, Default, Clone)]
pub enum SearchLabel {
    #[default]
    Toggle,
    Status,
    Scope,
    ReferenceTarget,
    PathSubmit,
    External(bool),
    Row(usize),
}
