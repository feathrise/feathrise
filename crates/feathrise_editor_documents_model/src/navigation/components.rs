//! 文档导航条、原生单行输入与可见行标记的私有组件。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub enum NavigationAction {
    #[default]
    Find,
    Replace,
    Go,
    Previous,
    Next,
    SubmitLine,
    Close,
    CaseSensitive,
    WholeWord,
    InSelection,
    ReplaceCurrent,
    ReplaceAll,
}
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub enum NavigationField {
    #[default]
    Query,
    Replacement,
    Line,
}
#[derive(Component, Default, Clone)]
pub struct NavigationTools;
#[derive(Component, Default, Clone)]
pub struct NavigationFindBar;
#[derive(Component, Default, Clone)]
pub struct NavigationReplaceBar;
#[derive(Component, Default, Clone)]
pub struct NavigationSearchHint;
#[derive(Component, Default, Clone)]
pub struct NavigationGoBar;
#[derive(Component, Default, Clone)]
pub struct NavigationCount;
#[derive(Component, Default, Clone)]
pub struct NavigationLineHint;
#[derive(Component, Default, Clone)]
pub struct NavigationGutter;
#[derive(Component, Default, Clone)]
pub struct NavigationMatchLayer;
#[derive(Component, Default, Clone)]
pub struct NavigationOverview;
#[derive(Component, Default, Clone)]
pub struct NavigationRow(pub usize);
#[derive(Component, Default, Clone)]
pub struct NavigationHighlight(pub usize);
