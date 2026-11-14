//! 植被作者图层结构操作的纯值回归。

use super::*; // 子模块可直接验证私有局部变换，不启动Bevy App。
#[path = "model_layer_tests.rs"]
mod model_layers;
use feathrise_scene_content::{
    SceneVegetationAuthorGroup, SceneVegetationAuthorGroupMode, SceneVegetationAuthorLayer,
    SceneVegetationKind, SceneVegetationOutputMergeMode,
}; // 构造一个最小正式层和一对一作者结构。

fn output() -> SceneVegetationLayer {
    SceneVegetationLayer {
        id: "grass".to_owned(),
        label: "普通草".to_owned(),
        kind: SceneVegetationKind::Blade,
        runtime_profile: Some("hn01_ordinary_grass".to_owned()),
        runtime_layer_id: Some(1),
        prototype_id: None,
        asset_path: "grass.glb".to_owned(),
        density_source_path: "scenes/test/vegetation/grass.r8".to_owned(),
        density_texture_path: "scenes/test/vegetation/grass.ktx2".to_owned(),
        resolution: [2, 2],
        spacing_meters: 1.0,
        scale_range: [1.0, 1.0],
        max_instances: 4,
        seed: 1,
    }
}

fn authoring() -> SceneVegetationAuthoring {
    SceneVegetationAuthoring {
        groups: vec![SceneVegetationAuthorGroup {
            id: "base".to_owned(),
            label: "基础".to_owned(),
            mode: SceneVegetationAuthorGroupMode::Additive,
            output_merge: SceneVegetationOutputMergeMode::Maximum,
            capacity: 1.0,
            claim_threshold: 0.0,
            selection_seed: 1,
            selection_cell_meters: 8.0,
            enabled: true,
        }],
        layers: vec![SceneVegetationAuthorLayer {
            id: "base-grass".to_owned(),
            label: "普通草 · 基础".to_owned(),
            group_id: "base".to_owned(),
            output_layer_id: "grass".to_owned(),
            density_source_path: "scenes/test/vegetation/author/base-grass.r8".to_owned(),
            resolution: [2, 2],
            enabled: true,
            locked: false,
            opacity: 1.0,
            priority: 0,
            weight: 1.0,
        }],
    }
}

#[test]
fn duplicate_uses_copy_on_write_and_new_group_moves_only_the_selected_layer() {
    let outputs = vec![output()];
    let mut authoring = authoring();
    let base = Arc::new(vec![0, 64, 128, 255]);
    let mut densities = BTreeMap::from([("base-grass".to_owned(), base.clone())]);
    let duplicated = mutate_authoring(
        EditorAction::VegetationDuplicateLayer,
        Some(0),
        None,
        "scenes/test",
        &outputs,
        &mut authoring,
        &mut densities,
    )
    .expect("复制作者图层应成功");
    assert_eq!(duplicated.selected, Some(1));
    let duplicate_id = authoring.layers[1].id.clone();
    assert!(Arc::ptr_eq(&base, &densities[&duplicate_id])); // 点击复制不在主线程复制整张R8。

    mutate_authoring(
        EditorAction::VegetationNewGroup,
        Some(1),
        None,
        "scenes/test",
        &outputs,
        &mut authoring,
        &mut densities,
    )
    .expect("新组应接入选中副本");
    assert_eq!(authoring.groups.len(), 2);
    assert_eq!(authoring.layers[0].group_id, "base");
    assert_eq!(authoring.layers[1].group_id, authoring.groups[1].id);
    mutate_authoring(
        EditorAction::VegetationCycleGroupMode,
        Some(1),
        None,
        "scenes/test",
        &outputs,
        &mut authoring,
        &mut densities,
    )
    .expect("选中组应能独立切换规则");
    assert_eq!(
        authoring.groups[1].mode,
        SceneVegetationAuthorGroupMode::WeightedPartition
    );
}

#[test]
fn locked_layer_rejects_destructive_structure_changes() {
    let outputs = vec![output()];
    let mut authoring = authoring();
    authoring.layers[0].locked = true;
    let mut densities = BTreeMap::from([("base-grass".to_owned(), Arc::new(vec![0; 4]))]);
    assert!(
        mutate_authoring(
            EditorAction::VegetationDeleteSelected,
            Some(0),
            None,
            "scenes/test",
            &outputs,
            &mut authoring,
            &mut densities,
        )
        .is_err()
    );
    assert_eq!(authoring.layers.len(), 1); // 失败局部变换没有删除锁定层。
}

#[test]
fn deleting_the_last_unlocked_layer_leaves_a_valid_empty_author_list() {
    let outputs = vec![output()];
    let mut authoring = authoring();
    let mut densities = BTreeMap::from([("base-grass".to_owned(), Arc::new(vec![0; 4]))]);

    let deleted = mutate_authoring(
        EditorAction::VegetationDeleteSelected,
        Some(0),
        None,
        "scenes/test",
        &outputs,
        &mut authoring,
        &mut densities,
    )
    .expect("最后一层未锁定时应允许删除到零层");

    assert_eq!(deleted.selected, None); // 零层状态不伪造第零层选择。
    assert!(authoring.layers.is_empty()); // 作者列表真实为空。
    assert!(densities.is_empty()); // 对应R8内存表同步移除且未破坏磁盘文件。
}

#[test]
fn empty_author_list_can_create_the_explicitly_selected_wheat_layer() {
    let grass = output();
    let mut wheat = grass.clone();
    wheat.id = "wheat".to_owned();
    wheat.label = "麦田".to_owned();
    wheat.runtime_profile = Some("hn01_wheat".to_owned());
    let outputs = vec![grass, wheat];
    let mut authoring = authoring();
    authoring.layers.clear(); // 复现用户删除全部作者图层后的真实空列表状态。
    let mut densities = BTreeMap::new();

    let created = mutate_authoring(
        EditorAction::VegetationAddLayer,
        None,
        Some("wheat"), // 类型按钮已经独立选择麦田，不依赖任何现有作者层。
        "scenes/test",
        &outputs,
        &mut authoring,
        &mut densities,
    )
    .expect("空作者列表应能按已选正式输出新建麦田层");

    assert_eq!(created.selected, Some(0));
    assert_eq!(authoring.layers.len(), 1);
    assert_eq!(authoring.layers[0].output_layer_id, "wheat");
    assert!(authoring.layers[0].label.starts_with("麦田"));
    assert_eq!(densities[&authoring.layers[0].id].as_slice(), &[0; 4]);
}

#[test] // Blade与Mesh共用R8作者事务，Clipmap不需要整图GLB高度缓存。
fn clipmap_blade_and_mesh_creation_work_without_height_cache() {
    // 真实作者事务回归。
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121])); // 复用完整场景夹具。
    draft.manifest.terrain.kind = feathrise_scene_content::SceneTerrainKind::SparseClipmap; // 稀疏场景不提供整图高度。
    draft.heights = None; // Blade创建不能再依赖派生GLB缓存。
    draft.vegetation = Some(feathrise_scene_content::SceneVegetationAsset {
        model_surfaces: Vec::new().into(),
        schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
        scene_id: draft.scene_id.clone(),
        layers: vec![output()],
        authoring: Some(authoring()),
    }); // 有效正式层与一对一作者图。
    draft
        .vegetation_author_densities
        .insert("base-grass".to_owned(), Arc::new(vec![0; 4])); // 最小R8作者画布。
    let mut session = EditorAuthoringSession {
        draft: Some(draft),
        ..Default::default()
    }; // 未处于保存状态。
    let mut editor = SceneEditorState::default(); // 未选择预设时应沿用选中作者层。
    assert_eq!(
        vegetation_blank_layer_pixel_count(&editor, &session, None).unwrap(),
        4
    ); // Blade后台分配入口可用。
    apply_vegetation_authoring_action_with_blank(&mut editor, &mut session, None, vec![0; 4])
        .unwrap(); // 完整创建并记录撤销。
    let count = session
        .draft
        .as_ref()
        .unwrap()
        .vegetation
        .as_ref()
        .unwrap()
        .authoring
        .as_ref()
        .unwrap()
        .layers
        .len(); // 记录切换正式输出类别前结构。
    let output = &mut session
        .draft
        .as_mut()
        .unwrap()
        .vegetation
        .as_mut()
        .unwrap()
        .layers[0]; // 模拟目录解析后的正式树木输出。
    output.kind = SceneVegetationKind::Mesh;
    output.runtime_profile = Some("hn01_tree".to_owned());
    output.prototype_id = Some(1);
    assert_eq!(
        vegetation_blank_layer_pixel_count(&editor, &session, None).unwrap(),
        4
    ); // Mesh同样只在后台分配R8。
    apply_vegetation_authoring_action_with_blank(&mut editor, &mut session, None, vec![0; 4])
        .expect("Clipmap应允许新建Mesh作者层");
    apply_vegetation_authoring_action(
        EditorAction::VegetationDuplicateLayer,
        &mut editor,
        &mut session,
        None,
    )
    .expect("Clipmap应允许复制Mesh作者层");
    assert_eq!(
        session
            .draft
            .as_ref()
            .unwrap()
            .vegetation
            .as_ref()
            .unwrap()
            .authoring
            .as_ref()
            .unwrap()
            .layers
            .len(),
        count + 2
    ); // 两次Mesh事务都完整采用。
    assert_eq!(session.undo.len(), 3); // Blade新建、Mesh新建和Mesh复制分别可撤销。
} // GLB场景能力另由既有用例保持。
