//! 路径节点与截面控件使用原F12主题；仅选中节点拥有坐标手柄，新增刷心独立于旧节点。
use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use super::{systems::build_request, types::*};
use crate::{
    authoring::{EditorAuthoringSession, volume_sculpt::VolumeAuthoringState},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::theme as colors,
    viewport::{
        placement_move::MoveInputContext,
        spatial_brush::{
            resources::SpatialBrushState,
            types::{ApplyMode, Identity},
        },
    },
};
use bevy::{
    prelude::*,
    tasks::IoTaskPool,
    ui::{FocusPolicy, InteractionDisabled},
};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputChanged, NumericInputCommitted,
    NumericInputDisabled, NumericInputScrubConfig, NumericInputSetValue, NumericInputSpec,
    NumericInputValue, NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue,
    spawn_numeric_scrub_field,
};
use feathrise_terrain::volume::VolumePathSection;
use std::sync::{Arc, atomic::AtomicBool};

#[derive(Component)]
pub(crate) struct PathOnly;
#[derive(Component)]
pub(in crate::viewport::spatial_brush) struct PathButton(pub PathAction);
#[derive(Component)]
pub(in crate::viewport::spatial_brush) struct PathSize(pub usize);
#[derive(Component)]
pub(in crate::viewport::spatial_brush) struct PathStatus;

pub(in crate::viewport::spatial_brush) fn setup(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
) {
    parent
        .spawn((
            PathOnly,
            super::super::components::SpatialOnly,
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                ..default()
            },
        ))
        .with_children(|panel| {
            for actions in [
                vec![
                    ("加入节点", PathAction::Add),
                    ("定位新点", PathAction::NewPoint),
                    ("上一节点", PathAction::Previous),
                    ("下一节点", PathAction::Next),
                    ("删除节点", PathAction::Delete),
                ],
                vec![
                    ("折线路径", PathAction::Polyline),
                    ("平滑曲线", PathAction::Curve),
                    ("矩形截面", PathAction::Rectangle),
                    ("圆形截面", PathAction::Round),
                ],
                vec![
                    ("保存路径", PathAction::Save),
                    ("上一存档", PathAction::SavedPrevious),
                    ("下一存档", PathAction::SavedNext),
                    ("加载路径", PathAction::Load),
                ],
            ] {
                panel
                    .spawn(Node {
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(3),
                        row_gap: px(3),
                        ..default()
                    })
                    .with_children(|row| {
                        for (label, action) in actions {
                            row.spawn((
                                InteractionButton,
                                PathButton(action),
                                FocusPolicy::Block,
                                Node {
                                    width: px(76),
                                    height: px(26),
                                    flex_shrink: 0.,
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    border: UiRect::all(px(1)),
                                    ..default()
                                },
                                BackgroundColor(colors::SLOT_BG),
                                BorderColor::all(colors::COPPER_BORDER_WEAK),
                            ))
                            .with_child((
                                Text::new(label),
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
                    });
            }
            panel
                .spawn(Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(3),
                    row_gap: px(3),
                    ..default()
                })
                .with_children(|row| {
                    for (index, label) in ["通道宽", "通道高"].into_iter().enumerate() {
                        let options = NumericScrubFieldOptions::new(
                            label,
                            "m",
                            NumericValue::F32(2.),
                            NumericInputSpec::f32(2)
                                .with_min(NumericValue::F32(0.5))
                                .with_max(NumericValue::F32(16.))
                                .with_step(NumericValue::F32(0.25)),
                            NumericInputScrubConfig::linear(0.02, 0.25, 0.5, 16., 1.),
                        )
                        .with_style(NumericScrubFieldStyle {
                            width: 140.,
                            height: 26.,
                            label_font_size: 11.,
                            value_font_size: 11.,
                            unit_font_size: 10.,
                            ..default()
                        });
                        let entity =
                            spawn_numeric_scrub_field(row, PathSize(index), font.clone(), options);
                        row.commands().entity(entity).insert(NumericInputDisabled);
                    }
                });
            panel.spawn((
                PathStatus,
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
}

pub(in crate::viewport::spatial_brush) fn input(
    mut state: ResMut<SpatialBrushState>,
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    volume: Res<VolumeAuthoringState>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    context: MoveInputContext,
    buttons: Query<(&PathButton, &EditorButtonClick, Has<InteractionDisabled>), Changed<EditorButtonClick>>,
    fields: Query<(&PathSize, Has<NumericInputDisabled>)>,
    coordinates: Query<(
        &super::super::components::Coordinate,
        Has<NumericInputDisabled>,
    )>,
    mut changed: MessageReader<NumericInputChanged>,
    mut committed: MessageReader<NumericInputCommitted>,
) {
    state.sync_scope(&editor, &session);
    let available = state.enabled
        && state.mode == ApplyMode::Path
        && Identity::capture(&editor, &session).is_some()
        && context.sculpt_parameters_available(&editor)
        && !session.saving
        && !volume.busy()
        && state.pending.is_none()
        && state.drag.is_none()
        && !state.path.busy();
    let mut apply = |entity: Entity, value: NumericValue| {
        if !available {
            return;
        }
        if let Ok((axis, false)) = coordinates.get(entity) {
            if let Some(value) = value.as_f32().filter(|v| v.is_finite())
                && let Some(center) = state.center.as_mut()
            {
                center[axis.0] = value.clamp(-100_000., 100_000.);
            }
            return;
        }
        let Ok((field, false)) = fields.get(entity) else {
            return;
        };
        let Some(value) = value
            .as_f32()
            .filter(|v| v.is_finite() && (0.5..=16.).contains(v))
        else {
            return;
        };
        let slot = if field.0 == 0 {
            &mut state.path.parameters.width
        } else {
            &mut state.path.parameters.height
        };
        if *slot != value {
            *slot = value;
            state.path.queue(Edit::Refresh);
        }
    };
    for event in changed.read() {
        apply(event.entity, event.value);
    }
    for event in committed.read() {
        apply(event.entity, event.value);
    }
    if !available || guard.blocked() || state.owns_mouse || !context.sculpt_available(&editor) {
        return;
    }
    for (button, interaction, disabled) in &buttons {
        if disabled || !interaction.is_clicked() {
            continue;
        }
        match button.0 {
            PathAction::Add => {
                if let Some(center) = state.center {
                    state.path.queue(Edit::Add(center));
                    editor.status = "已加入路径底部节点；移动刷心可继续加入下一点".into();
                }
            }
            PathAction::NewPoint => {
                state.path.selected = None;
                state.path.observed_center = None;
                editor.status = "当前刷心用于新节点；移动后点击加入节点".into();
            }
            PathAction::Delete => {
                if let Some(index) = state.path.selected {
                    state.path.queue(Edit::Delete(index));
                }
            }
            PathAction::Previous | PathAction::Next => {
                let count = state.path.nodes.len();
                if count == 0 {
                    continue;
                }
                let index = if button.0 == PathAction::Previous {
                    state
                        .path
                        .selected
                        .map_or(count - 1, |i| i.saturating_sub(1))
                } else {
                    state.path.selected.map_or(0, |i| (i + 1).min(count - 1))
                };
                state.path.selected = Some(index);
                state.center = Some(Vec3::from_array(state.path.nodes[index]));
                state.path.observed_center = state.center;
                state.armed = false;
                state.click_pending = false;
                state.click_identity = None;
            }
            PathAction::Polyline | PathAction::Curve => {
                state.path.parameters.curved = button.0 == PathAction::Curve;
                state.path.queue(Edit::Refresh);
            }
            PathAction::Rectangle | PathAction::Round => {
                state.path.parameters.section = if button.0 == PathAction::Rectangle {
                    VolumePathSection::Rectangle
                } else {
                    VolumePathSection::Round
                };
                state.path.queue(Edit::Refresh);
            }
            PathAction::Save => {
                if build_request(&state, &editor, &session).is_err() || state.path.io.is_some() {
                    continue;
                }
                let draft = session.draft.as_ref().unwrap();
                let folder = draft.folder.clone();
                let map = draft.scene_id.0.clone();
                let nodes = state.path.nodes.clone();
                let parameters = state.path.parameters;
                let active = Arc::new(AtomicBool::new(true));
                state.path.io_active = active.clone();
                state.path.io_scope = Some((map.clone(), folder.clone()));
                state.path.io = Some(IoTaskPool::get().spawn(async move {
                    IoResult {
                        library: crate::authoring::save_sculpt_path(
                            folder, map, nodes, parameters, active,
                        )
                        .await,
                        saved: true,
                    }
                }));
                editor.status = "正在后台保存工具路径；地形保持当前版本".into();
            }
            PathAction::SavedPrevious => {
                state.path.saved_index = state.path.saved_index.saturating_sub(1)
            }
            PathAction::SavedNext => {
                if let Some(library) = &state.path.library {
                    state.path.saved_index =
                        (state.path.saved_index + 1).min(library.paths.len().saturating_sub(1));
                }
            }
            PathAction::Load => {
                if let Some(library) = state.path.library.clone()
                    && let Some(entry) = library.paths.get(state.path.saved_index)
                {
                    let index = state.path.saved_index;
                    state.path.parameters = entry.parameters;
                    state.path.queue(Edit::Load(library, index));
                    state.armed = false;
                    state.click_pending = false;
                    state.click_identity = None;
                    editor.status = "正在加载路径预览；点击执行雕刻才应用到地形".into();
                }
            }
        }
        if state.path.busy() {
            break;
        } // 同帧多个结构点击也只受理一项，不静默覆盖前一项节点操作。
    }
}

pub(in crate::viewport::spatial_brush) fn sync(
    mut commands: Commands,
    state: Res<SpatialBrushState>,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    volume: Res<VolumeAuthoringState>,
    context: MoveInputContext,
    mut buttons: Query<(
        Entity,
        &PathButton,
        &mut BackgroundColor,
        Has<InteractionDisabled>,
    )>,
    fields: Query<(
        Entity,
        &PathSize,
        &NumericInputValue,
        Has<NumericInputDisabled>,
    )>,
    active: Res<ActiveNumericInput>,
    scrub: Res<ActiveNumericInputScrub>,
    mut values: MessageWriter<NumericInputSetValue>,
    mut texts: Query<&mut Text, With<PathStatus>>,
) {
    let available = state.enabled
        && state.mode == ApplyMode::Path
        && Identity::capture(&editor, &session).is_some()
        && context.sculpt_parameters_available(&editor)
        && !session.saving
        && !volume.busy()
        && state.pending.is_none()
        && state.drag.is_none()
        && !state.path.busy()
        && !state.owns_mouse;
    let count = state.path.nodes.len();
    let saved_count = state.path.library.as_ref().map_or(0, |l| l.paths.len());
    for (entity, button, mut bg, disabled) in &mut buttons {
        let ready = available
            && match button.0 {
                PathAction::Add | PathAction::NewPoint => state.center.is_some(),
                PathAction::Delete => state.path.selected.is_some(),
                PathAction::Previous | PathAction::Next => count > 0,
                PathAction::Save => state.path.ready(state.center) && state.path.io.is_none(),
                PathAction::SavedPrevious | PathAction::SavedNext | PathAction::Load => {
                    saved_count > 0 && state.path.io.is_none()
                }
                _ => true,
            };
        if ready == disabled {
            if ready {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        let selected = match button.0 {
            PathAction::Curve => state.path.parameters.curved,
            PathAction::Polyline => !state.path.parameters.curved,
            PathAction::Rectangle => state.path.parameters.section == VolumePathSection::Rectangle,
            PathAction::Round => state.path.parameters.section == VolumePathSection::Round,
            _ => false,
        };
        let color = if selected {
            colors::BUTTON_SETTINGS_PRESSED
        } else {
            colors::SLOT_BG
        };
        if bg.0 != color {
            bg.0 = color;
        }
    }
    for (entity, field, value, disabled) in &fields {
        if available == disabled {
            if available {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            }
        }
        let target = if field.0 == 0 {
            state.path.parameters.width
        } else {
            state.path.parameters.height
        };
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
    let node = state.path.selected.map_or_else(
        || "刷心用于新节点".to_owned(),
        |i| format!("正在编辑节点 {}", i + 1),
    );
    let saved = state
        .path
        .library
        .as_ref()
        .and_then(|l| l.paths.get(state.path.saved_index))
        .map_or("暂无保存路径", |p| p.name.as_str());
    let length = state.path.plan.as_ref().map_or(0., |p| p.length());
    let phase = state.path.error.as_deref().unwrap_or(if state.path.busy() {
        "路径预览更新中"
    } else if count < 2 {
        "至少加入两个不同节点"
    } else {
        "检查完整预览后执行雕刻"
    });
    let status = format!(
        "{node}；共{count}节点，{length:.2}m。{phase}。\n路径库：{saved}（{}条）",
        saved_count
    );
    for mut text in &mut texts {
        if text.0 != status {
            text.0.clone_from(&status);
        }
    }
}
