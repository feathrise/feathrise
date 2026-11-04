//! 设置模态中的手柄吸附字段；复用专业数值控件、取消协议与已有自动保存链。

use feathrise_editor_ui::metrics as ui_metrics;
use crate::resources::{MapEditorSettingsState, MapEditorState};
use bevy::prelude::*;
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputCommitOutcome, NumericInputCommitted,
    NumericInputDisabled, NumericInputScrubConfig, NumericInputSetValue, NumericInputSpec,
    NumericInputValue, NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue,
    spawn_numeric_scrub_field,
};
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::EditorSnapSettings;

#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SnapField {
    #[default]
    Translation,
    Rotation,
    Scale,
}
#[derive(Component, Clone, Default)]
pub(crate) struct SnapInputSlot(SnapField);

impl SnapField {
    pub(super) fn value(self, snap: EditorSnapSettings) -> f32 {
        match self {
            Self::Translation => snap.translation_meters,
            Self::Rotation => snap.rotation_degrees,
            Self::Scale => snap.scale_factor,
        }
    }
    fn set(self, snap: &mut EditorSnapSettings, value: f32) {
        match self {
            Self::Translation => snap.translation_meters = value,
            Self::Rotation => snap.rotation_degrees = value,
            Self::Scale => snap.scale_factor = value,
        }
    }
    fn config(self) -> (&'static str, &'static str, f32, f32, f32) {
        match self {
            Self::Translation => ("移动步长", "米", 0.001, 1000.0, 0.01),
            Self::Rotation => ("旋转步长", "°", 0.1, 180.0, 1.0),
            Self::Scale => ("缩放步长", "倍率", 0.001, 1.0, 0.01),
        }
    }
}

pub(super) fn input_slot(field: SnapField) -> impl Scene {
    bsn! { SnapInputSlot(field) Node { width: percent(100), height: px(26), flex_shrink: 0.0 } }
}

pub(crate) fn setup_snap_inputs(
    mut commands: Commands,
    assets: Res<AssetServer>,
    settings: Res<EditorSettings>,
    slots: Query<(Entity, &SnapInputSlot), Added<SnapInputSlot>>,
) {
    for (entity, slot) in &slots {
        let field = slot.0;
        let (label, unit, min, max, step) = field.config();
        let spec = NumericInputSpec::f32(3)
            .with_min(NumericValue::F32(min))
            .with_max(NumericValue::F32(max));
        let options = NumericScrubFieldOptions::new(
            label,
            unit,
            NumericValue::F32(field.value(settings.editor_snapping.sanitized())),
            spec,
            NumericInputScrubConfig::linear(step, step, min, max, 1.0),
        )
        .with_style(NumericScrubFieldStyle {
            width: 300.0,
            height: 26.0,
            label_font_size: ui_metrics::FONT_HINT,
            value_font_size: 12.0,
            unit_font_size: 10.0,
            ..default()
        });
        let font = feathrise_editor_core::editor_font(&assets, super::FONT_PATH);
        let mut input = None;
        commands.entity(entity).with_children(|parent| {
            input = Some(spawn_numeric_scrub_field(parent, field, font, options));
        });
        if let Some(input) = input {
            commands
                .entity(input)
                .entry::<Node>()
                .and_modify(|mut node| node.width = percent(100));
        }
    }
}

pub(crate) fn handle_snap_inputs(
    mut commits: MessageReader<NumericInputCommitted>,
    fields: Query<&SnapField>,
    editor: Res<MapEditorState>,
    mut modal: ResMut<MapEditorSettingsState>,
    mut settings: ResMut<EditorSettings>,
    access: Option<Res<crate::workbench::WorkbenchSettingsAccess>>,
) {
    // 消息游标始终推进；交接时丢弃旧提交，恢复后不能把同一条消息写进新偏好。
    for event in commits.read() {
        let Ok(field) = fields.get(event.entity) else {
            continue;
        };
        if !editor.open || !modal.open || modal.shortcuts || editor.map_transition_in_progress
            || access.as_ref().is_some_and(|access| !access.editable) {
            continue;
        }
        modal.owns_mouse_this_frame = true; // 数值提交/取消不能同帧点透模态或改作者选择。
        if event.outcome != NumericInputCommitOutcome::Accepted {
            continue;
        }
        let Some(value) = event
            .value
            .as_f32()
            .filter(|value| value.is_finite() && *value > 0.0)
        else {
            continue;
        };
        let mut next = settings.editor_snapping.sanitized();
        field.set(&mut next, value);
        let next = next.sanitized();
        if settings.editor_snapping != next {
            settings.editor_snapping = next;
        }
    }
}

pub(crate) fn sync_snap_inputs(
    mut commands: Commands,
    editor: Res<MapEditorState>,
    modal: Res<MapEditorSettingsState>,
    settings: Res<EditorSettings>,
    active: Res<ActiveNumericInput>,
    scrub: Res<ActiveNumericInputScrub>,
    fields: Query<(
        Entity,
        &SnapField,
        &NumericInputValue,
        Has<NumericInputDisabled>,
    )>,
    mut values: MessageWriter<NumericInputSetValue>,
    access: Option<Res<crate::workbench::WorkbenchSettingsAccess>>,
) {
    let blocked = access.as_ref().is_some_and(|access| !access.editable);
    let enabled =
        editor.open && modal.open && !modal.shortcuts && !editor.map_transition_in_progress
            && !blocked;
    let snap = settings.editor_snapping.sanitized();
    for (entity, field, current, disabled) in &fields {
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            }
        }
        let editing = active.entity == Some(entity) || scrub.entity == Some(entity);
        if editing && !blocked {
            continue;
        }
        let value = field.value(snap);
        if editing || current.0.as_f32() != Some(value) {
            values.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(value),
                cancel_edit: true,
            });
        }
    }
}

#[cfg(test)]
#[path = "snapping_tests.rs"]
mod tests;
