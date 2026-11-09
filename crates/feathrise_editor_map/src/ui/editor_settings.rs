//! 编辑器偏好设置：视口显示、缩放、配色、吸附与操作偏好、默认布局恢复，不接管作者数据。

use feathrise_editor_ui::UiInteraction;
pub(super) mod components;
mod scene;
mod shortcuts_scene;

use feathrise_editor_ui::InteractionButton;
use self::components::{
    SettingsBody, SettingsGeneralOnly, SettingsPageButton, SettingsShortcutsOnly,
    SettingsThemeText, SettingsUtilityButton,
};
use feathrise_editor_ui::EditorButtonClick;
use feathrise_editor_ui::metrics as ui_metrics;

use crate::ui::theme as colors;
use bevy::ecs::message::MessageCursor;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::FontSourceTemplate;
use bevy::ui::{FocusPolicy, RelativeCursorPosition};
use feathrise_editor_core::sanitize_editor_ui_scale_auto_offset;
use feathrise_editor_core::EditorColorTheme;
use feathrise_editor_core::EditorLayoutPreferences;
use feathrise_editor_core::EditorSettings;
use feathrise_editor_core::effective_editor_ui_scale;
use feathrise_editor_core::sanitize_editor_auto_ui_scale;
use feathrise_editor_core::sanitize_editor_ui_scale;

use super::scene::FONT_PATH;
use super::text_input::EditorTextInput;
use crate::components::{
    MapEditorSettingsButton, MapEditorSettingsPanel, MapEditorSettingsScaleText,
    MapEditorShortcutSettingsPanel,
};
use crate::resources::{
    MapEditorLayoutState, MapEditorNumericScrubMouseGuard, MapEditorSettingsState, MapEditorState,
};
use crate::types::{EditorSettingsAction, EditorShortcutNoticeKind};

mod snapping; // 三个公共数值框只改编辑器偏好，不进入地图作者历史。
pub(crate) use snapping::{handle_snap_inputs, setup_snap_inputs, sync_snap_inputs};
mod mesh_lod; // 全局简化精度独立于作者高级分层，关闭F12后继续保留。
pub(crate) use mesh_lod::{handle_mesh_lod_inputs, setup_mesh_lod_inputs, sync_mesh_lod_inputs};

pub(super) fn settings_button(label: &str, action: EditorSettingsAction, width: f32) -> impl Scene {
    settings_button_with_height(label, action, width, 28.0)
}

fn settings_button_with_height(
    label: &str,
    action: EditorSettingsAction,
    width: f32,
    height: f32,
) -> impl Scene {
    let style = feathrise_editor_ui::dialog_button_style(false);
    bsn! {
        InteractionButton
        MapEditorSettingsButton(action)
        ~{Node {
            width: px(width), min_width: px(0), height: px(height), ..style.node
        }}
        @feathrise_editor_ui::dialog_button_colors_scene(false)
        ~{BackgroundColor(style.colors.normal)}
        BorderColor::all(feathrise_editor_ui::colors::DIALOG_BORDER)
        Children [
            Text(label)
            @feathrise_editor_ui::localization::label_scene(label)
            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
            TextLayout::no_wrap()
            TextColor(colors::TEXT_PRIMARY)
        ]
    }
}

pub(super) fn settings_entry() -> impl Scene {
    settings_button_with_height("设置", EditorSettingsAction::Open, 48.0, 26.0)
}

pub(super) fn settings_panel() -> impl Scene {
    scene::settings_panel()
}

/// 编辑器与玩法视图共用显示偏好；F12开关不撤销背面显示，避免关闭后画面变化。
pub(crate) fn sync_terrain_backfaces_preview(
    settings: Res<EditorSettings>,
    preview: Option<ResMut<feathrise_scene::TerrainBackfacesPreview>>,
) {
    let Some(mut preview) = preview else {
        return;
    };
    let enabled = settings.editor_show_terrain_backfaces;
    if preview.enabled != enabled {
        preview.enabled = enabled;
    }
}

/// 模态独占作者输入，但不暂停后台任务、重建表单或修改未保存的作者数据。
pub(crate) fn handle_settings_input(
    editor: Res<MapEditorState>,
    guard: Res<MapEditorNumericScrubMouseGuard>,
    mut keyboard: ResMut<ButtonInput<KeyCode>>,
    mut state: ResMut<MapEditorSettingsState>,
    mut settings: ResMut<EditorSettings>,
    mut layout: ResMut<MapEditorLayoutState>,
    windows: Query<&Window>,
    buttons: Query<(&MapEditorSettingsButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut focus: Option<ResMut<InputFocus>>,
    fields: Query<Entity, With<EditorTextInput>>,
    numeric: Option<Res<feathrise_editor_ui::ActiveNumericInput>>,
    scrub: Option<Res<feathrise_editor_ui::ActiveNumericInputScrub>>,
    numeric_fields: Query<
        (
            Option<&snapping::SnapField>,
            Option<&mesh_lod::MeshLodField>,
        ),
        Or<(With<snapping::SnapField>, With<mesh_lod::MeshLodField>)>,
    >,
    feedback: (
        Option<Res<Messages<feathrise_editor_ui::NumericInputCommitted>>>,
        Option<Res<super::context_help::EditorContextHelpState>>,
        Option<Res<crate::workbench::WorkbenchSettingsAccess>>,
    ),
    mut commit_cursor: Local<MessageCursor<feathrise_editor_ui::NumericInputCommitted>>,
    mut set_values: Option<ResMut<Messages<feathrise_editor_ui::NumericInputSetValue>>>,
) {
    let editable = feedback.2.as_ref().is_none_or(|access| access.editable);
    let numeric_canceled = feedback.0.as_ref().is_some_and(|messages| {
        commit_cursor.read(messages).fold(false, |canceled, event| {
            canceled
                || numeric_fields.contains(event.entity)
                    && event.outcome == feathrise_editor_ui::NumericInputCommitOutcome::Canceled
        })
    });
    if state.owns_mouse_this_frame {
        state.owns_mouse_this_frame = false;
    }
    if !editor.open
        || editor.close_requested
        || editor.map_transition_in_progress
        || editor.map_switch_confirmation.is_some()
    {
        if state.open {
            state.open = false;
            state.listening = None;
        }
        return;
    }
    if state.open {
        if !editable && state.listening.take().is_some() {
            state.shortcut_notice.clear();
            state.shortcut_notice_kind = EditorShortcutNoticeKind::Idle;
        }
        if feedback
            .1
            .as_ref()
            .is_some_and(|help| help.blocks_modal_input())
        {
            state.owns_mouse_this_frame = true;
            return; // 帮助拥有Esc/点外及关闭帧，不能顺带关闭设置或改写快捷键录入。
        }
        let numeric_editing = numeric
            .as_ref()
            .and_then(|active| active.entity)
            .is_some_and(|entity| numeric_fields.contains(entity))
            || scrub
                .as_ref()
                .and_then(|active| active.entity)
                .is_some_and(|entity| numeric_fields.contains(entity))
            || numeric_canceled; // 公共控件先结束焦点；同帧Esc仍只取消数值，不能顺带关闭设置。
        if keyboard.just_pressed(KeyCode::Escape) && numeric_editing {
            if let Some((entity, (snap, lod))) = scrub
                .as_ref()
                .and_then(|scrub| scrub.entity)
                .and_then(|entity| numeric_fields.get(entity).ok().map(|field| (entity, field)))
                && let Some(values) = set_values.as_mut()
            {
                let value = snap
                    .map(|field| field.value(settings.editor_snapping.sanitized()))
                    .or_else(|| lod.map(|field| field.value(settings.render.mesh_lod.sanitized())));
                if let Some(value) = value {
                    values.write(feathrise_editor_ui::NumericInputSetValue {
                        entity,
                        value: feathrise_editor_ui::NumericValue::F32(value),
                        cancel_edit: true,
                    });
                }
            } // 左拖只在松手写偏好，因此Esc用原偏好取消公共捕获，不形成保存。
            keyboard.clear();
            state.owns_mouse_this_frame = true;
            return;
        }
        let close = keyboard.just_pressed(KeyCode::Escape) && !numeric_editing;
        let cancel_listening = close && state.shortcuts && state.listening.is_some();
        // 切页或关闭优先取消录入，不能把同帧迟到的按键写进即将离开的页面。
        let navigating = buttons.iter().any(|(button, interaction)| {
            interaction.is_clicked()
                && matches!(
                    button.0,
                    EditorSettingsAction::Open
                        | EditorSettingsAction::OpenShortcuts
                        | EditorSettingsAction::Close
                )
        });
        if editable && state.shortcuts && (!navigating || close) {
            super::shortcut_settings::capture_shortcut(&keyboard, &mut state, &mut settings);
        }
        if !numeric_editing {
            keyboard.clear();
        } // 数值控件拥有字符/Esc；其余模态帧阻止底层快捷键。
        if close {
            if cancel_listening {
                state.owns_mouse_this_frame = true;
                return; // 录入中的Esc只取消本次改键；下一次Esc才关闭面板。
            }
            state.open = false;
            state.listening = None;
            state.owns_mouse_this_frame = true;
            return;
        }
    }
    for (button, interaction) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        // 无工程偏好交接期间仍消费原点击与键盘，但只允许打开、切页和关闭，不积压设置动作。
        if !editable && !matches!(button.0,
            EditorSettingsAction::Open | EditorSettingsAction::OpenShortcuts | EditorSettingsAction::Close) {
            continue;
        }
        if matches!(
            button.0,
            EditorSettingsAction::Open | EditorSettingsAction::OpenShortcuts
        ) {
            // 模态本身也持有鼠标门，内部切页不能被自己的门挡住；仅外部打开要等作者手势结束。
            if (!state.open && guard.blocked()) || editor.new_map_dialog_open {
                continue;
            }
            let shortcuts = button.0 == EditorSettingsAction::OpenShortcuts;
            if !state.open || state.shortcuts != shortcuts {
                state.shortcuts = shortcuts;
                state.listening = None;
                state.shortcut_notice.clear();
                state.shortcut_notice_kind = EditorShortcutNoticeKind::Idle;
            }
            state.open = true;
            if let Some(focus) = focus.as_mut()
                && focus.get().is_some_and(|entity| fields.contains(entity))
            {
                focus.clear();
            }
        } else if state.open {
            if scrub.as_ref().is_some_and(|scrub| scrub.entity.is_some()) {
                continue;
            }
            match button.0 {
                EditorSettingsAction::Close => {
                    state.open = false;
                    state.listening = None;
                }
                action @ (EditorSettingsAction::RebindShortcut(_)
                | EditorSettingsAction::UnbindShortcut(_)
                | EditorSettingsAction::ResetShortcuts) => {
                    if state.shortcuts {
                        super::shortcut_settings::apply_shortcut_button(
                            action,
                            &mut state,
                            &mut settings,
                        );
                    }
                }
                EditorSettingsAction::SetColorTheme(theme) => {
                    if state.shortcuts {
                        continue;
                    }
                    if settings.editor_color_theme != theme {
                        settings.editor_color_theme = theme; // 重选不触发保存，不接触作者草稿。
                    }
                }
                EditorSettingsAction::ToggleTerrainBackfaces => {
                    if state.shortcuts {
                        continue;
                    }
                    settings.editor_show_terrain_backfaces =
                        !settings.editor_show_terrain_backfaces;
                }
                EditorSettingsAction::ResetLayout => {
                    if state.shortcuts {
                        continue;
                    }
                    let defaults = EditorLayoutPreferences::default();
                    layout.apply_preferences(defaults); // 即使偏好已默认，也要退出当前的临时专注/拖动。
                    if settings.editor_layout != defaults {
                        settings.editor_layout = defaults;
                    }
                }
                EditorSettingsAction::ResetSnapping => {
                    if state.shortcuts {
                        continue;
                    }
                    let defaults = feathrise_editor_core::EditorSnapSettings::default();
                    if settings.editor_snapping != defaults {
                        settings.editor_snapping = defaults;
                    }
                }
                EditorSettingsAction::SetLocale(locale) => {
                    // 刷新文案由 UI 系统完成；保存仍使用当前实例持有的个人偏好写入槽。
                    if !state.shortcuts && settings.ui_locale != locale {
                        settings.ui_locale = locale;
                    }
                }
                EditorSettingsAction::SetAutoScale => {
                    if state.shortcuts {
                        continue;
                    }
                    if !settings.editor_ui_scale_auto {
                        settings.editor_ui_scale_auto = true; // 自动档只改变模式，保留作者此前选定的手动倍率。
                    }
                }
                action @ (EditorSettingsAction::DecreaseScale
                | EditorSettingsAction::IncreaseScale
                | EditorSettingsAction::SetScale(_)) => {
                    if state.shortcuts {
                        continue;
                    }
                    let (width, height) = windows.single().map_or((0.0, 0.0), |window| {
                        (window.resolution.width(), window.resolution.height())
                    });
                    let current = effective_editor_ui_scale(
                        settings.editor_ui_scale,
                        settings.editor_ui_scale_auto,
                        settings.editor_ui_scale_auto_offset,
                        width,
                        height,
                    );
                    let next = match action {
                        EditorSettingsAction::DecreaseScale => current - 0.05,
                        EditorSettingsAction::IncreaseScale => current + 0.05,
                        EditorSettingsAction::SetScale(percent) => percent as f32 / 100.0,
                        _ => current,
                    };
                    let adjusting_auto = settings.editor_ui_scale_auto
                        && matches!(
                            action,
                            EditorSettingsAction::DecreaseScale
                                | EditorSettingsAction::IncreaseScale
                        );
                    let next = (next * 100.0).round() / 100.0;
                    let next = if adjusting_auto {
                        sanitize_editor_auto_ui_scale(next)
                    } else {
                        sanitize_editor_ui_scale(next)
                    };
                    if adjusting_auto {
                        let base = effective_editor_ui_scale(
                            settings.editor_ui_scale,
                            true,
                            0.0,
                            width,
                            height,
                        );
                        // 从当前可见倍率求偏移，边界处多点加减不累计不可见调整，反向点击立即生效。
                        let offset = sanitize_editor_ui_scale_auto_offset(
                            ((next - base) * 100.0).round() / 100.0,
                        );
                        if next.to_bits() != current.to_bits()
                            && settings.editor_ui_scale_auto_offset.to_bits() != offset.to_bits()
                        {
                            settings.editor_ui_scale_auto_offset = offset;
                        }
                    } else if settings.editor_ui_scale.to_bits() != next.to_bits()
                        || settings.editor_ui_scale_auto
                    {
                        settings.editor_ui_scale = next;
                        settings.editor_ui_scale_auto = false; // 固定百分比和重置缩放选择手动档，自动微调仍被记住。
                    }
                }
                _ => {}
            }
        } else {
            continue;
        }
        state.owns_mouse_this_frame = true;
        keyboard.clear();
        break;
    }
}

pub(crate) fn sync_settings_panel(
    state: Res<MapEditorSettingsState>,
    settings: Res<EditorSettings>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    windows: Query<Ref<Window>>,
    mut panels: Query<(&mut Node, Option<&mut FocusPolicy>), With<MapEditorSettingsPanel>>,
    mut pages: Query<
        (&mut Node, Option<&SettingsGeneralOnly>),
        (
            Or<(
                With<SettingsGeneralOnly>,
                With<SettingsShortcutsOnly>,
                With<MapEditorShortcutSettingsPanel>,
            )>,
            Without<MapEditorSettingsPanel>,
        ),
    >,
    mut labels: Query<
        (
            &mut Text,
            Option<&MapEditorSettingsScaleText>,
            Option<&SettingsThemeText>,
        ),
        Or<(With<MapEditorSettingsScaleText>, With<SettingsThemeText>)>,
    >,
    mut buttons: Query<(
        &MapEditorSettingsButton,
        &UiInteraction,
        &mut BackgroundColor,
        &mut BorderColor,
        Option<&SettingsUtilityButton>,
        Option<&SettingsPageButton>,
        Option<&mut feathrise_editor_ui::ButtonColors>,
    )>,
) {
    let window = windows.single().ok();
    let window_changed = window.as_ref().is_some_and(|window| window.is_changed());
    let (width, height) = window.as_ref().map_or((0.0, 0.0), |window| {
        (window.resolution.width(), window.resolution.height())
    });
    let scale = effective_editor_ui_scale(
        settings.editor_ui_scale,
        settings.editor_ui_scale_auto,
        settings.editor_ui_scale_auto_offset,
        width,
        height,
    );
    for (mut panel, focus) in &mut panels {
        let display = if state.open {
            Display::Flex
        } else {
            Display::None
        };
        if panel.display != display {
            panel.display = display;
        }
        if let Some(mut focus) = focus {
            focus.set_if_neq(if display == Display::Flex {
                FocusPolicy::Block
            } else {
                FocusPolicy::Pass
            });
        }
    }
    for (mut page, general) in &mut pages {
        let display = if state.open && state.shortcuts == general.is_none() {
            Display::Flex
        } else {
            Display::None
        };
        if page.display != display {
            page.display = display;
        }
    }
    if settings.is_changed()
        || state.is_changed()
        || locale.as_ref().is_some_and(|locale| locale.is_changed())
        || (settings.editor_ui_scale_auto && window_changed)
    {
        let locale = locale.as_deref().copied().unwrap_or_default();
        for (mut label, scale_marker, theme_marker) in &mut labels {
            let text = if scale_marker.is_some() {
                if settings.editor_ui_scale_auto {
                    locale.format("settings.scale.auto_value", &[("percent", format!("{:.0}", scale * 100.0))])
                } else {
                    format!("{:.0}%", scale * 100.0)
                }
            } else if theme_marker.is_some() {
                let theme = match settings.editor_color_theme {
                    EditorColorTheme::NeutralGray => "中性深灰",
                    EditorColorTheme::GraphiteBlue => "石墨灰＋雾蓝",
                    EditorColorTheme::WarmCopper => "暖墨铜金",
                };
                locale.format("settings.theme.current", &[("theme", locale.editor_label(theme).into())])
            } else {
                String::new()
            };
            if label.as_str() != text {
                **label = text;
            }
        }
    }
    for (button, interaction, mut background, mut border, utility, page, button_colors) in &mut buttons {
        if button.0 == EditorSettingsAction::Close && utility.is_none() {
            continue;
        }
        // 重置与关闭只显示交互反馈，不冒充选项；原关闭遮罩保留透明背景。
        let selected = utility.is_none()
            && match button.0 {
                EditorSettingsAction::Open => state.open && (page.is_none() || !state.shortcuts),
                EditorSettingsAction::OpenShortcuts => state.open && state.shortcuts,
                EditorSettingsAction::RebindShortcut(action) => {
                    state.open && state.shortcuts && state.listening == Some(action)
                }
                EditorSettingsAction::SetScale(percent) => {
                    !settings.editor_ui_scale_auto && (scale * 100.0 - percent as f32).abs() < 0.1
                }
                EditorSettingsAction::SetAutoScale => settings.editor_ui_scale_auto,
                EditorSettingsAction::SetLocale(locale) => settings.ui_locale == locale,
                EditorSettingsAction::SetColorTheme(theme) => settings.editor_color_theme == theme,
                EditorSettingsAction::ToggleTerrainBackfaces => {
                    settings.editor_show_terrain_backfaces
                }
                _ => false,
            };
        // 操作按钮与选项共用UI库配色，回显不能把标题/底栏的公共反馈改回旧样式。
        let style = feathrise_editor_ui::dialog_button_style(selected);
        let palette = style.colors;
        if let Some(mut colors) = button_colors { colors.set_if_neq(palette); }
        let color = match *interaction {
            UiInteraction::Pressed => palette.pressed,
            UiInteraction::Hovered => palette.hovered,
            UiInteraction::None => palette.normal,
        };
        if background.0 != color {
            background.0 = color;
        }
        let wanted = BorderColor::all(if selected && matches!(button.0, EditorSettingsAction::RebindShortcut(_)) {
            // 只有真正录入中的键位表达输入焦点；普通选中选项与其他对话框一样只改变填充。
            colors::INPUT_ACTIVE_BORDER
        } else {
            style.border_color.unwrap_or(Color::NONE)
        });
        if *border != wanted {
            *border = wanted;
        }
    }
}

/// 只滚动设置正文；像素消息按实际UI/DPI换算，重开归顶但修改偏好不抢走位置。
pub(crate) fn scroll_settings_body(
    mut wheels: MessageReader<MouseWheel>,
    editor: Res<MapEditorState>,
    state: Res<MapEditorSettingsState>,
    mut was_open: Local<bool>,
    mut bodies: Query<
        (&RelativeCursorPosition, &ComputedNode, &mut ScrollPosition),
        With<SettingsBody>,
    >,
) {
    let (lines, pixels) = wheels.read().filter(|event| event.y.is_finite()).fold(
        (0.0, 0.0),
        |(lines, pixels), event| match event.unit {
            MouseScrollUnit::Line => (lines + event.y, pixels),
            MouseScrollUnit::Pixel => (lines, pixels + event.y),
        },
    );
    let visible = editor.open && state.open && !state.shortcuts;
    let reopened = visible && !*was_open;
    *was_open = visible;
    for (cursor, node, mut position) in &mut bodies {
        if !visible || reopened {
            if position.x != 0.0 || position.y != 0.0 {
                *position = ScrollPosition::default();
            }
            continue; // 隐藏时仍消费滚轮，不能在下一次打开或另一设置页重放。
        }
        if node.size().min_element() <= 0.0 {
            continue;
        }
        let maximum =
            ((node.content_size().y - node.size().y) * node.inverse_scale_factor()).max(0.0);
        let delta = if cursor.cursor_over() {
            lines * 24.0 + pixels * node.inverse_scale_factor()
        } else {
            0.0
        };
        let wanted = (position.y - delta).clamp(0.0, maximum);
        if position.y != wanted {
            position.y = wanted;
        }
    }
}

#[cfg(test)]
#[path = "editor_settings_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "editor_settings_layout_tests.rs"]
mod layout_tests;

#[cfg(test)]
#[path = "editor_settings_gpu_tests.rs"]
mod gpu_tests;

#[cfg(test)]
#[path = "editor_settings/layout_tests.rs"]
mod snapping_layout_tests;

#[cfg(test)]
#[path = "editor_settings/host_access_tests.rs"]
mod host_access_tests;
