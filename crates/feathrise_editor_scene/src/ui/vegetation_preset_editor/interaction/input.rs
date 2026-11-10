//! 活动预设字段的单缓冲键盘、中文IME与独立多行备注输入。

use super::super::types::PresetEditorFieldText; // 真实可见字段身份。
use crate::ui::text_input::EditorTextInputGuard; // 同帧候选事件保护业务快捷键。
use bevy::input::keyboard::{Key, KeyboardInput}; // 逻辑字符与按键分别处理。
use bevy::prelude::*; // 消息、资源和当前键盘修饰键。
use bevy::text::EditableText; // 中文、选区和剪贴板由官方组件处理。
use bevy::window::{PrimaryWindow, Window}; // 中文候选与窗口IME所有权。

use super::super::fields::commit_field; // 唯一强类型字段写回入口。
use super::super::types::{PresetField, VegetationPresetEditorState}; // 沿用所有预设的单缓冲状态。
use crate::resources::SceneEditorState; // F12关闭后停止输入并清除焦点。
use crate::ui::actions::set_ime_enabled; // 不另建输入法开关。

const MAX_PRESET_FIELD_CHARS: usize = 2_048; // 普通路径字段保持原有输入边界。

/// 单行仍Enter提交；备注Enter换行、Ctrl+Enter提交，IME确认不得多插入一行。
pub(crate) fn handle_text_input(
    // 所有输入先进入当前明确聚焦字段，不修改其他标签。
    mut keyboard_inputs: MessageReader<KeyboardInput>, // 读取本次键盘批次。
    browser: Option<Res<crate::resources::SceneBrowserState>>, // 模态文件夹输入屏蔽底层预设。
    focus: Res<bevy::input_focus::InputFocus>,         // 原生点击直接聚焦可见文本。
    texts: Query<(Entity, &PresetEditorFieldText, &EditableText)>, // 官方已提交文本唯一来源。
    guard: Res<EditorTextInputGuard>,                  // 候选确认不能同时触发提交。
    keys: Option<Res<ButtonInput<KeyCode>>>,           // 最小无窗口测试可以省略物理修饰键资源。
    mut state: ResMut<VegetationPresetEditorState>,    // 当前唯一输入焦点和字符缓冲。
    editor: Res<SceneEditorState>,                       // 只有打开F12时允许作者输入。
    mut windows: Query<&mut Window, With<PrimaryWindow>>, // 聚焦收口时释放主窗口IME。
) {
    // 多行备注不改变其他数值和资产字段的解析规则。
    if let Some(previous) = state.active_field.filter(|field| field.accepts_text()) // 同帧官方编辑先读旧字段，避免切焦点丢末字。
        && let Some((_, _, editable)) = texts.iter().find(|(_, marker, _)| marker.0 == previous)
    {
        // 原业务身份确定唯一来源。
        let value = editable.value().to_string(); // 官方已提交内容不包含预编辑。
        if state.input_buffer != value {
            state.input_buffer = value;
        } // 保存/切字段使用同帧最新文本。
    } // 不依赖InputFocus仍指向旧字段。
    if editor.new_scene_dialog_open
        || browser
            .as_ref()
            .is_some_and(|browser| browser.dialog.is_some())
    {
        // 模态层拥有所有业务快捷键。
        keyboard_inputs.clear(); // 底层字段不处理Enter/Esc。
        return; // 保留底层未提交事务，关闭模态后继续。
    } // 官方焦点稍后由公共模态同步收口。
    if editor.open
        && !state.busy()
        && let Some((_, marker, _)) = focus.get().and_then(|entity| texts.get(entity).ok())
    {
        // 子文本点击无需依赖父按钮命中。
        if state.active_field != Some(marker.0) {
            if guard.block_shortcuts {
                keyboard_inputs.clear();
                return;
            } // 组合中禁止切字段，只查询公共官方状态。
            // 当前字段再次点击保持光标和选区。
            if let Err(error) = commit_pending_input(&mut state, &mut windows) {
                state.status = error;
                keyboard_inputs.clear();
                return;
            } // 切字段先提交原业务值。
            state.active_field = Some(marker.0); // 记录官方已聚焦字段身份。
        } // 后续只镜像新字段官方值。
    } // 不从其它域焦点猜本域输入。
    let Some(field) = state.active_field else {
        // 没有焦点时丢弃旧消息，避免下次聚焦重放字符。
        keyboard_inputs.clear(); // 消费无主键盘消息。
        return; // 没有字段需要修改。
    }; // 本帧字段在进入时冻结。
    if !editor.open || state.active_draft().is_none() || state.busy() {
        // 关闭、换页失效或资产任务期间不保留悬挂焦点。
        clear_input(&mut state, &mut windows); // 一并取消未完成的备注粘贴。
        keyboard_inputs.clear(); // 无效批次不能稍后补写。
        return; // 不再处理当前帧按键。
    } // 正常字段进入相同单缓冲编辑链。
    if field.accepts_text() {
        // 中文字段不再自行读取IME和普通字符。
        let Some((_, _, editable)) = focus
            .get()
            .and_then(|entity| texts.get(entity).ok())
            .filter(|(_, marker, _)| marker.0 == field)
        else {
            keyboard_inputs.clear();
            return;
        }; // 动态构建间隙丢弃输入。
        let value = editable.value().to_string(); // 官方值排除未确认的拼音。
        if state.input_buffer != value {
            state.input_buffer = value;
        } // 不触发重建以保留光标。
        if guard.block_shortcuts || editable.is_composing() {
            keyboard_inputs.clear();
            return;
        } // 候选确认键只归输入法。
    } // 数值输入仍走既有ASCII语法。
    let control = keys.as_deref().is_some_and(|keys| {
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
    }); // 左右Ctrl等价。
    for input in keyboard_inputs.read() {
        // 单批次只写当前字段。
        if !input.state.is_pressed() {
            continue;
        } // 键释放不重复插字。
        match (&input.logical_key, &input.text) {
            // 特殊键先于普通文本。
            (Key::Enter, _) if field == PresetField::Notes && !control => {
                // 官方EditableText负责备注换行，业务系统不重复插字。
            } // Enter只换行，不保存或失焦。
            (Key::Enter | Key::Tab, _) => {
                // 单行Enter、备注Ctrl+Enter与Tab只提交草稿。
                match commit_pending_input(&mut state, &mut windows) {
                    // 保存按钮复用相同收口入口。
                    Ok(()) => {
                        state.status = "字段已提交到当前预设草稿；保存全局预设后写入文件".to_owned()
                    } // 不谎称已写盘。
                    Err(error) => state.status = error, // 无效字段保留输入供修正。
                } // 本次提交结束。
                break; // 不把提交后消息写回已清空焦点。
            } // 明确提交边界。
            (Key::Escape, _) => {
                // 无IME候选时Esc只放弃字符修改。
                clear_input(&mut state, &mut windows); // 已提交草稿保持原值。
                state.status = "已放弃本次字符修改；预设草稿保持原值".to_owned(); // 明确取消范围。
                break; // 同帧后续键盘不进入已取消字段。
            } // 单缓冲取消完成。
            (Key::Backspace, _) if !field.accepts_text() => {
                state.input_buffer.pop();
            } // 普通字段沿用尾部退格。
            (_, Some(text)) => {
                // 普通文本按字段类别进入缓冲。
                if field.accepts_text() {
                    // 普通字符、删除、光标、选区和剪贴板全部由官方组件处理。
                } else {
                    push_number(&mut state.input_buffer, text);
                } // 数值语法不变。
            } // 普通输入完成。
            _ => {} // 无文本键不新增字符。
        } // 当前按键处理完毕。
    } // 真实描述仍等待显式提交。
}

/// 提交活动字符缓冲；备注允许空串并完整保留换行和空白。
pub(in super::super) fn commit_pending_input(
    // 多行备注复用保存、切页和普通离焦链。
    state: &mut VegetationPresetEditorState, // 唯一预设会话。
    windows: &mut Query<&mut Window, With<PrimaryWindow>>, // 提交后归还IME。
) -> Result<(), String> {
    // 没有焦点时不修改描述。
    let Some(field) = state.active_field else {
        return Ok(());
    }; // 无焦点提交为空操作。
    let input = state.input_buffer.clone(); // 只冻结当前字段，不克隆全部配置。
    let draft = state
        .active_draft_mut()
        .ok_or_else(|| "活动预设标签已不存在".to_owned())?; // 不猜另一标签。
    commit_field(&mut draft.descriptor, field, &input)?; // 备注Some("")明确表示已清空。
    clear_input(state, windows); // 取消粘贴和全选残留。
    state.touch_ui(); // 采用后刷新草稿脏标记。
    Ok(()) // 此处没有磁盘写入。
}

/// 清焦点同时撤销粘贴身份，迟到结果不能写入后来字段。
pub(in super::super) fn clear_input(
    // 单行与多行共用收口行为。
    state: &mut VegetationPresetEditorState, // 当前唯一输入会话。
    windows: &mut Query<&mut Window, With<PrimaryWindow>>, // 主窗口IME状态。
) {
    // 不改任何已提交描述。
    state.active_field = None; // 清除字段拥有者。
    state.input_buffer.clear(); // 丢弃未提交字符。
    set_ime_enabled(windows, false); // 归还窗口输入法。
}

/// 数值只接受既有Rust数字语法，语义由字段提交与目录校验负责。
fn push_number(target: &mut String, value: &str) {
    // 不改变数值字段外观或鼠标操作。
    let remaining = MAX_PRESET_FIELD_CHARS.saturating_sub(target.len()); // 单次追加不越过原边界。
    target.extend(
        value
            .chars()
            .filter(|character| character.is_ascii_digit() || ".-+eE".contains(*character))
            .take(remaining),
    ); // 只接受ASCII数字。
}

#[cfg(test)] // 用真实KeyboardInput/IME验证多行和提交。
mod tests; // 回归不启动窗口、不读取用户剪贴板。
