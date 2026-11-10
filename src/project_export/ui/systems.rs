//! 导出弹窗只投递明确请求并非阻塞轮询自己的目录任务；实际构建与发布由后台单槽完成。

use super::super::{components::*, messages::{CancelExport, RequestExport}, resources::ExportState};
use super::resources::{ExportFolderPicker, ExportOverlayState};
use bevy::{input::mouse::{MouseScrollUnit, MouseWheel}, input_focus::{FocusCause, InputFocus}, prelude::*, tasks::{IoTaskPool, futures::check_ready},
    text::EditableText, ui::{InteractionDisabled, RelativeCursorPosition}, window::{PrimaryWindow, RawHandleWrapper}};
use feathrise_editor_ui::{EditorButtonClick, colors as c};
use std::path::PathBuf;

pub(in crate::project_export) fn handle(world: &mut World) {
    poll_directory_tasks(world);
    scroll_body(world);
    let was_open = world.resource::<ExportOverlayState>().was_open;
    let blocked = super::super::systems::modal_blocked(world);
    if blocked {
        // 关闭确认、关于、日志及先认领的插件管理器优先；只隐藏弹窗，不取消仍有自己收口责任的后台导出。
        world.resource_mut::<ExportState>().open = false;
        world.resource_mut::<ExportOverlayState>().close_when_idle = false;
        for mut click in world.query_filtered::<&mut EditorButtonClick, With<ExportAction>>().iter_mut(world) {
            if click.is_clicked() { click.invalidate(); }
        }
    }
    let mut open = world.resource::<ExportState>().open;
    let busy = world.resource::<ExportState>().busy();
    let picking = picker_active(world);
    if open && !was_open {
        let destination = world.resource::<ExportState>().destination.clone();
        sync_destination(world, &destination);
        world.resource_mut::<ExportOverlayState>().close_when_idle = false;
        let field = world.query_filtered::<Entity, With<ExportDestination>>().iter(world).next();
        if let Some(field) = field && !busy && !picking {
            world.resource_mut::<InputFocus>().set(field, FocusCause::Navigated);
        }
    }
    if open {
        let field = world.query_filtered::<&EditableText, With<ExportDestination>>().iter(world).next()
            .map(|text| (text.value().to_string(), text.is_composing()));
        let composing = field.as_ref().is_some_and(|(_, composing)| *composing);
        if !busy && !picking && let Some((value, _)) = field {
            let mut state = world.resource_mut::<ExportState>();
            if state.destination != value { state.destination = value; }
        }
        let action = world.query::<(&ExportAction, &EditorButtonClick, Has<InteractionDisabled>, Option<&InheritedVisibility>)>()
            .iter(world).find_map(|(action, click, disabled, visible)|
                (!disabled && visible.is_none_or(|visible| visible.get()) && click.is_clicked()).then_some(*action));
        let escape = !composing && world.resource::<ButtonInput<KeyCode>>().just_pressed(KeyCode::Escape);
        if escape || matches!(action, Some(ExportAction::Close | ExportAction::Cancel)) {
            request_close(world, busy, picking);
        } else if !composing {
            match action {
                Some(ExportAction::Start) if !busy && !picking => {
                    let destination = world.resource::<ExportState>().destination.trim().to_owned();
                    if destination.is_empty() {
                        let mut state = world.resource_mut::<ExportState>();
                        state.failed = Some("请填写导出目录。".into());
                    } else {
                        world.resource_mut::<InputFocus>().clear();
                        world.write_message(RequestExport { destination: PathBuf::from(destination) });
                    }
                }
                Some(ExportAction::Browse) if !busy && !picking => choose_directory(world),
                Some(ExportAction::OpenDirectory) if !busy && !picking => open_directory(world),
                _ => {}
            }
        }
        if world.resource::<ExportOverlayState>().close_when_idle && !busy && !picker_active(world) {
            world.resource_mut::<ExportState>().open = false;
            world.resource_mut::<ExportOverlayState>().close_when_idle = false;
        }
        open = world.resource::<ExportState>().open;
    }
    if (!open || busy || picker_active(world)) && world.resource::<InputFocus>().get().is_some_and(|focused| {
        world.get::<ExportDestination>(focused).is_some()
    }) {
        world.resource_mut::<InputFocus>().clear();
    }
    let picker = picker_active(world);
    world.resource_mut::<ExportOverlayState>().capture(open, picker);
    if world.resource::<ExportOverlayState>().captured {
        // 官方文本处理已经执行，再消费边沿以隔离场景快捷键；真实按住状态由原系统处理释放。
        if let Some(mut guard) = world.get_resource_mut::<feathrise_editor_core::EditorTextInputGuard>() {
            guard.block_shortcuts = true;
        }
        if let Some(mut overlay) = world.get_resource_mut::<feathrise_editor_core::EditorHostOverlay>() {
            overlay.active = true;
        }
        let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
        let edges: Vec<_> = keys.get_just_pressed().chain(keys.get_just_released()).copied().collect();
        for key in edges { keys.clear_just_pressed(key); keys.clear_just_released(key); }
    }
}

fn request_close(world: &mut World, busy: bool, picking: bool) {
    if picking {
        if let Some(picker) = &world.non_send::<ExportFolderPicker>().0 { picker.cancel(); }
    }
    if busy { world.write_message(CancelExport); }
    if busy || picking {
        // “取消”是取消请求；只有真实worker和STA选择器都退出后才能隐藏进行中的面板。
        world.resource_mut::<ExportOverlayState>().close_when_idle = true;
    } else {
        world.resource_mut::<ExportState>().open = false;
    }
}

fn choose_directory(world: &mut World) {
    let owner = world.query_filtered::<&RawHandleWrapper, With<PrimaryWindow>>().iter(world)
        .next().and_then(crate::workspace_window::primary_hwnd);
    let Some(owner) = owner else {
        world.resource_mut::<ExportState>().failed = Some("当前窗口尚未准备好；可直接填写导出目录。".into());
        return;
    };
    let initial = PathBuf::from(&world.resource::<ExportState>().destination);
    match crate::folder_picker::FolderPicker::start_export(owner, initial) {
        Ok(picker) => {
            world.non_send_mut::<ExportFolderPicker>().0 = Some(picker);
            world.resource_mut::<InputFocus>().clear();
        }
        Err(error) => world.resource_mut::<ExportState>().failed = Some(error),
    }
}

fn open_directory(world: &mut World) {
    if world.resource::<ExportOverlayState>().open_directory.is_some() { return; }
    let Some(directory) = world.resource::<ExportState>().receipt.as_ref().map(|receipt| receipt.directory.clone()) else { return; };
    world.resource_mut::<ExportOverlayState>().open_directory = Some(IoTaskPool::get().spawn(async move {
        #[cfg(target_os = "windows")]
        let mut command = std::process::Command::new("explorer.exe");
        #[cfg(not(target_os = "windows"))]
        let mut command = std::process::Command::new("xdg-open");
        command.arg(directory).spawn().map(|_| ()).map_err(|error| format!("无法打开输出目录：{error}"))
    }));
}

fn poll_directory_tasks(world: &mut World) {
    let result = world.non_send_mut::<ExportFolderPicker>().0.as_mut().and_then(crate::folder_picker::FolderPicker::poll);
    if let Some(result) = result {
        world.non_send_mut::<ExportFolderPicker>().0 = None;
        let accepts = world.resource::<ExportState>().open && !world.resource::<ExportState>().busy()
            && !world.resource::<ExportOverlayState>().close_when_idle;
        if accepts {
            match result {
                Ok(Some(path)) => {
                    world.resource_mut::<ExportState>().destination = path.clone();
                    sync_destination(world, &path);
                }
                Err(error) => world.resource_mut::<ExportState>().failed = Some(error),
                Ok(None) => {}
            }
        }
    }
    let result = world.resource_mut::<ExportOverlayState>().open_directory.as_mut().and_then(check_ready);
    if let Some(result) = result {
        world.resource_mut::<ExportOverlayState>().open_directory = None;
        if let Err(error) = result { world.resource_mut::<ExportState>().failed = Some(error); }
    }
}

fn picker_active(world: &World) -> bool { world.non_send::<ExportFolderPicker>().0.is_some() }

fn scroll_body(world: &mut World) {
    let (lines, pixels) = world.resource_scope(|world, mut view: Mut<ExportOverlayState>| {
        let Some(messages) = world.get_resource::<Messages<MouseWheel>>() else { return (0., 0.); };
        view.wheel_cursor.read(messages).filter(|wheel| wheel.y.is_finite()).fold((0., 0.), |(lines, pixels), wheel| {
            if wheel.unit == MouseScrollUnit::Line { (lines + wheel.y, pixels) }
            else { (lines, pixels + wheel.y) }
        })
    });
    if !world.resource::<ExportState>().open || super::super::systems::modal_blocked(world)
        || (lines == 0. && pixels == 0.) { return; }
    for (cursor, node, mut scroll) in world.query_filtered::<(&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition), With<ExportBody>>().iter_mut(world) {
        if !cursor.cursor_over() || node.is_empty() { continue; }
        let maximum = ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.);
        // 像素滚轮先转为UI逻辑尺寸，行滚轮始终使用编辑器统一的24逻辑像素。
        let delta = lines * 24. + pixels * node.inverse_scale_factor();
        scroll.y = (scroll.y - delta).clamp(0., maximum);
    }
}

fn sync_destination(world: &mut World, value: &str) {
    for mut field in world.query_filtered::<&mut EditableText, With<ExportDestination>>().iter_mut(world) {
        if field.value() != value { field.clear(); field.editor.set_text(value); }
    }
    world.resource_mut::<ExportOverlayState>().last_destination = value.to_owned();
}

pub(in crate::project_export) fn present(world: &mut World) {
    let locale = world.get_resource::<feathrise_editor_ui::localization::EditorLocale>().copied().unwrap_or_default();
    let state = world.resource::<ExportState>();
    let (open, busy, destination, failed, receipt) = (state.open, state.busy(), state.destination.clone(), state.failed.clone(), state.receipt.clone());
    let mut status = failed.as_deref().unwrap_or(&state.status).to_owned();
    if let Some(warning) = receipt.as_ref().and_then(|receipt| receipt.warning.as_deref()) {
        if !status.contains(warning) { status.push('\n'); status.push_str(warning); }
    }
    let status = visible_status(&locale.editor_notice(&status));
    let picking = picker_active(world);
    for mut node in world.query_filtered::<&mut Node, With<ExportOverlay>>().iter_mut(world) {
        let display = if open { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    if !open { return; }
    if world.resource::<ExportOverlayState>().last_destination != destination { sync_destination(world, &destination); }
    let profile = world.get_resource::<feathrise_editor_core::ProjectLaunchSettings>()
        .map_or(feathrise_editor_core::ProjectBuildProfile::Debug, |launch| launch.profile);
    let profile = locale.format("project.export.profile", &[("profile", profile.label().into())]);
    for (mut text, status_node, profile_node, label) in world.query::<(&mut Text, Has<ExportStatus>, Has<ExportProfile>, Option<&ExportActionLabel>)>().iter_mut(world) {
        let next = if status_node { Some(status.as_str()) } else if profile_node { Some(profile.as_str()) }
            else { label.map(|label| locale.editor_label(match label.0 {
                ExportAction::Browse => "选择目录…",
                ExportAction::OpenDirectory => "打开输出目录",
                ExportAction::Start => "开始导出",
                ExportAction::Close => "×",
                ExportAction::Cancel if busy || picking => "取消导出",
                ExportAction::Cancel => "关闭 (Esc)",
            })) };
        if let Some(next) = next && text.as_str() != next { text.0 = next.to_owned(); }
    }
    for mut color in world.query_filtered::<&mut TextColor, With<ExportStatus>>().iter_mut(world) {
        let next = if failed.is_some() { Color::srgb(0.92, 0.45, 0.40) } else { c::TEXT_PRIMARY };
        if color.0 != next { color.0 = next; }
    }
    let changes: Vec<_> = world.query::<(Entity, &ExportAction, Has<InteractionDisabled>)>().iter(world)
        .filter_map(|(entity, action, disabled)| {
            let next = match action {
                ExportAction::Start => busy || picking || destination.trim().is_empty(),
                ExportAction::Browse => busy || picking,
                ExportAction::OpenDirectory => busy || picking || receipt.is_none(),
                ExportAction::Close | ExportAction::Cancel => false,
            };
            (disabled != next).then_some((entity, next))
        }).collect();
    for (entity, disabled) in changes {
        let mut entity = world.entity_mut(entity);
        if disabled { entity.insert(InteractionDisabled); } else { entity.remove::<InteractionDisabled>(); }
    }
    let fields: Vec<_> = world.query_filtered::<(Entity, Has<InteractionDisabled>), With<ExportDestination>>().iter(world)
        .filter_map(|(entity, disabled)| (disabled != (busy || picking)).then_some(entity)).collect();
    for field in fields {
        if busy || picking { world.entity_mut(field).insert(InteractionDisabled); }
        else { world.entity_mut(field).remove::<InteractionDisabled>(); }
    }
}

/// 完整错误保留在导出状态和日志中，浮窗只排版有界首段，避免超长路径挤掉操作区。
pub(super) fn visible_status(value: &str) -> String {
    let mut end = value.len().min(2048);
    while !value.is_char_boundary(end) { end -= 1; }
    if end == value.len() { value.to_owned() } else { format!("{}…（完整输出见日志）", &value[..end]) }
}
