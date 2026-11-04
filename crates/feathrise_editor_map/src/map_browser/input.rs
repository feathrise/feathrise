//! 虚拟目录树和地图浏览器输入互斥入口。

use feathrise_editor_ui::EditorButtonClick;
use bevy::prelude::*; // ECS资源、Interaction和鼠标按钮。
use bevy::time::Real; // F12暂停玩法时间后，地图双击仍按真实时间识别。
use bevy::window::{PrimaryWindow, Window}; // 地图和文件夹按下记录使用主窗口物理光标坐标。
use feathrise_map_content::{
    MapCatalogAsset, MapContentRegistry, MapId, MapManifestAsset,
}; // 编辑地图资料必须读取正式清单的只读规格。

use crate::components::{MapEditorActionButton, MapEditorMapRootRow, MapEditorMapRow}; // 虚拟行池、固定根行和静态动作按钮。
use crate::constants::MAP_FOLDER_MAX_FOLDERS; // 创建按钮在打开模态框前执行结构硬预算。
use crate::resources::{
    MapBrowserDragState, MapBrowserIndexState, MapBrowserState, MapCreationTask, MapDeletionTask,
    MapEditorNumericScrubMouseGuard, MapEditorState, MapFolderPersistence,
}; // 导航、任务互斥、拖拽和回滚状态。
use crate::types::{
    EditorAction, MapBrowserDragPress, MapBrowserIndexSignature, MapBrowserItem, MapEditTarget,
    MapFolderDialogMode, MapFolderDialogState,
}; // 文件夹操作与拖拽普通数据身份。
use crate::ui::actions::set_ime_enabled; // 文件夹名称输入复用窗口IME开关规则。
use crate::ui::map_transition::MapTransitionAuthoring;
use crate::ui::rows::request_selected_map_entry; // 地图显式进入继续复用既有切图保护链。 // 加载补齐不阻止浏览，真实编辑与保存继续受保护。

use super::helpers::{folder_by_id, folder_is_empty, selected_container_folder_id}; // 所有结构入口共享同一纯校验语义。

mod dialog; // 名称IME、确认校验与文件夹结构提交。
mod drag; // 物理像素阈值、落点判断与地图归属提交。
mod navigation; // 拖拽期间的真实时间悬停展开与地图树边缘滚动。

pub(crate) use dialog::handle_map_folder_dialog_input;
pub(crate) use drag::{map_browser_general_input_available, update_map_browser_drag};
pub(crate) use navigation::navigate_map_browser_drag;

const MAP_DOUBLE_CLICK_MAX_SECONDS: f64 = 0.36; // 与其他作者列表保持一致的双击窗口。

/// 只保留上一次地图合法松开的稳定身份和目录代次，不缓存整棵树或启动加载任务。
pub(crate) struct MapBrowserClick {
    map_id: MapId,
    seconds: f64,
    projection: (MapBrowserIndexSignature, u64),
}

impl MapBrowserClick {
    /// 独立宿主与一体版本共用合法松开的双击判定；目录、工程换代均打断旧序列。
    pub(crate) fn release(last: &mut Option<Self>, map_id: &MapId,
        projection: &(MapBrowserIndexSignature, u64), seconds: f64) -> bool {
        let previous = last.take();
        let double = seconds.is_finite() && previous.is_some_and(|previous| {
            previous.map_id == *map_id && previous.projection == *projection
                && seconds >= previous.seconds && seconds - previous.seconds <= MAP_DOUBLE_CLICK_MAX_SECONDS
        });
        if !double && seconds.is_finite() {
            *last = Some(Self { map_id: map_id.clone(), seconds, projection: projection.clone() });
        }
        double
    }
}

/// 判断地图创建是否已经冻结请求或占用后台catalog写入任务。
fn map_creation_busy(creation: &MapCreationTask) -> bool {
    creation.task.is_some() || creation.pending_after_save.is_some() // 保存后创建也必须冻结已选虚拟归类上下文。
}

/// 拒绝会改变归类文件结构的操作，并把原因回显给作者。
pub(super) fn structure_change_allowed(
    browser: &MapBrowserState,          // 损坏文件禁写原因。
    persistence: &MapFolderPersistence, // 独立归类保存single-flight。
    creation: &MapCreationTask,         // 新地图冻结已选归类上下文并改写catalog。
    deletion: &MapDeletionTask,         // 地图回收同步改写catalog与归类文件。
) -> Result<(), String> {
    if let Some(reason) = browser.write_blocked.as_deref() {
        return Err(format!("地图归类暂不可修改：{reason}")); // 损坏原件绝不能被覆盖。
    }
    if persistence.busy() {
        return Err("地图归类正在保存，请等待本次写入完成".to_owned()); // 回滚槽只能服务一个结构操作。
    }
    if map_creation_busy(creation) {
        return Err("新地图创建完成前不能修改虚拟文件夹结构".to_owned()); // 创建请求冻结了已选归类身份。
    }
    if deletion.busy() {
        return Err("地图回收事务完成前不能修改虚拟文件夹结构".to_owned()); // 删除必须与归属移除保持一致。
    }
    Ok(())
}

/// 原地打开文件夹操作模态框并同步中文IME。
fn open_folder_dialog(
    browser: &mut MapBrowserState,    // 写入冻结模式、目标和初始名称。
    mode: MapFolderDialogMode,        // 创建、重命名或删除。
    target_folder_id: Option<String>, // 创建使用父级，重命名和删除使用稳定目标。
    name: String,                     // 创建为空，其他模式冻结当前名称。
    windows: &mut Query<&mut Window, With<PrimaryWindow>>, // 输入法开关作用于主窗口。
) {
    browser.dialog = Some(MapFolderDialogState {
        mode,             // 确认时不再依赖可能变化的当前选择。
        target_folder_id, // 稳定ID允许重命名而不影响后代归属。
        name,             // 删除模式也用于明确目标回显。
    });
    browser.last_error = None; // 新操作不继承旧弹框校验文字。
    set_ime_enabled(windows, mode != MapFolderDialogMode::Delete); // 删除确认无需中文输入候选框。
}

/// 普通文件夹点击在松开时才改变展开状态，拖动不会先把源子树折叠掉。
fn toggle_folder_expansion(
    browser: &mut MapBrowserState,
    editor: &mut MapEditorState,
    folder_id: &str,
) {
    let Some(folder) = folder_by_id(&browser.document, folder_id) else {
        editor.status = "所选虚拟文件夹已不存在".to_owned();
        return;
    };
    let name = folder.display_name.clone();
    let expanded = if browser.expanded_folder_ids.remove(folder_id) {
        false
    } else {
        browser.expanded_folder_ids.insert(folder_id.to_owned());
        true
    };
    browser.expansion_revision = browser.expansion_revision.wrapping_add(1);
    editor.status = format!("已{}文件夹“{name}”", if expanded { "展开" } else { "收起" });
}

/// 地图行选择和地图浏览器专属按钮动作。
#[allow(clippy::too_many_arguments)] // 单系统必须在同一Interaction边沿冻结行、任务门、catalog和物理光标身份。
pub(crate) fn handle_map_browser_input(
    roots: Query<&EditorButtonClick, (With<MapEditorMapRootRow>, Changed<EditorButtonClick>)>, // 固定根目录行选择事件。
    rows: Query<(Ref<Interaction>, &MapEditorMapRow, Ref<EditorButtonClick>),
        Or<(Changed<Interaction>, Changed<EditorButtonClick>)>>, // 按下只捕获拖动候选，普通行操作等待合法松开。
    buttons: Query<
        (
            &EditorButtonClick,
            &MapEditorActionButton,
            Option<&crate::ui::map_sidebar::MapSidebarActionBinding>,
            Option<&crate::ui::map_sidebar::MapPropertiesEditBinding>,
        ),
        Changed<EditorButtonClick>,
    >, // 原动作继续处理，侧栏额外核对显示目标。
    mut windows: Query<&mut Window, With<PrimaryWindow>>, // 同一主窗口既提供物理起点也切换中文IME。
    numeric_scrub_mouse: Res<MapEditorNumericScrubMouseGuard>, // 公共数值和弦期间丢弃目录点击。
    index: Res<MapBrowserIndexState>,                     // 当前树可见名称和总项数。
    mut browser: ResMut<MapBrowserState>,                 // 高亮、展开集合和弹框权威状态。
    mut drag: ResMut<MapBrowserDragState>,                // 地图和文件夹按下只登记阈值候选。
    mut editor: ResMut<MapEditorState>,                   // 兼容既有地图按钮目标和状态。
    persistence: Res<MapFolderPersistence>,               // 结构操作single-flight门。
    creation: Res<MapCreationTask>,                       // 新建地图事务互斥门。
    deletion: Res<MapDeletionTask>,                       // 地图回收与切图保护门。
    registry: Option<Res<MapContentRegistry>>,            // 当前正式catalog句柄。
    catalogs: Res<Assets<MapCatalogAsset>>,               // 显式进入地图时重新验证目标。
    manifests: Res<Assets<MapManifestAsset>>, // 编辑弹框从正式清单读取备注和只读地形规格。
    context: (
        MapTransitionAuthoring,
        crate::ui::map_sidebar::MapSidebarInputContext,
        Option<Res<crate::ui::map_documents::MapDocuments>>,
        Res<Time<Real>>,
        Local<Option<MapBrowserClick>>,
        Option<Res<ButtonInput<KeyCode>>>,
        Option<Res<ButtonInput<MouseButton>>>,
        Local<crate::types::MapBrowserSelectionWork>,
        Option<Res<crate::resources::MapEditorSettingsState>>,
        Option<Res<crate::authoring::TerrainExternalChangeState>>,
    ),
) {
    let (
        authoring,
        mut sidebar_context,
        documents,
        real_time,
        mut last_click,
        keys,
        mouse,
        mut selection_work,
        settings,
        terrain_conflict,
    ) = context;
    let directory_visible = sidebar_context.visible();
    let blocked = !editor.open
        || editor.close_requested
        || (!directory_visible && !sidebar_context.map_properties_visible())
        || editor.map_transition_in_progress
        || editor.map_switch_confirmation.is_some()
        || browser.dialog.is_some()
        || editor.new_map_dialog_open
        || deletion.confirmation.is_some()
        || numeric_scrub_mouse.blocked()
        || drag.blocks_other_mouse()
        || settings
            .as_ref()
            .is_some_and(|settings| settings.open || settings.owns_mouse_this_frame)
        || terrain_conflict
            .as_ref()
            .is_some_and(|conflict| conflict.modal_open());
    // 连选只在完整投影和原上下文仍有效时一次采用；取消任务也先等真实结束再归还唯一槽。
    if selection_work.task.is_some() {
        selection_work.cancelled |= blocked || !directory_visible
            || persistence.batch.is_some()
            || selection_work.projection != index.applied_projection
            || index.rebuilding
            || selection_work
                .projection
                .as_ref()
                .is_none_or(|(source, expansion)| {
                    source.folder_revision != browser.document_revision
                        || source.catalog_revision
                            != registry.as_ref().map_or(0, |registry| registry.revision)
                        || *expansion != browser.expansion_revision
                })
            || windows.single().map_or(true, |window| !window.focused);
        let task = selection_work.task.as_mut().unwrap();
        if let Some(selection) = bevy::tasks::futures::check_ready(task) {
            selection_work.task = None;
            browser.selection_pending = false;
            if !selection_work.cancelled {
                browser.selected_item = selection.focus.clone();
                browser.selection = selection;
                editor.map_list_selection_id = if browser.selection_count() == 1 {
                    match browser.selected_item.as_ref() {
                        Some(MapBrowserItem::Map(id)) => Some(id.clone()),
                        _ => None,
                    }
                } else {
                    None
                };
                editor.status = format!(
                    "已连选{}项；拖动任一选中项批量整理",
                    browser.selection_count()
                );
                if editor.map_list_selection_id.is_some() {
                    sidebar_context.reveal_map_properties();
                }
                if mouse
                    .as_ref()
                    .is_some_and(|mouse| mouse.pressed(MouseButton::Left))
                    && let Some(start_physical) = selection_work.start_physical
                    && let Some(item) = browser.selected_item.clone()
                    && structure_change_allowed(&browser, &persistence, &creation, &deletion)
                        .is_ok()
                {
                    drag.press = Some(MapBrowserDragPress {
                        item,
                        document_revision: browser.document_revision,
                        display_name: "所选目录项".into(),
                        start_physical,
                    });
                    drag.selection =
                        (browser.selection_count() > 1).then(|| browser.selection.clone());
                    drag.catalog_revision =
                        Some(registry.as_ref().map_or(0, |registry| registry.revision));
                    drag.suppress_folder_toggle = true;
                }
            }
        }
        *last_click = None;
        return; // 等待与采用帧都不重放期间已消费的按钮/目录Interaction。
    }
    if blocked || persistence.batch.is_some() {
        *last_click = None; // 模态、隐藏页和拖拽介入后，旧单击不能与解除阻塞后的新按下组成双击。
        return; // 任一模态或鼠标所有权期间只推进Changed tick，不执行迟到动作。
    }
    if !directory_visible {
        *last_click = None; // 右侧地图属性仍可编辑，但隐藏目录必须独立终止旧地图双击序列。
    }
    let projection = index.applied_projection.as_ref().filter(|projection| {
        !index.rebuilding
            && Some(*projection) == index.desired_projection.as_ref()
            && projection.0.folder_revision == browser.document_revision
            && projection.0.catalog_revision
                == registry.as_ref().map_or(0, |registry| registry.revision)
            && projection.1 == browser.expansion_revision
    }); // 旧可见树仍允许单击选择，双击必须属于当前完整采用的目录。
    if deletion.busy()
        || editor.pending_map_transition.is_some()
        || last_click.as_ref().is_some_and(|last| {
            projection != Some(&last.projection)
                || !matches!(browser.selected_item.as_ref(), Some(MapBrowserItem::Map(id)) if id == &last.map_id)
        })
    {
        *last_click = None; // 目录热更、改选和在途切图都会终止旧点击序列。
    }
    let catalog = registry
        .as_deref()
        .and_then(|registry| catalogs.get(&registry.catalog)); // 只有地图进入按钮需要正式目录值。
    for interaction in &roots {
        if !directory_visible || !interaction.is_clicked() {
            continue; // 根目录没有拖放；选择由按钮内松开确认。
        }
        *last_click = None; // 根目录按下只选择新建上下文，不参与地图双击。
        if deletion.busy() || editor.pending_map_transition.is_some() {
            editor.status = "地图回收或进入请求完成前不能更改目录选择".to_owned(); // 冻结已有事务目标。
            continue;
        }
        browser.select_only(Some(MapBrowserItem::Root)); // 根目录是独占上下文，不能混入批量来源。
        editor.map_list_selection_id = None; // 根目录绝不能沿用旧地图进入或删除目标。
        drag.press = None; // 根目录行不可拖动。
        drag.selection = None;
        editor.status = "已选中虚拟根目录".to_owned(); // 根固定展开，不执行无意义收起。
    }
    for (interaction, row, click) in &rows {
        if !directory_visible {
            continue;
        }
        let Some(item) = row.item.clone() else {
            *last_click = None;
            continue; // 隐藏槽没有业务目标。
        };
        if deletion.busy() || editor.pending_map_transition.is_some() {
            editor.status = "地图回收或进入请求完成前不能更改目录选择".to_owned(); // 冻结已有事务目标。
            continue;
        }
        let control = keys
            .as_ref()
            .is_some_and(|keys| keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]));
        let shift = keys
            .as_ref()
            .is_some_and(|keys| keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
        if interaction.is_changed() && *interaction == Interaction::Pressed {
            // 拖放必须在按下时冻结来源，但此时不改选择、属性页或展开状态。
            drag.press = None;
            drag.selection = None;
            drag.collapse_on_release = false;
            drag.suppress_folder_toggle = true; // 阈值内的普通松开统一由共享按钮协议确认。
            if !shift && !matches!(item, MapBrowserItem::Root)
                && structure_change_allowed(&browser, &persistence, &creation, &deletion).is_ok()
                && let Ok(window) = windows.single_mut()
                && let Some(start_physical) = window.physical_cursor_position()
            {
                let display_name = match &item {
                    MapBrowserItem::Folder(id) => folder_by_id(&browser.document, id)
                        .map(|folder| folder.display_name.clone()),
                    _ => index.entries.iter().find(|entry| entry.item == item)
                        .map(|entry| entry.display_name.clone()),
                }.unwrap_or_else(|| "所选目录项".into());
                drag.selection = (browser.item_selected(&item) && browser.selection_count() > 1)
                    .then(|| browser.selection.clone());
                drag.catalog_revision = Some(registry.as_ref().map_or(0, |registry| registry.revision));
                drag.press = Some(MapBrowserDragPress { item: item.clone(),
                    document_revision: browser.document_revision, display_name, start_physical });
            }
        }
        if !click.is_changed() || !click.is_clicked() {
            continue; // 悬停、持续按住和移出松开都不提交目录动作。
        }
        drag.press = None;
        drag.selection = None;
        drag.collapse_on_release = false;
        drag.suppress_folder_toggle = control || shift;
        if shift {
            if projection.is_none() {
                *last_click = None;
                editor.status = "地图目录正在整理，请完成后再连续选择".into();
                continue; // 旧投影上的Shift点击不能静默降级为单选或清空原选集。
            }
            super::selection::begin_range(
                &mut browser,
                &index,
                &mut selection_work,
                item,
                control,
                None, // 连选始于合法松开，不再为后台采用帧补造按下拖动源。
            );
            editor.status = "正在按可见目录顺序连选…".into();
            *last_click = None;
            continue;
        }
        if control {
            browser.toggle_selection(item.clone());
            *last_click = None;
        } else {
            if browser.selection_count() > 1 { *last_click = None; }
            browser.select_only(Some(item.clone()));
        }
        if !browser.item_selected(&item) {
            editor.map_list_selection_id = match browser.selected_item.as_ref() {
                Some(MapBrowserItem::Map(id)) if browser.selection_count() == 1 => Some(id.clone()),
                _ => None,
            };
            editor.status = format!("已选择{}项", browser.selection_count());
            continue; // Ctrl取消的行不能成为拖动来源，也不能展开目录或打开地图。
        }
        match item {
            MapBrowserItem::Folder(folder_id) => {
                *last_click = None; // 文件夹展开/收起打断地图双击。
                editor.map_list_selection_id = None; // 文件夹选择绝不能误删旧地图目标。
                let Some(folder) = folder_by_id(&browser.document, &folder_id) else {
                    browser.select_only(None); // 热更新移除后的旧虚拟行不能修改展开集合。
                    editor.status = "所选虚拟文件夹已不存在".to_owned();
                    continue;
                };
                let display_name = folder.display_name.clone();
                if !drag.suppress_folder_toggle {
                    toggle_folder_expansion(&mut browser, &mut editor, &folder_id); // 合法松开才导航；归类禁写仍允许展开。
                } else {
                    editor.status = format!("已选中文件夹“{display_name}”");
                }
            }
            MapBrowserItem::Map(map_id) => {
                editor.map_list_selection_id =
                    (browser.selection_count() == 1).then(|| map_id.clone()); // 多选不能把焦点误用作单图操作目标。
                if editor.map_list_selection_id.is_some() {
                    // 只在明确选择边沿揭示地图属性；不进入地图、不改作者对象选择或工具。
                    sidebar_context.reveal_map_properties();
                }
                let display_name = index
                    .entries
                    .iter()
                    .find(|entry| entry.item == MapBrowserItem::Map(map_id.clone()))
                    .map(|entry| entry.display_name.clone())
                    .unwrap_or_else(|| map_id.as_str().to_owned()); // 行池与索引错帧时使用稳定ID。
                editor.status =
                    format!("已选中地图“{display_name}” [{map_id}]；可进入、删除或拖入文件夹");
                let seconds = real_time.elapsed_secs_f64();
                let double_clicked = !control
                    && !shift
                    && browser.selection_count() == 1
                    && projection.is_some_and(|projection| MapBrowserClick::release(&mut last_click, &map_id, projection, seconds));
                if double_clicked {
                    drag.press = None; // 第二次合法松开用于打开地图，不再成为拖拽候选。
                    request_selected_map_entry(
                        &mut editor,
                        &deletion,
                        catalog,
                        &authoring.session,
                        &authoring.global,
                        documents.is_some(),
                    ); // 与进入按钮共用脏草稿确认、多文档和异步切图保护。
                    if let Some(feedback) = sidebar_context.feedback.as_deref_mut() {
                        feedback.record(&editor, &browser, registry.as_deref(), deletion.busy());
                    }
                    continue; // 打开失败也消费这次双击，不在保存或模态结束后补执行。
                }
            }
            MapBrowserItem::Root => {
                *last_click = None;
                editor.map_list_selection_id = None; // 根目录不会正常进入虚拟行池，防御性保持选择安全。
                drag.press = None; // 根目录不可拖动。
            }
        }
        if control || browser.selection_count() > 1 {
            editor.status = format!(
                "已选择{}项；Ctrl点击增减，Shift连选，拖动选中项整批归类",
                browser.selection_count()
            );
        }
    }

    let mut sidebar_before = None;
    for (interaction, button, binding, properties) in &buttons {
        if !interaction.is_clicked() {
            continue; // 每个按钮只消费按下边沿。
        }
        if let Some(properties) = properties {
            if !sidebar_context.map_properties_visible()
                || !properties.enabled
                || !matches!(browser.selected_item.as_ref(), Some(MapBrowserItem::Map(id)) if properties.map_id.as_ref() == Some(id))
                || button.0 != EditorAction::MapFolderRename
            {
                continue; // 属性已切源或显示身份过期时，旧点击不能改派给新的地图。
            }
        } else if !directory_visible {
            continue; // 左导航隐藏时，不能因右侧属性可见而重放目录行或菜单动作。
        }
        *last_click = None; // 显式按钮操作不能跨越地图双击序列。
        if browser.selection_count() > 1
            && matches!(
                button.0,
                EditorAction::EnterSelectedMap
                    | EditorAction::MapFolderRename
                    | EditorAction::DeleteMap
            )
        {
            continue; // 多选只承诺批量整理，不把单项资料/删除按钮暗中作用于一个焦点。
        }
        if binding.is_some_and(|binding| {
            !sidebar_context.visible() || !binding.matches(browser.selected_item.as_ref())
        }) {
            continue; // 目录选择已变而按钮仍显示旧目标时，消费旧按下，不重定向到新对象。
        }
        if (binding.is_some() || properties.is_some())
            && matches!(
                button.0,
                EditorAction::EnterSelectedMap
                    | EditorAction::MapFolderCreate
                    | EditorAction::MapFolderRename
                    | EditorAction::DeleteMap
            )
            && !(button.0 == EditorAction::DeleteMap
                && matches!(browser.selected_item, Some(MapBrowserItem::Map(_))))
        {
            sidebar_before = Some(editor.status.clone()); // 只观察本页真正执行的原入口，不捕获其他工具或行选择的底栏。
        }
        match button.0 {
            EditorAction::EnterSelectedMap => match browser.selected_item.clone() {
                Some(MapBrowserItem::Folder(folder_id)) => {
                    if folder_by_id(&browser.document, &folder_id).is_none() {
                        browser.select_only(None); // 热更新移除旧选择后不操作失效ID。
                        editor.status = "所选虚拟文件夹已不存在".to_owned();
                        continue;
                    }
                    editor.status = "目录请直接点击行展开或收起；“进入地图”只作用于地图".to_owned(); // 树结构不再切换当前目录。
                }
                Some(MapBrowserItem::Map(map_id)) => {
                    editor.map_list_selection_id = Some(map_id); // 显式同步既有切图保护函数目标。
                    request_selected_map_entry(
                        &mut editor,
                        &deletion,
                        catalog,
                        &authoring.session,
                        &authoring.global,
                        documents.is_some(),
                    ); // 继续走原加载链，并在按钮帧核对作者加载基线。
                }
                Some(MapBrowserItem::Root) => {
                    editor.status = "根目录固定展开；请选中一张地图后进入".to_owned(); // 根目录不是玩法场景。
                }
                None => editor.status = "请先选择一张地图".to_owned(), // 不猜测列表首项。
            },
            EditorAction::MapFolderCreate => {
                if let Err(message) =
                    structure_change_allowed(&browser, &persistence, &creation, &deletion)
                {
                    editor.status = message;
                    continue;
                }
                if browser.document.folders.len() >= MAP_FOLDER_MAX_FOLDERS {
                    editor.status = format!("虚拟文件夹已达到{MAP_FOLDER_MAX_FOLDERS}项上限");
                    continue;
                }
                let parent_folder_id =
                    selected_container_folder_id(&browser.document, browser.selected_item.as_ref()); // 目录选择建子目录，地图选择沿用其父级，根或空选择建在根下。
                open_folder_dialog(
                    &mut browser,
                    MapFolderDialogMode::Create,
                    parent_folder_id,
                    String::new(),
                    &mut windows,
                ); // 已选归类上下文在确认时读取，保存任务开始后即冻结。
            }
            EditorAction::MapFolderRename => {
                if let Err(message) =
                    structure_change_allowed(&browser, &persistence, &creation, &deletion)
                {
                    editor.status = message;
                    continue;
                }
                if let Some(MapBrowserItem::Map(map_id)) = browser.selected_item.clone() {
                    let Some(entry) = catalog
                        .and_then(|catalog| catalog.maps.iter().find(|entry| entry.id == map_id))
                    else {
                        editor.status = "地图目录尚未就绪或目标已不存在".to_owned(); // 不猜测名称或磁盘目录。
                        continue;
                    };
                    let Some(manifest) = registry
                        .as_deref()
                        .and_then(|registry| registry.handles(&map_id))
                        .and_then(|handles| manifests.get(&handles.manifest))
                        .filter(|manifest| {
                            manifest.id == map_id && manifest.display_name == entry.display_name
                        })
                    else {
                        editor.status = "地图清单尚未就绪或名称与目录不一致，请稍后重试".to_owned(); // 不显示别张地图的旧资料。
                        continue;
                    };
                    // 地图资料只冻结身份与文本基线，网格规格和LOD由对象属性独立编辑。
                    editor.new_map_edit_target = Some(MapEditTarget {
                        id: map_id,
                        original_name: manifest.display_name.clone(),
                        original_notes: manifest.notes.clone(),
                    });
                    editor.new_map_name = manifest.display_name.clone();
                    editor.new_map_notes = manifest.notes.clone();
                    editor.new_map_input_field = crate::types::NewMapInputField::Name; // 打开先聚焦可编辑名称。
                    editor.new_map_dialog_open = true; // 复用新建弹框布局，不弹出旧重命名小框。
                    editor.status =
                        "可修改名称和备注；从左侧场景树选择网格，在右侧属性编辑参数与LOD"
                            .to_owned(); // 未保存显示和碰撞配置都不产生实时预览。
                    set_ime_enabled(&mut windows, true); // 中文名称和备注沿用正式输入法焦点。
                    continue;
                }
                let folder_id = match browser.selected_item.clone() {
                    Some(MapBrowserItem::Folder(folder_id)) => folder_id, // 文件夹继续使用原事务。
                    Some(MapBrowserItem::Root) => {
                        editor.status = "虚拟根目录不能重命名".to_owned();
                        continue;
                    }
                    _ => {
                        editor.status = "请先选择地图或虚拟文件夹再编辑".to_owned();
                        continue;
                    }
                };
                let Some(folder) = folder_by_id(&browser.document, &folder_id) else {
                    editor.status = "所选虚拟文件夹已不存在".to_owned();
                    browser.select_only(None);
                    continue;
                };
                let display_name = folder.display_name.clone(); // 在可变借用browser打开弹框前冻结短名称。
                open_folder_dialog(
                    &mut browser,
                    MapFolderDialogMode::Rename,
                    Some(folder_id),
                    display_name,
                    &mut windows,
                ); // 稳定ID保证重命名不影响后代和地图归属。
            }
            EditorAction::DeleteMap => {
                let folder_id = match browser.selected_item.clone() {
                    Some(MapBrowserItem::Folder(folder_id)) => folder_id, // 文件夹走空目录二次确认。
                    Some(MapBrowserItem::Root) => {
                        editor.status = "虚拟根目录不能删除".to_owned();
                        continue;
                    }
                    _ => continue, // 地图删除仍交给既有可恢复事务系统。
                };
                if let Err(message) =
                    structure_change_allowed(&browser, &persistence, &creation, &deletion)
                {
                    editor.status = message;
                    continue;
                }
                let Some(folder) = folder_by_id(&browser.document, &folder_id) else {
                    editor.status = "所选虚拟文件夹已不存在".to_owned();
                    browser.select_only(None);
                    continue;
                };
                if !folder_is_empty(&browser.document, &folder_id) {
                    editor.status = "只能删除没有子文件夹和地图的空文件夹".to_owned(); // 首版不递归删除。
                    continue;
                }
                let display_name = folder.display_name.clone(); // 在可变借用browser打开弹框前冻结确认名称。
                open_folder_dialog(
                    &mut browser,
                    MapFolderDialogMode::Delete,
                    Some(folder_id),
                    display_name,
                    &mut windows,
                ); // 二次确认冻结稳定目标和可读名称。
            }
            _ => {} // 其他编辑器动作由各自功能域处理。
        }
    }
    if sidebar_before.is_some_and(|before| {
        before != editor.status || (browser.dialog.is_none() && !editor.new_map_dialog_open)
    }) && let Some(feedback) = sidebar_context.feedback.as_deref_mut()
    {
        feedback.record(&editor, &browser, registry.as_deref(), deletion.busy());
    }
}
