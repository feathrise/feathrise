//! 植被侧栏六个固定回显位置，分别说明地图目标、任务和全局库诊断。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum VegetationSidebarFeedbackKind {
    #[default]
    CreationTarget,
    Availability,
    Compile,
    Preview,
    Catalog,
    Message,
}
