//! 独立编辑器常驻工程视口的只读作者投影：固定预算捕获元数据，后台编码完整运行源。
//! 复用生产 KTX2/GLB 格式，不触发保存、不清脏位，不改变原撤销历史和工程磁盘内容。

mod capture;
mod encode;
mod resources;
mod systems;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod numeric_cancel_tests;

use bevy::prelude::*;
pub use resources::{EmbeddedPoseTargets, EmbeddedSnapshot, EmbeddedSnapshotState};

pub struct SnapshotCapturePlugin;

impl Plugin for SnapshotCapturePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<crate::authoring::AuthoringRetirement>()
            .init_resource::<EmbeddedSnapshotState>()
            .add_systems(
                Update,
                systems::advance.after(crate::MapEditorSystemSet::Tasks),
            );
    }
}

/// 发起试玩时再次核对全部来源身份和未完成手势；旧成功帧不能替当前草稿提供信用。
pub fn ready_is_current(world: &World) -> bool {
    let Some(state) = world.get_resource::<EmbeddedSnapshotState>() else {
        return false;
    };
    state.enabled
        && !state.pending
        && state.error.is_none()
        && state
            .ready
            .as_ref()
            .is_some_and(|ready| ready.author_revision == state.revision)
        && state.source == capture::source_identity(world)
        && systems::busy_reason(world).is_none()
}

/// 宿主的等待提示只读取固定状态，不扫描作者文档，也不把进度查询当作成功回执。
pub fn waiting_reason(world: &World) -> Option<&'static str> {
    let state = world.get_resource::<EmbeddedSnapshotState>()?;
    if !state.enabled || state.error.is_some() {
        return None;
    }
    if let Some(reason) = systems::busy_reason(world) {
        return Some(reason);
    }
    if !state.pending {
        return None;
    }
    Some(if state.task.is_some() {
        "正在后台编码当前作者草稿"
    } else if state.capture.is_some() {
        "正在按帧捕获当前作者草稿"
    } else {
        "正在准备当前作者草稿"
    })
}

/// 仅复制当前作者相机的小型数值；离屏图像、Entity 和 GPU 纹理都不进入进程合同。
pub fn capture_camera(world: &World) -> Option<(Transform, Projection)> {
    let camera = world
        .get_resource::<crate::resources::MapEditorViewportState>()?
        .camera;
    Some((
        *world.get::<Transform>(camera)?,
        world.get::<Projection>(camera)?.clone(),
    ))
}
