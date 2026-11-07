//! 模式栏下方的定位、应用与坐标控件；所有编辑仅修改空间笔刷会话。
use feathrise_editor_ui::EditorButtonClick;
use super::{components::*, request::build_request, resources::SpatialBrushState, types::*};
use crate::{
    authoring::{EditorAuthoringSession, volume_sculpt::VolumeAuthoringState},
    resources::{MapEditorNumericScrubMouseGuard, MapEditorState},
    ui::{scene::FONT_PATH, theme as colors},
    viewport::{modes::ViewportControlsRoot, placement_move::MoveInputContext},
};
use bevy::{
    prelude::*,
    ui::{FocusPolicy, InteractionDisabled},
};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputChanged, NumericInputCommitted,
    NumericInputDisabled, NumericInputScrubConfig, NumericInputSetValue, NumericInputSpec,
    NumericInputValue, NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue,
    spawn_numeric_scrub_field,
};

fn button(row: &mut ChildSpawnerCommands, font: &Handle<Font>, label: &str, action: Action) {
    row.spawn((
        Button,
        ActionButton(action),
        FocusPolicy::Block,
        Node {
            height: px(26),
            width: px(76),
            flex_shrink: 0.,
            border: UiRect::all(px(1)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(colors::SLOT_BG),
        BorderColor::all(colors::COPPER_BORDER_WEAK),
    ))
    .with_child((
        feathrise_editor_ui::localization::editor_label(label),
        TextFont {
            font: font.clone().into(),
            font_size: 12.0.into(),
            ..default()
        },
        TextColor(colors::TEXT_PRIMARY),
        TextLayout::no_wrap(),
        FocusPolicy::Pass,
        Pickable::IGNORE,
    ));
}
fn row(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    actions: &[(&str, Action)],
    spatial: bool,
) {
    let mut node = parent.spawn(Node {
        column_gap: px(3),
        row_gap: px(3),
        flex_wrap: FlexWrap::Wrap,
        align_items: AlignItems::Center,
        ..default()
    });
    if spatial {
        node.insert(SpatialOnly);
    }
    node.with_children(|row| {
        if !label.is_empty() {
            row.spawn((
                feathrise_editor_ui::localization::editor_label(label),
                TextFont {
                    font: font.clone().into(),
                    font_size: 12.0.into(),
                    ..default()
                },
                TextColor(colors::TEXT_MUTED),
                TextLayout::no_wrap(),
                Node {
                    width: px(68),
                    flex_shrink: 0.,
                    ..default()
                },
                FocusPolicy::Pass,
                Pickable::IGNORE,
            ));
        }
        for (label, action) in actions {
            button(row, font, label, *action);
        }
    });
}
pub(super) fn setup(
    mut commands: Commands,
    roots: Query<Entity, Added<ViewportControlsRoot>>,
    assets: Res<AssetServer>,
) {
    for root in &roots {
        let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
        commands.entity(root).with_children(|parent| {
            parent
                .spawn((
                    Controls,
                    Node {
                        display: Display::None,
                        row_gap: px(3),
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                ))
                .with_children(|panel| {
                    row(
                        panel,
                        &font,
                        "定位方式",
                        &[("随鼠标", Action::Mouse), ("空间定位", Action::Spatial)],
                        false,
                    );
                    row(
                        panel,
                        &font,
                        "应用方式",
                        &[
                            ("单次", Action::Point),
                            ("直线", Action::Line),
                            ("路径", Action::Path),
                        ],
                        true,
                    );
                    row(
                        panel,
                        &font,
                        "",
                        &[
                            ("放置笔刷", Action::Place),
                            ("设为起点", Action::Start),
                            ("设为终点", Action::End),
                            ("执行雕刻", Action::Execute),
                            ("清除", Action::Clear),
                        ],
                        true,
                    );
                    panel
                        .spawn((
                            SpatialOnly,
                            Node {
                                column_gap: px(3),
                                row_gap: px(3),
                                flex_wrap: FlexWrap::Wrap,
                                ..default()
                            },
                        ))
                        .with_children(|row| {
                            for (axis, label) in ["X", "Y", "Z"].into_iter().enumerate() {
                                let options = NumericScrubFieldOptions::new(
                                    label,
                                    "m",
                                    NumericValue::F32(0.),
                                    NumericInputSpec::f32(2)
                                        .with_min(NumericValue::F32(-100_000.))
                                        .with_max(NumericValue::F32(100_000.))
                                        .with_step(NumericValue::F32(0.1)),
                                    NumericInputScrubConfig::linear(
                                        0.02, 0.1, -100_000., 100_000., 1.,
                                    ),
                                )
                                .with_style(
                                    NumericScrubFieldStyle {
                                        width: 100.,
                                        height: 26.,
                                        label_font_size: 11.,
                                        value_font_size: 11.,
                                        unit_font_size: 10.,
                                        ..default()
                                    },
                                );
                                let entity = spawn_numeric_scrub_field(
                                    row,
                                    Coordinate(axis),
                                    font.clone(),
                                    options,
                                );
                                row.commands().entity(entity).insert(NumericInputDisabled);
                            }
                        });
                    super::path::setup(panel, &font);
                    panel.spawn((
                        SpatialOnly,
                        Status,
                        Text::new(""),
                        TextFont {
                            font: font.clone().into(),
                            font_size: 11.0.into(),
                            ..default()
                        },
                        TextColor(colors::TEXT_MUTED),
                        TextLayout {
                            linebreak: LineBreak::AnyCharacter,
                            ..default()
                        },
                        Node {
                            max_width: px(350),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                });
        });
    }
}

pub(super) fn input(
    mut state: ResMut<SpatialBrushState>,
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    context: MoveInputContext,
    guard: Res<MapEditorNumericScrubMouseGuard>,
    volume: Res<VolumeAuthoringState>,
    buttons: Query<(&ActionButton, &EditorButtonClick, Has<InteractionDisabled>), Changed<EditorButtonClick>>,
    fields: Query<(&Coordinate, Has<NumericInputDisabled>)>,
    mut changed: MessageReader<NumericInputChanged>,
    mut committed: MessageReader<NumericInputCommitted>,
) {
    state.sync_scope(&editor, &session);
    let available = context.sculpt_parameters_available(&editor)
        && Identity::capture(&editor, &session).is_some()
        && !session.saving
        && !volume.busy()
        && state.pending.is_none()
        && state.drag.is_none();
    let mut apply_coordinate = |entity: Entity, value: NumericValue| {
        if !available || !state.enabled {
            return;
        }
        let Ok((axis, false)) = fields.get(entity) else {
            return;
        };
        let Some(value) = value.as_f32().filter(|v| v.is_finite()) else {
            return;
        };
        if let Some(center) = state.center.as_mut() {
            center[axis.0] = value.clamp(-100_000., 100_000.);
        }
    };
    for event in changed.read() {
        apply_coordinate(event.entity, event.value);
    }
    for event in committed.read() {
        apply_coordinate(event.entity, event.value);
    }
    if !available || !context.sculpt_available(&editor) || guard.blocked() || state.owns_mouse {
        return;
    }
    for (button, interaction, disabled) in &buttons {
        if disabled || !interaction.is_clicked() {
            continue;
        }
        match button.0 {
            Action::Mouse => {
                state.enabled = false;
                state.armed = false;
                state.click_pending = false;
                state.click_identity = None;
                state.pending = None;
            }
            Action::Spatial => {
                state.enabled = true;
                state.armed = state.center.is_none();
            }
            Action::Point => state.mode = ApplyMode::Point,
            Action::Line => state.mode = ApplyMode::Line,
            Action::Path => {
                state.mode = ApplyMode::Path;
                state.enabled = true;
            }
            Action::Place => {
                if state.mode == ApplyMode::Path {
                    state.path.selected = None;
                }
                state.armed = true;
                state.click_pending = false;
                state.click_identity = None;
                editor.status = "在地形上点击放置笔刷；之后可以通过手柄或XYZ移入地下".into();
            }
            Action::Start => {
                if state.enabled
                    && let Some(center) = state.center
                {
                    // 从已执行的固定刷心接续下一段；旧终点须重新确认，不能复用旧路径。
                    state.mode = ApplyMode::Line;
                    state.start = Some(center);
                    state.end = None;
                    editor.status = "已设置直线起点，移动笔刷后设置新的终点".into();
                } else {
                    editor.status = "请先放置笔刷".into();
                }
            }
            Action::End => {
                state.end = state.center;
                editor.status = if state.end.is_some() {
                    "已设置直线终点，检查预览后执行雕刻"
                } else {
                    "请先放置笔刷"
                }
                .into();
            }
            Action::Execute => match if state.mode == ApplyMode::Path {
                super::path::build_request(&state, &editor, &session)
            } else {
                build_request(
                    &editor,
                    &session,
                    state.center,
                    state.mode,
                    state.start,
                    state.end,
                )
            } {
                Ok(request) => {
                    // 确认只冻结雕刻请求，保留定位笔刷及XYZ，采用后可直接再设为起点。
                    state.pending = Some(request);
                    editor.status = "已确认空间雕刻，等待完整地形采用…".into();
                }
                Err(error) => editor.status = error,
            },
            Action::Clear => state.clear(),
        }
    }
}

pub(super) fn sync(
    mut commands: Commands,
    state: Res<SpatialBrushState>,
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    volume: Res<VolumeAuthoringState>,
    context: MoveInputContext,
    mut nodes: Query<
        (&mut Node, Has<SpatialOnly>, Has<super::path::PathOnly>),
        Or<(With<Controls>, With<SpatialOnly>)>,
    >,
    mut buttons: Query<(
        Entity,
        &ActionButton,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
        Has<InteractionDisabled>,
    )>,
    fields: Query<(
        Entity,
        &Coordinate,
        &NumericInputValue,
        Has<NumericInputDisabled>,
    )>,
    active: Res<ActiveNumericInput>,
    scrub: Res<ActiveNumericInputScrub>,
    mut values: MessageWriter<NumericInputSetValue>,
    mut texts: Query<&mut Text, With<Status>>,
) {
    let visible = editor.open
        && editor.tool.is_volume_sculpt()
        && Identity::capture(&editor, &session).is_some();
    let available = visible
        && context.sculpt_parameters_available(&editor)
        && !session.saving
        && !volume.busy()
        && state.pending.is_none()
        && !state.owns_mouse;
    for (mut node, spatial, path) in &mut nodes {
        let display =
            if visible && (!spatial || state.enabled) && (!path || state.mode == ApplyMode::Path) {
                Display::Flex
            } else {
                Display::None
            };
        if node.display != display {
            node.display = display;
        }
    }
    for (entity, button, interaction, mut bg, mut border, disabled) in &mut buttons {
        let selected = match button.0 {
            Action::Mouse => !state.enabled,
            Action::Spatial => state.enabled,
            Action::Point => state.mode == ApplyMode::Point,
            Action::Line => state.mode == ApplyMode::Line,
            Action::Path => state.mode == ApplyMode::Path,
            Action::Place => state.armed,
            _ => false,
        };
        let ready = available
            && match button.0 {
                Action::Start => {
                    state.enabled && state.mode != ApplyMode::Path && state.center.is_some()
                }
                Action::End => {
                    state.enabled && state.mode == ApplyMode::Line && state.center.is_some()
                }
                Action::Execute => {
                    state.enabled
                        && if state.mode == ApplyMode::Path {
                            state.path.ready(state.center)
                        } else if state.mode == ApplyMode::Line {
                            state.start.is_some() && state.end.is_some()
                        } else {
                            state.center.is_some()
                        }
                }
                Action::Place | Action::Clear => state.enabled,
                _ => true,
            };
        let color = if selected {
            colors::BUTTON_SETTINGS_PRESSED
        } else if ready && *interaction == Interaction::Hovered {
            colors::BUTTON_SETTINGS_HOVERED
        } else {
            colors::SLOT_BG
        };
        if bg.0 != color {
            bg.0 = color;
        }
        let edge = BorderColor::all(if selected {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
        if *border != edge {
            *border = edge;
        }
        if disabled == ready {
            if ready {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
    }
    for (entity, axis, value, disabled) in &fields {
        let ready = available && state.enabled && state.center.is_some();
        if disabled == ready {
            if ready {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            }
        }
        let target = state.center.map_or(0., |p| p[axis.0]);
        if active.entity != Some(entity)
            && scrub.entity != Some(entity)
            && value.0.as_f32() != Some(target)
        {
            values.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(target),
                cancel_edit: true,
            });
        }
    }
    let status = if state.armed {
        "点击地形放置笔刷；这次点击只定位。".into()
    } else if state.center.is_none() {
        "先放置笔刷，再调整XYZ位置。".into()
    } else if state.mode == ApplyMode::Path {
        "路径XYZ代表底部位置；加入节点后可继续移动刷心，前后选择节点可修改已有路径。".into()
    } else if state.mode == ApplyMode::Line {
        match (state.start, state.end) {
            (Some(a), Some(b)) => format!(
                "起点 ({:.2}, {:.2}, {:.2}) → 终点 ({:.2}, {:.2}, {:.2})，长度 {:.2}m",
                a.x,
                a.y,
                a.z,
                b.x,
                b.y,
                b.z,
                a.distance(b)
            ),
            (Some(_), None) => "起点已设置；移动笔刷后设置终点。".into(),
            _ => "设置起点 → 移动笔刷 → 设置终点 → 执行雕刻。".into(),
        }
    } else {
        "拖动XYZ轴/XZ方框或修改坐标；点击执行雕刻才改变地形。".into()
    };
    for mut text in &mut texts {
        if text.0 != status {
            text.0.clone_from(&status);
        }
    }
}
