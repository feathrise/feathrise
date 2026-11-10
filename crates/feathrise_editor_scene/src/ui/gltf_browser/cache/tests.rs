use std::collections::BTreeSet;

use bevy::world_serialization::WorldAsset;

use super::*;

fn cached_asset(handle: Handle<WorldAsset>, revision: u64) -> CachedGltfAsset {
    CachedGltfAsset {
        handle,
        meshes: vec![super::super::CachedGltfMesh {
            node_path: vec!["旧节点".to_owned()],
            mesh_name: "旧网格".to_owned(),
            mesh_index: Some(1),
            valid: true,
            ambiguous: false,
        }],
        active_ready: true,
        phase: CachedGltfAssetPhase::Loading,
        truncated: false,
        content_revision: revision,
    }
}

#[test]
fn stale_scene_build_result_never_replaces_active_cache() {
    let path = "gltf/river.glb".to_owned();
    let handle = Handle::<WorldAsset>::default();
    let mut state = EditorGltfBrowserState::default();
    state
        .assets
        .insert(path.clone(), cached_asset(handle.clone(), 2)); // 热更已把缓存推进到修订2。
    state.loading_asset_count = 1;
    let build = BrowserSceneBuild::new(
        path.clone(),
        handle.id(),
        1, // 修订1的分帧任务现在才完成。
        &World::new(),
    );

    assert_eq!(
        adopt_scene_build_result(&mut state, build).ok(),
        Some(false)
    );
    let cached = &state.assets[&path];
    assert_eq!(cached.content_revision, 2);
    assert_eq!(cached.meshes[0].mesh_name, "旧网格"); // 过期空结果没有覆盖活动旧缓存。
    assert!(cached.active_ready);
    assert_eq!(state.retirement.generation_count(), 1); // 迟到草稿也进入有界退役而不是直接Drop。
}

#[test]
fn successful_scene_publish_retires_completed_root_scratch_over_multiple_frames() {
    let path = "gltf/wide.glb".to_owned();
    let handle = Handle::<WorldAsset>::default();
    let mut world = World::new();
    for _ in 0..1_024 {
        world.spawn_empty(); // 完成草稿保留大量根实体，即使没有网格也不能随发布整批Drop。
    }
    let mut build = BrowserSceneBuild::new(path.clone(), handle.id(), 1, &world);
    while !advance_scene_build(&mut build, &world, |_| None) {}
    let mut state = EditorGltfBrowserState::default();
    state.assets.insert(path.clone(), cached_asset(handle, 1));
    state.loading_asset_count = 1;

    assert_eq!(adopt_scene_build_result(&mut state, build).ok(), Some(true));
    assert!(state.assets[&path].meshes.is_empty()); // 新完整空结果已原子替换旧网格。
    state.retirement.advance_frame();
    assert!(!state.retirement.is_empty()); // 1024根受128元素预算约束，首帧不能清完。

    let mut frames = 1usize;
    while !state.retirement.is_empty() {
        state.retirement.advance_frame();
        frames += 1;
    }
    assert!(frames > 1);
}

#[test]
fn closing_browser_drops_scene_build_and_strong_asset_cache() {
    let path = "gltf/river.glb".to_owned();
    let handle = Handle::<WorldAsset>::default();
    let world = World::new();
    let mut state = EditorGltfBrowserState::default();
    state
        .assets
        .insert(path.clone(), cached_asset(handle.clone(), 1));
    state.scene_build = Some(BrowserSceneBuild::new(path, handle.id(), 1, &world));

    release_browser_session(&mut state);

    assert!(state.scene_build.is_none());
    assert!(state.assets.is_empty()); // 唯一强Scene0句柄随缓存一起释放。
    assert!(state.loading_queue.is_empty());
}

#[test]
fn repeated_hot_reload_coalesces_to_one_physical_loading_ticket() {
    let path = "gltf/river.glb".to_owned();
    let handle = Handle::<WorldAsset>::default();
    let mut state = EditorGltfBrowserState::default();
    state.assets.insert(path.clone(), cached_asset(handle, 1));
    state.loading_asset_count = 1;

    for _ in 0..128 {
        assert!(invalidate_cached_paths(
            &mut state,
            BTreeSet::from([path.clone()]),
        )); // 模拟同一个仍在Loading的GLB连续收到内容热更。
    }

    assert_eq!(state.loading_queue.len(), 1); // 队列物理长度不随热更次数增长。
    assert_eq!(state.queued_revision_by_path[&path], 129); // 唯一票的目标修订被原地合并到最新值。
    assert_eq!(state.assets[&path].content_revision, 129);
}
