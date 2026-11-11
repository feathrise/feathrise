//! Scene0资产事件的固定预算收集与突发事件全缓存游标。

use std::collections::BTreeSet; // 单帧只聚合固定数量唯一命中路径。
use std::ops::Bound::{Excluded, Unbounded}; // 溢出后按BTreeMap路径游标渐进失效。

use bevy::asset::{AssetEvent, AssetId}; // 标准WorldAsset热更消息与稳定资产身份。
use bevy::world_serialization::WorldAsset; // Scene0缓存资产类型。

use super::super::{
    BrowserHotReloadSweep, EditorGltfBrowserState, MAX_GLTF_ASSET_EVENTS_PER_FRAME,
}; // 活动反向索引与固定事件预算集中由浏览器资源维护。

/// 提取标准WorldAsset事件的稳定资产ID。
fn event_asset_id(event: &AssetEvent<WorldAsset>) -> AssetId<WorldAsset> {
    match event {
        AssetEvent::Added { id }
        | AssetEvent::Modified { id }
        | AssetEvent::Removed { id }
        | AssetEvent::LoadedWithDependencies { id }
        | AssetEvent::Unused { id } => *id, // 五种事件共享同一类型化ID。
    }
}

/// 读取固定数量热更事件；事件突发时改为有游标的全缓存渐进失效。
pub(super) fn collect_invalidated_paths(
    scene_events: &mut feathrise_scene_core::residency::ResidentAssetEventReader<WorldAsset>,
    state: &mut EditorGltfBrowserState,
) -> BTreeSet<String> {
    let mut invalidated = BTreeSet::new();
    let overflowed;
    {
        let mut events = scene_events.read();
        for _ in 0..MAX_GLTF_ASSET_EVENTS_PER_FRAME {
            let Some(event) = events.next() else {
                break;
            };
            if !matches!(event, AssetEvent::Unused { .. })
                && let Some(path) = state.asset_paths_by_id.get(&event_asset_id(event))
            {
                invalidated.insert(path.clone()); // 单条事件直接走O(1)反向索引。
            }
        }
        overflowed = events.next().is_some(); // 只多读一条判断是否需要保守全量失效。
    }
    if overflowed {
        scene_events.clear(); // 丢弃的尾部由下方全缓存渐进失效覆盖。
        state.hot_reload_sweep = Some(BrowserHotReloadSweep::default());
        if let Some(path) = state
            .scene_build
            .as_ref()
            .map(|build| build.asset_path.clone())
        {
            if !invalidated.contains(&path) && invalidated.len() >= MAX_GLTF_ASSET_EVENTS_PER_FRAME
            {
                invalidated.pop_last(); // 全缓存sweep仍会覆盖被让位的普通事件路径。
            }
            invalidated.insert(path); // 优先取消唯一活动World读取，避免跨修订混读。
        }
    }

    let mut remaining = MAX_GLTF_ASSET_EVENTS_PER_FRAME.saturating_sub(invalidated.len());
    while remaining > 0 {
        let Some(sweep) = state.hot_reload_sweep.as_mut() else {
            break;
        };
        let next_path = match &sweep.after_path {
            Some(after) => state
                .assets
                .range((Excluded(after.clone()), Unbounded))
                .next()
                .map(|(path, _)| path.clone()),
            None => state.assets.keys().next().cloned(),
        };
        let Some(path) = next_path else {
            state.hot_reload_sweep = None; // 游标已越过当前缓存末尾。
            break;
        };
        sweep.after_path = Some(path.clone()); // 先推进游标，后续释放该路径也不影响继续。
        invalidated.insert(path);
        remaining -= 1;
    }
    invalidated
}
