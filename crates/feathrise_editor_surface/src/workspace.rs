//! 中央地表文档工作台：固定页头、顶部三分区与弹性正文，沿用共享标签身份。

use bevy::prelude::*;

use super::components::{
    SurfaceButtonKind, SurfaceDocumentBadge, SurfaceDocumentHeader, SurfaceEditorForm,
    SurfaceEditorNavigation,
};
use super::types::{
    SurfaceDraft, SurfaceDraftMode, SurfacePresetAction, SurfacePresetEditorState,
    SurfacePresetStatusText, SurfaceSection, SurfaceTab,
};
use super::widgets::{action_button, row, text_font};
use feathrise_editor_core::{PresetWorkspaceDynamicRoot, PresetWorkspaceTab};
use feathrise_editor_ui::colors;

/// 共享工作区只传会话身份，不借用或修改地表的作者描述。
pub(crate) fn spawn_active_surface_editor(
    commands: &mut Commands,
    host: Entity,
    font: &Handle<Font>,
    state: &SurfacePresetEditorState,
    active: PresetWorkspaceTab,
) -> bool {
    let PresetWorkspaceTab::Surface(id) = active else {
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
                    SurfaceEditorNavigation,
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
                        ("资料与纹理", SurfaceSection::Textures),
                        ("材质参数", SurfaceSection::Material),
                        ("映射与混合", SurfaceSection::Mapping),
                    ] {
                        action_button(
                            navigation,
                            font,
                            label,
                            SurfacePresetAction::Section(section),
                            SurfaceButtonKind::Navigation,
                            tab.section == section,
                        );
                    }
                }); // 分区永远位于正文上方，不抢占左侧浏览器宽度。
                root.spawn((
                    SurfaceEditorForm,
                    Node {
                        width: Val::Percent(100.0),
                        max_width: Val::Px(960.0),
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(18.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ))
                .with_children(|body| super::fields::spawn_fields(body, font, state, tab));
            });
    });
    true
}

fn header(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    state: &SurfacePresetEditorState,
    tab: &SurfaceTab,
) {
    parent
        .spawn((
            SurfaceDocumentHeader,
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
                    SurfaceDocumentBadge,
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
                    "保存地表",
                    SurfacePresetAction::Save,
                    SurfaceButtonKind::Primary,
                    false,
                );
                action_button(
                    actions,
                    font,
                    "关闭标签",
                    SurfacePresetAction::Cancel,
                    SurfaceButtonKind::Ordinary,
                    false,
                );
            });
            header.spawn((
                SurfacePresetStatusText,
                feathrise_editor_ui::PanelStatusText("地表预设"),
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
            feathrise_editor_ui::context_help::spawn_help_button(header, font, "地表预设保存",
                "保存会重新处理作者源并重建 PBR 数组；完整预热后才整体切换，期间保留上一有效材质。");
        });
}

pub(super) fn draft_badge(draft: &SurfaceDraft, busy: bool) -> &'static str {
    if busy {
        "保存 / 热采用中"
    } else if matches!(draft.mode, SurfaceDraftMode::Create) {
        "新建 · 未保存"
    } else if draft.dirty() {
        "未保存"
    } else {
        "已保存"
    }
}
