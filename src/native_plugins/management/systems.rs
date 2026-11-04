//! 原管理按钮驱动单槽后台事务；输入由整窗模态与释放帧持有，不穿透作者工具。
use super::{components::*, resources::*, types::*};
use bevy::{
    input_focus::InputFocus, prelude::*, tasks::futures::check_ready, ui::InteractionDisabled,
};
use feathrise_editor_core::EditorTextInputGuard;
use feathrise_editor_ui::EditorButtonClick;

pub(super) fn control(world: &mut World) {
    let blocked = world
        .get_resource::<crate::native_project_close::NativeProjectCloseState>()
        .is_some_and(|state| state.captures_input())
        || world
            .get_resource::<crate::project_log::ProjectLogState>()
            .is_some_and(|state| state.captures_input())
        || world
            .get_resource::<crate::workspace_ui::AboutState>()
            .is_some_and(|state| state.captures_input())
        || world
            .get_resource::<crate::code_iteration::diagnostics_ui::DiagnosticsOverlayState>()
            .is_some_and(|state| state.captures_input())
        || world
            .get_resource::<feathrise_editor_map::workbench::WorkbenchHostPageState>()
            .is_some_and(|state| state.settings_open)
        // 导出已打开或仍在释放输入时，不允许旧管理动作跨过另一浮窗；写入中的后台计划继续收口。
        || crate::project_export::ui::captures_input(world);
    if blocked {
        for mut click in world.query_filtered::<&mut EditorButtonClick, With<ManagerAction>>().iter_mut(world) {
            if click.is_clicked() { click.invalidate(); }
        }
    }
    let escape = world
        .get_resource::<ButtonInput<KeyCode>>()
        .is_some_and(|keys| keys.just_pressed(KeyCode::Escape));
    let (open, revision, busy) = {
        let state = world.resource::<PluginManagerState>();
        (state.open, state.revision, state.busy)
    };
    let action = world
        .query::<(
            &ManagerAction,
            &EditorButtonClick,
            Option<&ManagerBinding>,
            Has<InteractionDisabled>,
        )>()
        .iter(world)
        .find_map(|(action, click, binding, disabled)| {
            (click.is_clicked()
                && !disabled
                && !blocked
                && open
                && binding.is_none_or(|binding| binding.0 == revision))
            .then(|| action.clone())
        });
    let mut choose = None;
    let native_ready = world
        .get_resource::<super::super::resources::NativeTools>()
        // 发现完成即表示启动文件事务已结束；个别DLL握手迟缓不能阻止管理其他插件。
        .is_none_or(|state| !state.discovery_pending);
    let clear_focus;
    {
        let mut state = world.resource_mut::<PluginManagerState>();
        if blocked {
            state.open = false;
            state.confirm_remove = None;
        } else if open && escape {
            if state.confirm_remove.take().is_none() {
                state.open = false;
            }
            state.revision = state.revision.wrapping_add(1);
        } else if let Some(action) = action {
            let mut result = Ok(());
            match action {
                ManagerAction::Close => {
                    state.open = false;
                    state.confirm_remove = None;
                }
                ManagerAction::Refresh if !busy && native_ready => {
                    result = state.refresh();
                }
                ManagerAction::OpenDirectory if !busy && native_ready => {
                    result = state.request(ManagementCommand::OpenDirectory);
                }
                ManagerAction::SetScope(scope) if !busy && native_ready => {
                    result = state.switch_scope(scope);
                }
                ManagerAction::Install if !busy && native_ready => {
                    choose = Some(None);
                }
                ManagerAction::Select(id) if state.rows.iter().any(|row| row.id == id) => {
                    state.selected = Some(id);
                    state.confirm_remove = None;
                    state.revision = state.revision.wrapping_add(1);
                }
                ManagerAction::SetEnabled { id, enabled }
                    if !busy && native_ready && state.selected.as_ref() == Some(&id) =>
                {
                    result = state.request(ManagementCommand::SetEnabled { id, enabled });
                }
                ManagerAction::Update(id)
                    if !busy && native_ready && state.selected.as_ref() == Some(&id) =>
                {
                    choose = Some(Some(id));
                }
                ManagerAction::RequestRemove(id)
                    if !busy && state.selected.as_ref() == Some(&id) =>
                {
                    state.confirm_remove = Some(id);
                    // 新操作采用自己的确认语境，避免上一次更新错误被误认为当前移除失败。
                    state.status = "请确认移除作者插件；已保存的作者数据会保留".into();
                    state.revision = state.revision.wrapping_add(1);
                }
                ManagerAction::ConfirmRemove if !busy && native_ready => {
                    if let Some(id) = state.confirm_remove.take() {
                        result = state.request(ManagementCommand::Remove { id });
                    }
                }
                ManagerAction::CancelRemove => {
                    state.confirm_remove = None;
                    state.status = "已取消移除，插件与作者数据保持原状".into();
                    state.revision = state.revision.wrapping_add(1);
                }
                ManagerAction::CancelPending(id)
                    if !busy && native_ready && state.selected.as_ref() == Some(&id) =>
                {
                    result = state.request(ManagementCommand::Cancel { id });
                }
                _ => {}
            }
            if let Err(error) = result {
                state.status = error;
                state.revision = state.revision.wrapping_add(1);
            }
        }
        clear_focus = state.open && !state.was_open;
    }
    if clear_focus && let Some(mut focus) = world.get_resource_mut::<InputFocus>() {
        focus.clear();
    }
    if let Some(expected) = choose {
        choose_package(world, expected);
    }
    {
        let mut state = world.resource_mut::<PluginManagerState>();
        if state.open && !state.refreshed && !state.busy && native_ready {
            if let Err(error) = state.refresh() {
                state.status = error;
            }
        }
        // 关闭点击与Esc所在帧继续持有输入；下一静默帧才将键鼠交回工作台。
        state.captured = state.open || state.was_open;
        state.was_open = state.open;
    }
    if world.resource::<PluginManagerState>().captures_input() {
        if let Some(mut guard) = world.get_resource_mut::<EditorTextInputGuard>() {
            guard.block_shortcuts = true;
        }
        if let Some(mut keys) = world.get_resource_mut::<ButtonInput<KeyCode>>() {
            let edges: Vec<_> = keys
                .get_just_pressed()
                .chain(keys.get_just_released())
                .copied()
                .collect();
            for key in edges {
                keys.clear_just_pressed(key);
                keys.clear_just_released(key);
            }
        }
    }
}

fn choose_package(world: &mut World, expected: Option<String>) {
    #[cfg(windows)]
    use bevy::window::{PrimaryWindow, RawHandleWrapper};
    if world.contains_non_send::<PluginPackagePicker>() {
        return;
    }
    #[cfg(windows)]
    let owner = world
        .query_filtered::<&RawHandleWrapper, With<PrimaryWindow>>()
        .iter(world)
        .find_map(crate::workspace_window::primary_hwnd)
        .unwrap_or(0);
    #[cfg(not(windows))]
    let owner = 0;
    let state = world.resource::<PluginManagerState>();
    let scope = state.scope;
    let roots = state.roots.clone();
    let initial = if scope == PluginScope::Global {
        roots.global.clone()
    } else {
        state.root.join("plugins")
    };
    match crate::folder_picker::FolderPicker::start_plugin(owner, initial) {
        Ok(picker) => {
            world.insert_non_send(PluginPackagePicker {
                picker,
                expected,
                scope,
                roots,
            });
            let mut state = world.resource_mut::<PluginManagerState>();
            state.busy = true;
            state.status = "选择当前平台的单个插件文件…".into();
            state.revision = state.revision.wrapping_add(1);
        }
        Err(error) => {
            world.resource_mut::<PluginManagerState>().status = error;
        }
    }
}

pub(super) fn poll(world: &mut World) {
    if let Some(mut picker) = world.remove_non_send::<PluginPackagePicker>() {
        if let Some(result) = picker.picker.poll() {
            let mut state = world.resource_mut::<PluginManagerState>();
            state.busy = false;
            let result = match result {
                Ok(Some(path)) if state.roots == picker.roots && state.scope == picker.scope => {
                    state.install_package_at(
                        picker.scope,
                        std::path::PathBuf::from(path),
                        picker.expected,
                    )
                }
                Ok(Some(_)) => Err("插件选择回执范围已过期，请重新选择".into()),
                Ok(None) => {
                    state.status = "已取消选择，插件保持原状".into();
                    Ok(())
                }
                Err(error) => Err(error),
            };
            if let Err(error) = result {
                state.status = error;
            }
            state.revision = state.revision.wrapping_add(1);
        } else {
            world.insert_non_send(picker);
        }
    }
    let mut archived_resolution = None;
    let result = world
        .resource_mut::<PluginManagerState>()
        .task
        .as_mut()
        .and_then(check_ready);
    if let Some(result) = result {
        let mut state = world.resource_mut::<PluginManagerState>();
        state.task = None;
        state.busy = false;
        if result.roots != state.roots
            || result.scope != state.scope
            || result.sequence != state.sequence
        {
            state.status = "插件操作回执身份已过期，请刷新".into();
        } else {
            match result.result {
                Ok(catalog) => {
                    archived_resolution = catalog.archived_legacy.map(|archived| (archived, result.native_diagnostics));
                    state.rows = catalog.entries;
                    state.loaded_revision = 0;
                    state.status = if catalog.warnings.is_empty() {
                        if let Some(completion) = catalog.completion {
                            completion
                        } else if result.changed {
                            "变更已记录，重开工程后生效；当前工具与作者数据保留".into()
                        } else {
                            if state.scope == PluginScope::Global {
                                "已读取当前用户的全局插件".into()
                            } else {
                                "已读取当前工程及继承的全局插件".into()
                            }
                        }
                    } else {
                        catalog.warnings.join("；")
                    };
                    // 保留当前操作回执；正常迁移说明只在管理器展示，不冒充插件加载错误。
                    if !catalog.notices.is_empty() {
                        state.status.push_str("；迁移提示：");
                        state.status.push_str(&catalog.notices.join("；"));
                    }
                    if state
                        .selected
                        .as_ref()
                        .is_none_or(|id| !state.rows.iter().any(|row| &row.id == id))
                    {
                        state.selected = state.rows.first().map(|row| row.id.clone());
                    }
                }
                Err(error) => {
                    state.status = error;
                }
            }
        }
        state.refreshed = true;
        state.revision = state.revision.wrapping_add(1);
    }
    if let Some((archived, expected_diagnostics)) = archived_resolution {
        let needs_reopen = if world.contains_resource::<super::super::resources::NativeTools>() {
            world.resource_scope(|world, mut native: Mut<super::super::resources::NativeTools>| {
                // 只比较冻结诊断实例与后台已验证的来源；主帧不补做磁盘读取。
                Some(native.diagnostics) != expected_diagnostics
                    || native.clear_archived_legacy(&archived, &world.resource::<PluginManagerState>().rows).is_err()
            })
        } else {
            false
        };
        if needs_reopen {
            let mut state = world.resource_mut::<PluginManagerState>();
            state.status.push_str("；旧加载诊断未匹配，请重开工程后重新检查");
        }
    }
    let native = world.get_resource::<super::super::resources::NativeTools>();
    let projection = native
        .filter(|native| {
            native.ui_revision != world.resource::<PluginManagerState>().loaded_revision
                || Some(native.diagnostics) != world.resource::<PluginManagerState>().native_diagnostics
        })
        .map(|native| {
            let mut states = std::collections::BTreeMap::new();
            for row in &world.resource::<PluginManagerState>().rows {
                let status = if !row.effective {
                    "已被工程副本覆盖".into()
                } else if row
                    .origin
                    .as_ref()
                    .is_some_and(|origin| native.load_origins.get(&row.id) == Some(origin))
                {
                    native
                        .tools
                        .values()
                        .find(|tool| tool.domain == row.id)
                        .map_or_else(
                            || {
                                native
                                    .load_states
                                    .get(&row.id)
                                    .cloned()
                                    .unwrap_or_else(|| "未加载此副本".into())
                            },
                            |tool| {
                                tool.fault.as_ref().map_or_else(
                                    || "已加载".into(),
                                    |error| format!("运行故障：{error}"),
                                )
                            },
                        )
                } else if let Some(error) = &row.error {
                    format!("加载失败：{error}")
                } else if native.discovery_pending {
                    "发现中…".into()
                } else {
                    "未加载此副本；重开工程后采用".into()
                };
                states.insert(row.id.clone(), status);
            }
            let error = native.management_errors.first().map(|first| {
                if native.management_errors.len() == 1 {
                    first.clone()
                } else {
                    format!(
                        "{first}（另有{}项重开诊断）",
                        native.management_errors.len() - 1
                    )
                }
            });
            (native.ui_revision, native.diagnostics, states, error)
        });
    if let Some((revision, diagnostics, states, error)) = projection {
        let mut state = world.resource_mut::<PluginManagerState>();
        state.loaded_revision = revision;
        state.native_diagnostics = Some(diagnostics);
        if state.load_states != states || state.initialization_error != error {
            state.load_states = states;
            state.initialization_error = error;
            state.revision = state.revision.wrapping_add(1);
        }
    }
}
