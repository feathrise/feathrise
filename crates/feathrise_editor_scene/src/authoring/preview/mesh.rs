//! Mesh密度层的正式多档、风材质和贴地预览。
//!
//! 【功能设计】每次松笔在后台重新散布选中层；已编辑的其他层继续保留，因此树、灌木和岩石
//! 能同时显示。关闭F12只归还相机，不撤销当前会话预览，游戏画面立刻沿用同一批正式LOD根。

use std::collections::{BTreeMap, BTreeSet}; // 正式输出与已采用预览按稳定层ID确定性选择下一修订。
use std::sync::Arc; // 建筑净空数组浅克隆到唯一后台散布任务。

use bevy::prelude::*; // ECS、Transform和Visibility。
use bevy::tasks::AsyncComputeTaskPool; // 全图候选计算离开主线程。
use feathrise_environment::vegetation::{
    VegetationMeshBatch, VegetationMeshInstance, VegetationMeshPrototypeRegistry,
    VegetationPrototypeId,
}; // 直接生成现有正式多档原型，不加载普通WorldAssetRoot替身。
use feathrise_scene_content::{
    BuildingWorldQuad, SceneId, SceneVegetationAsset, SceneVegetationKind, SceneVegetationLayer,
    generate_vegetation_instances, VegetationPresetRegistry, VegetationPresetRuntime,
};
use feathrise_scene::{
    BuildingVegetationClearanceState, DynamicMeshProfileReadiness,
}; // 素材身份保持不变，场景统一使用确定性散布。

use super::super::resources::{
    EditorAuthoringSession, VegetationBrushStroke, VegetationMeshLayerPreview,
    VegetationPreviewBuild, VegetationPreviewState,
};
use super::super::selected_vegetation_layer; // 作者层必须先映射到已有正式Mesh profile。
use crate::components::EditorVegetationPreview;
use crate::resources::SceneEditorState;
use crate::types::EditorTool;

mod install; // 大批正式LOD根在隐藏父根下分帧生成，再按层原子替换旧预览。
mod source; // 当前地形画布的高度、采样范围与姿态必须来自同一对象。

pub(crate) use install::poll_vegetation_preview_rebuild;

/// 读取一个正式Mesh层生成预览所需的全部受检技术身份。
fn mesh_preview_identity(
    layer: &SceneVegetationLayer, // 正式层提供profile、运行时层和公共原型身份。
) -> Result<(String, u64, u64), String> {
    if layer.kind != SceneVegetationKind::Mesh {
        return Err("GPU叶片层由正式四级窗口即时预览，不生成CPU Mesh替身".to_owned());
        // 该入口绝不降级生成叶片Mesh。
    }
    let runtime_profile = layer
        .runtime_profile
        .clone()
        .ok_or_else(|| format!("植被层{}没有正式运行时profile", layer.id))?; // 未登记正式profile的层整层拒绝。
    let runtime_layer_id = layer
        .runtime_layer_id
        .ok_or_else(|| format!("植被层{}没有正式运行时层ID", layer.id))?; // 生命周期、LOD和风场必须使用稳定层身份。
    let prototype_id = layer
        .prototype_id
        .ok_or_else(|| format!("植被层{}没有正式Mesh原型ID", layer.id))?; // 禁止回退普通GLB Scene 0。
    Ok((
        runtime_profile,    // 完整正式profile名称。
        runtime_layer_id,   // 公共生命周期层身份。
        prototype_id,       // 公共四级Mesh原型身份。
    ))
}

/// 与运行时使用同一工程目录值，不能靠旧游戏 ID 猜测树根、岩石的下扎。
fn mesh_embed_depth(layer: &SceneVegetationLayer, presets: Option<&VegetationPresetRegistry>) -> Result<f32, String> {
    let preset = layer.runtime_profile.as_deref()
        .and_then(|id| presets?.resolve(id))
        .ok_or_else(|| format!("植被层{}等待有效的工程预设目录", layer.id))?;
    match &preset.runtime {
        VegetationPresetRuntime::Mesh { ground_embed_depth, .. } => Ok(*ground_embed_depth),
        _ => Err(format!("植被层{}的工程预设不是 Mesh", layer.id)),
    }
}

/// 零密度正式层无需扫描高度或加载原型，仍返回可原子清除旧预览的完整身份结果。
fn empty_vegetation_preview(
    layer: &SceneVegetationLayer, // 零结果仍必须验证正式profile、层ID和原型ID。
) -> Result<VegetationPreviewBuild, String> {
    let (runtime_profile, runtime_layer_id, prototype_id) =
        mesh_preview_identity(layer)?; // 只解析固定小型描述，不读取任何R8或高度像素。
    Ok(VegetationPreviewBuild {
        runtime_profile,               // 空结果仍持有完整正式profile。
        runtime_layer_id,              // 用于隐藏上一份正式运行时根。
        prototype_id,                  // 保持完成值结构一致；零实例不会实际读取注册表原型。
        instance_ids: Vec::new(),      // 空结果没有待安装稳定身份。
        transforms: Vec::new(),        // 主线程据此只撤销旧预览实体，不生成替身。
    })
}

/// 选择尚未采用当前编译修订的下一个正式Mesh输出，而不是依赖仍存在的作者层。
fn next_mesh_preview_layer<'a>(
    vegetation: &'a SceneVegetationAsset, // 正式层数组在作者层全部删除后仍保持固定。
    nonzero_output_ids: &BTreeSet<String>, // 后台编译顺带生成的非零身份，避免主线程扫描R8。
    selected_output_id: Option<&str>,   // 当前作者层只决定交互优先级，不决定是否需要收敛。
    scene_id: &SceneId,                     // 已采用预览必须属于同一真实场景。
    revision: u64,                      // 所有正式输出最终都应收敛到该编译修订。
    clearance_revision: u64,            // 所有当前预览最终都应收敛到该建筑净空修订。
    preset_revision: u64, // 目录热更新同样重建已采用的预览。
    prototypes: &VegetationMeshPrototypeRegistry, // 同目录内原型登记替换也需重新安装。
    allow_new_layers: bool,             // 非植被工具只换版已经存在的预览层。
    applied: &BTreeMap<String, VegetationMeshLayerPreview>, // 已经可见接管的正式层状态。
) -> Option<(&'a SceneVegetationLayer, bool)> {
    let previewable = |layer: &&SceneVegetationLayer| {
        layer.kind == SceneVegetationKind::Mesh
            && layer.runtime_profile.is_some()
            && layer.runtime_layer_id.is_some()
            && layer.prototype_id.is_some() // 坏正式层继续由现有明确诊断路径拒绝。
    };
    let needs_revision = |layer: &&SceneVegetationLayer| {
        applied.get(&layer.id).map_or(allow_new_layers, |current| {
            current.scene_id != *scene_id
                || current.applied_revision != revision
                || current.clearance_revision != clearance_revision
                || current.preset_revision != preset_revision
                || current.prototype_revision != if nonzero_output_ids.contains(&layer.id) {
                    layer.prototype_id.and_then(|id| prototypes.revision(VegetationPrototypeId(id))).unwrap_or(0)
                } else { 0 }
        }) // 建筑工具只重建当前可见预览；植被工具仍会补齐尚未显示的正式层。
    };
    let layers = || {
        vegetation
            .layers
            .iter()
            .filter(previewable)
            .filter(needs_revision)
    }; // 每次只遍历固定最多三十二个正式层。
    let candidate = layers()
        .find(|layer| !nonzero_output_ids.contains(&layer.id)) // 零输出先清理，不被等待新原型的非零层饿死。
        .or_else(|| layers().find(|layer| applied.contains_key(&layer.id)))
        .or_else(|| {
            selected_output_id.and_then(|selected| layers().find(|layer| layer.id == selected))
            // 再响应作者当前操作层。
        })
        .or_else(|| layers().next())?; // 最后让同次编译影响的其他正式Mesh层依次收敛。
    Some((candidate, nonzero_output_ids.contains(&candidate.id))) // bool决定是否需要真实散布与原型。
}

#[allow(clippy::too_many_arguments)]
fn build_vegetation_preview(
    layer: &SceneVegetationLayer,
    density: &[u8],
    heights: &[u16],
    terrain_resolution: [u32; 2],
    world_min: [f32; 2],
    world_max: [f32; 2],
    base_height: f32,
    height_scale: f32,
    source_transform: Transform,
    clearances: &[BuildingWorldQuad],
    embed_depth: f32, // 冻结工程目录的受检深度，后台不访问 ECS。
) -> Result<VegetationPreviewBuild, String> {
    let (runtime_profile, runtime_layer_id, prototype_id) =
        mesh_preview_identity(layer)?; // 零图和普通散布共享完全相同的正式身份校验。
    let layout_transforms = generate_vegetation_instances(
        layer, density, heights, terrain_resolution, world_min, world_max, base_height, height_scale,
    )?;
    let mut instance_ids = Vec::with_capacity(layout_transforms.len()); // 保留全图候选格稳定编号。
    let mut filtered_layout = Vec::with_capacity(layout_transforms.len()); // 完整结果在后台一次收口。
    for value in layout_transforms {
        let world = source_transform.transform_point(Vec3::from_array(value.translation));
        let point = Vec2::new(world.x, world.z); // 源坐标只变换一次后进入世界建筑净空。
        if clearances.iter().any(|quad| quad.contains(point)) {
            continue; // 只过滤最终派生实例，不改写作者密度。
        }
        instance_ids.push(value.instance_id); // 净空过滤不改变其他候选的身份。
        filtered_layout.push(value); // 可见布局与稳定身份保持严格同长。
    }
    let transforms = filtered_layout
        .into_iter()
        .map(|value| {
            let mut translation = Vec3::from_array(value.translation);
            translation.y -= embed_depth; // 与正式根部下扎契约一致，消除坡面细缝。
            source_transform.mul_transform(Transform::from_translation(translation)
                .with_rotation(Quat::from_rotation_y(value.yaw_radians))
                .with_scale(Vec3::splat(value.scale)))
        })
        .collect();
    Ok(VegetationPreviewBuild {
        runtime_profile,
        runtime_layer_id,
        prototype_id,
        instance_ids,
        transforms,
    })
}

/// 松笔后为当前正式Mesh密度层提交唯一后台任务；关闭F12时保留已采用结果给游戏画面。
pub(crate) fn start_vegetation_preview_rebuild(
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    stroke: Res<VegetationBrushStroke>,
    registry: Res<VegetationMeshPrototypeRegistry>,
    presets: Option<Res<VegetationPresetRegistry>>,
    profile_readiness: DynamicMeshProfileReadiness,
    clearances: Res<BuildingVegetationClearanceState>,
    mut preview: ResMut<VegetationPreviewState>,
) {
    if !editor.open || stroke.active {
        return;
    }
    if session.vegetation_compiled_revision != session.vegetation_revision {
        return; // 作者笔划或组参数尚未完整编译时继续显示上一正式预览，绝不散布半成品。
    }
    let Some(draft_scene_id) = session.draft.as_ref().map(|draft| draft.scene_id.clone()) else {
        return;
    };
    let clearance_snapshot = clearances.snapshot_for(&draft_scene_id); // 作者BVH优先，尚未发布时使用显式空净空。
    let clearance_revision = clearance_snapshot
        .as_ref()
        .map_or(0, |snapshot| snapshot.revision);
    let clearance_regions = clearance_snapshot.as_ref().map_or_else(
        || Arc::from(Vec::<BuildingWorldQuad>::new()),
        |snapshot| snapshot.regions.clone(),
    ); // 后台任务只浅克隆完整不可变数组。
    let allow_new_layers = editor.tool == EditorTool::Vegetation; // 建筑操作不会突然为全部未编辑层创建F12接管根。
    if !allow_new_layers && preview.layers.is_empty() {
        return; // 没有当前Mesh预览时由正式动态常驻路径直接消费作者净空。
    }
    let Some((
        scene_id,
        layer,
        nonzero,
        density,
        terrain_source,
    )) = session.draft.as_ref().and_then(|draft| {
        let vegetation = draft.vegetation.as_ref()?; // 作者层可为空，但固定正式输出必须继续存在。
        let selected_output_id = selected_vegetation_layer(draft, editor.selected_vegetation_layer)
            .map(|selected| selected.output.id.as_str()); // 当前作者层只用于优先显示，不再成为唯一入口。
        let (layer, nonzero) = next_mesh_preview_layer(
            vegetation,                           // 固定正式输出全集。
            &draft.vegetation_nonzero_layers,     // 后台编译的非零身份。
            selected_output_id,                   // 可为空，删除最后作者层仍继续收敛。
            &draft.scene_id,                        // 防止旧图预览冒充当前图。
            session.vegetation_compiled_revision, // 当前完整正式输出修订。
            clearance_revision,                   // 当前完整建筑净空修订。
            presets.as_deref().map_or(0, |presets| presets.revision),
            &registry,
            allow_new_layers,                     // 仅植被工具补齐尚未显示的层。
            &preview.layers,                      // 已显示层需要替换旧实体。
        )?;
        Some((
            draft.scene_id.clone(),
            layer.clone(),
            nonzero, // 零输出使用常数完成值，不扫描整图或等待原型。
            draft.vegetation_densities.get(&layer.id)?.clone(),
            source::capture(draft)?, // 高度来自当前画布，容器的empty描述不能提供采样规格。
        ))
    })
    else {
        return;
    };
    let preset_revision = presets.as_deref().map_or(0, |presets| presets.revision);
    let prototype_revision = if nonzero {
        let ready = layer.runtime_profile.as_deref().and_then(|profile| {
            profile_readiness.ready_revision(profile, VegetationPrototypeId(layer.prototype_id?), presets.as_deref()?, &registry)
        });
        let Some(revision) = ready else {
            let detail = layer.runtime_profile.as_deref().and_then(|profile|
                profile_readiness.failure_detail(profile, presets.as_deref()?));
            let status = detail.map_or_else(
                || format!("植被层{}等待当前目录对应的完整 Mesh 原型；保留上一完整预览", layer.id),
                |error| format!("正式 Mesh 预览加载失败：{error}；再次点选画布可重试"));
            if editor.status != status { editor.status = status; }
            return;
        };
        revision
    } else { 0 }; // 零输出只清理旧节点，不借用任何原型就绪信用。
    let embed_depth = if nonzero {
        match mesh_embed_depth(&layer, presets.as_deref()) {
            Ok(depth) => depth,
            Err(error) => { editor.status = error; return; }
        }
    } else { 0.0 };
    preview.desired_scene_id = Some(scene_id.clone());
    preview.desired_layer_id = Some(layer.id.clone());
    preview.desired_revision = session.vegetation_compiled_revision;
    preview.desired_clearance_revision = clearance_revision;
    if preview.task.is_some()
        || preview.install.is_some() // 隐藏父根尚未完整排入时保持single-flight，不再提交第二份全层结果。
        || preview.layers.get(&layer.id).is_some_and(|applied| {
            applied.scene_id == scene_id
                && applied.applied_revision == session.vegetation_compiled_revision
                && applied.clearance_revision == clearance_revision
                && applied.preset_revision == preset_revision
                && applied.prototype_revision == prototype_revision
        })
    {
        return;
    }
    let layer_id = layer.id.clone();
    let revision = session.vegetation_compiled_revision; // 后台任务身份绑定原子完成的正式输出修订。
    preview.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let result = if nonzero {
            build_vegetation_preview(
                &layer,
                &density,
                &terrain_source.heights,
                terrain_source.terrain.resolution,
                terrain_source.world_min,
                terrain_source.world_max,
                terrain_source.terrain.base_height,
                terrain_source.terrain.height_scale,
                terrain_source.transform,
                &clearance_regions,
                embed_depth,
            )
        } else {
            empty_vegetation_preview(&layer) // 删除全部作者层后以常数空结果清除旧树、灌木和岩石。
        };
        (scene_id, layer_id, revision, clearance_revision, preset_revision, prototype_revision, result)
    }));
}

/// 动态场景保存后正式运行时会热生成同一层的新根；预览仍托管当前作者会话时只隐藏新增根，
/// 避免正式根与所见即所得预览重叠成双份植被。查询只处理`Added`，稳定帧不扫描全世界。
pub(crate) fn suppress_new_runtime_mesh_roots(
    mut commands: Commands,
    roots: Query<
        (Entity, &VegetationMeshInstance, &Visibility),
        (
            Added<VegetationMeshInstance>,
            Without<EditorVegetationPreview>,
        ),
    >,
    batch_roots: Query<
        (Entity, &VegetationMeshBatch, &Visibility),
        (Added<VegetationMeshBatch>, Without<EditorVegetationPreview>),
    >,
    existing_roots: Query<(), Or<(With<VegetationMeshInstance>, With<VegetationMeshBatch>)>>,
    mut preview: ResMut<VegetationPreviewState>,
) {
    for (entity, instance, visibility) in &roots {
        let Some(layer) = preview
            .layers
            .values_mut()
            .find(|layer| layer.runtime_layer_id == instance.layer.0)
        else {
            continue;
        };
        layer
            .hidden_entities
            .retain(|(hidden, _)| existing_roots.contains(*hidden));
        if layer
            .hidden_entities
            .iter()
            .any(|(hidden, _)| *hidden == entity)
        {
            continue;
        }
        layer.hidden_entities.push((entity, *visibility));
        commands.entity(entity).insert(Visibility::Hidden);
    }
    for (entity, batch, visibility) in &batch_roots {
        let Some(layer) = preview
            .layers
            .values_mut()
            .find(|layer| layer.runtime_layer_id == batch.layer.0)
        else {
            continue;
        };
        layer
            .hidden_entities
            .retain(|(hidden, _)| existing_roots.contains(*hidden));
        if layer
            .hidden_entities
            .iter()
            .any(|(hidden, _)| *hidden == entity)
        {
            continue;
        }
        layer.hidden_entities.push((entity, *visibility));
        commands.entity(entity).insert(Visibility::Hidden); // F12预览已托管该层时，新常驻批次首帧保持隐藏。
    }
}

/// 离开当前场景时终止迟到任务、恢复尚存旧根并清空接管记录，防止作者预览串图。
pub(crate) fn clear_vegetation_previews(
    mut commands: Commands,
    mut preview: ResMut<VegetationPreviewState>,
) {
    preview.task = None;
    if let Some(install) = preview.install.take() {
        commands.entity(install.container).try_despawn(); // 离图时完整回收尚未显示的隐藏分帧候选。
    }
    preview.desired_scene_id = None;
    preview.desired_layer_id = None;
    preview.desired_clearance_revision = 0;
    let layer_ids = preview.layers.keys().cloned().collect::<Vec<_>>();
    for layer_id in layer_ids {
        let Some(mut layer) = preview.layers.remove(&layer_id) else {
            continue;
        };
        commands.entity(layer.container).try_despawn(); // 每层预览由唯一父根整体回收。
        for (entity, visibility) in layer.hidden_entities.drain(..) {
            commands.entity(entity).try_insert(visibility);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_mesh_embed_depth_comes_from_project_catalog_and_moves_preview_roots() {
        use feathrise_scene_content::{VegetationPresetCatalogAsset, ResolvedVegetationPresetCatalog};
        let mut asset: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/vegetation_presets.vegetation_presets.json"
        ))).unwrap();
        let mut preset = asset.presets.iter().find(|preset| preset.kind() == SceneVegetationKind::Mesh).unwrap().clone();
        preset.id = "project_crystal".into();
        preset.aliases.clear();
        preset.output_id = "project_crystal".into();
        if let VegetationPresetRuntime::Mesh { ground_embed_depth, .. } = &mut preset.runtime { *ground_embed_depth = 0.37; }
        asset.presets = vec![preset];
        let presets = VegetationPresetRegistry { catalog: Handle::default(), revision: 3,
            resolved: ResolvedVegetationPresetCatalog::build(&asset).unwrap(), last_error: None };
        let layer = presets.resolve("project_crystal").unwrap().create_scene_layer("scenes/custom");
        let depth = mesh_embed_depth(&layer, Some(&presets)).unwrap();
        assert_eq!(depth, 0.37);
        let mut layer = layer;
        layer.resolution = [2; 2];
        layer.spacing_meters = 1.0;
        let result = build_vegetation_preview(&layer, &[255; 4], &[0; 9], [3; 2], [0.; 2], [2.; 2], 0., 1., Transform::IDENTITY, &[], depth).unwrap();
        assert!(!result.transforms.is_empty());
        assert!(result.transforms.iter().all(|transform| (transform.translation.y + 0.37).abs() < 0.0001));
        assert!(mesh_embed_depth(&layer, None).is_err());
    }

    fn mesh_layer(id: &str, runtime_layer_id: u64) -> SceneVegetationLayer {
        SceneVegetationLayer {
            id: id.to_owned(),                                            // 固定正式输出身份。
            label: id.to_owned(),                                         // 显示名不参与候选选择。
            kind: SceneVegetationKind::Mesh,                                // 进入正式Mesh预览链路。
            runtime_profile: Some(format!("profile-{id}")), // 保持可预览技术身份完整。
            runtime_layer_id: Some(runtime_layer_id),       // 用于隐藏旧正式根。
            prototype_id: Some(runtime_layer_id + 100),     // 零输出不会实际加载该原型。
            asset_path: format!("{id}.glb"),                // 候选选择不读取资产。
            density_source_path: format!("scenes/test/vegetation/{id}.r8"), // 正式R8源。
            density_texture_path: format!("scenes/test/vegetation/{id}.ktx2"), // 正式派生纹理。
            resolution: [2, 2],                             // 最小合法测试尺寸。
            spacing_meters: 1.0,                            // 候选选择不散布实例。
            scale_range: [1.0, 1.0],                        // 保持完整正式描述。
            max_instances: 4,                               // 保持完整正式描述。
            seed: 1,                                        // 保持确定性正式描述。
        }
    }

    #[test]
    fn zero_mesh_outputs_still_converge_without_any_selected_author_layer() {
        let scene_id = SceneId::parse("MAP-test").expect("测试场景ID有效");
        let vegetation = SceneVegetationAsset {
            schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,                                           // 当前作者格式。
            scene_id: scene_id.clone(),                                      // 正式输出属于当前场景。
            layers: vec![mesh_layer("tree", 7), mesh_layer("shrub", 8)], // 作者层删除后仍保留的固定输出。
            authoring: None, // 候选纯函数不应依赖作者层或当前选中项。
        };
        let mut applied = BTreeMap::new();
        let revision = 5;
        let clearance_revision = 9; // 候选测试冻结同一建筑净空代次，验证逐层收敛。

        let (tree, tree_nonzero) = next_mesh_preview_layer(
            &vegetation,
            &BTreeSet::new(), // 两个正式输出都由编译器标记为零。
            None,             // 删除最后作者层后没有选中输出。
            &scene_id,
            revision,
            clearance_revision,
            0,
            &VegetationMeshPrototypeRegistry::default(),
            true, // 植被工具允许首次建立尚未显示的正式层。
            &applied,
        )
        .expect("零正式Mesh输出仍必须进入清理采用链路");
        assert_eq!(tree.id, "tree");
        assert!(!tree_nonzero); // 清理路径不得等待或加载树木原型。
        applied.insert(
            tree.id.clone(),
            VegetationMeshLayerPreview {
                scene_id: scene_id.clone(),
                runtime_layer_id: tree.runtime_layer_id.expect("测试层身份完整"),
                applied_revision: revision,
                preset_revision: 0,
                prototype_revision: 0,
                clearance_revision,   // 第二次选择应把同代树层视为已经收敛。
                instance_count: 0,    // 纯候选测试不生成实例，缓存数量保持零。
                batch_count: 0,       // 零输出没有非空批次。
                render_node_count: 0, // 零输出没有渲染承接节点。
                container: Entity::PLACEHOLDER, // 纯候选测试不执行ECS回收，只需完整活动层结构。
                hidden_entities: Vec::new(), // 真实系统会在采用时保存被隐藏旧根。
            },
        );

        let (shrub, shrub_nonzero) = next_mesh_preview_layer(
            &vegetation,
            &BTreeSet::new(),
            None,
            &scene_id,
            revision,
            clearance_revision,
            0,
            &VegetationMeshPrototypeRegistry::default(),
            true, // 同次编译继续补齐尚未显示的灌木层。
            &applied,
        )
        .expect("同一修订还应继续清理灌木输出");
        assert_eq!(shrub.id, "shrub");
        assert!(!shrub_nonzero);
    }

    #[test]
    fn a_new_prototype_registration_rebuilds_the_applied_layer_even_in_the_same_catalog() {
        use feathrise_environment::vegetation::VegetationMeshPrototype;
        let scene_id = SceneId::parse("MAP-prototype-revision").unwrap();
        let layer = mesh_layer("project_crystal", 7);
        let vegetation = SceneVegetationAsset { schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION, scene_id: scene_id.clone(), layers: vec![layer], authoring: None };
        let mut registry = VegetationMeshPrototypeRegistry::default();
        let id = VegetationPrototypeId(107);
        registry.register(VegetationMeshPrototype { id, lods: vec![] });
        let revision = registry.revision(id).unwrap();
        let applied = BTreeMap::from([("project_crystal".into(), VegetationMeshLayerPreview {
            scene_id: scene_id.clone(), runtime_layer_id: 7, applied_revision: 3, clearance_revision: 4,
            preset_revision: 5, prototype_revision: revision, instance_count: 1, batch_count: 1,
            render_node_count: 1, container: Entity::PLACEHOLDER, hidden_entities: vec![],
        })]);
        let nonzero = BTreeSet::from(["project_crystal".into()]);
        assert!(next_mesh_preview_layer(&vegetation, &nonzero, None, &scene_id, 3, 4, 5, &registry, true, &applied).is_none());
        registry.register(VegetationMeshPrototype { id, lods: vec![] });
        assert!(next_mesh_preview_layer(&vegetation, &nonzero, None, &scene_id, 3, 4, 5, &registry, true, &applied).is_some());
        let mut vegetation = vegetation;
        vegetation.layers.push(mesh_layer("empty_layer", 8));
        assert_eq!(next_mesh_preview_layer(&vegetation, &nonzero, None, &scene_id, 3, 4, 5, &registry, true, &applied).unwrap().0.id,
            "empty_layer", "零输出清理不能被等待新目录模型的旧可见层饿死");
    }

    #[test] // F12动态预览必须完整保留作者候选，并在重建后得到逐值相同结果。
    fn deterministic_hash_is_stable_and_not_instance_capped() {
        let layer = SceneVegetationLayer {
            id: "willow".to_owned(),
            label: "柳树".to_owned(),
            kind: SceneVegetationKind::Mesh,
            runtime_profile: Some("test_tree".to_owned()),
            runtime_layer_id: Some(7),
            prototype_id: Some(9),
            asset_path: "gltf/vegetation/willow.glb".to_owned(),
            density_source_path: "scenes/test/vegetation/willow.r8".to_owned(),
            density_texture_path: "scenes/test/vegetation/willow.ktx2".to_owned(),
            resolution: [2, 2],
            spacing_meters: 1.0, // 十米方图形成一百个满密度候选。
            scale_range: [0.8, 1.2],
            max_instances: 1, // 故意保留极小旧兼容值，动态预览不得读取它截断。
            seed: 42,
        };
        let build = || {
            build_vegetation_preview(
                &layer,
                &[255; 4],
                &[u16::MAX / 2; 9],
                [3, 3],
                [0.0, 0.0],
                [10.0, 10.0],
                -10.0,
                20.0,
                Transform::IDENTITY,
                &[], // 无建筑净空时保持原确定性候选数量。
                0.0,
            )
            .expect("完整密度与高度输入应生成预览")
        };
        let first = build();
        let second = build();
        assert_eq!(first.runtime_profile, second.runtime_profile);
        assert_eq!(first.runtime_layer_id, second.runtime_layer_id);
        assert_eq!(first.prototype_id, second.prototype_id);
        assert_eq!(first.transforms.len(), 100); // 一百个候选全部显示，旧一根字段不形成隐藏上限。
        assert_eq!(first.transforms.len(), second.transforms.len());
        for (left, right) in first.transforms.iter().zip(&second.transforms) {
            assert_eq!(left.translation, right.translation);
            assert_eq!(left.rotation, right.rotation);
            assert_eq!(left.scale, right.scale);
        }
    }
}
