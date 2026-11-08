//! 同一套墨底铜边固定菜单和中文名称浮层，按F12根的实际倍率钳制位置。

use feathrise_editor_ui::InteractionButton;
use super::{
    components::*, resources::SceneContextMenuState, systems::MenuContext, types::SceneMenuAction,
};
use crate::{components::MapEditorRoot, resources::MapEditorState, ui::theme as colors};
use bevy::{prelude::*, ui::FocusPolicy};

fn label(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, text: &str) {
    parent.spawn((
        feathrise_editor_ui::localization::editor_label(text),
        TextFont {
            font: font.clone().into(),
            font_size: 11.0.into(),
            ..default()
        },
        TextColor(colors::TEXT_PRIMARY),
        TextLayout::no_wrap(),
        FocusPolicy::Pass,
    ));
}

fn button_node() -> Node {
    Node {
        width: percent(100),
        min_width: px(0),
        height: px(26),
        flex_shrink: 0.0,
        padding: UiRect::horizontal(px(8)),
        align_items: AlignItems::Center,
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(3)),
        ..default()
    }
}

pub(super) fn setup(
    mut commands: Commands,
    roots: Query<Entity, Added<MapEditorRoot>>,
    assets: Res<AssetServer>,
) {
    if roots.is_empty() {
        return;
    }
    let font = feathrise_editor_core::editor_font(&assets, crate::ui::scene::FONT_PATH);
    for root in &roots {
        commands.entity(root).with_children(|parent| {
            parent
                .spawn((
                    Backdrop,
                    Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    GlobalZIndex(428),
                    FocusPolicy::Block,
                ))
                .with_children(|overlay| {
                    overlay
                        .spawn((
                            Popup,
                            Node {
                                position_type: PositionType::Absolute,
                                width: px(184),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(2),
                                padding: UiRect::all(px(4)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(6)),
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                            ScrollPosition::default(),
                            bevy::ui::RelativeCursorPosition::default(),
                            BackgroundColor(colors::PANEL_BG_STRONG),
                            BorderColor::all(colors::COPPER_BORDER_WEAK),
                            FocusPolicy::Block,
                        ))
                        .with_children(|popup| {
                            for action in SceneMenuAction::ALL {
                                popup
                                    .spawn((
                                        InteractionButton,
                                        MenuButton(action),
                                        button_node(),
                                        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                                        BorderColor::all(colors::COPPER_BORDER_WEAK),
                                    ))
                                    .with_children(|parent| label(parent, &font, ""));
                            }
                            popup
                                .spawn((
                                    RenamePanel,
                                    Node {
                                        display: Display::None,
                                        width: percent(100),
                                        min_width: px(0),
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(6),
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                ))
                                .with_children(|rename| {
                                    label(rename, &font, "重命名对象");
                                    rename
                                        .spawn((
                                            InteractionButton,
                                            RenameField,
                                            Node {
                                                height: px(30),
                                                width: percent(100),
                                                min_width: px(0),
                                                padding: UiRect::horizontal(px(5)),
                                                align_items: AlignItems::Center,
                                                flex_shrink: 0.0,
                                                border: UiRect::all(px(1)),
                                                overflow: Overflow::clip(),
                                                ..default()
                                            },
                                            BackgroundColor(colors::INPUT_BG),
                                            BorderColor::all(colors::COPPER_BORDER),
                                        ))
                                        .with_child((
                                            RenameInput,
                                            crate::ui::text_input::EditorTextInput,
                                            crate::ui::text_input::single_line_input("", 128),
                                            TextFont {
                                                font: font.clone().into(),
                                                font_size: 11.0.into(),
                                                ..default()
                                            },
                                            TextColor(colors::TEXT_PRIMARY),
                                            TextLayout::no_wrap(),
                                            bevy::text::LineHeight::Px(20.0),
                                            Node {
                                                width: percent(100),
                                                min_width: px(0),
                                                height: px(20),
                                                overflow: Overflow::clip(),
                                                ..default()
                                            },
                                        ));
                                    rename
                                        .spawn((
                                            InteractionButton,
                                            RenameConfirm,
                                            button_node(),
                                            BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                                            BorderColor::all(colors::COPPER_BORDER_WEAK),
                                        ))
                                        .with_children(|parent| label(parent, &font, "确认"));
                                    rename
                                        .spawn((
                                            InteractionButton,
                                            RenameCancel,
                                            button_node(),
                                            BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                                            BorderColor::all(colors::COPPER_BORDER_WEAK),
                                        ))
                                        .with_children(|parent| label(parent, &font, "取消"));
                                });
                        });
                });
        });
    }
}

pub(super) fn sync(
    mut commands: Commands,
    context: MenuContext,
    editor: Res<MapEditorState>,
    mut menu: ResMut<SceneContextMenuState>,
    rows: Query<
        (Entity, &SceneObjectRow, &Node, &Interaction),
        (
            Without<Backdrop>,
            Without<Popup>,
            Without<MenuButton>,
            Without<RenamePanel>,
        ),
    >,
    mut panels: Query<
        (&mut Node, Has<Backdrop>, Option<&mut ScrollPosition>),
        (
            Or<(With<Backdrop>, With<Popup>)>,
            Without<MenuButton>,
            Without<RenamePanel>,
            Without<SceneObjectRow>,
        ),
    >,
    mut items: Query<
        (
            Entity,
            &MenuButton,
            &Interaction,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
            &Children,
            Has<bevy::ui::InteractionDisabled>,
        ),
        (
            Without<Backdrop>,
            Without<Popup>,
            Without<RenamePanel>,
            Without<SceneObjectRow>,
        ),
    >,
    mut rename: Query<
        &mut Node,
        (
            With<RenamePanel>,
            Without<Backdrop>,
            Without<Popup>,
            Without<MenuButton>,
            Without<SceneObjectRow>,
        ),
    >,
    mut texts: Query<&mut Text>,
    mut rename_buttons: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (
            Or<(With<RenameConfirm>, With<RenameCancel>)>,
            Without<MenuButton>,
        ),
    >,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    inputs: Query<Entity, With<RenameInput>>,
) {
    let valid = context.enabled(&editor)
        && menu.target.as_ref().is_some_and(|target| {
            target.object.current(&editor, &context.session)
                && rows.get(target.row).is_ok_and(|(_, row, node, _)| {
                    node.display != Display::None && row.0.as_ref() == Some(&target.object)
                })
        });
    if !valid && menu.target.is_some() {
        menu.target = None;
        menu.renaming = false;
        if inputs.iter().any(|entity| focus.get() == Some(entity)) {
            focus.clear();
        }
    }
    let target = menu.target.as_ref().map(|target| &target.object);
    let count = target.map_or(0, |target| {
        SceneMenuAction::ALL
            .into_iter()
            .filter(|action| target.supports(*action))
            .count()
    });
    let desired = if menu.renaming {
        Vec2::new(240.0, 146.0)
    } else {
        Vec2::new(184.0, 8.0 + count as f32 * 28.0)
    };
    let viewport = context.roots.single().ok().map_or(Vec2::ZERO, |(root, _)| {
        root.size() * root.inverse_scale_factor()
    });
    let rect =
        crate::ui::asset_context_menu::helpers::sized_menu_rect(menu.anchor, viewport, desired);
    for (mut node, backdrop, scroll) in &mut panels {
        if menu.is_changed()
            && let Some(mut scroll) = scroll
            && scroll.0 != Vec2::ZERO
        {
            scroll.0 = Vec2::ZERO;
        }
        if backdrop {
            let display = if valid { Display::Flex } else { Display::None };
            if node.display != display {
                node.display = display;
            }
        } else {
            let values = [
                px(rect.min.x),
                px(rect.min.y),
                px(rect.width()),
                px(rect.height()),
            ];
            if [node.left, node.top, node.width, node.height] != values {
                [node.left, node.top, node.width, node.height] = values;
            }
        }
    }
    for mut node in &mut rename {
        let display = if menu.renaming {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (entity, action, interaction, mut node, mut background, mut border, children, disabled) in
        &mut items
    {
        let visible = !menu.renaming && target.is_some_and(|target| target.supports(action.0));
        let enabled = visible && target.is_some_and(|target| target.allows(action.0));
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if enabled && disabled {
            commands
                .entity(entity)
                .remove::<bevy::ui::InteractionDisabled>();
        }
        if !enabled && !disabled {
            commands
                .entity(entity)
                .insert(bevy::ui::InteractionDisabled);
        }
        background.0 = if !enabled {
            colors::INPUT_BG
        } else {
            match interaction {
                Interaction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
                Interaction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
                Interaction::None => colors::BUTTON_DEV_SMALL_NORMAL,
            }
        };
        *border = BorderColor::all(if enabled && *interaction != Interaction::None {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child) {
                let label = target.map_or("", |target| action.0.label(target));
                if text.0 != label {
                    text.0 = label.into();
                }
            }
        }
    }
    for (interaction, mut background, mut border) in &mut rename_buttons {
        background.0 = match interaction {
            Interaction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
            Interaction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
            Interaction::None => colors::BUTTON_DEV_SMALL_NORMAL,
        };
        *border = BorderColor::all(colors::COPPER_BORDER_WEAK);
    }
}

/// 极矮窗口只滚动菜单正文；输入被同一遮罩拥有，背景列表不能跟随滚轮移动。
pub(super) fn scroll(
    menu: Res<SceneContextMenuState>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut popups: Query<
        (
            &ComputedNode,
            &bevy::ui::RelativeCursorPosition,
            &mut ScrollPosition,
        ),
        With<Popup>,
    >,
) {
    let (lines, pixels) =
        wheel
            .read()
            .fold((0.0, 0.0), |(lines, pixels), event| match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => (lines + event.y, pixels),
                bevy::input::mouse::MouseScrollUnit::Pixel => (lines, pixels + event.y),
            });
    let Some(target) = menu.target.as_ref() else {
        return;
    };
    let Ok((node, cursor, mut position)) = popups.single_mut() else {
        return;
    };
    if !cursor.cursor_over() {
        return;
    }
    let height = if menu.renaming {
        146.0
    } else {
        8.0 + SceneMenuAction::ALL
            .into_iter()
            .filter(|action| target.object.supports(*action))
            .count() as f32
            * 28.0
    };
    let maximum = (height - node.size().y * node.inverse_scale_factor()).max(0.0);
    let delta = lines * 24.0 + pixels * node.inverse_scale_factor();
    let value = (position.0.y - delta).clamp(0.0, maximum);
    if position.0.y != value {
        position.0.y = value;
    }
}
