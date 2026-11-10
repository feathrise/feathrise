//! 虚拟文件夹创建、重命名和删除模态框输入。

use feathrise_editor_ui::EditorButtonClick;
use std::time::{SystemTime, UNIX_EPOCH}; // 文件夹稳定ID使用可审计的Unix毫秒来源。

use bevy::input::ButtonState; // KeyboardInput只处理按下边沿。
use bevy::input::keyboard::{Key, KeyboardInput}; // Unicode文本、Enter、Escape与退格。
use bevy::prelude::*; // ECS资源、Interaction和输入消息。
use bevy::window::{PrimaryWindow, Window}; // 中文预编辑、提交与主窗口IME开关。

use crate::components::SceneEditorActionButton;
use crate::constants::SCENE_FOLDER_MAX_FOLDERS;
use crate::resources::{
    SceneBrowserDragState, SceneBrowserState, SceneCreationTask, SceneDeletionTask, SceneEditorState,
    SceneFolderPersistence,
};
use crate::types::{
    EditorAction, SceneBrowserItem, SceneFolderDialogMode, SceneFolderDialogState, SceneFolderRecord,
};
use crate::ui::actions::set_ime_enabled;

use super::structure_change_allowed;
use crate::scene_browser::helpers::{
    expand_folder_ancestors, folder_by_id, folder_is_empty, next_folder_id, normalize_folder_name,
    sibling_name_exists,
};
use crate::scene_browser::persistence::begin_scene_folder_save;

/// 执行创建、重命名或删除空文件夹的单次乐观结构修改。
fn confirm_folder_dialog(
    dialog: &SceneFolderDialogState, // 按确认时冻结的模式、目标和名称。
    browser: &mut SceneBrowserState, // 当前有效文档、树选择和展开集合。
    persistence: &mut SceneFolderPersistence, // 保存任务及完整回滚槽。
    editor: &mut SceneEditorState,   // 场景选择和状态回显。
    creation: &SceneCreationTask,    // 防止与新建场景目录事务并发。
    deletion: &SceneDeletionTask,    // 防止与场景回收目录事务并发。
) -> Result<(), String> {
    structure_change_allowed(browser, persistence, creation, deletion)?; // 确认帧重新检查所有互斥门。
    let mut document = browser.document.clone(); // 固定最多512文件夹，乐观修改保留回滚原件。
    match dialog.mode {
        SceneFolderDialogMode::Create => {
            if document.folders.len() >= SCENE_FOLDER_MAX_FOLDERS {
                return Err(format!("虚拟文件夹已达到{SCENE_FOLDER_MAX_FOLDERS}项上限"));
            }
            let parent_id = dialog.target_folder_id.clone(); // 模态打开时已经冻结目录、场景父级或根上下文。
            if parent_id
                .as_deref()
                .is_some_and(|folder_id| folder_by_id(&document, folder_id).is_none())
            {
                return Err("新建文件夹的父目录已不存在".to_owned()); // 热更新错帧不能把目标静默改到根目录。
            }
            let name = normalize_folder_name(&dialog.name)?; // 修剪空白并拒绝路径和保留名称。
            if sibling_name_exists(&document, parent_id.as_deref(), &name, None) {
                return Err(format!("当前目录已经存在名为“{name}”的文件夹")); // 同级名称不区分大小写。
            }
            let unix_millis = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| format!("系统时间早于Unix纪元：{error}"))?
                .as_millis(); // ID来源在确认帧冻结。
            let folder_id = next_folder_id(&document, unix_millis)?; // 同毫秒碰撞使用有界后缀。
            document.folders.push(SceneFolderRecord {
                id: folder_id.clone(),        // 重命名后保持不变的稳定身份。
                display_name: name,           // 只保存已经规范化的显示名。
                parent_id: parent_id.clone(), // 新文件夹属于打开模态时冻结的树容器。
            });
            let mut expanded = browser.expanded_folder_ids.clone(); // 创建子目录后自动展开父链，使新行立即可见。
            expand_folder_ancestors(&document, parent_id.as_deref(), &mut expanded); // 根目录固定展开，无需特殊哨兵。
            begin_scene_folder_save(
                browser,
                persistence,
                editor,
                document,
                Some(SceneBrowserItem::Folder(folder_id)),
                Some(expanded),
            )?; // 乐观显示新文件夹并选中它。
        }
        SceneFolderDialogMode::Rename => {
            let folder_id = dialog
                .target_folder_id
                .as_deref()
                .ok_or_else(|| "重命名目标缺少稳定文件夹ID".to_owned())?; // 模态状态损坏时不猜测当前选择。
            let target = folder_by_id(&document, folder_id)
                .ok_or_else(|| "待重命名文件夹已不存在".to_owned())?;
            let parent_id = target.parent_id.clone(); // 查重必须使用目标真实父级，而非当前导航猜测。
            let name = normalize_folder_name(&dialog.name)?; // 统一修剪与字符限制。
            if sibling_name_exists(&document, parent_id.as_deref(), &name, Some(folder_id)) {
                return Err(format!("同级已经存在名为“{name}”的文件夹")); // 排除目标自身但拒绝其他大小写同名。
            }
            let target = document
                .folders
                .iter_mut()
                .find(|folder| folder.id == folder_id)
                .ok_or_else(|| "待重命名文件夹已不存在".to_owned())?;
            target.display_name = name; // 只改显示名，稳定ID、父级、子级和场景归属全不变。
            begin_scene_folder_save(
                browser,
                persistence,
                editor,
                document,
                Some(SceneBrowserItem::Folder(folder_id.to_owned())),
                None,
            )?; // 保持目标高亮便于连续确认。
        }
        SceneFolderDialogMode::Delete => {
            let folder_id = dialog
                .target_folder_id
                .as_deref()
                .ok_or_else(|| "删除目标缺少稳定文件夹ID".to_owned())?;
            if folder_by_id(&document, folder_id).is_none() {
                return Err("待删除文件夹已不存在".to_owned()); // 热更新移除后不能误删其他行。
            }
            if !folder_is_empty(&document, folder_id) {
                return Err("文件夹已包含子文件夹或场景，不能删除".to_owned()); // 确认帧再次检查空目录条件。
            }
            document.folders.retain(|folder| folder.id != folder_id); // 只移除冻结的单个空文件夹记录。
            let mut expanded = browser.expanded_folder_ids.clone(); // 被删除身份不能继续留在会话展开集合。
            expanded.remove(folder_id); // 空目录没有后代需要额外清理。
            begin_scene_folder_save(browser, persistence, editor, document, None, Some(expanded))?; // 删除后清除旧高亮和已失效展开身份。
        }
    }
    Ok(())
}

/// 处理文件夹模态框按钮、中文IME和键盘确认。
#[allow(clippy::too_many_arguments)] // 模态确认必须在同一系统内重新验证两个catalog事务和归类single-flight。
pub(crate) fn handle_scene_folder_dialog_input(
    mut keyboard_inputs: MessageReader<KeyboardInput>, // Unicode键盘文本与控制键。
    guard: Res<crate::ui::text_input::EditorTextInputGuard>, // 候选确认帧只交给官方输入插件。
    official_inputs: Query<&bevy::text::EditableText, With<crate::ui::text_input::EditorTextInput>>, // 只读官方组合状态，不处理文字。
    buttons: Query<(&EditorButtonClick, &SceneEditorActionButton), Changed<EditorButtonClick>>, // 输入框焦点、取消和确认。
    mut browser: ResMut<SceneBrowserState>, // 唯一模态状态和错误回显。
    mut drag: ResMut<SceneBrowserDragState>, // 模态关闭帧复用release门阻止底层作者输入补触发。
    mut persistence: ResMut<SceneFolderPersistence>, // 确认后启动single-flight保存。
    mut editor: ResMut<SceneEditorState>,   // 状态和场景选择回滚输入。
    creation: Res<SceneCreationTask>,       // 与新场景创建互斥。
    deletion: Res<SceneDeletionTask>,       // 与场景回收互斥。
    mut windows: Query<&mut Window, With<PrimaryWindow>>, // 中文IME开关和候选位置。
) {
    if !editor.open || browser.dialog.is_none() {
        keyboard_inputs.clear(); // 独立MessageReader游标保持追上全局尾部。
        return;
    }
    let mut cancel = false; // 按钮与Escape汇合为一个无副作用关闭动作。
    let mut confirm = false; // 按钮与Enter汇合为一个结构提交动作。
    for (interaction, button) in &buttons {
        if !interaction.is_clicked() {
            continue; // 悬停和释放不改变模态状态。
        }
        match button.0 {
            EditorAction::CancelSceneFolderDialog => cancel = true, // 本帧末统一关闭IME。
            EditorAction::ConfirmSceneFolderDialog => confirm = true, // 本帧末统一校验并保存。
            EditorAction::FocusSceneFolderName => {
                if browser
                    .dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.mode != SceneFolderDialogMode::Delete)
                {
                    set_ime_enabled(&mut windows, true); // 创建和重命名重新取得中文输入焦点。
                }
            }
            _ => {} // 底层按钮被模态遮罩阻止命中。
        }
    }

    let ime_owns_keyboard =
        guard.block_shortcuts || official_inputs.iter().any(|text| text.is_composing()); // 官方组件独占候选过程，业务只保护快捷键。
    for input in keyboard_inputs.read() {
        if input.state != ButtonState::Pressed {
            continue; // 只消费松开点击，系统重复键由窗口明确产生新的Pressed。
        }
        if ime_owns_keyboard {
            continue;
        } // 当前键盘批次已经由中文输入法完整解释。
        match (&input.logical_key, &input.text) {
            (Key::Escape, _) => cancel = true, // Escape与取消按钮完全一致。
            (Key::Enter, _) => confirm = true, // Enter确认当前冻结模式。
            _ => {}                            // 其他快捷键在模态期间全部忽略。
        }
    }

    if cancel {
        browser.dialog = None; // 未提交结构没有任何文件副作用。
        browser.last_error = None; // 下次打开不继承旧校验诊断。
        drag.blocked = true; // 鼠标或键盘关闭模态的本帧仍由归类域独占输入。
        set_ime_enabled(&mut windows, false); // 立即把输入法归还给系统。
        return;
    }
    if !confirm || ime_owns_keyboard {
        // 按钮同样不能抢先提交尚未确认的候选。
        return; // 纯输入帧只更新名称和预编辑回显。
    }
    let Some(dialog) = browser.dialog.clone() else {
        return; // 防御性避免同帧取消后的空状态。
    };
    let result = confirm_folder_dialog(
        &dialog,      // 文件夹创建、重命名与删除继续使用原独立事务。
        &mut browser, // 场景资料编辑已迁移到新建表单，不经文件夹模态。
        &mut persistence,
        &mut editor,
        &creation,
        &deletion,
    );
    match result {
        Ok(()) => {
            browser.dialog = None; // 乐观保存成功启动后关闭模态框。
            browser.last_error = None; // 状态栏由保存任务显示进度。
            drag.blocked = true; // 确认按钮或Enter不得在同帧落入底层视口与其他按钮。
            set_ime_enabled(&mut windows, false); // 结构任务期间不保留文本焦点。
        }
        Err(message) => {
            browser.last_error = Some(message.clone()); // 保持弹框打开供作者修正或取消。
            editor.status = message; // 底栏同步提供同一诊断。
        }
    }
}
