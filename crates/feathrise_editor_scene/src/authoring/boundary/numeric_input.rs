//! 空气墙厚度、底部高度和顶部高度共用专业数值输入及单字段所有权事务。
mod contract;
mod scene;
mod sync;
#[cfg(test)]
mod tests;
use super::path_history::commit_path_change;
use crate::authoring::resources::EditorAuthoringSession;
use crate::resources::SceneEditorState;
use bevy::prelude::*;
#[cfg(test)]
use feathrise_scene_content::SceneBoundariesAsset;
use feathrise_scene_content::{SceneBoundaryPath, SceneId};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::SceneEditorScrubField;
use feathrise_editor_ui::{
    ActiveNumericInput, NumericInputChanged, NumericInputCommitOutcome, NumericInputCommitSource,
    NumericInputCommitted, NumericInputScrubSensitivityCommitted, NumericInputSetValue,
    NumericValue,
};
use contract::input_contract;
pub(crate) use scene::setup_boundary_thickness_input;
pub(crate) use sync::sync_boundary_thickness_input;

const SCRUB_SENSITIVITY_MIN: f32 = 0.01;
const SCRUB_SENSITIVITY_MAX: f32 = 100.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BoundaryField {
    #[default]
    Thickness,
    Bottom,
    Top,
}
impl BoundaryField {
    fn label(self) -> &'static str {
        match self {
            Self::Thickness => "墙厚",
            Self::Bottom => "底部高度",
            Self::Top => "顶部高度",
        }
    }
    fn range(self) -> (f32, f32) {
        match self {
            Self::Thickness => (0.25, 32.0),
            _ => (f32::MIN, f32::MAX),
        }
    }
    fn get(self, path: &SceneBoundaryPath) -> f32 {
        match self {
            Self::Thickness => path.thickness,
            Self::Bottom => path.bottom_y,
            Self::Top => path.top_y,
        }
    }
    fn write(self, path: &mut SceneBoundaryPath, value: f32) {
        match self {
            Self::Thickness => path.thickness = value,
            Self::Bottom => path.bottom_y = value,
            Self::Top => path.top_y = value,
        }
    }
}
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct BoundaryThicknessInputSlot;
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct BoundaryThicknessInput(pub BoundaryField);
#[derive(Clone, Debug, PartialEq, Eq)]
struct BoundaryThicknessTarget {
    scene_id: SceneId,
    path_id: String,
    authoring_revision: u64,
}
#[derive(Debug)]
struct BoundaryScrub {
    entity: Entity,
    target: BoundaryThicknessTarget,
    index: usize,
    field: BoundaryField,
    before: SceneBoundaryPath,
    latest: f32,
    aborted: bool,
}
#[derive(Resource, Default)]
pub(crate) struct BoundaryThicknessInputState {
    focus: Option<(Entity, BoundaryThicknessTarget)>,
    scrub: Option<BoundaryScrub>,
    cached: Option<(BoundaryThicknessTarget, usize)>, // 保存选择ID+修订，稳态不扫描全列表。
}

impl BoundaryThicknessInputState {
    /// 保存捕获必须等待墙体数值实时事务完整收口。
    pub(crate) fn busy(&self) -> bool {
        self.scrub.is_some() || self.focus.is_some()
    }
}
fn selected_target(session: &EditorAuthoringSession) -> Option<BoundaryThicknessTarget> {
    let draft = session.draft.as_ref()?;
    let path = draft
        .boundaries
        .as_ref()?
        .paths
        .get(super::helpers::selected_path_index(session)?)?;
    Some(BoundaryThicknessTarget {
        scene_id: draft.scene_id.clone(),
        path_id: path.id.clone(),
        authoring_revision: session.revision,
    })
}
fn cached_target(
    session: &EditorAuthoringSession,
    state: &mut BoundaryThicknessInputState,
) -> Option<(BoundaryThicknessTarget, usize)> {
    let draft = session.draft.as_ref()?;
    if let Some((target, index)) = &state.cached {
        let selected_matches = session
            .selected_boundary_path
            .as_ref()
            .filter(|(map, _)| *map == draft.scene_id)
            .map_or(false, |(_, id)| *id == target.path_id);
        if target.scene_id == draft.scene_id
            && target.authoring_revision == session.revision
            && selected_matches
            && draft
                .boundaries
                .as_ref()?
                .paths
                .get(*index)
                .is_some_and(|path| path.id == target.path_id)
        {
            return Some((target.clone(), *index));
        }
    }
    let target = selected_target(session)?;
    let index = super::helpers::selected_path_index(session)?;
    state.cached = Some((target.clone(), index));
    Some((target, index))
}
#[cfg(test)]
fn set_target_value(
    document: &mut SceneBoundariesAsset,
    target: &BoundaryThicknessTarget,
    value: f32,
) -> Result<bool, String> {
    let path = document
        .paths
        .iter_mut()
        .find(|path| path.id == target.path_id)
        .ok_or("空气墙已删除")?;
    set_field(path, BoundaryField::Thickness, value)
}
fn set_field(path: &mut SceneBoundaryPath, field: BoundaryField, value: f32) -> Result<bool, String> {
    let (min, max) = field.range();
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(format!("{}必须在{min}至{max}米之间", field.label()));
    }
    if (field == BoundaryField::Bottom && value >= path.top_y)
        || (field == BoundaryField::Top && value <= path.bottom_y)
    {
        return Err("顶部高度必须高于底部高度".into());
    }
    let span = match field {
        BoundaryField::Bottom => path.top_y - value,
        BoundaryField::Top => value - path.bottom_y,
        BoundaryField::Thickness => path.top_y - path.bottom_y,
    }; // 两端有限还需要高度差有限，避免物理盒体溢出。
    if !span.is_finite() {
        return Err("空气墙高度跨度过大".into());
    }
    if field.get(path) == value {
        return Ok(false);
    }
    field.write(path, value);
    Ok(true)
}
fn rollback(session: &mut EditorAuthoringSession, scrub: &BoundaryScrub) {
    let Some(draft) = session
        .draft
        .as_mut()
        .filter(|draft| draft.scene_id == scrub.target.scene_id)
    else {
        return;
    };
    let Some(path) = draft
        .boundaries
        .as_mut()
        .and_then(|doc| doc.paths.get_mut(scrub.index))
        .filter(|path| path.id == scrub.target.path_id)
    else {
        return;
    };
    if scrub.field.get(path) == scrub.latest {
        scrub.field.write(path, scrub.field.get(&scrub.before));
    } // 只恢复事务仍拥有的字段，保留外部修改。
}
fn reset(
    entity: Entity,
    field: BoundaryField,
    session: &EditorAuthoringSession,
    state: &mut BoundaryThicknessInputState,
    values: &mut MessageWriter<NumericInputSetValue>,
) {
    let value = cached_target(session, state)
        .and_then(|(_, index)| {
            session
                .draft
                .as_ref()?
                .boundaries
                .as_ref()?
                .paths
                .get(index)
        })
        .map(|path| field.get(path))
        .unwrap_or(0.0);
    values.write(NumericInputSetValue {
        entity,
        value: NumericValue::F32(value),
        cancel_edit: true,
    });
}
#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_boundary_thickness_input(
    mut changed: MessageReader<NumericInputChanged>,
    mut committed: MessageReader<NumericInputCommitted>,
    mut sensitivities: MessageReader<NumericInputScrubSensitivityCommitted>,
    active: Res<ActiveNumericInput>,
    inputs: Query<&BoundaryThicknessInput>,
    mut editor: ResMut<SceneEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut settings: ResMut<EditorSettings>,
    mut state: ResMut<BoundaryThicknessInputState>,
    mut values: MessageWriter<NumericInputSetValue>,
) {
    let enabled =
        editor.open && !editor.close_requested && !editor.new_scene_dialog_open && !session.saving;
    if let Some(entity) = active.entity.filter(|entity| inputs.contains(*entity)) {
        if state.focus.as_ref().is_none_or(|(root, _)| *root != entity) {
            state.focus = cached_target(&session, &mut state).map(|(target, _)| (entity, target));
        }
    }
    for event in changed.read() {
        let Ok(input) = inputs.get(event.entity) else {
            continue;
        };
        if !enabled {
            continue;
        }
        let Some(value) = event.value.as_f32() else {
            continue;
        };
        if state.scrub.is_none() {
            let Some((target, index)) = cached_target(&session, &mut state) else {
                continue;
            };
            let before = session
                .draft
                .as_ref()
                .unwrap()
                .boundaries
                .as_ref()
                .unwrap()
                .paths[index]
                .clone(); // 只冻结一道墙。
            state.scrub = Some(BoundaryScrub {
                entity: event.entity,
                target,
                index,
                field: input.0,
                latest: input.0.get(&before),
                before,
                aborted: false,
            });
        }
        let scrub = state.scrub.as_mut().unwrap();
        if scrub.aborted {
            continue;
        }
        if scrub.entity != event.entity || scrub.target.authoring_revision != session.revision {
            rollback(&mut session, scrub);
            scrub.aborted = true;
            continue;
        }
        let Some(path) = session
            .draft
            .as_mut()
            .filter(|draft| draft.scene_id == scrub.target.scene_id)
            .and_then(|draft| draft.boundaries.as_mut())
            .and_then(|doc| doc.paths.get_mut(scrub.index))
            .filter(|path| path.id == scrub.target.path_id)
        else {
            scrub.aborted = true;
            continue;
        };
        match set_field(path, scrub.field, value) {
            Ok(true) => {
                scrub.latest = value;
                editor.status = format!("{}：{value:.2}米；松手完成", scrub.field.label());
            }
            Ok(false) => {}
            Err(error) => {
                editor.status = error;
            }
        }
    }
    for event in committed.read() {
        let Ok(input) = inputs.get(event.entity) else {
            continue;
        };
        if event.source == NumericInputCommitSource::Scrub {
            let Some(scrub) = state.scrub.take() else {
                continue;
            };
            if !enabled
                || scrub.aborted
                || !event.changed
                || scrub.entity != event.entity
                || scrub.target.authoring_revision != session.revision
            {
                rollback(&mut session, &scrub);
                reset(event.entity, input.0, &session, &mut state, &mut values);
                continue;
            }
            let after = session
                .draft
                .as_ref()
                .filter(|draft| draft.scene_id == scrub.target.scene_id)
                .and_then(|draft| draft.boundaries.as_ref())
                .and_then(|doc| doc.paths.get(scrub.index))
                .filter(|path| path.id == scrub.target.path_id)
                .cloned();
            if let Some(after) = after {
                if let Err(error) = commit_path_change(
                    &mut session,
                    &mut editor,
                    scrub.index,
                    Some(scrub.before),
                    Some(after),
                    format!("{}已调整", input.0.label()),
                ) {
                    editor.status = error;
                }
            }
            continue;
        }
        let target = state
            .focus
            .take()
            .filter(|(root, _)| *root == event.entity)
            .map(|(_, target)| target);
        if !enabled || event.outcome != NumericInputCommitOutcome::Accepted || !event.edited {
            continue;
        }
        let Some((current, index)) = cached_target(&session, &mut state) else {
            continue;
        };
        if target.as_ref().is_some_and(|target| *target != current) {
            reset(event.entity, input.0, &session, &mut state, &mut values);
            continue;
        }
        let Some(value) = event.value.as_f32() else {
            continue;
        };
        let before = session
            .draft
            .as_ref()
            .unwrap()
            .boundaries
            .as_ref()
            .unwrap()
            .paths[index]
            .clone();
        let mut after = before.clone();
        match set_field(&mut after, input.0, value) {
            Ok(true) => {
                if let Err(error) = commit_path_change(
                    &mut session,
                    &mut editor,
                    index,
                    Some(before),
                    Some(after),
                    format!("{}已设为{value:.2}米", input.0.label()),
                ) {
                    editor.status = error;
                }
            }
            Ok(false) => {}
            Err(error) => {
                editor.status = error;
                reset(event.entity, input.0, &session, &mut state, &mut values);
            }
        }
    }
    for event in sensitivities.read() {
        if inputs.contains(event.entity) {
            settings.scene_editor_scrub_sensitivities.set(
                SceneEditorScrubField::BoundaryThickness,
                event
                    .after
                    .clamp(SCRUB_SENSITIVITY_MIN, SCRUB_SENSITIVITY_MAX),
            );
        } // 三个米制墙体字段共用空气墙灵敏度。
    }
}
