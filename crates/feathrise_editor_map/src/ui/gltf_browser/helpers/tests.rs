use bevy::prelude::Handle;

use super::super::entries::{advance_visible_entries_build, request_visible_entries_rebuild};
use super::super::{
    CachedGltfAssetPhase, EditorGltfBrowserEntryKind, PlacementSignature,
    PlacementWaterBindingSignature,
};
use super::*; // 锁定稳定标签、显式命名规则和失效绑定组合行为。

/// 测试内完整推进分帧任务；生产代码每次Update只调用一次advance。
fn finish_entries_build(state: &mut EditorGltfBrowserState) {
    request_visible_entries_rebuild(state);
    while state.entries_build.is_some() {
        advance_visible_entries_build(state);
    }
}

#[test]
fn primitive_label_only_accepts_bevy_mesh_primitive_format() {
    assert_eq!(
        mesh_index_from_primitive_label("Mesh12/Primitive3"),
        Some(12)
    ); // 标准多primitive标签恢复父网格。
    assert_eq!(mesh_index_from_primitive_label("Mesh0/Primitive0"), Some(0)); // 零索引同样有效。
    assert_eq!(mesh_index_from_primitive_label("Mesh12"), None); // GltfMesh资产标签不能冒充Mesh3d primitive。
    assert_eq!(mesh_index_from_primitive_label("MeshA/Primitive0"), None); // 非十进制索引必须拒绝。
    assert_eq!(
        mesh_index_from_primitive_label("Mesh1/Primitive0/Extra"),
        None
    ); // 任意尾部都不是稳定Bevy标签。
}

#[test]
fn only_loader_generated_node_names_are_rejected() {
    assert!(is_generated_gltf_node_name("GltfNode0")); // loader缺名节点的首个索引。
    assert!(is_generated_gltf_node_name("GltfNode2048")); // 任意十进制索引均属于生成名。
    assert!(!is_generated_gltf_node_name("GltfNode")); // 显式同名前缀但无索引不误判。
    assert!(!is_generated_gltf_node_name("桥_GltfNode1")); // Blender正常名称包含该字样仍有效。
    assert!(!is_generated_gltf_node_name("GltfNode河岸")); // 非数字后缀不是loader规则。
}

#[test]
fn stable_reference_requires_named_nodes_named_mesh_and_mesh_index() {
    let named_path = vec!["河道".to_owned(), "水面".to_owned()]; // 两层Blender节点路径。
    assert!(stable_mesh_reference(
        &named_path,
        "WaterSurface",
        Some(2),
        true
    )); // 四项完整即可稳定引用。
    assert!(!stable_mesh_reference(&named_path, "", Some(2), true)); // 缺GltfMeshName必须无效。
    assert!(!stable_mesh_reference(
        &["GltfNode7".to_owned()],
        "WaterSurface",
        Some(2),
        true,
    )); // loader生成节点名无法跨导出稳定。
    assert!(!stable_mesh_reference(
        &named_path,
        "WaterSurface",
        None,
        true
    )); // 缺子资产索引不能猜测。
}

#[test]
fn selection_identity_ignores_reexported_mesh_index() {
    let left = EditorGltfMeshSelection {
        placement_id: "river-1".to_owned(), // 同一摆放实例内比较稳定网格身份。
        node_path: vec!["河道".to_owned(), "水面".to_owned()], // 完整Blender对象路径。
        mesh_name: "WaterSurface".to_owned(), // 显式Mesh datablock名。
        mesh_index: Some(2),                // 旧导出的非权威诊断索引。
        valid: true,                        // 初始Scene0引用完整有效。
        orphaned_water_binding: false,      // 当前行来自真实网格。
    };
    let mut right = left.clone(); // 只改变重导出后可能漂移的索引。
    right.mesh_index = Some(17); // Blender重导出会因其他网格增删而改变数组位置。
    assert!(same_selection_identity(&left, &right)); // 稳定路径和名称不变时仍是同一作者目标。
}

#[test]
fn placement_roots_are_collapsed_without_scene_cache_by_default() {
    let mut state = EditorGltfBrowserState {
        initialized: true,
        draft_available: true,
        signature: vec![PlacementSignature {
            editor: Default::default(),
            placement_id: "river-1".to_owned(),
            asset_path: "gltf/river.glb".to_owned(),
            asset_target: Default::default(), // 折叠根沿用可展开Scene0语义。
            water_bindings: Vec::new(),
        }],
        source_placement_count: 1,
        ..Default::default()
    };

    finish_entries_build(&mut state);

    assert_eq!(state.entries.len(), 1); // 默认只发布实例根，不需要Scene0占位子行。
    assert!(matches!(
        state.entries[0].kind,
        EditorGltfBrowserEntryKind::Placement {
            collapsed: true,
            ..
        }
    ));
    assert!(state.assets.is_empty()); // 默认折叠状态无需任何强Scene0句柄。
    assert_eq!(state.status, "场景对象1"); // 未展开时没有外部加载需求，不显示虚假的GLB 0/0门。
    assert!(!state.entries[0].display_name.contains("加载中")); // 未请求不能误报成后台加载。
}

#[test]
fn missing_water_binding_stays_visible_and_clearable_after_ready_reexport() {
    let asset_path = "gltf/river.glb".to_owned(); // 当前摆放继续指向已经重导出的同一GLB路径。
    let binding = PlacementWaterBindingSignature {
        node_path: vec!["河道".to_owned(), "旧水面".to_owned()], // 重导后已经消失的原Blender对象路径。
        mesh_name: "OldWaterSurface".to_owned(), // 作者JSON仍保存的原Mesh datablock名。
        mesh_index: 3,                           // 非权威旧诊断索引仅供行选择快照使用。
        kind: MapPlacementWaterKind::Sloped,     // 失效行应直接告诉作者原类别。
    };
    let mut state = EditorGltfBrowserState {
        initialized: true,     // 模拟浏览器已经取得当前草稿。
        draft_available: true, // 空Scene0网格不是“草稿等待”状态。
        signature: vec![PlacementSignature {
            editor: Default::default(),
            placement_id: "river-1".to_owned(), // 失效绑定所属实例。
            asset_path: asset_path.clone(),     // 命中下方Ready缓存。
            asset_target: Default::default(),   // 失效水绑定只存在于Scene0层级。
            water_bindings: vec![binding],      // 当前作者记录保留唯一旧水绑定。
        }],
        source_placement_count: 1, // 状态摘要显示一个真实实例。
        ..Default::default()       // 其余UI分页与折叠状态使用空默认值。
    };
    state.assets.insert(
        asset_path,
        super::super::CachedGltfAsset {
            handle: Handle::default(),          // 纯测试不加载真实WorldAsset。
            meshes: Vec::new(),                 // 重导后的Ready GLB已经没有旧目标。
            phase: CachedGltfAssetPhase::Ready, // 只有Ready才能确认目标确实消失。
            active_ready: true,                 // 空网格表属于完整采用结果。
            truncated: false,                   // 空结果不是预算截断。
            content_revision: 1,                // 纯测试使用首个稳定内容修订。
        },
    );
    state.expanded_placements.insert("river-1".to_owned()); // 只有展开实例才发布Ready网格和失效绑定子行。
    finish_entries_build(&mut state); // 合并当前Scene0与作者绑定生成诊断行。
    assert_eq!(state.entries.len(), 2); // 实例根之后必须保留一条失效绑定。
    let orphan = state.entries[1]
        .selection
        .clone()
        .expect("失效绑定行必须可选择"); // 行必须携带原身份供清除动作定位。
    assert!(matches!(
        state.entries[1].kind,
        EditorGltfBrowserEntryKind::MissingWaterBinding
    )); // 专用行类型不得冒充当前Scene0真实网格。
    assert!(!orphan.valid); // 失效目标不能继续改类别或参数。
    assert!(orphan.orphaned_water_binding); // 属性区据此开放唯一清除动作。

    state.selection = Some(orphan); // 模拟作者选中失效绑定后执行清除。
    state.signature[0].water_bindings.clear(); // 清除动作已经从草稿移除原绑定。
    finish_entries_build(&mut state); // 下一次分帧重组必须移除诊断行和悬空选择。
    assert_eq!(state.entries.len(), 1); // 只剩实例根。
    assert!(state.selection.is_none()); // 已清除绑定不能继续显示旧选择诊断。
}
