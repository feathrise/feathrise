//! 地表列表的稳定点击目标、局部动作和常数规模刷新快照。

use feathrise_scene_content::SceneId; // 场景切换后旧UI行立即失效。

#[derive(Clone, Debug, PartialEq, Eq)] // 比较完整身份而不是只比较数组下标。
pub(super) struct SurfaceLayerTarget {
    pub(super) scene_id: SceneId,    // 行创建时对应的作者场景。
    pub(super) revision: u64,    // 删除和重排后禁止消费旧行。
    pub(super) index: usize,     // 有效身份下以O(1)直接取层。
    pub(super) layer_id: String, // 数组位置复用也不能串选另一材质层。
} // 仅持有短身份，不复制整图像素。

#[derive(Clone, Copy, Default, PartialEq, Eq)] // BSN默认模板不扩大全局EditorAction。
pub(super) enum SurfaceLayerAction {
    #[default] // 默认只导航，未绑定模板不能默认为删除。
    PreviousPage, // 选择上一页第一层。
    NextPage,       // 选择下一页第一层。
    Create,         // 使用所选预设显式新建独立作者层。
    DeleteSelected, // 删除当前明确高亮的场景引用层。
} // 所有实际写入交由作者域统一事务。

#[derive(Default, PartialEq, Eq)] // 稳定帧只比较常数规模字段，不扫描层表或重建文字。
pub(super) struct SurfaceLayerListSnapshot {
    pub(super) open: bool,                // 重新打开页面需要刷新固定行池。
    pub(super) scene_id: Option<SceneId>,     // 切换场景时重新绑定行目标。
    pub(super) surface_revision: u64,     // 地表结构和像素采用时更新列表身份。
    pub(super) registry_revision: u64,    // 全局改名后回显最新预设名。
    pub(super) selected: usize,           // 当前选中索引决定页码。
    pub(super) preset_id: Option<String>, // 尚未使用的预设不能错误高亮旧层。
    pub(super) count: usize,              // 防御加载或初始化后结构变化。
    pub(super) pending: bool,             // 删除后台任务期间按钮给出等待反馈。
} // 不因相机或鼠标位置变化而刷新UI。
