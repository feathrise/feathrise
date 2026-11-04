//! 一帧释放点击与可失效身份；业务不得再把真实Pressed当作普通按钮动作。

use bevy::prelude::*;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EditorButtonClick {
    pub(super) clicked: bool,
    pub(super) generation: u64,
}

impl EditorButtonClick {
    pub fn is_clicked(&self) -> bool {
        self.clicked
    }

    /// 已确认的动作队列或无窗口夹具可转发脉冲；真实指针只由共享系统在合法释放后发布。
    pub fn triggered() -> Self {
        Self {
            clicked: true,
            generation: 0,
        }
    }

    /// 动态行换绑时取消旧手势，不能让同一实体的松手改派给新地图、文件或对象。
    pub fn invalidate(&mut self) {
        self.clicked = false;
        self.generation = self.generation.wrapping_add(1);
    }
}
