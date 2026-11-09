//! 四球着色按钮与按当前模式显示的紧凑选项；沿用正式松开点击及统一鼠标门。
use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::InteractionButton;
use super::{components::*, resources::ShadingState, types::*};
use crate::{
    components::MapEditorViewport,
    resources::{MapEditorNumericScrubMouseGuard, MapEditorState},
    ui::theme as colors,
};
use bevy::{
    prelude::*,
    ui::{FocusPolicy, RelativeCursorPosition},
};
use feathrise_editor_ui::EditorButtonClick;

pub(super) fn setup(
    mut commands: Commands,
    assets: Res<AssetServer>,
    viewports: Query<Entity, Added<MapEditorViewport>>,
) {
    let font: Handle<Font> = feathrise_editor_core::editor_font(&assets, crate::ui::scene::FONT_PATH);
    for viewport in &viewports {
        commands.entity(viewport).with_children(|parent| {
            parent
                .spawn((
                    ViewShadingRoot,
                    Name::new("视口着色"),
                    UiInteraction::default(),
                    RelativeCursorPosition::default(),
                    FocusPolicy::Block,
                    ZIndex(28),
                    Node {
                        position_type: PositionType::Absolute,
                        right: px(8),
                        top: px(8),
                        height: px(28),
                        column_gap: px(2),
                        padding: UiRect::all(px(2)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(2)),
                        ..default()
                    },
                    BackgroundColor(colors::PANEL_BG_STRONG),
                    BorderColor::all(colors::COPPER_BORDER_WEAK),
                ))
                .with_children(|row| {
                    for mode in ShadingMode::ALL {
                        row.spawn((
                            InteractionButton,
                            Name::new(mode.label()),
                            ShadingButton(ShadingAction::Mode(mode)),
                            crate::ui::tooltips::EditorTooltipTarget(
                                crate::ui::tooltips::TooltipAction::ViewShading(mode),
                            ),
                            FocusPolicy::Block,
                            Node {
                                width: px(24),
                                height: px(22),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border_radius: BorderRadius::all(px(2)),
                                ..default()
                            },
                            BackgroundColor(colors::SLOT_BG),
                        ))
                        .with_children(|button| {
                            // 小球直接由UI圆、半球和高光构成，沿用同一图标尺寸而不依赖字体特殊字符。
                            button
                                .spawn((
                                    Pickable::IGNORE,
                                    Node {
                                        width: px(16),
                                        height: px(16),
                                        overflow: Overflow::clip(),
                                        border: UiRect::all(px(1)),
                                        border_radius: BorderRadius::all(percent(50)),
                                        ..default()
                                    },
                                    BorderColor::all(colors::TEXT_PRIMARY),
                                    BackgroundColor(match mode {
                                        ShadingMode::Wireframe => colors::SLOT_BG,
                                        ShadingMode::Solid => Color::srgb(0.62, 0.64, 0.66),
                                        ShadingMode::MaterialPreview => {
                                            Color::srgb(0.35, 0.38, 0.43)
                                        }
                                        ShadingMode::Rendered => Color::srgb(0.16, 0.18, 0.21),
                                    }),
                                ))
                                .with_children(|sphere| {
                                    sphere.spawn((
                                        Pickable::IGNORE,
                                        Node {
                                            position_type: PositionType::Absolute,
                                            left: px(2),
                                            top: px(2),
                                            width: px(if mode == ShadingMode::Wireframe {
                                                5.0
                                            } else {
                                                8.0
                                            }),
                                            height: px(10),
                                            border: UiRect::all(px(1)),
                                            border_radius: BorderRadius::all(percent(50)),
                                            ..default()
                                        },
                                        BorderColor::all(colors::TEXT_PRIMARY),
                                        BackgroundColor(if mode == ShadingMode::Wireframe {
                                            Color::NONE
                                        } else {
                                            Color::srgb(0.86, 0.88, 0.9)
                                        }),
                                    ));
                                    if mode == ShadingMode::Wireframe {
                                        sphere.spawn((
                                            Pickable::IGNORE,
                                            Node {
                                                position_type: PositionType::Absolute,
                                                left: px(1),
                                                top: px(6),
                                                width: px(12),
                                                height: px(1),
                                                ..default()
                                            },
                                            BackgroundColor(colors::TEXT_PRIMARY),
                                        ));
                                    }
                                });
                        });
                    }
                    row.spawn((
                        InteractionButton,
                        ShadingButton(ShadingAction::Menu),
                        Name::new("视口着色选项"),
                        FocusPolicy::Block,
                        Node {
                            width: px(20),
                            height: px(22),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                        BackgroundColor(colors::SLOT_BG),
                    ))
                    .with_child((
                        Text::new("▼"),
                        TextFont {
                            font: font.clone().into(),
                            font_size: 12.0.into(),
                            ..default()
                        },
                        TextColor(colors::TEXT_PRIMARY),
                        Pickable::IGNORE,
                    ));
                    row.spawn((
                        ShadingMenu,
                        ViewShadingRoot,
                        UiInteraction::default(),
                        RelativeCursorPosition::default(),
                        FocusPolicy::Block,
                        Node {
                            display: Display::None,
                            position_type: PositionType::Absolute,
                            top: px(30),
                            right: px(0),
                            width: px(142),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                            padding: UiRect::all(px(4)),
                            border: UiRect::all(px(1)),
                            ..default()
                        },
                        BackgroundColor(colors::PANEL_BG_STRONG),
                        BorderColor::all(colors::COPPER_BORDER_WEAK),
                        ZIndex(40),
                    ))
                    .with_children(|menu| {
                        for action in [
                            ShadingAction::Xray,
                            ShadingAction::Cavity,
                            ShadingAction::Normals,
                            ShadingAction::Studio,
                            ShadingAction::Background,
                        ] {
                            menu.spawn((
                                InteractionButton,
                                ShadingButton(action),
                                FocusPolicy::Block,
                                Node {
                                    height: px(25),
                                    width: percent(100),
                                    padding: UiRect::horizontal(px(5)),
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                                BackgroundColor(colors::SLOT_BG),
                            ))
                            .with_child((
                                ShadingOptionText(action),
                                Text::new(""),
                                TextFont {
                                    font: font.clone().into(),
                                    font_size: 12.0.into(),
                                    ..default()
                                },
                                TextColor(colors::TEXT_PRIMARY),
                                TextLayout::no_wrap(),
                                Pickable::IGNORE,
                            ));
                        }
                    });
                });
        });
    }
}

pub(super) fn close_menu_escape(
    mut state: ResMut<ShadingState>,
    mut keyboard: Option<ResMut<ButtonInput<KeyCode>>>,
    editor: Res<MapEditorState>,
    input: super::super::view_controls_input::ViewControlsInput,
) {
    if !input.available(&editor) {
        state.menu_open = false;
        return;
    }
    if state.menu_open
        && keyboard
            .as_ref()
            .is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
    {
        state.menu_open = false;
        if let Some(keys) = keyboard.as_deref_mut() {
            keys.clear_just_pressed(KeyCode::Escape);
        }
    }
}

pub(super) fn input(
    mut state: ResMut<ShadingState>,
    mut editor: ResMut<MapEditorState>,
    input: super::super::view_controls_input::ViewControlsInput,
    guard: Res<MapEditorNumericScrubMouseGuard>,
    mesh: Option<Res<crate::authoring::mesh_edit::MeshEditState>>,
    buttons: Query<(Entity, &ShadingButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    nodes: Query<(&Node, Option<&ChildOf>)>,
    mut keyboard: Option<ResMut<ButtonInput<KeyCode>>>,
) {
    if !input.available(&editor)
        || editor.new_map_dialog_open
        || editor.map_switch_confirmation.is_some()
        || editor.map_transition_in_progress
        || editor.pending_map_transition.is_some()
        || guard.blocked()
        || mesh.as_ref().is_some_and(|mesh| mesh.moving())
    {
        state.menu_open = false;
        return;
    }
    if state.menu_open
        && keyboard
            .as_ref()
            .is_some_and(|keys| keys.just_pressed(KeyCode::Escape))
    {
        state.menu_open = false;
        if let Some(keys) = keyboard.as_deref_mut() {
            keys.clear_just_pressed(KeyCode::Escape);
        }
        return; // Esc只关闭当前显示菜单，不得同时清除元素选择。
    }
    for (entity, button, click) in &buttons {
        if !click.is_clicked() || !super::super::modes::controls_visible(entity, &nodes) {
            continue;
        }
        match button.0 {
            ShadingAction::Mode(mode) => {
                state.mode = mode;
                state.menu_open = false;
            }
            ShadingAction::Menu => state.menu_open = !state.menu_open,
            ShadingAction::Xray => state.xray = !state.xray,
            ShadingAction::Cavity => state.cavity = !state.cavity,
            ShadingAction::Normals => state.normals = !state.normals,
            ShadingAction::Studio => state.studio = !state.studio,
            ShadingAction::Background => state.background = !state.background,
        }
        editor.suppress_world_pick_this_frame = true;
        break;
    }
}

fn option(action: ShadingAction, state: &ShadingState) -> (&'static str, bool, bool) {
    match action {
        ShadingAction::Xray => ("穿透线框", state.xray, state.mode == ShadingMode::Wireframe),
        ShadingAction::Cavity => (
            "增强凹凸轮廓",
            state.cavity,
            state.mode == ShadingMode::Solid,
        ),
        ShadingAction::Normals => ("法线颜色", state.normals, state.mode == ShadingMode::Solid),
        ShadingAction::Studio => (
            "工作室补光",
            state.studio,
            state.mode == ShadingMode::MaterialPreview,
        ),
        ShadingAction::Background => ("显示场景背景", state.background, state.mode != ShadingMode::Wireframe),
        _ => ("", false, true),
    }
}
pub(super) fn style(
    state: Res<ShadingState>,
    mut menus: Query<&mut Node, (With<ShadingMenu>, Without<ShadingButton>)>,
    mut buttons: Query<
        (
            &ShadingButton,
            &UiInteraction,
            &mut Node,
            &mut BackgroundColor,
        ),
        Without<ShadingMenu>,
    >,
    mut texts: Query<(&ShadingOptionText, &mut Text)>,
) {
    for mut node in &mut menus {
        let display = if state.menu_open {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (button, interaction, mut node, mut bg) in &mut buttons {
        let selected = match button.0 {
            ShadingAction::Mode(mode) => state.mode == mode,
            ShadingAction::Menu => state.menu_open,
            action => option(action, &state).1,
        };
        let display = if option(button.0, &state).2 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let color = match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (_, true) => colors::SELECT_BG,
            _ => colors::SLOT_BG,
        };
        if bg.0 != color {
            bg.0 = color;
        }
    }
    for (option_text, mut text) in &mut texts {
        let (label, enabled, _) = option(option_text.0, &state);
        let value = format!("{} {label}", if enabled { "✓" } else { "  " });
        if text.as_str() != value {
            **text = value;
        }
    }
}

/// 工具条已固定单排横滚；缩窄或缩放不再把观察控件从右上角移到底部。
pub(super) fn place_overlays(
    viewports: Query<&ComputedNode, With<MapEditorViewport>>,
    mut shading: Query<
        &mut Node,
        (
            With<ViewShadingRoot>,
            Without<ShadingMenu>,
            Without<crate::components::MapEditorOrientationAxisRoot>,
        ),
    >,
    mut axes: Query<
        &mut Node,
        (
            With<crate::components::MapEditorOrientationAxisRoot>,
            Without<ViewShadingRoot>,
        ),
    >,
    mut menus: Query<
        &mut Node,
        (
            With<ShadingMenu>,
            Without<crate::components::MapEditorOrientationAxisRoot>,
        ),
    >,
) {
    let Ok(_) = viewports.single() else {
        return;
    };
    // XYZ 与着色栏使用视口自身的绝对锚点，不参与工具条布局，也不侵占底部游戏 HUD。
    for mut node in &mut shading {
        if node.position_type != PositionType::Absolute {
            node.position_type = PositionType::Absolute;
        }
        if node.right != px(8) {
            node.right = px(8);
        }
        if node.top != px(8) {
            node.top = px(8);
        }
        if node.bottom != Val::Auto {
            node.bottom = Val::Auto;
        }
    }
    for mut node in &mut axes {
        if node.position_type != PositionType::Absolute {
            node.position_type = PositionType::Absolute;
        }
        if node.right != px(8) {
            node.right = px(8);
        }
        if node.top != px(42) {
            node.top = px(42);
        }
        if node.bottom != Val::Auto {
            node.bottom = Val::Auto;
        }
    }
    for mut node in &mut menus {
        if node.position_type != PositionType::Absolute {
            node.position_type = PositionType::Absolute;
        }
        if node.right != px(0) {
            node.right = px(0);
        }
        if node.top != px(30) {
            node.top = px(30);
        }
        if node.bottom != Val::Auto {
            node.bottom = Val::Auto;
        }
    }
}
