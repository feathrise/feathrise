//! 工程页输入、轻量回显与真实控件探针；磁盘和进程工作只通过宿主消息请求。

use super::{
    components::*,
    resources::Presentation,
    types::{ProbeButton, ProbeField},
    *,
};
use bevy::{
    ecs::system::SystemParam,
    input_focus::InputFocus,
    prelude::*,
    text::EditableText,
    ui::{CalculatedClip, InteractionDisabled, RelativeCursorPosition},
    window::{FileDragAndDrop, Ime, PrimaryWindow, RawHandleWrapper},
};
use feathrise_editor_map::workbench::{WorkbenchCentralPageHost, WorkbenchFloatingPanels, WorkbenchHostPageState};
use feathrise_editor_ui::colors as c;

pub(super) fn show(state: &mut Presentation, page: Page) {
    state.page = page;
    state.menu = false;
    state.error.clear();
    match page {
        Page::Start => state.start_open = true,
        Page::New => state.new_open = true,
        Page::Open => state.open_open = true,
        Page::Editor | Page::Empty => {}
    }
}

fn set_field(fields: &mut Query<(&Input, &mut EditableText)>, kind: Input, value: &str) {
    for (field, mut text) in fields.iter_mut() {
        if *field == kind && text.value() != value {
            text.clear();
            text.editor.set_text(value);
        }
    }
}

pub(super) fn adopt(
    model: Res<WorkspaceModel>,
    mut state: ResMut<Presentation>,
    mut fields: Query<(&Input, &mut EditableText)>,
    host: Res<WorkbenchHostPageState>,
) {
    if state.native_tab_revision != host.native_tab_revision {
        state.native_tab_revision = host.native_tab_revision;
        state.page = Page::Editor;
    }
    if state.project != model.project {
        state.project = model.project.clone();
        state.page = if model.project.is_some() {
            Page::Editor
        } else {
            Page::Start
        };
        state.start_open = true;
        state.error.clear();
    }
    if state.applied_default.as_deref() != Some(model.default_directory.as_str()) {
        let previous = state.applied_default.clone().unwrap_or_default();
        let actual = fields
            .iter()
            .find(|(kind, _)| **kind == Input::Parent)
            .map(|(_, text)| text.value().to_string())
            .unwrap_or_else(|| state.parent.clone());
        // 仅空表单或仍使用旧默认值的表单跟随偏好；用户自填父目录不会被后台回执覆盖。
        if actual.is_empty() || actual == previous {
            state.parent = model.default_directory.clone();
            set_field(&mut fields, Input::Parent, &state.parent);
        }
        state.default_directory = model.default_directory.clone();
        set_field(
            &mut fields,
            Input::DefaultDirectory,
            &state.default_directory,
        );
        state.applied_default = Some(model.default_directory.clone());
    }
    if state.recent != model.recent {
        state.recent = model.recent.clone();
        state.recent_revision = state.recent_revision.wrapping_add(1);
    }
    let fills = std::mem::take(&mut state.fills);
    for (field, value) in fills {
        let kind = match field {
            WorkspaceField::NewParent => {
                state.parent = value.clone();
                Input::Parent
            }
            WorkspaceField::OpenPath => {
                state.open_path = value.clone();
                Input::Open
            }
            WorkspaceField::DefaultDirectory => {
                state.default_directory = value.clone();
                Input::DefaultDirectory
            }
        };
        set_field(&mut fields, kind, &value);
    }
}

fn submit_create(state: &mut Presentation, out: &mut MessageWriter<WorkspaceCommand>) {
    let name = state.name.trim();
    if name.is_empty()
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        state.error = "请填写有效工程名称，名称中不能包含路径分隔符。".into();
        return;
    }
    if state.parent.trim().is_empty() {
        state.error = "请选择父目录。".into();
        return;
    }
    out.write(WorkspaceCommand::Create {
        parent: std::path::PathBuf::from(state.parent.trim()),
        name: name.to_owned(),
        game_view: state.game_view,
    });
}

pub(super) fn input(
    model: Res<WorkspaceModel>,
    about: Res<AboutState>,
    close: Option<Res<crate::native_project_close::NativeProjectCloseState>>,
    export: Option<Res<crate::project_export::ui::ExportOverlayState>>,
    plugins: Option<Res<crate::native_plugins::management::PluginManagerState>>,
    mut state: ResMut<Presentation>,
    fields: Query<(Entity, &Input, &EditableText)>,
    buttons: Query<
        (
            &Action,
            &feathrise_editor_ui::EditorButtonClick,
            &InheritedVisibility,
        ),
        Changed<feathrise_editor_ui::EditorButtonClick>,
    >,
    recents: Query<
        (
            &RecentAction,
            &feathrise_editor_ui::EditorButtonClick,
            &InheritedVisibility,
        ),
        Changed<feathrise_editor_ui::EditorButtonClick>,
    >,
    menus: Query<&RelativeCursorPosition, With<MenuNode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut ime: MessageReader<Ime>,
    mut focus: ResMut<InputFocus>,
    mut host: ResMut<WorkbenchHostPageState>,
    mut out: MessageWriter<WorkspaceCommand>,
) {
    // 开始/新建/打开页与作者页共用模态输入门，取消后的静默释放帧也不能执行底层旧点击。
    if about.captures_input() {
        state.menu = false;
        host.menu_open = true;
        ime.clear();
        return;
    }
    if close.as_ref().is_some_and(|close| close.captures_input())
        || export.as_ref().is_some_and(|export| export.captures_input())
        || plugins.as_ref().is_some_and(|plugins| plugins.captures_input()) {
        ime.clear();
        return;
    }
    let ime_frame = ime
        .read()
        .any(|event| !matches!(event, Ime::Enabled { .. }));
    ime.clear();
    let composing = ime_frame || fields.iter().any(|(_, _, text)| text.is_composing());
    for (_, field, text) in &fields {
        let value = text.value();
        match field {
            Input::Name => state.name = value.to_string(),
            Input::Parent => state.parent = value.to_string(),
            Input::Open => state.open_path = value.to_string(),
            Input::DefaultDirectory => state.default_directory = value.to_string(),
        }
    }
    let action = buttons.iter().find_map(|(action, click, visible)| {
        (visible.get() && click.is_clicked()).then_some(*action)
    });
    let previous_menu = state.menu;
    if !composing {
        if let Some(action) = action {
            state.error.clear();
            match action {
                Action::ToggleMenu => state.menu = !state.menu,
                Action::Show(page) => {
                    show(&mut state, page);
                    focus.clear();
                }
                Action::CloseTab(page) => {
                    match page {
                        Page::Start => state.start_open = false,
                        Page::New => state.new_open = false,
                        Page::Open => state.open_open = false,
                        _ => {}
                    }
                    if state.page == page {
                        state.page = if model.project.is_some() {
                            Page::Editor
                        } else if state.start_open {
                            Page::Start
                        } else {
                            Page::Empty
                        };
                    }
                    focus.clear();
                }
                Action::Create if !model.busy => submit_create(&mut state, &mut out),
                Action::SelectGameView(view) if !model.busy => state.game_view = view,
                Action::Open if !model.busy => {
                    if state.open_path.trim().is_empty() {
                        state.error = "请选择工程目录或 editor-project.json。".into();
                    } else {
                        out.write(WorkspaceCommand::Open(state.open_path.trim().into()));
                    }
                }
                Action::CloseProject if !model.busy && model.project.is_some() => {
                    state.menu = false;
                    out.write(WorkspaceCommand::CloseProject);
                }
                Action::QuitApp => {
                    state.menu = false;
                    out.write(WorkspaceCommand::QuitApp);
                }
                Action::SetStartup if !model.busy && model.project.is_some() => {
                    state.menu = false;
                    out.write(WorkspaceCommand::SetStartupMap);
                }
                Action::ExportProject if !model.busy && model.project.is_some() => {
                    state.menu = false;
                    out.write(WorkspaceCommand::ExportProject);
                }
                Action::Plugins if !model.busy => {
                    state.menu = false;
                    out.write(WorkspaceCommand::OpenPlugins);
                }
                Action::Browse(field) if !model.busy => {
                    let value = match field {
                        WorkspaceField::NewParent => &state.parent,
                        WorkspaceField::OpenPath => &state.open_path,
                        WorkspaceField::DefaultDirectory => &state.default_directory,
                    };
                    out.write(WorkspaceCommand::Browse {
                        field,
                        initial: value.into(),
                    });
                }
                Action::SaveDefault if !model.busy => {
                    out.write(WorkspaceCommand::SaveDefaultDirectory(
                        state.default_directory.clone(),
                    ));
                }
                Action::ResetDefault if !model.busy => {
                    out.write(WorkspaceCommand::SaveDefaultDirectory("~/feathrise".into()));
                }
                Action::CancelOperation if model.busy && model.cancellable => {
                    out.write(WorkspaceCommand::CancelOperation);
                }
                _ => {}
            }
        }
        if !model.busy {
            for (recent, click, visible) in &recents {
                if visible.get() && click.is_clicked() {
                    if recent.remove {
                        out.write(WorkspaceCommand::RemoveRecent(recent.path.clone()));
                    } else {
                        out.write(WorkspaceCommand::Open(recent.path.clone()));
                    }
                    break;
                }
            }
        }
        if keys.just_pressed(KeyCode::Escape) && state.menu {
            state.menu = false;
        } else if keys.just_pressed(KeyCode::Enter) && !model.busy && !host.settings_open {
            if state.page == Page::New {
                submit_create(&mut state, &mut out);
            } else if state.page == Page::Open && !state.open_path.trim().is_empty() {
                out.write(WorkspaceCommand::Open(state.open_path.trim().into()));
            }
        }
    } else if action.is_some() {
        state.error = "请先完成或取消输入法候选。".into();
    }
    if previous_menu
        && mouse.get_just_pressed().next().is_some()
        && !menus.iter().any(RelativeCursorPosition::cursor_over)
        && action != Some(Action::ToggleMenu)
    {
        state.menu = false;
    }
    host.active = state.page != Page::Editor;
    if previous_menu || state.menu {
        state.menu_releasing = true;
    }
    if !state.menu
        && mouse.get_pressed().next().is_none()
        && mouse.get_just_pressed().next().is_none()
        && mouse.get_just_released().next().is_none()
    {
        state.menu_releasing = false;
    }
    host.menu_open = state.menu_releasing;
    if host.active && !host.settings_open {
        // 官方文本输入已处理原键盘消息；只消费作者快捷键边沿，不重置实际按住的修饰键。
        let edges: Vec<_> = keys
            .get_just_pressed()
            .copied()
            .filter(|key| *key != KeyCode::F12)
            .collect();
        for key in edges {
            keys.clear_just_pressed(key);
        }
    }
}

pub(super) fn present(
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    mut commands: Commands,
    model: Res<WorkspaceModel>,
    state: Res<Presentation>,
    mut bodies: Query<
        &mut Node,
        (
            With<WorkbenchCentralPageHost>,
            Without<PageNode>,
            Without<TabNode>,
            Without<MenuNode>,
        ),
    >,
    mut pages: Query<
        (&PageNode, &mut Node),
        (
            Without<WorkbenchCentralPageHost>,
            Without<TabNode>,
            Without<MenuNode>,
        ),
    >,
    mut tabs: Query<
        (&TabNode, &mut Node, &mut BorderColor),
        (
            Without<WorkbenchCentralPageHost>,
            Without<PageNode>,
            Without<MenuNode>,
        ),
    >,
    mut menus: Query<
        &mut Node,
        (
            With<MenuNode>,
            Without<WorkbenchCentralPageHost>,
            Without<PageNode>,
            Without<TabNode>,
        ),
    >,
    mut targets: Query<&mut Text, (With<TargetText>, Without<StatusText>)>,
    mut statuses: Query<&mut Text, (With<StatusText>, Without<TargetText>)>,
    buttons: Query<(Entity, &Action, Has<InteractionDisabled>)>,
) {
    for mut body in &mut bodies {
        body.display = if state.page == Page::Editor {
            Display::None
        } else {
            Display::Flex
        };
    }
    for (page, mut node) in &mut pages {
        node.display = if state.page == page.0 {
            Display::Flex
        } else {
            Display::None
        };
    }
    for (tab, mut node, mut border) in &mut tabs {
        let open = match tab.0 {
            Page::Start => state.start_open,
            Page::New => state.new_open,
            Page::Open => state.open_open,
            _ => false,
        };
        node.display = if open { Display::Flex } else { Display::None };
        *border = BorderColor::all(if tab.0 == state.page {
            c::TEXT_ACCENT
        } else {
            c::COPPER_BORDER_WEAK
        });
    }
    for mut node in &mut menus {
        node.display = if state.menu {
            Display::Flex
        } else {
            Display::None
        };
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    let target = if state.name.trim().is_empty() {
        locale.text("workspace.enter_project_name").into()
    } else {
        let destination = crate::project::display_path(
            &std::path::Path::new(state.parent.trim()).join(state.name.trim()),
        );
        format!("{}\n{destination}", locale.editor_label(state.game_view.label()))
    };
    for mut text in &mut targets {
        if text.as_str() != target {
            **text = target.clone();
        }
    }
    let status = if state.error.is_empty() {
        &model.status
    } else {
        &state.error
    };
    let status = locale.editor_notice(status);
    for mut text in &mut statuses {
        if text.as_str() != status.as_ref() {
            **text = status.as_ref().to_owned();
        }
    }
    for (entity, action, disabled) in &buttons {
        let enabled = match action {
            Action::Create
            | Action::SelectGameView(_)
            | Action::Open
            | Action::Browse(_)
            | Action::SaveDefault
            | Action::ResetDefault => !model.busy,
            Action::CloseProject | Action::SetStartup | Action::ExportProject => !model.busy && model.project.is_some(),
            Action::Plugins => !model.busy,
            Action::CancelOperation => model.busy && model.cancellable,
            _ => true,
        };
        if enabled && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
        if !enabled && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
}

pub(super) fn follow_native_navigation(
    host: Res<WorkbenchHostPageState>,
    mut state: ResMut<Presentation>,
) {
    if state.native_tab_revision != host.native_tab_revision {
        state.native_tab_revision = host.native_tab_revision;
        state.page = Page::Editor;
    }
}

pub(super) fn disable_busy_inputs(
    mut commands: Commands,
    model: Res<WorkspaceModel>,
    fields: Query<(Entity, Has<InteractionDisabled>), With<Input>>,
    recent: Query<(Entity, Has<InteractionDisabled>), (With<RecentAction>, Without<Input>)>,
    mut focus: ResMut<InputFocus>,
) {
    for (entity, disabled) in fields.iter().chain(recent.iter()) {
        if model.busy && !disabled {
            commands
                .entity(entity)
                .insert((InteractionDisabled, Pickable::IGNORE));
            if focus.get() == Some(entity) {
                focus.clear();
            }
        } else if !model.busy && disabled {
            commands
                .entity(entity)
                .remove::<InteractionDisabled>()
                .insert(Pickable::default());
        }
    }
}

pub(super) fn cancel_visibility(
    model: Res<WorkspaceModel>,
    mut buttons: Query<(&Action, &mut Node)>,
) {
    for (action, mut node) in &mut buttons {
        if *action == Action::CancelOperation {
            node.display = if model.busy && model.cancellable {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
}

#[derive(SystemParam)]
pub(super) struct PageOverlays<'w> {
    host: Res<'w, WorkbenchHostPageState>,
    model: Option<Res<'w, feathrise_editor_map::workbench::WorkbenchModel>>,
    overlay: Option<Res<'w, feathrise_editor_core::EditorHostOverlay>>,
    close: Option<Res<'w, crate::native_project_close::NativeProjectCloseState>>,
    export: Option<Res<'w, crate::project_export::ui::ExportOverlayState>>,
    plugins: Option<Res<'w, crate::native_plugins::management::PluginManagerState>>,
    about: Option<Res<'w, AboutState>>,
}

impl PageOverlays<'_> {
    fn captures_input(&self) -> bool {
        self.host.settings_open
            || self.host.menu_open
            || self.model.as_ref().is_some_and(|model| model.modal)
            || self.overlay.as_ref().is_some_and(|overlay| overlay.active)
            || self.close.as_ref().is_some_and(|close| close.captures_input())
            || self.export.as_ref().is_some_and(|export| export.captures_input())
            || self.plugins.as_ref().is_some_and(|plugins| plugins.captures_input())
            || self.about.as_ref().is_some_and(|about| about.captures_input())
    }
}

pub(super) fn open_dropped_project(
    mut drops: MessageReader<FileDragAndDrop>,
    windows: Query<(Entity, &Window, Option<&RawHandleWrapper>), With<PrimaryWindow>>,
    pages: Query<(&PageNode, &ComputedNode, &UiGlobalTransform, &InheritedVisibility, Option<&CalculatedClip>)>,
    model: Res<WorkspaceModel>,
    mut state: ResMut<Presentation>,
    overlays: PageOverlays,
    mut out: MessageWriter<WorkspaceCommand>,
) {
    if drops.is_empty() {
        return;
    }
    // 被遮挡、忙碌或切走的页面丢弃本帧拖放，恢复后不能重放旧输入。
    if state.page != Page::Start || model.busy || overlays.captures_input() {
        drops.clear();
        return;
    }
    let Ok((entity, window, raw)) = windows.single() else {
        drops.clear();
        return;
    };
    let mut paths = drops.read().filter_map(|event| match event {
        FileDragAndDrop::DroppedFile { window, path_buf } if *window == entity => Some(path_buf),
        _ => None,
    });
    // 系统按路径发送拖放事件，单次只接受一个工程，避免替作者从多选结果中挑选。
    let path = match (paths.next(), paths.next()) {
        (Some(path), None) => Some(path.clone()),
        _ => None,
    };
    drops.clear();
    let Some(path) = path else { return };
    let Some(position) = drop_cursor_position(window, raw) else { return };
    if pages.iter().any(|(page, node, transform, visible, clip)| {
        page.0 == Page::Start && visible.get() && !node.is_empty()
            && node.contains_point(*transform, position)
            && clip.is_none_or(|clip| clip.contains_point(position))
    }) {
        // 保留原始路径，目录与清单检查沿用宿主的后台打开流程。
        state.error.clear();
        out.write(WorkspaceCommand::Open(path));
    }
}

fn drop_cursor_position(window: &Window, _raw: Option<&RawHandleWrapper>) -> Option<Vec2> {
    #[cfg(windows)]
    if let Some(raw) = _raw {
        use windows_sys::Win32::{
            Foundation::{HWND, POINT}, Graphics::Gdi::ScreenToClient,
            UI::WindowsAndMessaging::GetCursorPos,
        };
        let hwnd = crate::workspace_window::primary_hwnd(raw)? as HWND;
        let mut cursor = POINT::default();
        // Windows 的系统拖放期间可能没有普通鼠标移动事件，按接收窗口的物理像素重新取样。
        let valid = unsafe { GetCursorPos(&mut cursor) != 0 && ScreenToClient(hwnd, &mut cursor) != 0 };
        return valid.then(|| Vec2::new(cursor.x as f32, cursor.y as f32));
    }
    window.physical_cursor_position()
}

pub(super) fn scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    state: Res<Presentation>,
    overlays: PageOverlays,
    panels: WorkbenchFloatingPanels,
    mut pages: Query<(
        &PageNode,
        &RelativeCursorPosition,
        &ComputedNode,
        &mut ScrollPosition,
    )>,
) {
    use bevy::input::mouse::MouseScrollUnit;
    let (lines, pixels) = wheel.read().filter(|event| event.y.is_finite()).fold(
        (0., 0.),
        |(lines, pixels), event| match event.unit {
            MouseScrollUnit::Line => (lines + event.y, pixels),
            MouseScrollUnit::Pixel => (lines, pixels + event.y),
        },
    );
    // 消费本帧滚轮后才判定归属；鼠标在实际浮层内时只滚动上层正文。
    if overlays.captures_input() || panels.hovered().is_some() {
        return;
    }
    for (page, cursor, node, mut offset) in &mut pages {
        if page.0 != state.page || !cursor.cursor_over() || node.is_empty() {
            continue;
        }
        let maximum =
            ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.);
        offset.y =
            (offset.y - lines * 24. - pixels * node.inverse_scale_factor()).clamp(0., maximum);
    }
}

pub(super) fn measure(
    state: Res<Presentation>,
    model: Res<WorkspaceModel>,
    mut probe: ResMut<WorkspaceProbe>,
    buttons: Query<(
        &Action,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
        Has<InteractionDisabled>,
    )>,
    recents: Query<(
        &RecentAction,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
    fields: Query<(
        &Input,
        &EditableText,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
    )>,
) {
    let rect = |node: &ComputedNode, at: &UiGlobalTransform| {
        let p = at.translation;
        let size = node.size();
        [p.x - size.x * 0.5, p.y - size.y * 0.5, size.x, size.y]
    };
    probe.page = state.page;
    probe.project = model.project.clone();
    probe.status = if state.error.is_empty() {
        model.status.clone()
    } else {
        state.error.clone()
    };
    probe.buttons.clear();
    probe.fields.clear();
    for (action, node, at, visible, disabled) in &buttons {
        if visible.get() && !node.is_empty() {
            probe.buttons.push(ProbeButton {
                action: format!("{action:?}"),
                rect: rect(node, at),
                enabled: !disabled,
            });
        }
    }
    for (action, node, at, visible) in &recents {
        if visible.get() && !node.is_empty() {
            probe.buttons.push(ProbeButton {
                action: format!(
                    "{}:{}",
                    if action.remove {
                        "RemoveRecent"
                    } else {
                        "OpenRecent"
                    },
                    action.path.display()
                ),
                rect: rect(node, at),
                enabled: !model.busy,
            });
        }
    }
    for (field, text, node, at, visible) in &fields {
        if visible.get() && !node.is_empty() {
            probe.fields.push(ProbeField {
                field: format!("{field:?}"),
                value: text.value().to_string(),
                rect: rect(node, at),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::input::mouse::{MouseScrollUnit, MouseWheel};

    fn scroll_app(page: Page) -> (App, Entity) {
        let mut app = App::new();
        app.insert_resource(Presentation { page, ..default() })
            .init_resource::<WorkbenchHostPageState>()
            .add_message::<MouseWheel>()
            .add_systems(
                Update,
                scroll
                    .after(crate::project_log::ProjectLogInput)
                    .after(crate::native_project_close::ProjectCloseInput),
            );
        let body = app.world_mut().spawn((
            PageNode(page),
            RelativeCursorPosition {
                cursor_over: true,
                normalized: Some(Vec2::splat(0.5)),
            },
            ComputedNode {
                size: Vec2::splat(100.),
                content_size: Vec2::new(100., 500.),
                inverse_scale_factor: 1.,
                ..default()
            },
            ScrollPosition(Vec2::new(0., 48.)),
        )).id();
        (app, body)
    }

    fn drop_app() -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_message::<FileDragAndDrop>()
            .add_message::<WorkspaceCommand>()
            .init_resource::<Presentation>()
            .init_resource::<WorkspaceModel>()
            .init_resource::<WorkbenchHostPageState>()
            .add_systems(Update, open_dropped_project);
        let mut window = Window::default();
        window.set_physical_cursor_position(Some(Vec2::new(320., 240.).as_dvec2()));
        let window = app.world_mut().spawn((PrimaryWindow, window)).id();
        let body = app.world_mut().spawn((
            PageNode(Page::Start),
            ComputedNode { size: Vec2::new(300., 200.), ..default() },
            UiGlobalTransform::from_xy(320., 240.),
            InheritedVisibility::VISIBLE,
        )).id();
        (app, window, body)
    }

    fn drop_frame(app: &mut App, targets: &[Entity]) -> Vec<WorkspaceCommand> {
        let world = app.world_mut();
        let mut commands = world.resource::<Messages<WorkspaceCommand>>().get_cursor_current();
        for &window in targets {
            world.write_message(FileDragAndDrop::DroppedFile {
                window,
                path_buf: "D:/验收/中文 工程".into(),
            });
        }
        world.run_schedule(Update);
        commands.read(world.resource::<Messages<WorkspaceCommand>>()).cloned().collect()
    }

    #[test]
    fn project_drop_requires_the_visible_start_body_of_the_primary_window() {
        let (mut app, window, body) = drop_app();
        assert!(matches!(
            drop_frame(&mut app, &[window]).as_slice(),
            [WorkspaceCommand::Open(path)] if path == std::path::Path::new("D:/验收/中文 工程")
        ));
        assert!(drop_frame(&mut app, &[Entity::PLACEHOLDER]).is_empty());

        app.world_mut().resource_mut::<Presentation>().page = Page::Open;
        assert!(drop_frame(&mut app, &[window]).is_empty());
        app.world_mut().resource_mut::<Presentation>().page = Page::Start;

        *app.world_mut().get_mut::<InheritedVisibility>(body).unwrap() = InheritedVisibility::HIDDEN;
        assert!(drop_frame(&mut app, &[window]).is_empty());
        *app.world_mut().get_mut::<InheritedVisibility>(body).unwrap() = InheritedVisibility::VISIBLE;

        app.world_mut().get_mut::<Window>(window).unwrap()
            .set_physical_cursor_position(Some(Vec2::new(320., 80.).as_dvec2()));
        assert!(drop_frame(&mut app, &[window]).is_empty());
    }

    #[test]
    fn project_drop_discards_multi_selection_and_blocked_input_without_replay() {
        let (mut app, window, _) = drop_app();
        assert!(drop_frame(&mut app, &[window, window, window]).is_empty());
        assert!(drop_frame(&mut app, &[]).is_empty());

        app.world_mut().resource_mut::<WorkspaceModel>().busy = true;
        assert!(drop_frame(&mut app, &[window]).is_empty());
        app.world_mut().resource_mut::<WorkspaceModel>().busy = false;
        assert!(drop_frame(&mut app, &[]).is_empty());

        app.insert_resource(AboutState { captured: true, ..default() });
        assert!(drop_frame(&mut app, &[window]).is_empty());
        app.world_mut().resource_mut::<AboutState>().captured = false;
        assert!(drop_frame(&mut app, &[]).is_empty());
        assert!(matches!(drop_frame(&mut app, &[window]).as_slice(), [WorkspaceCommand::Open(_)]));
    }

    fn scroll_frame(app: &mut App, amount: f32, unit: MouseScrollUnit) {
        app.world_mut().write_message(MouseWheel {
            x: 0., y: amount, unit, window: Entity::PLACEHOLDER,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        app.world_mut().run_schedule(Update);
    }

    #[test]
    fn unobstructed_workspace_pages_keep_line_pixel_and_hover_scroll() {
        for page in [Page::Start, Page::New, Page::Open] {
            let (mut app, body) = scroll_app(page);
            scroll_frame(&mut app, -1., MouseScrollUnit::Line);
            assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 72.);
            app.world_mut().get_mut::<ComputedNode>(body).unwrap().inverse_scale_factor = 0.5;
            scroll_frame(&mut app, -40., MouseScrollUnit::Pixel);
            assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 92.);
            app.world_mut().get_mut::<RelativeCursorPosition>(body).unwrap().cursor_over = false;
            scroll_frame(&mut app, -1., MouseScrollUnit::Line);
            assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 92.);
        }
    }

    #[test]
    fn workspace_modal_gates_keep_background_scroll_and_discard_only_its_old_wheels() {
        for gate in 0..9 {
            let (mut app, body) = scroll_app(Page::Start);
            match gate {
                0 => app.world_mut().resource_mut::<WorkbenchHostPageState>().settings_open = true,
                1 => app.world_mut().resource_mut::<WorkbenchHostPageState>().menu_open = true,
                2 => { app.insert_resource(feathrise_editor_map::workbench::WorkbenchModel {
                    modal: true, ..default()
                }); }
                3 => { app.insert_resource(feathrise_editor_core::EditorHostOverlay { active: true }); }
                4 => {
                    let mut close = crate::native_project_close::NativeProjectCloseState::default();
                    close.hold("滚轮隔离回归".into());
                    app.insert_resource(close);
                }
                5 | 6 => {
                    let mut plugins = crate::native_plugins::management::PluginManagerState::with_roots(None, "unused-global-scroll-fixture".into());
                    plugins.open = gate == 5;
                    plugins.captured = gate == 6;
                    app.insert_resource(plugins);
                }
                7 => { app.insert_resource(AboutState { captured: true, ..default() }); }
                _ => {
                    // 导出及其关闭释放帧沿用同一输入租约，不能回放开始页的旧滚轮。
                    let mut export = crate::project_export::ui::ExportOverlayState::default();
                    export.captured = true;
                    app.insert_resource(export);
                }
            }
            scroll_frame(&mut app, -1., MouseScrollUnit::Line);
            assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 48., "门 {gate}");
            assert!(!app.world().resource::<Messages<MouseWheel>>().is_empty(),
                "不能清空全局滚轮而令浮层自己的读取器失效");
            app.insert_resource(WorkbenchHostPageState::default());
            app.world_mut().remove_resource::<feathrise_editor_map::workbench::WorkbenchModel>();
            app.world_mut().remove_resource::<feathrise_editor_core::EditorHostOverlay>();
            app.world_mut().remove_resource::<crate::native_project_close::NativeProjectCloseState>();
            app.world_mut().remove_resource::<crate::native_plugins::management::PluginManagerState>();
            app.world_mut().remove_resource::<AboutState>();
            app.world_mut().remove_resource::<crate::project_export::ui::ExportOverlayState>();
            app.world_mut().run_schedule(Update);
            assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 48., "解除后不能重放旧滚轮");
            scroll_frame(&mut app, -1., MouseScrollUnit::Line);
            assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 72.);
        }
    }

    #[test]
    fn production_log_only_captures_scroll_inside_the_floating_panel() {
        use crate::project_log::{LogDecoder, LogSource, ProjectLogChannel, ProjectLogPlugin};
        use bevy::ui_widgets::{ControlOrientation, Scrollbar};
        use feathrise_editor_map::workbench::{WorkbenchFloatingPanel, WorkbenchHeader, WorkbenchProjectLogButton};

        let (mut app, body) = scroll_app(Page::Start);
        app.add_plugins((MinimalPlugins, AssetPlugin {
            file_path: concat!(env!("CARGO_MANIFEST_DIR"), "/assets").into(),
            watch_for_changes_override: Some(false), ..default()
        }, bevy::text::TextPlugin))
            .init_resource::<InputFocus>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>();
        let root = std::env::temp_dir().join("workspace-log-scroll-no-project-created");
        let project = crate::project::Project {
            id: "workspace-log-scroll".into(), name: "工作区日志滚轮回归".into(),
            manifest: root.join("Cargo.toml"), root: root.clone(), package: "unused".into(),
            context: crate::project::ProjectContext {
                root: root.clone(), id: "workspace-log-scroll".into(), name: "工作区日志滚轮回归".into(),
                format: 1, asset_roots: vec![root.join("assets")], editor_data: root.join(".editor"),
                storage_key: "workspace-log-scroll".into(), startup_scene: None,
            },
        };
        app.add_plugins(ProjectLogPlugin::new(&project));
        app.world_mut().spawn((WorkbenchHeader, Node::default()));
        let channel = app.world().resource::<ProjectLogChannel>().clone();
        let mut decoder = LogDecoder::new(Some(channel.begin()), LogSource::Stdout);
        decoder.write((0..80).map(|index| format!("日志 {index}\n")).collect::<String>().as_bytes());
        decoder.finish();
        for _ in 0..4 { app.world_mut().run_schedule(Update); }
        let world = app.world_mut();
        // Node的必需组件也会带ScrollPosition；按原纵向滚动条的target取得唯一日志阅读容器。
        let panes = world.query::<&Scrollbar>().iter(world)
            .filter(|bar| bar.orientation == ControlOrientation::Vertical)
            .map(|bar| bar.target).collect::<Vec<_>>();
        assert_eq!(panes.len(), 1);
        let pane = panes[0];
        // 此定向回归只提供正文几何；窗口、字体排版和实体键鼠仍由原实窗验收覆盖。
        world.entity_mut(pane).insert((
            RelativeCursorPosition { cursor_over: true, normalized: Some(Vec2::splat(0.5)) },
            ComputedNode {
                size: Vec2::splat(100.), content_size: Vec2::new(100., 2000.),
                inverse_scale_factor: 1., ..default()
            },
        ));
        let toggle = world.spawn((WorkbenchProjectLogButton,
            feathrise_editor_ui::EditorButtonClick::triggered())).id();
        world.run_schedule(Update);
        world.despawn(toggle);
        let panel = world.query_filtered::<Entity, With<WorkbenchFloatingPanel>>().single(world).unwrap();
        assert_eq!(world.get::<Node>(panel).unwrap().display, Display::Flex);
        world.entity_mut(panel).insert((ComputedNode { size: Vec2::splat(120.), ..default() }, InheritedVisibility::VISIBLE));

        let before = world.get::<ScrollPosition>(pane).unwrap().y;
        scroll_frame(&mut app, -1., MouseScrollUnit::Line);
        assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 72., "浮窗外可继续滚动工作区");
        assert_eq!(app.world().get::<ScrollPosition>(pane).unwrap().y, before);
        app.world_mut().get_mut::<RelativeCursorPosition>(panel).unwrap().cursor_over = true;
        scroll_frame(&mut app, 1., MouseScrollUnit::Line);
        assert!(app.world().get::<ScrollPosition>(pane).unwrap().y < before, "日志自己的生产滚轮仍应有效");
        assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 72.);
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
        scroll_frame(&mut app, -1., MouseScrollUnit::Line);
        assert_eq!(app.world().get::<Node>(panel).unwrap().display, Display::None);
        assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 72., "关闭当帧仍归已绘制的浮窗");
        app.world_mut().get_mut::<ComputedNode>(panel).unwrap().size = Vec2::ZERO;
        app.world_mut().resource_mut::<ButtonInput<KeyCode>>().clear();
        app.world_mut().run_schedule(Update);
        assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 72., "释放后不能重放日志旧滚轮");
        scroll_frame(&mut app, -1., MouseScrollUnit::Line);
        assert_eq!(app.world().get::<ScrollPosition>(body).unwrap().y, 96.);
    }

    #[test]
    fn closing_an_engineering_tab_does_not_replace_the_author_page_identity() {
        let mut state = Presentation::default();
        show(&mut state, Page::New);
        assert!(state.start_open && state.new_open);
        show(&mut state, Page::Editor);
        assert_eq!(state.page, Page::Editor);
        assert!(state.new_open, "返回原文档不销毁未提交工程表单");
    }

    #[test]
    fn changed_default_directory_preserves_a_user_edited_new_project_form() {
        let mut app = App::new();
        let mut state = Presentation::default();
        state.applied_default = Some("D:/old-default".into());
        state.parent = "D:/用户工程".into();
        app.insert_resource(state)
            .insert_resource(WorkspaceModel {
                default_directory: "E:/new-default".into(),
                ..default()
            })
            .init_resource::<WorkbenchHostPageState>()
            .add_systems(Update, adopt);
        let parent = app
            .world_mut()
            .spawn((Input::Parent, EditableText::new("D:/用户工程")))
            .id();
        let name = app
            .world_mut()
            .spawn((Input::Name, EditableText::new("我的未提交工程")))
            .id();
        let preference = app
            .world_mut()
            .spawn((Input::DefaultDirectory, EditableText::new("D:/old-default")))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .get::<EditableText>(parent)
                .unwrap()
                .value()
                .to_string(),
            "D:/用户工程"
        );
        assert_eq!(
            app.world()
                .get::<EditableText>(name)
                .unwrap()
                .value()
                .to_string(),
            "我的未提交工程"
        );
        assert_eq!(
            app.world()
                .get::<EditableText>(preference)
                .unwrap()
                .value()
                .to_string(),
            "E:/new-default"
        );
    }

    #[test]
    fn folder_picker_return_changes_only_its_explicit_field() {
        let mut app = App::new();
        app.init_resource::<Presentation>()
            .insert_resource(WorkspaceModel {
                default_directory: "D:/default".into(),
                ..default()
            })
            .init_resource::<WorkbenchHostPageState>()
            .add_systems(Update, adopt);
        let parent = app
            .world_mut()
            .spawn((Input::Parent, EditableText::new("D:/default")))
            .id();
        let name = app
            .world_mut()
            .spawn((Input::Name, EditableText::new("保留名字")))
            .id();
        app.update();
        super::super::fill_path(
            app.world_mut(),
            WorkspaceField::NewParent,
            "E:/选择的目录".into(),
        );
        app.update();
        assert_eq!(
            app.world()
                .get::<EditableText>(parent)
                .unwrap()
                .value()
                .to_string(),
            "E:/选择的目录"
        );
        assert_eq!(
            app.world()
                .get::<EditableText>(name)
                .unwrap()
                .value()
                .to_string(),
            "保留名字"
        );
    }
}
