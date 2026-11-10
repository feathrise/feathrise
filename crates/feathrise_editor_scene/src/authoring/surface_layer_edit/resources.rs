//! 地表删除和共享作者图写侧准备的严格single-flight任务资源。

use super::types::SurfaceEditResult; // 引入地表作者层事务所需类型和纯助手。
use bevy::prelude::Resource; // 后台任务生命周期由当前编辑器插件统一持有。
use bevy::tasks::Task; // 帧系统只持有并非阻塞轮询纯数据完成值。
use feathrise_scene_content::SceneId; // 物化失败只影响同场景同作者版本的待补写入意图。 // 结果同时包含票据、作者身份和完整像素。

#[derive(Resource, Default)] // 关闭或切换场景时继续保留在途句柄，直到真正完成才释放槽。
pub(crate) struct SurfaceLayerEditTask {
    // 完成当前地表作者事务的受检参数或值转换。
    // 删除与独立写侧不能叠加无界整图复制。
    pub(super) task: Option<Task<SurfaceEditResult>>, // 全局最多一个纯CPU任务和一套候选像素。
    pub(super) ticket: u64,                           // 新任务单调换代，旧完成值不能冒充当前任务。
    pub(super) deleting: bool, // 保存和F12关闭需要等待作者删除完成，普通写侧只读无需等待。
    pub(super) materialize_failure: Option<(SceneId, u64, u64, String)>, // 真实分配失败交给待补笔划或撤销一次消费，禁止每帧重新OOM。
} // 任务资源结束。

impl SurfaceLayerEditTask {
    // 完成当前地表作者事务的受检参数或值转换。
    // 对外仅暴露轻量状态，调用方不能清除在途CPU任务。
    pub(crate) fn is_pending(&self) -> bool {
        // 地表作者操作在受检输入下执行当前步骤。
        // UI可以显示删除或准备画布中的状态。
        self.task.is_some() // 固定时间检查，不等待线程池。
    } // 在途状态查询结束。

    pub(crate) fn is_deleting(&self) -> bool {
        // 地表作者操作在受检输入下执行当前步骤。
        // 保存和关闭只串行等待真实作者删除。
        self.deleting && self.is_pending() // 已完成或仅物化时不制造无关保存门。
    } // 删除状态查询结束。
} // 资源查询结束。
