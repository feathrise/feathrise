//! 全局LOD设置只写偏好；数值收口、隐藏取消和同帧鼠标所有权沿用正式模态协议。

use feathrise_editor_ui::metrics as ui_metrics;
use feathrise_editor_ui::EditorButtonClick;
use super::components::*;
use bevy::prelude::*;
use crate::resources::{MapEditorSettingsState, MapEditorState};
use feathrise_editor_core::EditorSettings;
use feathrise_settings::{ MeshLodSettings};
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputCommitOutcome, NumericInputCommitted,
    NumericInputDisabled, NumericInputScrubConfig, NumericInputSetValue, NumericInputSpec,
    NumericInputValue, NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue,
    spawn_numeric_scrub_field,
};
use crate::ui::theme as colors;

pub(crate) fn setup_mesh_lod_inputs(
    mut commands: Commands, server: Res<AssetServer>, settings: Res<EditorSettings>,
    slots: Query<(Entity, &Slot), Added<Slot>>,
) {
    for (entity, slot) in &slots {
        let field = slot.0;
        let (label, unit, min, max, step) = field.config();
        let spec = NumericInputSpec::f32(4).with_min(NumericValue::F32(min)).with_max(NumericValue::F32(max))
            .with_live_update(false).with_step_buttons(false);
        let options = NumericScrubFieldOptions::new(label, unit,
            NumericValue::F32(field.value(settings.render.mesh_lod.sanitized())), spec,
            NumericInputScrubConfig::linear(step, step, min, max, 1.0))
            .with_style(NumericScrubFieldStyle { width: 300.0, height: 26.0,
                label_font_size: ui_metrics::FONT_HINT, value_font_size: 12.0, unit_font_size: 10.0, ..default() });
        commands.entity(entity).with_children(|parent| {
            let root = spawn_numeric_scrub_field(parent, field, feathrise_editor_core::editor_font(&server, crate::ui::scene::FONT_PATH), options);
            parent.commands().entity(root).entry::<Node>().and_modify(|mut node| node.width = percent(100));
        });
    }
}

pub(crate) fn handle_mesh_lod_inputs(
    editor: Res<MapEditorState>, mut modal: ResMut<MapEditorSettingsState>,
    mut settings: ResMut<EditorSettings>, fields: Query<&MeshLodField>,
    mut commits: MessageReader<NumericInputCommitted>,
    buttons: Query<(&EditorButtonClick, &ActionButton), Changed<EditorButtonClick>>,
    numeric: Res<ActiveNumericInput>, scrub: Res<ActiveNumericInputScrub>,
    access: Option<Res<crate::workbench::WorkbenchSettingsAccess>>,
) {
    let visible = editor.open && modal.open && !modal.shortcuts && !editor.map_transition_in_progress
        && access.as_ref().is_none_or(|access| access.editable);
    for event in commits.read() {
        let Ok(field) = fields.get(event.entity) else { continue; };
        if !visible { continue; }
        modal.owns_mouse_this_frame = true;
        if event.outcome != NumericInputCommitOutcome::Accepted || !event.edited { continue; }
        let (_, _, min, max, _) = field.config();
        let Some(value) = event.value.as_f32().filter(|value| value.is_finite() && (min..=max).contains(value)) else { continue; };
        let mut next = settings.render.mesh_lod.sanitized();
        field.set(&mut next, value);
        if settings.render.mesh_lod != next { settings.render.mesh_lod = next; }
    }
    if !visible || numeric.entity.is_some() || scrub.entity.is_some() { return; }
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() { continue; }
        let mut next = settings.render.mesh_lod.sanitized();
        match button.0 {
            Action::Display => next.display_enabled = !next.display_enabled,
            Action::Collision => next.collision_enabled = !next.collision_enabled,
            Action::Reset => next = MeshLodSettings::default(),
        }
        if settings.render.mesh_lod != next { settings.render.mesh_lod = next; }
        modal.owns_mouse_this_frame = true;
        break;
    }
}

pub(crate) fn sync_mesh_lod_inputs(
    mut commands: Commands, editor: Res<MapEditorState>, modal: Res<MapEditorSettingsState>,
    settings: Res<EditorSettings>, numeric: Res<ActiveNumericInput>, scrub: Res<ActiveNumericInputScrub>,
    fields: Query<(Entity, &MeshLodField, &NumericInputValue, Has<NumericInputDisabled>)>,
    mut values: MessageWriter<NumericInputSetValue>,
    mut labels: Query<(&ActionLabel, &mut Text)>,
    mut buttons: Query<(&ActionButton, &Interaction, &mut BackgroundColor, &mut BorderColor)>,
    access: Option<Res<crate::workbench::WorkbenchSettingsAccess>>,
) {
    let visible = editor.open && modal.open && !modal.shortcuts && !editor.map_transition_in_progress
        && access.as_ref().is_none_or(|access| access.editable);
    let policy = settings.render.mesh_lod.sanitized();
    for (entity, field, current, disabled) in &fields {
        if visible == disabled {
            if visible { commands.entity(entity).remove::<NumericInputDisabled>(); }
            else { commands.entity(entity).insert(NumericInputDisabled); }
        }
        let editing = numeric.entity == Some(entity) || scrub.entity == Some(entity);
        if editing && visible { continue; }
        let value = field.value(policy);
        if editing || current.0.as_f32() != Some(value) {
            values.write(NumericInputSetValue { entity, value: NumericValue::F32(value), cancel_edit: true });
        } // 隐藏或换页取消精确输入，迟到99不能成为关闭模态的最后一次偏好修改。
    }
    for (label, mut text) in &mut labels {
        let value = match label.0 {
            Action::Display => if policy.display_enabled { "显示 LOD：开启" } else { "显示 LOD：原始" },
            Action::Collision => if policy.collision_enabled { "碰撞 LOD：开启" } else { "碰撞 LOD：原始" },
            Action::Reset => continue,
        };
        if text.0 != value { text.0 = value.into(); }
    }
    for (button, interaction, mut background, mut border) in &mut buttons {
        let selected = match button.0 { Action::Display => policy.display_enabled, Action::Collision => policy.collision_enabled, Action::Reset => false };
        let color = match (*interaction, selected) {
            (Interaction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (Interaction::Hovered, _) => colors::BUTTON_DEV_SMALL_HOVERED,
            (Interaction::None, true) => colors::SELECT_BG,
            _ => colors::BUTTON_DEV_SMALL_NORMAL,
        };
        if background.0 != color { background.0 = color; }
        let wanted = BorderColor::all(if selected { colors::COPPER_BORDER } else { colors::COPPER_BORDER_WEAK });
        if *border != wanted { *border = wanted; }
    }
}
