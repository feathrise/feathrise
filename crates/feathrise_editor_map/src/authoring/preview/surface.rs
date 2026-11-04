//! 编辑器复用正式索引PBR地表；配对Mip后台生成、完整GPU候选原子采用。

use super::super::resources::{
    EditorAuthoringSession, SurfaceBrushStroke, SurfacePreviewCandidate, SurfacePreviewState, TerrainPreviewState,
}; // 完整作者与候选身份。
use crate::resources::MapEditorState;
use bevy::prelude::*; // 主世界只登记完整资产。
use bevy::render::storage::ShaderBuffer; // 正式地表局部层表。
use bevy::tasks::{AsyncComputeTaskPool, futures::check_ready}; // single-flight后台计算。
use feathrise_environment::terrain_surface::{
    TerrainSurfaceExtension, TerrainSurfaceFallbackImages, TerrainSurfaceIndexedDescriptor,
    TerrainSurfaceMaterial, load_terrain_surface_array, terrain_surface_image_failed,
}; // 复用运行时PBR路径。
use feathrise_map_content::{MapTerrainKind, SurfaceCraterResponse, SurfacePresetRegistry, TerrainId}; // 全局预设与对象画布权威快照。
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
}; // 渲染证据通过独立原子跨世界发布。
use std::time::{Duration, Instant}; // 资源失败或超时保留旧画面。 // 关闭后不得发布结果。

mod build; // 配对纹理及层表CPU编码。
mod render; // 独立候选GPU与管线就绪证据。
pub(crate) use render::install_surface_preview_render; // 插件注册只暴露领域入口。

/// 预热和辅助显示共用同一个完整对象来源；体素首块只供顶点布局预热。
pub(crate) struct SurfaceCanvasMesh {
    pub(crate) mesh: Handle<Mesh>,
    pub(crate) transform: Transform,
    pub(crate) kind: MapTerrainKind,
}

fn canvas_id(session: &EditorAuthoringSession) -> Option<Option<TerrainId>> {
    let draft = session.draft.as_ref()?;
    if draft.terrain_documents.loading
        || (draft.manifest.terrains.is_some() && draft.terrain_documents.active.is_none()) {
        return None; // 换槽等待不能借上一对象的作者或GPU身份。
    }
    Some(draft.terrain_documents.active.as_ref().map(|active| active.definition.id.clone()))
}

/// 旧单地形认领primary只改变寻址；它与任意其他显式对象仍严格隔离。
fn same_canvas(current: Option<&TerrainId>, applied: Option<&TerrainId>) -> bool {
    current == applied || match (current, applied) {
        (Some(id), None) | (None, Some(id)) => id == &TerrainId::legacy(),
        _ => false,
    }
}

/// 反馈回执只允许旧根尚未转成显式对象清单时认领primary；显式画布始终精确匹配。
pub(crate) fn same_surface_canvas(
    session: &EditorAuthoringSession,
    current: Option<&TerrainId>,
    applied: Option<&TerrainId>,
) -> bool {
    current == applied
        || (session.draft.as_ref().is_some_and(|draft| draft.manifest.terrains.is_none())
            && same_canvas(current, applied))
}

#[cfg(test)]
mod tests;

/// 只浅克隆当前作者几何；Clipmap辅助色必须另用正式高度扩展，不能把此平面当成实际山坡。
pub(crate) fn surface_canvas_mesh(
    session: &EditorAuthoringSession,
    terrain: &TerrainPreviewState,
    pose: Option<&feathrise_scene::TerrainObjectTransformPreview>,
    queries: &feathrise_scene::TerrainObjectQueries,
) -> Option<SurfaceCanvasMesh> {
    let draft = session.draft.as_ref()?;
    let id = canvas_id(session)?;
    let kind = draft.terrain_manifest().terrain.kind;
    if kind == MapTerrainKind::Heightfield
        && terrain.applied_map_id.as_ref() == Some(&draft.map_id)
        && terrain.applied_revision == session.terrain_revision
        && same_canvas(id.as_ref(), terrain.applied_identity.as_ref().and_then(|identity| identity.terrain_id.as_ref()))
        && let Some(mesh) = &terrain.mesh {
        return Some(SurfaceCanvasMesh {
            mesh: mesh.clone(),
            transform: crate::authoring::terrain_documents::canvas_transform(draft, pose),
            kind,
        });
    }
    let (mesh, matrix, actual_kind) = if let Some(id) = id.as_ref() {
        if draft.manifest.terrains.is_none() && id == &TerrainId::legacy() {
            queries.root_render_mesh(&draft.map_id)? // 认领先于新对象清单时，旧Clipmap仍由原正式根提供。
        } else { queries.render_mesh(&draft.map_id, id)? }
    } else { queries.root_render_mesh(&draft.map_id)? };
    (actual_kind == kind).then(|| SurfaceCanvasMesh {
        mesh,
        transform: Transform::from_matrix(matrix), // 运行时返回完整姿态，不再叠加一次对象缩放/平移。
        kind,
    })
}

/// 拖笔合并修订；稳定帧只比较固定身份，不扫描整张图片。
pub(crate) fn start_surface_preview_rebuild(
    mut editor: ResMut<MapEditorState>, // 预览失败需要在状态栏明确解释。
    session: Res<EditorAuthoringSession>, // 权威作者源。
    stroke: Res<SurfaceBrushStroke>,    // 松笔才提交完整Mip重建。
    presets: Option<Res<SurfacePresetRegistry>>, // 全局预设修订。
    mut preview: ResMut<SurfacePreviewState>, // 唯一CPU任务或GPU候选。
    active: Option<Res<feathrise_map_content::ActiveDynamicMap>>, // 关闭后仅继续当前游戏脏草稿。
) {
    let Some(draft) = session.draft.as_ref() else {
        preview.desired_map_id = None;
        preview.desired_terrain_id = None;
        return;
    }; // 无草稿无预览。
    if !editor.open
        && (!draft.surface_dirty
            || active.as_ref().and_then(|active| active.map_id.as_ref()) != Some(&draft.map_id))
    {
        preview.desired_map_id = None; // 关闭后的其他地图不继续后台构建。
        preview.desired_terrain_id = None;
        return; // 已有任务仍占单飞槽直至真实完成。
    }
    let Some(terrain_id) = canvas_id(&session) else { return; };
    let Some(presets) = presets.as_deref() else {
        return;
    }; // 目录等待期间保留旧图。
    preview.desired_map_id = Some(draft.map_id.clone()); // 拖笔也立即废弃旧候选身份。
    preview.desired_terrain_id = terrain_id.clone();
    preview.desired_revision = session.surface_revision; // 作者层和像素共同换代。
    preview.desired_preset_revision = presets.revision; // PBR参数与三数组共同换代。
    let identity = (
        draft.map_id.clone(),
        terrain_id.clone(),
        session.surface_revision,
        presets.revision,
    ); // 同修订失败不连续重试。
    if stroke.active || preview.task.is_some() || preview.candidate.is_some() // CPU与GPU共同限制唯一在途结果。
        || preview.failed_identity.as_ref() == Some(&identity) // 等待实际修改再尝试失败任务。
        || (preview.applied_map_id.as_ref() == Some(&draft.map_id) && preview.applied_terrain_id == terrain_id && preview.applied_revision == session.surface_revision && preview.applied_preset_revision == presets.revision)
    {
        return;
    } // 稳定帧不扫描层表。
    let (Some(surface), Some(indices), Some(weights)) = (
        &draft.surface,
        &draft.surface_indices,
        &draft.surface_weights,
    ) else {
        return;
    }; // 只接受完整作者源。
    let (table, crater_coefficients) = match build::layer_table(surface, presets) {
        // 冻结与运行时一致的PBR参数。
        Ok(table) => table, // 参数快照供后台编码。
        Err(error) => {
            warn!("地表预览保留上一版本：{error}");
            editor.status = format!("地表预览失败：{error}；已保留上一完整画面"); // 缺失预设不能只记日志。
            preview.failed_identity = Some(identity);
            return;
        } // 不能以白色猜测缺失预设。
    }; // 参数与数组切片具有同一修订。
    let layer_count = surface.layers.len() as u32; // 正式作者索引以u32表达。
    let resolution = surface.resolution; // 配对作者尺寸。
    let indices = Arc::clone(indices); // 启动帧不复制整图。
    let weights = Arc::clone(weights); // 完整不可变配对快照。
    let paths = presets.resolved.array_paths(); // 固定三条短资源路径。
    let paths = [paths.0.to_owned(), paths.1.to_owned(), paths.2.to_owned()]; // 与表切片对应同版数组。
    let world_min = Vec2::from_array(draft.terrain_manifest().world_min); // 纹素属于当前对象源XZ，地图容器包围盒不能代替画布范围。
    let world_max = Vec2::from_array(draft.terrain_manifest().world_max); // 对象姿态另由正式来源框架应用一次。
    let supports_craters = draft.terrain_manifest().terrain.kind == MapTerrainKind::SparseClipmap;
    preview.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        // 唯一CPU任务不阻塞主线程。
        (
            identity.0,
            identity.1,
            identity.2,
            identity.3,
            build::build_surface_preview(
                &indices,
                &weights,
                resolution,
                layer_count,
                table,
                paths,
                world_min,
                world_max,
            ).and_then(|mut image| {
                if supports_craters {
                    image.crater_response = Some(Arc::new(SurfaceCraterResponse::from_author_buffers(
                        world_min, world_max, resolution, indices, weights, crater_coefficients,
                    )?)); // 校验仍在原单飞后台内；CPU快照只持Arc，不另复制作者像素。
                }
                Ok(image)
            }),
        ) // 整套Mip和表完成再返回。
    })); // 后续帧非阻塞轮询。
}

/// GPU与管线确认之前保留旧材质，完整成功后同步发布句柄与作者身份。
pub(crate) fn poll_surface_preview_rebuild(
    mut editor: ResMut<MapEditorState>, // Tasks阶段检查关闭并发布可见错误。
    session: Res<EditorAuthoringSession>, // 不能仅使用上帧desired修订。
    presets: Option<Res<SurfacePresetRegistry>>, // 检查热更新后的真实目录版本。
    asset_server: Res<AssetServer>,     // 正式图片异步加载设置。
    fallback: Res<TerrainSurfaceFallbackImages>, // 空表合法绑定。
    mut images: ResMut<Assets<Image>>,  // 登记完整配对Image。
    mut buffers: ResMut<Assets<ShaderBuffer>>, // 登记后台编码表。
    mut materials: ResMut<Assets<TerrainSurfaceMaterial>>, // 正式PBR资产。
    mut preview: ResMut<SurfacePreviewState>, // 唯一候选状态。
    active: Option<Res<feathrise_map_content::ActiveDynamicMap>>, // 游戏只采用当前地图末笔。
) {
    let current = |map_id: &feathrise_map_content::MapId, terrain_id: &Option<TerrainId>, revision: u64, preset_revision: u64| {
        // 固定时间核对权威输入。
        session.draft.as_ref().is_some_and(|draft| {
            &draft.map_id == map_id
                && (editor.open
                    || (draft.surface_dirty
                        && active.as_ref().and_then(|active| active.map_id.as_ref())
                            == Some(map_id)))
        }) && canvas_id(&session).as_ref() == Some(terrain_id)
            && session.surface_revision == revision
            && presets
                .as_ref()
                .is_some_and(|presets| presets.revision == preset_revision) // 拒绝跨地图和跨目录结果。
    }; // 所有采用共用精确谓词。
    if let Some(candidate) = preview.candidate.as_ref() {
        // GPU阶段继续占用唯一任务槽。
        let stale = !current(
            &candidate.map_id,
            &candidate.terrain_id,
            candidate.revision,
            candidate.preset_revision,
        ); // 笔刷改动后旧候选失效。
        let failed = candidate.failed.load(Ordering::Acquire)
            || candidate
                .images
                .iter()
                .any(|image| terrain_surface_image_failed(&asset_server, image))
            || candidate.started.elapsed() > Duration::from_secs(30); // 错误有明确终点。
        if stale || failed {
            // 只回收从未采用的候选。
            let candidate = preview.candidate.take().expect("已检查候选"); // 撤销下一帧GPU输入。
            if failed && !stale {
                preview.failed_identity = Some((
                    candidate.map_id,
                    candidate.terrain_id,
                    candidate.revision,
                    candidate.preset_revision,
                ));
                warn!("地表预览准备失败，保留上一完整画面");
                editor.status =
                    "地表预览纹理或管线准备失败/超时，已保留上一完整画面；修改地表后会重试"
                        .to_owned(); // 失败必须对用户可见。
            } // 旧结果不会被清空。
            materials.remove(candidate.material.id()); // 删除未发布材质。
            buffers.remove(candidate.layer_table.id()); // 删除未发布表。
            for image in candidate.images.iter().take(2) {
                images.remove(image.id());
            } // 仅回收局部图，不触碰共享数组。
        } else if candidate.ready.load(Ordering::Acquire) {
            // 完整GPU与管线证据才允许发布。
            let candidate = preview.candidate.take().expect("已检查候选"); // 同时交接所有资源。
            preview.image = candidate.images.get(1).cloned(); // 保留权重诊断句柄，绝不生成纯色彩图。
            preview.material = Some(candidate.material); // 旧资产由强引用生命周期安全释放。
            preview.crater_response = candidate.crater_response; // 同版CPU与GPU仅在完整候选就绪时一起可见。
            preview.applied_map_id = Some(candidate.map_id); // 完整候选与地图身份同时可见。
            preview.applied_terrain_id = candidate.terrain_id;
            preview.applied_revision = candidate.revision; // 像素与局部层表同版。
            preview.applied_preset_revision = candidate.preset_revision; // 全局PBR三数组同版。
            preview.failed_identity = None; // 成功清除旧失败。
        } // 未就绪不修改活动结果。
        return; // 每帧最多推进一个完整事务阶段。
    } // 完成GPU等待处理。
    let Some(task) = preview.task.as_mut() else {
        return;
    }; // 无工作不做分配。
    let Some((map_id, terrain_id, revision, preset_revision, result)) = check_ready(task) else {
        return;
    }; // 非阻塞单次轮询。
    preview.task = None; // CPU确实结束才释放槽。
    if !current(&map_id, &terrain_id, revision, preset_revision) {
        return;
    } // 迟到结果没有资产副作用。
    let result = match result {
        Ok(result) => result,
        Err(error) => {
            warn!("地表预览保留上一版本：{error}");
            preview.failed_identity = Some((map_id, terrain_id, revision, preset_revision));
            editor.status = format!("地表预览失败：{error}；已保留上一完整画面"); // 编码错误同样明确显示。
            return;
        }
    }; // 编码失败保持旧材质。
    let index = images.add(result.indices); // 全Mip整数图。
    let weight = images.add(result.weights); // 同版全Mip权重。
    let arrays = if result.layer_count == 0 {
        [
            fallback.white_base_color_array.clone(),
            fallback.neutral_normal_array.clone(),
            fallback.neutral_arm_array.clone(),
        ]
    } else {
        // 空作者不依赖全局目录。
        std::array::from_fn(|slot| {
            load_terrain_surface_array(
                &asset_server,
                result.array_paths[slot].clone(),
                slot == 0,
                "editor_surface_pbr_array",
            )
        }) // 复用正式颜色空间及Repeat采样。
    }; // 完成三数组绑定。
    let table = buffers.add(result.layer_table); // 任意长度表编码已经在后台完成。
    let material = materials.add(TerrainSurfaceMaterial {
        // 唯一正式PBR扩展资产。
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 1.0,
            metallic: 0.0,
            ..default()
        }, // 真实材质参数由扩展提供。
        extension: TerrainSurfaceExtension::new_indexed(TerrainSurfaceIndexedDescriptor {
            // 不复制Shader。
            world_min: result.world_min,
            world_max: result.world_max, // 世界米制UV。
            index_map: index.clone(),
            weight_map: weight.clone(), // 同一Mip配对。
            base_color_array: arrays[0].clone(),
            normal_array: arrays[1].clone(),
            arm_height_array: arrays[2].clone(), // 全局纹理三件套。
            layer_table: table.clone(),
            layer_count: result.layer_count,
            zero_weight: fallback.zero_weight.clone(), // 全部绑定完整。
        }), // 复用前向与Deferred正式地表Shader。
    }); // 当前视口尚不切换。
    preview.candidate = Some(SurfacePreviewCandidate {
        crater_response: result.crater_response,
        // 保持唯一GPU候选。
        map_id,
        terrain_id,
        revision,
        preset_revision,
        material, // 精确身份与候选材质。
        images: vec![
            index,
            weight,
            arrays[0].clone(),
            arrays[1].clone(),
            arrays[2].clone(),
            fallback.zero_weight.clone(),
        ], // 固定六张GPU图。
        layer_table: table,
        ready: Arc::new(AtomicBool::new(false)),
        failed: Arc::new(AtomicBool::new(false)),
        started: Instant::now(), // 新候选绝不继承旧证据。
    }); // RenderApp完成预热后再原子采用。
}
