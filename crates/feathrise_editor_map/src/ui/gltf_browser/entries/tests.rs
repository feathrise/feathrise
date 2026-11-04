use bevy::prelude::Handle;
use feathrise_map_content::MapPlacementAssetTarget;

use super::*;
use crate::ui::gltf_browser::{CachedGltfAsset, CachedGltfMesh, PlacementSignature};

fn placement_signature(index: usize) -> PlacementSignature {
    PlacementSignature {
        editor: Default::default(),
        placement_id: format!("instance-{index}"),
        asset_path: format!("gltf/{index}.glb"),
        asset_target: Default::default(), // 展开预算夹具全部使用历史Scene0。
        water_bindings: Vec::new(),
    }
}

fn mesh_entry(index: usize) -> EditorGltfBrowserEntry {
    EditorGltfBrowserEntry {
        kind: EditorGltfBrowserEntryKind::Mesh, // 旧公开行携带完整稳定选择。
        display_name: format!("旧河流网格-{index}"), // 每行独立拥有显示字符串。
        depth: 5,                               // 视觉缩进不影响退役路径深度。
        selection: Some(EditorGltfMeshSelection {
            placement_id: format!("placement-{index}"), // 每行独立实例身份。
            node_path: (0..16).map(|depth| format!("节点-{depth}")).collect(), // 深路径确保析构预算可见。
            mesh_name: format!("WaterSurface-{index}"),                        // 独立网格名分配。
            mesh_index: Some(index as u32),                                    // 非权威诊断索引。
            valid: true,                   // 模拟当前Ready选择。
            orphaned_water_binding: false, // 真实网格行而非失效绑定。
        }),
    }
}

#[test]
fn replacing_4096_deep_rows_retires_old_tree_over_multiple_frames() {
    let mut state = EditorGltfBrowserState {
        initialized: true, // 模拟已经发布旧活动树。
        entries: (0..MAX_GLTF_BROWSER_CHILD_ENTRIES)
            .map(mesh_entry)
            .collect(), // 一整份展开子行预算足以验证深树不会同步Drop。
        observation: super::super::BrowserSessionObservation {
            scene_objects_revision: Default::default(),
            draft_available: true,                            // 完整草稿已存在。
            draft_map_id: Some("MAP-retire-rows".to_owned()), // 当前结构稳定地图身份。
            source_placement_count: 0,                        // 新树可以为空。
            placement_structure_revision: 7,                  // 与新草稿身份一致。
        },
        entries_revision: 11, // 与下方构建冻结修订一致。
        ..Default::default()
    };
    let build = BrowserEntriesBuild::new(&state); // 完整空新树代表全部旧行被移除。

    assert!(publish_completed_entries(&mut state, build).is_ok()); // 新树一次原子发布。
    assert!(state.entries.is_empty()); // 固定行池同帧不再观察旧行。
    assert_eq!(state.retirement.generation_count(), 1); // 旧4096行整体移入唯一正常代。

    let mut second = BrowserEntriesBuild::new(&state); // 退役期间仍可发布无需替换旧行的最新完整树。
    second.entries.push(mesh_entry(9_000)); // 建立一条新的活动行。
    assert!(publish_completed_entries(&mut state, second).is_ok());
    let mut newest = BrowserEntriesBuild::new(&state); // 下一次更新需要换下这一条活动行。
    newest.entries.push(mesh_entry(9_001)); // 最新请求保持在调用方唯一草稿中。
    let newest = publish_completed_entries(&mut state, newest)
        .expect_err("上一代尚未清空时必须背压新的拥有型替换");
    assert_eq!(newest.entries.len(), 1); // 背压返回完整最新草稿，不丢失也不创建新代。
    assert_eq!(
        state.entries[0]
            .selection
            .as_ref()
            .and_then(|selection| selection.mesh_index),
        Some(9_000)
    ); // 活动树继续保持上一份完整结果。
    assert_eq!(state.retirement.generation_count(), 1); // 连续换代没有积累第二正常代。

    state.retirement.advance_frame();
    assert_eq!(state.retirement.generation_count(), 1); // 一帧不能析构全部深行。

    let mut frames = 1usize;
    while !state.retirement.is_empty() {
        state.retirement.advance_frame();
        frames += 1;
        assert!(frames < 300, "4096条深子行应按固定预算在有限帧内收口");
    }
    assert!(frames > 2); // 锁定替换后的跨帧释放行为。
}

#[test]
fn worst_case_collapsed_roots_are_built_over_multiple_updates() {
    let mut state = EditorGltfBrowserState {
        initialized: true,
        draft_available: true,
        observation: super::super::BrowserSessionObservation {
            scene_objects_revision: Default::default(),
            draft_available: true,
            draft_map_id: Some("MAP-roots".to_owned()),
            source_placement_count: MAX_GLTF_BROWSER_PLACEMENTS,
            placement_structure_revision: 4,
        },
        signature: (0..MAX_GLTF_BROWSER_PLACEMENTS)
            .map(placement_signature)
            .collect(),
        source_placement_count: MAX_GLTF_BROWSER_PLACEMENTS,
        ..Default::default()
    };

    request_visible_entries_rebuild(&mut state);
    assert!(!advance_visible_entries_build(&mut state));
    let first = state.entries_build.as_ref().expect("首帧后仍在构建");
    assert_eq!(first.placement_index, MAX_GLTF_ENTRY_ROWS_PER_FRAME);
    assert_eq!(first.entries.len(), MAX_GLTF_ENTRY_ROWS_PER_FRAME); // 单Update只复制256个根行。
    assert!(state.entries.is_empty()); // 半成品不会泄漏到固定UI行池。

    let mut frames = 1usize;
    while state.entries_build.is_some() {
        advance_visible_entries_build(&mut state);
        frames += 1;
        assert!(frames <= 33, "8192个合法根应在固定预算内全部物化");
    }

    assert_eq!(frames, 33); // 三十二帧各输出256根，最后一帧只确认游标已到末尾并原子发布。
    assert_eq!(state.entries.len(), MAX_GLTF_BROWSER_PLACEMENTS);
    assert!(!state.truncated); // 根行不占展开子行预算，合法尾部实例不能被截断。
    assert!(matches!(
        &state.entries[MAX_GLTF_BROWSER_PLACEMENTS - 1].kind,
        EditorGltfBrowserEntryKind::Placement { placement_id, .. }
            if placement_id == "instance-8191"
    )); // 第8192个placement必须真实进入可分页树。
}

#[test]
fn primitive_placement_is_a_labeled_non_expandable_root() {
    let asset_path = "gltf/plant.glb".to_owned();
    let mut state = EditorGltfBrowserState {
        initialized: true,
        draft_available: true,
        observation: super::super::BrowserSessionObservation {
            scene_objects_revision: Default::default(),
            draft_available: true,
            draft_map_id: Some("MAP-primitive-root".to_owned()),
            source_placement_count: 1,
            placement_structure_revision: 6,
        },
        signature: vec![PlacementSignature {
            editor: Default::default(),
            placement_id: "leaf-1".to_owned(),
            asset_path: asset_path.clone(),
            asset_target: MapPlacementAssetTarget::Primitive {
                mesh: 6,
                primitive: 0,
                material: Some(2),
            },
            water_bindings: Vec::new(),
        }],
        source_placement_count: 1,
        ..Default::default()
    };
    state.expanded_placements.insert("leaf-1".to_owned()); // 旧Scene0展开偏好不得让子资产生成完整树。
    state.assets.insert(
        asset_path,
        CachedGltfAsset {
            handle: Handle::default(),
            meshes: vec![CachedGltfMesh {
                node_path: vec!["完整场景对象".to_owned()],
                mesh_name: "不应显示".to_owned(),
                mesh_index: Some(99),
                valid: true,
                ambiguous: false,
            }],
            phase: CachedGltfAssetPhase::Ready,
            active_ready: true,
            truncated: false,
            content_revision: 1,
        },
    );

    request_visible_entries_rebuild(&mut state);
    while state.entries_build.is_some() {
        advance_visible_entries_build(&mut state);
    }

    assert_eq!(state.entries.len(), 1); // 同路径即使已有Scene0缓存也只显示当前primitive根。
    assert!(state.entries[0].display_name.contains("Mesh6/Primitive0"));
    assert!(matches!(
        state.entries[0].kind,
        EditorGltfBrowserEntryKind::Placement {
            collapsed: true,
            expandable: false,
            ..
        }
    ));
}

#[test]
fn builtin_cuboid_is_a_mesh_row_with_the_original_object_identity_and_no_glb_load_summary() {
    let mut cube = placement_signature(0);
    cube.asset_path.clear();
    cube.asset_target = MapPlacementAssetTarget::DEFAULT_CUBOID;
    cube.editor = std::sync::Arc::new(feathrise_map_content::MapPlacementEditorMetadata {
        name: "白模墙".into(), hidden: true, locked: true, ..Default::default()
    });
    let mut state = EditorGltfBrowserState {
        initialized: true,
        draft_available: true,
        source_placement_count: 2,
        signature: vec![cube, placement_signature(1)],
        ..Default::default()
    };
    state.expanded_placements.insert("instance-0".into()); // 旧展开偏好不能为内置网格制造GLB需求。
    request_visible_entries_rebuild(&mut state);
    while state.entries_build.is_some() { advance_visible_entries_build(&mut state); }
    assert_eq!(state.entries.len(), 2);
    assert_eq!(state.entries[0].display_name, "白模墙 · 网格 · 立方体 · 隐藏 · 锁定");
    assert!(matches!(&state.entries[0].kind,
        EditorGltfBrowserEntryKind::Placement { placement_id, expandable: false, collapsed: true }
        if placement_id == "instance-0")); // 视觉分类改变后仍由原稳定ID处理选择、右键和历史。
    assert!(state.entries[0].selection.is_none());
    assert!(state.assets.is_empty());
    assert!(state.asset_demand_counts.is_empty());
    assert_eq!(state.status, "场景对象2"); // 同时计入内置与外部对象，未请求外部文件时不显示伪加载门。
}

#[test]
fn last_legal_placement_can_expand_and_publish_a_selectable_mesh() {
    let mut state = EditorGltfBrowserState {
        initialized: true,
        draft_available: true,
        observation: super::super::BrowserSessionObservation {
            scene_objects_revision: Default::default(),
            draft_available: true,
            draft_map_id: Some("MAP-last-root".to_owned()),
            source_placement_count: MAX_GLTF_BROWSER_PLACEMENTS,
            placement_structure_revision: 5,
        },
        signature: (0..MAX_GLTF_BROWSER_PLACEMENTS)
            .map(placement_signature)
            .collect(),
        source_placement_count: MAX_GLTF_BROWSER_PLACEMENTS,
        ..Default::default()
    };
    let placement_id = "instance-8191".to_owned();
    let asset_path = "gltf/8191.glb".to_owned();
    state.expanded_placements.insert(placement_id.clone());
    state.assets.insert(
        asset_path,
        CachedGltfAsset {
            handle: Handle::default(),
            meshes: vec![CachedGltfMesh {
                node_path: vec!["RiverObject".to_owned()],
                mesh_name: "RiverMesh".to_owned(),
                mesh_index: Some(7),
                valid: true,
                ambiguous: false,
            }],
            phase: CachedGltfAssetPhase::Ready,
            active_ready: true,
            truncated: false,
            content_revision: 1,
        },
    );

    request_visible_entries_rebuild(&mut state);
    let mut frames = 0usize;
    while state.entries_build.is_some() {
        advance_visible_entries_build(&mut state);
        frames += 1;
        assert!(frames <= 34, "尾部展开实例应在根扫描预算后完成");
    }

    let selection = state
        .entries
        .iter()
        .filter_map(|entry| entry.selection.as_ref())
        .find(|selection| selection.placement_id == placement_id)
        .expect("第8192个placement的网格必须可选");
    assert_eq!(selection.node_path, ["RiverObject"]);
    assert_eq!(selection.mesh_name, "RiverMesh");
    assert!(!state.truncated);
}

#[test]
fn one_large_ready_asset_obeys_the_per_update_row_budget() {
    let asset_path = "gltf/large.glb".to_owned();
    let mut state = EditorGltfBrowserState {
        initialized: true,
        draft_available: true,
        observation: super::super::BrowserSessionObservation {
            scene_objects_revision: Default::default(),
            draft_available: true,
            draft_map_id: Some("MAP-large".to_owned()),
            source_placement_count: 1,
            placement_structure_revision: 9,
        },
        signature: vec![PlacementSignature {
            editor: Default::default(),
            placement_id: "large-instance".to_owned(),
            asset_path: asset_path.clone(),
            asset_target: Default::default(), // 大网格缓存测试需要可展开Scene0。
            water_bindings: Vec::new(),
        }],
        source_placement_count: 1,
        ..Default::default()
    };
    state
        .expanded_placements
        .insert("large-instance".to_owned());
    state.assets.insert(
        asset_path,
        CachedGltfAsset {
            handle: Handle::default(),
            meshes: (0..super::super::MAX_GLTF_MESHES_PER_ASSET)
                .map(|index| CachedGltfMesh {
                    node_path: vec![format!("node-{index}")],
                    mesh_name: format!("mesh-{index}"),
                    mesh_index: Some(index as u32),
                    valid: true,
                    ambiguous: false,
                })
                .collect(),
            phase: CachedGltfAssetPhase::Ready,
            active_ready: true,
            truncated: false,
            content_revision: 1,
        },
    );

    request_visible_entries_rebuild(&mut state);
    let mut previous_rows = 0usize;
    let mut frames = 0usize;
    while state.entries_build.is_some() {
        advance_visible_entries_build(&mut state);
        frames += 1;
        if let Some(build) = state.entries_build.as_ref() {
            assert!(
                build.entries.len().saturating_sub(previous_rows) <= MAX_GLTF_ENTRY_ROWS_PER_FRAME
            );
            previous_rows = build.entries.len();
        }
        assert!(
            frames <= 18,
            "2048网格应按行与路径段双预算在有限Update内完成"
        );
    }

    assert!(frames > 1); // 展开一个大GLB也不允许同步重组全部网格行。
    assert_eq!(
        state.entries.len(),
        1 + super::super::MAX_GLTF_MESHES_PER_ASSET
    );
}

fn ready_state_with_mesh_paths(paths: Vec<Vec<String>>) -> EditorGltfBrowserState {
    let asset_path = "gltf/path-budget.glb".to_owned();
    let mut state = EditorGltfBrowserState {
        initialized: true,     // 直接构造稳定活动签名。
        draft_available: true, // 状态摘要不是“等待草稿”。
        observation: super::super::BrowserSessionObservation {
            scene_objects_revision: Default::default(),
            draft_available: true,                             // 完整草稿可用。
            draft_map_id: Some("MAP-entry-budget".to_owned()), // 固定地图身份。
            source_placement_count: 1,                         // 只有一个展开实例。
            placement_structure_revision: 13,                  // 行草稿身份。
        },
        signature: vec![PlacementSignature {
            editor: Default::default(),
            placement_id: "budget-instance".to_owned(), // 唯一稳定实例ID。
            asset_path: asset_path.clone(),             // 命中下方Ready缓存。
            asset_target: Default::default(),           // 路径预算测试需要展开Scene0。
            water_bindings: Vec::new(),                 // 只验证真实网格行物化预算。
        }],
        source_placement_count: 1, // 状态摘要显示一个实例。
        ..Default::default()
    };
    state
        .expanded_placements
        .insert("budget-instance".to_owned()); // 显式请求网格子行。
    state.assets.insert(
        asset_path,
        CachedGltfAsset {
            handle: Handle::default(), // 纯测试无需真实AssetServer。
            meshes: paths
                .into_iter()
                .enumerate()
                .map(|(index, node_path)| CachedGltfMesh {
                    node_path,                          // 调用方控制路径深度与字符串量。
                    mesh_name: format!("mesh-{index}"), // 每行独立名称。
                    mesh_index: Some(index as u32),     // 稳定诊断索引。
                    valid: true,                        // 全部允许选择。
                    ambiguous: false,                   // 测试资产身份唯一。
                })
                .collect(),
            phase: CachedGltfAssetPhase::Ready, // 网格缓存已经完整采用。
            active_ready: true,                 // 当前树允许读取缓存。
            truncated: false,                   // 不触发其他硬上限。
            content_revision: 1,                // 单一稳定内容代次。
        },
    );
    state
}

#[test]
fn deep_mesh_rows_obey_path_segment_budget() {
    let deep_path = (0..64).map(|depth| format!("n{depth}")).collect::<Vec<_>>(); // 每网格路径复制近似256段。
    let mut state = ready_state_with_mesh_paths(vec![deep_path; 16]); // 行数预算本可在一帧处理全部。
    request_visible_entries_rebuild(&mut state);
    assert!(!advance_visible_entries_build(&mut state));
    let build = state.entries_build.as_ref().expect("深路径仍有后续行");
    assert_eq!(build.entries.len(), 3); // 一个根加两个网格恰好耗尽512路径段预算。
}

#[test]
fn long_mesh_rows_obey_string_byte_budget() {
    let long_path = (0..16).map(|_| "水".repeat(256)).collect::<Vec<_>>(); // 每路径约12KiB UTF-8字节。
    let mut state = ready_state_with_mesh_paths(vec![long_path; 8]); // 路径段额度不会先耗尽。
    request_visible_entries_rebuild(&mut state);
    assert!(!advance_visible_entries_build(&mut state));
    let build = state.entries_build.as_ref().expect("长字符串仍有后续行");
    assert_eq!(build.entries.len(), 3); // 一个根加两个约60KiB网格，第三条受128KiB预算留到下帧。
}
