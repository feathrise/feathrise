//! 固定反馈位置的显示身份，与作者对象稳定ID无关；教程由问号帮助承载。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WaterSidebarFeedbackKind {
    #[default]
    Mode,
    Availability,
    Message,
    Target,
}
