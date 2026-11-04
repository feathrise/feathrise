//! 建筑预设字段输入、缓冲提交、输入法清理及草稿动作共享的小型帮助函数。
//!
//! 【功能设计】中文名称交给 Bevy 官方文本组件，数值字段只接受有界 ASCII 浮点语法。

use super::*; // 复用父模块的字段、输入法、草稿与工作区协议。

/// 处理唯一活动建筑字段的键盘提交、取消与数值字符。
#[allow(clippy::too_many_arguments)] // 输入所有权需要同时核对模态、官方焦点、草稿和窗口。
pub(in crate::ui::building_presets) fn handle_text_input(
    mut keyboard_inputs: MessageReader<KeyboardInput>,
    fields: Query<(Entity, &EditableText, &BuildingPresetButton)>, // 当前活动名称或连接技术字段。
    modal_browser: Option<Res<crate::resources::MapBrowserState>>, // 文件夹模态独占快捷键。
    numeric: Option<Res<feathrise_editor_ui::ActiveNumericInput>>, // 专业数值弹框优先。
    official_inputs: Query<(), With<EditableText>>, // 数值字段不能偷取其他文本框事件。
    guard: Res<EditorTextInputGuard>,               // 输入法确认帧继续保护业务键。
    mut input_focus: ResMut<InputFocus>,            // 成功或取消后释放建筑名称焦点。
    mut state: ResMut<BuildingPresetEditorState>,   // 活动字段和缓冲。
    mut workspace: ResMut<VegetationPresetEditorState>, // 当前标签身份。
    editor: Res<MapEditorState>,                    // F12 与模态状态。
    mut windows: Query<&mut Window, With<PrimaryWindow>>, // IME 开关。
) {
    let Some(field) = state.active_field else {
        keyboard_inputs.clear();
        return; // 本域无输入会话时不积压消息。
    };
    if !editor.open || state.active_draft(workspace.active_tab).is_none() || state.busy() {
        if fields
            .iter()
            .any(|(entity, _, _)| input_focus.get() == Some(entity))
        {
            input_focus.clear(); // 只释放本域真实文本实体。
        }
        clear_input(&mut state, &mut windows);
        keyboard_inputs.clear();
        return; // 失效标签或任务期间绝不提交旧缓冲。
    }
    let mut composing = false; // 数值字段没有输入法组合态。
    if field.accepts_text() {
        let Some((entity, editable, _)) = fields
            .iter()
            .find(|(_, _, button)| button.0 == BuildingPresetAction::Focus(field))
        else {
            keyboard_inputs.clear();
            return; // 工作区重建间隙不猜测旧文本实体。
        };
        state.input_buffer = editable.value().to_string(); // 官方已确认文本是唯一中文来源。
        if input_focus.get() != Some(entity) {
            keyboard_inputs.clear();
            return; // 焦点已经交给其他输入域。
        }
        composing = editable.is_composing() || (field.accepts_ime() && guard.block_shortcuts);
    }
    if editor.new_map_dialog_open
        || modal_browser
            .as_ref()
            .is_some_and(|browser| browser.dialog.is_some())
        || numeric.as_ref().is_some_and(|input| input.entity.is_some())
        || (!field.accepts_text()
            && input_focus
                .get()
                .is_some_and(|entity| official_inputs.contains(entity)))
    {
        keyboard_inputs.clear();
        return; // 上层输入目标存在时保留建筑缓冲但不消费事件。
    }
    let before = state.input_buffer.clone(); // 只有实际字符变化才重建数值显示。
    let mut submit = false;
    let mut cancel = false;
    for input in keyboard_inputs.read() {
        if !input.state.is_pressed() || composing {
            continue; // 释放键与候选组合过程没有业务语义。
        }
        match (&input.logical_key, &input.text) {
            (Key::Enter | Key::Tab, _) => submit = true, // 提交并退出字段。
            (Key::Escape, _) => cancel = true,           // 丢弃本次缓冲。
            (Key::Backspace, _) if !field.accepts_text() => {
                state.input_buffer.pop(); // 数值退格按 UTF-8 安全边界执行。
            }
            (_, Some(text)) if !field.accepts_text() => push_number(&mut state.input_buffer, text),
            _ => {} // 中文、选区与剪贴板完全交给 EditableText。
        }
    }
    if submit {
        match commit_pending_input(
            &mut state,
            workspace.active_tab,
            &mut workspace,
            &mut windows,
        ) {
            Ok(()) => {
                if field.accepts_text() {
                    input_focus.clear();
                }
                state.status = "建筑字段已提交到当前草稿".to_owned();
            }
            Err(error) => state.status = error, // 无效值保留原缓冲供作者修正。
        }
    } else if cancel {
        if field.accepts_text() {
            input_focus.clear();
        }
        clear_input(&mut state, &mut windows);
        state.status = "已放弃本次建筑字段修改".to_owned();
        touch(&mut state, &mut workspace);
    } else if !field.accepts_text() && before != state.input_buffer {
        touch(&mut state, &mut workspace); // 只为可见数值缓冲推进显示修订。
    }
}

/// 提交并关闭当前字段输入会话。
pub(super) fn commit_pending_input(
    state: &mut BuildingPresetEditorState,
    active: PresetWorkspaceTab,
    workspace: &mut VegetationPresetEditorState,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    let Some(field) = state.active_field else {
        return Ok(()); // 普通按钮无需额外工作。
    };
    let input = state.input_buffer.clone(); // 避免借用草稿时继续借用状态缓冲。
    let draft = state
        .active_draft_mut(active)
        .ok_or_else(|| "活动建筑预设标签已不存在".to_owned())?;
    commit_field(&mut draft.descriptor, field, &input)?;
    clear_input(state, windows);
    touch(state, workspace);
    Ok(())
}

/// 清除字段身份和窗口输入法状态。
pub(super) fn clear_input(
    state: &mut BuildingPresetEditorState,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) {
    state.active_field = None;
    state.input_buffer.clear();
    set_ime_enabled(windows, false);
}

/// 追加一个有界 ASCII 浮点字符片段。
fn push_number(target: &mut String, value: &str) {
    if target.len() >= MAX_NUMBER_CHARS {
        return; // 异常粘贴不会无界扩大输入缓冲。
    }
    target.extend(
        value
            .chars()
            .filter(|character| character.is_ascii_digit() || ".-+eE".contains(*character)),
    );
    target.truncate(target.len().min(MAX_NUMBER_CHARS)); // 多字符粘贴仍受同一字节上限保护。
}

/// 生成目录内唯一的新建筑规范身份。
pub(super) fn unique_id(registry: &BuildingPresetRegistry) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let base = format!("building-{millis}");
    if registry.resolve(&base).is_none() {
        return base;
    }
    (2_u32..)
        .map(|suffix| format!("{base}-{suffix}"))
        .find(|candidate| registry.resolve(candidate).is_none())
        .expect("u32 后缀空间足以形成新建筑预设 ID")
}

/// 从相对资产路径生成可继续编辑的短名称。
pub(super) fn label_from_asset_path(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path); // 项目资产路径统一使用正斜杠。
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let label: String = stem.chars().take(128).collect(); // 与目录文字上限一致。
    if label.trim().is_empty() {
        "新建筑".to_owned()
    } else {
        label
    }
}

/// 同时推进建筑局部修订与中央共享工作区修订。
pub(super) fn touch(
    state: &mut BuildingPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
) {
    state.touch_ui();
    workspace.touch_ui(); // 唯一中央壳只观察共享修订。
}
