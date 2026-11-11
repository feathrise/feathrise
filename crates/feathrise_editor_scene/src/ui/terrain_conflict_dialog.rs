//! Blender 与 F12 地形并发修改的全屏模态选择。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::EditorButtonClick;
use feathrise_editor_ui::metrics as ui_metrics;
use bevy::prelude::*; // BSN场景、按钮、文本和增量显隐。
use bevy::text::FontSourceTemplate; // 使用项目中文字体。
use bevy::ui::{FocusPolicy, RelativeCursorPosition}; // 模态命中与两个独立滚动区。
use crate::ui::theme as colors; // 复用编辑器暗色与铜边。

use crate::authoring::{TerrainConflictResolution, TerrainExternalChangeState};
use crate::components::{SceneEditorActionButton, TerrainConflictDialog, TerrainConflictDialogText};
use crate::resources::SceneEditorState;
use crate::types::EditorAction;

use super::management_dialogs::{DialogKind, components::*, scene as modal};
use super::scene::FONT_PATH;

pub(super) fn terrain_conflict_dialog() -> impl Scene {
    bsn! {
        TerrainConflictDialog UiInteraction::default()
        Node {
            display: Display::None, position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: percent(100), height: percent(100),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            padding: UiRect::all(px(8)),
        }
        GlobalZIndex(450)
        BackgroundColor(colors::OVERLAY_BG)
        Children [
            ManagementPanel(DialogKind::TerrainConflict)
            ~{modal::panel_node()}
            BackgroundColor(colors::PANEL_BG_STRONG)
            BorderColor::all(colors::COPPER_BORDER_WEAK)
            Children [
                @modal::heading("地形文件发生冲突")--

                    ManagementScroll(DialogKind::TerrainConflict)
                    ScrollPosition::default() RelativeCursorPosition::default()
                    ~{modal::body_node()}
                    Children [
                        @modal::caption("冲突目标 · 外部版本已变化")--

                            TerrainConflictDialogText Text("等待外部地形版本") @feathrise_editor_ui::localization::label_scene("等待外部地形版本")
                            TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                            TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                            TextColor(colors::TEXT_PRIMARY)
                            Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                        --
                        @modal::caption("采用 Blender")--
                        @modal::caption("放弃地形草稿，重新加载并验证外部地形。")--
                        @modal::caption("使用草稿覆盖")--
                        @modal::caption("保留当前顶点并覆盖本次检测到的外部版本。外部文件再次变化时必须重新处理冲突，不能沿用旧授权。")
                    ]
                --

                    ManagementScroll(DialogKind::TerrainConflict) ManagementFeedback(DialogKind::TerrainConflict)
                    ScrollPosition::default() RelativeCursorPosition::default()
                    ~{modal::feedback_node()}
                    BackgroundColor(colors::PANEL_BG_SECTION)
                    Children [
                        TerrainConflictFeedbackText Text("地形草稿尚未保存，请选择保留哪一版。") @feathrise_editor_ui::localization::label_scene("地形草稿尚未保存，请选择保留哪一版。")
                        TextLayout::linebreak(bevy::text::LineBreak::AnyCharacter)
                        TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(ui_metrics::FONT_CONTROL) }
                        TextColor(colors::TEXT_PRIMARY)
                        Node { width: percent(100), min_width: px(0), flex_shrink: 0.0 }
                    ]
                --

                    ManagementFooter(DialogKind::TerrainConflict)
                    ~{modal::footer_node()}
                    // 原协议必须显式二选一；不虚构“取消”来解除真实冲突保护。
                    Children [
                        @modal::button("采用 Blender", EditorAction::AdoptBlenderTerrain)--
                        @modal::button("使用草稿覆盖", EditorAction::OverwriteBlenderTerrain)
                    ]

            ]
        ]
    }
}

pub(crate) fn configure_terrain_conflict_dialog_focus(
    mut commands: Commands,
    dialogs: Query<Entity, Added<TerrainConflictDialog>>,
) {
    for entity in &dialogs {
        commands.entity(entity).insert(FocusPolicy::Pass);
    }
}

pub(crate) fn sync_terrain_conflict_dialog(
    editor: Res<SceneEditorState>,
    conflict: Res<TerrainExternalChangeState>,
    mut dialogs: Query<
        (Entity, &mut Node, &mut UiInteraction, &mut FocusPolicy),
        With<TerrainConflictDialog>,
    >,
    children: Query<&Children>,
    mut descendant_interactions: Query<&mut UiInteraction, Without<TerrainConflictDialog>>,
    mut texts: Query<
        (&mut Text, Has<TerrainConflictFeedbackText>),
        Or<(
            With<TerrainConflictDialogText>,
            With<TerrainConflictFeedbackText>,
        )>,
    >,
) {
    if !(editor.is_changed() || conflict.is_changed()) {
        return;
    }
    let visible = editor.open && conflict.modal_open();
    for (entity, mut node, mut interaction, mut focus) in &mut dialogs {
        node.display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        *focus = if visible {
            FocusPolicy::Block
        } else {
            FocusPolicy::Pass
        };
        if !visible {
            *interaction = UiInteraction::None;
            for descendant in children.iter_descendants(entity) {
                if let Ok(mut descendant) = descendant_interactions.get_mut(descendant) {
                    *descendant = UiInteraction::None;
                }
            }
        }
    }
    let Some(target) = conflict.conflict.as_ref() else {
        return;
    };
    let digest = target.digest[..6]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let message = format!(
        "SceneId：{}\n外部文件：terrain.glb\n检测版本：SHA-256 {}…",
        target.scene_id, digest
    );
    let feedback = match conflict.resolution {
        None => "地形草稿尚未保存，请选择保留哪一版。",
        Some(TerrainConflictResolution::AdoptBlender) => {
            "已选择采用 Blender · 等待后台读取与验证，尚未完成采用。"
        }
        Some(TerrainConflictResolution::OverwriteWithF12) => {
            "已选择使用草稿覆盖，等待保存。"
        }
    }; // 选择仅是请求，不能把按钮按下伪装成采用或保存成功。
    for (mut text, is_feedback) in &mut texts {
        let value = if is_feedback { feedback } else { &message };
        if text.0 != value {
            text.0 = value.to_owned();
        }
    }
}

/// 冲突按钮只冻结选择；实际磁盘读取、采用或保存统一在Tasks阶段执行。
pub(crate) fn handle_terrain_conflict_buttons(
    buttons: Query<(&EditorButtonClick, &SceneEditorActionButton), Changed<EditorButtonClick>>,
    mut conflict: ResMut<TerrainExternalChangeState>,
) {
    if conflict.conflict.is_none() {
        return;
    }
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() {
            continue;
        }
        conflict.resolution = match button.0 {
            EditorAction::AdoptBlenderTerrain => Some(TerrainConflictResolution::AdoptBlender),
            EditorAction::OverwriteBlenderTerrain => {
                Some(TerrainConflictResolution::OverwriteWithF12)
            }
            _ => conflict.resolution,
        };
    }
}
