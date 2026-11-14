//! 场景草稿的完整事务保存批次构建。
//!
//! 【功能设计】本模块先验证全部输入并收集全部目标，再把编码器和冻结输入交给事务逐项
//! stage；大型KTX2/GLB不会同时驻留，任一编码失败仍发生在正式目标开始提交之前。

use std::collections::BTreeSet; // 摆放实例稳定ID在首次stage前完成确定性去重。
use std::path::Path; // 所有正式目标必须位于受检资产根。
use std::sync::{Arc, Mutex}; // 保存身份共享不可变句柄，延迟纹理编码在同一事务间传递完整结果。

use bevy::math::Rect; // 保存前使用清单受检XZ范围复用公共摆放变换校验。
use feathrise_scene_content::{
    SCENE_BOUNDARIES_FILE_NAME, SCENE_MANIFEST_FILE_NAME,
    SCENE_PORTALS_FILE_NAME, SCENE_SCHEMA_VERSION, SCENE_SURFACE_FILE_NAME, SCENE_SURFACE_SCHEMA_VERSION,
    SCENE_VEGETATION_FILE_NAME, MAX_SCENE_PLACEMENTS_PER_SCENE, SceneId, ScenePlacementsAsset,
    SceneSurfaceAsset, SurfaceSourceSaveIdentity, WORLD_ENVIRONMENT_ASSET_PATH, WorldEnvironmentAsset,
}; // 场景协议固定目标与成功结果身份。

use super::super::files::{safe_asset_path, workspace_asset_root}; // 所有自定义输出继续受assets根约束。
use super::super::image_io::{encode_density_png, is_png_path}; // PNG作者源逐项编码到stage。
use super::super::ktx2::{encode_r8, encode_surface_maps}; // GPU纹理派生逐项生成并立即释放。
use super::super::resources::EditorSceneDraft; // 后台任务冻结的完整作者草稿。
use super::output_paths::collect_reserved_asset_paths; // 全批目标先做跨域碰撞预检。
use super::preflight::prepare_water_files; // 水网、预算、路径与R8输入先取得纯预检完成值。
use super::publish::placement_documents_equal; // 无变化摆放不重写文件，避免文件监听制造伪Modified。
use super::transaction::{
    PreparedWrite, TransactionFailure, commit_prepared_writes, commit_prepared_writes_checked,
}; // 唯一允许正式落盘的跨文件事务入口。
use super::write_helpers::{
    prepare_optional_catalog_bytes, resolve_water_json_output_path,
}; // R16编码、自定义water JSON与目录新快照。
use feathrise_scene_content::SceneTerrainKind; // Clipmap三维补片和地表写入共用地形类型约束。

/// 在任何stage或正式文件替换前完整验证摆放文档与整网格水体引用。
#[cfg(test)]
fn validate_placements(
    document: &ScenePlacementsAsset, // 即将进入事务stage的完整摆放文档。
    scene_id: &SceneId,                // 当前草稿场景身份。
    world_bounds: Rect,            // 当前清单已经确认有效的世界XZ范围。
) -> Result<(), String> {
    validate_placements_by_source(document, scene_id, world_bounds, None)
}
fn validate_placements_by_source(document: &ScenePlacementsAsset, scene_id: &SceneId, world_bounds: Rect,
    objects: Option<&[feathrise_scene_content::SceneTerrainObject]>) -> Result<(),String> {
    if document.schema_version != SCENE_SCHEMA_VERSION || document.scene_id != *scene_id {
        return Err(format!("场景{scene_id}摆放文档的版本或场景身份不一致")); // 错图或旧格式不能进入同一场景的原子保存批次。
    }
    document
        .validate_water_surfaces()
        .map_err(|error| format!("场景{scene_id}的整网格水体文档无效：{error}"))?; // 单项格式与全图256绑定预算在任何文件stage前复用公共纯校验。
    document
        .validate_courtyards()
        .map_err(|error| format!("场景{scene_id}的院落成员文档无效：{error}"))?; // 同一副本的预设、锚点与成员身份必须在任何保存stage前一致。
    if document.placements.len() > MAX_SCENE_PLACEMENTS_PER_SCENE {
        return Err(format!(
            "场景{scene_id}包含{}个显式摆放，超过上限{MAX_SCENE_PLACEMENTS_PER_SCENE}",
            document.placements.len(),
        )); // 复制操作和手改JSON都必须在任何stage之前命中同一数量硬门。
    }
    let mut placement_ids = BTreeSet::new(); // 水体覆盖内嵌于实例，先保证实例身份本身唯一。
    let mut terrain_patches = 0;
    let mut terrain_holes = 0;
    for placement in &document.placements {
        if placement.id.trim().is_empty() || !placement_ids.insert(placement.id.as_str()) {
            return Err(format!("场景{scene_id}包含空白或重复的摆放实例ID")); // 运行时根和场景树都依赖唯一placement id。
        }
        let bounds = if let Some(patch) = &placement.terrain_patch && let Some(objects) = objects {
            let id = patch.terrain_id.clone().unwrap_or_else(feathrise_scene_content::TerrainId::legacy);
            let source = objects.iter().find(|object| object.id == id && object.terrain.kind == SceneTerrainKind::SparseClipmap)
                .ok_or_else(|| format!("三维地形{}关联的Clipmap不存在",placement.id))?;
            Rect::from_corners(source.world_min.into(), source.world_max.into())
        } else { world_bounds };
        placement.validated_transform(scene_id, bounds)?; // bounds只校验关联地形切洞，普通摆放不受容器范围约束。
        // 一个雕刻根可带多个洞口；按受检的完整列表累计，避免根数合法却保存出超过运行时容量的文档。
        terrain_holes += placement.terrain_cutouts(bounds)?.len();
        if terrain_holes > feathrise_scene_content::MAX_TERRAIN_CUTOUTS {
            return Err("场景三维地形洞口超过32项预算".into());
        }
        terrain_patches += usize::from(placement.terrain_patch.is_some());
        if terrain_patches > feathrise_scene_content::MAX_TERRAIN_CUTOUTS {
            return Err("场景三维地形实例超过32项预算".into());
        }
    }
    Ok(()) // 文档与全部内嵌水体覆盖都可安全序列化和正式采用。
}

#[derive(Debug)]
pub(crate) struct DraftSaveError {
    message: String,                           // 面向作者的完整失败与回滚诊断。
    reload_asset_paths: Vec<String>, // 主线程必须显式重载，纠正文件监听可能缓存的中间新版本。
    rollback_complete: bool, // false表示磁盘仍可能混合新旧目标，只能进入不可关闭的待重试状态。
    terrain_conflict_sha256: Option<[u8; 32]>, // Blender并发版本供“采用/覆盖”弹框绑定一次性授权。
}

impl DraftSaveError {
    pub(super) fn from_transaction(root: &std::path::Path, failure: TransactionFailure) -> Self {
        let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let reload_asset_paths = failure
            .rollback_targets()
            .iter()
            .filter_map(|path| {
                path.strip_prefix(root)
                    .or_else(|_| path.strip_prefix(&canonical_root))
                    .ok()
            }) // 兼容旧编辑器词法根和外部文档服务的规范根。
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect(); // 所有保存目标已受assets根约束，转换后可直接交给AssetServer。
        Self {
            message: failure.message().to_owned(), // 保留事务原始失败和回滚完整性。
            reload_asset_paths,                    // 空集合表示正式目标从未开始发布。
            rollback_complete: failure.rollback_complete(),
            terrain_conflict_sha256: None,
        }
    }

    pub(crate) fn reload_asset_paths(&self) -> &[String] {
        &self.reload_asset_paths
    }

    pub(crate) fn rollback_complete(&self) -> bool {
        self.rollback_complete
    }

    pub(crate) fn terrain_conflict_sha256(&self) -> Option<[u8; 32]> {
        self.terrain_conflict_sha256
    }

}

impl From<String> for DraftSaveError {
    fn from(message: String) -> Self {
        Self {
            message,                        // 纯预检或批次构建失败的原始诊断。
            reload_asset_paths: Vec::new(), // 正式目标尚未提交，无需纠正AssetServer缓存。
            rollback_complete: true,
            terrain_conflict_sha256: None,
        }
    }
}

impl std::fmt::Display for DraftSaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

/// 保存成功后交给主线程的结构化发布决定。
pub(crate) struct DraftSaveSuccess {
    pub(crate) terrain_manifest: Option<feathrise_scene_content::SceneManifestAsset>,
    pub(crate) terrain_saved: Vec<feathrise_scene_content::TerrainId>,
    pub(crate) terrain_reload_paths: Vec<String>,
    pub(crate) terrain_digests: Vec<(feathrise_scene_content::TerrainId, [u8; 32])>,
    pub(crate) terrain_water_changed: std::collections::BTreeSet<feathrise_scene_content::TerrainId>, // 只为真实水定义/遮罩或依赖高度变化的画布建立正式水交接。
    pub(crate) root_water_changed: bool, // 根水与私有画布分别建立正式门，其他作者域保存不重发根水。
    pub(crate) domain_hash: Option<String>, // 本批真正写入的外部文档原字节基线。
    pub(crate) scene_id: SceneId,                   // 本次原子事务所属场景。
    pub(crate) placements_changed: bool, // 只有语义内容相对磁盘真实变化时才允许发布正式摆放资产。
    pub(crate) placement_glb_hash: Option<String>,
    pub(crate) surface_definition_changed: bool, // 地表JSON进入本批时才允许发布正式定义资产。
    pub(crate) surface_weights_changed: bool, // RGBA作者源与KTX2实际进入本批时才允许重载权重纹理。
    pub(crate) active_terrain_surface_changes: Option<(bool, bool)>, // 当前独立画布的定义/像素真实写入回执，不能用空容器的false代替。
    pub(crate) surface_source_identity: Option<Arc<SurfaceSourceSaveIdentity>>, // 本次实际保存画布的内容证据，迟到资产通知不能冒充。
    pub(crate) global_environment_saved: bool, // true 表示全局配置与场景文件已在同一事务完整发布。
}

/// 完整待写集合与各类内容变化证据必须来自同一次后台预检。
struct PreparedDraftWrites<'a> {
    domain_hash: Option<String>, // 只有外部脏文档进入事务才有新基线。
    writes: Vec<PreparedWrite<'a>>,  // 仍由唯一跨文件事务统一stage、发布和回滚。
    placements_changed: bool,        // false时批次中完全没有placements目标。
    placement_glb_hash: Option<String>,
    surface_definition_changed: bool, // false时批次中完全没有地表JSON目标。
    surface_weights_changed: bool,   // false时批次中完全没有RGBA作者源和权重KTX2目标。
    water_changed: bool, // 后台比较小型水JSON；正文排版和其他作者域变化不要求无关水画布重建。
}

pub(super) async fn save_draft_with_volumes(
    draft: &EditorSceneDraft,
    global_environment: Option<&WorldEnvironmentAsset>,
    volumes: &[crate::authoring::volume_sculpt::VolumeSaveFile],
) -> Result<DraftSaveSuccess, DraftSaveError> {
    let verified_volumes = super::volume::verify_async(volumes).await?; // CPU等值面校验完成前不持有作者IO锁，不创建stage。
    let (terrain_manifest, object_drafts, terrain_saved) = super::terrain_documents::prepare(draft).await?;
    let manifest_bytes = if let Some(manifest) = &terrain_manifest {
        let snapshot = manifest.clone();
        let target = workspace_asset_root().join(&draft.folder).join(SCENE_MANIFEST_FILE_NAME);
        Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
            feathrise_scene_content::authoring_transaction::encode_json_bytes(&target, &snapshot)
        }).await?)
    } else { None };
    let root = workspace_asset_root();
    let _author_lock = feathrise_scene_content::ContentAuthorLock::acquire(&root)?; // 与AI及预设删除同源串行保护引用。
    if !draft.manifest.subscenes.is_empty() {
        let catalog_path = root.join(feathrise_scene_content::SCENE_CATALOG_ASSET_PATH);
        let bytes = std::fs::read(&catalog_path).map_err(|e| format!("读取子场景引用目录失败：{e}"))?;
        let catalog = super::super::files::decode_scene_catalog(&catalog_path, &bytes)?;
        feathrise_scene_content::validate_saved_scene_instances(&root, &catalog, &draft.manifest)?;
    } // 同一作者锁内验证完整来源链，任何失败发生在首个stage创建之前。
    let source_guard = super::domain_files::DraftSourceSaveGuard::prepare(&root, draft)?;
    let mut prepared = prepare_draft_writes(&root, draft, global_environment)?;
    let mut terrain_reload_paths = Vec::new();
    let mut terrain_water_changed = std::collections::BTreeSet::new();
    let mut active_terrain_surface_changes = None;
    let mut surface_source_identity = surface_save_identity(draft, prepared.surface_definition_changed, prepared.surface_weights_changed)?;
    for object in &object_drafts {
        let object_writes = prepare_draft_writes(&root, object, None)?;
        if object_writes.water_changed && let Some(active) = &object.terrain_documents.active {
            terrain_water_changed.insert(active.definition.id.clone());
        } // 删除但为撤销保留的脏画布仍交接正式文件；冻结的runtime_required阻止等待不存在的实例。
        if object.terrain_documents.active.as_ref().map(|active| &active.definition.id)
            == draft.terrain_documents.active.as_ref().map(|active| &active.definition.id) {
            active_terrain_surface_changes = Some((object_writes.surface_definition_changed, object_writes.surface_weights_changed));
            surface_source_identity = surface_save_identity(object, object_writes.surface_definition_changed, object_writes.surface_weights_changed)?;
        } // 根容器没有地表写项，当前画布仍必须按它实际提交的四槽源武装保存交接门。
        terrain_reload_paths.extend(object_writes.writes.iter().filter_map(|write| write.path.strip_prefix(&root).ok()
            .map(|relative| relative.to_string_lossy().replace('\\', "/"))));
        prepared.writes.extend(object_writes.writes);
    }
    if let Some(bytes) = manifest_bytes {
        let path = root.join(&draft.folder).join(SCENE_MANIFEST_FILE_NAME);
        prepared.writes.retain(|write| write.path != path);
        prepared.writes.push(PreparedWrite::new(path, bytes)); // CPU编码后的独占清单最后发布，不把借用延伸到成功结果。
    }
    let generated = super::volume::prepare_verified(&root, draft, volumes, &verified_volumes)?;
    prepared.writes.splice(0..0, generated); // 生成GLB先于引用它的placement JSON提交，回滚仍覆盖同一批。
    let PreparedDraftWrites { domain_hash, mut writes, placements_changed, placement_glb_hash, surface_definition_changed, surface_weights_changed, water_changed } = prepared;
    let catalog_path = root.join(feathrise_scene_content::SCENE_CATALOG_ASSET_PATH);
    if let Some(index) = writes.iter().position(|write| write.path == catalog_path) {
        let catalog = writes.remove(index);
        writes.push(catalog); // 根水首次登记要在所有根/私有源、派生和容器清单完整发布之后。
    }
    super::output_paths::validate_preserved_document_outputs(&root, draft, &writes, domain_hash.is_some())?;
    // 私有画布和体积生成写项在此已全部合并，不能绕过根文档保留路径的零写入拒绝门。
    let root = workspace_asset_root();
    let placement_path = safe_asset_path(&root, &draft.placements_path())?;
    let check_source = |path: &Path| {
        source_guard.check(path)?;
        if path == placement_path && let Some(expected) = &draft.placement_glb_hash {
            verify_placement_glb_hash(expected, &feathrise_scene_content::authoring_io::file_hash(path)?)?;
        }
        Ok(())
    };
    commit_prepared_writes_checked(writes, |_, path| check_source(path))
        .map_err(|failure| DraftSaveError::from_transaction(&root, failure))?;
    // 高度作者源由分页文档保存；模型继续使用普通 scene.glb 摘要守卫。
    let terrain_digests = Vec::new();
    Ok(DraftSaveSuccess {
        domain_hash, // 成功之后才更新主世界外部文档磁盘基线。
        root_water_changed: water_changed,
        scene_id: draft.scene_id.clone(),              // 主线程只接纳同票据同作者修订结果。
        placements_changed, // 发布决策与实际事务目标严格同源。
        placement_glb_hash,
        surface_definition_changed, // 只发布本批真正替换的地表JSON。
        surface_weights_changed, // 只重载本批真正替换的权重纹理。
        active_terrain_surface_changes,
        surface_source_identity,
        global_environment_saved: global_environment.is_some(), // 成功返回即证明全局与场景属于同一次完整提交。
        terrain_manifest, terrain_saved, terrain_reload_paths, terrain_digests, terrain_water_changed,
    })
}

/// 仍在原有保存worker内冻结定义与本批像素摘要；成功主帧只移动Arc，不重新扫描四槽画布。
fn surface_save_identity(draft: &EditorSceneDraft, definition_changed: bool, pixels_changed: bool) -> Result<Option<Arc<SurfaceSourceSaveIdentity>>, String> {
    if draft.terrain_manifest().terrain.kind != SceneTerrainKind::SparseClipmap || (!definition_changed && !pixels_changed) {
        return Ok(None);
    }
    let Some(surface) = draft.surface.as_ref() else { return Ok(None); };
    let identity = if pixels_changed {
        let indices = draft.surface_indices.as_deref().ok_or("保存地表缺少索引内容证据")?;
        let weights = draft.surface_weights.as_deref().ok_or("保存地表缺少权重内容证据")?;
        SurfaceSourceSaveIdentity::for_author_buffers(surface, indices, weights)
    } else { SurfaceSourceSaveIdentity::for_definition(surface) };
    Ok(Some(Arc::new(identity)))
}

fn prepare_draft_writes<'a>(
    root: &Path,
    draft: &'a EditorSceneDraft,
    global_environment: Option<&'a WorldEnvironmentAsset>,
) -> Result<PreparedDraftWrites<'a>, String> {
    let canvas_only = draft.terrain_documents.detached;
    let container = draft.manifest.terrains.is_some() && !canvas_only;
    let root_water = super::super::terrain_documents::root_water::source(draft);
    if canvas_only { super::super::terrain_documents::TerrainCanvas::snapshot(draft).validate_paths(root, &draft.folder)?; }
    feathrise_scene_content::validate_terrain_objects(&draft.manifest)?;
    if draft.folder.is_empty()
        || draft.folder.starts_with('/')
        || draft.folder.contains("..")
        || draft.folder.contains('\\')
    {
        return Err(format!("场景{}私有目录越过assets根", draft.scene_id));
    }
    let folder = root.join(&draft.folder);
    let water_json_path = resolve_water_json_output_path(
        &root,
        &draft.folder,
        root_water.asset_path,
        root_water.water.is_some(),
    )?; // 旧场景自定义water_path继续写回目录实际声明的私有文件。

    draft.manifest.environment.validate()?; // 非有限参数和伪天空路径不得进入权威清单。
    let placement_bounds = draft
        .manifest
        .world_bounds()
        .ok_or_else(|| format!("场景{}清单世界范围无效", draft.scene_id))?; // 摆放变换不能借用损坏清单伪造边界。
    validate_placements_by_source(&draft.placements, &draft.scene_id, placement_bounds, draft.manifest.terrains.as_deref())?;
    if draft.manifest.terrains.is_none() && draft.manifest.legacy_terrain().kind != SceneTerrainKind::SparseClipmap
        && draft
            .placements
            .placements
            .iter()
            .any(|record| record.terrain_patch.is_some())
    {
        return Err("三维地形洞口只允许保存到 Clipmap 场景".into());
    }
    let reserved_asset_paths = collect_reserved_asset_paths(&root, draft)?;
    let water_files = prepare_water_files(
        &root,
        &draft.scene_id,
        &draft.folder,
        root_water.water,
        root_water.dirty_layers,
        root_water.masks,
        &reserved_asset_paths,
    )?; // 水域正式格式、运行时预算、输出路径与R8长度全部先纯验收。

    let mut writes: Vec<PreparedWrite<'a>> = Vec::new();
    let domain_hash = if canvas_only { None } else {
        super::domain_files::prepare_domain_write(&root, draft, &mut writes)?
    }; // 对象投影不能写另一份外部文档。
    if let Some(global_environment) = global_environment {
        global_environment.validate()?; // 无效全局草稿在任何场景或全局 stage 前失败关闭。
        writes.push(PreparedWrite::json(
            root.join(WORLD_ENVIRONMENT_ASSET_PATH), // 唯一全局目标与场景文件进入同一跨目录回滚事务。
            global_environment,
        ));
    }
    if !canvas_only { writes.push(PreparedWrite::json(
        folder.join(SCENE_MANIFEST_FILE_NAME),
        &draft.manifest,
    )); }
    let (placements_changed, placement_glb_hash) = if canvas_only {
        (false, None) // 对象投影没有独立摆放文档。
    } else {
        prepare_placements_write(root, draft, &mut writes)?
    };
    if !canvas_only { writes.push(PreparedWrite::json(
        folder.join(SCENE_PORTALS_FILE_NAME),
        &draft.portals,
    )); }

    if let Some(boundaries) = draft.boundaries.as_ref() {
        let world_bounds = draft
            .manifest
            .world_bounds()
            .ok_or_else(|| "场景世界范围无效，无法保存区域边界".to_owned())?;
        boundaries.validate(&draft.scene_id, world_bounds)?; // 未闭合或越界草稿在任何暂存前拒绝。
        writes.push(PreparedWrite::json(
            folder.join(SCENE_BOUNDARIES_FILE_NAME),
            boundaries,
        ));
    }

    let mut surface_definition_changed = false; // 未声明地表的场景不制造定义写入或资产事件。
    let mut surface_weights_changed = false; // 高度等无关保存不能误触发权重纹理重载。
    if !container && let Some(surface) = draft.surface.as_ref() {
        if surface.schema_version != SCENE_SURFACE_SCHEMA_VERSION
            || surface.scene_id != draft.scene_id
            // 空层表是删除最后一层后的合法状态；像素完整性由配对编码验证。
            || surface.layers.iter().any(|layer| {
                layer.id.trim().is_empty()
                    || layer.label.trim().is_empty()
                    || layer.preset_id.trim().is_empty()
            })
        {
            return Err(format!(
                "场景{}的地表版本、身份或预设引用无效",
                draft.scene_id
            ));
        }
        let mut layer_ids = BTreeSet::new();
        if surface
            .layers
            .iter()
            .any(|layer| !layer_ids.insert(layer.id.as_str()))
        {
            return Err(format!("场景{}包含重复地表层 ID", draft.scene_id));
        }
        let surface_path = folder.join(SCENE_SURFACE_FILE_NAME); // 比较与正式事务必须指向同一权威JSON。
        let saved_surface_bytes = std::fs::read(&surface_path).ok(); // 有界后台保存任务只读取这一份小型定义。
        (surface_definition_changed, surface_weights_changed) =
            surface_write_changes(saved_surface_bytes.as_deref(), surface, draft.surface_dirty); // 定义与权重发布证据必须和本批目标由同一纯决定产生。
        if surface_definition_changed {
            writes.push(PreparedWrite::json(surface_path, surface)); // 缺失、损坏或真实变化仍由整批事务完整覆盖。
        }
        if draft.surface_dirty {
            let weights = draft
                .surface_weights
                .as_ref()
                .ok_or_else(|| "地表标记为已修改但缺少 RGBA8 权重作者源".to_owned())?;
            let indices = draft
                .surface_indices
                .as_ref()
                .ok_or_else(|| "地表标记为已修改但缺少 RGBA32 索引作者源".to_owned())?;
            validate_grid_len("地表RGBA8作者源", weights.len(), surface.resolution, 4)?;
            validate_grid_len("地表RGBA32索引源", indices.len(), surface.resolution, 16)?;
            let weight_source_path = safe_asset_path(&root, &surface.weight_source_path)?;
            let index_source_path = safe_asset_path(&root, &surface.index_source_path)?;
            let weight_texture_path = safe_asset_path(&root, &surface.weight_texture_path)?;
            let index_texture_path = safe_asset_path(&root, &surface.index_texture_path)?;
            writes.push(PreparedWrite::borrowed(
                index_source_path,
                indices.as_slice(),
            ));
            writes.push(PreparedWrite::borrowed(
                weight_source_path,
                weights.as_slice(),
            ));
            let resolution = surface.resolution;
            let layer_count = u32::try_from(surface.layers.len())
                .map_err(|_| "场景地表层数量超出 u32 可表达范围".to_owned())?;
            let pending_weight_texture = Arc::new(Mutex::new(None::<Vec<u8>>));
            let pending_weight_for_index = Arc::clone(&pending_weight_texture);
            writes.push(PreparedWrite::deferred(index_texture_path, move || {
                let (index_texture, weight_texture) =
                    encode_surface_maps(indices, weights, resolution, layer_count)?;
                *pending_weight_for_index
                    .lock()
                    .map_err(|_| "地表派生纹理临时槽被破坏".to_owned())? = Some(weight_texture);
                Ok(index_texture)
            })); // 同一次聚合保证两张纹理各级 Mip 严格匹配。
            writes.push(PreparedWrite::deferred(weight_texture_path, move || {
                pending_weight_texture
                    .lock()
                    .map_err(|_| "地表派生纹理临时槽被破坏".to_owned())?
                    .take()
                    .ok_or_else(|| "地表索引 Mip 未先完成，无法发布匹配权重".to_owned())
            })); // 索引 stage 完成后只短暂保留一张权重 KTX2，不重算全图。
        }
    }

    let vegetation_source = if container {
        super::super::terrain_documents::root_vegetation::source(draft)
    } else {
        Some(super::super::terrain_documents::root_vegetation::VegetationSource::active(draft))
    };
    if let Some(source) = vegetation_source.filter(|source| !container || source.has_document()) {
        prepare_vegetation_writes(root, &draft.folder, &source, &mut writes)?;
    }

    if !container { super::super::puddles::prepare_writes(&root, draft, &mut writes)?; }
    let mut water_changed = false;
    if let Some(water) = root_water.water {
        let path = water_json_path.expect("存在水域资产时上方已解析权威JSON路径");
        water_changed = !water_files.is_empty() || (!container && draft.terrain_dirty)
            || root_water.registration_required
            || !std::fs::read(&path).ok().is_some_and(|bytes| saved_water_bytes_match(&bytes, water));
        for prepared in water_files {
            let resolution = prepared.resolution;
            let texture_mask = prepared.mask.clone();
            writes.push(PreparedWrite::shared(prepared.source_path, prepared.mask));
            writes.push(PreparedWrite::deferred(prepared.texture_path, move || {
                encode_r8(texture_mask.as_slice(), resolution)
            })); // 每个最大水层KTX2完成stage后即释放，再编码下一层。
        }
        if water_changed || !canvas_only && !container {
            writes.push(PreparedWrite::json(path, water));
        } // 独立画布仅其他作者域变化时不重发相同水JSON；真实水改动仍与R8/KTX2共用唯一事务。
    }

    if !canvas_only && let Some((catalog_path, catalog_bytes)) = prepare_optional_catalog_bytes(&root, draft)? {
        writes.push(PreparedWrite::new(catalog_path, catalog_bytes)); // 目录始终是本批最后一个可见目标。
    }
    super::output_paths::validate_preserved_document_outputs(root, draft, &writes, domain_hash.is_some())?;
    // 根批次先拒绝领域正文与普通目标的别名，后续清单重编码不能移除真实领域写项后冒用它的权限。
    Ok(PreparedDraftWrites {
        domain_hash,
        writes,                     // transaction会在创建首个stage前再次拒绝任何目标路径别名。
        placements_changed,         // 主线程据此决定是否建立正式placement交接门。
        placement_glb_hash,
        surface_definition_changed, // 主线程据此决定是否发布唯一正式地表定义句柄。
        surface_weights_changed,    // 主线程据此决定是否显式重载权重KTX2。
        water_changed,
    })
}

/// 只修改全局配置时使用单文件原子事务，不构建或预检任何场景派生内容。
pub(super) fn save_global_environment(asset: &WorldEnvironmentAsset) -> Result<(), DraftSaveError> {
    asset.validate()?; // 数值或版本错误必须发生在首个 stage 前。
    let root = workspace_asset_root(); // 与联合事务使用同一真实 assets 根。
    let writes = vec![PreparedWrite::json(
        root.join(WORLD_ENVIRONMENT_ASSET_PATH), // 固定常量目标不接受用户路径输入。
        asset,
    )];
    commit_prepared_writes(writes)
        .map_err(|failure| DraftSaveError::from_transaction(&root, failure)) // 即使单文件替换失败也保留统一回滚诊断。
}

/// 读取旧正式摆放文档并做语义比较；缺失或损坏文件继续按旧行为由事务完整覆盖。
fn verify_placement_glb_hash(expected: &str, actual: &str) -> Result<(), String> {
    if expected != actual {
        return Err("场景 GLB 已被外部程序修改，请重新打开场景后保存".into());
    }
    Ok(())
}

fn prepare_placements_write<'a>(
    root: &Path,
    draft: &'a EditorSceneDraft,
    writes: &mut Vec<PreparedWrite<'a>>,
) -> Result<(bool, Option<String>), String> {
    let relative = draft.placements_path();
    let path = safe_asset_path(root, &relative)?;
    let Some(expected) = &draft.placement_glb_hash else {
        if saved_placements_match(&path, &draft.placements) {
            return Ok((false, None));
        }
        writes.push(PreparedWrite::json(path, &draft.placements));
        return Ok((true, None));
    };
    use feathrise_scene_content::{authoring_io::bytes_hash, read_scene_placements, write_scene_placements_glb};
    let previous = std::fs::read(&path).map_err(|error| format!("读取场景 GLB {} 失败：{error}", path.display()))?;
    verify_placement_glb_hash(expected, &bytes_hash(&previous))?;
    if read_scene_placements(&previous, &relative)?.placements == draft.placements.placements {
        return Ok((false, None));
    }
    let glb = write_scene_placements_glb(&draft.placements, &relative, Some(&previous))?;
    if glb == previous {
        return Ok((false, None));
    }
    let digest = bytes_hash(&glb);
    writes.push(PreparedWrite::new(path, glb));
    Ok((true, Some(digest)))
}

fn saved_placements_match(path: &std::path::Path, snapshot: &ScenePlacementsAsset) -> bool {
    std::fs::read(path)
        .ok()
        .is_some_and(|bytes| saved_placement_bytes_match(&bytes, snapshot)) // 文件读取只发生在有界IO保存任务内。
}

/// JSON排版和默认字段省略不属于作者内容变化；损坏字节则继续沿旧行为完整覆盖。
fn saved_placement_bytes_match(bytes: &[u8], snapshot: &ScenePlacementsAsset) -> bool {
    serde_json::from_slice::<ScenePlacementsAsset>(bytes)
        .ok()
        .is_some_and(|saved| placement_documents_equal(&saved, snapshot))
}

/// 反序列化后重新规范化比较，避免缩进、字段顺序或默认空值制造伪地表热更新。
fn saved_surface_bytes_match(bytes: &[u8], snapshot: &SceneSurfaceAsset) -> bool {
    let Ok(saved) = serde_json::from_slice::<SceneSurfaceAsset>(bytes) else {
        return false; // 损坏正式定义不能被当作无变化，下一批必须覆盖并发布修复值。
    };
    match (serde_json::to_value(saved), serde_json::to_value(snapshot)) {
        (Ok(saved), Ok(snapshot)) => saved == snapshot, // 结构化Value比较全部schema字段而不依赖JSON排版。
        _ => false, // 非有限浮点等不可序列化定义必须继续进入正式校验/重写失败路径。
    }
}

/// 水JSON语义比较只在保存IO任务内执行；清空末层与样条参数变化也会形成真实发布回执。
fn saved_water_bytes_match(bytes: &[u8], snapshot: &feathrise_scene_content::SceneWaterAsset) -> bool {
    let Ok(saved) = serde_json::from_slice::<feathrise_scene_content::SceneWaterAsset>(bytes) else { return false; };
    match (serde_json::to_value(saved), serde_json::to_value(snapshot)) {
        (Ok(saved), Ok(snapshot)) => saved == snapshot,
        _ => false,
    }
}

/// 从磁盘语义快照和RGBA脏标记生成两个互不冒充的地表写入/发布决定。
fn surface_write_changes(
    saved_bytes: Option<&[u8]>,
    snapshot: &SceneSurfaceAsset,
    weights_dirty: bool,
) -> (bool, bool) {
    let definition_changed =
        !saved_bytes.is_some_and(|bytes| saved_surface_bytes_match(bytes, snapshot)); // 缺失或损坏JSON必须完整重写。
    (definition_changed, weights_dirty) // 高度保存不会再借定义JSON或权重KTX2制造伪热更新。
}

/// 模型样本分块和原有二维R8进入同一保存事务，引用文档最后提交，失败仍完整回滚。
fn prepare_vegetation_writes<'a>(
    root: &Path,
    folder: &str,
    source: &super::super::terrain_documents::root_vegetation::VegetationSource<'_>,
    writes: &mut Vec<PreparedWrite<'a>>,
) -> Result<(), String> {
    let Some(vegetation) = source.vegetation else { return Ok(()); };
    if let Some(authoring) = &vegetation.authoring {
        authoring.validate(&vegetation.layers)?;
        for layer in &authoring.layers {
            if !source.author_dirty_layers.contains(&layer.id) { continue; }
            let density = source.author_densities.get(&layer.id).cloned()
                .ok_or_else(|| format!("植被作者层{}缺少R8画布", layer.id))?;
            validate_grid_len(&format!("植被作者层{}的R8", layer.id), density.len(), layer.resolution, 1)?;
            writes.push(PreparedWrite::shared(safe_asset_path(root, &layer.density_source_path)?, density));
        }
    }
    prepare_vegetation_layer_writes(root, source, vegetation, writes)?;
    let (metadata, blocks) = feathrise_scene_content::prepare_model_vegetation_blocks(folder, vegetation)?;
    for (path, bytes) in blocks {
        writes.push(PreparedWrite::new(super::super::files::safe_scene_private_asset_path(root, folder, &path)?, bytes));
    }
    let path = root.join(folder).join(SCENE_VEGETATION_FILE_NAME);
    let bytes = super::transaction::encode_json_bytes(&path, &metadata)?;
    writes.push(PreparedWrite::new(path, bytes));
    Ok(())
}

#[cfg(test)]
#[path = "model_vegetation_tests.rs"]
mod model_vegetation_tests;

fn prepare_vegetation_layer_writes<'a>(
    root: &std::path::Path,
    source: &super::super::terrain_documents::root_vegetation::VegetationSource<'_>,
    vegetation: &feathrise_scene_content::SceneVegetationAsset,
    writes: &mut Vec<PreparedWrite<'a>>,
) -> Result<(), String> {
    for layer in &vegetation.layers {
        if !source.dirty_layers.contains(&layer.id) {
            continue;
        }
        let density = source
            .densities
            .get(&layer.id)
            .cloned()
            .ok_or_else(|| format!("植被层{}缺少R8作者源", layer.id))?;
        validate_grid_len(
            &format!("植被层{}的R8", layer.id),
            density.len(),
            layer.resolution,
            1,
        )?;
        let source_path = safe_asset_path(root, &layer.density_source_path)?;
        let texture_path = safe_asset_path(root, &layer.density_texture_path)?;
        let resolution = layer.resolution;
        if is_png_path(&source_path) {
            let png_density = density.clone();
            writes.push(PreparedWrite::deferred(source_path.clone(), move || {
                encode_density_png(png_density.as_slice(), resolution)
            })); // PNG内部RGB工作区与编码结果只存活到当前stage写完。
        } else {
            writes.push(PreparedWrite::shared(source_path.clone(), density.clone()));
        }
        if texture_path != source_path {
            writes.push(PreparedWrite::deferred(texture_path, move || {
                encode_r8(density.as_slice(), resolution)
            })); // HN-01等source==texture不重复写；正式KTX2也只逐项驻留。
        }
    }
    Ok(())
}

fn validate_grid_len(
    label: &str,
    actual_len: usize,
    resolution: [u32; 2],
    channels: usize,
) -> Result<(), String> {
    let expected_len = usize::try_from(resolution[0])
        .ok()
        .and_then(|width| {
            usize::try_from(resolution[1])
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(channels))
        .ok_or_else(|| format!("{label}尺寸超出平台地址空间"))?;
    if actual_len != expected_len {
        return Err(format!(
            "{label}为{actual_len}个元素，声明尺寸需要{expected_len}个"
        )); // 所有长度错误都在首个stage前拒绝，避免无意义的磁盘临时文件。
    }
    Ok(())
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests; // 保存纯预检与语义写入回归独立维护，生产批次文件保持职责紧凑。
