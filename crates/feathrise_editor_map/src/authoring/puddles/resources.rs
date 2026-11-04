//! 积水作者单在途后台任务、整笔历史与GPU图像编译状态。
use super::types::{PaintResult, PuddleSnapshot, Sample};
use bevy::prelude::*; // 资源、世界点和Image强句柄。
use bevy::tasks::Task; // 每类最多一个运行中任务。
use feathrise_map_content::MapId; // 跨图迟到结果必须拒收。
use std::sync::Arc; // 保留实际源对象身份避免地址复用误判。 // 独立普通类型。
#[derive(Resource, Default)] // 编辑器初始没有活动笔划或图像。
pub(crate) struct PuddleAuthoringState {
    // 笔划和预览拥有固定任务容量。
    pub(super) task: Option<Task<PaintResult>>, // 全图写侧复制与笔刷运算只在后台。
    pub(super) completed: Option<PaintResult>, // 高度身份暂不可查询时保留唯一完整候选，继续占用作者单飞槽。
    pub(super) map_id: Option<MapId>,          // 本笔归属，换图立即失效。
    pub(super) before: Option<PuddleSnapshot>, // 首次真实操作前版本，一笔仅压一个历史。
    pub(super) pending_ground: Option<(MapId, u64)>, // 精确落点未到时仅保留当前地图和高度修订的按压意图。
    pub(super) ground_content: Option<[u64; 7]>, // 待补段和后台结果必须属于开始时的作者高度内容。
    pub(super) expected_revision: u64, // 本笔自己推进后的完整作者版本，外部修改取消旧尾段。
    pub(super) current: Option<PuddleSnapshot>, // 只用本笔最后完整采用的画布收口，不能把外部新像素混入撤销。
    pub(super) pending: Option<Sample>,         // 在途时只保留一个合并尾段。
    pub(super) last: Option<Vec3>,              // 最后受理点，下一后台块沿线补点。
    pub(super) released: bool,                  // 松手后继续消费已受理尾段，再提交整笔。
    pub(super) changed: bool,                   // 累计真实变化而非鼠标移动。
    pub(super) preview_task: Option<Task<(MapId, Arc<Vec<u8>>, [u32; 2], Result<Image, String>)>>, // 图像分配和复制同样后台完成。
    pub(super) preview_failure: Option<(MapId, Arc<Vec<u8>>, [u32; 2], String)>, // 失败绑定完整源身份，避免稳定帧无限重试。
    pub(super) preview_source: Option<Arc<Vec<u8>>>, // 已发布完整版本的真实Arc身份。
    pub(super) preview_resolution: Option<[u32; 2]>, // 相同Arc但声明尺寸变化时仍须重新准备Image。
    pub(super) preview_map: Option<MapId>,           // 发布画布必须属于当前地图。
    pub(super) image: Option<Handle<Image>>,         // 强句柄维持已发布GPU纹理。
} // 结束唯一状态资源。
impl PuddleAuthoringState {
    // 保存及历史门只读取常数大小状态。
    pub(crate) fn preview_pending_for(
        &self,
        draft: &super::super::resources::EditorMapDraft,
    ) -> bool {
        // 关闭玩法测试前等待当前完整GPU画布。
        draft.puddle_mask.as_ref().is_some_and(|source| {
            self.preview_map.as_ref() != Some(&draft.map_id)
                || self.preview_resolution
                    != draft
                        .terrain_manifest()
                        .puddles
                        .as_ref()
                        .map(|layer| layer.resolution)
                || !self
                    .preview_source
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(source, current))
        }) // 常数大小身份比较，不扫描像素。
    } // 结束预览完成门。
    pub(crate) fn preview_error_for(
        &self,
        draft: &super::super::resources::EditorMapDraft,
    ) -> Option<&str> {
        // 只报告当前作者版本的确定失败。
        self.preview_failure
            .as_ref()
            .and_then(|(map_id, source, resolution, error)| {
                // 不把旧地图错误带入新草稿。
                (map_id == &draft.map_id
                    && draft
                        .terrain_manifest()
                        .puddles
                        .as_ref()
                        .is_some_and(|layer| layer.resolution == *resolution)
                    && draft
                        .puddle_mask
                        .as_ref()
                        .is_some_and(|mask| Arc::ptr_eq(mask, source)))
                .then_some(error.as_str()) // 常数时间身份门。
            }) // 其他版本可正常重新准备。
    } // 结束失败身份查询。
    pub(crate) fn busy(&self) -> bool {
        // 未收口的整笔不能保存半个版本。
        self.before.is_some()
            || self.task.is_some()
            || self.completed.is_some()
            || self.pending.is_some() // GPU预览任务不修改作者源。
    } // 结束活动判定。
} // 结束状态行为。
