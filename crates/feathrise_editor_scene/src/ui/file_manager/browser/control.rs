//! 浮窗只提交已核验的浏览意图；选择与原属性、参考骨架和唯一中央预览保持相同合同。

use super::super::{
    components::{FileRow, FileWindowRoot},
    messages::ExtraRequest,
    resources::FileManagerState,
    types::ExtraAction,
};
use super::{components::*, resources::*, types::*};
use crate::{
    resources::{EditorAssetCatalog, SceneEditorLayoutState, SceneEditorState},
    ui::{
        asset_preview::{AssetPreviewFocus, PickProjectAnimationClip, rigging::RigReferenceInput},
        asset_shortcuts::{AssetShortcuts, AssetVisited},
        sidebars::SceneEditorSidebarState,
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::{
    ecs::system::SystemParam,
    input::mouse::{MouseScrollUnit, MouseWheel},
    input_focus::InputFocus,
    prelude::*,
    ui::RelativeCursorPosition,
};
use feathrise_editor_core::AssetBrowserViewMode;
use feathrise_editor_ui::EditorButtonClick;

#[derive(SystemParam)]
pub(super) struct Controls<'w, 's> {
    buttons: Query<
        'w,
        's,
        (Entity, &'static BrowserButton, &'static EditorButtonClick),
        Changed<EditorButtonClick>,
    >,
    choices: Query<
        'w,
        's,
        (&'static BrowserChoice, &'static EditorButtonClick),
        Changed<EditorButtonClick>,
    >,
    expanders: Query<
        'w,
        's,
        (Entity, &'static ClipExpander, &'static EditorButtonClick),
        Changed<EditorButtonClick>,
    >,
    rows: Query<'w, 's, &'static FileRow>,
    overlays: Query<
        'w,
        's,
        (
            Entity,
            &'static BrowserOverlay,
            &'static RelativeCursorPosition,
        ),
    >,
    roots: Query<'w, 's, &'static FileWindowRoot>,
    floating: crate::workbench::WorkbenchFloatingPanels<'w, 's>,
    parents: Query<'w, 's, &'static ChildOf>,
    ime: Option<Res<'w, crate::resources::EditorTextInputGuard>>,
}

pub(super) fn control(
    mut browsers: ResMut<BrowserSessions>,
    mut files: ResMut<FileManagerState>,
    controls: Controls,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    focus: Res<InputFocus>,
    mut wheels: MessageReader<MouseWheel>,
    mut clips: MessageWriter<PickProjectAnimationClip>,
    mut shortcuts: Option<ResMut<AssetShortcuts>>,
    mut navigation: MessageReader<BrowserNavigate>,
    mut extras: MessageReader<ExtraRequest>,
) {
    browsers.clip_intent = None;
    if mouse.get_pressed().next().is_none() && mouse.get_just_released().next().is_none() {
        browsers.release_window = None;
    }
    let wheel: f32 = wheels
        .read()
        .map(|wheel| {
            -wheel.y
                * if wheel.unit == MouseScrollUnit::Line {
                    3.
                } else {
                    1. / 24.
                }
        })
        .sum();
    browsers.service_input = false;
    browsers.service_pointer = false;
    if files.suspended || files.prompt.is_some() {
        browsers.popup = None;
        browsers.service = None;
        extras.clear();
        navigation.clear();
        return;
    }
    if browsers.service.as_ref().is_some_and(|host| {
        !files
            .windows
            .iter()
            .any(|window| window.id == host.window && !window.minimized)
    }) {
        browsers.service = None;
    }
    if browsers.popup.as_ref().is_some_and(|popup| {
        !files
            .windows
            .iter()
            .any(|window| window.id == popup.window && !window.minimized)
    }) {
        browsers.popup = None;
    }
    for request in navigation.read() {
        let Some(window) = files.active_mut(request.window).filter(|window| !window.minimized) else {
            continue;
        };
        let Some(root) = browsers
            .asset_root
            .as_ref()
            .map(|root| crate::ui::external_files::platform_path(root))
        else {
            continue;
        };
        let path = root.join(&request.path);
        let directory = if request.directory {
            path
        } else {
            path.parent().unwrap_or(&root).to_path_buf()
        };
        window.filter.clear();
        window.navigate(directory, true);
        if let Some(session) = browsers.sessions.get_mut(&request.window) {
            session.kind = Default::default();
            // 扫描返回完整目标前保留旧选择；成功后同时采用目录、唯一选择和滚动位置。
            session.reveal = (!request.directory).then(|| (window.revision, request.path.clone()));
        }
        browsers.release_window = Some(request.window);
        browsers.service = None;
    }
    for request in extras.read() {
        if request.action != ExtraAction::References {
            continue;
        }
        if let Some(id) = files.focused.filter(|id| {
            files
                .active(*id)
                .is_some_and(|window| window.path == request.target.directory)
        }) {
            browsers.service = Some(ServiceHost {
                window: id,
                search: true,
                recent: false,
            });
            browsers.service_input = true;
        }
    }
    let focused = files.focused;
    // 真实前景浮层先仲裁；日志/详情盖住文件窗时，几何重叠不能被当成服务鼠标资格。
    let foreground = controls.floating.hovered();
    let top_hovered = foreground
        .and_then(|entity| controls.roots.get(entity).ok())
        .map(|root| root.id);
    if let Some(host) = &browsers.service {
        let host_entity = controls
            .overlays
            .iter()
            .find(|(_, overlay, _)| overlay.window == host.window && overlay.service);
        let focused_inside = host_entity.is_some_and(|(entity, _, _)| {
            let mut current = focus.get();
            for _ in 0..24 {
                if current == Some(entity) {
                    return true;
                }
                current = current
                    .and_then(|entity| controls.parents.get(entity).ok().map(ChildOf::parent));
                if current.is_none() {
                    break;
                }
            }
            false
        });
        let available = focused == Some(host.window)
            && files.menu.is_none()
            && files.gesture.is_none()
            && !files.drag.as_ref().is_some_and(|drag| drag.dragging)
            && (foreground.is_none() || top_hovered == Some(host.window));
        let pointer = available
            && top_hovered == Some(host.window)
            && host_entity.is_some_and(|(_, _, cursor)| cursor.cursor_over());
        browsers.service_input = available && (focused_inside || pointer);
        browsers.service_pointer = pointer;
    }
    if keys.just_pressed(KeyCode::Escape)
        && !controls
            .ime
            .as_ref()
            .is_some_and(|guard| guard.block_shortcuts)
    {
        browsers.release_window = browsers
            .popup
            .as_ref()
            .map(|popup| popup.window)
            .or_else(|| browsers.service.as_ref().map(|host| host.window));
        browsers.popup = None;
        browsers.service = None;
        return;
    }
    if files.menu.is_some() {
        browsers.popup = None;
        return;
    }
    for (entity, button, click) in &controls.buttons {
        if !click.is_clicked() || top_hovered != Some(button.window) {
            continue;
        }
        match button.action {
            BrowserAction::Filter => {
                browsers.popup = Some(BrowserPopup {
                    window: button.window,
                    anchor: entity,
                    kind: PopupKind::Filters,
                    first: 0,
                });
                browsers.service = None;
            }
            BrowserAction::View => {
                if let Some(session) = browsers.sessions.get_mut(&button.window) {
                    session.view = if session.view == AssetBrowserViewMode::List {
                        AssetBrowserViewMode::Thumbnails
                    } else {
                        AssetBrowserViewMode::List
                    };
                }
                if let Some(window) = files.active_mut(button.window) {
                    window.first = 0;
                    window.last_click = None;
                }
            }
            BrowserAction::Search
            | BrowserAction::Missing
            | BrowserAction::Favorites
            | BrowserAction::Recent => {
                let search = matches!(
                    button.action,
                    BrowserAction::Search | BrowserAction::Missing
                );
                let recent = button.action == BrowserAction::Recent;
                let close_shortcuts = !search && browsers
                    .service
                    .as_ref()
                    .is_some_and(|host| host.shows_shortcuts(button.window, recent));
                browsers.popup = None;
                browsers.service = if close_shortcuts {
                    browsers.release_window = Some(button.window);
                    None
                } else {
                    Some(ServiceHost {
                        window: button.window,
                        search,
                        recent,
                    })
                };
                browsers.service_input = true; // 明确工具按钮本身拥有本帧；原搜索系统随后消费同一点击。
                browsers.service_pointer = true;
                if !search && !close_shortcuts && let Some(shortcuts) = shortcuts.as_mut() {
                    shortcuts.show_section(recent);
                }
            }
            BrowserAction::Dismiss => {
                browsers.release_window = Some(button.window);
                browsers.service = None;
                browsers.popup = None;
            }
        }
        return;
    }
    for (anchor, expander, click) in &controls.expanders {
        if !click.is_clicked() {
            continue;
        }
        let Ok(row) = controls.rows.get(expander.row) else {
            continue;
        };
        let Some(window) = files.active(row.window).filter(|window| {
            top_hovered == Some(row.window) && !window.loading && window.revision == row.revision
        }) else {
            continue;
        };
        let Some(session) = browsers
            .sessions
            .get(&row.window)
            .filter(|session| session.applied == window.revision)
        else {
            continue;
        };
        let Some(entry) = row
            .index
            .and_then(|index| session.entries.get(index))
            .filter(|entry| !entry.is_directory)
        else {
            continue;
        };
        if !session
            .animations
            .get(&entry.asset_path)
            .is_some_and(|clips| !clips.is_empty())
        {
            continue;
        }
        browsers.popup = Some(BrowserPopup {
            window: row.window,
            anchor,
            kind: PopupKind::Clips {
                path: entry.asset_path.clone(),
                revision: window.revision,
            },
            first: 0,
        });
        browsers.service = None;
        return;
    }
    if let Some(open) = browsers.popup.as_ref().cloned() {
        let current = files
            .active(open.window)
            .filter(|window| top_hovered == Some(open.window) && !window.loading);
        let valid = current.is_some()
            && match &open.kind {
                PopupKind::Filters => true,
                PopupKind::Clips { revision, .. } => {
                    current.is_some_and(|window| window.revision == *revision)
                }
            };
        if !valid {
            browsers.release_window = Some(open.window);
            browsers.popup = None;
            return;
        }
        for (choice, click) in &controls.choices {
            if !click.is_clicked() || choice.window != open.window {
                continue;
            }
            match &open.kind {
                PopupKind::Filters => {
                    if let Some(filter) = choice.filter {
                        if let Some(session) = browsers.sessions.get_mut(&choice.window) {
                            session.kind = filter;
                        }
                        if let Some(window) = files.active_mut(choice.window) {
                            window.refresh();
                        }
                    }
                }
                PopupKind::Clips { path, .. } => {
                    let index = browsers
                        .sessions
                        .get(&open.window)
                        .and_then(|session| session.animations.get(path))
                        .and_then(|clips| clips.get(open.first + choice.slot))
                        .map(|clip| clip.index)
                        .filter(|index| Some(*index) == choice.clip);
                    if let Some(index) = index {
                        let path = path.clone();
                        browsers.clip_intent = Some((path.clone(), index));
                        clips.write(PickProjectAnimationClip {
                            asset_path: path,
                            clip_index: index,
                        });
                    }
                }
            }
            browsers.release_window = Some(choice.window);
            browsers.popup = None;
            return;
        }
        let over = controls.overlays.iter().any(|(_, overlay, cursor)| {
            !overlay.service && overlay.window == open.window && cursor.cursor_over()
        });
        if mouse.just_pressed(MouseButton::Left) && !over {
            browsers.release_window = Some(open.window);
            browsers.popup = None;
        } else if wheel != 0. && over {
            let total = match &open.kind {
                PopupKind::Filters => FILTERS.len(),
                PopupKind::Clips { path, .. } => browsers
                    .sessions
                    .get(&open.window)
                    .and_then(|session| session.animations.get(path))
                    .map_or(0, |clips| clips.len()),
            };
            let capacity = files.active(open.window).map_or(1, |window| {
                (((window.rect.height() - 150.).max(24.) / 26.).floor() as usize).clamp(1, 10)
            });
            if let Some(open) = browsers.popup.as_mut() {
                open.first = (open.first as f32 + wheel).max(0.) as usize;
                open.first = open.first.min(total.saturating_sub(capacity));
            }
        }
    }
}

pub(super) fn select(
    rows: Query<(&FileRow, &EditorButtonClick), Changed<EditorButtonClick>>,
    files: Res<FileManagerState>,
    mut browsers: ResMut<BrowserSessions>,
    mut catalog: ResMut<EditorAssetCatalog>,
    mut editor: ResMut<SceneEditorState>,
    mut sidebar: ResMut<SceneEditorSidebarState>,
    mut layout: ResMut<SceneEditorLayoutState>,
    mut focus: ResMut<AssetPreviewFocus>,
    workspace: Res<VegetationPresetEditorState>,
    mut visits: MessageWriter<AssetVisited>,
    mut reference: RigReferenceInput,
) {
    let revealed = browsers.revealed.take();
    if files.suspended || files.prompt.is_some() || files.menu.is_some() {
        return;
    }
    // 后到的显式文件点击优先。两种来源验证同一目录修订后共用原属性和预览服务。
    let clicked = rows.iter().find_map(|(row, click)| {
        if !click.is_clicked()
            || files.focused != Some(row.window)
            || browsers.service_open(row.window)
        {
            return None;
        }
        let window = files
            .active(row.window)
            .filter(|window| !window.loading && window.revision == row.revision)?;
        let session = browsers
            .sessions
            .get(&row.window)
            .filter(|session| session.assets && session.applied == window.revision)?;
        let index = row.index?;
        let entry = session
            .entries
            .get(index)
            .filter(|entry| !entry.is_directory)?;
        (row.path.as_ref() == Some(&window.entries.get(index)?.path)).then_some(entry.asset_path.as_str())
    });
    let located = || {
        let (id, revision, index) = revealed?;
        if files.focused != Some(id) || browsers.service_open(id) {
            return None; // 等待期间切窗或打开另一面板，不能用旧定位抢回属性。
        }
        let window = files.active(id).filter(|window| !window.minimized && !window.loading && window.revision == revision)?;
        if !window.selection.contains(&window.entries.get(index)?.path) {
            return None;
        }
        browsers.asset_path(id, index)
    };
    let Some(path) = clicked.or_else(located) else { return; };
    catalog.cancel_pending_reveal();
    if reference.select(path) {
        return;
    }
    editor.selected_asset = Some(path.to_owned());
    sidebar.reveal_asset_properties();
    layout.inspector_visible = true;
    focus.browse(workspace.active_tab);
    visits.write(AssetVisited { path: path.to_owned(), is_directory: false });
    editor.status = format!("已选择 assets/{path}；双击在中央打开");
}
