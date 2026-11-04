//! 地图页语义变化时原位回显；操作仍由原目录、切图和回收入口处理。

use super::{components::*, lookup::advance_lookup, resources::SidebarSources, types::*};
use crate::components::MapEditorActionButton;
use crate::constants::MAP_BROWSER_ROW_PITCH;
use crate::types::{EditorAction, MapBrowserItem};
use crate::ui::theme as colors;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

fn operation_phase(source: &SidebarSources) -> OperationPhase {
    if source.editor.map_transition_in_progress {
        return OperationPhase::Switching;
    }
    if source.editor.map_switch_confirmation.is_some() {
        return OperationPhase::SwitchConfirmation;
    }
    if source.editor.pending_map_transition.is_some() {
        return OperationPhase::SwitchWaiting;
    }
    if source.deletion.task.is_some() {
        return OperationPhase::Deleting;
    }
    if source.deletion.pending_start.is_some() {
        return OperationPhase::DeleteWaiting;
    }
    if source.deletion.confirmation.is_some() {
        return OperationPhase::DeleteConfirmation;
    }
    if source.persistence.rename_task.is_some() {
        return OperationPhase::EditingMap;
    }
    if source.persistence.task.is_some() || source.persistence.batch.is_some() {
        return OperationPhase::SavingFolders;
    }
    if source.creation.task.is_some() {
        return OperationPhase::Creating;
    }
    if source.creation.pending_after_save.is_some() {
        return OperationPhase::CreateAfterSave;
    }
    if source.browser.dialog.is_some() {
        return OperationPhase::FolderDialog;
    }
    if source.editor.new_map_dialog_open {
        return OperationPhase::MapDialog;
    }
    if source.drag.active {
        return OperationPhase::Dragging;
    }
    OperationPhase::Idle
}

fn directory_feedback<'a>(source: &'a SidebarSources) -> (&'a str, bool) {
    if source.hosted.is_some() {
        return if source.index.rebuilding { ("正在整理工程地图目录", false) }
            else if source.index.total_entries == 0 { ("当前工程没有地图", false) }
            else { ("", false) };
    }
    if let Some(error) = source
        .registry
        .as_ref()
        .and_then(|registry| registry.last_error.as_deref())
    {
        return (error, true); // 正式目录校验诊断不被其他工具的底栏覆盖。
    }
    if source
        .registry
        .as_ref()
        .is_none_or(|registry| registry.revision == 0)
    {
        return ("正在加载地图目录", false);
    }
    if !source.browser.initial_load_resolved {
        return ("正在加载地图归类", false);
    }
    if let Some(error) = source.browser.write_blocked.as_deref() {
        return (error, true);
    }
    if source.index.rebuilding
        || source.index.applied_projection.is_none()
        || source.index.applied_projection != source.index.desired_projection
    {
        // 同一来源的展开/收起继续使用旧投影；短任务不在操作栏上方插入提示，避免整列闪动。
        // 首次整理或来源换代仍显示等待状态，不能把真实加载和目录错误一起隐藏。
        let expansion_only = source.index.applied_projection.as_ref()
            .zip(source.index.desired_projection.as_ref())
            .is_some_and(|(applied, desired)| applied.0 == desired.0 && applied.1 != desired.1);
        if !expansion_only {
            return ("正在整理目录 · 保留已显示的目录项", false);
        }
    }
    if source.index.total_entries == 0 {
        return ("当前没有地图或文件夹", false);
    }
    ("", false) // 稳定目录不再常驻整段手势说明；加载、空目录与失败仍直接显示。
}

fn operation_message<'a>(source: &'a SidebarSources, phase: OperationPhase) -> &'a str {
    if let Some(error) = source.browser.last_error.as_deref() {
        return error;
    }
    match phase {
        OperationPhase::SavingFolders => "正在保存地图归类 · 完成前不能再次修改结构",
        OperationPhase::EditingMap => "正在保存地图资料 · 保留原地图与作者草稿",
        OperationPhase::Creating => "正在创建地图 · 等待原事务完成",
        OperationPhase::CreateAfterSave => "保存当前地图后再创建 · 保留已冻结的创建请求",
        OperationPhase::Deleting => "正在回收地图 · 等待原事务完成",
        OperationPhase::DeleteWaiting => "正在等待安全地图 · 尚未执行回收",
        OperationPhase::DeleteConfirmation => "请在确认框核对待回收地图 · 尚未删除",
        OperationPhase::SwitchConfirmation => "当前地图有未保存修改 · 请在切图确认框选择处理方式",
        OperationPhase::SwitchWaiting => "进入请求已登记 · 等待目标地图完整准备",
        OperationPhase::Switching => "正在切换地图 · 详细进度见中央工作区",
        // 拖拽过程与拒绝原因由原随鼠标浮层完整显示；不再用重复正文压缩树高、移动滚动边缘。
        OperationPhase::Dragging => "",
        OperationPhase::FolderDialog => "请在弹框中确认目录操作 · 不改真实地图资产目录",
        OperationPhase::MapDialog if source.editor.new_map_edit_target.is_some() => {
            "请在弹框中编辑地图资料 · 确认后由原事务保存"
        }
        OperationPhase::MapDialog => "请在弹框中设置新地图 · 确认后由原事务创建",
        OperationPhase::Idle => source.feedback.current_message(
            &source.editor,
            &source.browser,
            source.registry.as_deref(),
            source.deletion.busy(),
        ),
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn sync_sidebar(
    mut commands: Commands,
    source: SidebarSources,
    mut lookup: Local<SelectionLookup>,
    mut previous: Local<SidebarObservation>,
    mut buttons: Query<(
        Entity,
        &MapEditorActionButton,
        &Interaction,
        Has<InteractionDisabled>,
        &mut MapSidebarActionBinding,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
        Option<&mut feathrise_editor_ui::EditorButtonClick>,
    )>,
    mut texts: Query<
        (&MapSidebarText, &mut Text, &mut TextColor, &mut Node),
        (
            Without<MapSidebarActionBinding>,
            Without<MapSidebarActionLabel>,
        ),
    >,
    mut labels: Query<(&MapSidebarActionLabel, &mut Text, &mut TextColor), Without<MapSidebarText>>,
) {
    if !source.editor.open || !source.sidebar.map_browser_visible() {
        for (_, _, _, _, mut binding, _, _, _, click) in &mut buttons {
            if let Some(mut click) = click { click.invalidate(); }
            if binding.enabled {
                binding.enabled = false;
            }
        }
        lookup.initialized = false;
        lookup.source = None; // 隐藏页不保留整棵旧树租约；短文字和滚动实体仍原位保留。
        previous.initialized = false;
        return;
    }
    let selected = source.browser.selected_item.as_ref();
    let multiple = source.browser.selection_count() > 1;
    let visible = selected.and_then(|item| {
        let scroll = source.scroll.single().ok()?;
        let start =
            ((scroll.y.max(0.0) / MAP_BROWSER_ROW_PITCH).floor() as usize).saturating_sub(1);
        source
            .rows
            .iter()
            .find(|row| row.item.as_ref() == Some(item))
            .and_then(|row| source.index.entries.get(start.saturating_add(row.slot)))
            .filter(|entry| &entry.item == item)
    }); // 固定二十八槽提供O(1)范围内的真实绑定，不按名称扫描完整目录。
    advance_lookup(&mut lookup, selected, &source.index, visible);
    let phase = operation_phase(&source);
    let selection_exists =
        lookup.record.is_some() || matches!(selected, None | Some(MapBrowserItem::Root));
    let base_available = !source.editor.close_requested
        && selection_exists
        && phase == OperationPhase::Idle
        && (!source.mouse_guard.blocked() || source.menu.as_ref().is_some_and(|menu| menu.owns_mouse()))
        && !source.drag.blocks_other_mouse();
    let structure_available = base_available && source.browser.write_blocked.is_none();
    let has_map = matches!(selected, Some(MapBrowserItem::Map(_)));
    let has_folder = matches!(selected, Some(MapBrowserItem::Folder(_)));
    for (
        entity,
        button,
        interaction,
        disabled,
        mut binding,
        mut node,
        mut background,
        mut border,
        click,
    ) in &mut buttons
    {
        let shown = if binding.menu_owned {
            source.menu.as_ref().is_some_and(|menu| menu.action_visible(button.0))
        } else { match button.0 {
            EditorAction::EnterSelectedMap => has_map,
            EditorAction::MapFolderRename | EditorAction::DeleteMap => has_map || has_folder,
            _ => true,
        } };
        let enabled = shown
            && source.hosted.as_ref().is_none_or(|host| !host.modal && !host.busy && host.enabled_actions.contains(&button.0))
            && !source.browser.selection_pending
            && !(multiple
                && matches!(
                    button.0,
                    EditorAction::EnterSelectedMap
                        | EditorAction::MapFolderRename
                        | EditorAction::DeleteMap
                ))
            && if button.0 == EditorAction::EnterSelectedMap {
                base_available
            } else {
                structure_available
                    && !(button.0 == EditorAction::DeleteMap && has_map && source.editor.dirty)
            };
        let frozen = if binding.menu_owned {
            source.menu.as_ref().and_then(|menu| menu.binding_target()).and_then(Option::as_ref)
        } else { selected };
        if (binding.selected.as_ref() != frozen || binding.enabled != enabled)
            && let Some(mut click) = click { click.invalidate(); }
        if binding.selected.as_ref() != frozen {
            binding.selected = frozen.cloned();
        }
        if binding.enabled != enabled {
            binding.enabled = enabled;
        }
        let dispatched = binding.menu_owned && source.menu.as_ref().is_some_and(|menu| menu.permits(entity));
        if binding.dispatched != dispatched { binding.dispatched = dispatched; }
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        if disabled == enabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        let weak = button.0 == EditorAction::DeleteMap;
        let color = if !enabled {
            colors::SLOT_BG
        } else {
            match interaction {
                Interaction::Pressed => colors::SELECT_BG_PRESSED,
                Interaction::Hovered => colors::SELECT_BG_HOVER,
                Interaction::None if weak => colors::SLOT_BG,
                Interaction::None => colors::BUTTON_DEV_SMALL_NORMAL,
            }
        };
        background.set_if_neq(BackgroundColor(color));
        border.set_if_neq(BorderColor::all(
            if enabled && *interaction != Interaction::None {
                colors::COPPER_BORDER
            } else {
                colors::COPPER_BORDER_WEAK
            },
        ));
    }
    for (label, mut text, mut color) in &mut labels {
        let wanted = match label.0 {
            EditorAction::MapFolderRename if has_folder => "重命名",
            EditorAction::MapFolderRename if source.menu.is_some() => "编辑",
            EditorAction::MapFolderRename => "资料",
            EditorAction::DeleteMap if has_folder && source.menu.is_some() => "删除空目录",
            EditorAction::DeleteMap => "删除",
            EditorAction::NewMap => "新地图",
            EditorAction::MapFolderCreate => "新目录",
            _ if source.menu.is_some() => "进入地图",
            _ => "进入",
        };
        if text.as_str() != wanted {
            **text = wanted.to_owned();
        }
        let enabled = if label.0 == EditorAction::EnterSelectedMap {
            base_available
        } else {
            structure_available
        };
        color.set_if_neq(TextColor(
            if label.0 == EditorAction::DeleteMap || !enabled {
                colors::TEXT_MUTED
            } else {
                colors::TEXT_PRIMARY
            },
        ));
    }
    let current = source.editor.selected_map_id.as_ref();
    let draft = source
        .session
        .draft
        .as_ref()
        .filter(|draft| Some(&draft.map_id) == current);
    let current_name = draft
        .map(|draft| draft.manifest.display_name.as_str())
        .or_else(|| {
            let id = current?;
            let handles = source.registry.as_ref()?.handles(id)?;
            source
                .manifests
                .as_ref()?
                .get(&handles.manifest)
                .filter(|manifest| &manifest.id == id)
                .map(|manifest| manifest.display_name.as_str())
        })
        .unwrap_or(if current.is_some() {
            "地图名称加载中"
        } else {
            "尚未进入地图"
        });
    let dirty = current.is_some() && source.editor.dirty;
    let saving = draft.is_some() && source.session.saving;
    let catalog_revision = source
        .registry
        .as_ref()
        .map_or(0, |registry| registry.revision);
    let catalog_error = source
        .registry
        .as_ref()
        .and_then(|registry| registry.last_error.as_ref());
    let projection_pending = source.index.applied_projection.is_none()
        || source.index.applied_projection != source.index.desired_projection;
    let message = operation_message(&source, phase);
    if previous.initialized
        && previous.current_map.as_ref() == current
        && previous.current_name == current_name
        && previous.dirty == dirty
        && previous.saving == saving
        && previous.selected.as_ref() == selected
        && previous.selection_revision == source.browser.selection.revision
        && previous.selection_pending == source.browser.selection_pending
        && previous.record == lookup.record
        && previous.lookup_complete == lookup.complete
        && previous.rebuilding == source.index.rebuilding
        && previous.projection == source.index.applied_projection
        && previous.total == source.index.total_entries
        && previous.projection_pending == projection_pending
        && previous.catalog_revision == catalog_revision
        && previous.catalog_error.as_ref() == catalog_error
        && previous.folder_ready == source.browser.initial_load_resolved
        && previous.blocked == source.browser.write_blocked
        && previous.folder_error == source.browser.last_error
        && previous.operation == phase
        && previous.action_feedback == message
    {
        return; // 光标、笔刷及其他工具底栏不会改写目录摘要和所选名称。
    }
    let (directory, failed) = directory_feedback(&source);
    let state = current.map_or_else(String::new, |id| {
        format!(
            "{} · {id}",
            if phase == OperationPhase::Switching {
                "正在切换"
            } else if saving {
                "保存中"
            } else if dirty {
                "未保存修改"
            } else {
                "已进入"
            }
        )
    });
    let selected_kind = if multiple {
        "批量选择"
    } else {
        match selected {
            Some(MapBrowserItem::Map(id)) if Some(id) == current => "当前地图",
            Some(MapBrowserItem::Map(_)) => "地图 · 尚未进入",
            Some(MapBrowserItem::Folder(_)) => "虚拟文件夹",
            Some(MapBrowserItem::Root) => "根目录",
            None => "尚未选择",
        }
    };
    let summary = format!(
        "{} 张地图、{} 个文件夹",
        source.browser.selection.maps, source.browser.selection.folders
    );
    let selected_name = if multiple {
        summary.as_str()
    } else {
        match selected {
            Some(MapBrowserItem::Root) => "根目录",
            None => "请在目录中选择地图或文件夹",
            _ => lookup
                .record
                .as_ref()
                .map(|entry| entry.display_name.as_str())
                .unwrap_or(if lookup.complete {
                    "所选对象已不在目录中"
                } else {
                    "正在同步所选名称"
                }),
        }
    };
    let identity = if let Some(MapBrowserItem::Map(id)) = selected.filter(|_| !multiple) {
        format!("地图ID：{id}")
    } else {
        String::new()
    };
    let current_summary = if current.is_some() {
        format!("{current_name} · {}", if phase == OperationPhase::Switching {
            "正在切换"
        } else if saving { "保存中" } else if dirty { "未保存修改" } else { "已进入" })
    } else {
        current_name.to_owned()
    };
    let selected_summary = if source.browser.selection_pending {
        format!("所选：{selected_name} · 正在连选…") // 连选进度属于真实状态，不能只留在折叠说明内。
    } else if selected.is_none() {
        "所选：尚未选择".to_owned()
    } else {
        format!("所选：{selected_name} · {selected_kind}")
    };
    let hint = if source.browser.selection_pending {
        "正在按可见目录顺序连选…"
    } else if multiple {
        "Ctrl点击增减选择，Shift连续选择；拖动任一选中项整批归类，父目录包含子项时自动去重"
    } else {
        match selected {
            Some(MapBrowserItem::Map(_)) if dirty => {
                "进入前由原切图确认处理未保存修改；当前不能删除地图"
            }
            Some(MapBrowserItem::Map(_)) => {
                "双击地图或点击进入打开；Ctrl多选、Shift连选，拖动批量归类；新建项建在该地图所在目录"
            }
            Some(MapBrowserItem::Folder(_)) => {
                "点击目录展开或收起；Ctrl多选、Shift连选，拖动可整体移动；新建项建在该目录，只能删除空文件夹"
            }
            _ => "新地图和文件夹建在根目录；目录选择不改变当前地图",
        }
    };
    for (field, mut text, mut color, mut node) in &mut texts {
        let value = match field.0 {
            SidebarField::CurrentName => current_name,
            SidebarField::CurrentState => &state,
            SidebarField::DirectoryFeedback => directory,
            SidebarField::SelectedKind => selected_kind,
            SidebarField::SelectedName => selected_name,
            SidebarField::SelectedIdentity => &identity,
            SidebarField::SelectionHint => hint,
            SidebarField::OperationFeedback => message,
            SidebarField::CurrentSummary => &current_summary,
            SidebarField::SelectedSummary => &selected_summary,
        };
        if text.as_str() != value {
            **text = value.to_owned();
        }
        let display = if value.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
        color.set_if_neq(TextColor(match field.0 {
            SidebarField::CurrentSummary if dirty || saving => colors::TEXT_ACCENT,
            SidebarField::CurrentName | SidebarField::SelectedName
                | SidebarField::CurrentSummary | SidebarField::SelectedSummary => colors::TEXT_PRIMARY,
            SidebarField::CurrentState if dirty || saving => colors::TEXT_ACCENT,
            SidebarField::DirectoryFeedback if failed => colors::TEXT_ACCENT,
            SidebarField::OperationFeedback => colors::TEXT_ACCENT,
            _ => colors::TEXT_MUTED,
        }));
    }
    *previous = SidebarObservation {
        initialized: true,
        current_map: current.cloned(),
        current_name: current_name.to_owned(),
        dirty,
        saving,
        selected: selected.cloned(),
        selection_revision: source.browser.selection.revision,
        selection_pending: source.browser.selection_pending,
        record: lookup.record.clone(),
        lookup_complete: lookup.complete,
        rebuilding: source.index.rebuilding,
        projection: source.index.applied_projection.clone(),
        total: source.index.total_entries,
        projection_pending,
        catalog_revision,
        catalog_error: catalog_error.cloned(),
        folder_ready: source.browser.initial_load_resolved,
        blocked: source.browser.write_blocked.clone(),
        folder_error: source.browser.last_error.clone(),
        operation: phase,
        action_feedback: message.to_owned(),
    };
}
