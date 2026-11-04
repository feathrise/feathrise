//! 未保存地形草稿的正式画面与物理联合接管功能域。
//!
//! GLB共享派生任务已经一次生成画面、内存高度和实际拓扑Trimesh；本域只负责把
//! 同一批候选接管到正式动态地形，并在F12关闭前跨帧确认结果已经落地。

mod handoff; // 保存后运行时GLB派生的真实时间截止。
mod helpers; // Trimesh类型与共享形状身份判断。
mod adoption; // 已跨帧实际采用的只读身份与宿主响应顺序。
mod systems; // 正式primitive接管和离图失效系统，不识别游戏角色。

pub use adoption::{TerrainLiveAdoption, TerrainLiveReceipt, TerrainLiveSet};
#[cfg(any(test, feature = "runtime_test_support"))]
pub mod test_support;
#[cfg(test)]
mod tests;

pub(crate) use systems::{
    // 作者插件只暴露调度所需的最小系统集合。
    clear_terrain_live_apply,    // 切图时使旧地图修订和文件交接身份失效。
    maintain_terrain_live_apply, // 持续核对GLB热重载没有覆盖当前草稿画面或碰撞。
};
