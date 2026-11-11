//! 从作者引用定位文件的会话事务：等待时保留旧选择，完成后一次发布目录、分页和预览目标。

use super::paths::normalize_asset_reference; // 路径规范化不读取文件或提交输入框。
use super::resources::EditorAssetCatalog; // 当前目录、唯一 IO 槽和未完成定位意图。
use super::types::{AssetRevealRequest, AssetScanRequest, AssetScanResult}; // 后台与前台共享完整身份。
use crate::resources::SceneEditorState; // 只更新选中资产与提示，不触碰场景 dirty 或作者动作。
use crate::types::EditorAssetEntry; // 旧目录只转移所有权，随后由后台释放。
use crate::ui::sidebars::{EditorNavigatorTab, SceneEditorSidebarState}; // 定位只揭示左侧资产浏览器。

/// 显式定位一份引用；现有目录与预览保持不变，直到后台确认完整路径确实存在。
pub fn request_asset_reveal(
    catalog: &mut EditorAssetCatalog, // 新请求与现有扫描共享同一单航班槽。
    editor: &mut SceneEditorState,      // 读取当前选择和场景身份，仅写轻量浏览状态。
    sidebar: &mut SceneEditorSidebarState, // 左侧资产页立即可见；采用前保留当前右侧属性。
    raw_path: &str,                   // 调用方可以传输入缓冲而不提交字段。
) -> Result<(), String> {
    request_reveal(catalog, editor, sidebar, raw_path, false)
}

/// 收藏目录验证成功前不改目录、选择或当前预览。
pub(crate) fn request_asset_folder(
    catalog: &mut EditorAssetCatalog, editor: &mut SceneEditorState,
    sidebar: &mut SceneEditorSidebarState, raw_path: &str,
) -> Result<(), String> {
    request_reveal(catalog, editor, sidebar, raw_path, true)
}

fn request_reveal(
    catalog: &mut EditorAssetCatalog, editor: &mut SceneEditorState,
    sidebar: &mut SceneEditorSidebarState, raw_path: &str, is_directory: bool,
) -> Result<(), String> {
    if !editor.open || editor.close_requested || editor.pending_scene_transition.is_some() {
        return Err("请在编辑会话中定位资产；关闭或切换场景期间不接受新请求".to_owned()); // 不让隐藏会话排入迟到导航。
    }
    // 工程目录跨入登记资产根时需要显式打开根目录；空文件引用仍必须由原规范化器拒绝。
    let reference = if is_directory && raw_path.is_empty() { String::new() }
        else { normalize_asset_reference(raw_path)? }; // 失败不改变旧请求、目录、选择或任何作者数据。
    let file_path = reference.split('#').next().unwrap_or_default().to_owned(); // 只定位 # 前的所属文件。
    if is_directory && reference.contains('#') { return Err("目录不能携带子资产标签".into()); }
    let folder = if is_directory { file_path.clone() } else {
        file_path.rsplit_once('/').map_or("", |(folder, _)| folder).to_owned()
    }; // 目录扫描自身，文件精确取直接父目录，不全库搜索同名文件。
    let revision = catalog.requested_revision.wrapping_add(1); // 最新意图代替尚未启动的旧定位，但不丢弃在途 IO 句柄。
    catalog.reveal = Some(AssetRevealRequest {
        is_directory,
        revision,                                          // 目录修订与定位票据同时推进。
        reference: reference.clone(),                      // 完成提示保留 glTF 子资产标签。
        file_path, // 浏览器选中文件不能携带 #Mesh 或 #Material。
        folder,    // 后台只扫描这一个直接父目录。
        original_folder: catalog.current_folder.clone(), // 等待期间保持当前已显示目录。
        original_scroll_intent_revision: catalog.scroll_intent_revision, // 手动滚动优先于尚未完成的目录定位。
        original_selection: editor.selected_asset.clone(), // 文件选择变化拥有优先权。
        scene_id: editor.selected_scene_id.clone(), // 切换场景后的同名文件不能恢复旧会话请求。
    });
    catalog.requested_revision = revision; // 后续 Tasks 阶段按修订触发唯一 IO 任务。
    sidebar.navigator = EditorNavigatorTab::Assets; // 只定位左侧浏览器，不碰作者工具或右侧属性。
    editor.status = if is_directory {
        format!("正在打开目录 assets/{reference}；验证完成前保留当前目录与预览")
    } else { format!("正在定位 assets/{reference}；文件验证完成前保留当前预览") }; // 作者可以继续浏览或取消。
    Ok(()) // 提交的是浏览意图，不是预设保存或场景修改。
}

/// 取消自己的定位代次；后台任务仍占用槽位直到真正完成，不能伪取消后并发扫描。
pub(super) fn cancel_asset_reveal(catalog: &mut EditorAssetCatalog) {
    let Some(request) = catalog.reveal.take() else {
        return; // 没有定位时保持普通目录浏览修订不变。
    };
    if request.revision == catalog.requested_revision {
        catalog.requested_revision = catalog.requested_revision.wrapping_add(1); // 原任务即使随后完成也必为旧身份。
        catalog.applied_revision = catalog.requested_revision; // 已显示旧目录仍有效，不为取消重复扫描。
    }
}

/// 原目录、页、选择、场景或可见会话变化都表示用户的新意图，迟到结果必须安静失效。
pub(super) fn expire_asset_reveal(
    catalog: &mut EditorAssetCatalog, // 只取消未采用定位，不释放或重建在途任务。
    editor: &SceneEditorState,          // 作者会话仅供身份核对。
    sidebar: &SceneEditorSidebarState,  // 离开资产页后不能被任务重新抢回。
) {
    let expired = catalog.reveal.as_ref().is_some_and(|request| {
        !editor.open // 关闭 F12 的帧立即撤销选择回跳资格。
            || editor.close_requested // 关闭联合门等待期间也不继续导航。
            || editor.pending_scene_transition.is_some() // 切换场景请求优先于旧资产定位。
            || !sidebar.assets_browser_visible() // 用户离开左侧资产页后，不采用旧导航意图。
            || request.revision != catalog.requested_revision // 上级、刷新与新目录修订会撤销旧请求。
            || request.original_folder != catalog.current_folder // 防御外部只改目录而未推进修订。
            || request.original_scroll_intent_revision != catalog.scroll_intent_revision // 纯布局换行或池扩容不会撤销请求。
            || request.original_selection != editor.selected_asset // 其他资产选择不得被覆盖。
            || request.scene_id != editor.selected_scene_id // 防止跨作者场景采用。
    });
    if expired {
        cancel_asset_reveal(catalog); // 仅处理一个轻量请求，稳定帧无目录扫描。
    }
}

/// 请求身份必须同时匹配修订、父目录与所属文件；普通扫描也不接受已换目录的结果。
fn scan_is_current(request: &AssetScanRequest, catalog: &EditorAssetCatalog) -> bool {
    if request.revision != catalog.requested_revision {
        return false; // 任何目录刷新或取消后的迟到结果不能发布。
    }
    match (&request.reveal_file, &catalog.reveal) {
        (None, Some(reveal)) if reveal.is_directory => {
            reveal.revision == request.revision && reveal.folder == request.folder
        }
        (Some(file_path), Some(reveal)) => {
            reveal.revision == request.revision // 定位票据必须还是当前意图。
                && reveal.file_path == *file_path // 不能因同目录同名别名而误选。
                && reveal.folder == request.folder // 防御父目录已被外部重定向。
        }
        (None, None) => request.folder == catalog.current_folder, // 普通目录结果不能覆盖等待中的反向定位。
        _ => false, // 普通扫描与定位扫描不是互相可替代的结果。
    }
}

/// 接收一次后台完成值；目录、目标页和 selected_asset 只在完整成功后一起替换。
pub(super) fn adopt_asset_scan(
    catalog: &mut EditorAssetCatalog, // 唯一结果采用点拥有当前目录。
    editor: &mut SceneEditorState,      // 只改浏览选择和诊断，预览由现有选择系统启动。
    sidebar: &SceneEditorSidebarState,  // 再核对本帧最终左侧导航意图。
    completed: AssetScanResult,       // 后台已经完成排序、全路径搜索和所有文件 IO。
) -> bool {
    // 只有成功定位才返回true，调用者据此切换预览；普通刷新不抢作者焦点。
    expire_asset_reveal(catalog, editor, sidebar); // 即使任务恰在换页或关闭帧完成也先失效。
    if !scan_is_current(&completed.request, catalog) {
        if let Ok(contents) = completed.result {
            retire_entries(catalog, contents.entries); // 迟到大结果也不能在主线程整表析构。
        }
        return false; // 不覆盖新状态文字、页码或当前预览。
    }
    catalog.applied_revision = completed.request.revision; // 成功或失败都结束本轮，显式重试才再扫描。
    let contents = match completed.result {
        Ok(contents) => contents, // 背景结果完整，仍需常数次验证目标索引。
        Err(error) => {
            editor.status = if catalog.reveal.take().is_some() {
                format!("资产定位失败：{error}") // 原选择、目录与预览继续保留。
            } else {
                error // 普通浏览沿用现有具体目录错误提示。
            };
            catalog.feedback = Some(super::types::AssetCatalogFeedback {
                revision: completed.request.revision,
                result: Err(editor.status.clone()),
            }); // 只在既有身份门之后记录，不改变失败保留原目录及预览的语义。
            return false; // 定位失败不能改变当前预览来源。
        }
    };
    let reveal_index = if let Some(file_path) = completed.request.reveal_file.as_deref() {
        match contents.reveal_index.filter(|index| {
            contents.entries.get(*index).is_some_and(|entry| {
                !entry.is_directory && entry.asset_path == file_path // 用完整路径复核后台索引，绝不回退搜索同名项。
            })
        }) {
            Some(index) => Some(index), // 后台提供全目录索引，供分页和一次性滚入可见区域共用。
            None => {
                editor.status =
                    format!("资产定位失败：后台未返回 assets/{file_path} 的有效文件位置"); // 损坏完成值不发布半结果。
                catalog.reveal = None; // 失败已经结束，等待作者显式重试。
                catalog.feedback = Some(super::types::AssetCatalogFeedback {
                    revision: completed.request.revision,
                    result: Err(editor.status.clone()),
                });
                retire_entries(catalog, contents.entries); // 完整私有候选由后台释放。
                return false; // 损坏候选不触发右栏预览切换。
            }
        }
    } else {
        None // 普通浏览没有新的定位滚动意图。
    };
    catalog.feedback = Some(super::types::AssetCatalogFeedback {
        revision: completed.request.revision,
        result: Ok(()),
    }); // 完整结果才发布成功证据；扫描、排序和原子采用链不变。
    let old_entries = std::mem::replace(&mut catalog.entries, contents.entries); // 所有权交换是常数成本，避免复制或同步析构全目录。
    retire_entries(catalog, old_entries); // 下一次唯一 IO 任务先释放旧缓存，再扫描最新请求。
    let folder_changed = catalog.adopted_folder.as_deref() != Some(completed.request.folder.as_str());
    catalog.adopted_folder = Some(completed.request.folder.clone());
    catalog.current_folder = completed.request.folder; // 只有此时定位父目录才真正成为公开目录。
    catalog.page = 0; // 旧兼容字段不再控制可见项；定位由绝对下标滚动处理。
    catalog.visible_start = 0;
    catalog.visible_count = 0;
    catalog.viewport_revision = catalog.viewport_revision.wrapping_add(1);
    catalog.scroll_anchor = None;
    if let Some(index) = reveal_index {
        catalog.request_scroll_anchor(index);
        if let Some(anchor) = catalog.scroll_anchor.as_mut() { anchor.require_selection = true; }
    } else if folder_changed { catalog.request_scroll_anchor(0); } // 新目录从首项开始，刷新同一目录保留手动位置。
    if let Some(reveal) = catalog.reveal.take() {
        catalog.revealed_revision = Some(reveal.revision); // 目录也记录真实成功意图，供工程树处理同帧完成。
        catalog.revealed_entry = reveal_index.map(|index| (reveal.revision, index)); // 仅完整成功发布一次性UI滚动目标。
        if reveal.is_directory {
            editor.status = format!("已打开目录 assets/{}：{}项", reveal.folder, catalog.entries.len());
            return false; // 目录导航不更换文件或抢回其预览焦点。
        }
        editor.selected_asset = Some(reveal.file_path); // 现有资产预览系统据此加载图片或 glTF Scene0。
        editor.status = format!(
            "已定位 assets/{}；已选中文件并在右侧显示属性与预览",
            reveal.reference
        ); // 子资产标签仍供作者核对。
    } else {
        editor.status = format!(
            "资产目录 assets/{}：{}项",
            catalog.current_folder,
            catalog.entries.len()
        ); // 普通浏览保持原提示。
    }
    reveal_index.is_some() // 即使定位与旧选择同路径，也报告一次明确的成功浏览意图。
}

/// 唯一完成值至多生成一份退役目录；下一任务启动前会移走它，稳定帧不遍历条目。
fn retire_entries(catalog: &mut EditorAssetCatalog, entries: Vec<EditorAssetEntry>) {
    if entries.is_empty() {
        return; // 空向量无需增加一次后台清理任务。
    }
    debug_assert!(catalog.retired_entries.is_none()); // single-flight 槽和采用点保证不会覆盖尚未释放的大目录。
    catalog.retired_entries = Some(entries); // 大字符串和 Vec 析构统一留到 IO 线程。
}

#[cfg(test)]
mod tests; // 路径跨目录、跨页、缺失与迟到取消全部使用独立夹具。
#[cfg(test)]
mod adopted_folder_tests; // 区分提前发布的导航意图与真正采用目录，防止等高新目录沿用旧深处偏移。
