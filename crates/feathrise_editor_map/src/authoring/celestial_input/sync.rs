//! 天体光照数值、继承禁用态、颜色预览、按钮文字与折叠正文同步。

use feathrise_editor_ui::UiInteraction;
use bevy::prelude::*;
use feathrise_map_content::{CelestialLightingProfile, resolve};
use feathrise_editor_core::EditorSettings;
use crate::ui::theme as colors;
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputDisabled, NumericInputScrubConfig,
    NumericInputSetValue, NumericInputValue, NumericValue,
};

use super::contract::{
    CelestialColorSwatch, CelestialNumericInput, CelestialNumericInputState, CelestialNumericScope,
    CelestialOverrideToggle, CelestialOverrideToggleLabel, CelestialSectionBody,
    CelestialSectionState, CelestialSectionToggleLabel,
};
use super::systems::rollback_owned_scrub;
use crate::authoring::{
    EditorAuthoringSession, GlobalEnvironmentAuthoringState, GlobalEnvironmentSaveTask,
};
use crate::resources::MapEditorState;

fn map_profile(
    session: &EditorAuthoringSession,
    global: &CelestialLightingProfile,
) -> Option<CelestialLightingProfile> {
    let draft = session
        .draft
        .as_ref()
        .filter(|draft| draft.manifest.is_data_driven())?;
    Some(resolve(
        global,
        &draft.manifest.environment.celestial_overrides,
    ))
}

/// 选择、撤销、重做、换图、全局热更和设置恢复后同步全部天体控件。
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_celestial_controls(
    mut commands: Commands,
    mut editor: ResMut<MapEditorState>,
    mut session: ResMut<EditorAuthoringSession>,
    mut global: ResMut<GlobalEnvironmentAuthoringState>,
    global_save: Res<GlobalEnvironmentSaveTask>,
    settings: Res<EditorSettings>,
    active: Res<ActiveNumericInput>,
    active_scrub: Res<ActiveNumericInputScrub>,
    mut input_state: ResMut<CelestialNumericInputState>,
    (sections, locale): (Res<CelestialSectionState>, Option<Res<feathrise_editor_ui::localization::EditorLocale>>),
    mut inputs: Query<(
        Entity,
        &CelestialNumericInput,
        &NumericInputValue,
        Has<NumericInputDisabled>,
        &mut NumericInputScrubConfig,
    )>,
    mut override_buttons: Query<
        (
            &CelestialOverrideToggle,
            &UiInteraction,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Without<CelestialColorSwatch>, // 覆盖按钮与颜色色块互斥，向 Bevy 证明两处 BackgroundColor 可变访问不会重叠。
    >,
    mut text_queries: ParamSet<(
        Query<(&CelestialOverrideToggleLabel, &mut Text)>,
        Query<(&CelestialSectionToggleLabel, &mut Text)>,
    )>,
    mut bodies: Query<(&CelestialSectionBody, &mut Node)>,
    mut swatches: Query<
        (&CelestialColorSwatch, &mut BackgroundColor),
        Without<CelestialOverrideToggle>, // 即使静态 UI 误挂双 marker，也不让同一实体进入两个写查询。
    >,
    mut set_values: MessageWriter<NumericInputSetValue>,
) {
    if input_state.scrub.as_ref().is_some_and(|transaction| {
        transaction.aborted && active_scrub.entity != Some(transaction.entity)
    }) {
        input_state.scrub = None;
    }
    if !editor.open && active.entity.is_none() && active_scrub.entity.is_none() {
        if let Some(transaction) = input_state.scrub.as_ref() {
            rollback_owned_scrub(transaction, &mut editor, &mut session, &mut global);
        }
        *input_state = CelestialNumericInputState::default();
    }

    let global_profile = &global.draft.celestial_lighting;
    let environment = session
        .draft
        .as_ref()
        .filter(|draft| draft.manifest.is_data_driven())
        .map(|draft| &draft.manifest.environment);
    let common_enabled = editor.open
        && !editor.close_requested
        && !editor.new_map_dialog_open
        && !session.saving
        && global_save.task.is_none();

    for (entity, marker, current, disabled, mut scrub) in &mut inputs {
        let key = marker.0;
        let value = key.displayed_value(global_profile, environment);
        let should_enable = common_enabled
            && match key.scope {
                CelestialNumericScope::Global => true,
                CelestialNumericScope::Map => {
                    environment.is_some_and(|environment| key.map_editable(environment))
                }
            };
        if should_enable && disabled {
            commands.entity(entity).remove::<NumericInputDisabled>();
        } else if !should_enable && !disabled {
            commands.entity(entity).insert(NumericInputDisabled);
        }
        let desired = value.unwrap_or(0.0);
        if active.entity != Some(entity)
            && active_scrub.entity != Some(entity)
            && current.0.as_f32() != Some(desired)
        {
            set_values.write(NumericInputSetValue {
                entity,
                value: NumericValue::F32(desired),
                cancel_edit: true,
            });
        }
        if active_scrub.entity != Some(entity) {
            let sensitivity = key.field.sensitivity(&settings);
            if scrub.sensitivity != sensitivity {
                scrub.sensitivity = sensitivity;
            }
        }
    }

    for (toggle, interaction, mut background, mut border) in &mut override_buttons {
        let overridden = environment
            .is_some_and(|environment| toggle.0.is_overridden(&environment.celestial_overrides));
        let desired = match interaction {
            UiInteraction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
            UiInteraction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
            UiInteraction::None if overridden => colors::SLOT_BG_FILLED,
            UiInteraction::None => colors::SLOT_BG,
        };
        if background.0 != desired {
            background.0 = desired;
        }
        let desired_border = BorderColor::all(if overridden {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
        if *border != desired_border {
            *border = desired_border;
        }
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (marker, mut text) in &mut text_queries.p0() {
        let overridden = environment
            .is_some_and(|environment| marker.0.is_overridden(&environment.celestial_overrides));
        let desired = if overridden {
            "地图覆盖"
        } else {
            "跟随全局"
        };
        let desired = locale.editor_label(desired);
        if text.as_str() != desired {
            **text = desired.to_owned();
        }
    }
    for (marker, mut text) in &mut text_queries.p1() {
        let expanded = sections.expanded(marker.0);
        let title = match marker.0 {
            CelestialNumericScope::Global => "太阳与月光 · 全局默认",
            CelestialNumericScope::Map => "太阳与月光 · 地图覆盖",
        };
        let desired = format!("{} {}", if expanded { "▼" } else { "▶" }, locale.editor_label(title));
        if text.as_str() != desired {
            **text = desired;
        }
    }
    for (marker, mut node) in &mut bodies {
        let display = if sections.expanded(marker.0) {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }

    let resolved_map = map_profile(&session, global_profile);
    for (swatch, mut background) in &mut swatches {
        let profile = match swatch.scope {
            CelestialNumericScope::Global => Some(global_profile),
            CelestialNumericScope::Map => resolved_map.as_ref(),
        };
        let color = profile.map_or([0.0; 3], |profile| {
            if swatch.day {
                profile.day_base_color_srgb
            } else {
                profile.night_base_color_srgb
            }
        });
        let desired = Color::srgb(color[0], color[1], color[2]);
        if background.0 != desired {
            background.0 = desired;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 直接注册正式同步系统，覆盖系统参数初始化阶段的 ECS 借用校验。

    #[test]
    fn celestial_button_and_swatch_queries_are_disjoint_on_first_update() {
        let mut app = App::new(); // B0001 只会在系统初始化或首帧执行时出现，单纯 cargo check 无法发现。
        app.init_resource::<MapEditorState>() // 提供 F12 开关与公共脏状态。
            .init_resource::<EditorAuthoringSession>() // 空地图会话是合法的编辑器启动阶段。
            .init_resource::<GlobalEnvironmentAuthoringState>() // 提供编译期安全全局光照草稿。
            .init_resource::<GlobalEnvironmentSaveTask>() // 没有进行中的全局保存任务。
            .init_resource::<EditorSettings>() // 数字拖动灵敏度读取正式设置资源。
            .init_resource::<ActiveNumericInput>() // 当前没有精确输入焦点。
            .init_resource::<ActiveNumericInputScrub>() // 当前没有数字拖动事务。
            .init_resource::<CelestialNumericInputState>() // 当前没有业务层天体编辑事务。
            .init_resource::<CelestialSectionState>() // 使用两个折叠区的默认展开状态。
            .add_message::<NumericInputSetValue>() // 同步系统通过公共消息更新数字框显示值。
            .add_systems(Update, sync_celestial_controls); // 注册用户启动时发生冲突的完整正式系统。

        app.world_mut().spawn((
            CelestialOverrideToggle(super::super::CelestialOverrideProperty::DayIlluminance), // 覆盖按钮只带按钮 marker。
            UiInteraction::None,          // 无悬停状态走普通样式分支。
            BackgroundColor::default(), // 系统会更新按钮背景色。
            BorderColor::default(),     // 系统会更新按钮边框色。
        ));
        app.world_mut().spawn((
            CelestialColorSwatch {
                scope: CelestialNumericScope::Global, // 色块读取安全全局光照草稿。
                day: true,                            // 使用日间基准颜色。
            },
            BackgroundColor::default(), // 系统会更新色块背景色。
        ));

        app.update(); // 成功完成首帧即证明两个 BackgroundColor 查询不再触发 Bevy B0001。
    }
}
