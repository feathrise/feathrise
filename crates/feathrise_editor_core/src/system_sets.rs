//! 工作区与系统阶段的唯一类型身份，独立插件共享同一提交屏障。

use bevy::prelude::*;

/// F12地图编辑器系统阶段，保证输入、阻塞、IO和UI回显顺序稳定。
#[derive(SystemSet, Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum MapEditorSystemSet {
    /// 收集F12和按钮请求并同步玩法输入阻塞。
    Input,
    /// 非阻塞轮询后台任务并原子采用完整结果。
    Tasks,
    /// 在草稿采用、视口尺寸和地图取景完成后计算本帧命中并发布自定义光标。
    Cursor,
    /// 把最终资源状态回显到静态BSN和固定行池。
    Presentation,
}

/// 整页换代先完成命令提交，再执行技能局部回显。
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PresetWorkspaceRebuildSet;
