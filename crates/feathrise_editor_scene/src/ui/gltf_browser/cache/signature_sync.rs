//! 浏览器摆放结构观察、分帧签名与资产需求原子过渡。

use super::super::demand::{observe_browser_session, session_change_requires_signature_refresh}; // 非结构数值修改只做O(1)观察。
use super::super::entries::request_visible_entries_rebuild; // 完整新签名采用后才请求行树换代。
use super::super::retirement::BrowserRetiredGeneration; // 结构换代不直接析构8192项旧签名/索引。
use super::super::signature::{BrowserSignatureBuild, advance_signature_build}; // 每帧固定记录预算。
use super::super::{
    BrowserAssetDemandTransition, EditorGltfBrowserState, MAX_GLTF_BROWSER_PLACEMENTS,
}; // 活动签名与需求过渡仍由浏览器唯一资源拥有。
use crate::authoring::EditorAuthoringSession; // 草稿摆放表只在分帧签名任务中读取。

/// 建立、推进并原子采用确定性摆放结构签名；单帧最多读取固定数量记录。
pub(super) fn synchronize_signature_build(
    session: &EditorAuthoringSession,
    session_changed: bool,
    state: &mut EditorGltfBrowserState,
) {
    let inspect = !state.initialized || session_changed || state.signature_build.is_some();
    if !inspect {
        return; // 稳定帧及流速/高度拖动之外不读取摆放Vec。
    }
    let next_observation = observe_browser_session(session); // 只读取scene_id、len和显式结构代次。
    let running_matches = state
        .signature_build
        .as_ref()
        .is_some_and(|build| build.observation == next_observation);
    let needs_refresh = !state.initialized
        || session_change_requires_signature_refresh(&state.observation, &next_observation);
    if !running_matches && (state.signature_build.is_some() || needs_refresh) {
        let scene_changed = state.observation.draft_scene_id != next_observation.draft_scene_id;
        let needs_retirement = state.signature_build.is_some()
            || state.entries_build.is_some()
            || (scene_changed && !state.expanded_placements.is_empty());
        if needs_retirement && !state.retirement.can_enqueue_normal() {
            state.status = "场景树：等待上一代分帧释放，最新场景结构已合并".to_owned();
            return; // 保留唯一旧草稿；下帧重新观察最新确定性代次。
        }
        let mut retired = BrowserRetiredGeneration::default();
        retired.signature_build(state.signature_build.take()); // 取消草稿逐记录/索引清理。
        retired.entries_build(state.entries_build.take()); // 旧行草稿逐条清理。
        if scene_changed {
            retired.string_set(std::mem::take(&mut state.expanded_placements)); // 展开偏好不跨场景且不整批clear。
            retired.selection(state.selection.take()); // 公开选择同帧立即失效，深路径随后释放。
            retired.string(state.selected_placement_id.take()); // 完整实例选择同样不能跨场景复用。
            state.page = 0; // 新场景从第一页开始。
        }
        state.retirement.enqueue_normal_reserved(retired); // release构建也真实接管取消草稿。
        state.signature_build = Some(BrowserSignatureBuild::new(next_observation));
    }
    let Some(mut build) = state.signature_build.take() else {
        return; // Changed只来自非结构字段时仍保持O(1)短路。
    };
    let placements = session
        .draft
        .as_ref()
        .map(|draft| draft.placements.placements.as_slice())
        .unwrap_or_default();
    let completed = advance_signature_build(&mut build, placements, &state.expanded_placements);
    if !completed {
        state.status = format!(
            "场景树：正在读取实例结构 {}/{}",
            build.next_placement,
            build
                .observation
                .source_placement_count
                .min(MAX_GLTF_BROWSER_PLACEMENTS),
        );
        state.signature_build = Some(build); // 保存精确游标供下一Update继续。
        return;
    }
    if state.asset_demand_transition.is_some() || !state.asset_demand_changes.is_empty() {
        state.status = "场景树：实例结构已读取，正在收口上一批资产需求".to_owned();
        state.signature_build = Some(build); // 不覆盖尚未完成的旧路径过渡集合。
        return;
    }

    if !state.retirement.can_enqueue_normal() {
        state.status = "场景树：新实例结构已完成，等待上一代签名分帧释放".to_owned();
        state.signature_build = Some(build); // 完整新代及其8192项seen_ids保持唯一所有权，不重复构建。
        return;
    }

    let mut retired = BrowserRetiredGeneration::default();
    retired.signatures(std::mem::take(&mut state.signature)); // 活动8192项签名逐记录/水绑定释放。
    retired.string_set(std::mem::take(&mut state.duplicate_placement_ids));
    retired.string_string_map(std::mem::take(&mut state.placement_asset_paths));
    retired.string_usize_map(std::mem::take(&mut state.placement_record_indices));
    retired.string_set(std::mem::take(&mut state.expanded_placements));
    retired.string(state.observation.draft_scene_id.take());

    let stale_paths = std::mem::replace(
        &mut state.asset_demand_counts,
        std::mem::take(&mut build.asset_demand_counts),
    ); // BTreeMap整体移动为O(1)，不复制全部路径。
    let wanted_paths = std::mem::take(&mut build.wanted_asset_paths); // 新需求随后每帧最多调和固定数量路径。
    state.asset_demand_transition = (!stale_paths.is_empty() || !wanted_paths.is_empty())
        .then_some(BrowserAssetDemandTransition {
            stale_paths,
            wanted_paths,
        });
    let next_observation = std::mem::take(&mut build.observation); // 采用字段逐个移动，保留草稿壳供辅助索引退役。
    state.initialized = true;
    state.draft_available = next_observation.draft_available;
    state.source_placement_count = next_observation.source_placement_count;
    state.observation = next_observation;
    state.signature = std::mem::take(&mut build.signature);
    state.has_object_groups = build.has_object_groups;
    state.duplicate_placement_ids = std::mem::take(&mut build.duplicate_ids);
    state.placement_asset_paths = std::mem::take(&mut build.placement_asset_paths);
    state.placement_record_indices = std::mem::take(&mut build.placement_record_indices);
    state.expanded_placements = std::mem::take(&mut build.expanded_live);
    if state
        .selected_placement_id
        .as_ref()
        .is_some_and(|placement_id| !state.placement_record_indices.contains_key(placement_id))
    {
        retired.string(state.selected_placement_id.take()); // 删除或重复ID换代后同帧撤销悬空作者目标。
        retired.selection(state.selection.take()); // 所属网格选择也不能继续写入另一条记录。
    }
    retired.signature_build(Some(build)); // seen_ids等仅用于构建的8192项辅助数据逐项释放。
    state.retirement.enqueue_normal_reserved(retired); // 旧活动代和完成草稿辅助索引共享唯一退役代。
    request_visible_entries_rebuild(state); // 完整签名与全部辅助索引已经原子采用。
}
