//! 同一套墨底铜边固定菜单和中文名称浮层，按F12根的实际倍率钳制位置。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{
    components::*, resources::SceneContextMenuState, systems::{MenuContext, valid_target},
    types::{SceneMenuAction, SceneObjectTarget},
};
use crate::{authoring::EditorAuthoringSession, components::SceneEditorRoot, resources::SceneEditorState,
    ui::{asset_context_menu::helpers::sized_menu_rect, menu_style::{self, PANEL_BORDER, PANEL_INSET, ROW_HEIGHT}, theme as colors}};
use bevy::{prelude::*, ui::{FocusPolicy, RelativeCursorPosition}};

const CREATION_WIDTH: f32 = 86.;

fn label(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, text: &str, color: Color) {
    parent.spawn(menu_style::label(
        feathrise_editor_ui::localization::editor_label(text),
        font,
        color,
    ));
}

fn popup_node(creation: bool) -> Node {
    Node {
        display: if creation { Display::None } else { Display::Flex },
        width: px(if creation { CREATION_WIDTH } else { 184. }),
        ..menu_style::panel()
    }
}

fn spawn_actions(parent: &mut ChildSpawnerCommands, font: &Handle<Font>, creation: bool, arrow: &Handle<Image>) {
    for action in SceneMenuAction::panel_actions(creation) {
        let color = if action == SceneMenuAction::DeleteGlbFile { colors::DANGER_TEXT } else { colors::TEXT_PRIMARY };
        let mut item = parent.spawn((MenuButton(action), menu_style::item()));
        if creation { item.insert(NewGlbPopupItem); }
        item.with_children(|parent| {
            label(parent, font, "", color);
            if action == SceneMenuAction::NewGlbContent {
                parent.spawn(menu_style::arrow(arrow));
            }
        });
    }
}

fn available_actions<'a>(
    target: &'a SceneObjectTarget, session: &'a EditorAuthoringSession, creation: bool,
) -> impl Iterator<Item = SceneMenuAction> + 'a {
    SceneMenuAction::panel_actions(creation).filter(move |action|
        action.is_creation_child(target) == creation && target.menu_has(*action, session))
}

/// 子菜单与主菜单相邻一个边框像素。窄窗口优先留给父菜单更多命中面积。
pub(super) fn creation_submenu_rect(main: Rect, viewport: Vec2, parent_top: f32, desired: Vec2) -> Rect {
    let right_edge = main.max.x - PANEL_BORDER;
    let left_edge = main.min.x + PANEL_BORDER;
    let room_right = (viewport.x - 4. - right_edge).max(0.);
    let width = desired.x.min(viewport.x);
    let x = if width <= room_right || (width > left_edge && room_right >= left_edge) {
        right_edge
    } else {
        left_edge - width
    };
    // 通用定位会加上光标避让边距。这里传面板边线而非光标，抵消该偏移。
    sized_menu_rect(Vec2::new(x, parent_top) - Vec2::splat(4.), viewport, desired)
}

pub(super) fn setup(
    mut commands: Commands,
    roots: Query<Entity, Added<SceneEditorRoot>>,
    assets: Res<AssetServer>,
) {
    if roots.is_empty() {
        return;
    }
    let font = feathrise_editor_core::editor_font(&assets, crate::ui::scene::FONT_PATH);
    let arrow = assets.load(menu_style::ARROW_ICON);
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
                            popup_node(false),
                            ScrollPosition::default(),
                            bevy::ui::RelativeCursorPosition::default(),
                            BackgroundColor(colors::PANEL_BG_STRONG),
                            BorderColor::all(colors::COPPER_BORDER_WEAK),
                            FocusPolicy::Block,
                        ))
                        .with_children(|popup| {
                            spawn_actions(popup, &font, false, &arrow);
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
                                    label(rename, &font, "名称", colors::TEXT_PRIMARY);
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
                                            RenameConfirm,
                                            menu_style::item(),
                                        ))
                                        .with_children(|parent| label(parent, &font, "确认", colors::TEXT_PRIMARY));
                                    rename
                                        .spawn((
                                            RenameCancel,
                                            menu_style::item(),
                                        ))
                                        .with_children(|parent| label(parent, &font, "取消", colors::TEXT_PRIMARY));
                                });
                        });
                    // 侧面板与主面板为同级，主菜单自身的尺寸与滚动布局不会因展开变化。
                    overlay.spawn((NewGlbPopup, popup_node(true), ZIndex(1), ScrollPosition::default(),
                        RelativeCursorPosition::default(), BackgroundColor(colors::PANEL_BG_STRONG),
                        BorderColor::all(colors::COPPER_BORDER_WEAK), FocusPolicy::Block))
                        .with_children(|panel| spawn_actions(panel, &font, true, &arrow));
                });
        });
    }
}

pub(super) fn sync(
    mut commands: Commands,
    context: MenuContext,
    editor: Res<SceneEditorState>,
    mut menu: ResMut<SceneContextMenuState>,
    rows: Query<
        (Entity, &SceneObjectRow, &Node, &UiInteraction),
        (
            Without<Backdrop>,
            Without<Popup>,
            Without<NewGlbPopup>,
            Without<MenuButton>,
            Without<RenamePanel>,
        ),
    >,
    mut panels: Query<
        (&mut Node, Has<Backdrop>, Has<NewGlbPopup>, Option<&mut ScrollPosition>),
        (
            Or<(With<Backdrop>, With<Popup>, With<NewGlbPopup>)>,
            Without<MenuButton>,
            Without<RenamePanel>,
            Without<SceneObjectRow>,
        ),
    >,
    mut items: Query<
        (
            Entity,
            &MenuButton,
            &UiInteraction,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
            &Children,
            Has<bevy::ui::InteractionDisabled>,
            Has<NewGlbPopupItem>,
        ),
        (
            Without<Backdrop>,
            Without<Popup>,
            Without<NewGlbPopup>,
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
            Without<NewGlbPopup>,
            Without<MenuButton>,
            Without<SceneObjectRow>,
        ),
    >,
    mut texts: Query<&mut Text>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    mut rename_buttons: Query<
        (&UiInteraction, &mut BackgroundColor, &mut BorderColor),
        (
            Or<(With<RenameConfirm>, With<RenameCancel>)>,
            Without<MenuButton>,
        ),
    >,
    mut focus: ResMut<bevy::input_focus::InputFocus>,
    inputs: Query<Entity, With<RenameInput>>,
) {
    let valid = context.enabled(&editor)
        && menu.target.as_ref().is_some_and(|target| valid_target(target, &editor, &context, &rows));
    if !valid && menu.target.is_some() {
        menu.target = None;
        menu.renaming = false;
        menu.creating = false;
        if inputs.iter().any(|entity| focus.get() == Some(entity)) {
            focus.clear();
        }
    }
    let reset_scroll = std::mem::take(&mut menu.bypass_change_detection().reset_scroll);
    let target = menu.target.as_ref().map(|target| &target.object);
    let locale = locale.as_deref().copied().unwrap_or_default();
    let count = target.map_or(0, |target| available_actions(target, &context.session, false).count());
    let desired = if menu.renaming {
        Vec2::new(240.0, 146.0)
    } else {
        Vec2::new(184.0, menu_style::panel_height(count))
    };
    let (viewport, inverse_scale) = context.roots.single().ok()
        .map_or((Vec2::ZERO, 1.), |(root, _)| (root.size() * root.inverse_scale_factor(), root.inverse_scale_factor()));
    let main = sized_menu_rect(menu.anchor, viewport, desired);
    let main_scroll = if reset_scroll { 0. } else {
        panels.iter().find_map(|(_, backdrop, creation, scroll)| {
            (!backdrop && !creation).then(|| scroll.map_or(0., |scroll| scroll.y))
        }).unwrap_or(0.).clamp(0., (desired.y - main.height()).max(0.))
    };
    let creation = target.filter(|_| valid && menu.creating && !menu.renaming).and_then(|target| {
        let row = available_actions(target, &context.session, false)
            .position(|action| action == SceneMenuAction::NewGlbContent)?;
        // 与 Bevy 布局一样把滚动偏移向下取整到物理像素，尺寸变化时仅调整侧面板。
        let offset = (main_scroll / inverse_scale).floor() * inverse_scale;
        let top = main.min.y + PANEL_INSET + row as f32 * menu_style::row_pitch() - offset;
        let rows = available_actions(target, &context.session, true).count();
        let height = menu_style::panel_height(rows);
        (rows > 0 && top + ROW_HEIGHT > main.min.y && top < main.max.y)
            .then(|| (creation_submenu_rect(main, viewport, top, Vec2::new(CREATION_WIDTH, height)), height))
    });
    if menu.creating && creation.is_none() { menu.creating = false; }
    let target = menu.target.as_ref().map(|target| &target.object);
    for (mut node, backdrop, side, scroll) in &mut panels {
        let rect = if side { creation.map(|(rect, _)| rect) } else { valid.then_some(main) };
        let display = if rect.is_some() { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        if let Some(mut scroll) = scroll {
            let y = if !valid || reset_scroll { 0. } else if side {
                creation.map_or(0., |(rect, height)| scroll.y.clamp(0., (height - rect.height()).max(0.)))
            } else { main_scroll };
            if scroll.y != y { scroll.y = y; }
        }
        if !backdrop && let Some(rect) = rect {
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
    for (entity, action, interaction, mut node, mut background, mut border, children, disabled, side) in
        &mut items
    {
        let visible = !menu.renaming && target.is_some_and(|target| {
            menu.shows_action(target, action.0, &context.session, side)
        });
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
        let interaction = if menu.creating && action.0 == SceneMenuAction::NewGlbContent && *interaction == UiInteraction::None {
            UiInteraction::Hovered
        } else {
            *interaction
        };
        let (color, outline) = menu_style::item_colors(enabled, interaction);
        background.set_if_neq(BackgroundColor(color));
        border.set_if_neq(outline);
        for child in children {
            if let Ok(mut text) = texts.get_mut(*child) {
                let label = target.map_or("", |target| locale.editor_label(action.0.label(target)));
                if text.0 != label {
                    text.0 = label.into();
                }
            }
        }
    }
    for (interaction, mut background, mut border) in &mut rename_buttons {
        background.0 = match interaction {
            UiInteraction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
            UiInteraction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
            UiInteraction::None => colors::BUTTON_DEV_SMALL_NORMAL,
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
            &Node,
            &RelativeCursorPosition,
            &mut ScrollPosition,
            Has<NewGlbPopup>,
        ),
        Or<(With<Popup>, With<NewGlbPopup>)>,
    >,
) {
    let (lines, pixels) =
        wheel
            .read()
            .fold((0.0, 0.0), |(lines, pixels), event| match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => (lines + event.y, pixels),
                bevy::input::mouse::MouseScrollUnit::Pixel => (lines, pixels + event.y),
            });
    if menu.target.is_none() || (lines == 0. && pixels == 0.) { return; }
    // 极窄窗口可能需要部分重叠，滚轮优先交给位于顶层的侧面板。
    for side in [true, false] {
        if side && !menu.creating { continue; }
        for (layout, node, cursor, mut position, creation) in &mut popups {
            if creation != side || node.display == Display::None || !cursor.cursor_over() { continue; }
            let maximum = ((layout.content_size().y - layout.size().y) * layout.inverse_scale_factor()).max(0.);
            let delta = lines * 24. + pixels * layout.inverse_scale_factor();
            let y = (position.y - delta).clamp(0., maximum);
            if position.y != y { position.y = y; }
            return;
        }
    }
}
