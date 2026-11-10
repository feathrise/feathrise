//! 启动配置下拉复用原配置请求；菜单只拥有展示和输入，不接管构建或游戏生命周期。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::BuildProfileMenuState};
use crate::environment_diagnostics::{BuildEnvironmentState, RequestBuildEnvironment};
use crate::native_play::ProjectBuildBackend;
use bevy::{input_focus::InputFocus, prelude::*, ui::{FocusPolicy, RelativeCursorPosition}, window::PrimaryWindow};
use feathrise_editor_core::{ProjectBuildProfile, ProjectLaunchSettings};
use feathrise_editor_ui::{EditorButtonClick, colors as c};

pub(super) fn mount(header: &mut ChildSpawnerCommands, font: &Handle<Font>, diagnostics: bool) {
    // 配置名按文字自然占宽，Debug/Release 都只保留两侧必要留白；菜单继续独立右对齐。
    header.spawn((crate::window_chrome::ChromeSlot::Profile, Node { height: px(26), flex_shrink: 0., ..default() }))
        .with_children(|selector| {
            selector.spawn((
                InteractionButton, ProfileToggle, Name::new("启动配置"),
                feathrise_editor_ui::ButtonColors::new(c::BUTTON_DEV_SMALL_NORMAL, c::BUTTON_DEV_SMALL_HOVERED, c::BUTTON_DEV_SMALL_PRESSED),
                Node {
                    height: px(26), padding: UiRect::horizontal(px(7)), flex_shrink: 0.,
                    align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default()
                },
                BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(c::COPPER_BORDER_WEAK),
            )).with_child((
                ProfileLabel(None), Text::new("Debug · DX ▼"),
                TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
                TextColor(c::TEXT_PRIMARY), TextLayout::no_wrap(), Pickable::IGNORE,
            ));
            selector.spawn((
                ProfileMenu, GlobalZIndex(4201), FocusPolicy::Block,
                UiInteraction::None, RelativeCursorPosition::default(),
                Node {
                    display: Display::None, position_type: PositionType::Absolute,
                    right: px(0), top: px(28), width: px(if diagnostics { 192 } else { 148 }),
                    flex_direction: FlexDirection::Column, padding: UiRect::all(px(4)),
                    row_gap: px(2), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default()
                },
                BackgroundColor(c::PANEL_BG_STRONG), BorderColor::all(c::COPPER_BORDER_WEAK),
            )).with_children(|menu| {
                for profile in [ProjectBuildProfile::Debug, ProjectBuildProfile::Release] {
                    menu.spawn((
                        InteractionButton, ProfileOption(profile),
                        feathrise_editor_ui::ButtonColors::new(c::BUTTON_DEV_SMALL_NORMAL, c::BUTTON_DEV_SMALL_HOVERED, c::BUTTON_DEV_SMALL_PRESSED),
                        Node {
                            width: percent(100), height: px(26), padding: UiRect::horizontal(px(5)),
                            align_items: AlignItems::Center, border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(2)), ..default()
                        },
                        BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(c::COPPER_BORDER_WEAK),
                    )).with_child((
                        ProfileLabel(Some(profile)), Text::new(profile.label()),
                        TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
                        TextColor(c::TEXT_PRIMARY), TextLayout::no_wrap(), Pickable::IGNORE,
                    ));
                }
                // 两种3D方式都可选择启动工具，共用配置菜单的遮罩和释放帧输入门。
                menu.spawn((BackendOptions, Node { display: Display::Flex, width: percent(100),
                    flex_direction: FlexDirection::Column, row_gap: px(2), ..default() }))
                    .with_children(|options| {
                        options.spawn((Node { height: px(1), margin: UiRect::vertical(px(3)), ..default() },
                            BackgroundColor(c::COPPER_BORDER_WEAK), Pickable::IGNORE));
                        for backend in [ProjectBuildBackend::Dx, ProjectBuildBackend::Native] {
                            options.spawn((InteractionButton, BackendOption(backend), Name::new(backend.label()),
                                feathrise_editor_ui::ButtonColors::new(c::BUTTON_DEV_SMALL_NORMAL, c::BUTTON_DEV_SMALL_HOVERED, c::BUTTON_DEV_SMALL_PRESSED),
                                Node { width: percent(100), height: px(26), padding: UiRect::horizontal(px(5)),
                                    align_items: AlignItems::Center, border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(2)), ..default() },
                                BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(c::COPPER_BORDER_WEAK),
                            )).with_child((BackendLabel(backend), Text::new(backend.label()),
                                TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
                                TextColor(c::TEXT_PRIMARY), TextLayout::no_wrap(), Pickable::IGNORE));
                        }
                    });
                if diagnostics {
                    menu.spawn((Node { height: px(1), margin: UiRect::vertical(px(3)), ..default() },
                        BackgroundColor(c::COPPER_BORDER_WEAK), Pickable::IGNORE));
                    // 主动预检沿用原启动菜单的输入门，结果写入工程日志浮层。
                    menu.spawn((InteractionButton, BuildEnvironmentButton, Name::new("检查构建环境"),
                        feathrise_editor_ui::ButtonColors::new(c::BUTTON_DEV_SMALL_NORMAL, c::BUTTON_DEV_SMALL_HOVERED, c::BUTTON_DEV_SMALL_PRESSED),
                        Node { width: percent(100), height: px(26), padding: UiRect::horizontal(px(5)),
                            align_items: AlignItems::Center, border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(2)), ..default() },
                        BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(c::COPPER_BORDER_WEAK),
                    )).with_child((BuildEnvironmentLabel, Text::new("检查构建环境"),
                        TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
                        TextColor(c::TEXT_PRIMARY), TextLayout::no_wrap(), Pickable::IGNORE));
                }
            });
        });
    // 遮罩按整个作者窗口拦截外部点击；工程原生子窗由同一宿主浮层门临时让出。
    header.spawn((
        ProfileBackdrop, GlobalZIndex(4200), FocusPolicy::Block, UiInteraction::None,
        Node {
            display: Display::None, position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: Val::Vw(100.), height: Val::Vh(100.), ..default()
        },
    ));
}

pub(super) fn input(
    toggles: Query<&EditorButtonClick, With<ProfileToggle>>,
    options: Query<(&ProfileOption, &EditorButtonClick), Changed<EditorButtonClick>>,
    backends: Query<(&BackendOption, &EditorButtonClick), Changed<EditorButtonClick>>,
    diagnostics: Query<&EditorButtonClick, (With<BuildEnvironmentButton>, Changed<EditorButtonClick>)>,
    panels: Query<&RelativeCursorPosition, Or<(With<ProfileMenu>, With<ProfileToggle>)>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    blockers: (
        Option<Res<crate::native_project_close::NativeProjectCloseState>>,
        Option<Res<crate::native_project_menu::NativeProjectMenuState>>,
        Option<Res<crate::project_export::ui::ExportOverlayState>>,
        Option<Res<crate::native_plugins::management::PluginManagerState>>,
    ),
    mut focus: ResMut<InputFocus>,
    mut state: ResMut<BuildProfileMenuState>,
    mut requests: MessageWriter<super::messages::RequestBuildProfile>,
    mut backend_requests: MessageWriter<super::messages::RequestBuildBackend>,
    mut diagnostic_requests: MessageWriter<RequestBuildEnvironment>,
) {
    let was_open = state.open;
    let blocked = windows.iter().any(|window| !window.focused)
        || blockers.0.is_some_and(|close| close.captures_input())
        || blockers.1.is_some_and(|menu| menu.is_open())
        || blockers.2.is_some_and(|export| export.captures_input())
        || blockers.3.is_some_and(|plugins| plugins.captures_input());
    if blocked {
        state.open = false;
    } else {
        if toggles.iter().any(EditorButtonClick::is_clicked) {
            state.open = !state.open;
            if state.open { focus.clear(); }
        }
        if was_open {
            if keys.just_pressed(KeyCode::Escape)
                || mouse.get_just_pressed().next().is_some()
                    && !panels.iter().any(RelativeCursorPosition::cursor_over)
            {
                state.open = false;
            } else if let Some(profile) = options.iter().find_map(|(option, click)| click.is_clicked().then_some(option.0)) {
                // 只发布原请求；业务门拒绝时仍回显实际采用的配置，不假装已经切换成功。
                requests.write(super::messages::RequestBuildProfile(profile));
                state.open = false;
            } else if let Some(backend) = backends.iter().find_map(|(option, click)| click.is_clicked().then_some(option.0)) {
                backend_requests.write(super::messages::RequestBuildBackend(backend));
                state.open = false;
            } else if diagnostics.iter().any(EditorButtonClick::is_clicked) {
                diagnostic_requests.write(RequestBuildEnvironment);
                state.open = false;
            }
        }
    }
    state.captured = was_open || state.open;
}

pub(super) fn present(
    mut commands: Commands,
    state: Res<BuildProfileMenuState>,
    launch: Res<ProjectLaunchSettings>,
    backend: Res<ProjectBuildBackend>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    mut panels: Query<&mut Node, Or<(With<ProfileMenu>, With<ProfileBackdrop>)>>,
    toggles: Query<(Entity, Has<GlobalZIndex>), With<ProfileToggle>>,
    mut labels: Query<(&ProfileLabel, &mut Text)>,
    mut options: Query<(&ProfileOption, &mut BorderColor)>,
    mut backend_labels: Query<(&BackendLabel, &mut Text), Without<ProfileLabel>>,
    mut backend_options: Query<(&BackendOption, &mut BorderColor), Without<ProfileOption>>,
    diagnostics: Option<Res<BuildEnvironmentState>>,
    mut diagnostic_labels: Query<&mut Text, (With<BuildEnvironmentLabel>, Without<ProfileLabel>, Without<BackendLabel>)>,
) {
    if !state.is_changed() && !launch.is_changed() && !backend.is_changed()
        && !locale.as_ref().is_some_and(|locale| locale.is_changed())
        && !diagnostics.as_ref().is_some_and(|diagnostics| diagnostics.is_changed()) { return; }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for mut node in &mut panels {
        node.display = if state.open { Display::Flex } else { Display::None };
    }
    // 收起时恢复普通子节点堆叠；显式全局0会把按钮压到作者工作台背景之后。
    for (entity, elevated) in &toggles {
        if state.open && !elevated { commands.entity(entity).insert(GlobalZIndex(4202)); }
        else if !state.open && elevated { commands.entity(entity).remove::<GlobalZIndex>(); }
    }
    for (label, mut text) in &mut labels {
        let caption = match label.0 {
            None => format!("{} · {} ▼", launch.profile.label(), locale.editor_label(backend.label())),
            Some(profile) => format!("{} {}", if profile == launch.profile { "✓" } else { "  " }, profile.label()),
        };
        if text.as_str() != caption { **text = caption; }
    }
    for (option, mut border) in &mut options {
        *border = BorderColor::all(if option.0 == launch.profile { c::COPPER_BORDER } else { c::COPPER_BORDER_WEAK });
    }
    for (label, mut text) in &mut backend_labels {
        let caption = format!("{} {}", if label.0 == *backend { "✓" } else { "  " }, locale.editor_label(label.0.label()));
        if text.as_str() != caption { **text = caption; }
    }
    for (option, mut border) in &mut backend_options {
        *border = BorderColor::all(if option.0 == *backend { c::COPPER_BORDER } else { c::COPPER_BORDER_WEAK });
    }
    let key = if diagnostics.as_ref().is_some_and(|diagnostics| diagnostics.checking()) {
        "build_environment.checking"
    } else {
        "build_environment.action"
    };
    for mut label in &mut diagnostic_labels {
        if label.as_str() != locale.text(key) { **label = locale.text(key).into(); }
    }
}
