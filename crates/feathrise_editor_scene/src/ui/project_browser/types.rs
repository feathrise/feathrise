//! 工程目录的内部呈现来源与局部按钮动作；源码不共享资产选中身份。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ProjectBrowserView {
    Assets,
    #[default]
    ProjectFiles,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ProjectFileAction {
    #[default]
    Up,
    Refresh,
}
