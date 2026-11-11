//! 场景与整棵文件夹子树的拖拽、落点校验和单次虚拟归属提交。

use feathrise_editor_ui::UiInteraction;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use bevy::window::{PrimaryWindow, Window};
use feathrise_scene_content::SceneContentRegistry;

use crate::components::{SceneEditorSceneRootRow, SceneEditorSceneRow};
use crate::resources::{
    SceneBrowserDragState, SceneBrowserState, SceneCreationTask, SceneDeletionTask, SceneEditorState,
    SceneFolderPersistence,
};
use crate::types::{SceneBrowserDragPress, SceneBrowserDropTarget, SceneBrowserItem, SceneFolderDocument};

use super::{structure_change_allowed, toggle_folder_expansion};
use crate::scene_browser::helpers::{
    assign_scene_to_folder, drag_threshold_reached, expand_folder_ancestors, folder_by_id,
    folder_move_target_allowed, move_folder,
};
use crate::scene_browser::persistence::begin_scene_folder_save;

/// 一次释放只生成一份完整目标；后台事务成功前保留原保存层的完整回滚快照。
struct PreparedDrop {
    document: SceneFolderDocument,
    selected: SceneBrowserItem,
    expanded: std::collections::BTreeSet<String>,
}

fn prepare_drop(
    browser: &SceneBrowserState,
    press: &SceneBrowserDragPress,
    target: &SceneBrowserDropTarget,
) -> Result<Option<PreparedDrop>, String> {
    if press.document_revision != browser.document_revision {
        return Err("场景归类已更新，拖拽已取消".to_owned());
    }
    let parent = match target {
        SceneBrowserDropTarget::Folder(id) => Some(id.as_str()),
        SceneBrowserDropTarget::Root => None,
    };
    let previous = match &press.item {
        SceneBrowserItem::Scene(id) => browser
            .document
            .scene_folders
            .get(id.as_str())
            .map(String::as_str),
        SceneBrowserItem::Folder(id) => folder_by_id(&browser.document, id)
            .ok_or_else(|| "待移动的文件夹已不存在".to_owned())?
            .parent_id
            .as_deref(),
        SceneBrowserItem::Root => return Err("根目录不能移动".to_owned()),
    };
    if previous == parent {
        return Ok(None); // 放回原目录不克隆文档、不写盘。
    }
    let mut document = browser.document.clone(); // 仅在真正释放到新目录时复用原有一次性事务快照。
    match &press.item {
        SceneBrowserItem::Scene(id) => assign_scene_to_folder(&mut document, id, parent)?,
        SceneBrowserItem::Folder(id) => {
            move_folder(&mut document, id, parent)?;
        }
        SceneBrowserItem::Root => unreachable!(),
    }
    let mut expanded = browser.expanded_folder_ids.clone();
    expand_folder_ancestors(&document, parent, &mut expanded); // 保留源子树展开状态，并展开落点父链让移动项可见。
    Ok(Some(PreparedDrop {
        document,
        selected: press.item.clone(),
        expanded,
    }))
}

pub(super) fn row_is_hovered(
    interaction: &UiInteraction,
    cursor: Option<&RelativeCursorPosition>,
) -> bool {
    matches!(interaction, UiInteraction::Hovered | UiInteraction::Pressed)
        && cursor.map_or(
            *interaction == UiInteraction::Hovered,
            RelativeCursorPosition::cursor_over,
        )
} // Bevy会保留移出后的Pressed，必须结合真实光标位置，否则源文件夹会一直遮住其他落点。

/// 更新拖拽阈值、有效落点和松开后的单次归属保存；稳定悬停复用父链检查结果。
#[allow(clippy::too_many_arguments)] // 拖拽release帧必须同时冻结物理输入、落点、任务互斥和完整回滚状态。
pub(crate) fn update_scene_browser_drag(
    mouse: Res<ButtonInput<MouseButton>>, // 左键按住、松开和release帧隔离。
    windows: Query<&Window, With<PrimaryWindow>>, // 物理阈值位置与窗口焦点；浮层在布局后独立定位。
    roots: Query<&UiInteraction, With<SceneEditorSceneRootRow>>, // 固定根目录行接受场景与文件夹移回根级。
    rows: Query<(
        &UiInteraction,
        &SceneEditorSceneRow,
        Option<&RelativeCursorPosition>,
    )>, // 当前悬停的任意可见文件夹行。
    mut drag: ResMut<SceneBrowserDragState>, // 候选、活动、落点与延迟释放门。
    mut browser: ResMut<SceneBrowserState>, // 目标归属的乐观文档。
    mut persistence: ResMut<SceneFolderPersistence>, // 唯一原子保存和回滚槽。
    mut editor: ResMut<SceneEditorState>,   // 场景选择和状态。
    creation: Res<SceneCreationTask>,       // 创建场景期间不得分类。
    deletion: Res<SceneDeletionTask>,       // 回收场景期间不得分类。
    registry: Option<Res<SceneContentRegistry>>,
    index: Option<Res<crate::resources::SceneBrowserIndexState>>,
    mut context: crate::ui::scene_sidebar::SceneSidebarInputContext,
    mut validation: Local<
        Option<(
            u64,
            SceneBrowserItem,
            SceneBrowserDropTarget,
            Result<(), String>,
        )>,
    >,
) {
    if !editor.open
        || !context.visible()
        || editor.close_requested
        || editor.new_scene_dialog_open
        || browser.dialog.is_some()
        || editor.scene_switch_confirmation.is_some()
        || editor.scene_transition_in_progress
        || editor.pending_scene_transition.is_some()
        || deletion.confirmation.is_some()
    {
        let owned = drag.press.is_some() || drag.active || drag.blocked;
        drag.press = None; // 隐藏或模态介入后，未越阈值的文件夹点击也不能补展开。
        drag.selection = None;
        drag.active = false; // 浮层立即隐藏。
        drag.drop_target = None; // 旧文件夹落点失效。
        drag.drop_error = None;
        drag.blocked = editor.open
            && owned
            && (mouse.pressed(MouseButton::Left)
                || mouse.pressed(MouseButton::Right)
                || mouse.pressed(MouseButton::Middle)
                || mouse.just_released(MouseButton::Left)
                || mouse.just_released(MouseButton::Right)
                || mouse.just_released(MouseButton::Middle)); // 中断手势仍等待所有按键归还。
        *validation = None;
        return;
    }
    let Ok(window) = windows.single() else {
        return; // 没有唯一主窗口时不推进坐标相关交互。
    };
    if let Some(press) = drag.press.as_ref() {
        let error = if press.document_revision != browser.document_revision {
            Some("场景归类已更新，拖拽已取消".to_owned())
        } else if drag.catalog_revision.is_some_and(|revision| {
            revision != registry.as_ref().map_or(0, |registry| registry.revision)
        }) {
            Some("场景目录已更新，拖拽已取消".to_owned())
        } else if !window.focused {
            Some("窗口已失焦，拖拽已取消".to_owned())
        } else {
            structure_change_allowed(&browser, &persistence, &creation, &deletion).err()
        };
        if let Some(error) = error {
            drag.blocked = true; // 阈值内的旧点击也须等待这次物理按键释放，不能改选后补执行。
            drag.press = None;
            drag.selection = None;
            drag.active = false;
            drag.drop_target = None;
            drag.drop_error = None;
            editor.status = error;
            if let Some(feedback) = context.feedback.as_deref_mut() {
                feedback.record(&editor, &browser, registry.as_deref(), deletion.busy());
            }
            *validation = None;
            return; // 旧手势永不重定向到换代后的目录；后续帧继续等待静默释放。
        }
    }

    if drag.active {
        drag.blocked = true; // 活动拖拽和release帧独占全部编辑器鼠标。
        let mut target = None; // 每帧从当前Interaction重新计算，不保留离开后的旧落点。
        for (interaction, row, cursor) in &rows {
            if !row_is_hovered(interaction, cursor) {
                continue; // 只有光标当前所在行可接受松开。
            }
            if let Some(SceneBrowserItem::Folder(folder_id)) = row.item.as_ref() {
                target = Some(SceneBrowserDropTarget::Folder(folder_id.clone())); // 树中任意可见文件夹都是有效目标。
                break;
            }
        }
        if target.is_none()
            && roots.iter().any(|interaction| {
                matches!(interaction, UiInteraction::Hovered | UiInteraction::Pressed)
            })
        {
            target = Some(SceneBrowserDropTarget::Root); // 根目录固定在滚动树上方，长列表也始终能拖回根级。
        }
        drag.drop_error = None;
        if let (Some(press), Some(target)) = (drag.press.as_ref(), target.as_ref()) {
            let cached = validation.as_ref().filter(|cached| {
                cached.0 == browser.document_revision
                    && cached.1 == press.item
                    && &cached.2 == target
            });
            if cached.is_none() {
                let parent = match target {
                    SceneBrowserDropTarget::Folder(id) => Some(id.as_str()),
                    SceneBrowserDropTarget::Root => None,
                };
                let result = if let Some(selection) = drag.selection.as_ref() {
                    crate::scene_browser::batch::target_allowed(
                        &browser.document,
                        &selection.items,
                        target,
                    )
                } else {
                    match &press.item {
                        SceneBrowserItem::Folder(id) => {
                            folder_move_target_allowed(&browser.document, id, parent)
                        }
                        _ => Ok(()),
                    }
                };
                *validation = Some((
                    browser.document_revision,
                    press.item.clone(),
                    target.clone(),
                    result,
                ));
            }
            drag.drop_error = validation
                .as_ref()
                .and_then(|cached| cached.3.as_ref().err())
                .cloned();
        } else {
            *validation = None;
        }
        drag.drop_target = if drag.drop_error.is_none() {
            target
        } else {
            None
        }; // 自身、后代或同名目录不显示有效落点。
        if !mouse.just_released(MouseButton::Left) {
            return; // 按住期间只更新浮层和落点，不修改归属。
        }

        let press = drag.press.take(); // 松开只允许消费一次冻结场景或文件夹身份。
        let selection = drag.selection.take();
        let drop_target = drag.drop_target.take(); // 松开后旧落点立即失效。
        let drop_error = drag.drop_error.take();
        drag.active = false; // 浮层在release帧关闭。
        if let (Some(press), Some(drop_target)) = (press, drop_target) {
            if let Some(selection) = selection {
                let result = index
                    .as_deref()
                    .ok_or_else(|| "场景目录尚未就绪".to_owned())
                    .and_then(|index| {
                        crate::scene_browser::batch::begin_batch(
                            &browser,
                            index,
                            &mut persistence,
                            selection.items,
                            drop_target,
                            registry.as_ref().map_or(0, |registry| registry.revision),
                        )
                    });
                editor.status =
                    result.map_or_else(|message| message, |_| "正在校验整批场景归类…".to_owned());
                *validation = None;
                return; // 整组由后台去重与校验，不逐项提交，也不改场景dirty。
            }
            match prepare_drop(&browser, &press, &drop_target) {
                Ok(Some(change)) => {
                    if let Err(message) = begin_scene_folder_save(
                        &mut browser,
                        &mut persistence,
                        &mut editor,
                        change.document,
                        Some(change.selected),
                        Some(change.expanded),
                    ) {
                        editor.status = message;
                    }
                }
                Ok(None) => editor.status = "所选对象已经属于该虚拟目录".to_owned(),
                Err(message) => editor.status = message,
            }
        } else {
            editor.status = drop_error.unwrap_or_else(|| "已取消目录归类拖拽".to_owned()); // 无效落点没有任何副作用。
        }
        if !persistence.busy()
            && let Some(feedback) = context.feedback.as_deref_mut()
        {
            feedback.record(&editor, &browser, registry.as_deref(), deletion.busy()); // 拒绝原因保留在原操作反馈；保存进度由原任务阶段回显。
        }
        *validation = None;
        return; // release帧保持blocked，其他按钮和视口不能补触发。
    }

    if let Some(press) = drag.press.as_ref() {
        if mouse.pressed(MouseButton::Left) {
            if let Some(current_physical) = window.physical_cursor_position()
                && drag_threshold_reached(press.start_physical, current_physical)
            {
                drag.active = true; // 达到六物理像素才从普通单击升级为拖拽。
                drag.collapse_on_release = false; // 按下组选中项后的真实拖动保留整组选集。
                drag.blocked = true; // 同帧开始独占所有编辑器鼠标。
            }
            return;
        }
        let press = drag.press.take().unwrap();
        let on_source = rows.iter().any(|(interaction, row, cursor)| {
            row_is_hovered(interaction, cursor) && row.item.as_ref() == Some(&press.item)
        });
        if drag.collapse_on_release && on_source {
            browser.select_only(Some(press.item.clone()));
            editor.scene_list_selection_id = match &press.item {
                SceneBrowserItem::Scene(id) => Some(id.clone()),
                _ => None,
            };
        }
        drag.selection = None;
        drag.collapse_on_release = false;
        if let SceneBrowserItem::Folder(id) = press.item
            && !drag.suppress_folder_toggle
            && rows.iter().any(|(interaction, row, cursor)| {
                row_is_hovered(interaction, cursor)
                    && row.item.as_ref() == Some(&SceneBrowserItem::Folder(id.clone()))
            })
        {
            toggle_folder_expansion(&mut browser, &mut editor, &id); // 未越阈值且仍在原行松开才执行一次展开。
        }
    }

    if drag.blocked {
        let any_pressed = mouse.pressed(MouseButton::Left)
            || mouse.pressed(MouseButton::Right)
            || mouse.pressed(MouseButton::Middle); // 作者三键任一仍按住时继续持有鼠标。
        let any_released = mouse.just_released(MouseButton::Left)
            || mouse.just_released(MouseButton::Right)
            || mouse.just_released(MouseButton::Middle); // release帧本身仍必须隔离。
        drag.blocked = any_pressed || any_released; // 全松后的下一静默帧才归还输入。
    }
}

#[cfg(test)]
#[path = "drag_tests.rs"]
mod tests;

/// 其他编辑器输入只在没有文件夹模态或场景拖拽鼠标所有权时运行。
pub(crate) fn scene_browser_general_input_available(
    editor: Res<SceneEditorState>, // 切换场景确认与等待保存同样独占底层作者输入。
    browser: Res<SceneBrowserState>,  // 文件夹模态状态。
    drag: Res<SceneBrowserDragState>, // 活动拖拽及release帧状态。
    terrain_conflict: Res<crate::authoring::TerrainExternalChangeState>, // Blender/F12冲突选择期间全体作者输入让位。
    placement_move: Option<Res<crate::viewport::placement_move::PlacementMoveState>>,
) -> bool {
    editor.scene_switch_confirmation.is_none()
        && browser.dialog.is_none()
        && !browser.selection_pending
        && !drag.blocks_other_mouse()
        && !terrain_conflict.modal_open()
        && !placement_move
            .as_ref()
            .is_some_and(|state| state.owns_mouse()) // 手柄及静默释放帧阻止其他作者动作，自己的状态机仍逐帧推进。
}
