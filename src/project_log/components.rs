//! 只读工程日志浮窗的固定节点与动作；不发布启动、停止或工程写入命令。

use bevy::prelude::*;

#[derive(Component)]
pub(super) struct LogOverlay;
#[derive(Component)]
pub(super) struct LogPanel;
#[derive(Component)]
pub(super) struct LogHeader;
#[derive(Component)]
pub(super) struct LogTerminalTab;
#[derive(Component)]
pub(super) struct LogFooter;
#[derive(Component)]
pub(super) struct LogToolButton;
#[derive(Component)]
pub(super) struct LogFollowButton;
#[derive(Component)]
pub(super) struct LogWrapButton;
#[derive(Component)]
pub(super) struct LogCloseButton;
#[derive(Component)]
pub(super) struct LogScroll;
#[derive(Component, Default)]
pub(super) struct LogBody {
    /// 虚拟正文的全局字节起点；换页只重排可见行，选区仍属于完整日志。
    pub origin: u64,
    pub read_selection: bool,
    pub extend_selection: bool,
}
/// 只负责富色字形；所有命中、选区和复制仍由同位置的原生正文处理。
#[derive(Component, Default)]
pub(super) struct LogDisplay {
    pub fingerprint: Option<u64>,
    /// 字形仍可处于上一折行修订。查找不能把新正文的字节范围套到旧页。
    pub origin: u64,
    pub revision: u64,
}
#[derive(Component)]
pub(super) struct LogRun;
#[derive(Component)]
pub(super) struct LogContent;
#[derive(Component)]
pub(super) struct LogScrollbar;
#[derive(Component)]
pub(super) struct LogHorizontalScrollbar;
#[derive(Component)]
pub(super) struct LogScrollThumb;
#[derive(Component)]
pub(super) struct LogStatus;
#[derive(Component)]
pub(super) struct LogFollowLabel;
#[derive(Component)]
pub(super) struct LogWrapLabel;
#[derive(Component)]
pub(super) struct LogSearchBar;
#[derive(Component)]
pub(super) struct LogSearchInput;
#[derive(Component)]
pub(super) struct LogSearchHint;
#[derive(Component)]
pub(super) struct LogSearchCount;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum LogAction { Close, Follow, Wrap, Clear, CopySelection, CopyAll, Search, SearchPrevious, SearchNext }
