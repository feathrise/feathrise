//! 植被作者图层编译任务与完成值资源。

use std::collections::{BTreeMap, BTreeSet}; // 正式输出、非零身份和冲突图按稳定ID保存，结果采用顺序确定。
use std::sync::Arc; // 完成值移动共享像素，不在主线程复制整张密度图。

use bevy::prelude::*; // SceneId、Resource和Task。
use bevy::tasks::Task; // 同一场景最多一个在途完整编译任务。
use feathrise_scene_content::{SceneId, TerrainId}; // 同场景不同网格仍是不同作者画布。

/// 像素刷印不改变来源；结构或画布换代后，旧完整结果也不能借用新网格的预览槽。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct VegetationCompileSource {
    pub(crate) scene_id: SceneId,
    pub(crate) terrain: Option<TerrainId>,
    pub(crate) structure_revision: u64,
}
impl VegetationCompileSource {
    pub(crate) fn capture(session: &crate::authoring::EditorAuthoringSession) -> Option<Self> {
        let draft = session.draft.as_ref()?;
        if draft.vegetation.is_none() || draft.terrain_documents.loading
            || (draft.manifest.terrains.is_some() && draft.terrain_documents.active.is_none()) { return None; }
        Some(Self { scene_id: draft.scene_id.clone(),
            terrain: draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone()),
            structure_revision: session.vegetation_structure_revision })
    }
}

/// 一张带显式分辨率的R8完成图。
#[derive(Clone)]
pub(crate) struct CompiledDensityImage {
    /// R8宽高。
    pub(crate) resolution: [u32; 2],
    /// 完整连续R8像素。
    pub(crate) pixels: Arc<Vec<u8>>,
}

/// 一张正式R8密度图在后台编译完成时顺带得到的常数大小统计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CompiledDensityStats {
    /// 正式输出包含的全部像素数。
    pub(crate) total_pixels: u64,
    /// 密度大于零的像素数。
    pub(crate) nonzero_pixels: u64,
    /// 全图R8密度值之和；用于无损计算全图与非零区域平均值。
    pub(crate) density_sum: u64,
    /// 非零像素中的最小R8值；全零图保持None。
    pub(crate) min_nonzero: Option<u8>,
    /// 全图最大R8值；全零图为零。
    pub(crate) max_value: u8,
}

/// 一次作者图层编译的原子完整结果。
pub(crate) struct CompiledVegetation {
    /// 正式运行时层ID到最终R8密度。
    pub(crate) outputs: BTreeMap<String, Arc<Vec<u8>>>,
    /// 至少包含一个非零像素的正式输出ID；由后台顺带计算，主线程不得重扫整图。
    pub(crate) nonzero_output_ids: BTreeSet<String>,
    /// 每个正式输出的密度统计与R8像素来自同一次后台编译完成值。
    pub(crate) output_stats: BTreeMap<String, CompiledDensityStats>,
    /// 作者组ID到“同像素至少两层有需求”的诊断覆盖。
    pub(crate) conflicts: BTreeMap<String, CompiledDensityImage>,
}

/// 后台任务返回场景、修订和完整成功或失败值。
pub(crate) type VegetationCompileTaskResult = (VegetationCompileSource, u64, Result<CompiledVegetation, String>);

/// 植被作者图层single-flight编译状态。
#[derive(Resource, Default)]
pub(crate) struct VegetationCompileState {
    /// 最新期望场景身份。
    pub(crate) desired_scene_id: Option<SceneId>,
    /// 最新期望作者修订。
    pub(crate) desired_revision: u64,
    pub(crate) desired_source: Option<VegetationCompileSource>,
    /// 当前唯一在途CPU任务。
    pub(crate) task: Option<Task<VegetationCompileTaskResult>>,
    /// 最近一次确定失败所属场景。
    pub(crate) failed_scene_id: Option<SceneId>,
    /// 最近一次确定失败修订，避免稳定帧无条件重试坏输入。
    pub(crate) failed_revision: Option<u64>,
}

impl VegetationCompileState {
    /// 切换场景或关闭会话时仅使身份失效；在途CPU任务自然完成后再释放并丢弃。
    pub(crate) fn invalidate(&mut self) {
        self.desired_scene_id = None; // 迟到结果不再匹配任何活动场景。
        self.desired_revision = 0; // 清除旧作者修订。
        self.desired_source = None;
        self.failed_scene_id = None; // 新场景允许首次尝试。
        self.failed_revision = None; // 新场景不继承旧错误门。
    }
}
