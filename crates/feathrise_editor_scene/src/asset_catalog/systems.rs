//! 资产目录有界后台任务、非阻塞采用与动态行点击。

use std::path::PathBuf; // 后台任务冻结本编辑实例的工程资产根。

use bevy::prelude::*; // ECS资源、Interaction与日志。
use bevy::tasks::IoTaskPool; // 文件系统遍历使用IO池而非帧主线程。
use bevy::tasks::futures::check_ready; // Update只探测任务完成状态，不等待IO。
use feathrise_scene_content::SceneAssetKind; // 点击不同资产用途选择对应工具。

use super::reveal::{adopt_asset_scan, cancel_asset_reveal, expire_asset_reveal}; // 定位生命周期与同身份原子采用。
use super::scan::scan_asset_request; // 纯后台扫描、排序和精确目标定位。
use super::types::AssetScanRequest; // 后台冻结完整目录身份，不再只返回一个松散修订。
use crate::components::{SceneEditorAssetRow, AssetRowBinding}; // 固定行保存当前条目索引。
use crate::resources::{EditorAssetCatalog, SceneEditorLayoutState, SceneEditorNumericScrubMouseGuard, SceneEditorState}; // 当前目录、鼠标所有权、在途任务和选择状态。
use crate::ui::sidebars::SceneEditorSidebarState; // 用户离开左侧资产页后旧定位不能抢回。

/// 返回当前工作树唯一assets绝对路径。
fn workspace_asset_root() -> PathBuf {
    feathrise_editor_core::workspace_asset_root()
}

/// 插件启动后请求一次根目录扫描，真正IO仍由后续系统启动后台任务。
pub(crate) fn request_initial_asset_scan(mut catalog: ResMut<EditorAssetCatalog>) {
    catalog.requested_revision = catalog.requested_revision.wrapping_add(1);
}

/// 没有在途任务且存在新请求时启动一个IO任务。
pub(crate) fn start_asset_scan(
    editor: Res<SceneEditorState>, // 关闭或切换场景时只允许释放旧目录，不启动新扫描。
    sidebar: Res<SceneEditorSidebarState>, // 定位仅在仍显示资产页时有效。
    mut catalog: ResMut<EditorAssetCatalog>, // 唯一在途槽同时为扫描与旧目录析构施加背压。
    documents: Option<Res<crate::ui::asset_preview::DocumentState>>, // 浏览、预览和收藏使用同一资产根。
) {
    expire_asset_reveal(&mut catalog, &editor, &sidebar); // 新用户意图先撤销旧请求，不假定丢 Task 会抢占 IO。
    if catalog.task.is_some() {
        return; // 旧任务真正完成并轮询前，绝不并发第二次目录扫描。
    }
    let source_available = catalog.hosted_source.as_ref().is_none_or(|(_, root)| root.is_some());
    let should_scan = editor.open
        && !editor.close_requested // 关闭联合门等待时不再增加资产工作。
        && source_available // 无工程时绝不读取编辑器自身的作者资产。
        && catalog.requested_revision != catalog.applied_revision; // 稳定帧或本轮失败不自动重试。
    if !should_scan && catalog.retired_entries.is_none() && catalog.retired_source.is_none() {
        return; // 无请求、无退役结果时完全不启动后台任务。
    }
    let request = should_scan.then(|| AssetScanRequest {
        revision: catalog.requested_revision, // 后台完成时必须仍为最新修订。
        folder: catalog.reveal.as_ref().map_or_else(
            || catalog.current_folder.clone(), // 普通浏览沿用当前目录。
            |reveal| reveal.folder.clone(),    // 定位先扫描目标父目录，公开目录暂时不动。
        ),
        reveal_file: catalog
            .reveal
            .as_ref()
            .filter(|reveal| !reveal.is_directory)
            .map(|reveal| reveal.file_path.clone()), // 精确文件搜索只在后台执行。
    });
    let retired = catalog.retired_entries.take(); // 旧 Vec O(1) 交给工作线程，主线程不析构全目录。
    let retired_source = catalog.retired_source.take();
    let root = match &catalog.hosted_source {
        Some((_, root)) => root.clone().unwrap_or_default(),
        None => documents.as_ref().map_or_else(workspace_asset_root, |documents| documents.asset_root().to_path_buf()),
    }; // canonicalize 留在 IO 任务中；宿主空根不会走扫描分支。
    catalog.task = Some(IoTaskPool::get().spawn(async move {
        drop(retired); // 任意长旧目录和迟到结果在后台释放，保持单一在途预算。
        drop(retired_source);
        request.map(|request| scan_asset_request(root, request)) // None 是纯退役完成回执，不触发目录采用。
    }));
}

/// 非阻塞采用完成扫描；过期修订直接丢弃并由下帧启动最新请求。
pub(crate) fn poll_asset_scan(
    mut catalog: ResMut<EditorAssetCatalog>, // 只有此系统消费唯一 Task 完成值。
    mut editor: ResMut<SceneEditorState>,      // 完整定位成功后写 selected_asset 触发现有预览。
    mut sidebar: ResMut<SceneEditorSidebarState>, // 与本帧最终左侧导航核对取消责任。
    mut preview_focus: Option<ResMut<crate::ui::asset_preview::AssetPreviewFocus>>, // 成功定位与点击文件具有相同预览意图。
    workspace: Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>, // 只记录当前标签，不读取作者内容。
    mut visits: Option<ResMut<Messages<crate::ui::asset_shortcuts::AssetVisited>>>,
) {
    expire_asset_reveal(&mut catalog, &editor, &sidebar); // 任务未完成也必须及时失效关闭或手动导航意图。
    let Some(task) = catalog.task.as_mut() else {
        return;
    };
    let Some(completed) = check_ready(task) else {
        return; // 没有完成就立即返回，不等待目录 IO。
    };
    catalog.task = None; // 只有 check_ready 给出完成值才真正释放在途槽。
    if let Some(completed) = completed {
        let revision = completed.request.revision;
        let folder = completed.request.folder.clone();
        let revealed = adopt_asset_scan(&mut catalog, &mut editor, &sidebar, completed); // 身份完整且定位存在时才原子发布目录、页和预览文件。
        if editor.open && !editor.close_requested && editor.pending_scene_transition.is_none()
            && sidebar.assets_browser_visible()
            && catalog.feedback.as_ref().is_some_and(|feedback| feedback.revision == revision && feedback.result.is_ok())
            && catalog.applied_revision == revision && revision == catalog.requested_revision
            && let Some(visits) = visits.as_mut()
        {
            if revealed && let Some(path) = editor.selected_asset.as_ref() {
                visits.write(crate::ui::asset_shortcuts::AssetVisited { path: path.clone(), is_directory: false });
            } else if !folder.is_empty() {
                visits.write(crate::ui::asset_shortcuts::AssetVisited { path: folder, is_directory: true });
            }
        } // 只记录真正成功采用的访问，缺失、取消和迟到结果不能进入历史。
        if revealed {
            sidebar.reveal_asset_properties(); // 文件身份完整采用后才切换右侧属性，目录或失败不抢属性。
            if let (Some(focus), Some(workspace)) = (preview_focus.as_mut(), workspace.as_ref()) {
                focus.browse(workspace.active_tab); // 同路径定位也明确取回右侧预览；失败或过期结果绝不抢面板。
            }
        }
    }
}

/// 点击目录进入下一层；点击文件按用途选择，不允许把音频等未知文件刷入场景。
pub(crate) fn handle_asset_row_clicks(
    rows: Query<(&feathrise_editor_ui::EditorButtonClick, &SceneEditorAssetRow, Option<&AssetRowBinding>), Changed<feathrise_editor_ui::EditorButtonClick>>,
    viewport: crate::ui::rows::AssetViewport,
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>, // 数值和弦期间只推进点击查询的Changed tick。
    mut catalog: ResMut<EditorAssetCatalog>,
    mut editor: ResMut<SceneEditorState>,
    mut sidebar: Option<ResMut<SceneEditorSidebarState>>, // 原最小目录夹具可省略；正式UI必须按左侧导航拒绝旧按下。
    layout: Option<Res<SceneEditorLayoutState>>,
    mut preview_focus: Option<ResMut<crate::ui::asset_preview::AssetPreviewFocus>>, // 独立目录测试可不安装预览；正式编辑器始终共享此资源。
    workspace: Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>, // 浏览归属于当前标签，保留其未保存草稿。
    time: Option<Res<Time<Real>>>, // 编辑器暂停不影响双击时间窗；无时钟的最小目录测试仍可单击。
    mut opens: Option<ResMut<Messages<crate::ui::asset_preview::OpenAssetPreview>>>,
    mut visits: Option<ResMut<Messages<crate::ui::asset_shortcuts::AssetVisited>>>,
    hosted: Option<Res<crate::workbench::WorkbenchModel>>,
) {
    if numeric_scrub_mouse.blocked() || !editor.open || editor.close_requested
        || hosted.as_ref().is_some_and(|model| !model.visible || model.modal || model.busy || model.asset_root.is_none())
        || sidebar.as_ref().is_some_and(|sidebar| !sidebar.assets_browser_visible())
        || layout.as_ref().is_some_and(|layout| !layout.content_shown())
    {
        catalog.last_click = None;
        return; // 数值拖动和关闭期间推进 Changed tick，但不补选、导航或取消新请求。
    }
    if !viewport.is_current(&catalog) { catalog.last_click = None; return; }
    for (interaction, row, binding) in &rows {
        if !interaction.is_clicked() {
            continue;
        }
        let Some(index) = row.entry_index else {
            continue;
        };
        if binding.is_some_and(|binding| binding.revision != catalog.applied_revision
            || binding.viewport_revision != catalog.viewport_revision || binding.index != Some(index)
            || row.slot >= catalog.visible_count || index != catalog.visible_start + row.slot) {
            continue; // 滚动窗口、目录或槽位换代后的释放不能改选新文件。
        }
        let Some(entry) = catalog.entries.get(index).cloned() else {
            continue;
        };
        cancel_asset_reveal(&mut catalog); // 主动点击任何真实文件或目录都优先于尚未完成的反向定位。
        if entry.is_directory {
            catalog.last_click = None;
            catalog.current_folder = entry.asset_path;
            catalog.page = 0;
            catalog.requested_revision = catalog.requested_revision.wrapping_add(1);
            editor.status = format!("正在打开 assets/{}", catalog.current_folder);
            continue;
        }
        editor.selected_asset = Some(entry.asset_path.clone());
        if let Some(sidebar) = sidebar.as_mut() {
            sidebar.reveal_asset_properties(); // 单击文件更新右侧属性；不改变工具、场景选择或目录滚动。
        }
        if let Some(visits) = visits.as_mut() {
            visits.write(crate::ui::asset_shortcuts::AssetVisited { path: entry.asset_path.clone(), is_directory: false });
        }
        if let Some(time) = time.as_ref() {
            let now = time.elapsed_secs_f64();
            let double = catalog.last_click.as_ref().is_some_and(|previous| {
                previous.path == entry.asset_path && previous.revision == catalog.applied_revision
                    && previous.viewport_revision == catalog.viewport_revision && (0.0..=0.4).contains(&(now - previous.seconds))
            });
            catalog.last_click = if double { None } else { Some(super::types::TimedAssetFileClick {
                path: entry.asset_path.clone(), revision: catalog.applied_revision, viewport_revision: catalog.viewport_revision, seconds: now,
            }) }; // 双击后第三下重新开始，不把三连击算成连续两次打开。
            if double && let Some(opens) = opens.as_mut() {
                opens.write(crate::ui::asset_preview::OpenAssetPreview { path: entry.asset_path.clone() });
            }
        }
        if let (Some(focus), Some(workspace)) = (preview_focus.as_mut(), workspace.as_ref()) {
            focus.browse(workspace.active_tab); // 包括重选同一GLB；装备/角色编辑页不能吞掉显式文件预览。
        }
        if let Some(format) = super::document_format(&entry.asset_path) {
            editor.status = format!(
                "已选择{}文件{}；可在右侧属性中预览或编辑，Ctrl+S保存当前文件",
                format.label(), entry.asset_path
            );
            continue; // 可编辑文本不能再提示“仅浏览”，也不作为模型放置到场景。
        }
        match entry.kind {
            SceneAssetKind::Scene => {
                editor.status = format!(
                    "已选择场景资产{}；可在植被页新建/编辑预设，或显式点击“摆资产”放置普通实例",
                    entry.asset_path
                );
            }
            SceneAssetKind::Texture => {
                let lower_path = entry.asset_path.to_ascii_lowercase(); // 天空主图与派生IBL图按固定后缀区分用途。
                editor.status = if lower_path.ends_with("_diffuse.ktx2")
                    || lower_path.ends_with("_specular.ktx2")
                {
                    format!(
                        "已选择环境 IBL 派生纹理{}；请改选不带_diffuse或_specular后缀的天空资源主路径",
                        entry.asset_path
                    ) // 派生图仍可浏览，但不能再次作为两件套派生入口。
                } else if lower_path.ends_with(".ktx2") {
                    format!(
                        "已选择KTX2纹理{}；若同目录存在两张IBL派生Cube，请显式点击“设置天空资源”实时预览",
                        entry.asset_path
                    )
                } else {
                    format!(
                        "已选择纹理{}；不会把普通二维纹理当作场景实例或环境 IBL",
                        entry.asset_path
                    )
                };
            }
            SceneAssetKind::Data => {
                editor.status = format!(
                    "已选择二进制数据文件{}；当前显示路径与类型",
                    entry.asset_path
                );
            }
            SceneAssetKind::Unsupported if crate::ui::asset_preview::preview_file_kind(&entry.asset_path)
                == crate::ui::asset_preview::AssetPreviewFileKind::Audio =>
            {
                editor.status = format!("已选择音频{}；可在右侧属性中试听，双击在中央打开", entry.asset_path);
            } // 音频用途只改变浏览提示，不武装场景摆放或修改作者数据。
            SceneAssetKind::Unsupported => {
                editor.status = format!("{}不是可刷场景的模型、纹理或作者数据", entry.asset_path);
            }
        }
    }
}
