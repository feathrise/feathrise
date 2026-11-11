//! 网格水属性文字的语义刷新指纹。

use feathrise_scene_content::SceneId; // 正式活动批次跨场景时必须换代。
use feathrise_scene::ScenePlacementRuntimeStatus; // 只读取公开活动批次证据，不跟随候选进度文字。

use crate::ui::gltf_browser::EditorGltfMeshSelection; // 属性文字实际消费的选择字段。

/// 不含分页、加载进度和易漂移mesh_index的属性文字选择身份。
#[derive(Clone, Debug, PartialEq, Eq)]
struct SelectedPresentationIdentity {
    placement_id: String,
    node_path: Vec<String>,
    mesh_name: String,
    valid: bool,
    orphaned_water_binding: bool,
}

impl SelectedPresentationIdentity {
    fn matches(&self, selection: &EditorGltfMeshSelection) -> bool {
        self.placement_id == selection.placement_id
            && self.node_path == selection.node_path
            && self.mesh_name == selection.mesh_name
            && self.valid == selection.valid
            && self.orphaned_water_binding == selection.orphaned_water_binding
    }
}

impl From<&EditorGltfMeshSelection> for SelectedPresentationIdentity {
    fn from(selection: &EditorGltfMeshSelection) -> Self {
        Self {
            placement_id: selection.placement_id.clone(),
            node_path: selection.node_path.clone(),
            mesh_name: selection.mesh_name.clone(),
            valid: selection.valid,
            orphaned_water_binding: selection.orphaned_water_binding,
        }
    }
}

/// Presentation本地保存的最小语义指纹；普通Resource Changed不再唤醒8192根汇总。
#[derive(Default)]
pub(crate) struct PlacementWaterTextRefreshState {
    initialized: bool,
    editor_open: bool,
    selection: Option<SelectedPresentationIdentity>,
    formal_scene_id: Option<SceneId>,
    formal_revision: u64,
    formal_placement_count: usize,
    runtime_status_revision: u64, // 当前选择的Pending/Ready/Failed语义票据。
}

impl PlacementWaterTextRefreshState {
    /// 只有文字实际依赖的选择、开关或正式活动门证据变化时返回true。
    pub(super) fn observe(
        &mut self,
        editor_open: bool,
        selection: Option<&EditorGltfMeshSelection>,
        runtime: &ScenePlacementRuntimeStatus,
        runtime_status_revision: u64,
    ) -> bool {
        let selection_matches = match (&self.selection, selection) {
            (Some(cached), Some(selection)) => cached.matches(selection),
            (None, None) => true,
            _ => false,
        };
        if self.initialized
            && self.editor_open == editor_open
            && selection_matches
            && self.formal_scene_id == runtime.scene_id
            && self.formal_revision == runtime.revision
            && self.formal_placement_count == runtime.placement_count
            && self.runtime_status_revision == runtime_status_revision
        {
            return false; // 候选phase/detail/attempt与浏览分页、状态文字变化不属于属性文字语义。
        }
        self.initialized = true;
        self.editor_open = editor_open;
        self.selection = selection.map(SelectedPresentationIdentity::from);
        self.formal_scene_id = runtime.scene_id.clone();
        self.formal_revision = runtime.revision;
        self.formal_placement_count = runtime.placement_count;
        self.runtime_status_revision = runtime_status_revision;
        true
    }
}

#[cfg(test)]
mod tests {
    use feathrise_scene::ScenePlacementRuntimePhase;

    use super::*;

    #[test]
    fn candidate_progress_is_not_a_formal_gate_change() {
        let mut state = PlacementWaterTextRefreshState::default();
        let mut runtime = ScenePlacementRuntimeStatus {
            phase: ScenePlacementRuntimePhase::Building,
            detail: "候选进度1/8192".to_owned(),
            attempt_revision: 7,
            ..Default::default()
        };
        assert!(state.observe(true, None, &runtime, 0));

        runtime.detail = "候选进度2/8192".to_owned();
        assert!(!state.observe(true, None, &runtime, 0)); // 每帧进度文字不得触发正式根扫描。
        runtime.attempt_revision = 8;
        assert!(!state.observe(true, None, &runtime, 0)); // 新隐藏候选同样不改变当前活动门。
        runtime.phase = ScenePlacementRuntimePhase::Failed;
        assert!(!state.observe(true, None, &runtime, 0)); // 候选失败保留旧活动根，不污染属性回显。

        runtime.revision = 1;
        runtime.placement_count = 32;
        assert!(state.observe(true, None, &runtime, 0)); // 原子采用推进公开正式修订时必须刷新。
        assert!(state.observe(true, None, &runtime, 1)); // 当前选择运行时状态边沿独立唤醒属性文字。
    }
}
