//! 中央水体文档工作台：固定页头、顶部四分区与弹性正文，沿用共享标签身份。

use bevy::prelude::*;

use super::components::{
    WaterButtonKind, WaterDocumentBadge, WaterDocumentHeader, WaterEditorForm,
    WaterEditorNavigation,
};
use super::types::{
    WaterDraft, WaterDraftMode, WaterPresetAction, WaterPresetButton, WaterPresetEditorState,
    WaterPresetStatusText, WaterSection, WaterTab,
};
use super::widgets::{action_button, readonly, row, text_font};
use crate::ui::theme as colors;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceDynamicRoot, PresetWorkspaceTab};
use crate::ui::workspace_tabs::spawn_preset_tab;

pub(crate) fn spawn_water_tabs(
    tabs: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &WaterPresetEditorState,
    active_tab: PresetWorkspaceTab,
) {
    for tab in &state.tabs {
        let active = active_tab == PresetWorkspaceTab::Water(tab.id);
        let dirty = if tab.draft.dirty() { "*" } else { "" };
        spawn_preset_tab(
            tabs,                                                      // 水体继续共享同一横向标签容器。
            font,                                                      // 使用工作区统一中文字体。
            &format!("水体·{}{}", tab.draft.descriptor.label, dirty), // 完整名称决定标题宽度，不截断字形。
            WaterPresetButton(WaterPresetAction::ActivateTab(tab.id)), // 保留原有活动标签切换。
            WaterPresetButton(WaterPresetAction::CloseTab(tab.id)), // 保留未保存与忙碌时的关闭保护。
            active,                 // 当前标签显示统一铜色活动外框。
            tab.close_confirmation, // 二次确认仍显式回显。
        );
    }
}

/// 共享工作区只传会话身份，不借用或修改水体的作者描述。
pub(crate) fn spawn_active_water_editor(
    commands: &mut Commands,
    host: Entity,
    font: &Handle<Font>,
    state: &WaterPresetEditorState,
    active: PresetWorkspaceTab,
) -> bool {
    let PresetWorkspaceTab::Water(id) = active else {
        return false;
    };
    let Some(tab) = state.tabs.iter().find(|tab| tab.id == id) else {
        return false;
    };
    commands.entity(host).with_children(|parent| {
        parent
            .spawn((
                PresetWorkspaceDynamicRoot,
                Node {
                    width: Val::Percent(100.0),
                    min_width: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ))
            .with_children(|root| {
                header(root, font, state, tab);
                root.spawn((
                    WaterEditorNavigation,
                    Node {
                        width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(7.0),
                        row_gap: Val::Px(4.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ))
                .with_children(|navigation| {
                    for (label, section) in [
                        ("资料与水色", WaterSection::Appearance),
                        ("宏观波浪", WaterSection::Waves),
                        ("微法线", WaterSection::Normals),
                        ("材质与岸边", WaterSection::Material),
                    ] {
                        action_button(
                            navigation,
                            font,
                            label,
                            WaterPresetAction::Section(section),
                            WaterButtonKind::Navigation,
                            tab.section == section,
                        );
                    }
                }); // 分区永远位于正文上方，不抢占左侧浏览器宽度。
                root.spawn((
                    WaterEditorForm,
                    Node {
                        width: Val::Percent(100.0),
                        max_width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ))
                .with_children(|body| super::form::spawn_fields(body, font, state, tab));
            });
    });
    true
}

fn header(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &WaterPresetEditorState,
    tab: &WaterTab,
) {
    parent
        .spawn((
            WaterDocumentHeader,
            Node {
                width: Val::Percent(100.0),
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(7.0),
                padding: UiRect::bottom(Val::Px(8.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(colors::COPPER_BORDER_WEAK),
        ))
        .with_children(|header| {
            row(header, |actions| {
                actions.spawn((
                    Text::new(&tab.draft.descriptor.label),
                    text_font(font, 16.0),
                    TextColor(colors::TEXT_PRIMARY),
                    TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                    Node {
                        min_width: Val::Px(0.0),
                        flex_basis: Val::Px(240.0),
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        ..default()
                    },
                ));
                actions.spawn((
                    WaterDocumentBadge,
                    Text::new(draft_badge(&tab.draft, state.busy())),
                    text_font(font, 11.0),
                    TextColor(if tab.draft.dirty() {
                        colors::TEXT_ACCENT
                    } else {
                        colors::TEXT_MUTED
                    }),
                    Node {
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
                action_button(
                    actions,
                    font,
                    "保存水体",
                    WaterPresetAction::Save,
                    WaterButtonKind::Primary,
                    false,
                );
                action_button(
                    actions,
                    font,
                    "关闭标签",
                    WaterPresetAction::Cancel,
                    WaterButtonKind::Ordinary,
                    false,
                );
            });
            header.spawn((
                WaterPresetStatusText,
                crate::ui::status_bar::PanelStatusText("静水预设"),
                feathrise_editor_ui::feedback::GlobalPanelStatus,
                Text::new(&state.status),
                text_font(font, 10.5),
                TextColor(colors::TEXT_VALUE),
                TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                Node {
                    width: Val::Percent(100.0),
                    min_width: Val::Px(0.0),
                    ..default()
                },
            )); // 保存/采用失败与关闭确认留在页头，不再只能到左栏找诊断。
            readonly(
                header,
                font,
                "编辑已有预设时，合法数值实时更新右侧独立样品；保存并热采用后才更新场景水面。",
            );
        });
}

pub(super) fn draft_badge(draft: &WaterDraft, busy: bool) -> &'static str {
    if busy {
        "保存 / 热采用中"
    } else if matches!(draft.mode, WaterDraftMode::Create) {
        "新建 · 未保存"
    } else if draft.dirty() {
        "未保存"
    } else {
        "已保存"
    }
}
