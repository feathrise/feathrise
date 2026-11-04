//! 场景对象树在无构建任务时的轻量状态摘要，内置网格与外部模型共用真实对象计数。

use super::super::EditorGltfBrowserState;

/// 当前公开状态没有构建任务时，按已维护计数生成常规摘要。
pub(super) fn idle_status(state: &EditorGltfBrowserState) -> String {
    if !state.draft_available {
        return "场景对象：等待地图草稿".to_owned(); // None草稿与真实空地图明确区分。
    }
    if state.source_placement_count == 0 {
        return "场景对象：暂无".to_owned(); // 地图本身另有独立节点，零对象不代表没有网格。
    }
    let loading = state.loading_asset_count; // 生命周期切换时O(1)维护，不为状态摘要扫描全部缓存。
    let failed = state.failed_asset_count; // 失败数直接提示作者检查GLB。
    if state.assets.is_empty() {
        return format!("场景对象{}{}", state.source_placement_count,
            if state.truncated { " · 已达浏览上限" } else { "" });
    } // 内置网格没有外部文件加载门，不显示虚假的GLB 0/0状态；仅使用已维护的O(1)计数。
    let ready = state
        .assets
        .len()
        .saturating_sub(loading)
        .saturating_sub(failed); // 生命周期枚举只有三态。
    format!(
        "场景对象{} · 展开{} · GLB {ready}/{}就绪{}{}{}",
        state.source_placement_count,
        state.expanded_placements.len(),
        state.assets.len(),
        if loading > 0 {
            format!(" · {loading}加载中")
        } else {
            String::new()
        },
        if failed > 0 {
            format!(" · {failed}失败")
        } else {
            String::new()
        },
        if state.truncated {
            " · 已达浏览上限"
        } else {
            ""
        },
    ) // 全部字段都是稳定帧O(1)维护的计数，不扫描签名、缓存或可见行。
}
