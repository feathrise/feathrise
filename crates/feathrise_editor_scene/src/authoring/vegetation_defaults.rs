//! 新建场景与旧编辑器草稿迁移共用的植被作者结构。
//!
//! 【功能设计】新场景从零正式输出开始；作者第一次选择开发者预设并新建画布时，
//! 才把该稳定预设 ID 对应的正式输出写入场景。旧场景只迁移自己已经存在的输出。

use feathrise_scene_content::{
    SceneVegetationAsset, SceneVegetationAuthorGroup, SceneVegetationAuthorGroupMode,
    SceneVegetationAuthorLayer, SceneVegetationAuthoring, SceneVegetationLayer,
    SceneVegetationOutputMergeMode,
}; // 任意数量正式输出和作者画布通过显式引用分离。

const DEFAULT_AUTHOR_GROUP_ID: &str = "base"; // 空白新场景与旧图迁移共享一个可立即接收新层的基础组。

/// 构造新建作者层默认进入的基础组；组本身不代表任何密度画布。
fn default_vegetation_author_group() -> SceneVegetationAuthorGroup {
    SceneVegetationAuthorGroup {
        id: DEFAULT_AUTHOR_GROUP_ID.to_owned(), // 新建第一层无需先额外创建组。
        label: "基础植被".to_owned(),           // 作者可随后拆成北部、南部等独立组。
        mode: SceneVegetationAuthorGroupMode::Additive, // 默认允许不同正式输出共存。
        output_merge: SceneVegetationOutputMergeMode::Maximum, // 同一正式输出的多个区域默认取最大密度。
        capacity: 1.0,        // 共存模式不消费容量，但保存完整兼容参数。
        claim_threshold: 0.0, // 共存模式不认领所有权。
        selection_seed: 0x4D41_505F_4241_5345, // “SCENE_BASE”稳定选择种子。
        selection_cell_meters: 8.0, // 切换覆盖模式后立即拥有可用世界选择格。
        enabled: true,        // 作者新建的第一层默认参与编译。
    }
}

/// 新场景只登记可用组，不预设正式输出、作者图层或 R8 文件。
pub(super) fn empty_vegetation_authoring() -> SceneVegetationAuthoring {
    SceneVegetationAuthoring {
        groups: vec![default_vegetation_author_group()], // 空列表仍能直接接收目录中的任意预设。
        layers: Vec::new(), // 作者必须先选择预设，再显式点击“新空白层”。
    }
}

/// 为旧场景已经登记的正式输出建立一对一作者画布；不从当前目录追加其他预设。
pub(super) fn default_vegetation_authoring(
    layers: &[SceneVegetationLayer],
    folder_relative: &str,
) -> SceneVegetationAuthoring {
    SceneVegetationAuthoring {
        groups: vec![default_vegetation_author_group()], // 迁移层保持在默认组规则中。
        layers: layers
            .iter()
            .map(|layer| author_layer_for_output(layer, folder_relative))
            .collect(), // 场景用过几种内容就迁移几张画布，不补齐目录全集。
    }
}

/// 缺少作者图时按现有正式输出生成一对一结构并返回 true；已有结构逐字保留。
pub(super) fn ensure_vegetation_authoring(
    vegetation: &mut SceneVegetationAsset,
    folder_relative: &str,
) -> bool {
    if vegetation.authoring.is_some() {
        return false; // 已有作者组规则属于权威数据，不做猜测迁移。
    }
    vegetation.authoring = Some(default_vegetation_authoring(
        &vegetation.layers,
        folder_relative,
    )); // 只在作者显式保存后写盘，不因目录新增预设扩大旧场景。
    true
}

fn author_layer_for_output(
    output: &SceneVegetationLayer,
    folder_relative: &str,
) -> SceneVegetationAuthorLayer {
    let id = format!("base-{}", output.id); // 作者 ID 与正式 ID 不同，避免重新合并时自覆盖。
    SceneVegetationAuthorLayer {
        id: id.clone(),                               // 笔刷、撤销和作者 R8 路径共享稳定身份。
        label: format!("{} · 基础", output.label),    // 明确这是可继续拆分的基础作者层。
        group_id: DEFAULT_AUTHOR_GROUP_ID.to_owned(), // 初始一对一共存组。
        output_layer_id: output.id.clone(),           // 编译回原正式预设输出。
        density_source_path: format!(
            "{}/vegetation/author/{id}.r8",
            folder_relative.trim_end_matches('/')
        ), // 私有作者源不覆盖正式运行时密度。
        resolution: output.resolution,                // 首次迁移逐纹素复制现有正式密度。
        enabled: true,                                // 保持旧内容可见。
        locked: false,                                // 作者可立即绘制。
        opacity: 1.0,                                 // 不缩放旧密度。
        priority: 0,                                  // 共存组内优先级无影响。
        weight: 1.0,                                  // 后续切为共享模式时默认一比一。
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_scene_content::{
        SCENE_SCHEMA_VERSION, SceneId, SceneVegetationKind, SceneVegetationLayer,
    };

    fn existing_layer() -> SceneVegetationLayer {
        SceneVegetationLayer {
            id: "existing-tree".to_owned(),
            label: "既有树木".to_owned(),
            kind: SceneVegetationKind::Mesh,
            runtime_profile: Some("developer_tree".to_owned()),
            runtime_layer_id: Some(91),
            prototype_id: Some(92),
            asset_path: "gltf/tree.glb#Scene0".to_owned(),
            density_source_path: "scenes/MAP-test/vegetation/existing-tree.r8".to_owned(),
            density_texture_path: "scenes/MAP-test/vegetation/existing-tree.ktx2".to_owned(),
            resolution: [512, 512],
            spacing_meters: 5.0,
            scale_range: [0.9, 1.1],
            max_instances: u32::MAX,
            seed: 7,
        }
    }

    #[test]
    fn new_scene_authoring_accepts_zero_formal_outputs_and_zero_layers() {
        let authoring = empty_vegetation_authoring();

        assert_eq!(authoring.groups.len(), 1); // 第一张层可直接落入基础组。
        assert!(authoring.layers.is_empty()); // 场景未使用任何预设前不制造作者画布。
        authoring
            .validate(&[])
            .expect("零正式输出与零作者层必须是合法的新场景状态");
    }

    #[test]
    fn legacy_migration_only_creates_layers_for_existing_outputs() {
        let layer = existing_layer();
        let mut vegetation = SceneVegetationAsset {
            schema_version: SCENE_SCHEMA_VERSION,
            scene_id: SceneId::parse("MAP-test").expect("测试场景 ID 合法"),
            layers: vec![layer.clone()],
            authoring: None,
        };

        assert!(ensure_vegetation_authoring(
            &mut vegetation,
            "scenes/MAP-test"
        ));
        let authoring = vegetation.authoring.as_ref().expect("迁移已建立作者结构");
        assert_eq!(vegetation.layers.len(), 1); // 当前开发者目录中的其他预设不会自动写入旧场景。
        assert_eq!(vegetation.layers[0].id, layer.id); // 唯一输出仍是旧文件原有稳定身份。
        assert_eq!(authoring.layers.len(), 1);
        assert_eq!(authoring.layers[0].output_layer_id, "existing-tree");
        assert!(
            authoring.layers[0]
                .density_source_path
                .contains("/vegetation/author/")
        );
        authoring
            .validate(&vegetation.layers)
            .expect("迁移结构必须保持完整引用");
    }

    #[test]
    fn existing_empty_authoring_is_never_repopulated() {
        let mut vegetation = SceneVegetationAsset {
            schema_version: SCENE_SCHEMA_VERSION,
            scene_id: SceneId::parse("MAP-test").expect("测试场景 ID 合法"),
            layers: vec![existing_layer()],
            authoring: Some(empty_vegetation_authoring()),
        };

        assert!(!ensure_vegetation_authoring(
            &mut vegetation,
            "scenes/MAP-test"
        ));
        assert!(
            vegetation
                .authoring
                .as_ref()
                .expect("权威作者结构仍存在")
                .layers
                .is_empty()
        ); // 作者明确删空后，重载不能把任何层复活。
    }
}
