use bevy::gltf::Gltf;
use bevy::prelude::Handle;
use feathrise_scene_content::ScenePlacementAssetTarget;

use super::*;

fn model_state(path: &str) -> AssetPreviewState {
    AssetPreviewState {
        selected_path: Some(path.to_owned()),
        model: Some(AssetPreviewModelSession::new(1, Handle::<Gltf>::default())),
        ..Default::default()
    }
}

#[test]
fn scene0_and_stale_preview_keep_legacy_placement_target() {
    let state = model_state("gltf/tree.glb");

    assert_eq!(
        state
            .placement_target_for("gltf/tree.glb")
            .expect("Scene0目标应可直接持久化"),
        ScenePlacementAssetTarget::Scene0,
    );
    assert_eq!(
        state
            .placement_target_for("gltf/new-selection.glb")
            .expect("切换途中的旧预览必须安全回退"),
        ScenePlacementAssetTarget::Scene0,
    ); // 旧文件的primitive选择绝不能串到新文件。
}

#[test]
fn selected_primitive_preserves_mesh_primitive_and_material_indices() {
    let mut state = model_state("gltf/plant.glb");
    let choice = AssetPreviewPrimitiveChoice {
        id: AssetPreviewPrimitiveId {
            mesh: 6,
            primitive: 2,
        },
        material: Some(4),
        requires_skinning: false, // 静态植物primitive允许写入精确摆放目标。
    };
    let model = state.model.as_mut().expect("测试模型会话存在");
    model.target = AssetPreviewModelTarget::Primitive(choice.id);
    model.selected_primitive = Some(choice); // 只有目标与已发布目录选择一致时才允许持久化。

    assert_eq!(
        state
            .placement_target_for("gltf/plant.glb")
            .expect("合法数字标签应进入摆放记录"),
        ScenePlacementAssetTarget::Primitive {
            mesh: 6,
            primitive: 2,
            material: Some(4),
            collision: false,
        },
    );
}

#[test] // 即使异常状态绕过点击入口，蒙皮primitive也不能进入场景作者数据。
fn selected_skinned_primitive_is_rejected_as_a_placement_target() {
    // 覆盖持久化前的最后一道编辑器安全门。
    let mut state = model_state("gltf/character.glb"); // 建立当前角色GLB会话。
    let choice = AssetPreviewPrimitiveChoice {
        // 模拟目录已经识别出的蒙皮身体网格。
        id: AssetPreviewPrimitiveId {
            // 使用正式数字标签身份。
            mesh: 0,      // 身体位于Mesh0。
            primitive: 0, // 身体网格的首个primitive。
        }, // 完成精确子资产编号。
        material: Some(0),       // 原关联材质不改变蒙皮限制。
        requires_skinning: true, // JOINTS与WEIGHTS要求完整Skin和关节层级。
    }; // 候选只保存小型分类结果。
    let model = state.model.as_mut().expect("测试模型会话存在"); // 取得唯一会话。
    model.target = AssetPreviewModelTarget::Primitive(choice.id); // 模拟异常调用方直接写入单体目标。
    model.selected_primitive = Some(choice); // 同时冻结对应蒙皮选择。

    let error = state // 使用正式作者目标转换入口。
        .placement_target_for("gltf/character.glb") // 文件身份与当前会话一致。
        .expect_err("蒙皮primitive不能脱离Scene0持久化"); // 必须失败而不是写入危险记录。
    assert!(error.contains("完整场景Scene0")); // 提示必须给作者明确恢复方式。
} // 测试结束时没有修改场景或资产。
