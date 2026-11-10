//! 唯一跨网格工作槽与有界刷段队列；取消任务必须完成后才释放槽位。
use super::types::Press;
use super::types::*;
use bevy::{prelude::*, tasks::Task};
use feathrise_scene_content::SceneId;
use std::collections::VecDeque;

#[derive(Resource, Default)]
pub(crate) struct VegetationPaintSettings {
    pub scene_id: Option<SceneId>,
    pub scope: PaintScope,
    pub group: Option<String>,
}
#[derive(Resource, Default)]
pub(crate) struct VegetationPaintState {
    pub(super) press: Option<Press>,
    pub(super) transaction: Option<Transaction>,
    pub(super) work: Option<Task<Result<Batch, String>>>,
    pub(super) staging: Option<Batch>,
    pub(super) queue: VecDeque<Dab>,
    pub(super) in_flight_inputs: usize, // 在途/待采用批次也占64段信用，余段回队时不能挤丢已受理输入。
    pub(super) last: Option<Vec3>,
    pub(super) waiting_exposure: f32, // 同源射线在途时保留时间积分，不把连续按住退化为间隔点按。
    pub(super) expected_revision: u64,
    pub(super) ending: bool,
    pub(super) cancelled: bool,
    pub(super) applied: bool,
    pub(super) history_written: bool, // 首次真实采用就占据历史位置，后续刷段只扩展同一命令。
    pub(super) waiting_ground:
        Option<std::sync::Arc<super::super::terrain_ground::TerrainGroundSnapshot>>,
    pub(super) refresh: std::collections::BTreeSet<feathrise_scene_content::TerrainId>,
    pub(super) refresh_work: Option<(SceneId, u64, Task<Result<Vec<PreparedCanvas>, String>>)>,
    pub(super) refresh_staging: Option<(SceneId, u64, Vec<PreparedCanvas>)>,
}
impl VegetationPaintState {
    pub(crate) fn busy(&self) -> bool {
        self.press.is_some()
            || self.transaction.is_some()
            || self.work.is_some()
            || self.staging.is_some()
            || !self.refresh.is_empty()
            || self.refresh_work.is_some()
            || self.refresh_staging.is_some()
    }
    pub(crate) fn refresh(&mut self, change: &VegetationPaintChange) {
        self.refresh.extend(change.canvases.keys().cloned());
    }
}
