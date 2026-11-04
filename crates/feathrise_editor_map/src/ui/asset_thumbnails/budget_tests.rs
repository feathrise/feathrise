//! 超高连续窗口的缩略图缓存预算与滚动周转回归；只测试生产采用逻辑，不创建GPU或用户文件。

use super::*;
use crate::ui::asset_thumbnails::{constants::CACHE_CAPACITY, demand::adopt_inspection};

fn paths(start: usize, count: usize) -> Vec<String> {
    (start..start + count).map(|index| format!("models/item-{index}.glb")).collect()
}

fn inspection(epoch: u64, paths: &[String]) -> InspectionResult {
    InspectionResult { epoch, sources: paths.iter().enumerate().map(|(index, path)| (path.clone(), Ok(SourceInfo {
        primary: FileStamp { length: 1, modified: None }, stamp: index as u64, dependencies: Arc::from([]),
    }))).collect() }
}

#[test]
fn oversized_visible_window_never_exceeds_the_original_thumbnail_cache_budget() {
    let count = crate::ui::continuous_list::pool_capacity(3000., 3, 123.);
    assert!(count > CACHE_CAPACITY, "4K与低UI缩放会形成超过64项的可见窗口");
    let mut state = ThumbnailState::default();
    state.paths = paths(0, count);
    state.identity = Some(WindowKey { folder: "models".into(), start: 0, count, revision: 7 });
    adopt_inspection(&mut state, inspection(0, &paths(0, count)));
    assert_eq!(state.cache.len(), CACHE_CAPACITY);
    assert_eq!(state.paths.len(), count, "缓存预算不能截断可滚动、可选择的完整窗口");
    let refused = format!("models/item-{CACHE_CAPACITY}.glb");
    assert!(!state.make_room(&refused), "全部受保护时不能继续插入第65项");
    assert!(!state.cache.contains_key(&refused));
    assert!(state.make_room("models/item-0.glb"), "已有键仍可更新同一源而不新增缓存");
    assert_eq!(state.cache.len(), CACHE_CAPACITY);
}

#[test]
fn scrolling_can_adopt_a_previously_uncached_file_without_evicting_visible_sources() {
    let mut state = ThumbnailState::default();
    state.paths = paths(0, CACHE_CAPACITY);
    state.identity = Some(WindowKey { folder: "models".into(), start: 0, count: CACHE_CAPACITY, revision: 7 });
    adopt_inspection(&mut state, inspection(0, &paths(0, CACHE_CAPACITY)));
    // 新窗口仍保留大部分原图，但第96项现在确实可见；原非窗口缓存才能被淘汰腾位。
    state.paths = paths(1, CACHE_CAPACITY - 1);
    state.paths.push("models/item-96.glb".into());
    state.identity = Some(WindowKey { folder: "models".into(), start: 1, count: CACHE_CAPACITY, revision: 7 });
    adopt_inspection(&mut state, inspection(0, &["models/item-96.glb".into()]));
    assert_eq!(state.cache.len(), CACHE_CAPACITY);
    assert!(!state.cache.contains_key("models/item-0.glb"));
    assert!(state.cache.contains_key("models/item-96.glb"), "预算外文件在实际滚入窗口后仍能正常加载");
    assert!((1..CACHE_CAPACITY).all(|index| state.cache.contains_key(&format!("models/item-{index}.glb"))),
        "滚动周转不得优先淘汰仍可见的源");
}
