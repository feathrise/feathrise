//! 工程页固定节点和动作标记；原场景/预设组件保持私有且不被宿主重建。

use super::{Page, WorkspaceField};
use bevy::prelude::*;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    ToggleMenu,
    Show(Page),
    CloseTab(Page),
    Create,
    SelectGameView(feathrise_runtime::GameView),
    Open,
    CloseProject,
    ShowAbout,
    CloseAbout,
    OpenLicenses,
    StepLicensePage(isize),
    QuitApp,
    SetStartup,
    ExportProject,
    Plugins,
    Browse(WorkspaceField),
    SaveDefault,
    ResetDefault,
    CancelOperation,
}

#[derive(Component)]
pub(super) struct RecentAction {
    pub path: std::path::PathBuf,
    pub kind: RecentActionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RecentActionKind {
    Open,
    Remove,
    SetPinned(bool),
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Input {
    Name,
    Parent,
    Open,
    DefaultDirectory,
}

#[derive(Component)]
pub(super) struct PageNode(pub Page);
#[derive(Component)]
pub(super) struct TabNode(pub Page);
#[derive(Component)]
pub(super) struct RecentHost;
#[derive(Component)]
pub(super) struct RecentRoot;
#[derive(Component)]
pub(super) struct TargetText;
#[derive(Component)]
pub(super) struct StatusText;
#[derive(Component)]
pub(super) struct MenuNode;

#[derive(Component)]
pub(super) struct AboutOverlay;
#[derive(Component)]
pub(super) struct AboutPanel;
#[derive(Component)]
pub(super) struct AboutHeader;
#[derive(Component)]
pub(super) struct AboutTitle;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum AboutBody { Overview, Licenses }
#[derive(Component)]
pub(super) struct AboutFooter;
#[derive(Component)]
pub(super) struct AboutLicenseText;
#[derive(Component)]
pub(super) struct AboutLicenseRegion;
#[derive(Component)]
pub(super) struct AboutLicenseScrollbar;
#[derive(Component)]
pub(super) struct AboutLicenseToggleLabel;
