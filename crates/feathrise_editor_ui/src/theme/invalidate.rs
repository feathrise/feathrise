//! 主题与按钮反馈接入 Bevy 持久提取的失效阶段，避免稳定帧重复映射字形。

use super::{
    components::{EditorCodeSurface, EditorLiteralColor, EditorThemeRoot},
    extract::owned,
    resources::{EditorPalette, ThemeExtraction, ThemeTopology},
};
use crate::InteractionButton;
use bevy::{
    prelude::*,
    render::Extract,
    ui::{InteractionDisabled, RelativeCursorPosition},
    ui_render::ExtractedUiNodes,
};
use feathrise_ui_widgets::{
    ButtonColors, NumericInputDisabled, NumericInputFieldButton, NumericInputStepButton,
};
use std::collections::HashSet;

#[allow(clippy::type_complexity)]
pub(super) fn theme_changes(
    mut commands: Commands,
    mut nodes: ResMut<ExtractedUiNodes>,
    palette: Extract<Res<EditorPalette>>,
    topology: Extract<Res<ThemeTopology>>,
    parents: Extract<Query<&ChildOf>>,
    roots: Extract<Query<(), With<EditorThemeRoot>>>,
    mut state: ResMut<ThemeExtraction>,
    interaction_changes: Extract<
        Query<
            (Entity, Ref<Interaction>, Option<&RelativeCursorPosition>),
            (
                With<InteractionButton>,
                Or<(Changed<Interaction>, Changed<RelativeCursorPosition>)>,
            ),
        >,
    >,
    appearance_changes: Extract<
        Query<
            Entity,
            Or<(
                Changed<InteractionDisabled>,
                Changed<ButtonColors>,
                Changed<EditorCodeSurface>,
                Changed<EditorLiteralColor>,
            )>,
        >,
    >,
    (
        mut removed_button,
        mut removed_disabled,
        mut removed_colors,
        mut removed_cursor,
        mut removed_code,
        mut removed_literal,
        mut removed_numeric_disabled,
    ): (
        Extract<RemovedComponents<InteractionButton>>,
        Extract<RemovedComponents<InteractionDisabled>>,
        Extract<RemovedComponents<ButtonColors>>,
        Extract<RemovedComponents<RelativeCursorPosition>>,
        Extract<RemovedComponents<EditorCodeSurface>>,
        Extract<RemovedComponents<EditorLiteralColor>>,
        Extract<RemovedComponents<NumericInputDisabled>>,
    ),
    numeric_disabled: Extract<Query<Entity, Changed<NumericInputDisabled>>>,
    numeric_buttons: Extract<
        Query<
            (Entity, Option<&NumericInputFieldButton>, Option<&NumericInputStepButton>),
            Or<(With<NumericInputFieldButton>, With<NumericInputStepButton>)>,
        >,
    >,
    mut dirty: Local<HashSet<Entity>>,
) {
    let topology_changed = state.topology != topology.0;
    let palette_changed = palette.is_changed();
    let parent = |entity| parents.get(entity).ok().map(ChildOf::parent);
    let root = |entity| roots.contains(entity);
    if topology_changed {
        state.topology = topology.0;
        state.scopes.clear();
    }
    if topology_changed || palette_changed {
        // 仅在切换色板或重新挂载时核对已有绘制项。从官方提取重新取得未映射的原色。
        for entity in nodes.uinodes.keys().map(|entity| entity.entity()) {
            let was_themed = state.themed_entities.contains(&entity);
            let is_themed = owned(entity, &mut state, parent, root);
            if was_themed != is_themed || (is_themed && palette_changed) {
                dirty.insert(entity);
            }
        }
    }
    for (entity, interaction, cursor) in &interaction_changes {
        let outside = *interaction == Interaction::Pressed
            && cursor.is_some_and(|cursor| !cursor.cursor_over());
        let boundary_changed = if outside {
            state.pressed_outside.insert(entity)
        } else {
            state.pressed_outside.remove(&entity)
        };
        if interaction.is_changed() || boundary_changed {
            dirty.insert(entity);
        }
    }
    for entity in removed_button.read().chain(removed_cursor.read()) {
        state.pressed_outside.remove(&entity);
        dirty.insert(entity);
    }
    dirty.extend(
        appearance_changes
            .iter()
            .chain(removed_disabled.read())
            .chain(removed_colors.read())
            .chain(removed_code.read())
            .chain(removed_literal.read()),
    );
    let changed_roots: HashSet<_> = numeric_disabled
        .iter()
        .chain(removed_numeric_disabled.read())
        .collect();
    if !changed_roots.is_empty() {
        for (entity, field, step) in &numeric_buttons {
            let root = field.map(|field| field.root).or_else(|| step.map(|step| step.root));
            if root.is_some_and(|root| changed_roots.contains(&root)) {
                dirty.insert(entity);
            }
        }
        dirty.extend(changed_roots);
    }

    for entity in dirty.drain() {
        if !state.themed_entities.contains(&entity) && !owned(entity, &mut state, parent, root) {
            continue;
        }
        let main = entity.into();
        if nodes.changed.insert(main)
            && let Some((_, mut draws)) = nodes.uinodes.remove(&main)
        {
            // 与官方 UI 失效共用同一组实体和改动标记，透明反馈也随所属组销毁。
            for (render_entity, _) in draws.drain(..) {
                commands.entity(render_entity).despawn();
            }
        }
    }
}
