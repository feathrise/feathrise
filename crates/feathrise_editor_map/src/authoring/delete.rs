//! 编辑器地图的保护校验、回收或永久删除事务与后台结果采用。

use std::path::{Path, PathBuf}; // 目录身份校验与同文件系统回收路径构造。
use std::time::{SystemTime, UNIX_EPOCH}; // 回收目录使用稳定可读的时间后缀避免碰撞。

use bevy::prelude::*; // ECS资源、状态、AssetServer与日志。
use bevy::tasks::IoTaskPool; // 目录扫描、JSON读写和文件夹移动全部离开帧关键路径。
use bevy::tasks::futures::check_ready; // 主线程只非阻塞探测单一删除任务。
use feathrise_map_content::{
    ActiveDynamicMap, MAP_CATALOG_ASSET_PATH, MAP_SCHEMA_VERSION, MapCatalogAsset, MapCatalogEntry,
    MapPortalsAsset,
}; // 删除严格复用正式目录、传送图和版本契约。
use feathrise_scene_core::state::ScenePhase; // 只有安全到达HN-01后才允许移动当前地图目录。

use super::files::{
    atomic_write, atomic_write_json, decode_map_catalog, safe_asset_path, workspace_asset_root,
}; // 目录更新沿用现有原子文件工具并限制在assets根内。
use crate::constants::MAP_FOLDER_ASSET_PATH; // 删除归属后显式刷新编辑器专用归类资产。
use crate::map_browser::{map_is_visible_in_tree, prepare_map_assignment_removal}; // 联合事务完成后只把当前树中真实可见的安全地图设为回退高亮。
use crate::map_identity::active_gameplay_map_id; // 后台回收只在确认计划指定的安全地图真正运行时启动。
use crate::resources::{EditorAssetCatalog, MapBrowserState, MapDeletionTask, MapEditorState}; // 删除状态机、虚拟目录与资产浏览刷新。
use crate::types::{DeletedMap, MapBrowserItem, MapDeletionMode, MapDeletionRequest}; // 后台只接收和返回轻量拥有数据。

#[cfg(test)]
const FALLBACK_MAP_ID: &str = "HN-01"; // 文件事务测试使用受保护的开封正式地图作为保留目录项。
const MAX_REFERENCE_SCAN_MAPS: usize = 4_096; // 删除前最多检查四千零九十六张地图，超限明确拒绝而不无界扫描。
const MAX_TRASH_NAME_ATTEMPTS: u32 = 10_000; // 同一毫秒回收目录碰撞具有固定最坏尝试次数。

/// 从正式目录入口构造受限删除请求；专用地图和自定义宽目录在弹框出现前即被拒绝。
pub(crate) fn map_deletion_request(entry: &MapCatalogEntry) -> Result<MapDeletionRequest, String> {
    let map_id = entry.id.as_str(); // 稳定ID决定保护规则和唯一私有目录。
    if !map_id.starts_with("MAP-") || map_id.len() <= "MAP-".len() {
        return Err(format!(
            "地图{}受保护；只允许删除由编辑器创建的 MAP-* 地图",
            entry.id
        )); // HN-01、HN-02和其他正式地图永远不能从F12删除。
    }
    let expected_folder = format!("maps/{}", entry.id); // 创建器唯一允许的可删除私有目录形状。
    if entry.folder.trim_end_matches('/') != expected_folder {
        return Err(format!(
            "地图{}目录不是编辑器私有目录{}，已拒绝删除",
            entry.id, expected_folder
        )); // 禁止移动共享目录、别名目录或比单图更宽的路径。
    }
    Ok(MapDeletionRequest {
        map_id: entry.id.clone(),                 // 冻结作者确认时的稳定身份。
        display_name: entry.display_name.clone(), // 后台目录热更后仍能回显原名称。
        folder: expected_folder,                  // 后台再次按精确路径校验而不信任可变目录文件。
    })
}

/// 为本次删除选择一个尚不存在的回收目录；从不覆盖以前删除的地图。
fn next_trash_destination(
    trash_root: &Path, // 已验证位于`assets/maps/.trash`的回收根。
    map_id: &str,      // 文件夹名使用稳定ASCII MapId。
    unix_millis: u128, // 时间后缀让人工恢复时可辨删除先后。
) -> Result<PathBuf, String> {
    let base = format!("{map_id}-{unix_millis}"); // 首选名称不附加多余序号。
    for suffix in 0..MAX_TRASH_NAME_ATTEMPTS {
        let name = if suffix == 0 {
            base.clone() // 同毫秒首次删除使用最短名称。
        } else {
            format!("{base}-{suffix}") // 极少碰撞时使用有界递增后缀。
        };
        let destination = trash_root.join(name); // 目标始终是回收根的直接子目录。
        if !destination.exists() {
            return Ok(destination); // 不覆盖任何既有备份。
        }
    }
    Err(format!(
        "地图{map_id}的回收目录名称尝试已超过{MAX_TRASH_NAME_ATTEMPTS}次"
    )) // 异常目录风暴下安全拒绝。
}

/// 删除前扫描其他地图的正式传送图，防止发布悬空跨地图目标。
fn ensure_no_incoming_portals(
    asset_root: &Path,           // 所有相对作者路径都从同一assets根解析。
    catalog: &MapCatalogAsset,   // 使用准备改写的同一目录快照。
    target: &MapDeletionRequest, // 查找指向当前删除目标的外部引用。
) -> Result<(), String> {
    if catalog.maps.len() > MAX_REFERENCE_SCAN_MAPS {
        return Err(format!(
            "地图目录共有{}项，超过删除引用检查上限{}项",
            catalog.maps.len(),
            MAX_REFERENCE_SCAN_MAPS
        )); // 不以一次删除请求启动无界文件IO。
    }
    for entry in catalog
        .maps
        .iter()
        .filter(|entry| entry.id != target.map_id)
    {
        let portals_path = safe_asset_path(asset_root, &entry.portals_path())?; // 路径先通过assets边界校验。
        let bytes = match std::fs::read(&portals_path) {
            Ok(bytes) => bytes, // 正常地图仍必须解析并核查全部入向引用。
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && map_deletion_request(entry).is_ok() =>
            {
                let folder = safe_asset_path(asset_root, &entry.folder)?; // 例外仅适用于精确的编辑器私有目录。
                match std::fs::symlink_metadata(&folder) {
                    Err(folder_error) if folder_error.kind() == std::io::ErrorKind::NotFound => {
                        continue; // 整个私有目录已手动删除，不可能再含有效传送图；保留其catalog入口供单独清理。
                    }
                    Err(folder_error) => {
                        return Err(format!("读取地图{}目录失败：{folder_error}", entry.id)); // 权限等异常不能伪装成手动删除。
                    }
                    Ok(_) => {
                        return Err(format!("读取地图{}传送图失败：{error}", entry.id)); // 文件缺失但目录仍在时无法证明无引用。
                    }
                }
            }
            Err(error) => return Err(format!("读取地图{}传送图失败：{error}", entry.id)), // 受保护地图、损坏路径及其他IO错误继续阻止删除。
        };
        let portals: MapPortalsAsset = serde_json::from_slice(&bytes)
            .map_err(|error| format!("解析地图{}传送图失败：{error}", entry.id))?; // 损坏传送图不能被静默忽略。
        if portals.schema_version != MAP_SCHEMA_VERSION || portals.map_id != entry.id {
            return Err(format!("地图{}传送图版本或所属身份不一致", entry.id)); // 只检查正式且身份一致的传送数据。
        }
        if let Some(portal) = portals
            .portals
            .iter()
            .find(|portal| portal.target.map_id == target.map_id)
        {
            return Err(format!(
                "地图{}的传送点“{}”仍指向{}；请先修改该传送目标",
                entry.display_name, portal.label, target.map_id
            )); // 明确指出首个阻塞引用，禁止留下坏链接。
        }
    }
    Ok(()) // 所有其他正式传送图均不存在入向引用。
}

/// 共用目录事务先隔离完整文件夹；永久删除只在元数据发布和移动都成功后执行。
fn delete_map_at(
    asset_root: &Path,            // 生产使用工作区assets，测试注入唯一临时根。
    request: &MapDeletionRequest, // 请求在确认时冻结并在后台重新验证。
    mode: MapDeletionMode,
) -> Result<DeletedMap, String> {
    delete_map_with_cleanup(asset_root, request, mode, |path| std::fs::remove_dir_all(path))
}

/// 注入清理操作以验证部分删除失败；它只能接收本事务刚隔离且复核过的目录。
fn delete_map_with_cleanup(
    asset_root: &Path,
    request: &MapDeletionRequest,
    mode: MapDeletionMode,
    cleanup: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<DeletedMap, String> {
    let _author_lock = feathrise_map_content::ContentAuthorLock::acquire(asset_root)?; // 地图回收与AI角色保存、首次目录登记共享同一锁。
    let catalog_path = asset_root.join(MAP_CATALOG_ASSET_PATH); // 全局地图发现入口。
    let original_catalog_bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("读取地图目录{}失败：{error}", catalog_path.display()))?; // 保留原字节用于失败精确回滚。
    let catalog = decode_map_catalog(&catalog_path, &original_catalog_bytes)?; // JSON与版本失败绝不继续。
    let entry = catalog
        .maps
        .iter()
        .find(|entry| entry.id == request.map_id)
        .ok_or_else(|| format!("地图{}已不在正式目录中", request.map_id))?; // 外部热更删除目标时拒收旧确认。
    let current_request = map_deletion_request(entry)?; // 使用最新目录重新执行MAP前缀和精确文件夹保护。
    if current_request.folder != request.folder {
        return Err(format!(
            "地图{}目录在确认后发生变化，请重新操作",
            request.map_id
        )); // 迟到确认不能移动新目录。
    }
    ensure_no_incoming_portals(asset_root, &catalog, request)?; // 目录改写前完成全部跨地图引用检查。
    let referrers = feathrise_map_content::saved_map_referrers(asset_root, &catalog, &request.map_id)?;
    if !referrers.is_empty() {
        return Err(format!("地图{}仍被以下地图作为子地图引用：{}；请先移除引用", request.map_id,
            referrers.iter().map(ToString::to_string).collect::<Vec<_>>().join("、")));
    } // 来源不能通过可恢复回收绕过引用保护，检查与目录删除共用作者锁。

    let maps_root = asset_root.join("maps"); // 删除目标与回收根必须同属该目录。
    let resolved_maps_root = maps_root
        .canonicalize()
        .map_err(|error| format!("无法解析地图根{}：{error}", maps_root.display()))?; // 后续比较使用真实绝对路径。
    let source = asset_root.join(&request.folder); // 请求已经限定为`maps/<MapId>`。
    let source_exists = match std::fs::symlink_metadata(&source) {
        Ok(source_metadata) => {
            if !source_metadata.is_dir() || source_metadata.file_type().is_symlink() {
                return Err(format!(
                    "地图目录{}不是可移动的真实文件夹",
                    source.display()
                )); // 符号链接或普通文件不能被误当作手动删除的地图。
            }
            let resolved_source = source
                .canonicalize()
                .map_err(|error| format!("无法解析地图目录{}：{error}", source.display()))?; // 解析junction等真实目标。
            if resolved_source != resolved_maps_root.join(request.map_id.as_str()) {
                return Err(format!(
                    "地图目录{}不是maps根的直接子目录",
                    source.display()
                )); // 同级junction别名也不能冒充目标地图，更不能永久删除其真实来源。
            }
            true // 真实私有目录继续走可恢复回收流程。
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false, // 作者已手动删目录时只清理残留记录。
        Err(error) => return Err(format!("读取地图目录{}失败：{error}", source.display())), // 权限等异常不能当作目录缺失。
    };

    let destination = if source_exists {
        let trash_root = maps_root.join(".trash"); // 只有真实文件夹需要创建可恢复备份目录。
        std::fs::create_dir_all(&trash_root)
            .map_err(|error| format!("创建地图回收区{}失败：{error}", trash_root.display()))?;
        let trash_metadata = std::fs::symlink_metadata(&trash_root)
            .map_err(|error| format!("读取地图回收区{}失败：{error}", trash_root.display()))?;
        if !trash_metadata.is_dir() || trash_metadata.file_type().is_symlink() {
            return Err(format!("地图回收区{}不是安全文件夹", trash_root.display())); // 拒绝把地图移动到外部链接。
        }
        let resolved_trash_root = trash_root
            .canonicalize()
            .map_err(|error| format!("无法解析地图回收区{}：{error}", trash_root.display()))?;
        if resolved_trash_root != resolved_maps_root.join(".trash") {
            return Err(format!("地图回收区{}越过maps根", trash_root.display())); // 回收区必须是maps的直接子目录。
        }
        let unix_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("系统时间早于Unix纪元：{error}"))?
            .as_millis(); // 只用于唯一且可读的备份目录名。
        Some(next_trash_destination(
            &trash_root,
            request.map_id.as_str(),
            unix_millis,
        )?) // 真实目录仍不覆盖任何既有回收备份。
    } else {
        None // 缺失目录不创建空的回收项。
    };
    let trash_folder = if let Some(destination) = destination.as_ref() {
        let trash_name = destination
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(|| format!("回收目录名称不是有效Unicode：{}", destination.display()))?;
        Some(format!("maps/.trash/{trash_name}")) // 发布目录前确定成功消息中的真实备份路径。
    } else {
        None // 手动删除的文件夹没有可以承诺恢复的备份。
    };

    let latest_catalog_bytes = std::fs::read(&catalog_path)
        .map_err(|error| format!("再次读取地图目录{}失败：{error}", catalog_path.display()))?; // 写入前检查外部并发修改。
    if latest_catalog_bytes != original_catalog_bytes {
        return Err("地图目录在删除检查期间发生变化，请重试".to_owned()); // 禁止覆盖同时发生的新建或外部编辑。
    }
    let folder_mutation = prepare_map_assignment_removal(asset_root, &request.map_id)?; // 损坏归类文件阻止留下失效键或覆盖原件。
    let mut next_catalog = catalog.clone(); // 完整副本只在后台单次删除事务中分配。
    next_catalog.maps.retain(|entry| entry.id != request.map_id); // 原子目录版本只移除唯一目标入口。
    if let Some(mutation) = folder_mutation.as_ref() {
        mutation.publish_checked()?; // 先移除编辑器归属，随后运行时catalog才隐藏地图入口。
    }
    if let Err(catalog_error) = atomic_write_json(&catalog_path, &next_catalog) {
        let rollback = folder_mutation.as_ref().map(|mutation| mutation.rollback()); // catalog失败同步恢复归类文件。
        return match rollback {
            Some(Err(rollback_error)) => Err(format!(
                "更新地图目录失败：{catalog_error}；恢复地图归类也失败：{rollback_error}"
            )),
            _ => Err(format!("更新地图目录失败：{catalog_error}")),
        };
    } // 先隐藏目录入口；崩溃时最多留下可恢复的原地孤儿目录。
    if let Some(destination) = destination.as_ref() {
        if let Err(move_error) = std::fs::rename(&source, destination) {
            let catalog_rollback = atomic_write(&catalog_path, &original_catalog_bytes); // 移动失败时尽力精确恢复原目录字节。
            let folder_rollback = folder_mutation.as_ref().map(|mutation| mutation.rollback()); // 同时恢复删除前归属。
            let mut message = format!("移动地图{}到回收区失败：{move_error}", request.map_id); // 始终保留原始移动错误。
            if let Err(rollback_error) = catalog_rollback {
                message.push_str(&format!("；恢复地图目录也失败：{rollback_error}")); // 不笼统声称目录仍可见。
            }
            if let Some(Err(rollback_error)) = folder_rollback {
                message.push_str(&format!("；恢复地图归类也失败：{rollback_error}")); // 独立说明编辑器结构恢复结果。
            }
            return Err(message); // 私有文件仍留在原位置供人工恢复。
        }
    }

    // 永久清理可能已经移除部分文件。此时绝不能回滚catalog，把半张地图重新登记；
    // 以已提交结果刷新目录，同时用警告明确给出残余位置。回收模式完全不调用清理。
    let mut cleanup_warning = None;
    let mut permanently_deleted = false;
    if mode == MapDeletionMode::Permanent
        && let Some(destination) = destination.as_ref()
    {
        cleanup_warning = finish_permanent_deletion(destination, &resolved_maps_root, cleanup).err();
        permanently_deleted = cleanup_warning.is_none();
    }

    Ok(DeletedMap {
        map_id: request.map_id.clone(),             // 返回已采用稳定身份。
        display_name: request.display_name.clone(), // 返回作者可读名称。
        trash_folder: if mode == MapDeletionMode::Recycle { trash_folder } else { None },
        permanently_deleted,
        cleanup_warning,
        source_folder: request.folder.clone(), // 供资产浏览器离开已移动目录。
        folder_snapshot: folder_mutation.as_ref().map(|mutation| mutation.snapshot()), // 主世界直接采用联合事务成功值。
    })
}

/// 递归删除前复核真实绝对路径仍位于本次隔离根，拒绝外部链接或临时路径替换。
fn finish_permanent_deletion(
    destination: &Path,
    resolved_maps_root: &Path,
    cleanup: impl FnOnce(&Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let result = (|| {
        let resolved = destination.canonicalize()?;
        let metadata = std::fs::symlink_metadata(destination)?;
        let expected = resolved_maps_root.join(".trash").join(destination.file_name().ok_or_else(|| {
            std::io::Error::other("缺少隔离目录名称")
        })?);
        if resolved != expected || !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(std::io::Error::other("隔离目录身份变化，拒绝递归删除"));
        }
        cleanup(&resolved)
    })();
    result.map_err(|error: std::io::Error| format!(
        "已移除地图目录入口，但永久清理未完成：{error}；残余文件位置：{}（可能已部分删除，不保证可恢复）",
        destination.display()
    ))
}

#[cfg(test)]
fn move_map_to_trash_at(asset_root: &Path, request: &MapDeletionRequest) -> Result<DeletedMap, String> {
    delete_map_at(asset_root, request, MapDeletionMode::Recycle)
}

/// HN-01切换完成后启动唯一IO事务；完整请求值移入后台线程。
fn start_map_deletion(
    editor: &mut MapEditorState,    // 状态栏立即显示后台阶段。
    deletion: &mut MapDeletionTask, // 单一任务槽提供票据和背压。
    request: MapDeletionRequest,    // 已确认并离开目标地图的冻结请求。
    mode: MapDeletionMode,          // 等待期间沿用点击确认时冻结的方式。
    references: crate::resources::MapDeletionReferences,
    stale: bool,
) {
    deletion.ticket = deletion.ticket.wrapping_add(1); // 每次真实文件事务使用新票据。
    let ticket = deletion.ticket; // 闭包只捕获Copy票据与拥有请求。
    let display_name = request.display_name.clone(); // 主线程状态不等待后台访问请求。
    deletion.feedback_target = Some(crate::types::EditorStatusTarget::new(
        request.map_id.as_str(),
        &display_name,
    )); // 仅冻结表现对象，后台请求及安全检查完全沿用原事务。
    deletion.task = Some(IoTaskPool::get().spawn(async move {
        let result = if stale { Err("引用检查期间作者文档发生变化，请重新删除".to_owned()) }
            else { ensure_no_draft_referrers(&references, &request.map_id).and_then(|()| delete_map_at(&workspace_asset_root(), &request, mode)) };
        (ticket, result) // 冻结引用的释放与正式文件检查均属于同一后台席位。
    })); // single-flight槽保证没有第二个删除并发改写catalog。
    editor.status = format!("正在后台执行{}：地图“{display_name}”", mode.label());
}

fn ensure_no_draft_referrers(references: &crate::resources::MapDeletionReferences,
    target: &feathrise_map_content::MapId) -> Result<(), String> {
    for (map, references) in &references.entries {
        if map != target && references.iter().any(|reference| &reference.source_map_id == target) {
            return Err(format!("打开的地图{map}仍把{target}作为子地图引用；请先移除并保存引用"));
        }
    }
    Ok(())
}

/// 等待安全回退并非阻塞采用删除结果；稳定帧只检查轻量状态和一个任务句柄。
pub(crate) fn poll_map_deletion(
    current_scene: Res<State<ScenePhase>>,   // 核对当前正式场景状态。
    active_dynamic: Res<ActiveDynamicMap>,  // 统一动态场景还需要真实MapId。
    mut deletion: ResMut<MapDeletionTask>,  // 维护切图等待、任务句柄和票据。
    mut editor: ResMut<MapEditorState>,     // 更新选中页和用户可见阶段。
    mut assets: ResMut<EditorAssetCatalog>, // 已删除私有目录不能继续作为资产浏览位置。
    mut browser: ResMut<MapBrowserState>,   // 删除归属键后同步采用完整虚拟目录快照。
    asset_server: Res<AssetServer>,         // 成功后显式请求全局目录热重载。
    documents: Option<Res<crate::ui::map_documents::MapDocuments>>,
    session: Option<Res<super::EditorAuthoringSession>>,
    world: crate::resources::MapEditorWorldSession,
    residency: Option<Res<feathrise_scene::ResidentScenes>>,
) {
    if deletion.task.is_none()
        && let Some(plan) = deletion.pending_start.as_ref()
    {
        let active_map_id = active_gameplay_map_id(*current_scene.get(), &active_dynamic); // 动态MAP不用共用场景枚举冒充身份。
        let world_stable = world.ready() || world.idle_menu();
        let safe_ready = active_map_id == plan.safe_active_map_id
            && active_map_id.as_ref() != Some(&plan.request.map_id)
            && if plan.safe_active_map_id.is_some() { world.ready() } else { world.idle_menu() && editor.selected_map_id.is_none() }
            && editor.pending_map_transition.is_none()
            && !editor.map_transition_in_progress; // 目标已卸载且完整就绪，作者外壳可见不能冒充切图完成。
        if safe_ready {
            if residency.as_ref().is_some_and(|scenes| scenes.contains(&plan.request.map_id)) {
                editor.status = format!("正在等待已关闭地图的场景资源退出，再执行{}", plan.mode.label());
                return; // 标签关闭先移除入口，实际实体按预算退役；仍被运行时持有时不移动其私有目录。
            }
            if documents.as_ref().is_some_and(|documents| documents.history_task.is_some() || documents.pending_history.is_some()) { return; }
            let current = session.as_ref().and_then(|session| session.draft.as_ref()).map(|draft| draft.map_id.clone());
            let revision = session.as_ref().map_or(0, |session| session.revision);
            let capture = deletion.references.get_or_insert_with(|| {
                let mut capture = crate::resources::MapDeletionReferences { current: current.clone(), revision, ..default() };
                if let Some(draft) = session.as_ref().and_then(|session| session.draft.as_ref()) {
                    capture.entries.insert(draft.map_id.clone(), draft.manifest.sub_maps.clone());
                }
                capture
            });
            let stale = capture.current != current || capture.revision != revision;
            let mut complete = documents.is_none();
            if !stale && let Some(documents) = documents.as_ref() {
                for _ in 0..2 {
                    let next = match &capture.cursor {
                        Some(cursor) => documents.parked.range((std::ops::Bound::Excluded(cursor), std::ops::Bound::Unbounded)).next(),
                        None => documents.parked.first_key_value(),
                    };
                    let Some((id, document)) = next else { complete = true; break; };
                    capture.cursor = Some(id.clone());
                    if let Some(draft) = document.session.draft.as_ref() { capture.entries.insert(id.clone(), draft.manifest.sub_maps.clone()); }
                }
            }
            if !complete && !stale { editor.status = "正在检查打开文档中的子地图引用".into(); return; }
            let references = deletion.references.take().unwrap();
            let plan = deletion
                .pending_start
                .take()
                .expect("已确认存在等待删除计划"); // 同帧只取走一次冻结目标与安全身份。
            start_map_deletion(&mut editor, &mut deletion, plan.request, plan.mode, references, stale); // 目标未被玩法世界使用时才触碰私有目录。
        } else if editor.open
            && world_stable
            && editor.pending_map_transition.is_none()
            && !editor.map_transition_in_progress
            && *current_scene.get() != ScenePhase::Loading
        {
            let target = plan.request.map_id.clone(); // 清除前保留错误反馈身份。
            let expected = plan.safe_active_map_id.as_ref().map_or("空工作台", |id| id.as_str()).to_owned(); // 空世界同样是明确冻结的安全目标。
            let feedback =
                crate::types::EditorStatusTarget::new(target.as_str(), &plan.request.display_name);
            deletion.pending_start = None; // 安全身份不符时绝不继续移动场景可能依赖的目录。
            editor.status = format!("当前运行地图未稳定在{expected}，地图{target}没有删除"); // 明确保持原地图完整。
            editor.record_operation_result(
                crate::types::EditorStatusOperation::MapDelete,
                &feedback,
                crate::types::EditorStatusKind::Error,
            );
        }
    }

    let Some(in_flight) = deletion.task.as_mut() else {
        return; // 没有文件任务时稳定帧立即返回。
    };
    let Some((ticket, result)) = check_ready(in_flight) else {
        return; // 未完成时不等待、不轮询文件系统。
    };
    deletion.task = None; // 已完成任务释放唯一并发槽。
    if ticket != deletion.ticket {
        return; // 迟到票据不得刷新目录或改写当前会话。
    }
    match result {
        Ok(deleted) => {
            asset_server.reload(MAP_CATALOG_ASSET_PATH); // 不仅依赖文件监听合并时序。
            if let Some(snapshot) = deleted.folder_snapshot.as_ref() {
                browser.document = snapshot.document.clone(); // 与联合事务成功磁盘值保持一致。
                browser.source_bytes = Some(snapshot.source_bytes.clone()); // 更新下一次并发检测基线。
                browser.initial_load_resolved = true; // 联合事务成功值已经是明确有效归类资产。
                browser.document_revision = browser.document_revision.wrapping_add(1); // 重建完整虚拟树索引。
                browser.write_blocked = None; // 正常删除保持结构写入口开放。
                browser.last_error = None; // 清除旧瞬时诊断。
                asset_server.reload(MAP_FOLDER_ASSET_PATH); // 不依赖文件监听合并时序。
            }
            let visible_fallback_map = editor.selected_map_id.clone().filter(|map_id| {
                map_is_visible_in_tree(&browser.document, &browser.expanded_folder_ids, map_id)
            }); // 安全地图只有整条父链展开时才是合法可见行选择。
            if browser.selected_item.as_ref() == Some(&MapBrowserItem::Map(deleted.map_id.clone()))
            {
                browser.select_only(visible_fallback_map.clone().map(MapBrowserItem::Map)); // 不继承旧整理选集或其他目录中的隐藏高亮。
            }
            if editor.map_list_selection_id.as_ref() == Some(&deleted.map_id) {
                editor.map_list_selection_id = visible_fallback_map; // 展开树看不到安全地图时清除旧按钮目标。
            }
            let deleted_prefix = format!("{}/", deleted.source_folder); // 判断资产浏览器是否位于其任意子目录。
            if assets.current_folder == deleted.source_folder
                || assets.current_folder.starts_with(&deleted_prefix)
            {
                assets.current_folder = "maps".to_owned(); // 离开已经移动的私有目录。
                assets.page = 0; // 新目录从第一页显示。
            }
            assets.requested_revision = assets.requested_revision.wrapping_add(1); // 无论浏览到哪一层，都异步移除已回收目录的旧快照。
            editor.status = if let Some(warning) = deleted.cleanup_warning.as_deref() {
                format!("地图“{}” [{}]：{warning}", deleted.display_name, deleted.map_id)
            } else if deleted.permanently_deleted {
                format!("地图“{}” [{}]已直接删除；文件不可恢复", deleted.display_name, deleted.map_id)
            } else if let Some(trash_folder) = deleted.trash_folder.as_deref() {
                format!(
                    "地图“{}” [{}]已移入 assets/{trash_folder}；可人工恢复",
                    deleted.display_name, deleted.map_id
                ) // 真实目录仍显示可恢复位置。
            } else {
                format!(
                    "地图“{}” [{}]的文件夹已不存在；已清理地图目录和归类记录",
                    deleted.display_name, deleted.map_id
                ) // 手动删除后只报告本次真正完成的元数据清理。
            };
            info!("{}", editor.status); // 开发日志保留同一可审计结果。
            if let Some(target) = deletion.feedback_target.take() {
                editor.record_operation_result(
                    crate::types::EditorStatusOperation::MapDelete,
                    &target,
                    if deleted.cleanup_warning.is_some() { crate::types::EditorStatusKind::Warning }
                        else { crate::types::EditorStatusKind::Success },
                ); // 半删失败也要采用已提交目录，但不能显示完整成功。
            }
        }
        Err(error) => {
            editor.status = format!("删除地图失败：{error}"); // 双重失败时不笼统声称目录仍可见，具体恢复结果由事务错误说明。
            if let Some(target) = deletion.feedback_target.take() {
                editor.record_operation_result(
                    crate::types::EditorStatusOperation::MapDelete,
                    &target,
                    crate::types::EditorStatusKind::Error,
                );
            }
            error!("{}", editor.status); // 文件与引用错误进入明确诊断。
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering}; // 并行测试使用进程内唯一临时目录。

    use feathrise_map_content::{MapPortalRecord, MapPortalTarget}; // 构造真实跨图传送引用。

    use crate::types::{MapFolderDocument, MapFolderRecord}; // 验证删除事务同步移除虚拟归属。

    use super::*; // 直接验证可注入assets根的完整文件事务。

    #[test]
    fn unsaved_and_parked_document_references_block_source_deletion() {
        use feathrise_map_content::{MapInstanceId, MapInstancePlacement};
        use std::sync::Arc;
        let source = feathrise_map_content::MapId::parse("MAP-source").unwrap();
        let parent = feathrise_map_content::MapId::parse("MAP-parent").unwrap();
        let other = feathrise_map_content::MapId::parse("MAP-other").unwrap();
        let mut references = crate::resources::MapDeletionReferences::default();
        references.entries.insert(parent.clone(), Arc::from([MapInstancePlacement {
            id: MapInstanceId::parse("village").unwrap(), source_map_id: source.clone(),
            label: "未保存引用".into(), transform: default() }]));
        references.current = Some(parent.clone());
        assert!(ensure_no_draft_referrers(&references, &source).unwrap_err().contains(parent.as_str()));
        references.current = Some(other.clone()); // 停放的父文档仍持有草稿引用，不能借切图绕过保护。
        assert!(ensure_no_draft_referrers(&references, &source).is_err());
        assert!(ensure_no_draft_referrers(&references, &other).is_ok());
        references.entries.insert(parent, Arc::from([]));
        assert!(ensure_no_draft_referrers(&references, &source).is_ok());
    }

    #[test]
    fn visible_editor_keeps_deletion_plan_until_safe_map_loading_finishes() {
        let target = feathrise_map_content::MapId::parse("MAP-wait-delete").unwrap();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()));
        app.insert_resource(MapEditorState {
            open: true,
            map_transition_in_progress: true,
            ..default()
        })
        .insert_resource(ActiveDynamicMap { map_id: Some(target.clone()) })
        .insert_resource(MapDeletionTask {
            pending_start: Some(crate::types::MapDeletionPlan {
                request: MapDeletionRequest {
                    folder: format!("maps/{target}"),
                    map_id: target,
                    display_name: "待回收地图".to_owned(),
                },
                safe_active_map_id: Some(feathrise_map_content::MapId::parse("HN-01").unwrap()),
                mode: MapDeletionMode::Recycle,
            }),
            ..default()
        })
        .init_resource::<EditorAssetCatalog>()
        .init_resource::<MapBrowserState>()
        .add_systems(Update, poll_map_deletion);
        for scene in [ScenePhase::Active, ScenePhase::Loading, ScenePhase::Active] {
            app.insert_resource(State::new(scene));
            app.update();
            let deletion = app.world().resource::<MapDeletionTask>();
            assert!(deletion.pending_start.is_some()); // 工作台可见及到达场景枚举都不能提前取消计划。
            assert!(deletion.task.is_none()); // 未完整就绪前绝不启动目录移动，不触碰正式资产。
        }
    }

    #[test]
    fn empty_workspace_deletion_waits_for_world_handoff_then_starts_the_confirmed_plan() {
        let target = feathrise_map_content::MapId::parse("MAP-empty-workspace-delete").unwrap();
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .insert_resource(State::new(ScenePhase::Idle))
            .insert_resource(MapEditorState { open: true, ..default() })
            // MainScene中的旧动态身份不能伪装为仍在运行的地图，正式场景与驻留门共同决定安全性。
            .insert_resource(ActiveDynamicMap { map_id: Some(target.clone()) })
            .insert_resource(feathrise_scene_core::residency::SceneResidencyActivity { switching: true, ..default() })
            .insert_resource(MapDeletionTask {
                pending_start: Some(crate::types::MapDeletionPlan {
                    request: MapDeletionRequest { folder: format!("maps/{target}"), map_id: target, display_name: "待回收空图".into() },
                    safe_active_map_id: None,
                    mode: MapDeletionMode::Permanent,
                }),
                // 强制引用票据失效，让正式任务在文件IO前返回；回归绝不接触用户地图。
                references: Some(crate::resources::MapDeletionReferences { revision: 1, ..default() }),
                ..default()
            })
            .init_resource::<EditorAssetCatalog>()
            .init_resource::<MapBrowserState>()
            .add_systems(Update, poll_map_deletion);
        app.update();
        assert!(app.world().resource::<MapDeletionTask>().pending_start.is_some());
        assert_eq!(app.world().resource::<MapDeletionTask>().ticket, 0);
        app.world_mut().resource_mut::<feathrise_scene_core::residency::SceneResidencyActivity>().switching = false;
        app.update();
        let deletion = app.world().resource::<MapDeletionTask>();
        assert!(deletion.pending_start.is_none(), "稳定空工作台必须消费已确认请求，不能要求先打开另一张地图");
        assert_eq!(deletion.ticket, 1);
    }

    static TEMP_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0); // 毫秒时间戳碰撞时仍不共用测试目录。
    const TEST_ROOT_PREFIX: &str = "bevy-learn5-map-delete-test-"; // Drop只回收带该前缀的直接临时子目录。

    /// 管理一个本测试创建的唯一assets根，失败时也不污染仓库。
    struct TestAssetRoot(PathBuf);

    impl TestAssetRoot {
        fn new(label: &str) -> Self {
            let sequence = TEMP_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed); // 同进程并行用例具有不同序号。
            let root = std::env::temp_dir().join(format!(
                "{TEST_ROOT_PREFIX}{}-{label}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir_all(root.join("maps")).expect("创建测试地图根"); // 生产事务要求maps根已存在。
            Self(root)
        }

        fn path(&self) -> &Path {
            &self.0 // 所有测试文件都从这一唯一根解析。
        }
    }

    impl Drop for TestAssetRoot {
        fn drop(&mut self) {
            let temporary_root = std::env::temp_dir(); // 回收前重新验证目标是系统临时根的直接子目录。
            let safe_target = self.0.parent() == Some(temporary_root.as_path())
                && self
                    .0
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(TEST_ROOT_PREFIX));
            if safe_target {
                let _ = std::fs::remove_dir_all(&self.0); // 只回收本用例创建且经前缀和父级双检查的临时树。
            }
        }
    }

    fn entry(id: &str, display_name: &str) -> MapCatalogEntry {
        let id = feathrise_map_content::MapId::parse(id).expect("测试MapId必须合法");
        MapCatalogEntry {
            folder: format!("maps/{id}"),          // 与F12创建器的私有目录形状一致。
            id,                                    // 目录和身份保持一致。
            display_name: display_name.to_owned(), // 确认和结果回显使用中文名称。
            published: true,                       // 删除规则同时覆盖已发布的编辑器图。
            surface_path: None,                    // 测试不需要加载私有内容资产。
            vegetation_path: None,                 // 测试不需要加载私有内容资产。
            water_path: None,                      // 测试不需要加载私有内容资产。
            characters_path: None,                 // 旧目录保持角色来源缺席。
            boundary_path: None,                   // 测试不需要加载私有内容资产。
        }
    }

    fn write_fixture(
        root: &Path,
        catalog: &MapCatalogAsset,
        fallback_portals: Vec<MapPortalRecord>,
    ) {
        for entry in &catalog.maps {
            std::fs::create_dir_all(root.join(&entry.folder)).expect("创建测试地图目录"); // 真实目录供canonicalize校验。
            let mut manifest = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121])).manifest;
            manifest.id = entry.id.clone();
            atomic_write_json(&root.join(entry.manifest_path()), &manifest).expect("写入测试地图清单供引用保护读取");
        }
        atomic_write_json(&root.join(MAP_CATALOG_ASSET_PATH), catalog).expect("写入测试目录");
        let fallback = catalog
            .maps
            .iter()
            .find(|entry| entry.id.as_str() == FALLBACK_MAP_ID)
            .expect("测试目录必须包含HN-01");
        atomic_write_json(
            &root.join(fallback.portals_path()),
            &MapPortalsAsset {
                schema_version: MAP_SCHEMA_VERSION, // 引用扫描只接受当前格式。
                map_id: fallback.id.clone(),        // 文件所属身份与目录一致。
                portals: fallback_portals,          // 用例决定是否构造入向引用。
            },
        )
        .expect("写入测试传送图");
    }

    #[test]
    fn only_exact_editor_private_maps_can_form_a_deletion_request() {
        assert!(map_deletion_request(&entry("HN-01", "开封城郊")).is_err()); // 正式HN地图始终受保护。
        let mut shared = entry("MAP-shared", "共享目录");
        shared.folder = "maps/shared".to_owned(); // MAP前缀也不授权移动共享目录。
        assert!(map_deletion_request(&shared).is_err());
        let private = entry("MAP-private", "私有地图");
        assert_eq!(
            map_deletion_request(&private).unwrap().folder,
            "maps/MAP-private"
        ); // 只有精确`maps/<MapId>`形状通过。
    }

    #[test]
    fn deletion_removes_catalog_entry_and_moves_private_folder_to_trash() {
        let root = TestAssetRoot::new("success");
        let fallback = entry("HN-01", "开封城郊");
        let target = entry("MAP-delete", "待回收地图");
        let catalog = MapCatalogAsset {
            schema_version: MAP_SCHEMA_VERSION,
            maps: vec![fallback, target.clone()],
        };
        write_fixture(root.path(), &catalog, Vec::new());
        atomic_write_json(
            &root.path().join(MAP_FOLDER_ASSET_PATH),
            &MapFolderDocument {
                folders: vec![MapFolderRecord {
                    id: "FOLDER-1".to_owned(),             // 目标地图所属虚拟文件夹。
                    display_name: "待回收区域".to_owned(), // 文件夹本身在删除地图后继续保留。
                    parent_id: None,                       // 根目录直接子文件夹。
                }],
                map_folders: std::collections::BTreeMap::from([(
                    target.id.as_str().to_owned(),
                    "FOLDER-1".to_owned(),
                )]), // 删除前显式归类目标地图。
                ..MapFolderDocument::default()
            },
        )
        .expect("写入测试归类文件");

        let request = map_deletion_request(&target).unwrap();
        let deleted = move_map_to_trash_at(root.path(), &request).expect("可恢复删除应成功");
        assert!(!root.path().join(&request.folder).exists()); // 正式私有目录不再可被运行时发现。
        assert!(
            root.path()
                .join(deleted.trash_folder.as_ref().unwrap())
                .is_dir()
        ); // 原始全目录保留在可恢复位置。
        let catalog_path = root.path().join(MAP_CATALOG_ASSET_PATH);
        let catalog_bytes = std::fs::read(&catalog_path).expect("读取删除后目录");
        let updated = decode_map_catalog(&catalog_path, &catalog_bytes).expect("解析删除后目录");
        assert_eq!(updated.maps.len(), 1); // 一次事务只移除唯一目标入口。
        assert_eq!(updated.maps[0].id.as_str(), FALLBACK_MAP_ID); // 受保护地图完整保留。
        let folder_bytes =
            std::fs::read(root.path().join(MAP_FOLDER_ASSET_PATH)).expect("读取删除后归类文件");
        let folders: MapFolderDocument =
            serde_json::from_slice(&folder_bytes).expect("解析删除后归类文件");
        assert!(!folders.map_folders.contains_key(target.id.as_str())); // 同一事务不留下已回收地图归属。
        assert_eq!(folders.folders.len(), 1); // 地图删除不删除其原虚拟文件夹。
        assert!(deleted.folder_snapshot.is_some()); // 主世界可直接采用联合事务成功快照。
    }

    #[test]
    fn permanent_deletion_removes_only_the_confirmed_map_and_preserves_old_trash() {
        let root = TestAssetRoot::new("permanent-delete");
        let target = entry("MAP-permanent", "直接删除地图");
        let catalog = MapCatalogAsset {
            schema_version: MAP_SCHEMA_VERSION,
            maps: vec![entry(FALLBACK_MAP_ID, "保留地图"), target.clone()],
        };
        write_fixture(root.path(), &catalog, vec![]);
        let old_trash = root.path().join("maps/.trash/old-backup");
        std::fs::create_dir_all(&old_trash).unwrap();
        std::fs::write(old_trash.join("keep.txt"), "旧回收文件").unwrap();
        let deleted = delete_map_at(root.path(), &map_deletion_request(&target).unwrap(), MapDeletionMode::Permanent).unwrap();
        assert!(deleted.permanently_deleted);
        assert!(deleted.cleanup_warning.is_none());
        assert!(deleted.trash_folder.is_none());
        assert!(!root.path().join(&target.folder).exists());
        assert_eq!(std::fs::read_dir(root.path().join("maps/.trash")).unwrap().count(), 1);
        assert_eq!(std::fs::read_to_string(old_trash.join("keep.txt")).unwrap(), "旧回收文件");
        assert!(root.path().join("maps/HN-01").is_dir());
        let path = root.path().join(MAP_CATALOG_ASSET_PATH);
        let after = decode_map_catalog(&path, &std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(after.maps.len(), 1);
        assert_eq!(after.maps[0].id.as_str(), FALLBACK_MAP_ID);
    }

    #[test]
    fn partial_permanent_cleanup_reports_residue_without_republishing_a_broken_map() {
        let root = TestAssetRoot::new("partial-permanent-delete");
        let target = entry("MAP-partial", "部分清理失败");
        write_fixture(root.path(), &MapCatalogAsset {
            schema_version: MAP_SCHEMA_VERSION, maps: vec![entry(FALLBACK_MAP_ID, "保留地图"), target.clone()],
        }, vec![]);
        let request = map_deletion_request(&target).unwrap();
        std::fs::write(root.path().join(&target.folder).join("removed.txt"), "将被删除").unwrap();
        std::fs::write(root.path().join(&target.folder).join("locked.txt"), "保留残余").unwrap();
        let deleted = delete_map_with_cleanup(root.path(), &request, MapDeletionMode::Permanent, |path| {
            std::fs::remove_file(path.join("removed.txt"))?;
            Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "模拟文件被占用"))
        }).unwrap();
        assert!(!deleted.permanently_deleted);
        assert!(deleted.trash_folder.is_none(), "半删文件不能承诺可恢复");
        let warning = deleted.cleanup_warning.unwrap();
        assert!(warning.contains("模拟文件被占用") && warning.contains("残余文件位置"));
        let residue = std::fs::read_dir(root.path().join("maps/.trash")).unwrap().next().unwrap().unwrap().path();
        assert!(residue.join("locked.txt").is_file());
        assert!(!residue.join("removed.txt").exists());
        assert!(!root.path().join(&target.folder).exists());
        let path = root.path().join(MAP_CATALOG_ASSET_PATH);
        let after = decode_map_catalog(&path, &std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(after.maps.len(), 1);
        assert_eq!(after.maps[0].id.as_str(), FALLBACK_MAP_ID);
    }

    #[test]
    fn recycle_never_calls_permanent_cleanup_and_keeps_exact_file_contents() {
        let root = TestAssetRoot::new("recycle-keeps-bytes");
        let target = entry("MAP-recycle", "回收地图");
        write_fixture(root.path(), &MapCatalogAsset {
            schema_version: MAP_SCHEMA_VERSION, maps: vec![entry(FALLBACK_MAP_ID, "保留地图"), target.clone()],
        }, vec![]);
        let payload = [0, 255, 13, 10, 128];
        std::fs::write(root.path().join(&target.folder).join("author.bin"), payload).unwrap();
        let deleted = delete_map_with_cleanup(root.path(), &map_deletion_request(&target).unwrap(),
            MapDeletionMode::Recycle, |_| panic!("回收不得调用永久清理")).unwrap();
        assert!(!deleted.permanently_deleted && deleted.cleanup_warning.is_none());
        assert_eq!(std::fs::read(root.path().join(deleted.trash_folder.unwrap()).join("author.bin")).unwrap(), payload);
    }

    #[test]
    fn incoming_portal_reference_blocks_deletion_without_moving_or_rewriting() {
        let root = TestAssetRoot::new("incoming-reference");
        let fallback = entry("HN-01", "开封城郊");
        let stale = entry("MAP-stale", "已手动删除的地图"); // 先跳过缺失目录后仍需继续扫描有效来源。
        let target = entry("MAP-linked", "被引用地图");
        let catalog = MapCatalogAsset {
            schema_version: MAP_SCHEMA_VERSION,
            maps: vec![stale.clone(), fallback, target.clone()], // 缺失项先于包含入向引用的地图。
        };
        let portal = MapPortalRecord {
            terrain_binding: None,
            arrival_terrain_binding: None,
            id: "to-linked".to_owned(),
            label: "通往被引用地图".to_owned(),
            label_customized: true, // 删除引用测试保留显式作者名称，不参与自动默认名迁移。
            position: [0.0, 0.0, 0.0],
            arrival: [1.0, 0.0, 0.0],
            target: MapPortalTarget::portal(target.id.clone(), "arrival"), // 外部地图仍指向待删除MapId的具体到达点。
            color_srgba: [1.0, 1.0, 1.0, 1.0],
            arrow_degrees: 0.0,
        };
        write_fixture(root.path(), &catalog, vec![portal]);
        std::fs::remove_dir_all(root.path().join(&stale.folder))
            .expect("模拟手动删除另一张地图目录"); // 例外只跳过该项，不提前宣告无入向引用。
        let catalog_path = root.path().join(MAP_CATALOG_ASSET_PATH);
        let original_catalog = std::fs::read(&catalog_path).expect("读取原目录快照");

        let request = map_deletion_request(&target).unwrap();
        for mode in [MapDeletionMode::Recycle, MapDeletionMode::Permanent] {
            let error = delete_map_at(root.path(), &request, mode).expect_err("两种删除方式都必须拦截外部引用");
            assert!(error.contains("通往被引用地图")); // 永久删除不能绕过原保护门。
        }
        assert!(root.path().join(&request.folder).is_dir()); // 校验失败时私有文件夹绝不移动。
        assert_eq!(
            std::fs::read(&catalog_path).expect("重读目录"),
            original_catalog
        ); // 校验失败时目录字节也不发生变化。
        assert!(!root.path().join("maps/.trash").exists()); // 引用门在创建回收区之前就终止事务。
    }

    #[test]
    fn missing_other_private_folder_does_not_block_deletion_or_later_cleanup() {
        let root = TestAssetRoot::new("missing-other-private-folder"); // 模拟catalog里残留已手动删掉的另一张地图。
        let fallback = entry("HN-01", "开封城郊"); // 受保护地图的有效传送图仍必须接受扫描。
        let stale = entry("MAP-stale", "残留地图"); // 缺失私有目录不提供任何可读取的传送图。
        let target = entry("MAP-delete", "待回收地图"); // 另一张仍有完整私有目录的地图应能回收。
        let catalog = MapCatalogAsset {
            schema_version: MAP_SCHEMA_VERSION, // 与正式目录使用同一版本。
            maps: vec![stale.clone(), fallback, target.clone()], // 缺失项排在有效传送图前，不能提前终止扫描。
        };
        write_fixture(root.path(), &catalog, Vec::new()); // 只为受保护地图写入真实传送图。
        std::fs::remove_dir_all(root.path().join(&stale.folder))
            .expect("模拟手动删除另一张地图的私有目录"); // 目录整体消失而非仅删除传送文件。

        let request = map_deletion_request(&target).expect("完整私有地图允许回收"); // 不因其他残留项拒绝操作。
        let deleted = move_map_to_trash_at(root.path(), &request).expect("有效地图应可回收"); // 有效传送图仍被扫描。
        assert!(root.path().join(deleted.trash_folder.unwrap()).is_dir()); // 存在的目标仍须实际移入回收区。
        let catalog_path = root.path().join(MAP_CATALOG_ASSET_PATH); // 检查第一轮只删除选中目标。
        let after_first = std::fs::read(&catalog_path).expect("读取首次删除后的目录"); // 回收成功后目录仍可解析。
        let after_first =
            decode_map_catalog(&catalog_path, &after_first).expect("解析首次删除后的目录"); // 保留残留地图供用户单独清理。
        assert_eq!(after_first.maps.len(), 2); // 不顺手删除另一条catalog记录。
        assert!(after_first.maps.iter().any(|entry| entry.id == stale.id)); // 残留项继续可在编辑器内选中。

        let stale_request = map_deletion_request(&stale).expect("残留地图仍可形成受限请求"); // 第二轮清理自身。
        let cleaned = move_map_to_trash_at(root.path(), &stale_request).expect("残留地图应可清理"); // 不创建虚假的备份。
        assert!(cleaned.trash_folder.is_none()); // 缺失目录只清理记录，不能宣称可恢复。
        let after_second = std::fs::read(&catalog_path).expect("读取清理后的目录"); // 两轮事务均保持目录有效。
        let after_second =
            decode_map_catalog(&catalog_path, &after_second).expect("解析清理后的目录"); // 唯一受保护地图保留。
        assert_eq!(after_second.maps.len(), 1); // 两张MAP地图均由作者分别确认删除。
        assert_eq!(after_second.maps[0].id.as_str(), FALLBACK_MAP_ID); // HN-01未被改动。
    }

    #[test]
    fn missing_portal_file_in_existing_other_folder_still_blocks_deletion() {
        let root = TestAssetRoot::new("missing-other-portal-file"); // 目录存在但传送文件缺失，不属于手动删目录例外。
        let fallback = entry("HN-01", "开封城郊"); // 保留一份完整的受保护地图。
        let other = entry("MAP-incomplete", "传送图缺失"); // 私有目录仍存在，无法证明没有入向引用。
        let target = entry("MAP-delete", "待回收地图"); // 失败时不得移动其目录。
        let catalog = MapCatalogAsset {
            schema_version: MAP_SCHEMA_VERSION, // 与正式目录保持一致。
            maps: vec![fallback, other.clone(), target.clone()], // 待扫描地图缺失单份传送图。
        };
        write_fixture(root.path(), &catalog, Vec::new()); // fixture只写入HN-01传送图。
        let catalog_path = root.path().join(MAP_CATALOG_ASSET_PATH); // 记录失败前目录原文。
        let original = std::fs::read(&catalog_path).expect("读取原目录"); // 失败必须保持字节不变。

        let request = map_deletion_request(&target).expect("有效删除请求"); // 让后台保护检查决定是否执行。
        let error = move_map_to_trash_at(root.path(), &request).expect_err("缺文件仍必须拦截"); // 不静默跳过可能含引用的地图。
        assert!(error.contains("MAP-incomplete传送图")); // 明确指出需修复的地图与文件。
        assert!(root.path().join(&other.folder).is_dir()); // 目录本身存在，不能当作已手动删除。
        assert!(root.path().join(&target.folder).is_dir()); // 未触碰目标私有目录。
        assert_eq!(std::fs::read(&catalog_path).expect("重读目录"), original); // 未发布任何更改。
        assert!(!root.path().join("maps/.trash").exists()); // 失败前不创建回收区。
    }

    #[test]
    fn missing_protected_map_folder_still_blocks_deletion() {
        let root = TestAssetRoot::new("missing-protected-folder"); // 受保护地图不能通过私有地图例外跳过。
        let fallback = entry("HN-01", "开封城郊"); // 保留有效传送图。
        let protected = entry("HN-02", "黄河故道"); // 即使目录缺失也不能忽略正式地图来源。
        let target = entry("MAP-delete", "待回收地图"); // 选中目标应保持原状。
        let catalog = MapCatalogAsset {
            schema_version: MAP_SCHEMA_VERSION, // 与正式目录版本一致。
            maps: vec![fallback, protected.clone(), target.clone()], // 正式地图排在目标前。
        };
        write_fixture(root.path(), &catalog, Vec::new()); // 建立fixture后只移除受保护地图目录。
        std::fs::remove_dir_all(root.path().join(&protected.folder)).expect("模拟缺失正式地图目录"); // 不应被误判为可跳过的MAP私有目录。

        let request = map_deletion_request(&target).expect("有效删除请求"); // 由入向检查拒绝不完整正式地图。
        let error = move_map_to_trash_at(root.path(), &request).expect_err("正式地图缺失必须拦截"); // 没有权限豁免。
        assert!(error.contains("HN-02传送图")); // 维持原有缺失文件诊断。
        assert!(root.path().join(&target.folder).is_dir()); // 未移动或删除目标。
        assert!(!root.path().join("maps/.trash").exists()); // 失败不创建空备份。
    }

    #[test]
    fn missing_private_folder_cleans_only_stale_catalog_and_assignment() {
        let root = TestAssetRoot::new("missing-private-folder"); // 独立临时根模拟作者手动删除了地图文件夹。
        let fallback = entry("HN-01", "开封城郊"); // 其他地图必须完整保留。
        let target = entry("MAP-missing", "已手动删除文件的地图"); // 目录入口仍引用不存在的私有文件夹。
        let catalog = MapCatalogAsset {
            schema_version: MAP_SCHEMA_VERSION,   // 复用正式地图目录版本。
            maps: vec![fallback, target.clone()], // 目标仍出现在F12地图列表。
        };
        write_fixture(root.path(), &catalog, Vec::new()); // 先生成有效目录与传送图。
        std::fs::remove_dir_all(root.path().join(&target.folder))
            .expect("模拟作者手动删除目标目录"); // 只删除本测试刚创建的私有目录。
        atomic_write_json(
            &root.path().join(MAP_FOLDER_ASSET_PATH),
            &MapFolderDocument {
                map_folders: std::collections::BTreeMap::from([(
                    target.id.as_str().to_owned(), // 冻结目标身份以验证归类键清理。
                    "FOLDER-1".to_owned(),         // 模拟作者此前把地图归入虚拟文件夹。
                )]),
                folders: vec![MapFolderRecord {
                    id: "FOLDER-1".to_owned(),             // 文件夹本身不会随地图记录删除。
                    display_name: "保留文件夹".to_owned(), // 清理后作者仍可使用此分类。
                    parent_id: None,                       // 根级虚拟文件夹。
                }],
                ..MapFolderDocument::default() // 其他编辑器归类字段保持默认。
            },
        )
        .expect("写入测试归类文件"); // 与真实清理路径共用归类事务。

        let request = map_deletion_request(&target).expect("精确私有目录可形成删除请求"); // 缺失目录不绕过身份保护。
        let deleted =
            move_map_to_trash_at(root.path(), &request).expect("手动删除后的残留记录应可清理"); // 执行完整引用检查和目录事务。
        assert!(deleted.trash_folder.is_none()); // 无原始文件时不得声称产生了可恢复备份。
        assert!(!root.path().join("maps/.trash").exists()); // 不为不存在的私有目录创建空回收区。
        let catalog_path = root.path().join(MAP_CATALOG_ASSET_PATH); // 读取本次事务更新的唯一目录。
        let catalog_bytes = std::fs::read(&catalog_path).expect("读取清理后的地图目录"); // 原子写入已发布。
        let remaining =
            decode_map_catalog(&catalog_path, &catalog_bytes).expect("解析清理后的地图目录"); // JSON格式和版本仍有效。
        assert_eq!(remaining.maps.len(), 1); // 只移除一条已失效地图入口。
        assert_eq!(remaining.maps[0].id.as_str(), FALLBACK_MAP_ID); // 其他地图保持可用。
        let folder_bytes =
            std::fs::read(root.path().join(MAP_FOLDER_ASSET_PATH)).expect("读取清理后的归类文件"); // 同事务的第二份元数据。
        let folders: MapFolderDocument =
            serde_json::from_slice(&folder_bytes).expect("解析清理后的归类文件"); // 归类结构仍完整。
        assert!(!folders.map_folders.contains_key(target.id.as_str())); // 不留下引用失效地图的归类键。
        assert_eq!(folders.folders.len(), 1); // 虚拟文件夹自身仍然保留。
    }
}
