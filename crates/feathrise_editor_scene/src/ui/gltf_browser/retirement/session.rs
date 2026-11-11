//! F12关闭时把GLB浏览器活动字段O(1)转移为一代退役工作。

use super::super::EditorGltfBrowserState; // 唯一浏览器资源同时持有活动态与退役队列。
use super::BrowserRetiredGeneration; // 所有大型字段汇入同一关闭代次。

/// F12关闭把全部拥有型会话数据转移为一代，公开行/选择/句柄索引同帧即为空。
pub(in crate::ui::gltf_browser) fn take_browser_session_generation(
    state: &mut EditorGltfBrowserState,
) -> BrowserRetiredGeneration {
    let mut generation = BrowserRetiredGeneration::default(); // 工作项个数固定，不随内容规模增长。
    generation.entries(std::mem::take(&mut state.entries)); // 公开行立即从UI资源撤下。
    generation.selection(state.selection.take()); // 对外选择立即失效。
    generation.string(state.selected_placement_id.take()); // 完整资产选择不跨F12会话。
    generation.string((!state.status.is_empty()).then(|| std::mem::take(&mut state.status)));
    generation.signature_build(state.signature_build.take()); // 取消但不整批析构未发布签名。
    generation.signatures(std::mem::take(&mut state.signature)); // 活动签名逐记录/绑定释放。
    generation.string_set(std::mem::take(&mut state.duplicate_placement_ids));
    generation.string_string_map(std::mem::take(&mut state.placement_asset_paths));
    generation.string_usize_map(std::mem::take(&mut state.placement_record_indices));
    generation.assets(std::mem::take(&mut state.assets)); // 强句柄随每个缓存完成退役后释放。
    generation.asset_path_map(std::mem::take(&mut state.asset_paths_by_id));
    generation.string_u64_map(std::mem::take(&mut state.queued_revision_by_path));
    generation.loading_queue(std::mem::take(&mut state.loading_queue));
    generation.string_usize_tree(std::mem::take(&mut state.asset_demand_counts));
    generation.string_set(std::mem::take(&mut state.asset_demand_changes));
    generation.demand_transition(state.asset_demand_transition.take());
    generation.string(
        state
            .hot_reload_sweep
            .take()
            .and_then(|sweep| sweep.after_path), // 热更游标不跨关闭会话。
    );
    generation.string_set(std::mem::take(&mut state.pending_asset_invalidations));
    generation.scene_build(state.scene_build.take());
    generation.string_set(std::mem::take(&mut state.expanded_placements));
    generation.entries_build(state.entries_build.take());
    generation.string(state.observation.draft_scene_id.take()); // 场景身份不跨F12会话。

    state.page = 0; // 固定行池下一次开启从第一页重新显示。
    state.truncated = false; // 旧场景截断诊断立即清除。
    state.initialized = false; // 退役完成后下一次开启重新观察权威草稿。
    state.draft_available = false; // 关闭态不宣称持有草稿。
    state.observation = Default::default(); // 剩余标量可同步清零。
    state.source_placement_count = 0; // 公开摘要立即变为空会话。
    state.loading_asset_count = 0; // 生命周期计数与活动assets同时清零。
    state.failed_asset_count = 0; // 旧失败摘要不跨会话。
    state.truncated_asset_count = 0; // 旧World上限诊断不跨会话。
    state.entries_revision = 0; // 新会话重新建立自身行修订链。
    generation // 调用方放入专用关闭槽，不能直接Drop。
}
