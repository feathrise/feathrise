//! 网格数字控件冻结一次编辑的完整目标；创建、所选变换、行内LOD与通用策略不会互相串写。
use feathrise_editor_ui::UiInteraction;
use super::types::{Mutation, NumericTarget};
use super::{components::*, resources::*, systems};
use crate::{authoring::EditorAuthoringSession, resources::SceneEditorState};
use bevy::prelude::*;
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputCommitOutcome, NumericInputCommitted,
    NumericInputCommitSource,
};
use feathrise_scene_content::{SceneTerrainObject, MeshLodMode};

pub(super) fn field_visible(field: Field, state: &TerrainObjectsEditorState) -> bool {
    match field {
        Field::Creation(_) | Field::CuboidCreation(_) => super::creation::field_visible(field, state),
        Field::Rotation(0..=2) => state.section_open(MeshPropertySection::Basic),
        Field::Rotation(_) => false,
        Field::Selected(index) if index < 7 => state.section_open(MeshPropertySection::Basic),
        Field::Selected(7) => state.section_open(MeshPropertySection::Type),
        Field::VoxelHeight => state.section_open(MeshPropertySection::Type),
        Field::Selected(_) => false,
        Field::Lod(display, index) => index >= 8 && state.section_open(TerrainObjectsEditorState::lod_section(display)),
        Field::Policy(index) => state.section_open(if index < 2 { MeshPropertySection::DisplayLods } else { MeshPropertySection::CollisionLods }),
    }
}

pub(super) fn field_value(
    field: Field,
    session: &EditorAuthoringSession,
    state: &TerrainObjectsEditorState,
) -> Option<f32> {
    match field {
        Field::Creation(index) => state.creation.get(index).copied(),
        Field::CuboidCreation(index) => state.cuboid_creation.get(index).copied(),
        Field::Rotation(axis) => systems::selected(session, state).and_then(|object| match axis {
            0 => Some(object.transform.pitch_degrees),
            1 => Some(object.transform.yaw_degrees),
            2 => Some(object.transform.roll_degrees),
            _ => None,
        }),
        Field::VoxelHeight => systems::selected(session, state).and_then(super::height::voxel_height),
        Field::Selected(index) if index < 8 => systems::selected(session, state).and_then(|object| {
            if (3..6).contains(&index) {
                // 通用网格属性显示变换倍率；作者height_scale是源编码范围，不能冒充实际几何高度。
                return Some(object.transform.scale[index - 3]);
            }
            super::jobs::field_value(
                object,
                index,
                true,
            )
        }),
        Field::Selected(_) => None,
        Field::Lod(display, index) => systems::selected(session, state)
            .filter(|object| index >= 8 && super::presentation::native_lods(object))
            .and_then(|object| super::jobs::field_value(object, index + state.lod(display).start * 2, display)),
        Field::Policy(index) => {
            systems::selected(session, state).and_then(|object| policy_value(object, index))
        }
    }
}

fn policy_value(object: &SceneTerrainObject, index: usize) -> Option<f32> {
    let policy = object.mesh_lod.as_ref()?;
    match index {
        0 | 1 => match &policy.display {
            MeshLodMode::Override(settings) => Some(if index == 0 {
                settings.max_error_pixels
            } else {
                settings.hysteresis_ratio
            }),
            _ => None,
        },
        2 | 3 => match &policy.collision {
            MeshLodMode::Override(settings) => Some(if index == 2 {
                settings.safe_near_meters
            } else {
                settings.max_error_meters
            }),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn target(
    field: Field,
    session: &EditorAuthoringSession,
    state: &TerrainObjectsEditorState,
    lod_row: Option<&super::types::LodRowTarget>,
) -> Option<NumericTarget> {
    if !field_visible(field, state) {
        return None;
    }
    let draft = session.draft.as_ref()?;
    let terrain = match field {
        Field::Creation(_) | Field::CuboidCreation(_) => None,
        Field::Selected(7) => return None, // 源格距只读；尺寸变换不触发未经批准的重采样。
        _ => {
            let object = systems::selected(session, state)?;
            if state.locked.contains(&object.id) {
                return None;
            }

            if matches!(field, Field::Policy(_))
                && !matches!(
                    object.terrain.kind,
                    feathrise_scene_content::SceneTerrainKind::Gltf
                )
            {
                return None;
            }
            Some(object.id.clone())
        }
    };
    field_value(field, session, state)?;
    let (index, display_lods) = match field {
        Field::Lod(display, index) => {
            let row = lod_row?;
            if row.scene_id != draft.scene_id || row.revision != session.revision
                || Some(&row.terrain) != terrain.as_ref() || row.display != display
                || row.index != state.lod(display).start.saturating_add((index - 8) / 2)
            {
                return None; // 只能编辑该行上次真正呈现的身份；滚动重绑和作者换代不能借新下标获权。
            }
            (8 + row.index * 2 + (index - 8) % 2, row.display)
        }
        _ => (field.index(), true),
    };
    Some(NumericTarget {
        purpose: field,
        scene_id: draft.scene_id.clone(),
        revision: session.revision,
        terrain,
        index,
        display_lods,
        creation_scope: field.is_creation().then_some((state.creation_source, state.placement_mode)),
    })
}

/// 此系统排在列表/页签输入前，按控件真正接管键鼠的边沿冻结旧显示身份。
pub(super) fn freeze(
    active: Option<Res<ActiveNumericInput>>,
    scrub: Option<Res<ActiveNumericInputScrub>>,
    session: Res<EditorAuthoringSession>,
    mut state: ResMut<TerrainObjectsEditorState>,
    mut fields: Query<(Entity, &mut NumericFieldBinding)>,
) {
    for (entity, mut binding) in &mut fields {
        let is_active = active
            .as_ref()
            .is_some_and(|active| active.entity == Some(entity))
            || scrub
                .as_ref()
                .is_some_and(|scrub| scrub.entity == Some(entity));
        if is_active && !binding.was_active {
            binding.editing = binding.current.clone();
            binding.invalidated = false;
            if let Some(target) = binding.current.as_ref()
                && matches!(target.purpose, Field::Lod(_, _))
                && target.revision == session.revision && target.terrain == state.selected
                && Some(&target.scene_id) == state.scene_id.as_ref()
            {
                state.lod_mut(target.display_lods).selected = (target.index - 8) / 2;
                state.display_lods = target.display_lods; // 聚焦只揭示这一组，不重定向另一组字段。
            }
        }
        if binding.was_active != is_active {
            binding.was_active = is_active;
        }
    }
}

pub(super) fn input(
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut state: ResMut<TerrainObjectsEditorState>,
    mut fields: Query<(&Field, &mut NumericFieldBinding, Option<&LodFieldRow>)>,
    lod_rows: Query<&LodRowBinding>,
    mut events: MessageReader<NumericInputCommitted>,
    gate: super::names::NamePanelGate,
    active: Option<Res<ActiveNumericInput>>,
    scrub: Option<Res<ActiveNumericInputScrub>>,
    navigation: (Option<Res<ButtonInput<MouseButton>>>,
        Query<(&UiInteraction, Option<&ActionButton>, Has<Row>, Has<RowAction>), Or<(With<ActionButton>, With<Row>)>>,
        Local<Option<NumericInputCommitted>>),
) {
    let (mouse, navigation_buttons, mut deferred) = navigation;
    let navigation_held = mouse.as_ref().is_some_and(|mouse| mouse.pressed(MouseButton::Left))
        && (deferred.is_some() || navigation_buttons.iter().any(|(interaction, action, row, row_action)| {
            *interaction == UiInteraction::Pressed && (row || row_action || action.is_some_and(|button| {
                button.0.creation_action() || matches!(button.0, Action::BasicProperties | Action::TypeProperties | Action::SourceProperties
                    | Action::StitchProperties | Action::DisplayLods | Action::CollisionLods
                    | Action::SelectLod(..) | Action::AddLod(_) | Action::RemoveLod(_)
                    | Action::ToggleCreation | Action::Rename | Action::Lock)
            }))
        }));
    // 公共文字控件在按下外部按钮时就失焦，但折叠/选层要等合法释放。
    // 唯一文字焦点只保留一条提交；每帧先复验身份，关闭/切换场景不留下可重放消息。
    let pending = deferred.take();
    for event in pending.into_iter().chain(events.read().copied()) {
        let Ok((field, mut binding, lod_row)) = fields.get_mut(event.entity) else {
            continue;
        };
        if active.as_ref().is_some_and(|active| active.entity == Some(event.entity))
            || scrub.as_ref().is_some_and(|scrub| scrub.entity == Some(event.entity))
        {
            continue; // 真正终结已释放公共焦点；旧消息不能在新的真实编辑期间抢先提交或取消它。
        }
        let bound = binding.editing.clone(); // 没有真实编辑边沿的消息不借当前显示身份获得写权限。
        if event.outcome != NumericInputCommitOutcome::Accepted
            || !event.edited
            || !gate.field_visible(*field, &state)
            || !systems::enabled(&editor, &session, &state)
        {
            binding.invalidated = true;
            continue;
        }
        if binding.invalidated {
            continue;
        }
        let Some(value) = event.value.as_f32().filter(|value| value.is_finite()) else {
            binding.invalidated = true;
            continue;
        };
        let row = lod_row.and_then(|row| lod_rows.get(row.0).ok()).and_then(|row| row.0.as_ref());
        let expected = target(*field, &session, &state, row);
        let layer_changed = lod_row.is_some() && bound.as_ref()
            .is_some_and(|bound| (bound.index - 8) / 2 != state.lod(bound.display_lods).selected);
        if bound.is_none() || bound != expected || layer_changed {
            binding.invalidated = true;
            continue; // 换场景、选择、用途、层号或作者修订后，旧控件消息只作废，不重新解码。
        }
        if navigation_held && event.source == NumericInputCommitSource::Text {
            *deferred = Some(event);
            continue; // 释放后先由导航改变作用域；移出取消则按原作用域正常失焦提交。
        }
        // 保留这一笔的身份墓碑。公共取消消息或多条迟到Accepted都不能借当前绑定重新获权。
        binding.invalidated = true;
        let bound = bound.expect("已核对数值完整身份");
        match *field {
            Field::Rotation(axis) => {
                let Some(object) = systems::selected(&session, &state) else { continue; };
                let mut transform = object.transform;
                match axis {
                    0 => transform.pitch_degrees = value.rem_euclid(360.0),
                    1 => transform.yaw_degrees = value.rem_euclid(360.0),
                    2 => transform.roll_degrees = value.rem_euclid(360.0),
                    _ => continue,
                }
                let id = object.id.clone();
                // 只改本轴但提交完整前后姿态，撤销、保存和重开沿用既有网格集合事务。
                systems::start(&mut session, &mut editor, &mut state, Mutation::Transform(id, transform));
            }
            Field::Creation(index) => {
                if let Err(error) = super::creation::set_field(&mut state.creation, index, value) {
                    editor.status = error;
                }
            }
            Field::CuboidCreation(index) => {
                if let Some(current) = state.cuboid_creation.get_mut(index) { *current = value; }
            }

            Field::VoxelHeight => {
                let Some(object) = systems::selected(&session, &state) else { continue; };
                let Some(old) = super::height::voxel_height(object).filter(|height| *height > 0.0) else { continue; };
                let mut transform = object.transform;
                transform.scale[1] *= value / old;
                let id = object.id.clone();
                systems::start(&mut session, &mut editor, &mut state, Mutation::Transform(id, transform));
            }
            Field::Selected(_) | Field::Lod(_, _) => {
                let Some(id) = bound.terrain else {
                    continue;
                };
                let mutation = if (3..6).contains(&bound.index) {
                    let Some(object) = systems::selected(&session, &state) else {
                        continue;
                    };
                    let mut transform = object.transform;
                    transform.scale[bound.index - 3] = value;
                    Mutation::Transform(id, transform)
                } else {
                    Mutation::Value(id, bound.index, value, bound.display_lods)
                };
                systems::start(&mut session, &mut editor, &mut state, mutation);
            }
            Field::Policy(index) => {
                let Some(object) = systems::selected(&session, &state) else {
                    continue;
                };
                let mut policy = object.mesh_lod.clone().unwrap_or_default();
                match (index, &mut policy.display, &mut policy.collision) {
                    (0, MeshLodMode::Override(settings), _) => settings.max_error_pixels = value,
                    (1, MeshLodMode::Override(settings), _) => settings.hysteresis_ratio = value,
                    (2, _, MeshLodMode::Override(settings)) => settings.safe_near_meters = value,
                    (3, _, MeshLodMode::Override(settings)) => settings.max_error_meters = value,
                    _ => continue,
                }
                let id = object.id.clone();
                systems::start(
                    &mut session,
                    &mut editor,
                    &mut state,
                    Mutation::LodPolicy(id, Some(policy)),
                );
            }
        }
    }
}
