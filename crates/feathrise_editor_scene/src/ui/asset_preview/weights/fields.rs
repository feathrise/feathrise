//! 在中央视口工具栏拖动或精确输入权重、画笔强度和像素半径。

use feathrise_editor_glb_model::weights::Tool;
use super::{
    super::animation::AnimationEditorState,
    panel::{Field, tr},
    resources::{Session, WeightEditor},
    systems::{Gate, Target},
};
use crate::{
    resources::SceneEditorState,
    ui::{
        brush_controls::brush_field_style,
        scene::FONT_PATH,
        sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
    },
};
use bevy::prelude::*;
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputCommitOutcome, NumericInputCommitted,
    NumericInputConfig, NumericInputDisabled, NumericInputScrubConfig, NumericInputSetValue,
    NumericInputSpec, NumericInputValue, NumericScrubFieldOptions, NumericValue,
    localization::EditorLocale, normalize_numeric_value, spawn_numeric_scrub_field,
};

#[derive(Component, Clone, Copy, Default)]
pub(super) struct ParameterToolbar;

#[derive(Component, Clone, Copy)]
pub(super) struct Slot(pub Field);

pub(in crate::ui::asset_preview) fn parameter_toolbar() -> impl Scene {
    bsn! {
        ParameterToolbar
        Node {
            display: Display::None,
            min_width: px(0),
            height: px(26),
            align_items: AlignItems::Center,
            column_gap: px(6),
            flex_grow: 1.0,
            flex_shrink: 0.0,
        }
        Children [@field_slot(Field::Radius)--@field_slot(Field::Strength)--@field_slot(Field::Amount)]
    }
}

fn field_slot(field: Field) -> impl Scene {
    bsn! {
        ~{Slot(field)}
        Node {
            display: Display::None,
            height: px(26),
            min_width: px(160), // 给标签和单位留出宽度，窄视口由外层整行横滚。
            max_width: px(220),
            flex_basis: px(160),
            flex_grow: 1.0,
            flex_shrink: 0.0,
        }
    }
}

fn matches_context(target: &Target, sidebar: &SceneEditorSidebarState, session: &Session) -> bool {
    sidebar.right == EditorRightSidebarTab::Weights
        && target.preview.render_visible
        && target.preview.external_preview.is_none()
        && target.preview.selected_path.as_deref() == Some(session.key.path.as_str())
        && target.matches(&session.key)
        && !session.stale
}

impl Field {
    fn applies(self, session: &Session) -> bool {
        if session.original {
            return false;
        }
        match self {
            Self::Radius | Self::Strength => session.tool.paint(),
            Self::Amount => {
                session.tool == Tool::Set
                    && !session.drafts[session.surface].selection.is_empty()
                    && !session.current_locks()[session.joint as usize]
            }
        }
    }

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
    fn displayed_value(self, session: &Session) -> f32 {
        match self {
            Self::Radius => session.radius,
            Self::Strength => session.strength * 100.0,
            Self::Amount => session.amount * 100.0,
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
        .with_style(brush_field_style());
        let font = feathrise_editor_core::editor_font(&assets, FONT_PATH);
        commands.entity(slot).with_children(|parent| {
            let root = spawn_numeric_scrub_field(parent, *field, font, options);
            parent
                .commands()
                .entity(root)
                .insert(NumericInputDisabled)
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
    fields: Query<&Field, Without<NumericInputDisabled>>,
    gate: Gate,
    editor: Res<SceneEditorState>,
    target: Target,
    sidebar: Res<SceneEditorSidebarState>,
    mut state: ResMut<WeightEditor>,
    animation: Option<Res<AnimationEditorState>>,
) {
    if animation.as_ref().is_some_and(|animation| animation.weight_motion_active()) {
        commits.clear();
        return;
    }
    for commit in commits.read() {
        let Ok(field) = fields.get(commit.entity) else {
            continue;
        };
        if commit.outcome != NumericInputCommitOutcome::Accepted
            || !gate.open(&editor)
            || state.busy()
        {
            continue;
        }
        let Some(session) = state
            .session
            .as_mut()
            .filter(|session| matches_context(&target, &sidebar, session) && field.applies(session))
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
    mut groups: Query<&mut Node, (With<ParameterToolbar>, Without<Slot>)>,
    mut slots: Query<(&Slot, &mut Node)>,
    fields: Query<(
        Entity,
        &Field,
        &NumericInputValue,
        &NumericInputConfig,
        Has<NumericInputDisabled>,
    )>,
    state: Res<WeightEditor>,
    target: Target,
    sidebar: Res<SceneEditorSidebarState>,
    gate: Gate,
    editor: Res<SceneEditorState>,
    animation: Option<Res<AnimationEditorState>>,
    active: Option<Res<ActiveNumericInput>>,
    scrub: Option<Res<ActiveNumericInputScrub>>,
    mut values: MessageWriter<NumericInputSetValue>,
) {
    let moving = animation.as_ref().is_some_and(|animation| animation.weight_motion_active());
    let context = state.session.as_ref().filter(|session| {
        editor.open && !moving && matches_context(&target, &sidebar, session)
    });
    let shows = |field: Field| context.is_some_and(|session| field.applies(session));
    let display = if shows(Field::Radius) || shows(Field::Amount) {
        Display::Flex
    } else {
        Display::None
    };
    let available = gate.open(&editor) && !state.busy();
    for mut group in &mut groups {
        if group.display != display {
            // 同地图笔刷，只控制右端参数组显隐，工具切换不改用户的横滚位置。
            group.display = display;
        }
    }
    for (Slot(field), mut slot) in &mut slots {
        let display = if shows(*field) { Display::Flex } else { Display::None };
        if slot.display != display {
            slot.display = display;
        }
    }
    for (entity, field, current, config, disabled) in &fields {
        let enabled = available && shows(*field);
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<NumericInputDisabled>();
            } else {
                commands.entity(entity).insert(NumericInputDisabled);
            }
        }
        let editing = active
            .as_ref()
            .is_some_and(|input| input.entity == Some(entity));
        if (editing && enabled)
            || scrub
                .as_ref()
                .is_some_and(|input| input.entity == Some(entity))
        {
            continue;
        }
        // 隐藏时结束精确输入。拖动仍由公共鼠标门持有到松键，失效的提交由业务门拒绝。
        let value = state
            .session
            .as_ref()
            .map_or_else(|| field.range().3, |session| field.displayed_value(session));
        let value = normalize_numeric_value(NumericValue::F32(value), config.0);
        if editing || current.0 != value {
            values.write(NumericInputSetValue {
                entity,
                value,
                cancel_edit: true,
            });
        }
    }
}
