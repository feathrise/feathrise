//! 场景草稿的有界后台加载与原子采用。

use std::collections::{BTreeMap, BTreeSet}; // 植被、水域密度和迁移脏层按稳定ID装入草稿。
use std::path::Path; // 作者源读取诊断使用绝对路径。
use std::sync::Arc; // 水域预览任务共享R8画布，不在每次启动时复制整图。

use bevy::prelude::*; // ECS资源与日志。
use bevy::tasks::futures::check_ready; // Update只探测完成值。
use bevy::tasks::{AsyncComputeTaskPool, IoTaskPool}; // 文件读取与GLB派生分别进入对应后台池。
use feathrise_scene_content::{
    SCENE_SCHEMA_VERSION, SCENE_SURFACE_SCHEMA_VERSION, SCENE_WATER_FILE_NAME, SceneBoundariesAsset,
    SceneCatalogAsset, SceneContentRegistry, SceneId, SceneManifestAsset, ScenePlacementsAsset,
    ScenePortalsAsset, SceneSurfaceAsset, SceneTerrainKind, SceneVegetationAsset, SceneWaterAsset,
}; // 已加载JSON在主线程完整克隆后交给IO任务。
use super::files::{
    safe_asset_path, safe_scene_private_asset_path, workspace_asset_root,
};
use super::image_io::read_height_vertices_png; // 旧GLTF场景高度单元PNG仍可进入兼容量化高度草稿。
use super::resources::{
    DraftLoadTask, EditorAuthoringSession, EditorCommand, EditorSceneDraft, SurfacePreviewState,
    VegetationPreviewState, WaterBrushStroke, WaterPreviewState,
};
use super::vegetation_defaults::ensure_vegetation_authoring; // 旧正式密度只迁移自己已有的输出，不补目录全集。
use crate::components::{EditorPlacedAsset, EditorPortalPreview, EditorVegetationPreview};
use crate::resources::SceneEditorState;

/// 作者草稿的空气墙分帧输入；文件事件与加载票据共同拒绝混合代次。
#[derive(Default)]
pub(crate) struct BoundaryDraftCapture {
    ticket: u64,                                  // 当前作者加载请求身份。
    source: Option<AssetId<SceneBoundariesAsset>>,  // 当前边界文件句柄身份。
    capture: feathrise_scene_content::SceneBoundaryCapture, // 点/路径双预算捕获器。
}

/// 地表拥有独立于场景通用文档的格式版本；F12必须接受运行时当前采用的同一版本。
fn validate_surface_authoring_identity(
    surface: &SceneSurfaceAsset, // 已由AssetServer完整反序列化的地表文档。
    scene_id: &SceneId,            // 当前明确进入并准备建立草稿的场景身份。
) -> Result<(), String> {
    if surface.scene_id != *scene_id {
        return Err(format!("文件所属场景为{}，期望{}", surface.scene_id, scene_id)); // 绝不把另一个场景的局部层和权重装入当前草稿。
    }
    if surface.schema_version != SCENE_SURFACE_SCHEMA_VERSION {
        return Err(format!(
            "格式版本为{}，当前要求{}",
            surface.schema_version, SCENE_SURFACE_SCHEMA_VERSION
        )); // 地表第二版不能被仍为第一版的场景通用版本常量误判。
    }
    Ok(()) // 身份与地表专用版本同时匹配后才允许进入后台作者源读取。
}

pub(super) fn read_exact_bytes(
    path: &Path,
    expected: usize,
    label: &str,
) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("读取{label}{}失败：{error}", path.display()))?;
    if bytes.len() != expected {
        return Err(format!(
            "{label}{}为{}字节，期望{}字节",
            path.display(),
            bytes.len(),
            expected
        ));
    }
    Ok(bytes)
}

/// 选择改变后等待Bevy清单资产完整就绪，再把GLB读取与高度派生交给有界后台任务。
#[allow(clippy::too_many_arguments)]
pub(crate) fn start_draft_load(
    mut editor: ResMut<SceneEditorState>,
    registry: Option<Res<SceneContentRegistry>>,
    catalogs: Res<Assets<SceneCatalogAsset>>,
    manifests: Res<Assets<SceneManifestAsset>>,
    placements: Res<Assets<ScenePlacementsAsset>>,
    portals: Res<Assets<ScenePortalsAsset>>,
    surfaces: Res<Assets<SceneSurfaceAsset>>,
    vegetations: Res<Assets<SceneVegetationAsset>>,
    waters: Res<Assets<SceneWaterAsset>>,
    boundaries_assets: Res<Assets<SceneBoundariesAsset>>,
    session: Res<EditorAuthoringSession>,
    mut load: ResMut<DraftLoadTask>,
    mut boundary_events: feathrise_scene_core::residency::ResidentAssetEventReader<SceneBoundariesAsset>, // 捕获期间热更新使旧输入失效。
    auxiliary: (Option<Res<crate::resources::SceneFolderPersistence>>, Option<Res<super::domain_documents::SceneDomainAdapter>>), // 重命名发布后才允许冻结清单名称。
    mut boundary_capture: Local<BoundaryDraftCapture>, // 单在途输入，不深拷贝整图。
    documents: Option<Res<crate::ui::scene_documents::SceneDocuments>>,
) {
    let (folder_persistence, domain_adapter) = auxiliary;
    if session.water_preset_delete_pending || session.model_vegetation_busy { return; } // 作者整笔结束后再改变文档来源。
    for event in boundary_events.read() {
        if boundary_capture.source.is_some_and(|id| matches!(event,
            AssetEvent::Modified { id: changed } | AssetEvent::Removed { id: changed } if *changed == id)) {
            load.ticket = load.ticket.wrapping_add(1); // 已启动旧后台结果也不能再采用。
            *boundary_capture = BoundaryDraftCapture::default(); // 下一帧从新完整资产重新捕获。
        }
    }
    if folder_persistence
        .as_deref()
        .is_some_and(|state| state.rename_task.is_some())
    {
        return;
    } // 保持原有效草稿直到名称事务收口。
    let target = editor.selected_scene_id.clone();
    if load.desired_scene_id != target {
        load.desired_scene_id = target.clone();
        load.ticket = load.ticket.wrapping_add(1);
    }
    let Some(scene_id) = target else {
        return;
    };
    if documents.as_ref().is_some_and(|docs| {
        docs.preparing_target.is_some()
            || editor.pending_scene_transition.is_some()
            || docs.parked.contains_key(&scene_id)
    }) {
        return; // 已打开标签从内存恢复，禁止重新读盘覆盖未保存草稿。
    }
    if session.draft.as_ref().map(|draft| &draft.scene_id) == Some(&scene_id) || load.task.is_some() {
        return;
    }
    let Some(registry) = registry.as_deref() else {
        return;
    };
    let Some(catalog) = catalogs.get(&registry.catalog) else {
        return;
    };
    let Some(entry) = catalog.scenes.iter().find(|entry| entry.id == scene_id) else {
        return;
    };
    let Some(handles) = registry.handles(&scene_id) else {
        return;
    };
    let (Some(manifest), Some(placements), Some(portals)) = (
        manifests.get(&handles.manifest),
        placements.get(&handles.placements),
        portals.get(&handles.portals),
    ) else {
        return;
    };
    if manifest.id != scene_id || placements.scene_id != scene_id || portals.scene_id != scene_id {
        return;
    }
    if !manifest.is_data_driven() {
        editor.status = format!("场景{scene_id}使用不支持的运行类型：{}", manifest.runtime_scene);
        return; // 即使上游手动注入资产，也不能绕过文件加载器的运行类型约束。
    }
    if let Some(handle) = handles.boundaries.as_ref() {
        let Some(source) = boundaries_assets.get(handle) else {
            return;
        }; // 等待正式JSON资源完整就绪。
        if boundary_capture.ticket != load.ticket || boundary_capture.source != Some(handle.id()) {
            *boundary_capture = BoundaryDraftCapture {
                ticket: load.ticket,
                source: Some(handle.id()),
                capture: default(),
            }; // 新请求不能继续旧点集。
        }
        if !boundary_capture.capture.step(source, 4096, 64) {
            editor.status = "正在准备空气墙作者数据…".into(); // 主线程继续响应输入，未完成输入不发布。
            return;
        }
    }
    let surface = match handles.surface.as_ref() {
        Some(handle) => {
            let Some(surface) = surfaces.get(handle) else {
                return;
            };
            if let Err(error) = validate_surface_authoring_identity(surface, &scene_id) {
                editor.status = format!("场景{scene_id}地表作者数据无效：{error}"); // 明确失败原因，避免只表现为草稿永久等待。
                return;
            }
            Some(surface.clone())
        }
        None => None,
    };
    let mut migrated_vegetation_authoring = false; // 旧文件只在作者显式保存后发布新图层结构。
    let vegetation = match handles.vegetation.as_ref() {
        Some(handle) => {
            let Some(vegetation) = vegetations.get(handle) else {
                return;
            };
            if vegetation.scene_id != scene_id || vegetation.schema_version != SCENE_SCHEMA_VERSION {
                return;
            }
            let mut vegetation = vegetation.clone();
            migrated_vegetation_authoring =
                ensure_vegetation_authoring(&mut vegetation, &entry.folder); // HN-01和动态场景共享相同无损迁移边界。
            if let Some(authoring) = vegetation.authoring.as_ref()
                && let Err(error) = authoring.validate(&vegetation.layers)
            {
                editor.status = format!("场景{scene_id}植被作者图层无效：{error}"); // 坏引用不得进入整图IO任务。
                return;
            }
            Some(vegetation)
        }
        None => None,
    };
    // 【功能设计】静水拥有独立R8范围；空场景也载入根水，地形只提供岸线裁切。
    let supports_data_driven_water = manifest.is_data_driven()
        && manifest.terrain.kind.supports_static_water();
    let water_catalog_registration_required = false;
    let water_asset_path = entry.water_path.clone().unwrap_or_else(|| {
        format!(
            "{}/{}",
            entry.folder.trim_end_matches('/'),
            SCENE_WATER_FILE_NAME
        )
    }); // 既有自定义路径必须随草稿冻结，保存不能悄悄另写一个无人引用的默认文件。
    let water = if supports_data_driven_water {
        match handles.water.as_ref() {
            Some(handle) => {
                let Some(water) = waters.get(handle) else {
                    return;
                };
                if water.schema_version != SCENE_SCHEMA_VERSION || water.validate(&scene_id).is_err() {
                    return;
                }
                Some(water.clone())
            }
            None => None, // Clipmap标准私有文件必须在IO线程读取、校验和冻结摘要。
        }
    } else {
        None
    };
    let migrated_water_layer_ids = water_catalog_registration_required
        .then(|| {
            water
                .as_ref()
                .map(|water| water.layers.iter().map(|layer| layer.id.clone()).collect())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    let mut boundary_catalog_registration_required = false; // 首次保存时登记新的空墙对象文档。
    let boundaries = match handles.boundaries.as_ref() {
        Some(_) => {
            let Some(boundaries) = boundary_capture.capture.take() else {
                return;
            }; // 只移动完整结果，不再全图clone。
            boundary_capture.capture = default(); // 若后台失败重试，可重新捕获当前完整资产。
            Some(boundaries)
        }
        None if manifest.is_data_driven() && manifest.terrains.is_none() => {
            boundary_catalog_registration_required = true; // 保存时登记独立空气墙文件。
            Some(SceneBoundariesAsset {
                schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION, // 使用当前对象契约。
                scene_id: scene_id.clone(),                              // 不跨场景复用文档。
                paths: Vec::new(), // 缺少文档表示尚未创建墙，不自动补外框。
            })
        }
        None => None, // HN-01、HN-02等专用静态场景继续使用各自自然边界和碰撞实现。
    };
    let mut draft = EditorSceneDraft {
        scene_id: scene_id.clone(),
        folder: entry.folder.clone(),
        water_asset_path,
        manifest: manifest.clone(),
        terrain_ground: None,
        terrain_documents: Default::default(),
        placements: placements.clone(),
        placement_glb_hash: None,
        placement_water_surface_count: 0, // 完整文档计数移到下方IO任务，主线程不额外扫描8192项。
        portals: portals.clone(),
        domain: None,
        preserved_asset_paths: entry.characters_path.iter().cloned().collect::<Vec<_>>().into(),
        terrain_mesh: None,
        terrain_editing_index: None,
        terrain_source_sha256: None,
        terrain_overwrite_authorized_sha256: None,
        heights: None,
        surface,
        puddle_mask: None,    // 尚未加载或创建独立R8作者源。
        puddles_dirty: false, // 未编辑积水不增加额外保存责任。
        surface_weights: None,
        surface_indices: None,
        vegetation,
        vegetation_registration_source_hash: None, // 后台读取旧Clipmap私有文件后再冻结首次登记基线。
        vegetation_densities: BTreeMap::new(),
        vegetation_nonzero_layers: BTreeSet::new(), // 首次后台作者编译会与正式R8同修订发布非零身份。
        vegetation_density_stats: BTreeMap::new(),  // 首次后台作者编译会顺带发布F4所需常数统计。
        vegetation_author_densities: BTreeMap::new(),
        vegetation_conflicts: BTreeMap::new(),
        water,
        water_masks: BTreeMap::new(),
        boundaries,
        terrain_dirty: false,
        surface_dirty: false,
        vegetation_dirty_layers: BTreeSet::new(), // 第二版预设协议不再在载入时改写旧适配器或散布字段。
        vegetation_author_dirty_layers: BTreeSet::new(),
        water_dirty_layers: migrated_water_layer_ids,
        environment_dirty: false, // 刚载入的清单仍由正式动态场景运行时拥有玩法天空。
        water_catalog_registration_required,
        water_registration_source_hash: None, // 未登记Clipmap文件在下方后台读取后填写精确基线。
        boundary_catalog_registration_required,
        boundaries_dirty: false,
    };
    let height_cells_path = match draft.manifest.terrain.kind {
        SceneTerrainKind::Gltf => draft
            .manifest
            .terrain
            .height_texture_path
            .clone()
            .filter(|path| path.to_ascii_lowercase().ends_with(".png")),
        SceneTerrainKind::SparseClipmap | SceneTerrainKind::Empty | SceneTerrainKind::Voxel => None, // 新格式不把稀疏清单误读成旧整图PNG或GLB。
    };
    let root = workspace_asset_root();
    let placement_glb_path = entry.placements_glb.then(|| entry.placements_path());
    let ticket = load.ticket;
    let domain_load = domain_adapter.map(|adapter| adapter.load);
    load.task = Some(IoTaskPool::get().spawn(async move {
        let result: Result<EditorSceneDraft, String> = async {
            if let Some(relative) = placement_glb_path {
                let path = safe_asset_path(&root, &relative)?;
                let bytes = std::fs::read(&path).map_err(|error| format!("读取场景 GLB {} 失败：{error}", path.display()))?;
                let (placements, digest) = AsyncComputeTaskPool::get().spawn(async move {
                    let placements = feathrise_scene_content::read_scene_placements(&bytes, &relative)?;
                    Ok::<_, String>((placements, feathrise_scene_content::authoring_io::bytes_hash(&bytes)))
                }).await?;
                if placements.scene_id != draft.scene_id {
                    return Err("摆放 GLB 属于另一场景".into());
                }
                draft.placements = placements;
                draft.placement_glb_hash = Some(digest); // 快照与保存冲突基线必须来自同一份文件。
            }
            if let Some(boundaries) = draft.boundaries.take() {
                let expected = draft.scene_id.clone(); // 校验纯数据身份。
                let bounds = draft
                    .manifest
                    .world_bounds()
                    .ok_or("场景世界范围无效，无法加载空气墙")?;
                draft.boundaries = Some(
                    AsyncComputeTaskPool::get()
                        .spawn(async move {
                            boundaries.validate(&expected, bounds)?; // 全点集校验在CPU池，不阻塞帧线程。
                            Ok::<_, String>(boundaries)
                        })
                        .await?,
                ); // 仅后台IO任务等待CPU结果。
            }
            super::puddles::read_source(&root, &mut draft)?; // 独立R8与其他作者源在同一个后台加载任务中完成。
            if let Some(read) = domain_load { draft.domain = Some(read(&root, &draft.scene_id)?); } // 具体文档仅由原游戏适配读取。
            if draft.water.is_some() {
                safe_scene_private_asset_path(&root, &draft.folder, &draft.water_asset_path)?; // 自定义catalog入口可以改文件名，但不能在F12会话中指向另一场景。
            }
            if let Some(relative) = height_cells_path {
                let path = safe_asset_path(&root, &relative)?;
                draft.heights = Some(Arc::new(read_height_vertices_png(
                    // 旧PNG高度只进入静态/专用场景的兼容量化高度草稿。
                    &path,                             // 读取旧场景高度顶点PNG。
                    draft.manifest.terrain.resolution, // 使用清单分辨率完成严格转换。
                )?));
            }
            if let Some((resolution, weight_source_path, index_source_path)) =
                draft.surface.as_ref().map(|surface| {
                    (
                        surface.resolution,
                        surface.weight_source_path.clone(),
                        surface.index_source_path.clone(),
                    )
                })
            {
                let texels = resolution[0] as usize * resolution[1] as usize;
                let weight_path = safe_asset_path(&root, &weight_source_path)?;
                let index_path = safe_asset_path(&root, &index_source_path)?;
                draft.surface_weights = Some(Arc::new(
                    // 读取或传递同版本RGBA8权重作者源。
                    // 地表权重加载后即可浅共享给删除历史、预览与保存。
                    read_exact_bytes(&weight_path, texels * 4, "地表权重源")?, // IO任务完整读取受检RGBA8作者源。
                )); // 两张源图均在草稿加载完成后统一采用。
                draft.surface_indices = Some(Arc::new(
                    // 读取或传递同版本小端RGBA32局部索引源。
                    // 地表局部索引和权重使用同一共享生命周期。
                    read_exact_bytes(&index_path, texels * 16, "地表索引源")?, // IO任务完整读取小端RGBA32索引源。
                )); // 失败时整个草稿任务保持原有不采用语义。
            }
            if draft.vegetation.is_none()
                && draft.manifest.is_data_driven()
            {
                // 旧实验场景未登记入口时，仅在后台读取既有私有文档。
                migrated_vegetation_authoring =
                    super::vegetation_load::load_unregistered_clipmap_vegetation(
                        &root, &mut draft,
                    )?; // 缺失建立内存草稿，坏文件明确失败，打开不写盘。
            } // 复用下方正式源和作者源的完整原子加载。
            super::vegetation_load::load_vegetation_sources(
                &root,
                &mut draft,
                migrated_vegetation_authoring,
            )?; // 正式输出、冻结基线与作者R8同一IO任务完整装载。
            if draft.water.is_none() && draft.manifest.is_data_driven()
                && matches!(draft.manifest.terrain.kind, SceneTerrainKind::Empty | SceneTerrainKind::SparseClipmap)
            {
                super::water_load::load_unregistered_clipmap_water(&root, &mut draft)?; // 打开只建立内存草稿；首次显式保存才联合登记目录。
            }
            super::water_load::load_sources(&root, &mut draft)?; // 根水和旧私有水复用完整原子R8载入，不依赖地面。
            draft.placement_water_surface_count = draft
                .placements
                .placements
                .iter()
                .map(|record| record.water_surfaces.len())
                .sum(); // 后台加载顺带建立O(1)全图预算缓存，拖动与按钮帧不再重扫完整文档。
            Ok(draft)
        }
        .await;
        (ticket, result)
    }));
}

/// 完成值只有票据和当前选择仍匹配时才原子替换草稿。
#[allow(clippy::too_many_arguments)] // Bevy系统分别借用任务、草稿、三类预览与UI状态，保持资源写侧互相独立。
pub(crate) fn poll_draft_load(
    mut commands: Commands,
    mut load: ResMut<DraftLoadTask>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    global_environment: Res<super::resources::GlobalEnvironmentAuthoringState>, // 切换场景不得丢弃未保存全局撤销历史。
    placement_previews: Query<Entity, With<EditorPlacedAsset>>,
    portal_previews: Query<Entity, With<EditorPortalPreview>>,
    vegetation_previews: Query<Entity, With<EditorVegetationPreview>>,
    mut surface_preview: ResMut<SurfacePreviewState>,
    mut vegetation_preview: ResMut<VegetationPreviewState>,
    mut water_preview: ResMut<WaterPreviewState>,
    mut water_stroke: ResMut<WaterBrushStroke>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut water_materials: ResMut<Assets<feathrise_scene::DynamicWaterMaterial>>,
    cleanup: (Option<ResMut<super::AuthoringRetirement>>, Local<Option<bevy::tasks::Task<(Vec<EditorCommand>, Vec<EditorCommand>)>>>), // 聚合文档历史与唯一后台回收，保持Bevy系统参数上限。
    mut documents: crate::ui::scene_documents::SceneDocumentLoad,
) {
    let (mut authoring_retirement, mut history_cleanup) = cleanup;
    if session.water_preset_delete_pending || session.model_vegetation_busy { return; } // 保留在途完成值，整笔收口后再按原票据采用。
    if let Some(task) = history_cleanup.as_mut() && let Some((mut undo, mut redo)) = check_ready(task) {
        *history_cleanup = None;
        undo.append(&mut session.undo); redo.append(&mut session.redo);
        session.undo = undo; session.redo = redo; session.saving = false;
    }
    if documents.documents.as_ref().is_some_and(|docs| {
        docs.history_task.is_some()
            || docs.preparing_target.is_some()
            || editor.pending_scene_transition.is_some()
    }) {
        return; // 完整历史尚未交接时保留后台完成值，不发布缺少撤销记录的半套文档。
    }
    let mut cached_document = load.desired_scene_id.as_ref().and_then(|map| {
        documents
            .documents
            .as_mut()
            .and_then(|docs| docs.parked.remove(map))
    });
    let (ticket, result) = if let Some(cached) = cached_document.as_mut() {
        (
            load.ticket,
            Ok(cached.session.draft.take().expect("缓存文档拥有完整草稿")),
        )
    } else {
        let Some(task) = load.task.as_mut() else {
            return;
        };
        let Some(value) = check_ready(task) else {
            return;
        };
        load.task = None;
        value
    };
    if ticket != load.ticket {
        if let Some(retire) = authoring_retirement.as_mut() {
            retire.push(result);
        } // 迟到完整外部文档不能在主帧析构。
        return;
    }
    match result {
        Ok(draft) if load.desired_scene_id.as_ref() == Some(&draft.scene_id) => {
            if cached_document.is_some()
                && documents
                    .residency
                    .as_ref()
                    .is_some_and(|scenes| scenes.restored(&draft.scene_id))
            {
                session.draft = Some(draft);
                crate::ui::scene_documents::restore_document(
                    &mut documents,
                    cached_document,
                    &mut session,
                    &mut editor,
                    &mut commands,
                    authoring_retirement.as_mut().map(|state| &mut **state),
                    global_environment.dirty,
                );
                if let Some(draft) = session.draft.as_ref() {
                    super::placement_deletion::register_document(&mut commands, &draft.scene_id, &session.placement_deletions);
                }
                return; // 驻留场景继续使用原预览实体、网格、材质和任务代次，禁止冷载清理。
            }
            if let Some(previous) = session.draft.as_ref() {
                super::placement_deletion::withdraw_document(&mut commands, &previous.scene_id, &session.placement_deletions);
            }
            let previous_deletions = std::mem::take(&mut session.placement_deletions);
            if let Some(retire) = authoring_retirement.as_mut() {
                retire.push(previous_deletions);
            } // 大删除表与旧草稿共用后台析构；缓存草稿稍后恢复其原逻辑票据，不按场景冷载清空。
            for entity in &placement_previews {
                crate::authoring::placement_preview_ownership::withdraw_cold_preview(&mut commands, entity);
                commands.entity(entity).try_despawn();
            } // 冷载明确撤回作者覆盖；暖驻留恢复在上方返回，不释放其原预览拥有权。
            for entity in portal_previews.iter().chain(vegetation_previews.iter()) {
                commands.entity(entity).try_despawn();
            }
            let scene_id = draft.scene_id.clone();
            let migrated_layer_count = draft.vegetation_dirty_layers.len();
            let migrated_author_layer_count = draft.vegetation_author_dirty_layers.len(); // 旧文件的一对一作者源等待显式保存。
            let previous = session.draft.replace(draft);
            session.sparse_terrain_dirty = false; // 新草稿的稀疏会话随后从已保存清单异步建立。
            if let Some(retire) = authoring_retirement.as_mut() {
                retire.push(previous);
            } // 旧外部文档与其余大草稿一起转移所有权。
            session.revision = session.revision.wrapping_add(1);
            session.placement_structure_revision =
                session.placement_structure_revision.wrapping_add(1); // 同图等长重载也以确定性代次强制刷新GLB实例树。
            session.portal_revision = session.portal_revision.wrapping_add(1); // 切换场景或重载强制双列表采用新传送文档身份。
            if documents.documents.is_some() {
                if let Some(retire) = authoring_retirement.as_mut() {
                    retire.push((
                        std::mem::take(&mut session.undo),
                        std::mem::take(&mut session.redo),
                    ));
                }
            } else if let Some(retire) = authoring_retirement.as_mut() {
                let mut undo = std::mem::take(&mut session.undo);
                let mut redo = std::mem::take(&mut session.redo);
                if global_environment.dirty && history_cleanup.is_none() {
                    *history_cleanup = Some(AsyncComputeTaskPool::get().spawn(async move {
                        undo.retain(EditorCommand::is_global_environment);
                        redo.retain(EditorCommand::is_global_environment);
                        (undo, redo)
                    }));
                } else {
                    retire.push((undo, redo));
                }
            } else if global_environment.dirty {
                session.undo.retain(EditorCommand::is_global_environment); // 场景换代只移除旧场景命令，保留全局线性历史。
                session.redo.retain(EditorCommand::is_global_environment); // 未保存全局重做分支同样跨场景保留。
            } else {
                session.undo.clear(); // 没有全局责任时完整建立新场景历史基线。
                session.redo.clear();
            }
            session.placement_previews.clear();
            session.portal_previews.clear();
            session.next_serial = 0;
            session.saving = history_cleanup.is_some(); // 历史后台清理完成前冻结作者输入。
            *water_stroke = WaterBrushStroke::default(); // 切换场景采用是最终防线，旧pending落点不得借同名layer写入新草稿。
            session.terrain_revision = session.terrain_revision.wrapping_add(1);
            session.surface_revision = session.surface_revision.wrapping_add(1);
            session.vegetation_revision = session.vegetation_revision.wrapping_add(1);
            session.vegetation_structure_revision = session.vegetation_structure_revision.wrapping_add(1);
            session.vegetation_compiled_revision = session.vegetation_revision.wrapping_sub(1); // 后台重编一次以生成最终图与冲突诊断，期间正式运行时继续显示已保存输出。
            session.water_revision = session.water_revision.wrapping_add(1);
            session.environment_revision = session.environment_revision.wrapping_add(1); // 切换场景使旧天空Image事件立即过期。
            session.boundary_revision = session.boundary_revision.wrapping_add(1); // 切换场景使上一场景未保存碰撞覆盖立即过期。
            surface_preview.applied_scene_id = None;
            surface_preview.desired_scene_id = Some(scene_id.clone());
            for layer in vegetation_preview.layers.values_mut() {
                for (entity, visibility) in layer.hidden_entities.drain(..) {
                    commands.entity(entity).try_insert(visibility);
                }
            }
            vegetation_preview.layers.clear();
            vegetation_preview.desired_scene_id = Some(scene_id.clone());
            vegetation_preview.desired_layer_id = None;
            vegetation_preview.task = None;
            vegetation_preview.install = None; // 上方已按EditorVegetationPreview标记回收旧隐藏父根，资源同步释放安装身份。
            for spawn in water_preview.spawns.drain(..) {
                commands.entity(spawn.entity).try_despawn();
                meshes.remove(spawn.mesh.id());
                water_materials.remove(spawn.material.id());
            }
            for (entity, visibility) in water_preview.hidden_runtime_entities.drain(..) {
                commands.entity(entity).try_insert(visibility);
            }
            water_preview.retarget(
                Some(scene_id.clone()),
                session.water_revision,
                session.terrain_revision,
            ); // 在途旧图CPU任务保留到自然结束；完成值由新身份拒收后才释放single-flight槽。
            editor.selected_surface_layer = 0;
            editor.surface_selected_preset_id = None; // 地表预设选择只属于原场景作者会话，不能在切换场景后把首次落笔写成旧图所选材质。
            editor.selected_vegetation_layer = 0;
            editor.vegetation_selected_preset_id = None; // 稳定预设选择仍属于当前作者会话，不跨场景继承。
            editor.vegetation_layer_page = 0; // 切换场景后固定行池必须回到第一批作者层。
            editor.selected_water_layer = 0;
            editor.water_selected_preset_id = session
                .draft
                .as_ref()
                .and_then(|draft| draft.water.as_ref())
                .and_then(|water| water.layers.first())
                .and_then(|layer| (!layer.preset_id.is_empty()).then(|| layer.preset_id.clone())); // 切换场景后只从新图首个静水层恢复真实预设身份，绝不继承旧图选择。
            editor.selected_water_river = 0; // 新场景河流选择从第一项重新建立。
            editor.selected_waterfall = 0; // 瀑布选择不得沿用上一场景数组下标。
            editor.water_feature_page = 0; // 水域固定行池不得沿用上一场景的页码。
            editor.selected_current_portal_id = None; // 当前传送点稳定ID不能跨场景复用。
            editor.portal_target = None; // 旧目标场景/端点不能在切换场景后静默继承。
            editor.selected_water_control_point = None; // 稳定点身份只属于原场景水网。
            editor.water_network_action_requested = None; // 丢弃切换场景前尚未消费的一帧按钮请求。
            editor.water_level_from_cursor_requested = None; // 跨帧取水位身份不能进入另一个场景。
            let water_registration_required = session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.water_catalog_registration_required);
            let clipmap_water_registration_required = session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.water_registration_source_hash.is_some()); // 旧Clipmap标准私有文件只在首次显式保存时补目录入口。
            let migrated_water_registration_required =
                water_registration_required && !clipmap_water_registration_required;
            let boundary_registration_required = session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.boundary_catalog_registration_required); // 旧动态场景默认安全圈等待首次保存。
            let vegetation_registration_required = session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.vegetation_registration_source_hash.is_some()); // 打开旧图只建立明确待保存的目录补登记责任。
            let loaded_migration = vegetation_registration_required // 空白旧图也必须能显式保存入口。
                || migrated_layer_count > 0
                || migrated_author_layer_count > 0
                || water_registration_required
                || boundary_registration_required;
            session.loaded_migration_revision = loaded_migration.then_some(session.revision); // 只记录本次完整加载补齐，不把之后作者编辑当成初始化。
            editor.dirty = loaded_migration || global_environment.dirty; // 顶栏保存责任保留；纯加载补齐不阻止下一次切换场景。
            editor.status = if migrated_layer_count > 0
                || migrated_author_layer_count > 0
                || migrated_water_registration_required
                || boundary_registration_required
            {
                format!(
                    "场景{scene_id}已自动迁移{migrated_layer_count}个正式植被层/参数和{migrated_author_layer_count}张一对一作者画布{}{}；运行时层数不变，保存后写入场景私有文件",
                    if migrated_water_registration_required {
                        "和1个湖泊画布"
                    } else {
                        ""
                    },
                    if boundary_registration_required {
                        "和1条闭合区域安全边界"
                    } else {
                        ""
                    },
                )
            } else if vegetation_registration_required || clipmap_water_registration_required {
                // 单独说明旧Clipmap读取与写盘边界，打开场景本身不得创建或覆盖私有文件。
                let registration = match (
                    vegetation_registration_required,
                    clipmap_water_registration_required,
                ) {
                    (true, true) => "Blade植被与静水",
                    (true, false) => "Blade植被",
                    (false, true) => "静水",
                    (false, false) => unreachable!("分支已确认至少存在一个补登记责任"),
                };
                format!(
                    "场景{scene_id}作者数据已就绪；首次保存将登记{registration}入口，打开过程未改写文件"
                ) // 不把空文档误报为已有草地或湖泊。
            } else if session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.manifest.terrain.kind == SceneTerrainKind::SparseClipmap)
            {
                // 稀疏高度不是只读Blender地形。
                format!(
                    "场景{scene_id}作者数据已就绪；Clipmap地形、地表、植被、水域、摆放、建筑院落、传送点和空气墙可编辑；整网格水在场景树的网格属性中设置"
                ) // 明确本次能力边界。
            } else {
                format!(
                    "场景{scene_id}作者数据已就绪；静态模型可通过表面绘制植被；高度地形使用Clipmap，体素地形使用密度源"
                )
            };
            crate::ui::scene_documents::restore_document(
                &mut documents,
                cached_document,
                &mut session,
                &mut editor,
                &mut commands,
                authoring_retirement.as_mut().map(|state| &mut **state),
                global_environment.dirty,
            ); // 缓存也经过同一预览回收与草稿采用路径，完整历史就绪后才恢复作者状态。
            if let Some(draft) = session.draft.as_ref() {
                super::placement_deletion::register_document(&mut commands, &draft.scene_id, &session.placement_deletions);
            }
        }
        Ok(draft) => {
            if let Some(retire) = authoring_retirement.as_mut() {
                retire.push(draft);
            }
        }
        Err(error) => {
            editor.status = format!("场景作者数据加载失败：{error}");
            error!("{}", editor.status);
        }
    }
}

#[cfg(test)]
mod tests; // 地表独立版本门的回归测试放在子模块，避免继续拉长作者加载主流程。
