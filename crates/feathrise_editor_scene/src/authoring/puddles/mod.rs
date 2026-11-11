//! 独立积水作者层入口；后台整笔处理、GPU预览与保存共享完整R8快照。
mod files;
mod helpers; // 无逐像素历史树的连续圆头笔迹纯算法。
mod image; // 预览与正式保存共用R8全Mip。
mod resources; // 单在途任务和作者笔划身份。
mod systems; // 输入与完整GPU画布采用。
mod types; // 不含ECS借用的完整版本与后台完成值。 // 后台源读取及统一事务写入。
pub(super) use files::{prepare_writes, read_source};
pub(crate) use resources::PuddleAuthoringState; // 插件与保存门共用唯一状态。
pub(crate) use systems::{apply_puddle_brush, sync_puddle_preview}; // 依次受理输入与显示完整画布。
pub(crate) use types::PuddleSnapshot; // 历史命令只保留Arc版本。 // 作者加载与保存沿用统一任务。
pub(crate) fn apply_snapshot(
    draft: &mut super::resources::EditorSceneDraft,
    snapshot: &PuddleSnapshot,
) {
    // 描述与像素必须同次恢复。
    draft.terrain_manifest_mut().puddles = snapshot.descriptor.clone(); // 小型元数据包含源与派生图的精确路径。
    draft.puddle_mask = snapshot.mask.clone(); // 全图只交换Arc，不复制像素。
    draft.puddles_dirty = true; // 撤销到空层也必须保存清单删除引用。
} // 结束原子快照采用。
#[cfg(test)] // 功能回归只在测试构建中登记。
mod tests; // 默认兼容、整笔历史和事务失败回滚。
