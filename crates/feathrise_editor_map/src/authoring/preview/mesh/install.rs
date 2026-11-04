//! F12 Mesh密度预览的隐藏分帧安装与按层原子接管。

use std::collections::BTreeMap; // 动态预览按确定性二维块顺序生成共享实例批次。

use bevy::prelude::*; // ECS、父子关系、可见性与场景退出生命周期。
use bevy::tasks::futures::check_ready; // 主线程只非阻塞轮询完整CPU散布结果。
use feathrise_environment::vegetation::{
    VegetationLayerId, VegetationMeshBatch,
    VegetationMeshBatchPlacement, VegetationMeshInstance, VegetationMeshPrototypeRegistry,
    VegetationPrototypeId, spawn_vegetation_mesh_batch,
}; // 预览直接生成正式四级LOD和风材质原型。
use feathrise_scene_core::state::ScenePhase; // 隐藏或活动预览父根随当前地图场景完整回收。

use super::super::super::resources::{
    VegetationMeshLayerPreview, VegetationPreviewInstall, VegetationPreviewState,
}; // CPU任务、隐藏安装和活动层使用显式不同状态。
use crate::components::EditorVegetationPreview; // 预览根不得被正式运行时新增根抑制系统误识别。
use crate::constants::EDITOR_VEGETATION_PREVIEW_LAYER_BIT; // 运行时拾取与预览安装共用最高位隔离语义。
use crate::resources::MapEditorState; // 状态栏回显真实分帧进度与最终数量。

const EDITOR_PREVIEW_BATCH_CHUNK_METERS: f32 = 128.0; // 与正式动态运行时同级空间粒度，保留视锥与回收局部性。

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use bevy::tasks::TaskPoolBuilder;
    use feathrise_map_content::{MapId, VegetationPresetRegistry, ResolvedVegetationPresetCatalog};
    use crate::authoring::resources::VegetationPreviewBuild;

    fn empty_build() -> VegetationPreviewBuild {
        VegetationPreviewBuild { runtime_profile: "project_crystal".into(), runtime_layer_id: 7,
            prototype_id: 9, instance_ids: Vec::new(), transforms: Vec::new() }
    }

    #[test]
    fn catalog_change_rejects_old_background_result_and_hidden_install() {
        let pool = TaskPoolBuilder::new().num_threads(1).build();
        let map = MapId::parse("MAP-custom-preset").unwrap();
        let task_map = map.clone();
        let mut world = World::new();
        world.init_resource::<VegetationMeshPrototypeRegistry>();
        world.init_resource::<MapEditorState>();
        world.insert_resource(State::new(ScenePhase::Active));
        world.insert_resource(VegetationPresetRegistry { catalog: Handle::default(), revision: 2,
            resolved: ResolvedVegetationPresetCatalog::default(), last_error: None });
        world.insert_resource(VegetationPreviewState { desired_map_id: Some(map.clone()),
            desired_layer_id: Some("crystal".into()), desired_revision: 3, desired_clearance_revision: 4,
            task: Some(pool.spawn(async move { (task_map, "crystal".into(), 3, 4, 1, 0, Ok(empty_build())) })), ..default() });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while world.resource::<VegetationPreviewState>().task.is_some() {
            world.run_system_once(poll_vegetation_preview_rebuild).unwrap();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(world.resource::<VegetationPreviewState>().layers.is_empty());
        let container = world.spawn((EditorVegetationPreview, Visibility::Hidden)).id();
        world.resource_mut::<VegetationPreviewState>().install = Some(VegetationPreviewInstall {
            map_id: map, layer_id: "crystal".into(), revision: 3, clearance_revision: 4, preset_revision: 1, prototype_revision: 0,
            build: empty_build(), container, cursor: 0,
        });
        world.run_system_once(poll_vegetation_preview_rebuild).unwrap();
        assert!(world.get_entity(container).is_err());
        assert!(world.resource::<VegetationPreviewState>().layers.is_empty());
    }
}

fn preview_batch_chunk_id(x: i32, z: i32) -> u64 {
    ((x as u32 as u64) << 32) | z as u32 as u64
}

/// 采用完整作者散布并仅替换同一正式输出层；其他Mesh层继续同时显示。
#[allow(clippy::too_many_arguments)]
pub(crate) fn poll_vegetation_preview_rebuild(
    mut commands: Commands,
    registry: Res<VegetationMeshPrototypeRegistry>,
    presets: Option<Res<feathrise_map_content::VegetationPresetRegistry>>,
    profile_readiness: feathrise_scene::DynamicMeshProfileReadiness,
    mut editor: ResMut<MapEditorState>,
    roots: Query<(Entity, &VegetationMeshInstance, &Visibility), Without<EditorVegetationPreview>>,
    batch_roots: Query<
        (Entity, &VegetationMeshBatch, &Visibility),
        Without<EditorVegetationPreview>,
    >,
    mut preview: ResMut<VegetationPreviewState>,
    scene: Res<State<ScenePhase>>,
) {
    let preset_revision = presets.as_deref().map_or(0, |presets| presets.revision);
    let prototype_ready = |build: &crate::authoring::resources::VegetationPreviewBuild, revision| {
        build.transforms.is_empty() || presets.as_deref().is_some_and(|presets| {
            profile_readiness.ready_revision(&build.runtime_profile, VegetationPrototypeId(build.prototype_id), presets, &registry) == Some(revision)
                && registry.revision(VegetationPrototypeId(build.prototype_id)) == Some(revision)
        })
    };
    let stale_install = preview.install.as_ref().is_some_and(|install| {
        preview.desired_map_id.as_ref() != Some(&install.map_id)
            || preview.desired_layer_id.as_deref() != Some(install.layer_id.as_str())
            || preview.desired_revision != install.revision
            || preview.desired_clearance_revision != install.clearance_revision
            || preset_revision != install.preset_revision
            || !prototype_ready(&install.build, install.prototype_revision)
    }); // 新笔划、切层或切图都使旧隐藏父根立即失去采用资格。
    if stale_install {
        if let Some(install) = preview.install.take() {
            commands.entity(install.container).try_despawn(); // 半套旧预览从未显示，可整体安全丢弃。
        }
    }

    if preview.install.is_none() {
        let completed = preview.task.as_mut().and_then(check_ready); // 每帧对唯一CPU任务执行常数次非阻塞轮询。
        if let Some((map_id, layer_id, revision, clearance_revision, completed_preset_revision, prototype_revision, result)) = completed {
            preview.task = None; // 完成后释放single-flight CPU槽。
            if preview.desired_map_id.as_ref() != Some(&map_id)
                || preview.desired_layer_id.as_deref() != Some(layer_id.as_str())
                || preview.desired_revision != revision
                || preview.desired_clearance_revision != clearance_revision
                || preset_revision != completed_preset_revision
            {
                return; // 迟到完整结果按地图、层和修订身份直接拒收。
            }
            let build = match result {
                Ok(build) => build,
                Err(error) => {
                    let detail = format!("正式植被预览散布失败：map={map_id} layer={layer_id} revision={revision} clearance={clearance_revision}：{error}；密度刷印仍保留可编辑");
                    if editor.status != detail { warn!("{detail}"); }
                    editor.status = detail;
                    return; // 保留完整底层原因与来源，不能把失败误报成空散布或仅覆盖泛化文案。
                }
            };
            if !prototype_ready(&build, prototype_revision) { return; } // 已完成的旧模型结果也不能被新目录采用。
            if !build.transforms.is_empty()
                && registry
                    .get(VegetationPrototypeId(build.prototype_id))
                    .is_none()
            {
                editor.status = format!(
                    "正式植被原型{}尚未就绪；不会用普通GLB替代",
                    build.runtime_profile
                );
                return; // 大资源在CPU任务期间丢失时保留上一完整层。
            }
            let container = commands
                .spawn((
                    Name::new(format!("植被预览候选_{layer_id}")), // 层级诊断可直接定位正在安装的正式输出。
                    Transform::IDENTITY,         // 子实例继续使用完整世界变换。
                    Visibility::Hidden,          // 全部作者实例排入前不显示半套结果。
                    EditorVegetationPreview,     // 正式根抑制查询始终排除该父根。
                    DespawnOnExit(*scene.get()), // 离图时即使编辑器资源未轮询也完整回收。
                ))
                .id();
            preview.install = Some(VegetationPreviewInstall {
                map_id,             // 冻结CPU结果所属地图。
                layer_id,           // 冻结本次只替换的正式输出层。
                revision,           // 冻结完整作者编译修订。
                clearance_revision,
                prototype_revision,
                preset_revision: completed_preset_revision, // 冻结散布时使用的工程目录修订。
                build,              // 变换数组在安装期间只保留这一份。
                container,          // 后续只切换该父根可见性。
                cursor: 0,          // 首帧从第一个作者实例开始排入。
            });
        }
    }

    let Some(mut install) = preview.install.take() else {
        return; // CPU任务未完成或当前没有待采用层。
    };
    let prototype = if install.build.transforms.is_empty() {
        None // 零实例结果只清理旧层，不依赖正式原型继续存在。
    } else {
        let Some(prototype) = registry.get(VegetationPrototypeId(install.build.prototype_id))
        else {
            commands.entity(install.container).try_despawn(); // 原型中途丢失时整块隐藏结果作废。
            editor.status = format!(
                "正式植被原型{}尚未就绪；保留上一完整预览",
                install.build.runtime_profile
            );
            return;
        };
        Some(prototype) // 所有空间批次共享同一份已就绪原型。
    };
    let preview_layer =
        VegetationLayerId(install.build.runtime_layer_id | EDITOR_VEGETATION_PREVIEW_LAYER_BIT); // 预览与正式层生命周期身份隔离。
    if install.build.transforms.len() != install.build.instance_ids.len()
    {
        commands.entity(install.container).try_despawn();
        editor.status = "正式植被预览的视觉变换与稳定身份数量不一致".to_owned();
        return; // 内部完成值失配不能靠zip静默丢掉作者实例。
    }
    let mut batch_count = 0usize;
    let mut render_node_count = 0usize;
    if install.cursor == 0 {
        if !install.build.transforms.is_empty() {
            let mut chunks = BTreeMap::<(i32, i32), Vec<VegetationMeshBatchPlacement>>::new();
            for (transform, instance_id) in install
                .build
                .transforms
                .iter()
                .zip(&install.build.instance_ids)
            {
                let x =
                    (transform.translation.x / EDITOR_PREVIEW_BATCH_CHUNK_METERS).floor() as i32;
                let z =
                    (transform.translation.z / EDITOR_PREVIEW_BATCH_CHUNK_METERS).floor() as i32;
                chunks
                    .entry((x, z))
                    .or_default()
                    .push(VegetationMeshBatchPlacement::new(*instance_id, *transform));
            } // 一次线性分组完整保留逐株值，ECS规模只随非空空间块增长。
            batch_count = chunks.len();
            let nodes_per_batch = prototype
                .expect("非空变换结果必须在上方取得正式原型")
                .lods
                .iter()
                .map(|lod| lod.parts.len())
                .sum::<usize>();
            render_node_count = batch_count.saturating_mul(nodes_per_batch);
            for ((x, z), placements) in chunks {
                let batch = spawn_vegetation_mesh_batch(
                    &mut commands,
                    preview_layer,
                    prototype.expect("非空变换结果必须在上方取得正式原型"),
                    preview_batch_chunk_id(x, z),
                    placements,
                    Transform::IDENTITY,
                );
                commands
                    .entity(batch)
                    .insert((EditorVegetationPreview, ChildOf(install.container)));
            }
        }
        install.cursor = install.build.transforms.len(); // 作者数量完整保留，安装成本只随一个实例缓冲增长。
    }

    let mut hidden_entities = Vec::new(); // 当前预览首次接管时保存原正式根显隐。
    if let Some(mut previous) = preview.layers.remove(&install.layer_id) {
        commands.entity(previous.container).try_despawn(); // 上一完整预览由唯一父根整体回收。
        if previous.map_id == install.map_id
            && previous.runtime_layer_id == install.build.runtime_layer_id
        {
            hidden_entities = previous.hidden_entities; // 同层换版继续沿用第一次保存的正式根原显隐。
        } else {
            for (entity, visibility) in previous.hidden_entities.drain(..) {
                commands.entity(entity).try_insert(visibility); // 串图或层身份变化先归还旧正式根。
            }
        }
    }
    if hidden_entities.is_empty() {
        for (entity, instance, visibility) in &roots {
            if instance.layer.0 != install.build.runtime_layer_id {
                continue; // 其他正式植被层保持同时显示。
            }
            hidden_entities.push((entity, *visibility)); // 保存正式根原显隐供离图恢复。
            commands.entity(entity).insert(Visibility::Hidden); // 与新父根显示在同一命令边界完成接管。
        }
        for (entity, batch, visibility) in &batch_roots {
            if batch.layer.0 != install.build.runtime_layer_id {
                continue;
            }
            hidden_entities.push((entity, *visibility)); // 正式动态块批次同样由F12预览按层原子接管。
            commands.entity(entity).insert(Visibility::Hidden);
        }
    }
    commands
        .entity(install.container)
        .insert(Visibility::Inherited); // 一次父根切换显示完整新层，不逐株闪现。
    let count = install.build.transforms.len(); // 最终数量就是作者密度选中的全部候选。
    let runtime_profile = install.build.runtime_profile.clone(); // 状态文字在移动安装值前冻结。
    preview.layers.insert(
        install.layer_id,
        VegetationMeshLayerPreview {
            map_id: install.map_id, // 活动预览严格绑定真实地图身份。
            runtime_layer_id: install.build.runtime_layer_id, // 后续正式热更新根按同层隐藏。
            applied_revision: install.revision, // 当前完整编译修订已经可见采用。
            clearance_revision: install.clearance_revision,
            prototype_revision: install.prototype_revision,
            preset_revision: install.preset_revision, // 当前工程目录深度已经随完整结果采用。
            instance_count: count,  // F4复用后台散布终点，不在刷新时查询全部实例根。
            batch_count,            // 记录真实非空空间块数。
            render_node_count,      // F4可直接证明逐树ECS已被有限批次节点替代。
            container: install.container, // 后续换版或离图只操作这一父根。
            hidden_entities,        // 离图时恢复第一次接管的正式根显隐。
        },
    );
    editor.status = {
        format!(
            "{runtime_profile}：已显示全部{count}个密度实例；{batch_count}个空间批次/{render_node_count}个ECS渲染节点"
        )
    }; // 作者数量与后端承接规模分开回显，不再把逐株实体数当作内容数量。
}
