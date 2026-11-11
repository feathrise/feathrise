//! 当前场景所选传送点名称的中文输入、作者提交和紧凑回显。

use bevy::input::keyboard::{Key, KeyboardInput}; // 读取Unicode字符、Enter、Escape和退格按键。
use bevy::prelude::*; // UI交互、资产资源、Commands与文本Presentation。
use bevy::window::{PrimaryWindow, Window}; // 中文预编辑、提交与主窗口IME开关。
use feathrise_scene_content::{SceneContentRegistry, SceneManifestAsset}; // 自动默认名需要解析目标场景显示名。
use crate::ui::theme as colors; // 输入框配色与IME键盘去重使用公共规则。

use crate::authoring::{
    EditorAuthoringSession, EditorCommand, EditorPreviewAssets, refresh_owned_portal_preview,
}; // 名称修改进入统一撤销栈并刷新尚未保存的世界标签。
use crate::components::{
    SceneEditorActionButton, SceneEditorPortalNameInput, SceneEditorPortalNameInputText,
}; // 输入框与两个结构化动作不依赖实体名称。
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState}; // 数值和弦优先占用作者输入。
use crate::types::EditorAction; // 侧栏名称操作不要求切到视口Edit。
use crate::ui::actions::set_ime_enabled; // 与新建场景和文件夹输入共用主窗口IME开关。

use super::helpers::PortalDisplayContext; // 聚焦时读取当前实际显示名而不是旧流水快照。
use super::navigation::PortalSidebarNavigation;

const MAX_PORTAL_NAME_CHARS: usize = 48; // 区块名称保留充足中文长度，同时限制JSON、Billboard和列表宽度。

/// 提交前统一去除首尾空白并拒绝无法显示或会破坏三段列表结构的名称。
fn normalize_portal_name(value: &str) -> Result<String, String> {
    let value = value.trim(); // 作者误输入的首尾空格不写入JSON和Billboard纹理。
    if value.is_empty() {
        return Err("传送点名称不能为空".to_owned()); // 空名称无法表达王府、河道等区域语义。
    }
    if value.chars().count() > MAX_PORTAL_NAME_CHARS {
        return Err(format!("传送点名称最多{MAX_PORTAL_NAME_CHARS}个字符")); // 手改状态也不能绕过输入上限。
    }
    if value.chars().any(char::is_control) || value.contains('|') {
        return Err("传送点名称不能包含控制字符或竖线".to_owned()); // 竖线保留给固定三段字段边界。
    }
    Ok(value.to_owned()) // 返回独立作者值供撤销命令同时保存前后快照。
}

/// 结束当前名称输入并归还系统IME；是否修改作者数据由调用侧在清理前决定。
fn close_portal_name_edit(
    editor: &mut SceneEditorState, // 清除冻结ID、正式文字和预编辑文字。
    windows: &mut Query<&mut Window, With<PrimaryWindow>>, // 中文输入焦点归还主窗口。
) {
    editor.portal_name_edit.clear(); // 下一次聚焦必须重新读取当时选中记录。
    set_ime_enabled(windows, false); // 防止关闭输入后候选框继续覆盖三维视口。
}

/// 把受检名称原子写入草稿、撤销栈和未保存预览；正式运行时由保存热重载采用同一字段。
fn apply_portal_name(
    commands: &mut Commands, // 未保存预览根需要原位替换Billboard请求。
    preview_assets: &EditorPreviewAssets, // 替换预览继续复用共享网格和中文字体。
    editor: &mut SceneEditorState, // 读取冻结输入并写状态、dirty和专用修订。
    session: &mut EditorAuthoringSession, // 修改当前草稿并登记统一撤销命令。
    windows: &mut Query<&mut Window, With<PrimaryWindow>>, // 成功提交后关闭IME。
) {
    if session.saving {
        editor.status = "场景保存进行中，暂不能修改传送点名称".to_owned(); // 保存快照冻结后拒绝并发作者写入。
        return;
    }
    let Some(portal_id) = editor.portal_name_edit.portal_id.clone() else {
        editor.status = "请先选择当前场景中的传送点".to_owned(); // 不猜测列表第一项。
        return;
    };
    if editor.selected_current_portal_id.as_deref() != Some(portal_id.as_str()) {
        editor.status = "传送点选择已经变化，请重新点击名称输入框".to_owned(); // 冻结身份与当前选择不一致时失败关闭。
        close_portal_name_edit(editor, windows); // 丢弃旧点尚未提交的文字。
        return;
    }
    let next_label = match normalize_portal_name(&editor.portal_name_edit.value) {
        Ok(label) => label, // 只有受检Unicode名称可以进入作者JSON。
        Err(error) => {
            editor.status = error; // 保留输入和焦点供作者原地修正。
            return;
        }
    };
    let Some(draft) = session.draft.as_mut() else {
        editor.status = "当前场景草稿已经失效".to_owned(); // 切换场景边界不写入旧草稿。
        close_portal_name_edit(editor, windows); // 失效会话不能继续持有IME。
        return;
    };
    let Some(record) = draft
        .portals
        .portals
        .iter_mut()
        .find(|record| record.id == portal_id)
    else {
        editor.status = "所选传送点已不存在，请重新选择".to_owned(); // 删除或热更后不按名称猜测替代项。
        close_portal_name_edit(editor, windows); // 清除悬空稳定ID。
        return;
    };
    if record.label == next_label && record.label_customized {
        editor.status = format!("传送点名称未变化：{next_label}"); // 重复应用不污染撤销栈和dirty状态。
        close_portal_name_edit(editor, windows); // 完成无变化输入会话。
        return;
    }
    let command = EditorCommand::PortalLabelChanged {
        portal_id: portal_id.clone(),               // 撤销始终按稳定ID定位。
        before_label: record.label.clone(),         // 保留修改前作者值或默认快照。
        before_customized: record.label_customized, // 自动默认状态必须随文字共同撤销。
        after_label: next_label.clone(),            // 重做使用同一受检名称。
        after_customized: true,                     // 显式应用后固定采用作者区域名称。
    };
    record.label = next_label.clone(); // 草稿列表和保存快照立即读取新名称。
    record.label_customized = true; // 后续目标场景改名不覆盖“前往王府”等作者语义。
    let updated_record = record.clone(); // 结束草稿借用后重建可能存在的未保存预览。
    session.undo.push(command); // 名称修改与增删点共享统一线性撤销栈。
    session.redo.clear(); // 新作者操作终止旧重做分支。
    refresh_owned_portal_preview(commands, preview_assets, session, &updated_record); // 新建未保存点同帧换成新Billboard。
    session.mark_portals_changed(editor); // 刷新两套列表并进入统一保存门。
    close_portal_name_edit(editor, windows); // 提交后结束IME和光标回显。
    editor.status = format!("传送点名称已改为“{next_label}”；保存后更新正式世界标签"); // 明确尚需保存。
}

/// 处理名称输入框聚焦、中文IME、Enter应用、Escape取消和显式“应用名称”按钮。
#[allow(clippy::too_many_arguments)] // 输入事务需要作者草稿、预览资产、IME窗口和两个结构化按钮共同收口。
pub(crate) fn handle_portal_name_editor(
    mut commands: Commands, // 提交时可能替换未保存传送预览根。
    navigation: PortalSidebarNavigation,
    modal_state: (
        Res<crate::resources::SceneBrowserState>,
        Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>,
    ), // 模态与中央标签独占作者键盘。
    input_focus: Res<bevy::input_focus::InputFocus>, // 官方字段直接点击无需父按钮冒泡。
    name_entities: Query<Entity, With<SceneEditorPortalNameInputText>>, // 精确识别可见输入本体。
    mut keyboard_inputs: MessageReader<KeyboardInput>, // Unicode键盘字符与控制键边沿。
    guard: Res<crate::ui::text_input::EditorTextInputGuard>, // 候选确认帧只交给官方输入插件。
    official_inputs: Query<&bevy::text::EditableText, With<crate::ui::text_input::EditorTextInput>>, // 只读官方组合状态，不处理文字。
    buttons: Query<(&feathrise_editor_ui::EditorButtonClick, &SceneEditorActionButton), Changed<feathrise_editor_ui::EditorButtonClick>>, // 普通按钮由共享释放点击驱动；文字区仍使用官方焦点。
    numeric_scrub_mouse: Res<SceneEditorNumericScrubMouseGuard>, // 三键数值和弦优先占用输入。
    mut editor: ResMut<SceneEditorState>,                        // 名称会话、选择和状态反馈。
    mut session: ResMut<EditorAuthoringSession>,               // 当前草稿、撤销栈与预览所有权。
    preview_assets: Res<EditorPreviewAssets>,                  // 未保存点改名后重建完整预览。
    registry: Option<Res<SceneContentRegistry>>,                 // 默认名称解析其他目标场景清单。
    manifests: Res<Assets<SceneManifestAsset>>, // 聚焦旧自动点时取得实际“前往目标场景”名称。
    mut windows: Query<&mut Window, With<PrimaryWindow>>, // 开关中文IME。
) {
    if !navigation.visible(&editor)
        || modal_state.0.dialog.is_some()
        || editor.new_scene_dialog_open
        || modal_state.1.active_tab != crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Scene
    {
        if editor.portal_name_edit.active {
            close_portal_name_edit(&mut editor, &mut windows); // 关闭F12或切工具时取消未提交名称。
        }
        keyboard_inputs.clear(); // 本系统的独立Reader不积压隐藏期间按键。
        return;
    }
    if numeric_scrub_mouse.blocked() {
        keyboard_inputs.clear(); // 数值交互帧不把字符重复写入名称。
        return;
    }

    let mut apply_requested = false; // Enter或按钮最终共用一次提交路径。
    let direct_focus = input_focus
        .get()
        .is_some_and(|entity| name_entities.contains(entity))
        && !editor.portal_name_edit.active; // 首次点击文字区同样打开业务事务。
    let direct_action = SceneEditorActionButton(EditorAction::PortalNameFocus); // 复用既有受检聚焦路径。
    for button in buttons
        .iter()
        .filter_map(|(click, button)| click.is_clicked().then_some(button))
        .chain(direct_focus.then_some(&direct_action))
    {
        match button.0 {
            EditorAction::PortalNameFocus => {
                if session.saving {
                    editor.status = "场景保存进行中，暂不能修改传送点名称".to_owned(); // 不打开无法提交的输入框。
                    continue;
                }
                let Some(portal_id) = editor.selected_current_portal_id.clone() else {
                    editor.status = "请先在当前场景列表选择传送点".to_owned(); // 输入框不默认选择第一项。
                    continue;
                };
                let Some(record) = session.draft.as_ref().and_then(|draft| {
                    draft
                        .portals
                        .portals
                        .iter()
                        .find(|record| record.id == portal_id)
                }) else {
                    editor.status = "所选传送点已不存在，请重新选择".to_owned(); // 热更边界失败关闭。
                    continue;
                };
                let display_context =
                    PortalDisplayContext::new(&session, registry.as_deref(), &manifests); // 旧自动名在输入框中显示当前真实默认名。
                if editor.portal_name_edit.active
                    && editor.portal_name_edit.portal_id.as_ref() == Some(&portal_id)
                {
                    continue;
                } // 再次点击只定位官方光标，不重置已输入文字。
                let current_label = display_context.portal_label(record); // 自定义名直接保留，旧流水名解析为“前往目标场景”。
                editor.portal_name_edit.active = true; // 后续键盘和IME开始写入本会话。
                editor.portal_name_edit.portal_id = Some(portal_id); // 冻结明确稳定ID。
                editor.portal_name_edit.value = current_label; // 首次编辑从作者实际看到的名称开始。
                set_ime_enabled(&mut windows, true); // 允许输入王府、河道等中文区块名。
                editor.status = "正在编辑传送点名称；Enter应用，Esc取消".to_owned(); // 给出完整键盘收口方式。
            }
            EditorAction::PortalNameApply => {
                if editor.portal_name_edit.active {
                    apply_requested = true; // 按钮与Enter共用受检提交和撤销路径。
                } else {
                    editor.status = "请先点击名称输入框进行编辑".to_owned(); // 不提交Presentation中的只读文字。
                }
            }
            _ => {} // 删除和其他静态动作由各自系统消费。
        }
    }

    if !editor.portal_name_edit.active {
        keyboard_inputs.clear(); // 未聚焦时本系统不消费普通游戏字符。
        return;
    }
    if editor.portal_name_edit.portal_id.as_deref() != editor.selected_current_portal_id.as_deref()
    {
        editor.status = "传送点选择已变化，名称编辑已取消".to_owned(); // 列表切项不能把旧文字写到新点。
        close_portal_name_edit(&mut editor, &mut windows); // 清除冻结身份并关闭IME。
        keyboard_inputs.clear(); // 丢弃切项同帧的字符。
        return;
    }

    let ime_owns_keyboard =
        guard.block_shortcuts || official_inputs.iter().any(|text| text.is_composing()); // 官方组件独占候选过程，业务只保护快捷键。
    let mut cancel_requested = false; // Escape在循环后统一归还IME。
    for input in keyboard_inputs.read() {
        if !input.state.is_pressed() {
            continue; // 只处理按下边沿，长按重复由窗口消息自然提供。
        }
        if ime_owns_keyboard
            || !input_focus
                .get()
                .is_some_and(|entity| name_entities.contains(entity))
        {
            continue;
        } // 当前批次的普通键盘消息属于操作系统组合输入。
        match (&input.logical_key, &input.text) {
            (Key::Enter, _) => {
                apply_requested = true; // 中文候选未打开时Enter提交名称。
            }
            (Key::Escape, _) => {
                cancel_requested = true; // Escape不修改草稿和撤销栈。
            }
            _ => {} // 中文候选期间普通按键由输入法拥有。
        }
    }
    if cancel_requested {
        close_portal_name_edit(&mut editor, &mut windows); // 丢弃尚未提交的名称。
        editor.status = "已取消传送点名称修改".to_owned(); // 明确没有产生dirty和撤销记录。
        return;
    }
    if apply_requested && !ime_owns_keyboard {
        // 候选未确认或提交当帧不能用按钮抢先应用。
        apply_portal_name(
            &mut commands,   // 提交后刷新未保存预览。
            &preview_assets, // 复用共享视觉与中文字体。
            &mut editor,     // 消费名称输入并写状态。
            &mut session,    // 修改草稿和统一撤销栈。
            &mut windows,    // 成功后关闭IME。
        );
    }
}

/// 原位回显当前选中名称、中文预编辑光标和聚焦边框，不复制或重建UI实体。
pub(crate) fn sync_portal_name_editor(
    editor: Res<SceneEditorState>,               // 读取输入会话和当前稳定选择。
    session: Res<EditorAuthoringSession>,      // 非编辑态从最新草稿读取名称。
    registry: Option<Res<SceneContentRegistry>>, // 自动默认名解析跨场景显示名称。
    manifests: Res<Assets<SceneManifestAsset>>,  // 场景改名或热加载后刷新默认名称。
    mut texts: Query<&mut bevy::text::EditableText, With<SceneEditorPortalNameInputText>>, // 唯一输入框文字节点。
    mut inputs: Query<&mut BorderColor, With<SceneEditorPortalNameInput>>, // 唯一输入框聚焦边框。
) {
    let selected = editor
        .selected_current_portal_id
        .as_deref()
        .and_then(|portal_id| {
            session.draft.as_ref().and_then(|draft| {
                draft
                    .portals
                    .portals
                    .iter()
                    .find(|record| record.id == portal_id)
            })
        }); // 稳定ID命中失败时显示明确占位而不是上一项名称。
    let display_context = PortalDisplayContext::new(&session, registry.as_deref(), &manifests); // 非编辑态显示自动或自定义最终名称。
    if !editor.portal_name_edit.active {
        // 官方组件独占编辑期间的绘制与光标，不再拼接模拟竖线。
        let value = selected
            .map(|record| display_context.portal_label(record))
            .unwrap_or_default(); // 未选中保持空输入，外部标签说明用途。
        for mut text in &mut texts {
            // 只读态跟随当前所选传送点名称。
            if text.value().to_string() != value {
                crate::ui::text_input::replace_single_line_value(&mut text, &value, MAX_PORTAL_NAME_CHARS);
            } // 身份变化时装入新值。
        } // 稳定帧不重置官方编辑器。
    } // 编辑期间不覆盖任何预编辑内容。
    for mut border in &mut inputs {
        *border = BorderColor::all(if editor.portal_name_edit.active {
            colors::INPUT_ACTIVE_BORDER // 聚焦时使用统一亮铜边。
        } else {
            colors::COPPER_BORDER_WEAK // 只读态退回弱铜边。
        });
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_portal_name; // 回归作者名称边界，不启动Bevy窗口。

    #[test]
    fn portal_name_accepts_regions_and_rejects_empty_or_delimiter() {
        assert_eq!(
            normalize_portal_name(" 前往王府 ").as_deref(),
            Ok("前往王府")
        ); // 中文区域名去空白后合法。
        assert!(normalize_portal_name("   ").is_err()); // 空名称不能覆盖默认名。
        assert!(normalize_portal_name("前往王府 | 调试").is_err()); // 固定列表分隔符不能混入名称。
    }
}
