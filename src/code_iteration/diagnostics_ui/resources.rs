//! 可见诊断行的窗口绑定独立于后台编译修订。

use bevy::prelude::*;

pub(super) const ROW_HEIGHT: f32 = 44.;
// 弹窗整体最高700逻辑像素；24槽覆盖正文最大可见行并保留上下过扫，文件量不增加实体。
pub(super) const ROW_POOL: usize = 24;

#[derive(Resource, Default)]
pub(crate) struct DiagnosticsOverlayState {
    pub(super) revision: Option<u64>,
    pub(super) first: usize,
    pub(super) scroll: Vec2,
    pub(super) window_revision: u64,
}
