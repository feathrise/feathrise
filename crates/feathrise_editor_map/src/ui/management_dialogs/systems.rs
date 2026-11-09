//! 管理弹窗的滚轮会话与专属危险色；只处理固定节点，不读盘或改动作者事务。

use feathrise_editor_ui::UiInteraction;
use super::components::*;
use super::types::ScrollSessions;
use crate::authoring::TerrainExternalChangeState;
use crate::components::MapEditorActionButton;
use crate::resources::{MapBrowserDragState, MapBrowserState, MapDeletionTask, MapEditorState};
use crate::types::{EditorAction, MapFolderDialogMode};
use crate::ui::theme as colors;
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

/// 几何悬停不受遮罩FocusPolicy影响；底层滚动必须另查原模态所有权。
#[derive(SystemParam)]
pub(crate) struct ManagementWheelGuard<'w> {
    map_drag: Res<'w, MapBrowserDragState>,
    browser: Option<Res<'w, MapBrowserState>>,
    deletion: Option<Res<'w, MapDeletionTask>>,
    terrain: Option<Res<'w, TerrainExternalChangeState>>,
}

impl ManagementWheelGuard<'_> {
    pub(crate) fn blocked(&self) -> bool {
        self.map_drag.blocks_other_mouse()
            || self
                .browser
                .as_ref()
                .is_some_and(|state| state.dialog.is_some())
            || self
                .deletion
                .as_ref()
                .is_some_and(|state| state.confirmation.is_some())
            || self
                .terrain
                .as_ref()
                .is_some_and(|state| state.modal_open())
    }
}

pub(super) fn scroll_dialogs(
    mut wheel: MessageReader<MouseWheel>,
    editor: Res<MapEditorState>,
    deletion: Res<MapDeletionTask>,
    browser: Res<MapBrowserState>,
    conflict: Res<TerrainExternalChangeState>,
    mut previous: Local<ScrollSessions>,
    mut panels: Query<(
        &ManagementScroll,
        &RelativeCursorPosition,
        &ComputedNode,
        &mut ScrollPosition,
    )>,
) {
    // 关闭期间仍消费消息；不能在下次打开后重放旧滚轮。
    let delta: f32 = wheel
        .read()
        .map(|event| {
            -event.y
                * match event.unit {
                    MouseScrollUnit::Line => 24.0,
                    MouseScrollUnit::Pixel => 1.0,
                }
        })
        .sum();
    let deletion_id = deletion
        .confirmation
        .as_ref()
        .filter(|_| editor.open)
        .map(|request| &request.map_id);
    let folder = browser.dialog.as_ref().filter(|_| editor.open);
    let terrain = conflict.conflict.as_ref().filter(|_| editor.open);
    let changed = [
        previous.deletion.as_ref() != deletion_id,
        match (&previous.folder, folder) {
            (Some((mode, id)), Some(dialog)) => {
                *mode != dialog.mode || *id != dialog.target_folder_id
            }
            (None, None) => false,
            _ => true,
        },
        match (&previous.terrain, terrain) {
            (Some((id, digest)), Some(target)) => *id != target.map_id || *digest != target.digest,
            (None, None) => false,
            _ => true,
        },
    ];
    // 只在语义身份边沿复制短ID；输入字符、最近错误和稳定帧不重复归顶。
    if changed[0] {
        previous.deletion = deletion_id.cloned();
    }
    if changed[1] {
        previous.folder = folder.map(|dialog| (dialog.mode, dialog.target_folder_id.clone()));
    }
    if changed[2] {
        previous.terrain = terrain.map(|target| (target.map_id.clone(), target.digest));
    }
    let visible = [deletion_id.is_some(), folder.is_some(), terrain.is_some()];
    for (kind, cursor, computed, mut scroll) in &mut panels {
        let index = kind.0.index();
        if !visible[index] || changed[index] {
            if scroll.x != 0.0 || scroll.y != 0.0 {
                *scroll = ScrollPosition::default();
            }
            continue;
        }
        if delta == 0.0 || !cursor.cursor_over() || computed.size().min_element() <= 0.0 {
            continue; // 显隐传播前的旧悬停、零尺寸裁切或其他区的滚轮不能抢占正文。
        }
        let maximum = ((computed.content_size().y - computed.size().y)
            * computed.inverse_scale_factor())
        .max(0.0);
        let next = (scroll.y + delta).clamp(0.0, maximum);
        if scroll.y != next {
            scroll.y = next;
        }
    }
}

pub(super) fn sync_button_styles(
    browser: Res<MapBrowserState>,
    mut buttons: Query<
        (
            Ref<UiInteraction>,
            Option<Ref<RelativeCursorPosition>>,
            &MapEditorActionButton,
            &Children,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<ManagementButton>,
    >,
    mut texts: Query<&mut TextColor, With<ManagementButtonText>>,
) {
    for (interaction, cursor, action, children, mut background, mut border) in &mut buttons {
        if !interaction.is_changed() && !browser.is_changed()
            && cursor.as_ref().is_none_or(|cursor| !cursor.is_changed()) {
            continue;
        }
        let interaction = if *interaction == UiInteraction::Pressed
            && cursor.as_ref().is_some_and(|cursor| !cursor.cursor_over()) {
            UiInteraction::None
        } else { *interaction };
        let danger = matches!(
            action.0,
            EditorAction::ConfirmDeleteMap | EditorAction::ConfirmDeleteMapPermanently | EditorAction::OverwriteBlenderTerrain
        ) || action.0 == EditorAction::ConfirmMapFolderDialog
            && browser
                .dialog
                .as_ref()
                .is_some_and(|dialog| dialog.mode == MapFolderDialogMode::Delete);
        let (fill, edge, ink) = if danger {
            match interaction {
                UiInteraction::Pressed => (
                    colors::DANGER_BG, // 保留危险语义底色，按压亮度由编辑器统一反馈层产生。
                    colors::DANGER_TEXT,
                    colors::DANGER_TEXT,
                ),
                UiInteraction::Hovered => {
                    (colors::DANGER_BG, colors::DANGER_TEXT, colors::DANGER_TEXT)
                }
                UiInteraction::None => (
                    colors::DANGER_BG,
                    colors::DANGER_BORDER,
                    colors::DANGER_TEXT,
                ),
            }
        } else {
            let fill = match interaction {
                UiInteraction::Pressed => colors::BUTTON_DEV_SMALL_PRESSED,
                UiInteraction::Hovered => colors::BUTTON_DEV_SMALL_HOVERED,
                UiInteraction::None => colors::BUTTON_DEV_SMALL_NORMAL,
            };
            (fill, colors::COPPER_BORDER_WEAK, colors::TEXT_PRIMARY)
        };
        background.set_if_neq(BackgroundColor(fill));
        border.set_if_neq(BorderColor::all(edge));
        for child in children.iter() {
            if let Ok(mut text) = texts.get_mut(child) {
                text.set_if_neq(TextColor(ink));
            }
        }
    }
}
