//! 项目套装天空字段的公共数值输入，冻结套装身份并复用环境字段规格。

use bevy::prelude::*;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputChanged, NumericInputCommitOutcome,
    NumericInputCommitSource, NumericInputCommitted, NumericInputDisabled, NumericInputScrubConfig,
    NumericInputScrubSensitivityCommitted, NumericInputSetValue, NumericInputValue, NumericValue,
};
use feathrise_scene_content::WorldEnvironmentProfile;

use super::transaction::{authored_field_value, rollback_owned_authored_field};
use super::{EnvironmentNumericKind, GlobalEnvironmentNumericInput};
use crate::authoring::{
    EditorAuthoringSession, GlobalEnvironmentAuthoringState, GlobalEnvironmentSaveTask,
    environment_library::{commit_profile_change, profile_sky, set_profile_sky},
};
use crate::resources::SceneEditorState;

#[derive(Clone, PartialEq, Eq)]
struct LibrarySkyTarget {
    id: String,
    revision: u64,
    source_revision: u64,
}

impl LibrarySkyTarget {
    fn current(global: &GlobalEnvironmentAuthoringState) -> Option<Self> {
        Some(Self {
            id: global.selected_environment_id.clone()?,
            revision: global.revision,
            source_revision: global.source_revision,
        })
    }
}

struct LibrarySkyDrag {
    entity: Entity,
    target: LibrarySkyTarget,
    kind: EnvironmentNumericKind,
    before: WorldEnvironmentProfile,
    latest: f32,
    was_dirty: bool,
}

#[derive(Default)]
pub(crate) struct LibrarySkyInputState {
    focus: Option<(Entity, LibrarySkyTarget)>,
    blocked: Option<Entity>,
    drag: Option<LibrarySkyDrag>,
}

fn enabled(
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    save: &GlobalEnvironmentSaveTask,
) -> bool {
    editor.open
        && !editor.close_requested
        && !editor.new_scene_dialog_open
        && !session.saving
        && save.task.is_none()
}

fn editable_value(profile: &WorldEnvironmentProfile, kind: EnvironmentNumericKind) -> f32 {
    kind.displayed_value(Some(&profile_sky(profile)))
        .expect("套装天空字段必须包含完整值")
}

fn update(
    profile: &mut WorldEnvironmentProfile,
    kind: EnvironmentNumericKind,
    value: f32,
) -> Result<bool, String> {
    let mut environment = profile_sky(profile);
    let changed = kind.set_environment_value(&mut environment, value)?;
    if changed {
        set_profile_sky(profile, &environment);
    }
    Ok(changed)
}

fn rollback_drag(
    drag: LibrarySkyDrag,
    global: &mut GlobalEnvironmentAuthoringState,
    session: &EditorAuthoringSession,
    editor: &mut SceneEditorState,
) {
    let Some(profile) = global.draft.library.profiles.get(&drag.target.id) else {
        return;
    };
    let mut environment = profile_sky(profile);
    let Some(before) = authored_field_value(&profile_sky(&drag.before), drag.kind) else {
        return;
    };
    if !rollback_owned_authored_field(&mut environment, drag.kind, before, drag.latest) {
        return;
    }
    if let Some(profile) = global.draft.library.profiles_mut().get_mut(&drag.target.id) {
        set_profile_sky(profile, &environment);
    }
    if global.revision == drag.target.revision {
        global.dirty = drag.was_dirty;
    }
    editor.dirty = global.dirty || session.has_scene_changes();
}

pub(crate) fn handle_library_sky_inputs(
    mut changed: MessageReader<NumericInputChanged>,
    mut committed: MessageReader<NumericInputCommitted>,
    mut sensitivity: MessageReader<NumericInputScrubSensitivityCommitted>,
    (active, active_drag): (Res<ActiveNumericInput>, Res<ActiveNumericInputScrub>),
    inputs: Query<&GlobalEnvironmentNumericInput>,
    mut global: ResMut<GlobalEnvironmentAuthoringState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    save: Res<GlobalEnvironmentSaveTask>,
    mut settings: ResMut<EditorSettings>,
    mut state: Local<LibrarySkyInputState>,
    mut values: MessageWriter<NumericInputSetValue>,
) {
    let target = enabled(&editor, &session, &save)
        .then(|| LibrarySkyTarget::current(&global))
        .flatten();
    if state
        .drag
        .as_ref()
        .is_some_and(|drag| Some(&drag.target) != target.as_ref())
    {
        let drag = state.drag.take().expect("已确认存在套装拖动");
        state.blocked = Some(drag.entity);
        rollback_drag(drag, &mut global, &session, &mut editor);
    }
    if let Some((entity, focus)) = &state.focus
        && Some(focus) != target.as_ref()
    {
        let entity = *entity;
        state.focus = None;
        state.blocked = Some(entity);
        if let Ok(input) = inputs.get(entity) {
            let value = global
                .selected_environment()
                .map_or(0.0, |profile| editable_value(profile, input.0));
            values.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(value),
                cancel_edit: true,
            });
        }
    }

    for event in changed.read() {
        let (Ok(input), Some(value), Some(target)) = (
            inputs.get(event.entity),
            event.value.as_f32(),
            target.as_ref(),
        ) else {
            continue;
        };
        if state.blocked == Some(event.entity) {
            continue;
        }
        if state.drag.is_none() {
            let Some(before) = global.selected_environment().cloned() else {
                continue;
            };
            state.drag = Some(LibrarySkyDrag {
                entity: event.entity,
                target: target.clone(),
                kind: input.0,
                latest: authored_field_value(&profile_sky(&before), input.0)
                    .expect("套装天空字段必须包含完整值"),
                before,
                was_dirty: global.dirty,
            });
        }
        let drag = state.drag.as_mut().expect("套装拖动已冻结");
        if drag.entity != event.entity {
            continue;
        }
        let Some(profile) = global.draft.library.profiles_mut().get_mut(&target.id) else {
            continue;
        };
        if update(profile, input.0, value).is_ok_and(|changed| changed) {
            drag.latest = editable_value(profile, input.0);
            global.dirty = true; // 实景只接管当前草稿正在预览的套装值。
            editor.dirty = true;
        }
    }

    for event in committed.read() {
        let Ok(input) = inputs.get(event.entity) else {
            continue;
        };
        if state.blocked == Some(event.entity) {
            state.blocked = None;
            continue;
        }
        if event.source == NumericInputCommitSource::Scrub {
            if !state
                .drag
                .as_ref()
                .is_some_and(|drag| drag.entity == event.entity)
            {
                continue;
            }
            let drag = state.drag.take().expect("拖动提交来自同一控件");
            if event.outcome != NumericInputCommitOutcome::Accepted || !event.changed {
                rollback_drag(drag, &mut global, &session, &mut editor);
                continue;
            }
            let Some(after) = global.draft.library.profiles.get(&drag.target.id).cloned() else {
                continue;
            };
            if drag.before == after {
                global.dirty = drag.was_dirty;
                editor.dirty = global.dirty || session.has_scene_changes();
            } else if let Err(error) = commit_profile_change(
                drag.target.id,
                Some(drag.before),
                after,
                &mut global,
                &mut session,
                &mut editor,
            ) {
                editor.status = error;
            }
            continue;
        }
        let focus = state
            .focus
            .take()
            .filter(|(entity, _)| *entity == event.entity);
        if event.outcome != NumericInputCommitOutcome::Accepted || !event.edited {
            continue;
        }
        let Some(target) = target.as_ref().filter(|target| {
            target.revision == global.revision
                && focus.as_ref().is_none_or(|(_, focus)| focus == *target)
        }) else {
            continue;
        };
        let (Some(value), Some(before)) =
            (event.value.as_f32(), global.selected_environment().cloned())
        else {
            continue;
        };
        let mut after = before.clone();
        if let Err(error) = update(&mut after, input.0, value).and_then(|_| {
            commit_profile_change(
                target.id.clone(),
                Some(before),
                after,
                &mut global,
                &mut session,
                &mut editor,
            )
        }) {
            editor.status = error;
        }
    }
    if state
        .drag
        .as_ref()
        .is_some_and(|drag| Some(drag.entity) != active_drag.entity)
    {
        let drag = state.drag.take().expect("未收到松手消息的拖动需回滚");
        rollback_drag(drag, &mut global, &session, &mut editor);
    }
    if let (Some(entity), Some(target)) = (active.entity, target)
        && inputs.contains(entity)
        && state.blocked != Some(entity)
        && target.revision == global.revision
    {
        state.focus.get_or_insert((entity, target));
    }
    if let Some(entity) = state.blocked
        && active.entity != Some(entity)
        && active_drag.entity != Some(entity)
    {
        state.blocked = None;
    }

    for event in sensitivity.read() {
        if let Ok(input) = inputs.get(event.entity) {
            let field = input.0.sensitivity_field();
            let value = event
                .after
                .clamp(super::SCRUB_SENSITIVITY_MIN, super::SCRUB_SENSITIVITY_MAX);
            if value != settings.scene_editor_scrub_sensitivities.get(field) {
                settings.scene_editor_scrub_sensitivities.set(field, value);
            }
        }
    }
}

pub(crate) fn sync_library_sky_inputs(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    save: Res<GlobalEnvironmentSaveTask>,
    global: Res<GlobalEnvironmentAuthoringState>,
    settings: Res<EditorSettings>,
    (active, drag): (Res<ActiveNumericInput>, Res<ActiveNumericInputScrub>),
    mut inputs: Query<(
        Entity,
        &GlobalEnvironmentNumericInput,
        &NumericInputValue,
        Has<NumericInputDisabled>,
        &mut NumericInputScrubConfig,
    )>,
    mut values: MessageWriter<NumericInputSetValue>,
) {
    let profile = global.selected_environment();
    let enable = profile.is_some() && enabled(&editor, &session, &save);
    for (entity, marker, current, disabled, mut scrub) in &mut inputs {
        if enable == disabled {
            if enable {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            }
        }
        let desired = profile.map_or(0.0, |profile| editable_value(profile, marker.0));
        if active.entity != Some(entity)
            && drag.entity != Some(entity)
            && current.0.as_f32() != Some(desired)
        {
            values.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(desired),
                cancel_edit: true,
            });
        }
        if drag.entity != Some(entity) {
            let sensitivity = marker.0.sensitivity(&settings);
            if scrub.sensitivity != sensitivity {
                scrub.sensitivity = sensitivity;
            }
        }
    }
}
