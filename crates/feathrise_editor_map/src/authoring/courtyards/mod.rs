//! F12 院落组合的全局模板保存、整组受检操作和单步作者历史。
//!
//! 【功能设计】模板只保存成员局部变换；每次盖印仍产生普通地图摆放记录。
//! 选择索引分帧建立，落点与整组几何在后台校验，主线程仅在完整身份仍有效时一次采用。

use feathrise_scene_core::residency::SceneResidencyAppExt;

mod catalog; // 显式保存模板并等待公共 AssetServer 热采用。
mod geometry; // 模板局部坐标与独立地图摆放记录之间的换算。
mod operations; // 视口请求、后台整组校验及原子作者事务。
mod state; // 分帧身份索引、单在途任务和目录保存任务。
mod types; // 精确地基worker使用的有限候选与分页进度。

pub(crate) use geometry::clipmap_cursor_target;
pub(crate) use state::{CourtyardAuthoringState, queue_visual_sync}; // 整组撤销把世界同步意图投递同一有界队列。
#[cfg(test)]
pub(crate) use state::sync_visuals; // 多选回归运行同一分帧队列，不假造撤销后的模型姿态。

use bevy::prelude::*; // 与编辑器现有四阶段调度共用 Update。

use crate::MapEditorSystemSet; // 作者请求与后台采用明确排序。

/// 安装院落作者链，UI 只发送稳定 ID 和意图。
pub(crate) fn install(app: &mut App) {
    app.init_resident_resource::<state::CourtyardAuthoringState>()
        .add_systems(
            Update,
            (
                catalog::request_save_selection,
                operations::request_rotation,
            )
                .chain()
                .in_set(MapEditorSystemSet::Input), // UI 消息先冻结当前作者身份。
        )
        .add_systems(
            Update,
            (
                state::advance_selection_index,
                catalog::poll_catalog_save,
                operations::poll_operation,
                state::sync_visuals, // 一帧最多16个正式/作者根，落印不产生集中ECS峰值。
            )
                .chain()
                .in_set(MapEditorSystemSet::Tasks), // 先采用目录或操作，随后下一帧索引换代。
        )
        .add_systems(
            Update,
            operations::request_viewport
                .in_set(MapEditorSystemSet::Presentation)
                .run_if(crate::map_residency::document_active), // 光标命中采用后读取真实地形世界点。
        );
}
