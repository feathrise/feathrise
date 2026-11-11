//! GLTF作者变换与整层替换回归；整层操作明确区别于单字段引用赋值。

use super::*; // 直接验证完整检查结果的唯一生产采用入口。
use feathrise_scene_content::VegetationPresetCatalogAsset; // 正式随包目录作为原值基线。

#[test]
fn mesh_transform_rejects_unrepresentable_pitch() {
    let transform = Transform::from_rotation(Quat::from_rotation_x(0.5)); // 现有格式只表达Y轴旋转。
    assert!(mesh_part_transform(transform).is_err()); // 非法作者姿态必须拒绝。
    assert!(mesh_part_transform(Transform::IDENTITY).is_ok()); // 零变换合法。
}

#[test]
fn blade_requires_applied_node_transform() {
    assert!(blade_transform_is_identity(Transform::IDENTITY)); // 已应用变换的作者网格合法。
    assert!(!blade_transform_is_identity(Transform::from_xyz(
        1.0, 0.0, 0.0
    ))); // Blade无节点平移字段，不能丢失作者位移。
}

/// 独立载入生产预设，不需要运行时注册表或加载作者模型。
fn draft_state(mesh: bool) -> VegetationPresetEditorState {
    let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/vegetation_presets.vegetation_presets.json"
    )))
    .unwrap(); // 沿用真实目录格式。
    let preset = catalog
        .presets
        .into_iter()
        .find(|preset| matches!(preset.runtime, VegetationPresetRuntime::Mesh { .. }) == mesh)
        .unwrap(); // 选择对应后端基线。
    let mut state = VegetationPresetEditorState::default(); // 独立工作区会话。
    state.open_draft(PresetDraft::new(PresetDraftMode::Create, preset)); // 和真实新建/编辑相同目标身份。
    state
}

#[test] // 整层导入不能顺带改写作者来源、本档距离或其他LOD。
fn whole_lod_replacement_preserves_source_and_other_lod_settings() {
    for mesh in [false, true] {
        let mut state = draft_state(mesh); // Blade与Mesh两种替换入口。
        let owner = state.active_preset_tab_id().unwrap(); // 冻结唯一目标标签。
        let mut expected = state.active_draft().unwrap().descriptor.clone(); // 全量参数比较基线。
        let mut parts = Vec::new(); // 只有Mesh整层替换会采用此列表。
        let new_mesh = "replacement.glb#Mesh3/Primitive2".to_owned(); // 不再隐式固定Mesh0。
        match &mut expected.runtime {
            VegetationPresetRuntime::Blade { lods, .. } => lods[0].mesh_path = new_mesh.clone(), // Blade只改当前档路径。
            VegetationPresetRuntime::Mesh { lods, .. } => {
                let mut part = lods[0].parts[0].clone(); // 构造一份可表达的完整受检部件。
                part.mesh_path = new_mesh.clone(); // 新源的真实网格路径。
                part.material_path = "replacement.glb#Material1/std".to_owned(); // 整层导入允许同时替换全部部件的材质。
                parts.push(part); // 一项列表会替代当前档旧的全部部件。
                lods[0].parts = parts.clone(); // 距离与其他档保持未修改。
            }
        }
        let inspected = InspectedAsset {
            source_path: "replacement.glb".to_owned(), // 检查来源不等于作者来源字段的编辑授权。
            blade_mesh_paths: vec![new_mesh],          // 唯一候选可被Blade旧入口接受。
            blade_error: None,                         // 作者变换已通过检查。
            mesh_parts: parts,                         // 完整受检部件。
        };
        apply_inspected_asset(
            &mut state,
            None,
            InspectionPurpose::ReplaceLod {
                tab_id: owner,
                lod: 0,
            },
            inspected,
        )
        .unwrap(); // 完整生产采用路径。
        assert_eq!(state.active_draft().unwrap().descriptor, expected); // 只允许上述对应后端的显式变化。
    }
}

#[test] // 旧Blade整层入口也不能偷偷选GLB的第一个primitive。
fn ambiguous_blade_replacement_is_rejected_without_partial_changes() {
    let mut state = draft_state(false); // 真实Blade预设。
    let owner = state.active_preset_tab_id().unwrap(); // 原目标会话。
    let original = state.active_draft().unwrap().descriptor.clone(); // 所有参数原值。
    let inspected = InspectedAsset {
        source_path: "multi.glb".to_owned(), // 合法GLB但包含歧义。
        blade_mesh_paths: vec![
            "multi.glb#Mesh0/Primitive0".to_owned(),
            "multi.glb#Mesh1/Primitive0".to_owned(),
        ], // 必须由作者明确挑选。
        blade_error: None,                   // 不能借变换错误掩盖多候选问题。
        mesh_parts: Vec::new(),              // Blade不使用PBR部件列表。
    };
    assert!(replace_draft_lod_asset(&mut state, owner, 0, inspected).is_err()); // 明确提示走路径旁的子资产选择。
    assert_eq!(state.active_draft().unwrap().descriptor, original); // 既不改路径也不改作者来源。
}
