//! 模式按钮挂在现有顶栏，在独立作者入口与同窗口工作台中使用相同交互。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{RequestViewportMode, ViewportMode, ViewportModeState, components::*};
use bevy::prelude::*;
use feathrise_editor_core::{ProjectLaunchButton, ProjectLaunchLabel, ProjectLaunchSettings};
use feathrise_editor_scene::workbench::WorkbenchHeader;
use feathrise_editor_ui::{EditorButtonClick, colors as c};

pub(super) fn mount(
    mut commands: Commands,
    headers: Query<(Entity, &Children), Added<WorkbenchHeader>>,
    launch_buttons: Query<(), With<ProjectLaunchButton>>,
    assets: Res<AssetServer>,
    diagnostics: Option<Res<crate::environment_diagnostics::BuildEnvironmentState>>,
) {
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for (header, children) in &headers {
        let launch_index = children.iter().position(|child| launch_buttons.contains(child));
        let modes = commands
                .spawn((crate::window_chrome::ChromeSlot::Modes, Node {
                    height: px(26),
                    flex_shrink: 0.,
                    column_gap: px(3),
                    margin: UiRect::left(px(6)),
                    ..default()
                }))
                .with_children(|row| {
                    for &mode in ViewportMode::available() {
                        row.spawn((
                            InteractionButton,
                            ModeButton(mode),
                            feathrise_editor_ui::ButtonColors::new(
                                c::BUTTON_DEV_SMALL_NORMAL,
                                c::BUTTON_DEV_SMALL_HOVERED,
                                c::BUTTON_DEV_SMALL_PRESSED,
                            ),
                            Node {
                                height: px(26),
                                flex_shrink: 0.,
                                padding: UiRect::horizontal(px(7)),
                                align_items: AlignItems::Center,
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(3)),
                                ..default()
                            },
                            BackgroundColor(c::BUTTON_DEV_SMALL_NORMAL),
                            BorderColor::all(c::COPPER_BORDER_WEAK),
                        ))
                        .with_children(|button| {
                            button.spawn((
                                ModeLabel,
                                feathrise_editor_ui::localization::editor_label(mode.label()),
                                TextFont {
                                    font: font.clone().into(),
                                    font_size: 12.0.into(),
                                    ..default()
                                },
                                TextColor(c::TEXT_PRIMARY),
                                Pickable::IGNORE,
                            ));
                            if mode == ViewportMode::EmbeddedWorld { super::busy_border::mount(button); }
                        });
                    }
                }).id();
        // 两种3D来源放在原启动入口之前，构建选择紧跟启动／预览右侧，避免拆散相关入口。
        if let Some(index) = launch_index {
            commands.entity(header).insert_children(index, &[modes]);
        } else {
            commands.entity(header).add_child(modes);
        }
        commands.entity(header).with_children(|header| {
            super::profile_ui::mount(header, &font, diagnostics.as_ref().is_some_and(|state| state.enabled));
            header.spawn((
                ModeStatus,
                feathrise_editor_ui::PanelStatusText("3D视口"),
                feathrise_editor_ui::feedback::GlobalPanelStatus,
                feathrise_editor_ui::feedback::PanelStatusState::default(),
                Text::new(""),
                Node {
                    display: Display::None,
                    ..default()
                },
            ));
        });
    }
}

/// 挂在原启动按钮下面，不插入同排控件；标记确保标题栏重排后也不重复挂载。
pub(super) fn mount_launch(
    mut commands: Commands,
    buttons: Query<Entity, (With<ProjectLaunchButton>, Without<LaunchBorderMounted>)>,
) {
    for button in &buttons {
        commands.entity(button).insert(LaunchBorderMounted).with_children(super::busy_border::mount_launch);
    }
}

pub(super) fn present_launch(
    state: Res<ViewportModeState>,
    launch: Option<Res<crate::native_play::NativePlayStatus>>,
    mut borders: Query<&mut Node, With<LaunchStatus>>,
) {
    let busy = state.selected == ViewportMode::SavedWorld
        && launch.is_some_and(|launch| launch.active && launch.preparing);
    let display = if busy { Display::Flex } else { Display::None };
    for mut node in &mut borders { if node.display != display { node.display = display; } }
}

#[cfg(test)]
#[test]
fn launch_caption_follows_both_selected_3d_sources() {
    use feathrise_editor_scene::standalone::SavedPlayFeedback;
    let mut app = App::new();
    app.init_resource::<ViewportModeState>()
        .init_resource::<super::ViewportModeAvailability>()
        .init_resource::<ProjectLaunchSettings>()
        .add_message::<RequestViewportMode>()
        .add_message::<super::ViewportModeChanged>()
        .add_message::<SavedPlayFeedback>()
        .add_systems(Update, (super::systems::apply, present).chain());
    let label = app.world_mut().spawn((ProjectLaunchLabel, Text::new("启动"))).id();
    for &mode in ViewportMode::available().iter().rev() {
        let expected = if mode == ViewportMode::EmbeddedWorld { "预览" } else { "启动" };
        app.world_mut().write_message(RequestViewportMode(mode));
        app.update();
        assert_eq!(app.world().get::<Text>(label).unwrap().as_str(), expected);
    }
}

pub(super) fn input(
    buttons: Query<(&ModeButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut requests: MessageWriter<RequestViewportMode>,
    plugins: Option<Res<crate::native_plugins::management::PluginManagerState>>,
) {
    if plugins.is_some_and(|plugins| plugins.captures_input()) { return; }
    for (button, click) in &buttons {
        if click.is_clicked() {
            requests.write(RequestViewportMode(button.0));
        }
    }
}

pub(super) fn present(
    state: Res<ViewportModeState>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    launch: Res<ProjectLaunchSettings>,
    interruption: Option<Res<feathrise_editor_core::ProjectLaunchInterruption>>,
    mut statuses: Query<&mut Text, (With<ModeStatus>, Without<ProjectLaunchLabel>)>,
    mut launch_labels: Query<&mut Text, (With<ProjectLaunchLabel>, Without<ModeStatus>, Without<ModeLabel>)>,
    mut buttons: Query<(&ModeButton, &UiInteraction, &mut BorderColor, &mut BackgroundColor, &mut feathrise_editor_ui::ButtonColors)>,
) {
    let locale = locale.as_deref().copied().unwrap_or_default();
    let status = locale.editor_notice(&state.status);
    for mut text in &mut statuses {
        if text.as_str() != status.as_ref() {
            **text = status.as_ref().to_owned();
        }
    }
    for mut text in &mut launch_labels {
        let caption = interruption.as_ref().and_then(|phase|phase.action_label()).unwrap_or_else(||launch.action_label());
        let caption = locale.editor_label(caption);
        if text.as_str() != caption { **text = caption.into(); }
    }
    // 文字始终使用挂载时的固定名称；选中态只变底色与边框，不插入会使按钮变宽的圆点。
    for (button, interaction, mut border, mut background, mut colors) in &mut buttons {
        let palette = if button.0 == state.selected {
            feathrise_editor_ui::ButtonColors::new(c::SELECT_BG, c::SELECT_BG_HOVER, c::SELECT_BG_PRESSED)
        } else {
            feathrise_editor_ui::ButtonColors::new(c::BUTTON_DEV_SMALL_NORMAL, c::BUTTON_DEV_SMALL_HOVERED, c::BUTTON_DEV_SMALL_PRESSED)
        };
        if *colors != palette { *colors = palette; }
        let wanted = match interaction {
            UiInteraction::Pressed => palette.pressed,
            UiInteraction::Hovered => palette.hovered,
            UiInteraction::None => palette.normal,
        };
        if background.0 != wanted { background.0 = wanted; }
        *border = BorderColor::all(if button.0 == state.selected {
            c::COPPER_BORDER
        } else {
            c::COPPER_BORDER_WEAK
        });
    }
}

/// 真实进程轮询后投影到固定UI源；不发送反馈消息，不覆盖其他工具的当前说明。
pub(super) fn present_connection(world: &mut World) {
    use crate::embedded_viewport::ConnectionState;
    use feathrise_editor_ui::feedback::{PanelStatusPhase, PanelStatusState};
    let selected = world.resource::<ViewportModeState>().selected == ViewportMode::EmbeddedWorld;
    let (summary, phase) = match crate::embedded_viewport::connection_state(world) {
        ConnectionState::Disconnected => ("工程3D：未连接", PanelStatusPhase::Info),
        ConnectionState::Starting => ("工程3D：启动中", PanelStatusPhase::Working),
        ConnectionState::Synchronizing => ("工程3D：同步中", PanelStatusPhase::Working),
        ConnectionState::Connected => ("工程3D：已连接", PanelStatusPhase::Info),
        ConnectionState::Stopping => ("工程3D：断开中", PanelStatusPhase::Working),
        ConnectionState::Restarting => ("工程3D：重启中", PanelStatusPhase::Working),
        ConnectionState::Failed => ("工程3D：连接失败", PanelStatusPhase::Failed),
    };
    let busy = selected && phase == PanelStatusPhase::Working;
    for mut node in world.query_filtered::<&mut Node, With<ConnectionStatus>>().iter_mut(world) {
        let display = if busy { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
    }
    let projected = if selected { PanelStatusState { phase, summary } } else { PanelStatusState::default() };
    for mut status in world.query_filtered::<&mut PanelStatusState, With<ModeStatus>>().iter_mut(world) {
        if *status != projected { *status = projected; }
    }
}
