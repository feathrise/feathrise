//! 地表作者层删除、整图索引重映射与共享画布写侧的单任务事务入口。

mod helpers; // 纯像素重映射、快照校验与历史恢复算法。
mod resources; // 删除与写侧物化共用一个后台任务槽。
mod systems; // 冻结输入、非阻塞轮询与同修订原子采用。
mod types; // 完整地表前后快照与后台结果普通类型。

pub(super) use helpers::apply_surface_authoring_change; // 统一撤销系统恢复同一份定义和两张作者图。
pub(crate) use resources::SurfaceLayerEditTask; // 插件只注册一个有界后台资源。
pub(super) use systems::start_surface_materialization_if_shared; // 笔刷和局部撤销共用后台独立写侧。
pub(crate) use systems::{poll_surface_layer_edit, start_surface_layer_delete}; // UI提交稳定层身份，插件持续轮询。
pub(super) use types::{PendingSurfaceBrushSample, SurfaceAuthoringChange}; // 命令持有浅快照，笔刷冻结固定大小待补段。

#[cfg(test)] // 算法、生命周期与精确历史回归不进入正式构建。
mod tests; // 测试按纯数据边界验证失败时绝不发布半套地表。
