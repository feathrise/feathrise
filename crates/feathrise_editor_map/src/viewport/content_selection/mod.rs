//! 中央视口对显式摆放对象、统一水域对象和刷制植被整层的点击选择。
//!
//! 【功能设计】显式 Scene0 primitive 通过真实 Mesh 射线恢复稳定摆放/节点身份；直接primitive
//! 通过生成侧专用标记只恢复完整摆放根，不伪造Scene0身份。逐株 ECS Mesh优先读取运行时层身份，
//! GPU Blade 与动态批次则从地形落点采样正式 R8。两条植被路径最终都只选择正式输出层并复用
//! 既有密度覆盖，不尝试选中或高亮某一株实例。静水、河流和瀑布则从统一水面Mesh marker恢复
//! 类别与稳定ID，选择完整作者对象并复用左栏列表及精确水面高亮。

mod gameplay; // 传送柱体与边界墙段纯几何命中及有界输入快照。
mod helpers; // 密度采样、作者层锚点和重叠候选循环保持为无ECS系统参数纯逻辑。
mod layer_apply; // 一个出口同步侧栏与正式作者层。
mod layer_canvas; // 最近真实网格与其作者画布续接，采样统一转换到源局部坐标。
mod layer_query; // 真实作者贡献与角色射线候选在后台完整解析。
mod layer_source; // 同批候选冻结来源与当前正式画布分离，恢复前校验完整缓存及作者下标。
mod layer_ui;
mod layers; // 跨类型作者候选的single-flight与正式/临时选择身份。
mod placement; // Scene0精确网格与直接primitive整项选择分离，不伪造跨形态身份。
mod systems; // 点击边沿射线、Scene0父链恢复以及UI选择同步集中在唯一输入系统。
mod water; // 水面marker到三类作者对象、列表和高亮的稳定选择发布保持独立职责。 // 点击旁固定八行候选浮层。

pub(crate) use layer_apply::{apply_layer_candidate, sync_sidebar_layer_selection}; // 侧栏选层也接入唯一高亮身份。
pub(crate) use layer_query::poll_layer_candidates; // 在射线请求之后启动和轮询。
pub(crate) use layer_ui::{
    LayerCandidateSlot, LayerCandidatePopup, layer_candidate_interactions, sync_layer_candidate_ui,
}; // 场景页挂载候选卡片。
pub use layers::LayerCandidateState;
pub(crate) use layers::LayerTarget; // 普通地图各类内部选择身份不属于外部作者合同。

pub(crate) use systems::select_editor_viewport_content; // 插件在相机发布本帧光标落点后执行选择。

mod sub_maps;
#[cfg(test)]
mod layer_tests;
#[cfg(test)]
mod tests; // 正式层采样、预览层身份、作者锚点和候选循环由纯逻辑回归锁定。 // 跨类型候选临时覆盖、确认与取消换代回归。
