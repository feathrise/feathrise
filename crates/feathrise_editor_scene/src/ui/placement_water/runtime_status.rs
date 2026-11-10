//! 整网格水体属性区对正式运行时接管状态的事件驱动定向回显。

use bevy::prelude::*; // 生命周期票据、定向Query与编辑器资源写入。
use feathrise_scene_content::SceneId; // 同名placement跨场景不能共享回显信用。
use feathrise_scene::ScenePlacementRuntimeStatus;
use feathrise_scene::{DynamicWaterMeshOverrideStatus, ScenePlacementInstance}; // 只读根身份与Pending/Ready/Failed结果。 // 正式批次O(1)切门也必须重聚合活动根。

use super::super::gltf_browser::EditorGltfBrowserState; // 当前网格选择限定需要回显的摆放实例。
use super::PlacementInstanceIndex; // 生命周期索引把当前选择限制到少量并存根。
use crate::authoring::EditorAuthoringSession; // 当前草稿提供失败关闭的场景身份。
use crate::resources::SceneEditorState; // 底栏状态只由明确运行时结果更新。

/// 当前选择对应活动根的完整运行时结论。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum RuntimeSelectionSummary {
    #[default]
    Pending, // 尚无根、缺状态或任一活动根仍在验证。
    Ready,          // 全部命中的活动根都明确Ready。
    Failed(String), // 任一活动根确定失败，保留首个具体诊断。
}

#[derive(Default)]
struct RuntimeSummaryAccumulator {
    matched_root: bool,
    ready: bool,
    pending: bool,
    failure: Option<String>,
}

impl RuntimeSummaryAccumulator {
    fn observe(&mut self, status: Option<&DynamicWaterMeshOverrideStatus>) {
        self.matched_root = true;
        match status {
            Some(status) if status.failure_detail().is_some() => {
                if self.failure.is_none() {
                    self.failure = status.failure_detail().map(str::to_owned); // 首个确定失败保留精确诊断。
                }
            }
            Some(status) if status.is_ready() => self.ready = true,
            Some(_) | None => self.pending = true, // 缺状态和显式Pending都不能继承旧Ready。
        }
    }

    fn finish(self) -> RuntimeSelectionSummary {
        if let Some(detail) = self.failure {
            RuntimeSelectionSummary::Failed(detail)
        } else if self.matched_root && self.ready && !self.pending {
            RuntimeSelectionSummary::Ready
        } else {
            RuntimeSelectionSummary::Pending
        }
    }
}

/// 当前选择的运行时状态缓存；稳定帧只比较几个标量，不扫描ECS表。
#[derive(Resource, Debug, Default)]
pub(crate) struct PlacementWaterRuntimeSelectionCache {
    initialized: bool,
    scene_id: Option<SceneId>,
    placement_id: Option<String>,
    membership_revision: u64,
    status_revision: u64,
    formal_scene_id: Option<SceneId>,
    formal_revision: u64,
    formal_placement_count: usize,
    presentation_revision: u64,
    summary: RuntimeSelectionSummary,
}

impl PlacementWaterRuntimeSelectionCache {
    pub(super) fn presentation_revision(&self) -> u64 {
        self.presentation_revision // 消费者不跟随无关根的成员票据变化。
    }

    pub(super) fn label(&self) -> &'static str {
        match self.summary {
            RuntimeSelectionSummary::Pending => "运行时：正在验证/等待接管",
            RuntimeSelectionSummary::Ready => "运行时：已验证并应用",
            RuntimeSelectionSummary::Failed(_) => "运行时：验证失败（详情见底栏）",
        }
    }
}

/// 只在选择身份或对应生命周期票据变化时，定向聚合少量同ID并存根。
pub(crate) fn refresh_placement_water_runtime_selection_cache(
    browser: Res<EditorGltfBrowserState>,
    session: Res<EditorAuthoringSession>,
    placement_runtime: Res<ScenePlacementRuntimeStatus>,
    root_index: Res<PlacementInstanceIndex>,
    roots: Query<(Entity, &ScenePlacementInstance)>,
    statuses: Query<&DynamicWaterMeshOverrideStatus>,
    mut cache: ResMut<PlacementWaterRuntimeSelectionCache>,
) {
    let scene_id = session.draft.as_ref().map(|draft| &draft.scene_id);
    let placement_id = browser
        .selection
        .as_ref()
        .map(|selection| selection.placement_id.as_str());
    let membership_revision = root_index.membership_revision();
    let status_revision = scene_id
        .zip(placement_id)
        .map(|(scene_id, placement_id)| root_index.status_revision(scene_id, placement_id))
        .unwrap_or_default();
    if cache.initialized
        && cache.scene_id.as_ref() == scene_id
        && cache.placement_id.as_deref() == placement_id
        && cache.membership_revision == membership_revision
        && cache.status_revision == status_revision
        && cache.formal_scene_id == placement_runtime.scene_id
        && cache.formal_revision == placement_runtime.revision
        && cache.formal_placement_count == placement_runtime.placement_count
    {
        return; // 真正稳定帧不取得ResMut写侧，也不遍历任一placement根。
    }

    let mut summary = RuntimeSummaryAccumulator::default();
    if let (Some(scene_id), Some(placement_id)) = (scene_id, placement_id) {
        root_index.for_each_active_root(scene_id, placement_id, &roots, |entity| {
            summary.observe(statuses.get(entity).ok()); // 每个ID只检查少量活动、候选或退役并存实体。
        });
    }
    let summary = summary.finish();
    let presentation_changed = cache.scene_id.as_ref() != scene_id
        || cache.placement_id.as_deref() != placement_id
        || cache.summary != summary;
    cache.initialized = true;
    cache.scene_id = scene_id.cloned();
    cache.placement_id = placement_id.map(str::to_owned);
    cache.membership_revision = membership_revision;
    cache.status_revision = status_revision;
    cache.formal_scene_id = placement_runtime.scene_id.clone();
    cache.formal_revision = placement_runtime.revision;
    cache.formal_placement_count = placement_runtime.placement_count;
    cache.summary = summary;
    if presentation_changed {
        cache.presentation_revision = cache.presentation_revision.wrapping_add(1).max(1); // 只有用户可见语义变化才唤醒文字消费者。
    }
}

/// 监听运行时根状态的真实Changed回执，用明确成功或失败替换按钮提交阶段提示。
pub(crate) fn sync_placement_water_runtime_status(
    browser: Res<EditorGltfBrowserState>, // 只回显当前场景树选择所属实例。
    runtime_cache: Res<PlacementWaterRuntimeSelectionCache>, // 事件票据变化时才定向聚合后的结论。
    mut observed_revision: Local<u64>,    // 无关根变化不会重复覆盖作者底栏。
    mut editor: ResMut<SceneEditorState>,   // 底栏公开真实运行时验证结果。
) {
    if !editor.open {
        return; // F12关闭期间不让后台候选日志覆盖玩家下一次打开时的作者提示。
    }
    let Some(selection) = browser.selection.as_ref() else {
        return; // 没有选中网格时不把其他placement的内部状态冒充当前操作结果。
    };
    if *observed_revision == runtime_cache.presentation_revision() {
        return; // 稳定帧和无关根状态边沿都不触碰底栏字符串。
    }
    *observed_revision = runtime_cache.presentation_revision();
    if let RuntimeSelectionSummary::Failed(detail) = &runtime_cache.summary {
        editor.status = format!("网格水体运行时验证失败：{detail}"); // 缺UV、材质、歧义和预算错误不再只写日志。
    } else if runtime_cache.summary == RuntimeSelectionSummary::Ready {
        editor.status = format!(
            "网格水体已验证并应用：{} / {}",
            selection.node_path.join("/"),
            selection.mesh_name,
        ); // 只有真实Ready组件Changed后才使用成功措辞。
    } else {
        editor.status = format!(
            "网格水体正在验证/等待接管：{} / {}",
            selection.node_path.join("/"),
            selection.mesh_name,
        ); // GLB热更使活动根回到Pending时，不能残留上一轮Ready的成功提示。
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 纯状态聚合不建立Bevy App即可覆盖三个公开阶段。

    #[test]
    fn runtime_label_keeps_pending_waiting_and_failure_wins_over_ready() {
        let pending = DynamicWaterMeshOverrideStatus::Pending; // 有界队列尚未轮到当前根。
        let ready = DynamicWaterMeshOverrideStatus::Ready; // 已完成全根接管。
        let failed = DynamicWaterMeshOverrideStatus::Failed("缺少UV0".to_owned()); // 明确失败详情。

        let mut pending_only = RuntimeSummaryAccumulator::default();
        pending_only.observe(Some(&pending));
        assert_eq!(pending_only.finish(), RuntimeSelectionSummary::Pending); // Pending不得显示任何成功措辞。

        let mut ready_and_failed = RuntimeSummaryAccumulator::default();
        ready_and_failed.observe(Some(&ready));
        ready_and_failed.observe(Some(&failed));
        assert_eq!(
            ready_and_failed.finish(),
            RuntimeSelectionSummary::Failed("缺少UV0".to_owned())
        ); // 同ID活动根明确失败必须优先于Ready。
    }

    #[test]
    fn inactive_failed_or_old_ready_never_pollutes_active_status() {
        let ready = DynamicWaterMeshOverrideStatus::Ready;
        let mut active_only = RuntimeSummaryAccumulator::default();
        active_only.observe(Some(&ready)); // 根索引已在进入累加器前排除隐藏候选与退役根。
        assert_eq!(active_only.finish(), RuntimeSelectionSummary::Ready); // 单个活动Ready必须形成成功结论。

        let mut missing_status = RuntimeSummaryAccumulator::default();
        missing_status.observe(None);
        assert_eq!(missing_status.finish(), RuntimeSelectionSummary::Pending); // 新实例缺回执时不能继承旧Ready。
    }
}
