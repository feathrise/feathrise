//! 场景作者草稿后台保存与热加载发布。

use bevy::ecs::system::SystemParam; // 保存完成发布参数合并后仍保持清晰的只读/可写边界。
use bevy::prelude::*; // Commands、ECS资源与日志。
use bevy::tasks::futures::check_ready; // Update只非阻塞探测结果。
#[cfg(test)]
use feathrise_scene_content::SceneId; // 保存预检与缓存发布单测构造稳定场景身份。
use feathrise_scene_content::{
    ActiveDynamicScene, SCENE_BOUNDARIES_FILE_NAME, SCENE_CATALOG_ASSET_PATH, SCENE_MANIFEST_FILE_NAME,
    SCENE_PORTALS_FILE_NAME, SCENE_VEGETATION_FILE_NAME, SceneContentRegistry,
    SceneManifestAsset, ScenePlacementsAsset, SceneSurfaceAsset, SceneVegetationAsset, SceneWaterAsset,
    WORLD_ENVIRONMENT_ASSET_PATH, WorldEnvironmentRegistry,
}; // 保存成功后显式重载稳定正式资产路径。
use feathrise_scene::{
    DynamicWaterRebuildRequest, DynamicWaterStatus,
    ScenePlacementRebuildRequest, ScenePlacementRuntimeStatus,
}; // 保存地形、水域与摆放只接受公开正式运行时完成证据。

use super::resources::{
    DraftSaveTask, EditorAuthoringSession, GlobalEnvironmentAuthoringState,
};
use super::terrain_external::TerrainExternalChangeState; // SHA冲突转为可见“采用/覆盖”模态状态。
use super::water_handoff::WaterFileHandoff; // 保存完成路径只编排正式交接与分帧预览回收。
use crate::resources::SceneEditorState;

mod batch; // 所有JSON、作者源、派生资产与目录先在内存组成一个完成批次。
mod terrain_documents; // 多地形画布投影与清单最后发布，共用同一次回滚事务。
mod blender_executable; // 绑骨仍复用有界工具发现；不再关联已退休的高度地形保存转换。
pub(crate) use blender_executable::find_blender_executable;
mod domain_files; // 外部文档版本核对与流式写项，不解释或复制正文。
mod global; // 仅全局配置脏时绕过全部场景预检的单文件原子保存。
mod handoff_helpers; // 水域文件事件代次基线保持独立小型纯辅助。
mod output_paths; // 水输出与场景全部其他权威/引用资产在首写入前统一检查碰撞。
mod placement_handoff; // 摆放内容变化检测后的正式发布、严格新修订门和原样失败重建。
mod portal_handoff; // 传送图立即发布并在正式新Ready修订前保留作者预览。
mod preflight; // 首次文件写入前完成水网、路径、R8与运行时网格预算的纯预检。
mod publish; // 保存完成后的小型JSON快照通过唯一Asset句柄立即交还正式热更新链路。
mod rollback; // 失败事务只为确实受影响的正式水输入建立旧版本缓存恢复门。
mod start; // 保存请求门与单一IO任务启动独立于完成采用路径。
mod surface_publish; // 地表定义与权重只按后台真实变化证据定向发布。
pub(crate) mod transaction; // 场景与全局派生目录共享跨文件stage、原子发布和失败逆序回滚。
mod write_helpers; // 编码与首次可选目录快照等纯辅助逻辑独立维护。
mod volume; // 私有体积源与网格同文件，加入现有事务而不另建落盘路径。

pub(crate) use batch::{DraftSaveError, DraftSaveSuccess}; // 保存成功携带真实摆放变化，失败携带必须纠正缓存的回滚路径。
use batch::save_global_environment; // 场景联合或全局单文件都只能通过整批回滚事务返回成功。
pub(crate) use global::{poll_global_environment_save, start_global_environment_save}; // 顶部保存按钮先尝试无场景预检的全局专用路径。
use handoff_helpers::water_handoff_generation_baseline;
use placement_handoff::{
    begin_placement_handoff, execute_placement_publish, prepare_placement_publish,
    schedule_unclaimed_placement_previews,
}; // 主完成系统只编排既有保存事务与摆放发布门。
pub(crate) use portal_handoff::{PortalPreviewHandoff, cleanup_saved_portal_previews}; // 插件初始化单批交接资源并注册有界清理系统。
use portal_handoff::{PortalSavePublish, publish_saved_portals_asset}; // 一个SystemParam收拢传送正式发布与预览交接资源。
#[cfg(test)]
use preflight::prepare_water_files; // 保存预检回归直接验证零文件写入契约。
use publish::{
    publish_saved_manifest_asset, publish_saved_vegetation_asset, publish_saved_water_asset,
}; // 场景清单、植被与混合水网共享唯一正式句柄发布策略。
use rollback::select_rollback_water_inputs; // 以IO启动快照筛选manifest/water/R8及兼容高度源恢复身份。
#[cfg(test)]
pub(crate) use start::{PlacementWaterSaveReadiness, placement_water_save_readiness};
pub(crate) use start::{
    observe_added_placement_water_status, observe_removed_placement_water_status,
    observe_replaced_placement_water_status, register_placement_water_evidence_root,
    start_draft_save, unregister_placement_water_evidence_root,
}; // 保存入口与证据生命周期observer从同一功能域导出，稳定帧不靠全量Query维护缓存。
use surface_publish::SavedSurfacePublish; // 地表发布输入在清脏前冻结，与Clipmap作者源共用本批保存证据。
#[cfg(test)]
use transaction::{PreparedWrite, commit_with_replace_hook}; // 纯文件故障注入验证无缺席原子替换与整批回滚。
#[cfg(test)]
use write_helpers::{encode_heights, resolve_water_json_output_path}; // 保持既有保存辅助测试入口。

/// 联合保存新增的全局状态与既有 AssetServer 合并成一个系统参数，避免轮询系统越过 Bevy 元组参数上限。
#[derive(SystemParam)]
pub(crate) struct SaveEnvironmentPublish<'w> {
    authoring: ResMut<'w, GlobalEnvironmentAuthoringState>,
    registry: Option<ResMut<'w, WorldEnvironmentRegistry>>,
    manifests: ResMut<'w, Assets<SceneManifestAsset>>, // 场景环境清脏前准备同帧正式清单接管。
    asset_server: Res<'w, AssetServer>,
    puddles: Option<ResMut<'w, feathrise_scene_content::ScenePuddleSaveHandoff>>, // 成功保存后冻结精确R8内容证明。
    surface_author: Option<ResMut<'w, feathrise_scene::DynamicSurfaceAuthorPreview>>, // 成功值显式发布前武装地表交接门。
    scene_preview: Option<ResMut<'w, feathrise_scene_content::SceneAuthorPreview>>, // 文件成功后由正式资产接回临时场景输入。
}

/// 只在票据和作者修订仍匹配时清理脏状态；失败保留完整草稿与预览。
pub(crate) fn poll_draft_save(
    mut save: ResMut<DraftSaveTask>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mut environment_publish: SaveEnvironmentPublish, // 联合事务成功后才发布全局值，并显式重载稳定正式资产路径。
    registry: Option<Res<SceneContentRegistry>>, // 已登记句柄允许保存结果在主线程立即替换摆放、植被和水域缓存。
    mut placements: ResMut<Assets<ScenePlacementsAsset>>, // 网格赋水保存后立即触发正式显式实例候选重建。
    mut portal_publish: PortalSavePublish, // 传送资产、正式修订和预览门合并后保持Bevy参数预算。
    mut surfaces: ResMut<Assets<SceneSurfaceAsset>>, // 地表定义真实变化时只替换目录持有的唯一正式句柄。
    mut vegetations: ResMut<Assets<SceneVegetationAsset>>, // 切换场景前同步作者结构并产生唯一Modified事件。
    mut waters: ResMut<Assets<SceneWaterAsset>>, // 静水、河流和瀑布同样立即替换唯一正式缓存。
    mut runtime_water: ParamSet<(
        Res<ActiveDynamicScene>,
        Res<DynamicWaterStatus>,
        MessageWriter<DynamicWaterRebuildRequest>,
        Res<ScenePlacementRuntimeStatus>,
        MessageWriter<ScenePlacementRebuildRequest>,
        ResMut<TerrainExternalChangeState>,
    )>, // 同一参数集冻结两类正式证据，并在全部发布动作之前写对应重建请求。
    mut water_handoff: ResMut<WaterFileHandoff>, // 在发布水JSON前先持有本次脏R8作者源。
    real_time: Res<Time<Real>>,                // 文件交接截止使用F12暂停期间仍推进的真实时间。
    mut authoring_retirement: Option<ResMut<super::AuthoringRetirement>>, // 完整作者历史由唯一后台回收，不在保存完成帧析构整图。
    mut object_documents: Option<ResMut<super::terrain_documents::TerrainDocumentTask>>,
) {
    let asset_server = &environment_publish.asset_server;
    if water_handoff.placement_preview_cleanup_pending() {
        return; // 上一精确实体批次按64项/帧释放完毕前，不消费可能已就绪的新IO结果。
    }
    let Some(task) = save.task.as_mut() else {
        return;
    };
    let Some((ticket, revision, result)) = check_ready(task) else {
        return;
    };
    let mut water_handoff_snapshot = save.water_handoff_snapshot.take(); // 成功发布与失败回滚都只消费IO启动前冻结的正式水域身份。
    let mut placement_handoff_snapshot = save.placement_handoff_snapshot.take(); // 正式门或分帧清理只会取得一次精确预览批次。
    let global_environment_snapshot = save.global_environment_snapshot.take(); // 联合成功、失败或过期结果都只消费一次冻结快照。
    save.task = None;
    if ticket != save.ticket {
        session.saving = false; // 过期任务不再拥有当前保存事务。
        editor
            .cancel_close_request("场景保存结果已经过期，本次关闭已取消；请确认当前场景后重新保存"); // 过期结果不能作为安全关闭的成功证据。
        editor.record_status_result(crate::types::EditorStatusKind::Warning);
        return;
    }
    let global_revision_matches =
        global_environment_snapshot
            .as_ref()
            .is_none_or(|(global_revision, _)| {
                *global_revision == environment_publish.authoring.revision
            }); // 可选全局快照必须仍属于当前作者分支。
    match result {
        Ok(success) if revision == session.revision && global_revision_matches => {
            let mut terrain_water_changed = success.terrain_water_changed;
            let root_water_changed = success.root_water_changed;
            let root_water_retry = water_handoff.pending_for(&success.scene_id, session.water_revision, session.terrain_revision);
            if let Some(draft) = &mut session.draft {
                if let Some(manifest) = success.terrain_manifest { draft.manifest = manifest; }
                for id in success.terrain_saved {
                    draft.terrain_documents.dirty_stored.remove(&id);
                    draft.terrain_documents.saved.insert(id);
                }
                draft.terrain_documents.active_dirty = false;
                draft.terrain_documents.paint_groups_dirty = false;
                for (id, digest) in success.terrain_digests {
                    draft.terrain_documents.saved_digests.insert(id.clone(), digest);
                    if draft.terrain_documents.active.as_ref().is_some_and(|active| active.definition.id == id) {
                        draft.terrain_source_sha256 = Some(digest);
                        draft.terrain_overwrite_authorized_sha256 = None;
                    }
                }
            }
            if let Some(documents) = object_documents.as_deref_mut() { documents.reloads.extend(success.terrain_reload_paths); }
            debug_assert_eq!(
                success.global_environment_saved,
                global_environment_snapshot.is_some(),
                "保存批次返回的全局写入证据必须与启动时冻结的全局快照一致"
            );
            let domain_hash = success.domain_hash; // 外部作者文件只按本批成功证据清脏。
            let placements_changed = success.placements_changed; // 只有本批语义内容不同才允许正式发布。
            let surface_definition_changed = success.surface_definition_changed; // 只有JSON真正进入事务才发布正式定义。
            let surface_weights_changed = success.surface_weights_changed; // 只有RGBA/KTX2真正进入事务才重载权重纹理。
            let surface_handoff_changes = success.active_terrain_surface_changes.unwrap_or((surface_definition_changed, surface_weights_changed)); // 独立网格按自己写盘的回执建门，不能借空容器的false提前归还文件。
            let placement_glb_hash = success.placement_glb_hash;
            let scene_id = success.scene_id; // 保存身份在下方所有显式reload与交接门中保持一致。
            let mut catalog_registration_changed = false; // 清脏前冻结是否需要显式刷新全局目录。
            let mut water_catalog_registration_required = false; // 当前水交接失败时必须恢复首次water_path登记责任。
            let mut boundary_asset_path = None; // 清脏前冻结当前场景私有边界AssetServer路径。
            let mut manifest_asset_path = None; // 环境草稿归还玩法相机前显式刷新正式清单。
            let mut manifest_snapshot = None; // 与落盘事务同源的完整清单立即发布到唯一正式句柄。
            let mut placements_asset_path = None; // 没有可写现有句柄时使用保存后的摆放文件兜底重载。
            let mut placements_snapshot = None; // 保存成功的实例与整网格水体覆盖立即发布到正式句柄。
            let mut portals_asset_path = None; // 正式句柄尚未Ready时显式重载刚落盘的传送图。
            let mut portals_snapshot = None; // 保存快照必须在回收作者预览前进入正式唯一句柄。
            let mut surface_publish = SavedSurfacePublish::default(); // 空值表示高度等无关保存严格不发布地表资产。
            let mut vegetation_asset_path = None; // 没有可写现有句柄时使用保存后的文件路径兜底重载。
            let mut vegetation_snapshot = None; // 保存成功修订的完整植被JSON直接替换内存旧缓存。
            let mut water_asset_path = None; // water_path尚未形成强句柄时使用刚写完的文件路径兜底重载。
            let mut water_snapshot = None; // 保存成功的静水、河流与瀑布完整JSON立即发布到现有句柄。
            let mut dirty_water_layer_ids = Vec::new(); // 交接失败时恢复原脏层，普通重试会再次写入raw R8/KTX2。
            let mut dirty_water_mask_paths = Vec::new(); // 清脏前冻结本次真正改写的权威R8作者源。
            let terrain_revision = session.terrain_revision; // 水域交接仍须绑定同次Clipmap高度修订，不能借用旧河床。
            let water_revision = session.water_revision; // 水交接绑定保存成功时的作者修订，后续笔划会作废旧门。
            if let Some(draft) = session.draft.as_mut() {
                let root_water = super::terrain_documents::root_water::source(draft);
                catalog_registration_changed = (draft.domain.as_ref().is_some_and(|domain| domain.registration_required())
                    && domain_hash.is_some())
                    || super::terrain_documents::root_vegetation::source(draft)
                        .is_some_and(|source| source.registration_source_hash.is_some())
                    || root_water.registration_required
                    || draft.boundary_catalog_registration_required; // 两类新入口都通过同一目录原子替换发布。
                water_catalog_registration_required = root_water.registration_required; // 清脏前冻结真正根水的登记责任。
                boundary_asset_path = draft.boundaries.as_ref().map(|_| {
                    format!(
                        "{}/{}",
                        draft.folder.trim_end_matches('/'),
                        SCENE_BOUNDARIES_FILE_NAME
                    )
                }); // 只为实际存在边界文档的场景请求重载。
                manifest_asset_path = Some(format!(
                    "{}/{}",
                    draft.folder.trim_end_matches('/'),
                    SCENE_MANIFEST_FILE_NAME
                )); // 保存总会原子写入完整清单，显式重载避免文件监听合并延迟玩法天空交接。
                manifest_snapshot = Some(draft.manifest.clone()); // 清理环境脏位前冻结本次成功保存的完整正式值。
                if let Some(hash) = placement_glb_hash {
                    draft.placement_glb_hash = Some(hash); // 只采用本次事务确认落盘的精确版本。
                }
                placements_asset_path = Some(draft.placements_path());
                placements_snapshot = Some(draft.placements.clone()); // 小型记录和网格水体覆盖与本次磁盘事务完全同源。
                portals_asset_path = Some(format!(
                    "{}/{}",
                    draft.folder.trim_end_matches('/'), // 复用目录登记的场景私有资产根。
                    SCENE_PORTALS_FILE_NAME, // 兜底重载必须命中刚完成原子写入的正式传送文件。
                ));
                portals_snapshot = Some(draft.portals.clone()); // 小型传送图与磁盘事务完全同源，主线程只克隆一次。
                surface_publish = SavedSurfacePublish::from_draft(
                    draft,
                    surface_definition_changed,
                    surface_weights_changed,
                ); // 定义与权重发布输入只由后台真实写入证据冻结。
                if let Some(vegetation) = super::terrain_documents::root_vegetation::source(draft)
                    .and_then(|source| source.vegetation) {
                    vegetation_asset_path = Some(format!(
                        "{}/{}",
                        draft.folder.trim_end_matches('/'),
                        SCENE_VEGETATION_FILE_NAME
                    )); // 资产尚未加载时仍能从刚写完的权威文件重新请求。
                    vegetation_snapshot = Some(vegetation.clone()); // 小型JSON描述克隆一次，不复制Arc密度整图。
                }
                let root_water = super::terrain_documents::root_water::source(draft);
                if (root_water_changed || root_water_retry) && let Some(water) = root_water.water {
                    water_asset_path = Some(root_water.asset_path.to_owned()); // 保存、交接和重载始终使用根目录实际声明路径。
                    water_snapshot = Some(water.clone()); // 小型网络描述克隆一次，R8遮罩继续留在独立Arc表。
                    for layer in &water.layers {
                        if root_water.dirty_layers.contains(&layer.id)
                            && !dirty_water_mask_paths.contains(&layer.mask_source_path)
                        {
                            dirty_water_layer_ids.push(layer.id.clone()); // 正式schema保证层ID与独占路径一一对应。
                            dirty_water_mask_paths.push(layer.mask_source_path.clone()); // 同一路径只持有并重载一次，严格沿用本批脏层身份。
                        }
                    }
                }
                draft.terrain_dirty = false;
                if draft.puddles_dirty {
                    // 本次独立raw与KTX2已完成同事务落盘。
                    if let Some(handoff) = environment_publish.puddles.as_mut() {
                        // 正式插件提供共享交接资源。
                        handoff.revision = handoff.revision.wrapping_add(1); // 旧比较任务立即失效。
                        handoff.scene_id = draft
                            .manifest
                            .puddles
                            .as_ref()
                            .map(|_| draft.scene_id.clone()); // 撤销到空层无需等文件。
                        handoff.texture_path = draft
                            .manifest
                            .puddles
                            .as_ref()
                            .map(|layer| layer.texture_path.clone()); // 同一保存路径。
                        handoff.expected_mask = draft.puddle_mask.clone(); // 仅共享原始R8，不复制整图。
                    } // 完成内容证明登记。
                    if let Some(layer) = draft.manifest.puddles.as_ref() {
                        // 交接门建立后才唤醒正式加载。
                        asset_server.reload(layer.texture_path.clone()); // 不依赖文件监听时序。
                    } // 清单无积水时不加载旧文件。
                    draft.puddles_dirty = false; // IO失败分支保留脏责任与原画面。
                } // 画面交接由正式场景的后台精确R8比对完成。
                draft.surface_dirty = false;
                if draft.manifest.terrains.is_some() { draft.puddles_dirty = false; }
                draft.vegetation_dirty_layers.clear();
                draft.vegetation_author_dirty_layers.clear(); // 独立作者R8与正式派生已同事务完整落盘。
                draft.water_dirty_layers.clear();
                draft.environment_dirty = false; // 当前请求继续显示同一画面，但后续变更重新服从正式AssetServer清单。
                draft.vegetation_registration_source_hash = None; // 只有整批事务成功后才结清首次植被登记责任。
                draft.water_catalog_registration_required = false;
                draft.water_registration_source_hash = None; // 水JSON与目录同批成功后才解除并发基线责任。
                super::terrain_documents::root_water::clear_saved(draft);
                super::terrain_documents::root_vegetation::clear_saved(draft);
                if let Some(hash) = domain_hash && let Some(domain) = draft.domain.as_mut() {
                    domain.value = (domain.ops.saved)(&*domain.value, &hash);
                    asset_server.reload(domain.asset_path().to_owned());
                } // 内容hash不变时，各领域运行时保留已采用内容。
                draft.boundary_catalog_registration_required = false; // 文件和目录已经完整落盘，下一帧归还AssetServer权威。
                draft.boundaries_dirty = false; // F12内存覆盖可以在文件加载期间保留上一完整根并随后清除。
                // 不把opening baseline替换成当前密度：本进程内冻结树位仍是打开时版本，后续笔划必须
                // 继续以同一基线区分“旧内容原样保留”和“作者已改像素”；下次启动会自然载入新二者。
            }
            let saved_portal_previews = std::mem::take(&mut session.portal_previews); // 预览所有权移交后不再属于后续撤销栈。
            session.clear_history(authoring_retirement.as_deref_mut()); // 清栈与对象归属游标同帧换代，大历史仍交后台释放。
            if let Some((_global_revision, snapshot)) = global_environment_snapshot.as_ref() {
                environment_publish.authoring.saved = snapshot.clone(); // 同一事务成功值成为新的全局保存基线。
                environment_publish.authoring.draft = snapshot.clone(); // 保存期间作者输入冻结，明确保留同一快照。
                environment_publish.authoring.dirty = false; // 场景和全局全部目标成功后才清理。
                environment_publish.authoring.load_error = None; // 联合保存也可修复缺失或损坏的启动文件。
                if let Some(registry) = environment_publish.registry.as_deref_mut() {
                    registry.celestial_lighting = snapshot.celestial_lighting.clone().normalized(); // 动态场景同帧采用落盘配置。
                    registry.last_error = None; // 清除旧加载错误。
                    registry.revision = registry.revision.wrapping_add(1); // 最终场景覆盖重新解析。
                    environment_publish.authoring.source_revision = registry.revision; // 后续同值资产事件不覆盖作者基线。
                }
                asset_server.reload(WORLD_ENVIRONMENT_ASSET_PATH); // 显式交还 AssetServer 文件权威。
            }
            editor.dirty = environment_publish.authoring.dirty; // 场景脏位已全部清理，可选全局成功后通常同样归零。
            if let Some(preview) = environment_publish.scene_preview.as_mut()
                && preview.for_scene(&scene_id).is_some()
            {
                preview.active = None;
            } // 临时文档不得盖住随后发布的已保存摆放和传送资产。
            let active_scene_id = runtime_water.p0().scene_id.clone(); // 保存完成时已经切走的场景只更新磁盘，不阻塞当前F12。
            let current_water_generation = runtime_water.p1().desired_generation; // IO期间的部分文件热更不能冒充最终完成值。
            let (
                placement_preparation,
                current_placement_revision,
                current_placement_attempt_revision,
                current_placement_progress_revision,
            ) = {
                let status = runtime_water.p3(); // 短借用后释放，稍后才能写显式重建消息。
                (
                    prepare_placement_publish(
                        registry.as_deref(),                 // 只读检查当前场景唯一正式句柄。
                        &scene_id,                             // 禁止比较到另一场景缓存。
                        placements_snapshot.as_ref(),        // 本次已经落盘的完整作者快照。
                        &placements,                         // 只读计划不会产生伪Modified。
                        placements_changed,                  // 后台事务返回的真实磁盘语义变化。
                        placement_handoff_snapshot.as_ref(), // IO启动时冻结失败重试责任。
                    ),
                    status.revision, // 建门时再次冻结，排除IO期间完成的旧候选。
                    status.attempt_revision, // 新Failed也必须来自建门之后的新尝试。
                    status.progress_revision, // 只允许建门后的真实运行时推进刷新停滞截止。
                )
            };
            let mut scene_handoff_start_error = None; // 文件仍可发布，但建立失败必须恢复dirty并取消关闭。
            let water_handoff_started = water_handoff_snapshot.as_mut().is_some_and(|snapshot| {
                if snapshot.scene_id != scene_id
                    || snapshot.water_revision != water_revision
                    || snapshot.terrain_revision != terrain_revision
                    || active_scene_id.as_ref() != Some(&scene_id)
                {
                    return false; // 保存期间切换场景或换水修订时绝不让旧事务阻止当前作者会话。
                }
                let private_required = !snapshot.terrains.is_empty()
                    && (!terrain_water_changed.is_empty() || water_handoff.terrain_water_retry_pending_for(&scene_id));
                let root_required = water_snapshot.is_some() && water_asset_path.is_some();
                if !root_required {
                    if !private_required { return false; }
                    return match water_handoff.begin_terrains(&asset_server, scene_id.clone(), water_revision,
                        terrain_revision, &snapshot.manifest_asset_path, &mut snapshot.terrains,
                        &mut terrain_water_changed, None, real_time.elapsed()) {
                        Ok(()) => true,
                        Err(error) => { scene_handoff_start_error = Some(error); false }
                    };
                }
                let baseline_generation = water_handoff_generation_baseline(
                    snapshot.baseline_generation,
                    current_water_generation,
                ); // IO期间清单、R8或分页高度源可能已推进代次；最终保存批次仍须严格再前进一代。
                let mut layer_ids = Vec::with_capacity(snapshot.dirty_layers.len());
                let mut mask_paths = Vec::with_capacity(snapshot.dirty_layers.len());
                let mut texture_paths = Vec::with_capacity(snapshot.dirty_layers.len());
                for (layer_id, mask_path, texture_path) in &snapshot.dirty_layers {
                    layer_ids.push(layer_id.clone());
                    mask_paths.push(mask_path.clone());
                    texture_paths.push(texture_path.clone());
                } // 只使用IO启动时冻结的本批raw R8与KTX2身份。
                match water_handoff.begin(
                    &asset_server,
                    scene_id.clone(),
                    snapshot.root_water_revision,
                    terrain_revision,
                    baseline_generation,
                    &snapshot.manifest_asset_path,
                    &snapshot.water_asset_path,
                    &layer_ids,
                    &mask_paths,
                    &texture_paths,
                    snapshot.dirty_height_path.as_deref(),
                    snapshot.catalog_registration_required,
                    real_time.elapsed(),
                ) {
                    Ok(()) => {
                        if private_required && let Err(error) = water_handoff.attach_terrain_batch(
                            &scene_id, water_revision, &snapshot.manifest_asset_path, &mut snapshot.terrains,
                            &mut terrain_water_changed, None) {
                            scene_handoff_start_error = Some(error);
                            return false;
                        }
                        runtime_water.p2().write(DynamicWaterRebuildRequest {
                            scene_id: scene_id.clone(),
                        }); // 强制新代次必须先于下面任何显式reload或JSON直接发布。
                        for path in &mask_paths {
                            asset_server.reload(path.clone()); // 门和代次请求都已存在，现在才发布本次raw R8。
                        }
                        for path in &texture_paths {
                            asset_server.reload(path.clone()); // Clipmap GPU遮罩显式重载，完成门会等待对应Image成功事件。
                        }
                        true
                    }
                    Err(error) => {
                        scene_handoff_start_error = Some(error);
                        false
                    }
                }
            }); // 必须早于下方water JSON直接发布或兜底reload，避免成功事件先于等待身份。
            let placement_handoff_started = if scene_handoff_start_error.is_none() {
                match begin_placement_handoff(
                    &mut water_handoff,                  // 与可能已建立的水域门组合成同一关闭责任。
                    &placement_preparation,              // 只有真实变化或失败重试需要建门。
                    placement_handoff_snapshot.as_mut(), // 门完整建立后才移动本批精确Scene0实体。
                    active_scene_id.as_ref(),              // 非当前玩法场景不阻止F12。
                    &scene_id,                             // 保存完成身份。
                    revision,                            // 保存对应全局作者修订。
                    current_placement_revision,          // 旧Ready不能冒充保存后正式采用。
                    current_placement_attempt_revision,  // 旧Failed同样不能冒充新发布结果。
                    current_placement_progress_revision, // IO期间旧进度不能给新门续命。
                    real_time.elapsed(),                 // 摆放独立三十秒真实截止起点。
                ) {
                    Ok(started) => started,
                    Err(error) => {
                        scene_handoff_start_error = Some(error);
                        false
                    }
                }
            } else {
                false // 水域门已经失败时不能继续建立半套摆放责任。
            }; // 必须早于AssetMut、reload或显式重建请求，避免新状态先于等待身份。
            if !placement_handoff_started && scene_handoff_start_error.is_none() {
                // 只有无错误的合法无门路径可回收；任一建门失败均保留整批预览供修复重试。
                schedule_unclaimed_placement_previews(
                    &mut water_handoff,
                    placement_handoff_snapshot.as_mut(),
                )
                .expect("保存结果采用前已等待上一预览清理完成"); // 非当前/无门路径也统一按64项/帧精确回收。
            } // 当前场景真正建立交接时，精确批次实体继续可见并为失败重试保留真实Scene0验证状态。
            let portals_snapshot = portals_snapshot
                .as_ref()
                .expect("有效保存结果必须仍持有同修订传送图快照"); // IO启动前已经冻结完整草稿，匹配修订不得缺失。
            let portal_baseline_revision = portal_publish.runtime.revision; // 短借用冻结发布前正式修订。
            portal_publish.handoff.begin(
                scene_id.clone(),                 // 交接只接受本次保存场景的新正式状态。
                portal_baseline_revision,       // 发布前已有Ready修订不能冒充新图接替预览。
                portals_snapshot.portals.len(), // 正式采用点数必须与保存快照完整一致。
                saved_portal_previews,          // 新增点预览在正式Ready前继续可见。
            ); // 必须早于AssetMut或reload，避免新事件先于预览交接身份。
            if !publish_saved_portals_asset(
                registry.as_deref(),        // 使用目录已经持有的唯一正式传送句柄。
                &scene_id,                    // 禁止把保存快照发布到另一个场景。
                portals_snapshot,           // 与磁盘原子写入完全相同的完整传送图。
                &mut portal_publish.assets, // 主线程替换并产生唯一Modified事件。
            ) && let Some(path) = portals_asset_path
            {
                asset_server.reload(path); // 句柄尚未就绪时显式请求正式传送图，不依赖Windows文件监听偶然命中。
            }
            if catalog_registration_changed {
                asset_server.reload(SCENE_CATALOG_ASSET_PATH); // 旧动态场景新增入口无需等待文件监听时序。
            }
            if let Some(path) = boundary_asset_path {
                asset_server.reload(path); // 保存后的边界文件显式进入正式热重载与原子替换路径。
            }
            if let Some(snapshot) = manifest_snapshot.as_ref()
                && !publish_saved_manifest_asset(
                    registry.as_deref(),                // 使用目录已经持有的唯一正式清单句柄。
                    &scene_id,                            // 防止保存结果串入另一张动态场景。
                    snapshot,                           // 与磁盘原子写入完全相同的完整场景清单。
                    &mut environment_publish.manifests, // 清脏同帧发布 Modified 并交还正式运行时。
                )
                && let Some(path) = manifest_asset_path
            {
                asset_server.reload(path); // 唯一句柄尚未就绪时才从刚落盘路径显式重载兜底。
            }
            if let Some(author) = environment_publish.surface_author.as_deref_mut() {
                author.begin_saved_handoff(
                    &scene_id,
                    session.surface_revision,
                    surface_handoff_changes.0,
                    surface_handoff_changes.1,
                    success.surface_source_identity,
                ); // 先武装再主动reload，早到文件监听不能吞掉本次门。
            }
            surface_publish.publish(registry.as_deref(), &scene_id, &mut surfaces, &asset_server); // 高度分页源由独立文档回执重载；地表只发布本批真实写入的定义和权重。
            {
                let mut placement_rebuild_requests = runtime_water.p4();
                execute_placement_publish(
                    &placement_preparation,           // 保存完成帧冻结的唯一发布动作。
                    placement_handoff_started,        // 当前场景必须先有可证明的新修订门。
                    active_scene_id.as_ref(),           // 非当前场景只更新正式缓存。
                    &scene_id,                          // 防止保存结果串入另一个场景。
                    placements_snapshot.as_ref(),     // 与磁盘JSON完全相同的完成值。
                    placements_asset_path.as_deref(), // 正式句柄失效时的显式reload兜底。
                    registry.as_deref(),              // 使用目录持有的唯一正式摆放句柄。
                    &mut placements,                  // 只有真实内容差异才取得AssetMut。
                    &asset_server,                    // reload同样只能发生在交接门已建立之后。
                    &mut placement_rebuild_requests,  // 原样失败重试不伪造Modified。
                );
            }
            if let Some(snapshot) = vegetation_snapshot.as_ref()
                && !publish_saved_vegetation_asset(
                    registry.as_deref(), // 使用目录已经持有的唯一正式句柄。
                    &scene_id,             // 防止保存结果串到其他动态场景。
                    snapshot,            // 与磁盘原子写入完全相同的作者图层结构。
                    &mut vegetations,    // 主线程一次替换并触发AssetEvent::Modified。
                )
                && let Some(path) = vegetation_asset_path
            {
                asset_server.reload(path); // 句柄尚未就绪时明确请求异步重载，不能继续依赖文件监听偶然命中。
            }
            if let Some(snapshot) = water_snapshot.as_ref()
                && !publish_saved_water_asset(
                    registry.as_deref(), // 使用目录已经持有的唯一正式水域句柄。
                    &scene_id,             // 防止保存结果串入另一张动态场景。
                    snapshot,            // 与磁盘JSON完全相同的混合水网完成值。
                    &mut waters,         // 主线程替换并产生唯一Modified事件。
                )
                && let Some(path) = water_asset_path
            {
                asset_server.reload(path); // 首次登记或加载间隙显式请求新水网JSON，避免依赖文件监听时序。
            }
            session.saving = false; // IO已经完成；文件交接另由明确关闭门保护，编辑器仍可继续工作。
            if let Some(error) = scene_handoff_start_error {
                if let Some(draft) = session.draft.as_mut() {
                    super::terrain_documents::root_water::restore_dirty(draft,
                        dirty_water_layer_ids.iter().cloned(), water_catalog_registration_required);
                }
                editor.dirty = true; // 磁盘已写入但正式水域交接身份不完整，保留显式重试入口。
                if placement_preparation.handoff_required {
                    water_handoff.mark_placement_unresolved(
                        scene_id.clone(),
                        format!("保存完成但无法建立正式场景交接：{error}"),
                    ); // 下一次同图保存即使placements字节未变，也会显式请求正式候选重建。
                } else {
                    water_handoff.mark_unresolved(
                        scene_id.clone(),
                        format!("保存完成但无法建立正式水域交接：{error}"),
                    ); // 已写入的正式批次在下一次完整保存接管前始终阻止同图关闭。
                }
                if let Some(snapshot) = water_handoff_snapshot.as_mut() && !snapshot.terrains.is_empty() {
                    water_handoff.remember_terrain_water_recovery(&snapshot.manifest_asset_path,
                        &mut snapshot.terrains, &mut terrain_water_changed);
                } // 建门失败也按本批冻结对象恢复，不能只把当前检查器的水层标脏。
                editor.cancel_close_with_error(format!(
                    "场景{scene_id}已保存，但无法建立正式场景交接：{error}；已进入不可关闭、可重试保存的安全状态"
                ));
                error!("{}", editor.status);
                return;
            }
            session.successful_save_sequence = session.successful_save_sequence.wrapping_add(1); // 文件事务及交接建门成功，切换场景续接才有正式回执。
            let save_prefix = format!("场景{scene_id}已保存");
            editor.status = match (water_handoff_started, placement_handoff_started) {
                (true, false) => {
                    format!("{save_prefix}；正在等待正式水域新代次原子采用")
                }
                (false, true) => {
                    format!("{save_prefix}；正在等待正式摆放新修订原子采用")
                }
                (true, true) => {
                    format!("{save_prefix}；正在等待正式水域与摆放共同完成原子热交接")
                }
                (false, false) => format!(
                    "{save_prefix}；环境 IBL、地表、植被、水域与区域边界正在热更新，当前会话继续显示正式预览"
                ),
            };
            info!("{}", editor.status);
            editor.record_status_result(crate::types::EditorStatusKind::Success); // 只在原同票据、同修订成功分支附加表现证据。
        }
        Ok(success) => {
            let scene_id = success.scene_id; // 旧作者修订已经落盘，但不得发布成当前会话正式完成证据。
            session.saving = false; // 旧修订已落盘但当前草稿仍标脏，可继续下一次保存。
            editor.cancel_close_request(format!(
                "场景{scene_id}旧修订已保存，但仍有更新修改待保存；本次关闭已取消"
            )); // 新修订尚未持久化，不能让旧任务完成后直接退出作者会话。
            editor.record_status_result(crate::types::EditorStatusKind::Warning);
        }
        Err(error) => {
            if let Some(digest) = error.terrain_conflict_sha256()
                && let Some(scene_id) = session.draft.as_ref().map(|draft| draft.scene_id.clone())
            {
                runtime_water.p5().register_save_conflict(scene_id, digest); // 正式文件未被覆盖，等待作者明确选择。
            }
            let rollback_paths = error.reload_asset_paths().to_vec(); // 后台事务已恢复磁盘，但文件监听可能仍缓存中间新版本。
            let mut reload_paths = rollback_paths.clone();
            let mut rollback_gate_started = false;
            let mut rollback_unresolved = false;
            if let Some(snapshot) = water_handoff_snapshot.as_mut()
                && runtime_water.p0().scene_id.as_ref() == Some(&snapshot.scene_id)
                && !rollback_paths.is_empty()
            {
                if !error.rollback_complete() {
                    water_handoff.mark_unresolved(
                        snapshot.scene_id.clone(),
                        "磁盘回滚未完整恢复全部正式目标".to_owned(),
                    ); // 混合磁盘版本不能通过任何AssetServer Ready证据放行。
                    rollback_unresolved = true;
                } else if let Some(inputs) = select_rollback_water_inputs(snapshot, &rollback_paths) {
                    let baseline_generation = water_handoff_generation_baseline(
                        snapshot.baseline_generation, runtime_water.p1().desired_generation);
                    let result = water_handoff.begin_rollback(&asset_server, snapshot.scene_id.clone(),
                        snapshot.root_water_revision, snapshot.terrain_revision, baseline_generation,
                        &inputs.manifest_path, &inputs.water_path, &inputs.mask_paths, &inputs.texture_paths,
                        inputs.height_path.as_deref(), real_time.elapsed())
                        .and_then(|()| {
                            if snapshot.terrains.is_empty() { return Ok(()); }
                            water_handoff.attach_terrain_batch(&snapshot.scene_id, snapshot.water_revision, &snapshot.manifest_asset_path,
                                &mut snapshot.terrains, &mut std::collections::BTreeSet::new(), Some(&rollback_paths))
                        });
                    match result {
                        Ok(()) => {
                            for required in [&inputs.manifest_path, &inputs.water_path] {
                                if !reload_paths.contains(required) { reload_paths.push(required.clone()); }
                            }
                            runtime_water.p2().write(DynamicWaterRebuildRequest { scene_id: snapshot.scene_id.clone() });
                            rollback_gate_started = true;
                        }
                        Err(gate_error) => {
                            water_handoff.mark_unresolved(snapshot.scene_id.clone(), format!("无法建立磁盘回滚版本重载门：{gate_error}"));
                            rollback_unresolved = true;
                        }
                    }
                } else if !snapshot.terrains.is_empty() {
                    match water_handoff.begin_terrains(&asset_server, snapshot.scene_id.clone(),
                        snapshot.water_revision, snapshot.terrain_revision, &snapshot.manifest_asset_path,
                        &mut snapshot.terrains, &mut std::collections::BTreeSet::new(),
                        Some(&rollback_paths), real_time.elapsed()) {
                        Ok(()) => rollback_gate_started = true,
                        Err(gate_error) => {
                            water_handoff.mark_unresolved(snapshot.scene_id.clone(), format!("无法建立独立水画布回滚恢复门：{gate_error}"));
                            rollback_unresolved = true;
                        }
                    }
                }
            }
            for path in &reload_paths {
                asset_server.reload(path.clone()); // 所有曾发布后回滚的目标及联合屏障输入都显式纠正缓存。
            }
            session.saving = false; // 保存失败保留草稿，但释放single-flight忙碌门允许修正后重试。
            editor.cancel_close_with_error(format!(
                "场景保存失败：{error}；本次关闭已取消{}",
                if rollback_gate_started {
                    "，正在确认磁盘旧版本已完整恢复到正式运行时"
                } else if rollback_unresolved {
                    "，正式文件状态尚未证明完整，请直接重试保存"
                } else {
                    "，可修正后重新保存"
                }
            )); // dirty始终保留；有回滚门或未解决态时后续F12也不能绕过正式缓存一致性。
            error!("{}", editor.status);
        }
    }
}

#[cfg(test)]
mod tests;
