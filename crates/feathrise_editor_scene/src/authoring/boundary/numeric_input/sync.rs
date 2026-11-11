//! 三个墙体字段共用选择缓存与UI同步；稳定帧只按已核验索引读取当前墙。
use super::{
    BoundaryThicknessInput, BoundaryThicknessInputState, cached_target, input_contract, rollback,
};
use crate::authoring::resources::EditorAuthoringSession;
use crate::resources::SceneEditorState;
use bevy::prelude::*;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputDisabled, NumericInputScrubConfig,
    NumericInputSetValue, NumericInputValue, NumericValue,
};
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_boundary_thickness_input(
    mut commands: Commands,
    editor: Res<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    settings: Res<EditorSettings>,
    active: Res<ActiveNumericInput>,
    active_scrub: Res<ActiveNumericInputScrub>,
    mut state: ResMut<BoundaryThicknessInputState>,
    mut inputs: Query<(
        Entity,
        &BoundaryThicknessInput,
        &NumericInputValue,
        Has<NumericInputDisabled>,
        &mut NumericInputScrubConfig,
    )>,
    mut values: MessageWriter<NumericInputSetValue>,
) {
    let enabled =
        editor.open && !editor.close_requested && !editor.new_scene_dialog_open && !session.saving;
    if state
        .scrub
        .as_ref()
        .is_some_and(|scrub| !enabled || active_scrub.entity != Some(scrub.entity))
    {
        if let Some(scrub) = state.scrub.take() {
            rollback(&mut session, &scrub);
        } // 丢失松手消息也不能遗留无历史实时值。
    }
    let target = cached_target(&session, &mut state);
    for (entity, field, current, disabled, mut scrub) in &mut inputs {
        let value = target
            .as_ref()
            .and_then(|(_, index)| {
                session
                    .draft
                    .as_ref()?
                    .boundaries
                    .as_ref()?
                    .paths
                    .get(*index)
            })
            .map(|path| field.0.get(path));
        let field_enabled = enabled && value.is_some();
        if field_enabled && disabled {
            commands.entity(entity).remove::<NumericInputDisabled>();
        } else if !field_enabled && !disabled {
            commands.entity(entity).insert(NumericInputDisabled);
        }
        let desired = value.unwrap_or(0.0);
        if active.entity != Some(entity)
            && active_scrub.entity != Some(entity)
            && current.0.as_f32() != Some(desired)
        {
            values.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(desired),
                cancel_edit: true,
            });
        }
        let (_, desired_scrub) = input_contract(&settings, field.0);
        if active_scrub.entity != Some(entity) && scrub.sensitivity != desired_scrub.sensitivity {
            scrub.sensitivity = desired_scrub.sensitivity;
        }
    }
}
