//! 编辑器地表独立GPU候选探针：完整纹理、存储表、材质与真实管线就绪后才采用。

use crate::authoring::resources::EditorAuthoringSession; // 关闭窗口后草稿仍是作者权威。
use crate::authoring::resources::{SurfacePreviewState, TerrainPreviewState}; // 唯一作者候选与同拓扑网格。
use crate::resources::MapEditorState;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers}; // 离屏微缩网格仍参与材质专用化。
use bevy::core_pipeline::core_3d::Opaque3d; // 正式地表不透明绘制阶段。
use bevy::core_pipeline::deferred::Opaque3dDeferred; // 光追期间Auto材质通过Deferred预通道排队。
use bevy::light::NotShadowCaster; // 离屏候选不参与游戏阴影。
use bevy::pbr::{
    PreparedMaterial, RenderMaterialInstances, SpecializedMaterialPipelineCache,
    SpecializedPrepassMaterialPipelineCache, queue_material_meshes, queue_prepass_material_meshes,
}; // 复用正式材质后端证据。
use bevy::prelude::*; // 固定候选实体与系统注册。
use bevy::render::erased_render_asset::ErasedRenderAssets; // 已准备的类型擦除材质。
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin}; // 官方按实体提取短候选身份。
use bevy::render::render_asset::RenderAssets; // GPU图片和存储表的实际上传状态。
use bevy::render::render_phase::ViewBinnedRenderPhases; // 排队证据不能用CPU资产存在替代。
use bevy::render::render_resource::{CachedPipelineState, CachedRenderPipelineId, PipelineCache}; // 异步管线等待与失败。
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer}; // 局部层表正式GPU类型。
use bevy::render::sync_world::MainEntity; // 精确对应候选实体材质实例。
use bevy::render::texture::GpuImage; // 六个固定图片绑定的GPU状态。
use bevy::render::{Render, RenderApp, RenderSystems}; // 主世界与RenderApp职责分离。
use feathrise_environment::terrain_surface::TerrainSurfaceMaterial; // 与游戏内相同的PBR地表材质。
use feathrise_map_content::ActiveDynamicMap; // 覆盖只安装到游戏正在运行的地图。
use feathrise_scene_core::residency::MapInstanceOwner; // 旧单地形查询不能误纳入独立对象或子地图。
use feathrise_scene::DynamicSurfaceAuthorPreview; // 正式运行时唯一内存输入。
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
}; // 每个候选独有无锁回执，旧渲染帧不能确认新候选。 // 关闭后立即释放预热实体。

#[derive(Component, Clone, ExtractComponent)] // 不进入正式地图的全局地表计数器。
#[extract_app(bevy::render::RenderApp)]
struct SurfacePreviewProbe {
    material: Handle<TerrainSurfaceMaterial>, // 只允许这一个候选材质被确认。
    images: Vec<Handle<Image>>,               // 固定六个绑定浅克隆，不复制像素。
    table: Handle<ShaderBuffer>,              // 与材质同一局部层表。
    ready: Arc<AtomicBool>,                   // 只在完整GPU证据连续成立时发布。
    failed: Arc<AtomicBool>,                  // 明确管线失败交给主世界显示诊断。
    frames: Arc<AtomicU8>,                    // 至少观察两个独立渲染帧。
}

/// 候选材质必须使用最终会承载它的顶点布局；当前玩法地图直接复用正式网格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SurfaceProbeMeshSource {
    Runtime,
    Editor,
}

fn surface_probe_mesh_source(
    candidate_map: &feathrise_map_content::MapId,
    active_map: Option<&feathrise_map_content::MapId>,
    editor_terrain_map: Option<&feathrise_map_content::MapId>,
) -> Option<SurfaceProbeMeshSource> {
    if active_map == Some(candidate_map) {
        Some(SurfaceProbeMeshSource::Runtime) // SparseClipmap 没有旧 TerrainPreviewState，正式网格才是唯一拓扑。
    } else if editor_terrain_map == Some(candidate_map) {
        Some(SurfaceProbeMeshSource::Editor)
    } else {
        None // 不允许借用另一张地图的网格伪造管线就绪。
    }
}

pub(crate) fn install_surface_preview_render(app: &mut App) {
    app.add_plugins(ExtractComponentPlugin::<SurfacePreviewProbe>::default()); // 自动管理实体提取与销毁。
    app.add_systems(
        Update,
        (sync_author_preview, sync_probe)
            .chain()
            .after(super::poll_surface_preview_rebuild)
            .in_set(crate::MapEditorSystemSet::Tasks),
    ); // 完成候选登记后创建固定预热实体。
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        // 无GPU的逻辑测试不虚构就绪。
        render.add_systems(
            Render,
            observe_probe
                .in_set(RenderSystems::QueueMeshes)
                .after(queue_material_meshes)
                .after(queue_prepass_material_meshes), // 同时等待Forward与Deferred实际排队。
        ); // 等正式排队结束再观察。
    } // 无额外相机或渲染视图。
}

fn sync_probe(
    mut commands: Commands,                // 主线程只操作固定数量实体。
    preview: Res<SurfacePreviewState>,     // 当前不可变候选身份。
    terrain: Res<TerrainPreviewState>,     // 正式拓扑Mesh的浅句柄。
    session: Res<EditorAuthoringSession>,
    pose: Option<Res<feathrise_scene::TerrainObjectTransformPreview>>,
    queries: feathrise_scene::TerrainObjectQueries,
    active: Option<Res<ActiveDynamicMap>>, // 只有同一游戏地图才复用正式网格布局。
    runtime_mesh: Query<&Mesh3d, (With<feathrise_scene::DynamicMapTerrainPrimitive>, Without<MapInstanceOwner>)>, // 只有旧主地形使用唯一源后备。
    probes: Query<(Entity, &SurfacePreviewProbe, &Mesh3d)>, // 正常最多一个候选实体。
) {
    let candidate = preview.candidate.as_ref().filter(|candidate| {
        super::canvas_id(&session).as_ref() == Some(&candidate.terrain_id)
            && session.draft.as_ref().is_some_and(|draft| draft.map_id == candidate.map_id)
    }); // 切同图画布后旧探针立即退出，不能把新对象几何预热给旧候选。
    let mesh_source = candidate.and_then(|candidate| {
        surface_probe_mesh_source(
            &candidate.map_id,
            active.as_ref().and_then(|active| active.map_id.as_ref()),
            terrain.applied_map_id.as_ref(),
        )
    });
    let candidate = candidate.filter(|_| mesh_source.is_some()); // 没有同图拓扑时继续等待，不跨地图预热。
    let object_mesh = candidate.and_then(|_| {
        super::surface_canvas_mesh(&session, &terrain, pose.as_deref(), &queries)
    });
    let target_mesh = if object_mesh.is_some() || candidate.is_some_and(|candidate| candidate.terrain_id.is_some()) {
        object_mesh.as_ref().map(|source| &source.mesh) // 精确对象句柄，不以世界唯一性或同MapId猜测。
    } else { match mesh_source {
        Some(SurfaceProbeMeshSource::Runtime) => {
            runtime_mesh.single().ok().map(|mesh| &mesh.0) // Clipmap 正式固定网格无需旧 GLB 预览身份。
        }
        Some(SurfaceProbeMeshSource::Editor) => terrain.mesh.as_ref(),
        None => None,
    } };
    let mut exists = false; // 每个候选仅生成一次实体。
    for (entity, probe, mesh) in &probes {
        // 候选数量固定，不随地图实体规模增长。
        let current = candidate.is_some_and(|candidate| {
            candidate.material == probe.material && Arc::ptr_eq(&candidate.ready, &probe.ready)
        }) && target_mesh == Some(&mesh.0); // 完整票据与拓扑句柄保持对应。
        if current {
            exists = true;
        } else {
            commands.entity(entity).try_despawn();
        } // 旧候选不能占用渲染探针。
    }
    if exists {
        return;
    } // 稳定帧不重复创建或写入组件。
    let (Some(candidate), Some(mesh)) = (candidate, target_mesh) else {
        return;
    }; // 等待真实地形拓扑就绪。
    commands.spawn((
        Name::new("编辑器地表PBR离屏预热"),         // 便于实体诊断识别。
        feathrise_environment::terrain_surface::TerrainSurfaceWarmup, // 与正式地表预热共用地图内容排除身份。
        Mesh3d(mesh.clone()),                       // 与最终预览完全相同的顶点布局。
        MeshMaterial3d(candidate.material.clone()), // 与最终预览完全相同的材质。
        Transform::from_xyz(0.0, -100_000.0, 0.0).with_scale(Vec3::splat(0.000_001)), // 沿用正式地表离屏微缩预热，不覆盖用户画面。
        Visibility::Visible, // Hidden不会进入管线专用化，不能用来预热。
        NoFrustumCulling,    // 离屏网格仍真实排队。
        NotShadowCaster,     // 微缩候选不污染阴影图。
        RenderLayers::from_layers(&[0, crate::viewport::MAP_EDITOR_BRUSH_RENDER_LAYER]), // 主游戏和F12共用预热，关闭窗口仍可完成末笔。
        SurfacePreviewProbe {
            material: candidate.material.clone(),  // 冻结材质身份。
            images: candidate.images.clone(),      // 固定纹理组。
            table: candidate.layer_table.clone(),  // 冻结配对存储表。
            ready: Arc::clone(&candidate.ready),   // 仅当前候选消费该完成位。
            failed: Arc::clone(&candidate.failed), // 管线错误不无限等待。
            frames: Arc::new(AtomicU8::new(0)),    // 新候选从零个有效渲染帧开始。
        }, // 不插入TerrainSurfaceCarrier，避免影响游戏地表全局统计。
    )); // Bevy自动提取且随生命周期回收。
}

/// 已GPU就绪的完整地表交给正式源；稳定帧不分配、不重新创建材质。
fn sync_author_preview(
    editor: Res<MapEditorState>, // 选择变化可撤销旧作者身份，关闭不会撤销。
    session: Res<EditorAuthoringSession>, // dirty和修订来自当前草稿。
    preview: Res<SurfacePreviewState>, // 只发布已完整准备的句柄。
    active: Option<Res<ActiveDynamicMap>>, // 专用地图不走动态地表覆盖。
    author: Option<ResMut<DynamicSurfaceAuthorPreview>>, // 无scene插件测试保持可运行。
) {
    let Some(mut author) = author else {
        return;
    }; // 正式scene持有资源。
    let draft = session.draft.as_ref().filter(|draft| {
        active.as_ref().and_then(|active| active.map_id.as_ref()) == Some(&draft.map_id)
            && (!editor.open || editor.selected_map_id.as_ref() == Some(&draft.map_id))
    }); // 必须是当前游戏地图。
    let Some(draft) = draft else {
        author.clear();
        return;
    }; // 切图或草稿丢弃立即撤销。
    if draft.terrain_documents.loading {
        return; // 换槽完成前保留原对象完整材质；不发布或清除过渡中的另一对象输入。
    }
    if !draft.surface_dirty {
        if !author.is_saving() || author.saved_revision != session.surface_revision {
            author.clear();
        } // 保存等待保持，重载和撤销干净归还文件。
        return; // 文件成功消费后绝不把旧作者句柄再塞回来。
    }
    let canvas = super::canvas_id(&session);
    if preview.applied_map_id.as_ref() == Some(&draft.map_id)
        && canvas.as_ref().is_some_and(|canvas| super::same_canvas(canvas.as_ref(), preview.applied_terrain_id.as_ref()))
        && let Some(material) = preview.material.as_ref()
    {
        author.publish_for_terrain_with_crater_response(
            draft.map_id.clone(),
            canvas.flatten(), // 首次认领后按实际primary发布，不继续扩大旧None语义。
            preview.applied_revision,
            material.clone(),
            preview.crater_response.clone(), // 与当前完整画面共同保留，关闭F12后的技能不能被作者覆盖永久堵住。
        ); // 新笔等待时继续同图上一完整版本。
    }
}

#[allow(clippy::too_many_arguments)] // 各类GPU证据保持只读且来自同一RenderApp帧。
fn observe_probe(
    probes: Query<(&MainEntity, &SurfacePreviewProbe)>, // 仅检查编辑器候选。
    images: Res<RenderAssets<GpuImage>>,                // 图片真实上传。
    tables: Res<RenderAssets<GpuShaderBuffer>>,         // 局部层表真实上传。
    materials: Res<ErasedRenderAssets<PreparedMaterial>>, // 完整材质bind group就绪。
    instances: Res<RenderMaterialInstances>,            // 精确实体与材质身份。
    pipelines: Res<SpecializedMaterialPipelineCache>,   // 真实视口专用化结果。
    cache: Res<PipelineCache>,                          // 编译成功而非仅请求成功。
    phases: Res<ViewBinnedRenderPhases<Opaque3d>>,      // 当前帧实际排队结果。
    prepass_pipelines: Res<SpecializedPrepassMaterialPipelineCache>, // Deferred管线拥有独立专用化缓存。
    deferred_phases: Res<ViewBinnedRenderPhases<Opaque3dDeferred>>, // 关闭F12后仍能核对游戏Deferred结果。
) {
    for (entity, probe) in &probes {
        // 单飞候选至多一项。
        let bound = probe.images.iter().all(|image| images.get(image).is_some())
            && tables.get(&probe.table).is_some()
            && materials.get(probe.material.id().untyped()).is_some()
            && instances
                .instances
                .get(entity)
                .is_some_and(|instance| instance.asset_id == probe.material.id().untyped()); // 六图、表和实例必须全部匹配。
        let mut found = false; // 没有专用化不算就绪。
        let mut complete = bound; // 任一依赖未就绪继续等待。
        let mut queued = false; // 必须有真实不透明排队证据。
        for view in pipelines.values() {
            // 视图数固定，按候选实体精确查找。
            if let Some(id) = view.get(entity) {
                // 不使用其他材质的管线充当候选成功。
                found = true; // 已形成实际管线请求。
                match cache.get_render_pipeline_state(*id) {
                    // 非阻塞查询异步编译状态。
                    CachedPipelineState::Ok(_) => {} // 管线可用。
                    CachedPipelineState::Err(_) => {
                        complete = false;
                        probe.failed.store(true, Ordering::Release);
                    } // 明确错误只属于该候选。
                    _ => complete = false,           // 编译和排队中继续保留旧画面。
                }
                queued |= pipeline_queued(&phases, *id); // 对应管线须进入实际绘制阶段。
            }
        }
        for view in prepass_pipelines.values() {
            // 只检查当前候选的预通道身份，不扫描场景网格。
            if let Some((_, id, _)) = view.get(entity) {
                // 与Forward一样要求准确候选实体。
                match cache.get_render_pipeline_state(*id) {
                    // 不阻塞异步管线编译。
                    CachedPipelineState::Ok(_) => {} // 当前预通道可用。
                    CachedPipelineState::Err(_) => {
                        // 明确失败归属当前候选。
                        complete = false; // 不发布不完整材质。
                        probe.failed.store(true, Ordering::Release); // 主世界保留旧画面并显示失败。
                    }
                    _ => complete = false, // 编译期间继续等待。
                }
                if deferred_pipeline_queued(&deferred_phases, *id) {
                    // 普通深度预通道不能冒充Deferred最终表面。
                    found = true; // 已找到正式Deferred绘制路径。
                    queued = true; // 候选管线确实进入Deferred分箱。
                }
            }
        }
        if complete && found && queued {
            // 全套证据同时成立才累计帧。
            let observed = probe.frames.fetch_add(1, Ordering::AcqRel); // 两帧计数不会随主线程快慢改变。
            if observed >= 1 {
                probe.ready.store(true, Ordering::Release);
            } // 主线程下一次轮询可原子采用。
        } else {
            probe.frames.store(0, Ordering::Release);
        } // 依赖中断后重新要求连续完整帧。
    }
}

fn pipeline_queued(phases: &ViewBinnedRenderPhases<Opaque3d>, id: CachedRenderPipelineId) -> bool {
    phases.values().any(|phase| {
        phase
            .multidrawable_meshes
            .keys()
            .any(|key| key.pipeline == id)
            || phase
                .batchable_meshes
                .keys()
                .any(|(key, _)| key.pipeline == id)
            || phase
                .unbatchable_meshes
                .keys()
                .any(|(key, _)| key.pipeline == id)
            || phase
                .non_mesh_items
                .keys()
                .any(|(key, _)| key.pipeline == id)
    }) // 与正式地表相同的公开分箱检查。
}

fn deferred_pipeline_queued(
    // Deferred与Forward分箱使用不同公开键类型，分别核对。
    phases: &ViewBinnedRenderPhases<Opaque3dDeferred>, // 当前游戏视口的实际Deferred分箱。
    id: CachedRenderPipelineId,                        // 精确候选管线身份。
) -> bool {
    phases.values().any(|phase| {
        // 视口数量有限，不查询地图内容。
        phase.multidrawable_meshes.keys().any(|key| key.pipeline == id) // GPU多绘制路径。
            || phase.batchable_meshes.keys().any(|(key, _)| key.pipeline == id) // 普通可批处理路径。
            || phase.unbatchable_meshes.keys().any(|(key, _)| key.pipeline == id) // 独立网格路径。
            || phase.non_mesh_items.keys().any(|(key, _)| key.pipeline == id) // 非标准绘制路径。
    }) // 至少一个真实视口已完成候选排队。
}

#[cfg(test)]
mod author_preview_tests {
    use super::*; // 运行实际发布系统，不虚构渲染结果。
    use crate::authoring::terrain::tests::test_heightfield_draft; // 与真实作者类型一致的草稿。

    #[test]
    fn active_clipmap_surface_probe_uses_runtime_mesh_without_legacy_preview() {
        let map_id = feathrise_map_content::MapId::parse("MAP-clipmap-probe").unwrap();
        assert_eq!(
            surface_probe_mesh_source(&map_id, Some(&map_id), None),
            Some(SurfaceProbeMeshSource::Runtime)
        ); // SparseClipmap 没有 TerrainPreviewState，不能因此等待三十秒后误报超时。
        assert_eq!(
            surface_probe_mesh_source(&map_id, None, Some(&map_id)),
            Some(SurfaceProbeMeshSource::Editor)
        ); // 浏览非当前旧图时仍只使用同图作者网格。
    }

    #[test]
    fn surface_author_survives_editor_close_and_saved_receipt_is_not_republished() {
        let mut app = App::new(); // 只测试主世界发布边界。
        let mut draft = test_heightfield_draft(Arc::new(vec![1000; 121])); // 有效动态地形。
        let map_id = draft.map_id.clone(); // 同一运行地图。
        draft.surface_dirty = true; // 未保存笔划。
        app.init_resource::<MapEditorState>()
            .init_resource::<EditorAuthoringSession>()
            .init_resource::<SurfacePreviewState>()
            .init_resource::<DynamicSurfaceAuthorPreview>()
            .insert_resource(ActiveDynamicMap {
                map_id: Some(map_id.clone()),
            })
            .add_systems(Update, sync_author_preview); // 默认关闭编辑器，验证游戏仍采用。
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .draft = Some(draft); // 草稿持续存在。
        {
            let mut preview = app.world_mut().resource_mut::<SurfacePreviewState>(); // 模拟已GPU就绪结果。
            preview.applied_map_id = Some(map_id.clone()); // 精确地图。
            preview.material = Some(Handle::default()); // 此测试不启动GPU。
            preview.crater_response = Some(Arc::new(feathrise_map_content::SurfaceCraterResponse::from_author_buffers(
                Vec2::splat(-16.0), Vec2::splat(16.0), [1, 1], Arc::new(vec![0; 16]),
                Arc::new(vec![255, 0, 0, 0]), vec![0.1]).unwrap()));
        }
        app.update(); // 关闭F12也发布正式源。
        assert!(
            app.world()
                .resource::<DynamicSurfaceAuthorPreview>()
                .material
                .is_some()
        ); // 未保存游戏预览仍在。
        assert!((app.world().resource::<DynamicSurfaceAuthorPreview>().crater_response.as_ref().unwrap().sample(Vec2::ZERO) - 0.1).abs() < 0.001,
            "关闭F12必须保留可见材质同版的岩石响应，不得把技能永远挂在作者门外");
        app.world_mut()
            .resource_mut::<DynamicSurfaceAuthorPreview>()
            .begin_saved_handoff(&map_id, 0, true, true, None); // 此边界测试直接模拟正式采用，不启动文件源加载。
        app.world_mut()
            .resource_mut::<EditorAuthoringSession>()
            .draft
            .as_mut()
            .unwrap()
            .surface_dirty = false; // 同次保存清脏。
        app.update(); // 文件尚未GPU就绪。
        assert!(
            app.world()
                .resource::<DynamicSurfaceAuthorPreview>()
                .material
                .is_some()
        ); // 不允许清脏瞬间闪回旧文件。
        assert!(app.world().resource::<DynamicSurfaceAuthorPreview>().crater_response.is_some());
        app.world_mut()
            .resource_mut::<DynamicSurfaceAuthorPreview>()
            .clear(); // 模拟正式新文件已完整采用并消费。
        app.update(); // 关闭F12的干净草稿仍保留preview句柄。
        assert!(
            app.world()
                .resource::<DynamicSurfaceAuthorPreview>()
                .material
                .is_none()
        ); // 旧句柄不得重新发布造成交接循环。
        assert!(app.world().resource::<DynamicSurfaceAuthorPreview>().crater_response.is_none());
        app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().surface_dirty = true;
        app.world_mut().resource_mut::<ActiveDynamicMap>().select(feathrise_map_content::MapId::parse("MAP-other-surface").unwrap());
        app.update();
        assert!(app.world().resource::<DynamicSurfaceAuthorPreview>().crater_response.is_none(), "切图不得复用上一图CPU响应");
    }

    #[test]
    fn same_map_canvas_switch_rejects_previous_surface_but_keeps_same_canvas_older_revision() {
        let mut app = App::new();
        let draft = super::super::tests::draft();
        let map = draft.map_id.clone();
        app.init_resource::<MapEditorState>()
            .init_resource::<SurfacePreviewState>()
            .init_resource::<DynamicSurfaceAuthorPreview>()
            .insert_resource(ActiveDynamicMap { map_id: Some(map.clone()) })
            .insert_resource(EditorAuthoringSession { draft: Some(draft), surface_revision: 4, ..default() })
            .add_systems(Update, sync_author_preview);
        {
            let mut state = app.world_mut().resource_mut::<SurfacePreviewState>();
            state.applied_map_id = Some(map.clone());
            state.applied_terrain_id = Some(feathrise_map_content::TerrainId::parse("paint-b").unwrap());
            state.applied_revision = 3;
            state.material = Some(Handle::default());
        }
        app.update();
        assert!(app.world().resource::<DynamicSurfaceAuthorPreview>().material.is_none(),
            "相同MapId与递增作者修订不能授权另一对象旧PBR");
        app.world_mut().resource_mut::<SurfacePreviewState>().applied_terrain_id =
            Some(feathrise_map_content::TerrainId::parse("paint-a").unwrap());
        app.update();
        let author = app.world().resource::<DynamicSurfaceAuthorPreview>();
        assert!(author.material.is_some(), "同一画布下一筆等待时应保留上一完整材质");
        assert_eq!(author.revision, 3);
        assert_eq!(author.terrain_id.as_ref().unwrap().as_str(), "paint-a");
    }

    #[test]
    fn legacy_probe_ignores_owned_meshes_from_objects_and_submaps() {
        let mut app = App::new();
        let draft = test_heightfield_draft(Arc::new(vec![0; 121]));
        let map = draft.map_id.clone();
        app.init_resource::<SurfacePreviewState>()
            .init_resource::<TerrainPreviewState>()
            .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
            .insert_resource(ActiveDynamicMap { map_id: Some(map.clone()) })
            .add_systems(Update, sync_probe);
        let owner = app.world_mut().spawn_empty().id();
        let mut meshes = Assets::<Mesh>::default();
        let source = meshes.add(Rectangle::new(2.0, 2.0));
        let unrelated = meshes.add(Rectangle::new(3.0, 3.0));
        app.world_mut().spawn((feathrise_scene::DynamicMapTerrainPrimitive, Mesh3d(source.clone())));
        app.world_mut().spawn((feathrise_scene::DynamicMapTerrainPrimitive, Mesh3d(unrelated), MapInstanceOwner(owner)));
        app.world_mut().resource_mut::<SurfacePreviewState>().candidate = Some(
            crate::authoring::resources::SurfacePreviewCandidate {
                crater_response: None,
                map_id: map, terrain_id: None, revision: 0, preset_revision: 0,
                material: Handle::default(), images: Vec::new(), layer_table: Handle::default(),
                ready: Arc::new(AtomicBool::new(false)), failed: Arc::new(AtomicBool::new(false)),
                started: std::time::Instant::now(),
            });
        app.update();
        let mut query = app.world_mut().query_filtered::<&Mesh3d, With<SurfacePreviewProbe>>();
        assert_eq!(query.single(app.world()).unwrap().0, source,
            "另一个所有权域的正式primitive不能阻止当前地表GPU预热");
    }

    #[test]
    fn claiming_legacy_canvas_keeps_previous_pbr_and_publishes_explicit_primary_identity() {
        let mut app = App::new();
        let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
        draft.surface_dirty = true;
        let map = draft.map_id.clone();
        app.init_resource::<MapEditorState>()
            .init_resource::<SurfacePreviewState>()
            .init_resource::<DynamicSurfaceAuthorPreview>()
            .insert_resource(ActiveDynamicMap { map_id: Some(map.clone()) })
            .insert_resource(EditorAuthoringSession { draft: Some(draft), surface_revision: 3, ..default() })
            .add_systems(Update, sync_author_preview);
        {
            let mut preview = app.world_mut().resource_mut::<SurfacePreviewState>();
            preview.applied_map_id = Some(map);
            preview.applied_revision = 3;
            preview.material = Some(Handle::default());
        }
        app.update();
        let before = app.world().resource::<DynamicSurfaceAuthorPreview>().material.clone();
        assert!(before.is_some());
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            crate::authoring::terrain_documents::ensure_primary_canvas(draft); // 走正式迁移认领，保留原作者槽。
            draft.manifest.terrains = Some(draft.manifest.terrain_objects_for_edit());
            draft.manifest.terrain = feathrise_map_content::MapTerrainDescriptor::empty();
            session.surface_revision += 1; // 新候选尚未完成，不伪造旧预览作者代次。
        }
        app.update();
        let author = app.world().resource::<DynamicSurfaceAuthorPreview>();
        assert_eq!(author.material, before, "认领primary不能闪回保存前的旧文件材质");
        assert_eq!(author.terrain_id, Some(feathrise_map_content::TerrainId::legacy()));
        assert_eq!(author.revision, 3);
        // 再切到任何非primary对象，旧None不能授权把同一材质发布到该画布。
        {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let active = session.draft.as_mut().unwrap().terrain_documents.active.as_mut().unwrap();
            Arc::make_mut(&mut active.definition).id = feathrise_map_content::TerrainId::parse("unrelated").unwrap();
            session.surface_revision += 1;
        }
        app.update();
        assert_eq!(app.world().resource::<DynamicSurfaceAuthorPreview>().terrain_id,
            Some(feathrise_map_content::TerrainId::legacy()), "旧活动句柄不能获得另一对象的新身份");
    }
}
