//! 天体光照精确输入、左拖事务、场景继承切换与折叠交互。

use bevy::prelude::*;
use feathrise_editor_ui::{
    ActiveNumericInput, NumericInputChanged, NumericInputCommitOutcome, NumericInputCommitSource,
    NumericInputCommitted, NumericInputScrubSensitivityChanged,
    NumericInputScrubSensitivityCommitted, NumericInputSetValue, NumericValue, EditorButtonClick,
};
use feathrise_scene_content::{CelestialLightingProfile, SceneEnvironmentDescriptor, resolve};
use feathrise_editor_core::EditorSettings;

use super::contract::{
    CelestialContextStamp, CelestialNumericFocus, CelestialNumericInput,
    CelestialNumericInputState, CelestialNumericKey, CelestialNumericScope, CelestialNumericScrub,
    CelestialOverrideToggle, CelestialScrubBefore, CelestialSectionState, CelestialSectionToggle,
};
use crate::authoring::environment::commit_environment_change;
use crate::authoring::{
    EditorAuthoringSession, EditorCommand, GlobalEnvironmentAuthoringState,
    GlobalEnvironmentSaveTask,
};
use crate::resources::SceneEditorState;

fn input_key(
    inputs: &Query<(Entity, &CelestialNumericInput)>,
    entity: Entity,
) -> Option<CelestialNumericKey> {
    inputs.get(entity).ok().map(|(_, marker)| marker.0)
}

fn dynamic_environment(session: &EditorAuthoringSession) -> Option<&SceneEnvironmentDescriptor> {
    session
        .draft
        .as_ref()
        .filter(|draft| draft.manifest.is_data_driven())
        .map(|draft| &draft.manifest.environment)
}

fn current_stamp(
    key: CelestialNumericKey,
    session: &EditorAuthoringSession,
    global: &GlobalEnvironmentAuthoringState,
) -> Option<CelestialContextStamp> {
    match key.scope {
        CelestialNumericScope::Global => Some(CelestialContextStamp {
            global_revision: global.revision,
            scene_id: None,
            scene_revision: None,
        }),
        CelestialNumericScope::Scene => {
            let draft = session
                .draft
                .as_ref()
                .filter(|draft| draft.manifest.is_data_driven())?;
            key.scene_editable(&draft.manifest.environment)
                .then(|| CelestialContextStamp {
                    global_revision: global.revision,
                    scene_id: Some(draft.scene_id.clone()),
                    scene_revision: Some(session.revision),
                })
        }
    }
}

fn current_value(
    key: CelestialNumericKey,
    session: &EditorAuthoringSession,
    global: &GlobalEnvironmentAuthoringState,
) -> Option<f32> {
    key.displayed_value(
        &global.draft.celestial_lighting,
        dynamic_environment(session),
    )
}

fn reset_input(
    entity: Entity,
    key: CelestialNumericKey,
    session: &EditorAuthoringSession,
    global: &GlobalEnvironmentAuthoringState,
    set_values: &mut MessageWriter<NumericInputSetValue>,
) {
    set_values.write(NumericInputSetValue {
        entity,
        value: NumericValue::F32(current_value(key, session, global).unwrap_or(0.0)),
        cancel_edit: true,
    });
}

fn edited_scene_environment(
    before: &SceneEnvironmentDescriptor,
    global: &CelestialLightingProfile,
    key: CelestialNumericKey,
    value: f32,
) -> Result<SceneEnvironmentDescriptor, String> {
    if key.scope != CelestialNumericScope::Scene
        || !key
            .field
            .property()
            .is_overridden(&before.celestial_overrides)
    {
        return Err(format!("{}当前正在跟随全局", key.field.property().label()));
    }
    let mut after = before.clone();
    let mut effective = resolve(global, &after.celestial_overrides);
    key.field.write(&mut effective, value)?;
    key.field
        .property()
        .set_overridden(&mut after.celestial_overrides, &effective, true);
    after.validate()?;
    Ok(after)
}

fn commit_global_change(
    before: CelestialLightingProfile,
    after: CelestialLightingProfile,
    global: &mut GlobalEnvironmentAuthoringState,
    session: &mut EditorAuthoringSession,
    editor: &mut SceneEditorState,
    status: String,
) {
    if before == after {
        editor.status = "全局天体光照参数没有变化".to_owned();
        return;
    }
    global.draft.celestial_lighting = after.clone();
    session
        .undo
        .push(EditorCommand::GlobalCelestialLightingChanged {
            sequence: global.revision.wrapping_add(1),
            before,
            after,
        });
    session.redo.clear();
    global.mark_changed(session, editor);
    global.dirty = global.draft != global.saved;
    editor.dirty = global.dirty || session.has_scene_changes();
    editor.status = status;
}

fn apply_exact(
    entity: Entity,
    key: CelestialNumericKey,
    value: f32,
    stamp: CelestialContextStamp,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    global: &mut GlobalEnvironmentAuthoringState,
    set_values: &mut MessageWriter<NumericInputSetValue>,
) {
    if current_stamp(key, session, global).as_ref() != Some(&stamp) {
        editor.status = format!("{}提交因作者上下文变化而取消", key.field.label());
        reset_input(entity, key, session, global, set_values);
        return;
    }
    match key.scope {
        CelestialNumericScope::Global => {
            let before = global.draft.celestial_lighting.clone();
            let mut after = before.clone();
            if let Err(error) = key.field.write(&mut after, value) {
                editor.status = format!("{}未采用：{error}", key.field.label());
                reset_input(entity, key, session, global, set_values);
                return;
            }
            let actual = key.field.read(&after);
            commit_global_change(
                before,
                after,
                global,
                session,
                editor,
                format!(
                    "全局{}已设为 {:.3}{}",
                    key.field.label(),
                    actual,
                    key.field.unit()
                ),
            );
        }
        CelestialNumericScope::Scene => {
            let Some(before) = dynamic_environment(session).cloned() else {
                editor.status = "当前动态场景环境草稿尚未就绪".to_owned();
                reset_input(entity, key, session, global, set_values);
                return;
            };
            let after =
                match edited_scene_environment(&before, &global.draft.celestial_lighting, key, value)
                {
                    Ok(after) => after,
                    Err(error) => {
                        editor.status = format!("场景{}未采用：{error}", key.field.label());
                        reset_input(entity, key, session, global, set_values);
                        return;
                    }
                };
            let actual = key
                .displayed_value(&global.draft.celestial_lighting, Some(&after))
                .unwrap_or(value);
            commit_environment_change(
                session,
                editor,
                before,
                after,
                format!(
                    "场景覆盖{}已设为 {:.3}{}",
                    key.field.label(),
                    actual,
                    key.field.unit()
                ),
            );
        }
    }
}

pub(super) fn rollback_owned_scrub(
    transaction: &CelestialNumericScrub,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    global: &mut GlobalEnvironmentAuthoringState,
) -> bool {
    match &transaction.before {
        CelestialScrubBefore::Global { dirty, .. } => {
            if transaction.key.field.read(&global.draft.celestial_lighting)
                != transaction.latest_value
            {
                return false;
            }
            if transaction
                .key
                .field
                .write(
                    &mut global.draft.celestial_lighting,
                    transaction.initial_value,
                )
                .is_err()
            {
                return false;
            }
            global.dirty = if global.revision == transaction.stamp.global_revision {
                *dirty
            } else {
                global.draft != global.saved
            };
            editor.dirty = global.dirty || session.has_scene_changes();
            true
        }
        CelestialScrubBefore::Scene { scene_id, .. } => {
            let Some(draft) = session
                .draft
                .as_mut()
                .filter(|draft| &draft.scene_id == scene_id)
            else {
                return false;
            };
            let profile = resolve(
                &global.draft.celestial_lighting,
                &draft.manifest.environment.celestial_overrides,
            );
            if transaction.key.field.read(&profile) != transaction.latest_value {
                return false;
            }
            let Ok(after) = edited_scene_environment(
                &draft.manifest.environment,
                &global.draft.celestial_lighting,
                transaction.key,
                transaction.initial_value,
            ) else {
                return false;
            };
            draft.manifest.environment = after;
            true
        }
    }
}

fn begin_scrub(
    entity: Entity,
    key: CelestialNumericKey,
    editor: &mut SceneEditorState,
    session: &EditorAuthoringSession,
    global: &GlobalEnvironmentAuthoringState,
) -> Option<CelestialNumericScrub> {
    let stamp = current_stamp(key, session, global)?;
    let initial_value = current_value(key, session, global)?;
    let before = match key.scope {
        CelestialNumericScope::Global => CelestialScrubBefore::Global {
            profile: global.draft.celestial_lighting.clone(),
            dirty: global.dirty,
        },
        CelestialNumericScope::Scene => {
            let draft = session.draft.as_ref()?;
            CelestialScrubBefore::Scene {
                scene_id: draft.scene_id.clone(),
                environment: draft.manifest.environment.clone(),
            }
        }
    };
    editor.status = format!("正在调整{}；松手后生成一次撤销", key.field.label());
    Some(CelestialNumericScrub {
        entity,
        key,
        stamp,
        before,
        initial_value,
        latest_value: initial_value,
        aborted: false,
    })
}

fn apply_scrub_changed(
    entity: Entity,
    key: CelestialNumericKey,
    value: f32,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    global: &mut GlobalEnvironmentAuthoringState,
    state: &mut CelestialNumericInputState,
) {
    if state.scrub.is_none() {
        state.scrub = begin_scrub(entity, key, editor, session, global);
    }
    let Some(transaction) = state.scrub.as_mut() else {
        editor.status = format!("当前没有可调整的{}", key.field.label());
        return;
    };
    if transaction.aborted {
        return;
    }
    if transaction.entity != entity
        || transaction.key != key
        || current_stamp(key, session, global).as_ref() != Some(&transaction.stamp)
    {
        rollback_owned_scrub(transaction, editor, session, global);
        transaction.aborted = true;
        editor.status = format!("{}拖动因作者上下文变化而取消", key.field.label());
        return;
    }
    let result = match key.scope {
        CelestialNumericScope::Global => {
            key.field.write(&mut global.draft.celestial_lighting, value)
        }
        CelestialNumericScope::Scene => {
            let Some(draft) = session.draft.as_mut() else {
                transaction.aborted = true;
                return;
            };
            match edited_scene_environment(
                &draft.manifest.environment,
                &global.draft.celestial_lighting,
                key,
                value,
            ) {
                Ok(after) => {
                    draft.manifest.environment = after;
                    Ok(())
                }
                Err(error) => Err(error),
            }
        }
    };
    if let Err(error) = result {
        rollback_owned_scrub(transaction, editor, session, global);
        transaction.aborted = true;
        editor.status = format!("{}拖动未采用：{error}", key.field.label());
        return;
    }
    transaction.latest_value = current_value(key, session, global).unwrap_or(value);
    editor.status = format!(
        "正在调整{}：{:.3}{}；松手后生成一次撤销",
        key.field.label(),
        transaction.latest_value,
        key.field.unit()
    );
}

fn finish_scrub(
    entity: Entity,
    key: CelestialNumericKey,
    committed_changed: bool,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    global: &mut GlobalEnvironmentAuthoringState,
    state: &mut CelestialNumericInputState,
    set_values: &mut MessageWriter<NumericInputSetValue>,
) {
    let Some(transaction) = state.scrub.take() else {
        return;
    };
    let continuous = transaction.entity == entity
        && transaction.key == key
        && !transaction.aborted
        && current_stamp(key, session, global).as_ref() == Some(&transaction.stamp);
    if !committed_changed || !continuous {
        rollback_owned_scrub(&transaction, editor, session, global);
        editor.status = if continuous {
            format!("{}拖动已回到起点，没有产生撤销", key.field.label())
        } else {
            format!("{}拖动身份已失效，没有产生撤销", key.field.label())
        };
        reset_input(entity, key, session, global, set_values);
        return;
    }
    match transaction.before {
        CelestialScrubBefore::Global {
            profile: before, ..
        } => {
            let after = global.draft.celestial_lighting.clone();
            if before == after {
                editor.status = format!("{}拖动没有净变化", key.field.label());
                return;
            }
            commit_global_change(
                before,
                after,
                global,
                session,
                editor,
                format!(
                    "全局{}已调整为 {:.3}{}",
                    key.field.label(),
                    transaction.latest_value,
                    key.field.unit()
                ),
            );
        }
        CelestialScrubBefore::Scene {
            scene_id,
            environment: before,
        } => {
            let Some(after) = session
                .draft
                .as_ref()
                .filter(|draft| draft.scene_id == scene_id)
                .map(|draft| draft.manifest.environment.clone())
            else {
                editor.status = "场景光照拖动结束时草稿已经失效".to_owned();
                return;
            };
            if before == after {
                editor.status = format!("{}拖动没有净变化", key.field.label());
                return;
            }
            commit_environment_change(
                session,
                editor,
                before,
                after,
                format!(
                    "场景覆盖{}已调整为 {:.3}{}",
                    key.field.label(),
                    transaction.latest_value,
                    key.field.unit()
                ),
            );
        }
    }
}

/// 消费二十个天体光照字段的公共数值消息。
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_celestial_numeric_inputs(
    mut changed: MessageReader<NumericInputChanged>,
    mut committed: MessageReader<NumericInputCommitted>,
    mut sensitivity_changed: MessageReader<NumericInputScrubSensitivityChanged>,
    mut sensitivity_committed: MessageReader<NumericInputScrubSensitivityCommitted>,
    active: Res<ActiveNumericInput>,
    inputs: Query<(Entity, &CelestialNumericInput)>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut global: ResMut<GlobalEnvironmentAuthoringState>,
    global_save: Res<GlobalEnvironmentSaveTask>,
    mut settings: ResMut<EditorSettings>,
    mut state: ResMut<CelestialNumericInputState>,
    mut set_values: MessageWriter<NumericInputSetValue>,
) {
    if let Some(entity) = active.entity
        && let Some(key) = input_key(&inputs, entity)
    {
        let stamp = current_stamp(key, &session, &global);
        let unavailable = !editor.open
            || editor.close_requested
            || editor.new_scene_dialog_open
            || session.saving
            || global_save.task.is_some();
        let stale = state.focus.as_ref().is_some_and(|focus| {
            focus.entity != entity || focus.key != key || stamp.as_ref() != Some(&focus.stamp)
        });
        if unavailable || stale || stamp.is_none() {
            reset_input(entity, key, &session, &global, &mut set_values);
            state.focus = None;
        } else if state.focus.is_none() {
            state.focus = stamp.map(|stamp| CelestialNumericFocus { entity, key, stamp });
            editor.status = format!("正在精确输入{}；Enter 或点击空白提交", key.field.label());
        }
    }

    for event in changed.read() {
        let Some(key) = input_key(&inputs, event.entity) else {
            continue;
        };
        let Some(value) = event.value.as_f32() else {
            continue;
        };
        apply_scrub_changed(
            event.entity,
            key,
            value,
            &mut editor,
            &mut session,
            &mut global,
            &mut state,
        );
    }

    for event in committed.read() {
        let Some(key) = input_key(&inputs, event.entity) else {
            continue;
        };
        if event.source == NumericInputCommitSource::Scrub {
            finish_scrub(
                event.entity,
                key,
                event.changed,
                &mut editor,
                &mut session,
                &mut global,
                &mut state,
                &mut set_values,
            );
            continue;
        }
        let focus = state
            .focus
            .take()
            .filter(|focus| focus.entity == event.entity && focus.key == key);
        match event.outcome {
            NumericInputCommitOutcome::Canceled => {
                editor.status = format!("已取消{}精确输入", key.field.label());
                continue;
            }
            NumericInputCommitOutcome::Rejected => {
                editor.status = format!("{}不是有效数字，已恢复原值", key.field.label());
                continue;
            }
            NumericInputCommitOutcome::Accepted if !event.edited => continue,
            NumericInputCommitOutcome::Accepted => {}
        }
        let Some(value) = event.value.as_f32() else {
            reset_input(event.entity, key, &session, &global, &mut set_values);
            continue;
        };
        let Some(stamp) = focus
            .map(|focus| focus.stamp)
            .or_else(|| current_stamp(key, &session, &global))
        else {
            editor.status = format!("{}提交时没有可编辑目标", key.field.label());
            reset_input(event.entity, key, &session, &global, &mut set_values);
            continue;
        };
        apply_exact(
            event.entity,
            key,
            value,
            stamp,
            &mut editor,
            &mut session,
            &mut global,
            &mut set_values,
        );
    }

    for event in sensitivity_changed.read() {
        let Some(key) = input_key(&inputs, event.entity) else {
            continue;
        };
        editor.status = format!(
            "正在调整{}拖动灵敏度：×{:.3}",
            key.field.label(),
            event.value
        );
    }
    for event in sensitivity_committed.read() {
        let Some(key) = input_key(&inputs, event.entity) else {
            continue;
        };
        key.field.set_sensitivity(&mut settings, event.after);
        editor.status = format!("{}拖动灵敏度已设为 ×{:.3}", key.field.label(), event.after);
    }
}

/// 六个场景属性在“跟随全局/场景覆盖”之间切换，并作为一次完整环境操作进入历史。
pub(crate) fn handle_celestial_override_toggles(
    buttons: Query<(&EditorButtonClick, &CelestialOverrideToggle), Changed<EditorButtonClick>>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    global: Res<GlobalEnvironmentAuthoringState>,
) {
    for (interaction, toggle) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        if session.saving {
            editor.status = "场景保存期间不能切换光照覆盖".to_owned();
            continue;
        }
        let Some(before) = dynamic_environment(&session).cloned() else {
            editor.status = "当前固定场景不迁移天体光照覆盖".to_owned();
            continue;
        };
        let effective = resolve(
            &global.draft.celestial_lighting,
            &before.celestial_overrides,
        );
        let was_overridden = toggle.0.is_overridden(&before.celestial_overrides);
        let mut after = before.clone();
        toggle
            .0
            .set_overridden(&mut after.celestial_overrides, &effective, !was_overridden);
        if let Err(error) = after.validate() {
            editor.status = format!("{}覆盖切换失败：{error}", toggle.0.label());
            continue;
        }
        commit_environment_change(
            &mut session,
            &mut editor,
            before,
            after,
            if was_overridden {
                format!("{}已恢复跟随全局", toggle.0.label())
            } else {
                format!("{}已启用场景覆盖；已复制当前有效值", toggle.0.label())
            },
        );
    }
}

/// 展开或收起全局与场景两个纵向区域；该状态不属于场景数据。
pub(crate) fn handle_celestial_section_toggles(
    buttons: Query<(&EditorButtonClick, &CelestialSectionToggle), Changed<EditorButtonClick>>,
    mut state: ResMut<CelestialSectionState>,
) {
    for (interaction, toggle) in &buttons {
        if interaction.is_clicked() {
            state.toggle(toggle.0);
        }
    }
}
