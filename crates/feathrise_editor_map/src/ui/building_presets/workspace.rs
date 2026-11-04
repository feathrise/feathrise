//! 建筑预设的文档页头、四个顶部分区和共享标签入口；不修改摆放、整平或目录事务。

mod sections;
mod widgets;

use super::components::*;
use super::types::*;
use crate::ui::theme as colors;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceDynamicRoot, PresetWorkspaceTab};
use crate::ui::workspace_tabs::spawn_preset_tab;
use bevy::prelude::*;
use widgets::{button, row, text_font};

/// 把全部打开建筑草稿追加到公共中央标签条。
pub(crate) fn spawn_building_tabs(
    tabs: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &BuildingPresetEditorState,
    active_tab: PresetWorkspaceTab,
) {
    for tab in &state.tabs {
        let active = active_tab == PresetWorkspaceTab::Building(tab.id);
        let dirty = if tab.draft.dirty() { "*" } else { "" };
        spawn_preset_tab(
            tabs, // 建筑与植被、地表、水体共享同一横向标签内容根。
            font, // 使用中央工作区已经加载的中文字体。
            &format!("建筑·{}{}", tab.draft.descriptor.label, dirty), // 类型和脏标记完整显示。
            BuildingPresetButton(BuildingPresetAction::ActivateTab(tab.id)), // 标题只负责激活。
            BuildingPresetButton(BuildingPresetAction::CloseTab(tab.id)), // 小关闭键保留二次确认。
            active, // 当前活动标签使用统一强铜边。
            tab.close_confirmation, // 脏草稿第一次关闭后显示“确认×”。
        );
    }
}

/// 只为当前建筑标签构造一页正文，隐藏分区不生成输入控件。
pub(crate) fn spawn_active_building_editor(
    commands: &mut Commands,
    host: Entity,
    font: &Handle<Font>,
    state: &BuildingPresetEditorState,
    active_tab: PresetWorkspaceTab,
) -> bool {
    let Some(draft) = state.active_draft(active_tab) else {
        return false;
    };
    commands.entity(host).with_children(|parent| {
        parent
            .spawn((
                PresetWorkspaceDynamicRoot,
                Node {
                    width: Val::Percent(100.0),
                    min_width: Val::Px(0.0),
                    max_width: Val::Px(960.0),
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.0,
                    row_gap: Val::Px(12.0),
                    ..default()
                }, // 阅读宽度只作用于文档，不增加相机或第二套滚动宿主。
            ))
            .with_children(|root| {
                root.spawn((
                    BuildingDocumentHeader,
                    Node {
                        width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(6.0),
                        ..default()
                    },
                ))
                .with_children(|header| {
                    row(header, |actions| {
                        actions.spawn((
                            Text::new(&draft.descriptor.label),
                            text_font(font, 18.0),
                            TextColor(colors::TEXT_PRIMARY),
                            TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                            Node {
                                flex_basis: Val::Px(260.0),
                                flex_grow: 1.0,
                                min_width: Val::Px(0.0),
                                max_width: Val::Percent(100.0),
                                ..default()
                            },
                        ));
                        button(
                            actions,
                            font,
                            "保存预设",
                            BuildingPresetAction::Save,
                            BuildingButtonKind::Primary,
                            false,
                        );
                        button(
                            actions,
                            font,
                            "恢复基线",
                            BuildingPresetAction::Reset,
                            BuildingButtonKind::Ordinary,
                            false,
                        );
                        button(
                            actions,
                            font,
                            "关闭标签",
                            BuildingPresetAction::Cancel,
                            BuildingButtonKind::Ordinary,
                            false,
                        );
                    });
                    header.spawn((
                        BuildingDocumentBadge,
                        Text::new(draft_badge(draft, state)),
                        text_font(font, 11.0),
                        TextColor(colors::TEXT_MUTED),
                    ));
                    header.spawn((
                        BuildingPresetStatusText,
                        crate::ui::status_bar::PanelStatusText("建筑工具"),
                        Text::new(&state.status),
                        text_font(font, 11.0),
                        TextColor(colors::TEXT_ACCENT),
                        TextLayout::new(Justify::Left, LineBreak::AnyCharacter),
                        Node {
                            width: Val::Percent(100.0),
                            min_width: Val::Px(0.0),
                            ..default()
                        },
                    )); // 错误与关闭确认在页头及原侧栏同时可见，不藏在长表单底部。
                });
                root.spawn((
                    BuildingDocumentNavigation,
                    Node {
                        width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(4.0),
                        row_gap: Val::Px(4.0),
                        ..default()
                    },
                ))
                .with_children(|navigation| {
                    for (section, title) in [
                        (BuildingPresetSection::Basics, "基础资料"),
                        (BuildingPresetSection::Footprint, "占地与地基"),
                        (BuildingPresetSection::Entrance, "门口与净空"),
                        (BuildingPresetSection::Connections, "模块连接"),
                    ] {
                        button(
                            navigation,
                            font,
                            title,
                            BuildingPresetAction::SelectSection(section),
                            BuildingButtonKind::Navigation,
                            draft.section == section,
                        );
                    }
                });
                root.spawn((
                    BuildingDocumentForm,
                    Node {
                        width: Val::Percent(100.0),
                        min_width: Val::Px(0.0),
                        max_width: Val::Px(960.0),
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(12.0),
                        row_gap: Val::Px(10.0),
                        align_items: AlignItems::Start,
                        ..default()
                    },
                ))
                .with_children(|form| sections::spawn(form, font, state, draft));
            });
    });
    true
}

/// 页头状态反映原输入和单槽IO/热采用阶段，不把新建且未修改的草稿误称为已保存。
pub(super) fn draft_badge(
    draft: &BuildingDraft,
    state: &BuildingPresetEditorState,
) -> &'static str {
    if state.write_task.is_some() {
        "保存中"
    } else if state.adoption.is_some() {
        "等待热采用"
    } else if state.active_field.is_some() {
        "字段输入中"
    } else if matches!(draft.mode, BuildingDraftMode::Create) {
        "新建未保存"
    } else if draft.dirty() {
        "未保存修改"
    } else {
        "已保存"
    }
}
