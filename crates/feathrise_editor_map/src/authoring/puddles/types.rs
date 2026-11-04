//! 积水后台任务与撤销所需普通数据；不保存World或资产可变借用。
use bevy::prelude::*; // 世界位置与CPU图像完成值。
use feathrise_map_content::{MapId, MapPuddleDescriptor};
use std::sync::Arc; // 完整作者图像只共享不可变数据。 // 稳定地图身份和独立作者协议。
#[derive(Clone, Default)] // 首次落笔前的空层也可精确撤销。
/// 外部作者只携带该历史值；内容和创建仍由普通编辑器自己的受检事务掌握。
pub struct PuddleSnapshot {
    // 两个字段共同定义完整作者版本。
    pub(crate) descriptor: Option<MapPuddleDescriptor>, // None表示地图未创建此层。
    pub(crate) mask: Option<Arc<Vec<u8>>>,              // 对应独立R8作者源。
} // 结束版本定义。
impl PuddleSnapshot {
    pub(super) fn is_current(&self, draft: &super::super::resources::EditorMapDraft) -> bool {
        self.descriptor == draft.terrain_manifest().puddles
            && match (&self.mask, &draft.puddle_mask) {
                (Some(expected), Some(current)) => Arc::ptr_eq(expected, current),
                (None, None) => true,
                _ => false,
            }
    } // 常数大小来源比较，外部重载不能混入旧笔历史。
}
pub(super) struct PaintResult {
    // 后台结果携带不能省略的完整输入身份。
    pub map_id: MapId,                    // 不采用另一张地图的结果。
    pub revision: u64,                    // 不覆盖并发作者更改。
    pub ground_content: Option<[u64; 7]>, // Clipmap高度换代后拒收迟到像素，GLB保持原修订门。
    pub source: PuddleSnapshot,           // 浅共享完整输入，实际作者源变化时不能仅凭修订号采用。
    pub snapshot: PuddleSnapshot,         // 完整下一版本。
    pub changed: bool,                    // 零强度及无交集不制造历史。
} // 结束后台笔刷结果。
#[derive(Clone, Copy)] // 最多只合并一个尾样本，形成固定背压。
pub(super) struct Sample {
    // 已经在视口中确认的落点与参数。
    pub world: Vec3, // 米制地形命中。
    pub radius: f32, // 公共笔刷半径。
    pub amount: f32, // 按真实时间累计的强度。
    pub erase: bool, // Shift明确擦除到零。
} // 结束采样参数。
