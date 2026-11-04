//! 编辑器专用虚拟文件夹资产加载、并发检测和原子写入事务。
//!
//! 运行时地图目录完全不依赖本文件；只有F12编辑器注册这个加载器和这些系统。

use std::collections::BTreeSet; // 写入回滚冻结最多512个会话展开文件夹身份。
use std::io::{Error, ErrorKind}; // 有界资产读取把超限数据转换为标准加载失败。
use std::path::{Path, PathBuf}; // 作者资产根和归类文件绝对路径。

use bevy::asset::{
    AssetEvent, AssetLoadError, AssetLoadFailedEvent, AssetLoader, AsyncReadExt, LoadContext,
    LoadState,
    io::{AssetReaderError, Reader},
}; // 自定义复合扩展、热更新和缺失文件识别。
use bevy::ecs::error::BevyError; // AssetLoader统一错误承载。
use bevy::prelude::*; // ECS系统参数与资源句柄。
use bevy::tasks::{IoTaskPool, futures::check_ready}; // 文件IO只在后台任务执行，主线程非阻塞轮询。
use feathrise_map_content::MapId; // 创建、删除和拖拽事务继续使用正式地图身份。

use crate::authoring::{atomic_write, workspace_asset_root}; // 复用项目既有同目录原子替换能力。
use crate::constants::{MAP_FOLDER_ASSET_PATH, MAP_FOLDER_MAX_FILE_BYTES}; // 唯一文件路径与硬大小预算。
use crate::resources::{
    MapBrowserState, MapEditorState, MapFolderAssetHandle, MapFolderPersistence,
}; // 内存权威、回滚快照和编辑器状态。
use crate::types::{
    MapBrowserItem, MapFolderAsset, MapFolderDocument, MapFolderFileSnapshot, MapFolderWriteFailure,
}; // 加载资产、普通文档与后台结果。

use super::helpers::{assign_map_to_folder, decode_folder_document, encode_folder_document}; // 所有入口共享同一校验门。

const READ_CHUNK_BYTES: usize = 8 * 1024; // 固定小缓冲避免按损坏文件声明预分配。

/// 归类文档换代后清理已经失效的目录选择与会话展开身份。
fn reconcile_tree_session(browser: &mut MapBrowserState) {
    let valid_folder_ids = browser
        .document
        .folders
        .iter()
        .map(|folder| folder.id.clone())
        .collect::<BTreeSet<_>>(); // 热更新采用帧一次建立最多512项受检身份集合。
    let old_expanded_count = browser.expanded_folder_ids.len(); // 只在真实清理后推进投影修订。
    browser
        .expanded_folder_ids
        .retain(|folder_id| valid_folder_ids.contains(folder_id)); // 失效展开身份不得跨多次外部换代累积。
    if browser.expanded_folder_ids.len() != old_expanded_count {
        browser.expansion_revision = browser.expansion_revision.wrapping_add(1); // 拒收仍包含旧目录的后台投影。
    }
    if matches!(
        browser.selected_item.as_ref(),
        Some(MapBrowserItem::Folder(folder_id)) if !valid_folder_ids.contains(folder_id)
    ) {
        browser.select_only(None); // 外部删除所选目录后清除不可见结构操作目标。
    }
}

/// 有界读取归类文件，遇到第256KiB后的首字节立即拒绝。
async fn read_bounded(reader: &mut dyn Reader) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new(); // 仅在资产IO任务内拥有读取缓冲。
    let mut chunk = [0_u8; READ_CHUNK_BYTES]; // 栈上固定大小分块。
    loop {
        let read = reader.read(&mut chunk).await?; // 异步资产读取不占用主世界Update。
        if read == 0 {
            return Ok(bytes); // EOF后交给统一JSON与结构校验。
        }
        if read > MAP_FOLDER_MAX_FILE_BYTES.saturating_sub(bytes.len()) {
            return Err(Error::new(
                ErrorKind::InvalidData,
                format!("地图归类文件超过{MAP_FOLDER_MAX_FILE_BYTES}字节上限"),
            )); // 超限前不追加当前分块。
        }
        bytes.extend_from_slice(&chunk[..read]); // 始终保持Vec长度不超过硬预算。
    }
}

/// 只接管编辑器私有`.map_folders.json`文件。
#[derive(Default, TypePath)]
pub(crate) struct MapFolderAssetLoader;

impl AssetLoader for MapFolderAssetLoader {
    type Asset = MapFolderAsset; // 加载成功同时保留文档和精确源字节。
    type Settings = (); // 当前格式没有每句柄加载设置。
    type Error = BevyError; // IO、UTF-8/JSON和结构诊断统一进入Bevy错误链。

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let source_bytes = read_bounded(reader).await?; // 主线程从不读取或解析完整归类文件。
        let document = decode_folder_document(&source_bytes)
            .map_err(|message| Error::new(ErrorKind::InvalidData, message))?; // 损坏内容形成加载失败而非空目录。
        Ok(MapFolderAsset {
            document,     // 已经过完整版本、层级和预算校验。
            source_bytes, // 并发检测必须比较作者原始字节而非重序列化结果。
        })
    }

    fn extensions(&self) -> &[&str] {
        &["map_folders.json"] // 不抢占项目其他普通JSON或地图运行时JSON。
    }
}

/// 返回编辑器归类文件的绝对作者路径。
pub(crate) fn folder_file_path(asset_root: &Path) -> PathBuf {
    asset_root.join(MAP_FOLDER_ASSET_PATH) // 常量保持正斜杠，Path负责当前平台组合。
}

/// 读取可选文件；缺失是合法空分类基线，其他错误保留诊断。
fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)), // 精确磁盘字节用于乐观并发比较。
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None), // 首次使用无需预建文件。
        Err(error) => Err(format!("读取地图归类文件{}失败：{error}", path.display())), // 权限和设备错误不能冒充缺失。
    }
}

/// 仅当磁盘仍等于操作开始时的精确字节时原子写入新文档。
fn write_document_if_unchanged(
    asset_root: &Path,               // 冻结的项目assets根。
    expected_bytes: Option<Vec<u8>>, // 操作前有效源字节或合法缺失。
    document: MapFolderDocument,     // 已乐观应用的完整目标文档。
) -> Result<MapFolderFileSnapshot, MapFolderWriteFailure> {
    let source_bytes = encode_folder_document(&document).map_err(MapFolderWriteFailure::Io)?;
    write_snapshot_if_unchanged(
        asset_root,
        expected_bytes,
        MapFolderFileSnapshot {
            document,
            source_bytes,
        },
    )
}

/// 单项与批量共用同一磁盘并发门；批量已在准备任务内完成完整编码校验。
pub(super) fn write_snapshot_if_unchanged(
    asset_root: &Path,
    expected_bytes: Option<Vec<u8>>,
    snapshot: MapFolderFileSnapshot,
) -> Result<MapFolderFileSnapshot, MapFolderWriteFailure> {
    let path = folder_file_path(asset_root); // 单一文件是本事务唯一写入目标。
    let current_bytes = read_optional(&path).map_err(MapFolderWriteFailure::Io)?; // 后台读取当前真实磁盘状态。
    if current_bytes != expected_bytes {
        return Err(MapFolderWriteFailure::ConcurrentChange(
            "磁盘字节已不同于操作开始时的有效归类文件".to_owned(),
        )); // 外部编辑或另一进程写入时绝不覆盖。
    }
    atomic_write(&path, &snapshot.source_bytes).map_err(MapFolderWriteFailure::Io)?;
    Ok(snapshot) // 成功后采用确切已写文档；失败时整批仍保留操作前内存和磁盘。
}

/// 一个可嵌入地图创建或删除事务的归类文件改写。
#[derive(Debug)]
pub(crate) struct MapFolderFileMutation {
    path: PathBuf,                        // 唯一归类文件绝对路径。
    original_bytes: Option<Vec<u8>>,      // 发布前必须仍与这份基线一致。
    next_snapshot: MapFolderFileSnapshot, // 完整目标文档和已编码字节。
}

impl MapFolderFileMutation {
    /// 在地图目录发布之前写入归类文件，并拒绝覆盖并发外部修改。
    pub(crate) fn publish_checked(&self) -> Result<(), String> {
        let current = read_optional(&self.path)?; // 每次发布都在最后一刻读取真实磁盘值。
        if current != self.original_bytes {
            return Err("地图归类文件已被外部修改，事务已取消".to_owned()); // 调用方保留未发布地图目录或恢复删除。
        }
        atomic_write(&self.path, &self.next_snapshot.source_bytes) // 与独立拖拽保存复用相同原子替换。
    }

    /// 仅在磁盘仍是本事务写入值时恢复操作前字节。
    pub(crate) fn rollback(&self) -> Result<(), String> {
        let current = read_optional(&self.path)?; // 不覆盖事务之后发生的第三方修改。
        if current.as_deref() != Some(self.next_snapshot.source_bytes.as_slice()) {
            return Err("地图归类文件在回滚前再次变化，已保留当前磁盘文件".to_owned()); // 宁可显式诊断也不盲写旧快照。
        }
        match self.original_bytes.as_deref() {
            Some(bytes) => atomic_write(&self.path, bytes), // 恢复原有有效文件。
            None => std::fs::remove_file(&self.path).map_err(|error| {
                format!(
                    "移除事务新建的地图归类文件{}失败：{error}",
                    self.path.display()
                )
            }), // 仅删除本事务刚创建且内容身份仍匹配的精确文件。
        }
    }

    /// 返回成功发布后供主世界同步采用的完整归类快照。
    pub(crate) fn snapshot(&self) -> MapFolderFileSnapshot {
        self.next_snapshot.clone() // 小型编辑器结构在事务完成帧复制一次。
    }
}

/// 为新地图准备归属改写；实际写入延迟到地图文件准备完成且catalog发布之前。
pub(crate) fn prepare_map_assignment_mutation(
    asset_root: &Path,               // 创建任务冻结的项目assets根。
    original_bytes: Option<Vec<u8>>, // 点击“创建”时冻结的归类文件基线。
    map_id: &MapId,                  // 创建任务最终生成的稳定地图身份。
    folder_id: Option<&str>,         // 点击时冻结的当前虚拟文件夹。
) -> Result<Option<MapFolderFileMutation>, String> {
    let mut document = match original_bytes.as_deref() {
        Some(bytes) => decode_folder_document(bytes)?, // 只基于点击时已验证字节构造目标。
        None => MapFolderDocument::default(),          // 缺失文件等价于空分类。
    };
    let old_assignment = document.map_folders.get(map_id.as_str()).cloned(); // 防御性支持测试中的已有键。
    assign_map_to_folder(&mut document, map_id, folder_id)?; // 目标文件夹必须仍存在于冻结文档。
    let new_assignment = document.map_folders.get(map_id.as_str()).cloned(); // 根目录以无键表示。
    if old_assignment == new_assignment {
        return Ok(None); // 根目录新地图无需制造一份空归类文件。
    }
    let source_bytes = encode_folder_document(&document)?; // 发布前冻结唯一目标字节。
    Ok(Some(MapFolderFileMutation {
        path: folder_file_path(asset_root), // 归类事务只写编辑器专用文件。
        original_bytes,                     // publish_checked最后检查外部并发变化。
        next_snapshot: MapFolderFileSnapshot {
            document,     // 完整归类树保持不变，只增加本地图键。
            source_bytes, // 原子写入内容。
        },
    }))
}

/// 为地图回收准备归属移除；文件缺失或地图本来在根目录时无需写入。
pub(crate) fn prepare_map_assignment_removal(
    asset_root: &Path, // 删除任务后台读取真实作者根。
    map_id: &MapId,    // 已完成全部保护校验的删除目标。
) -> Result<Option<MapFolderFileMutation>, String> {
    let path = folder_file_path(asset_root); // 只读编辑器专用归类文件。
    let original_bytes = read_optional(&path)?; // 删除事务在后台冻结最新磁盘基线。
    let Some(bytes) = original_bytes.as_deref() else {
        return Ok(None); // 从未建立分类时没有同步工作。
    };
    let mut document = decode_folder_document(bytes)?; // 损坏文件阻止删除，不能留下失效归属或覆盖原件。
    if document.map_folders.remove(map_id.as_str()).is_none() {
        return Ok(None); // 根目录地图不在映射中，避免无意义重写。
    }
    let source_bytes = encode_folder_document(&document)?; // 移除后仍需通过全部结构预算。
    Ok(Some(MapFolderFileMutation {
        path,           // 使用刚读取的精确目标路径。
        original_bytes, // 目录移动前再次比较防止竞态。
        next_snapshot: MapFolderFileSnapshot {
            document,     // 删除目标的归属键已移除。
            source_bytes, // 成功后成为新编辑器基线。
        },
    }))
}

/// 启动时请求唯一归类资产并以强句柄保持热更新订阅。
pub(crate) fn load_map_folder_asset(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle = asset_server.load::<MapFolderAsset>(MAP_FOLDER_ASSET_PATH); // 文件缺失也保留稳定句柄等待首次创建。
    commands.insert_resource(MapFolderAssetHandle { handle }); // 避免无强引用时资产被回收。
}

/// 统一采用缺失或损坏加载结果；初始LoadState回查和后续失败事件必须具有相同语义。
fn apply_map_folder_load_failure(
    error: &AssetLoadError,        // Bevy保留的完整加载失败原因。
    path: &str,                    // 状态栏中的作者可读资产路径。
    browser: &mut MapBrowserState, // 缺失归零，损坏保留上一有效树并禁写。
    editor: &mut MapEditorState,   // F12打开时即时回显损坏诊断。
) {
    let missing = matches!(
        error,
        AssetLoadError::AssetReaderError(AssetReaderError::NotFound(_))
    ); // 只有真实NotFound允许解释为空分类。
    browser.initial_load_resolved = true; // 明确失败同样完成首次加载收口，稳定帧无需继续探测。
    if missing {
        browser.document = MapFolderDocument::default(); // 缺失文件按计划显示全部地图在根目录。
        browser.source_bytes = None; // 下一次写入要求磁盘仍然缺失。
        browser.document_revision = browser.document_revision.wrapping_add(1); // 重建浏览索引。
        if !browser.expanded_folder_ids.is_empty() {
            browser.expanded_folder_ids.clear(); // 缺失文件没有任何可继续展开的文件夹身份。
            browser.expansion_revision = browser.expansion_revision.wrapping_add(1); // 拒收旧树投影。
        }
        browser.select_only(None); // 清除旧结构选择。
        browser.write_blocked = None; // 允许首次创建文件夹。
        browser.last_error = None; // 缺失不是作者错误。
        return;
    }
    let message = format!("地图归类文件损坏或加载失败：{path} ({error})"); // 保留精确Bevy诊断。
    browser.write_blocked = Some(message.clone()); // 禁止覆盖损坏原件。
    browser.last_error = Some(message.clone()); // UI可持续回显。
    if editor.open {
        editor.status = message; // 作者无需查看日志即可发现结构写入已禁用。
    }
}

/// 应用归类资产首次加载和热更新；损坏文件始终保留上一份有效内存结果。
pub(crate) fn apply_map_folder_asset_events(
    assets: Res<Assets<MapFolderAsset>>, // 读取已经由加载器完整验收的资产值。
    asset_server: Res<AssetServer>,      // 首次事件早到时只回查一次最终LoadState。
    handle: Option<Res<MapFolderAssetHandle>>, // 独立系统测试可能尚未执行Startup。
    persistence: Res<MapFolderPersistence>, // 在途乐观写入期间忽略文件监听中间事件。
    mut events: MessageReader<AssetEvent<MapFolderAsset>>, // 成功首载和热更新。
    mut failures: MessageReader<AssetLoadFailedEvent<MapFolderAsset>>, // 缺失与损坏需区分处理。
    mut browser: ResMut<MapBrowserState>, // 原子替换有效树或保留旧树并禁写。
    mut editor: ResMut<MapEditorState>,  // F12打开时提供直接可见诊断。
) {
    let Some(handle) = handle else {
        return; // Startup之前没有目标资产身份。
    };
    let target_id = handle.handle.id(); // 只处理唯一编辑器归类句柄。
    let changed = events.read().any(|event| {
        event.is_added(target_id)
            || event.is_modified(target_id)
            || event.is_loaded_with_dependencies(target_id)
    });
    let initial_asset_ready =
        !browser.initial_load_resolved && assets.get(&handle.handle).is_some(); // Startup内已完成加载时AssetEvent可能早于本系统读游标。
    if (changed || initial_asset_ready) && !persistence.busy() {
        if let Some(asset) = assets.get(&handle.handle) {
            if browser.source_bytes.as_ref() != Some(&asset.source_bytes)
                && browser.selection_count() > 1
            {
                let selected = browser.selected_item.clone();
                browser.select_only(selected); // 外部分类换代收为单选；本次批量自写的同字节重载保留整组选集。
            }
            browser.document = asset.document.clone(); // 完整有效文档一次替换。
            browser.source_bytes = Some(asset.source_bytes.clone()); // 更新精确并发基线。
            browser.initial_load_resolved = true; // 后续稳定帧只依赖资产事件，不比较整份源字节。
            browser.document_revision = browser.document_revision.wrapping_add(1); // 作废旧索引后台结果。
            reconcile_tree_session(&mut browser); // 保留仍有效展开分支，清理外部删除的稳定身份。
            browser.write_blocked = None; // 成功加载重新开放结构写入。
            browser.last_error = None; // 清除旧损坏或并发诊断。
        }
    }

    for failure in failures.read() {
        if failure.id != target_id || persistence.busy() {
            continue; // 无关资产和在途自写事件不影响乐观状态。
        }
        let failure_path = failure.path.to_string(); // 借用前固定诊断路径。
        apply_map_folder_load_failure(&failure.error, &failure_path, &mut browser, &mut editor);
    }

    if !browser.initial_load_resolved
        && !persistence.busy()
        && let Some(LoadState::Failed(error)) = asset_server.get_load_state(target_id)
    {
        apply_map_folder_load_failure(
            error.as_ref(),
            MAP_FOLDER_ASSET_PATH,
            &mut browser,
            &mut editor,
        ); // Startup内已完成失败时，事件早到也能一次收口为缺失或损坏语义。
    }
}

/// 乐观采用一份结构修改并启动唯一原子保存任务。
pub(crate) fn begin_map_folder_save(
    browser: &mut MapBrowserState,          // 当前有效树、选择与展开状态。
    persistence: &mut MapFolderPersistence, // single-flight任务和回滚快照。
    editor: &mut MapEditorState,            // 地图按钮目标与状态回显。
    next_document: MapFolderDocument,       // 调用方已应用单次语义修改的完整目标。
    next_selected_item: Option<MapBrowserItem>, // 操作后的高亮项。
    next_expanded_folder_ids: Option<BTreeSet<String>>, // 需要同步展开父链或移除失效目录时提供完整会话集合。
) -> Result<(), String> {
    if persistence.busy() {
        return Err("地图归类正在保存，请等待本次写入完成".to_owned()); // 禁止第二个结构修改覆盖回滚槽。
    }
    if let Some(reason) = browser.write_blocked.as_deref() {
        return Err(format!("地图归类暂不可写：{reason}")); // 损坏或未加载原件不能被覆盖。
    }
    let encoded_check = encode_folder_document(&next_document)?; // 启动任务前同步做固定小预算结构校验。
    drop(encoded_check); // 后台仍重新编码，避免主线程持有重复大字节。
    persistence.ticket = persistence.ticket.wrapping_add(1); // 为不可取消任务生成新身份。
    persistence.rollback_document = Some(browser.document.clone()); // 保存失败恢复完整旧树。
    persistence.rollback_selected_item = Some(browser.selected_item.clone()); // 恢复旧高亮。
    persistence.rollback_expanded_folder_ids = Some(browser.expanded_folder_ids.clone()); // 恢复操作前展开分支。
    persistence.rollback_source_bytes = Some(browser.source_bytes.clone()); // 恢复并发基线。
    persistence.rollback_map_selection_id = Some(editor.map_list_selection_id.clone()); // 恢复地图按钮目标。
    let expected_bytes = browser.source_bytes.clone(); // 后台冻结操作前精确磁盘字节。
    browser.document = next_document.clone(); // 先乐观更新内存，让左栏立即响应。
    browser.select_only(next_selected_item); // 单项创建/重命名/删除原子切换焦点，不能继承旧整组选集。
    if let Some(expanded_folder_ids) = next_expanded_folder_ids
        && browser.expanded_folder_ids != expanded_folder_ids
    {
        browser.expanded_folder_ids = expanded_folder_ids; // 创建和删除目录同步更新会话树可见性。
        browser.expansion_revision = browser.expansion_revision.wrapping_add(1); // 后台投影拒收旧展开集合。
    }
    browser.document_revision = browser.document_revision.wrapping_add(1); // 新索引拒收旧任务。
    let document_revision = browser.document_revision; // 后台结果必须与这次乐观文档修订同时匹配。
    editor.map_list_selection_id = match browser.selected_item.as_ref() {
        Some(MapBrowserItem::Map(map_id)) => Some(map_id.clone()), // 地图高亮继续兼容既有进入/删除链。
        _ => None,                                                 // 文件夹选择不能误删旧地图。
    };
    editor.status = "正在保存地图归类…".to_owned(); // 明确single-flight期间的短暂限制。
    let ticket = persistence.ticket; // 结果必须匹配当前任务身份。
    let asset_root = workspace_asset_root(); // 后台只拥有绝对项目路径，不读取ECS。
    persistence.task = Some(IoTaskPool::get().spawn(async move {
        (
            ticket,
            document_revision,
            write_document_if_unchanged(&asset_root, expected_bytes, next_document),
        )
    })); // 主线程绝不等待文件系统。
    Ok(())
}

/// 非阻塞采用归类保存结果；失败恢复操作前目录树、选择和地图归属。
pub(crate) fn poll_map_folder_save(
    mut persistence: ResMut<MapFolderPersistence>, // 唯一在途任务和回滚槽。
    mut browser: ResMut<MapBrowserState>,          // 成功更新基线，失败完整恢复。
    mut editor: ResMut<MapEditorState>,            // 恢复地图选择并回显结果。
    asset_server: Res<AssetServer>,                // 成功或并发失败后显式请求重新加载。
) {
    let Some(task) = persistence.task.as_mut() else {
        return; // 稳定帧没有轮询成本之外的工作。
    };
    let Some((ticket, document_revision, result)) = check_ready(task) else {
        return; // Update只探测，不阻塞等待IO线程。
    };
    persistence.task = None; // 先释放single-flight槽，随后统一清理回滚字段。
    if ticket != persistence.ticket || document_revision != browser.document_revision {
        browser.write_blocked = Some("地图归类状态在保存期间发生换代，正在重新加载".to_owned()); // 身份不符时不猜测采用或回滚哪份树。
        browser.last_error = browser.write_blocked.clone(); // 保留可见诊断直到完整有效资产重新采用。
        editor.status = "地图归类保存结果已过期，正在重新加载磁盘文件".to_owned();
        asset_server.reload(MAP_FOLDER_ASSET_PATH); // 磁盘任务可能已经成功，统一以真实文件重新收口。
        persistence.rollback_document = None;
        persistence.rollback_selected_item = None;
        persistence.rollback_expanded_folder_ids = None;
        persistence.rollback_source_bytes = None;
        persistence.rollback_map_selection_id = None;
        return; // 迟到任务绝不覆盖较新的主世界状态。
    }
    match result {
        Ok(snapshot) => {
            browser.document = snapshot.document; // 与磁盘成功值保持完全一致。
            browser.source_bytes = Some(snapshot.source_bytes); // 建立下一次精确并发基线。
            browser.initial_load_resolved = true; // 成功写入已经建立明确磁盘基线。
            browser.write_blocked = None; // 正常写入保持结构入口开放。
            browser.last_error = None; // 清除旧瞬时IO诊断。
            editor.status = "地图归类已保存".to_owned(); // 归类不改变地图dirty或撤销栈。
            asset_server.reload(MAP_FOLDER_ASSET_PATH); // 不依赖Windows文件监听合并时序。
        }
        Err(error) => {
            if let Some(document) = persistence.rollback_document.take() {
                browser.document = document; // 恢复操作前完整目录树和地图归属。
            }
            if let Some(selected_item) = persistence.rollback_selected_item.take() {
                browser.select_only(selected_item); // 恢复旧行选择。
            }
            if let Some(expanded_folder_ids) = persistence.rollback_expanded_folder_ids.take() {
                browser.expanded_folder_ids = expanded_folder_ids; // 恢复操作前全部展开分支。
                browser.expansion_revision = browser.expansion_revision.wrapping_add(1); // 作废失败操作产生的投影。
            }
            if let Some(source_bytes) = persistence.rollback_source_bytes.take() {
                browser.source_bytes = source_bytes; // 恢复旧并发基线。
            }
            if let Some(map_id) = persistence.rollback_map_selection_id.take() {
                editor.map_list_selection_id = map_id; // 恢复旧地图按钮目标。
            }
            browser.document_revision = browser.document_revision.wrapping_add(1); // 作废乐观索引结果并重建回滚值。
            let message = match error {
                MapFolderWriteFailure::ConcurrentChange(_message) => {
                    browser.write_blocked =
                        Some("检测到地图归类文件被外部修改，正在重新加载".to_owned()); // 重载成功前禁止再写。
                    asset_server.reload(MAP_FOLDER_ASSET_PATH); // 尽快取得外部最新有效文档。
                    "地图归类保存已取消：文件被外部修改".to_owned()
                }
                MapFolderWriteFailure::Io(message) => {
                    browser.write_blocked = None; // 普通瞬时IO错误允许作者原样重试。
                    format!("地图归类保存失败：{message}")
                }
            };
            browser.last_error = Some(message.clone()); // 状态栏之外保留结构域诊断。
            editor.status = message; // 作者立即看到回滚原因。
        }
    }
    persistence.rollback_document = None; // 成功与失败都清空旧快照。
    persistence.rollback_selected_item = None; // 不允许下一任务误用旧选择。
    persistence.rollback_expanded_folder_ids = None; // 不允许下一任务误用旧展开集合。
    persistence.rollback_source_bytes = None; // 不允许下一任务误用旧基线。
    persistence.rollback_map_selection_id = None; // 不允许下一任务误用旧地图目标。
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap; // 构造最小归属文档。

    use super::*; // 直接验证磁盘并发门与跨事务回滚。
    use crate::types::MapFolderRecord; // 测试目标文件夹记录。

    /// 在独立临时assets根创建归类事务测试目录。
    fn temporary_asset_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "bevy-learn5-map-folders-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("测试时钟应晚于Unix纪元")
                .as_nanos(),
        )) // 唯一后缀避免并行测试互相覆盖。
    }

    /// 构造一层合法文件夹文档。
    fn folder_document() -> MapFolderDocument {
        MapFolderDocument {
            folders: vec![MapFolderRecord {
                id: "FOLDER-1".to_owned(),           // 稳定测试身份。
                display_name: "河南区域".to_owned(), // 作者可读名称。
                parent_id: None,                     // 根目录直接子项。
            }],
            map_folders: BTreeMap::new(), // 初始没有地图归属。
            ..MapFolderDocument::default()
        }
    }

    #[test]
    fn moved_folder_subtree_round_trips_through_the_original_checked_writer() {
        let root = temporary_asset_root("folder-subtree");
        let path = folder_file_path(&root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut document = folder_document();
        document.folders.extend([
            MapFolderRecord {
                id: "FOLDER-2".into(),
                display_name: "子目录".into(),
                parent_id: Some("FOLDER-1".into()),
            },
            MapFolderRecord {
                id: "FOLDER-3".into(),
                display_name: "目标目录".into(),
                parent_id: None,
            },
        ]);
        document
            .map_folders
            .insert("MAP-1".into(), "FOLDER-2".into());
        let original = encode_folder_document(&document).unwrap();
        std::fs::write(&path, &original).unwrap();
        assert!(
            crate::map_browser::helpers::move_folder(&mut document, "FOLDER-1", Some("FOLDER-3"))
                .unwrap()
        );
        let saved = write_document_if_unchanged(&root, Some(original), document.clone()).unwrap();
        assert_eq!(
            decode_folder_document(&std::fs::read(&path).unwrap()).unwrap(),
            document
        );
        assert_eq!(
            saved.document.map_folders.get("MAP-1").map(String::as_str),
            Some("FOLDER-2")
        );
        assert_eq!(
            saved.document.folders[1].parent_id.as_deref(),
            Some("FOLDER-1")
        );
        assert!(root.starts_with(std::env::temp_dir())); // 只清理本用例生成的独立临时资产根。
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn checked_write_rejects_external_change_without_overwriting_it() {
        let root = temporary_asset_root("concurrent"); // 本用例独立作者根。
        let path = folder_file_path(&root); // 精确归类文件目标。
        std::fs::create_dir_all(path.parent().expect("测试路径有父目录")).expect("应创建测试目录");
        let original = encode_folder_document(&folder_document()).expect("测试文档合法");
        std::fs::write(&path, &original).expect("应写入原始文件");
        let external = b"external-change".to_vec(); // 模拟另一编辑器修改文件。
        std::fs::write(&path, &external).expect("应写入外部变化");
        let result = write_document_if_unchanged(&root, Some(original), folder_document());
        assert!(matches!(
            result,
            Err(MapFolderWriteFailure::ConcurrentChange(_))
        )); // 精确字节不符必须拒绝。
        assert_eq!(std::fs::read(&path).expect("外部文件应保留"), external); // 绝不覆盖损坏或并发内容。
        std::fs::remove_dir_all(&root).expect("应清理测试目录"); // 仅删除本测试唯一目录。
    }

    #[test]
    fn map_assignment_mutation_can_publish_and_restore_original_bytes() {
        let root = temporary_asset_root("rollback"); // 本用例独立作者根。
        let path = folder_file_path(&root); // 精确归类文件目标。
        std::fs::create_dir_all(path.parent().expect("测试路径有父目录")).expect("应创建测试目录");
        let original = encode_folder_document(&folder_document()).expect("测试文档合法");
        std::fs::write(&path, &original).expect("应写入原始文件");
        let map_id = MapId::parse("HN-01").expect("测试MapId合法");
        let mutation = prepare_map_assignment_mutation(
            &root,
            Some(original.clone()),
            &map_id,
            Some("FOLDER-1"),
        )
        .expect("应准备归类改写")
        .expect("归属变化应产生事务");
        mutation.publish_checked().expect("基线一致时应发布");
        assert_ne!(std::fs::read(&path).expect("新文件存在"), original); // 发布确实写入地图归属。
        mutation.rollback().expect("后续地图事务失败时应恢复");
        assert_eq!(std::fs::read(&path).expect("旧文件恢复"), original); // 回滚恢复精确原字节。
        std::fs::remove_dir_all(&root).expect("应清理测试目录"); // 仅删除本测试唯一目录。
    }
}
