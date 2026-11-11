//! 直接输入或拖动精确权重、画笔强度和像素半径，遵循原数值编辑提交协议。

use super::{
    panel::{Field, tr},
    resources::WeightEditor,
    systems::{Gate, Target},
};
use crate::{
    resources::SceneEditorState,
    ui::{
        scene::FONT_PATH,
        sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
    },
};
use bevy::prelude::*;
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputCommitOutcome, NumericInputCommitted,
    NumericInputDisabled, NumericInputScrubConfig, NumericInputSetValue, NumericInputSpec,
    NumericScrubFieldOptions, NumericScrubFieldStyle, NumericValue, localization::EditorLocale,
    spawn_numeric_scrub_field,
};

#[derive(Component, Clone, Copy)]
pub(super) struct Slot(pub Field);

impl Field {
    fn range(self) -> (f32, f32, f32, f32, &'static str) {
        match self {
            Self::Radius => (4.0, 256.0, 1.0, 32.0, "px"),
            Self::Strength => (1.0, 100.0, 1.0, 30.0, "%"),
            Self::Amount => (0.0, 100.0, 0.5, 50.0, "%"),
        }
    }
    fn label(self, locale: EditorLocale) -> &'static str {
        match self {
            Self::Radius => tr(locale, "半径", "Radius"),
            Self::Strength => tr(locale, "强度", "Strength"),
            Self::Amount => tr(locale, "设定权重", "Target weight"),
        }
    }
    fn index(self) -> usize {
        match self {
            Self::Radius => 0,
            Self::Strength => 1,
            Self::Amount => 2,
        }
    }
}

pub(super) fn setup(
    mut commands: Commands,
    slots: Query<(Entity, &Slot), Added<Slot>>,
    assets: Option<Res<AssetServer>>,
    locale: Option<Res<EditorLocale>>,
) {
    let Some(assets) = assets else { return };
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (slot, Slot(field)) in &slots {
        let (min, max, step, start, unit) = field.range();
        let spec = NumericInputSpec::f32(if *field == Field::Radius { 0 } else { 2 })
            .with_step_buttons(false)
            .with_live_update(false)
            .with_min(NumericValue::F32(min))
            .with_max(NumericValue::F32(max));
        let options = NumericScrubFieldOptions::new(
            field.label(locale),
            unit,
            NumericValue::F32(start),
            spec,
            NumericInputScrubConfig::linear(step, step, min, max, 1.0),
        )
        .with_style(NumericScrubFieldStyle {
            width: 156.0,
            height: 24.0,
            ..default()
        });
        let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
        commands.entity(slot).with_children(|parent| {
            let root = spawn_numeric_scrub_field(parent, *field, font, options);
            parent
                .commands()
                .entity(root)
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.width = percent(100);
                    node.min_width = px(0);
                });
        });
    }
}

pub(super) fn input(
    mut commits: MessageReader<NumericInputCommitted>,
    fields: Query<&Field>,
    gate: Gate,
    editor: Res<SceneEditorState>,
    target: Target,
    sidebar: Res<SceneEditorSidebarState>,
    mut state: ResMut<WeightEditor>,
) {
    for commit in commits.read() {
        let Ok(field) = fields.get(commit.entity) else {
            continue;
        };
        if commit.outcome != NumericInputCommitOutcome::Accepted
            || !gate.open(&editor)
            || sidebar.right != EditorRightSidebarTab::Weights
            || state.stroke.is_some()
            || state.work.is_some()
        {
            continue;
        }
        let Some(session) = state
            .session
            .as_mut()
            .filter(|session| target.matches(&session.key) && !session.stale)
        else {
            continue;
        };
        let (min, max, _, _, _) = field.range();
        let Some(value) = commit.value.as_f32().filter(|value| value.is_finite()) else {
            continue;
        };
        let value = value.clamp(min, max);
        match field {
            Field::Radius => session.radius = value,
            Field::Strength => session.strength = value * 0.01,
            Field::Amount => session.amount = value * 0.01,
        }
        state.bump();
    }
}

pub(super) fn sync(
    mut commands: Commands,
    fields: Query<(Entity, &Field, Has<NumericInputDisabled>)>,
    state: Res<WeightEditor>,
    target: Target,
    sidebar: Res<SceneEditorSidebarState>,
    active: Option<Res<ActiveNumericInput>>,
    scrub: Option<Res<ActiveNumericInputScrub>>,
    mut sent: Local<[Option<(Entity, f32)>; 3]>,
    mut values: MessageWriter<NumericInputSetValue>,
) {
    let session = state
        .session
        .as_ref()
        .filter(|session| target.matches(&session.key) && !session.stale);
    let enabled = session.is_some()
        && sidebar.right == EditorRightSidebarTab::Weights
        && state.work.is_none()
        && state.stroke.is_none();
    for (entity, field, disabled) in &fields {
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            }
        }
        let Some(session) = session else { continue };
        if active
            .as_ref()
            .is_some_and(|input| input.entity == Some(entity))
            || scrub
                .as_ref()
                .is_some_and(|input| input.entity == Some(entity))
        {
            continue;
        }
        let value = match field {
            Field::Radius => session.radius,
            Field::Strength => session.strength * 100.0,
            Field::Amount => session.amount * 100.0,
        };
        let last = &mut sent[field.index()];
        if *last != Some((entity, value)) {
            *last = Some((entity, value));
            values.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(value),
                cancel_edit: true,
            });
        }
    }
}
