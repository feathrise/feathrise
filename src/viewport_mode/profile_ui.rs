//! 启动配置下拉复用原配置请求；菜单只拥有展示和输入，不接管构建或游戏生命周期。

use super::{components::*, resources::BuildProfileMenuState};
use bevy::{input_focus::InputFocus, prelude::*, ui::{FocusPolicy, RelativeCursorPosition}, window::PrimaryWindow};
use feathrise_editor_core::{ProjectBuildProfile, ProjectLaunchSettings};
use feathrise_editor_ui::{EditorButtonClick, colors as c};

pub(super) fn mount(header: &mut ChildSpawnerCommands, font: &Handle<Font>) {
    // 配置名按文字自然占宽，Debug/Release 都只保留两侧必要留白；菜单继续独立右对齐。
    header.spawn((crate::window_chrome::ChromeSlot::Profile, Node { height: px(26), flex_shrink: 0., ..default() }))
        .with_children(|selector| {
            selector.spawn((
                Button, ProfileToggle, Name::new("启动配置"),
                feathrise_editor_ui::ButtonColors::new(c::BUTTON_DEV_SMALL_NORMAL, c::BUTTON_DEV_SMALL_HOVERED, c::BUTTON_DEV_SMALL_PRESSED),
                Node {
                    height: px(26), padding: UiRect::horizontal(px(7)), flex_shrink: 0.,
                    align_items: AlignItems::Center, justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default()
                },
                BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL), BorderColor::all(c::COPPER_BORDER_WEAK),
            )).with_child((
                ProfileLabel(None), Text::new("Debug ▼"),
                TextFont { font: font.clone().into(), font_size: 12.0.into(), ..default() },
                TextColor(c::TEXT_PRIMARY), TextLayout::no_wrap(), Pickable::IGNORE,
            ));
            selector.spawn((
                ProfileMenu, GlobalZIndex(4201), FocusPolicy::Block,
                Interaction::None, RelativeCursorPosition::default(),
                Node {
                    display: Display::None, position_type: PositionType::Absolute,
                    right: px(0), top: px(28), width: px(112),
                    flex_direction: FlexDirection::Column, padding: UiRect::all(px(4)),
                    row_gap: px(2), border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(3)), ..default()
                },
                BackgroundColor(c::PANEL_BG_STRONG), BorderColor::all(c::COPPER_BORDER_WEAK),
            )).with_children(|menu| {
                for profile in [ProjectBuildProfile::Debug, ProjectBuildProfile::Release] {
                    menu.spawn((
                        Button, ProfileOption(profile),
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
            });
        });
    // 遮罩按整个作者窗口拦截外部点击；工程原生子窗由同一宿主浮层门临时让出。
    header.spawn((
        ProfileBackdrop, GlobalZIndex(4200), FocusPolicy::Block, Interaction::None,
        Node {
            display: Display::None, position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: Val::Vw(100.), height: Val::Vh(100.), ..default()
        },
    ));
}

pub(super) fn input(
    toggles: Query<&EditorButtonClick, With<ProfileToggle>>,
    options: Query<(&ProfileOption, &EditorButtonClick), Changed<EditorButtonClick>>,
    panels: Query<&RelativeCursorPosition, Or<(With<ProfileMenu>, With<ProfileToggle>)>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    blockers: (
        Option<Res<crate::native_project_close::NativeProjectCloseState>>,
        Option<Res<crate::native_project_menu::NativeProjectMenuState>>,
        Option<Res<crate::project_log::ProjectLogState>>,
        Option<Res<crate::project_export::ui::ExportOverlayState>>,
        Option<Res<crate::native_plugins::management::PluginManagerState>>,
    ),
    mut focus: ResMut<InputFocus>,
    mut state: ResMut<BuildProfileMenuState>,
    mut requests: MessageWriter<super::messages::RequestBuildProfile>,
) {
    let was_open = state.open;
    let blocked = windows.iter().any(|window| !window.focused)
        || blockers.0.is_some_and(|close| close.captures_input())
        || blockers.1.is_some_and(|menu| menu.is_open())
        || blockers.2.is_some_and(|logs| logs.captures_input())
        || blockers.3.is_some_and(|export| export.captures_input())
        || blockers.4.is_some_and(|plugins| plugins.captures_input());
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
            }
        }
    }
    state.captured = was_open || state.open;
}

pub(super) fn present(
    mut commands: Commands,
    state: Res<BuildProfileMenuState>,
    launch: Res<ProjectLaunchSettings>,
    mut panels: Query<&mut Node, Or<(With<ProfileMenu>, With<ProfileBackdrop>)>>,
    toggles: Query<(Entity, Has<GlobalZIndex>), With<ProfileToggle>>,
    mut labels: Query<(&ProfileLabel, &mut Text)>,
    mut options: Query<(&ProfileOption, &mut BorderColor)>,
) {
    if !state.is_changed() && !launch.is_changed() { return; }
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
            None => format!("{} ▼", launch.profile.label()),
            Some(profile) => format!("{} {}", if profile == launch.profile { "✓" } else { "  " }, profile.label()),
        };
        if text.as_str() != caption { **text = caption; }
    }
    for (option, mut border) in &mut options {
        *border = BorderColor::all(if option.0 == launch.profile { c::COPPER_BORDER } else { c::COPPER_BORDER_WEAK });
    }
}
