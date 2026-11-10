//! 按宿主实际目标呈现工程关闭或编辑器退出确认；原作者控件始终保留在后方。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, resources::*};
use bevy::{
    input::mouse::{MouseScrollUnit, MouseWheel},
    input_focus::InputFocus,
    prelude::*,
    ui::{FocusPolicy, InteractionDisabled, RelativeCursorPosition},
};
use feathrise_editor_scene::workbench::WorkbenchHeader;
use feathrise_editor_ui::colors as c;

pub(super) fn mount(
    mut commands: Commands,
    headers: Query<Entity, Added<WorkbenchHeader>>,
    assets: Res<AssetServer>,
) {
    let font = feathrise_editor_core::editor_font(&assets, feathrise_editor_core::FONT_PATH);
    for header in &headers {
        commands.entity(header).with_children(|header| {
            header
                .spawn((
                    CloseOverlay,
                    GlobalZIndex(6000),
                    FocusPolicy::Block,
                    UiInteraction::None,
                    Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        width: Val::Vw(100.),
                        height: Val::Vh(100.),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        ..default()
                    },
                    BackgroundColor(c::DIALOG_BACKDROP),
                ))
                .with_children(|overlay| {
                    overlay
                        .spawn((
                            ClosePanel,
                            GlobalZIndex(6001),
                            FocusPolicy::Block,
                            Node {
                                width: px(440),
                                max_width: percent(94),
                                max_height: Val::Vh(92.),
                                // 日志、详情、设置与关闭确认使用同一边界和三段式外壳。
                                ..feathrise_editor_ui::dialog_panel_node()
                            },
                            BackgroundColor(c::DIALOG_BG), BorderColor::all(c::DIALOG_BORDER),
                        ))
                        .with_children(|panel| {
                            panel.spawn((CloseHeader, feathrise_editor_ui::dialog_header_node(),
                                BackgroundColor(c::DIALOG_HEADER_BG), BorderColor::all(c::DIALOG_BORDER),
                            )).with_children(|header| {
                                header.spawn(Node { flex_grow: 1., min_width: px(0), ..default() })
                                    .with_child((CloseTitle, caption(&font, CloseTarget::default().title(), feathrise_editor_ui::metrics::FONT_BODY)));
                                feathrise_editor_ui::spawn_custom_text_button(header, CloseHeaderClose,
                                    font.clone(), "×", feathrise_editor_ui::dialog_close_button_style());
                            });
                            panel.spawn((CloseBody, feathrise_editor_ui::dialog_body_node(),
                                BackgroundColor(c::DIALOG_BG),
                            )).with_children(|body| {
                                body.spawn((CloseStatus, caption(&font, "", 13.)));
                                body.spawn((
                                    CloseScroll, RelativeCursorPosition::default(), ScrollPosition::default(),
                                    Node { display: Display::None, width: percent(100), min_height: px(0),
                                        max_height: Val::Vh(28.), overflow: Overflow::scroll_y(), ..default() },
                                )).with_child((CloseDocuments, caption(&font, "", 13.)));
                            });
                            panel
                                .spawn((CloseFooter, CloseActions, feathrise_editor_ui::dialog_footer_node(),
                                    BackgroundColor(c::DIALOG_HEADER_BG), BorderColor::all(c::DIALOG_BORDER)))
                                .with_children(|buttons| {
                                    button(buttons, &font, CloseAction::Close);
                                    button(buttons, &font, CloseAction::Save);
                                    button(buttons, &font, CloseAction::Discard);
                                    button(buttons, &font, CloseAction::Cancel);
                                });
                        });
                });
        });
    }
}

pub(super) fn handle(
    mut state: ResMut<NativeProjectCloseState>,
    buttons: Query<(
        Entity,
        &CloseAction,
        &feathrise_editor_ui::EditorButtonClick,
        Has<InteractionDisabled>,
        &Node,
    )>,
    header_close: Query<&feathrise_editor_ui::EditorButtonClick, With<CloseHeaderClose>>,
    mut focus: ResMut<InputFocus>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut was_open: Local<bool>,
    mut wheels: MessageReader<MouseWheel>,
    mut scrolls: Query<(&RelativeCursorPosition, &mut ScrollPosition), With<CloseScroll>>,
) {
    let open = state.active();
    // 取消确认的当前帧与下一输入阶段仍由浮层拥有，避免松开/Enter穿透成工具命令。
    let captured = open || *was_open;
    if state.input_active != captured {
        state.input_active = captured;
    }
    *was_open = open;
    if !open {
        wheels.clear();
        return;
    }
    if state.focus_cancel && state.stage != CloseStage::Blocked {
        if let Some((entity, ..)) = buttons
            .iter()
            .find(|(_, action, _, _, _)| **action == CloseAction::Cancel)
        {
            focus.set(entity, bevy::input_focus::FocusCause::Navigated);
            state.focus_cancel = false;
        }
    }
    let choice = if keys.just_pressed(KeyCode::Escape)
        || keys.just_pressed(KeyCode::Enter)
        || keys.just_pressed(KeyCode::NumpadEnter)
        || header_close.iter().any(feathrise_editor_ui::EditorButtonClick::is_clicked)
    {
        // 确认层没有“默认破坏”快捷键；回车和Esc都安全返回作者现场。
        Some(CloseAction::Cancel)
    } else {
        buttons.iter().find_map(|(_, action, click, disabled, node)| {
            (!disabled && node.display != Display::None && state.action_enabled(*action)
                && click.is_clicked()).then_some(*action)
        })
    };
    if choice.is_some() {
        state.choice = choice;
        // 确认框已消费的取消键不能再关闭底层设置，Enter也不能在取消后提交新建/打开表单。
        for key in [KeyCode::Escape, KeyCode::Enter, KeyCode::NumpadEnter] {
            keys.clear_just_pressed(key);
        }
    }
    for wheel in wheels.read() {
        for (cursor, mut scroll) in &mut scrolls {
            if cursor.cursor_over() {
                let delta = wheel.y
                    * if wheel.unit == MouseScrollUnit::Line {
                        24.
                    } else {
                        1.
                    };
                scroll.y = (scroll.y - delta).max(0.);
            }
        }
    }
}

pub(super) fn present(
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    mut commands: Commands,
    state: Res<NativeProjectCloseState>,
    mut overlays: Query<&mut Node, With<CloseOverlay>>,
    mut panels: Query<&mut Node, (With<ClosePanel>, Without<CloseOverlay>)>,
    mut statuses: Query<&mut Text, (With<CloseStatus>, Without<CloseDocuments>)>,
    mut documents: Query<&mut Text, (With<CloseDocuments>, Without<CloseStatus>)>,
    mut labels: Query<(&mut Text, Option<&CloseActionLabel>, &ChildOf),
        (Or<(With<CloseTitle>, With<CloseActionLabel>)>, Without<CloseStatus>, Without<CloseDocuments>)>,
    mut scrolls: Query<&mut Node, (With<CloseScroll>, Without<CloseOverlay>, Without<ClosePanel>)>,
    mut buttons: Query<(Entity, &CloseAction, &mut Node, Has<InteractionDisabled>, &mut feathrise_editor_ui::EditorButtonClick),
        (Without<CloseOverlay>, Without<ClosePanel>, Without<CloseScroll>)>,
) {
    if !state.is_changed() {
        return;
    }
    for mut node in &mut overlays {
        node.display = if state.active() {
            Display::Flex
        } else {
            Display::None
        };
    }
    if !state.active() {
        return;
    }
    for mut node in &mut panels {
        let width = px(if state.needs_save_decision() { 440. } else { 360. });
        if node.width != width {
            node.width = width;
        }
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (mut text, action, parent) in &mut labels {
        let label = locale.editor_label(action.map_or_else(|| state.target().title(), |action| state.target().action_label(action.0)));
        if text.as_str() != label {
            // 同一按钮改为另一退出目标时作废原按下代次，旧按钮松开不能确认新的含义。
            if action.is_some()
                && let Ok((.., mut click)) = buttons.get_mut(parent.parent())
            {
                click.invalidate();
            }
            text.0 = label.into();
        }
    }
    let status = locale.editor_notice(&state.status);
    for mut text in &mut statuses {
        if text.as_str() != status.as_ref() {
            text.0 = status.as_ref().to_owned();
        }
    }
    let rows = state
        .documents
        .iter()
        .map(|name| format!("• {name}"))
        .collect::<Vec<_>>()
        .join("\n");
    for mut text in &mut documents {
        if text.0 != rows {
            text.0.clone_from(&rows);
        }
    }
    // 干净工程只提供直接关闭和取消，保存/放弃仅在真实修改或未解决失败时出现。
    for mut node in &mut scrolls {
        node.display = if state.documents.is_empty() { Display::None } else { Display::Flex };
    }
    for (entity, action, mut node, disabled, _) in &mut buttons {
        let display = if state.action_visible(*action) { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let next = !state.action_enabled(*action);
        if next != disabled {
            if next {
                commands.entity(entity).insert(InteractionDisabled);
            } else {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        }
    }
}

fn caption(font: &Handle<Font>, label: &str, size: f32) -> impl Bundle {
    (
        Text::new(label),
        TextFont {
            font: font.clone().into(),
            font_size: size.into(),
            ..default()
        },
        TextColor(c::TEXT_PRIMARY),
        Pickable::IGNORE,
        Node {
            width: percent(100),
            flex_shrink: 0.,
            ..default()
        },
    )
}

fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    action: CloseAction,
) {
    // 复用共享按钮实体树和配色，避免此弹窗继续维护一套全宽按钮及独立描边。
    feathrise_editor_ui::spawn_custom_text_button_with_text_marker(
        parent, action, CloseActionLabel(action), font.clone(), CloseTarget::default().action_label(action),
        feathrise_editor_ui::confirmation_button_style(matches!(action, CloseAction::Close | CloseAction::Save)),
    );
}
