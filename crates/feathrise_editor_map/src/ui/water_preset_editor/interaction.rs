//! 水体预设选择、中央标签输入、贴图绑定和静水层引用应用。

use crate::ui::text_input::EditorTextInputGuard; // 保护候选确认所在帧。
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::{FocusCause, InputFocus}; // 使用主菜单已验证的官方焦点。
use bevy::prelude::*;
use bevy::text::EditableText; // 业务只读取已确认文本。
use bevy::window::{PrimaryWindow, Window};
use feathrise_map_content::WaterPresetRegistry;

use super::assets::set_texture;
use super::fields::{commit_field, field_value};
use super::ids::unique_id;
use super::persistence::{start_delete, start_upsert};
use super::types::{
    WaterDraft, WaterDraftMode, WaterPresetAction, WaterPresetButton, WaterPresetEditorState,
    WaterPresetField, WaterSection,
};
use crate::authoring::{EditorAuthoringSession, EditorCommand};
use crate::resources::MapEditorState;
use crate::types::EditorTool;
use crate::ui::actions::set_ime_enabled;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};

const MAX_NUMBER_CHARS: usize = 64;

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_buttons(
    buttons: Query<(Entity, &feathrise_editor_ui::EditorButtonClick, &WaterPresetButton), Changed<feathrise_editor_ui::EditorButtonClick>>, // 点击目标就是官方输入实体。
    fields: Query<&EditableText, With<WaterPresetButton>>, // 检查当前真实候选状态。
    guard: Res<EditorTextInputGuard>,                      // 候选确认所在帧不允许按钮提前保存。
    mut input_focus: ResMut<InputFocus>,                   // 切换业务字段时同步官方焦点。
    mut state: ResMut<WaterPresetEditorState>,
    mut workspace: ResMut<VegetationPresetEditorState>,
    mut editor: ResMut<MapEditorState>,
    registry: Option<Res<WaterPresetRegistry>>,
    mut session: ResMut<EditorAuthoringSession>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if !editor.open {
        return;
    }
    for (entity, interaction, button) in &buttons {
        // 保留点击实体身份。
        if !interaction.is_clicked() {
            continue;
        }
        let action = button.0;
        if matches!(action, WaterPresetAction::Focus(WaterPresetField::Label))
            && state.active_field == Some(WaterPresetField::Label)
        {
            // 重复点击仅移动官方光标。
            input_focus.set(entity, FocusCause::Navigated); // 保留选区与候选。
            continue; // 不重复提交或重建输入框。
        } // 其他动作继续业务处理。
        if state.active_field == Some(WaterPresetField::Label)
            && (guard.block_shortcuts || fields.iter().any(EditableText::is_composing))
            && !matches!(
                action,
                WaterPresetAction::Cancel | WaterPresetAction::CloseTab(_)
            )
        {
            // 候选不能提前进入草稿。
            state.status = "请先确认输入法候选".to_owned(); // 告知阻止提交的原因。
            editor.status = state.status.clone(); // 同步状态栏。
            continue; // 保持当前输入会话。
        } // 取消仍可放弃候选。

        if state.active_field == Some(WaterPresetField::Label)
            && let Ok(editable) = fields.single()
        {
            // 点击按钮时直接取官方已确认值，避免焦点先变导致缓冲落后一帧。
            state.input_buffer = editable.value().to_string(); // 保存与数值切换都提交完整名称。
        } // 非名称输入继续使用原缓冲。
        if !matches!(action, WaterPresetAction::DeleteSelected) {
            state.delete_confirmation_id = None;
        }
        if !matches!(
            action,
            WaterPresetAction::Cancel | WaterPresetAction::CloseTab(_)
        ) && let Err(error) = commit_pending_input(
            &mut state,
            workspace.active_tab,
            &mut workspace,
            &mut windows,
        ) {
            state.status = error;
            editor.status = state.status.clone();
            if action == WaterPresetAction::Save {
                state.record_save_warning(&mut editor, workspace.active_tab);
            }
            continue;
        }
        if !matches!(action, WaterPresetAction::Focus(WaterPresetField::Label)) {
            input_focus.clear();
        } // 验证成功或取消后才释放焦点，错误继续保留名称编辑。
        if let Err(error) = dispatch_action(
            action,
            &mut state,
            &mut workspace,
            &mut editor,
            registry.as_deref(),
            &mut session,
            &mut windows,
        ) {
            state.status = error;
            if action == WaterPresetAction::Save {
                editor.status = state.status.clone();
                state.record_save_warning(&mut editor, workspace.active_tab);
            }
        }
        if matches!(action, WaterPresetAction::Focus(WaterPresetField::Label))
            && state.active_field == Some(WaterPresetField::Label)
        {
            // 名称成功激活后才设置官方焦点。
            input_focus.set(entity, FocusCause::Navigated); // 聚焦真实可见输入框。
        } // 数值输入不占用官方中文焦点。
        editor.status = state.status.clone();
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_action(
    action: WaterPresetAction,
    state: &mut WaterPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    editor: &mut MapEditorState,
    registry: Option<&WaterPresetRegistry>,
    session: &mut EditorAuthoringSession,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    match action {
        WaterPresetAction::SelectPreset(index) => select_preset(index, state, editor, registry),
        WaterPresetAction::Create => create_preset(state, workspace, registry),
        WaterPresetAction::EditSelected => edit_selected(state, workspace, editor, registry),
        WaterPresetAction::DeleteSelected => delete_selected(state, editor, registry, session),
        WaterPresetAction::ApplyToSelectedStillWater => {
            apply_to_selected_still_water(state, editor, registry, session)
        }
        WaterPresetAction::ActivateTab(id) => activate_tab(state, workspace, id, windows),
        WaterPresetAction::CloseTab(id) => close_tab(state, workspace, id, windows),
        WaterPresetAction::Section(section) => switch_section(state, workspace, section),
        WaterPresetAction::Focus(field) => focus_field(state, workspace, field, windows),
        WaterPresetAction::ApplySelectedAsset(layer) => {
            set_texture(state, workspace, editor, layer, false)
        }
        WaterPresetAction::ClearTexture(layer) => {
            set_texture(state, workspace, editor, layer, true)
        }
        WaterPresetAction::Save => save_draft(state, workspace, registry),
        WaterPresetAction::Cancel => {
            let PresetWorkspaceTab::Water(id) = workspace.active_tab else {
                return Err("当前不是水体预设标签".to_owned());
            };
            close_tab(state, workspace, id, windows)
        }
    }
}

fn select_preset(
    index: usize,
    state: &mut WaterPresetEditorState,
    editor: &mut MapEditorState,
    registry: Option<&WaterPresetRegistry>,
) -> Result<(), String> {
    let registry = registry.ok_or_else(|| "水体预设目录尚未加载".to_owned())?;
    let preset = registry
        .presets()
        .get(index)
        .ok_or_else(|| "水体预设列表已热更，请等待界面同步".to_owned())?;
    editor.water_selected_preset_id = Some(preset.id.clone());
    state.status = format!(
        "已选择“{}”；可应用到当前静水体或用于新建静水体",
        preset.label
    );
    Ok(())
}

fn create_preset(
    state: &mut WaterPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    registry: Option<&WaterPresetRegistry>,
) -> Result<(), String> {
    if state.busy() {
        return Err("水体目录任务进行中，暂不能新建".to_owned());
    }
    let registry = registry.ok_or_else(|| "水体预设目录尚未加载".to_owned())?;
    let mut descriptor = registry
        .presets()
        .first()
        .cloned()
        .unwrap_or_else(super::defaults::new_descriptor);
    descriptor.id = unique_id(registry);
    descriptor.aliases.clear();
    descriptor.label = "新水体预设".to_owned();
    let id = state.open_draft(WaterDraft::new(WaterDraftMode::Create, descriptor));
    workspace.active_tab = PresetWorkspaceTab::Water(id);
    touch(state, workspace);
    state.status = "已打开新水体标签；四层波浪与两层微法线均可独立编辑".to_owned();
    Ok(())
}

fn edit_selected(
    state: &mut WaterPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    editor: &MapEditorState,
    registry: Option<&WaterPresetRegistry>,
) -> Result<(), String> {
    if state.busy() {
        return Err("水体目录任务进行中，暂不能编辑".to_owned());
    }
    let registry = registry.ok_or_else(|| "水体预设目录尚未加载".to_owned())?;
    let id = editor
        .water_selected_preset_id
        .as_deref()
        .ok_or_else(|| "请先点击左栏一个水体预设".to_owned())?;
    let preset = registry
        .resolve(id)
        .ok_or_else(|| format!("所选水体预设{id}已不存在"))?
        .clone();
    let label = preset.label.clone();
    let tab_id = state.open_draft(WaterDraft::new(
        WaterDraftMode::Edit {
            original_id: preset.id.clone(),
        },
        preset,
    ));
    workspace.active_tab = PresetWorkspaceTab::Water(tab_id);
    touch(state, workspace);
    state.status = format!("已在中央打开“{label}”完整水体编辑标签");
    Ok(())
}

fn delete_selected(
    state: &mut WaterPresetEditorState,
    editor: &MapEditorState,
    registry: Option<&WaterPresetRegistry>,
    session: &mut EditorAuthoringSession,
) -> Result<(), String> {
    if state.delete_preflight.take().is_some() {
        session.water_preset_delete_pending = false;
        state.status = "已取消水体预设删除检查，没有写入文件".into();
        return Ok(());
    }
    if state.busy() {
        return Err("水体目录任务进行中，暂不能删除".to_owned());
    }
    let registry = registry.ok_or_else(|| "水体预设目录尚未加载".to_owned())?;
    let id = editor
        .water_selected_preset_id
        .as_deref()
        .ok_or_else(|| "请先点击左栏一个水体预设".to_owned())?;
    let preset = registry
        .resolve(id)
        .ok_or_else(|| "所选预设已不存在".to_owned())?;
    if state.tabs.iter().any(|tab| {
        matches!(&tab.draft.mode, WaterDraftMode::Edit { original_id } if original_id == &preset.id)
    }) {
        return Err("该水体预设仍在中央编辑标签中；请先关闭标签".to_owned());
    }
    if state.delete_confirmation_id.as_deref() != Some(preset.id.as_str()) {
        state.delete_confirmation_id = Some(preset.id.clone());
        state.status = format!(
            "再次点击删除确认移除「{}」；任何地图引用都会自动阻止",
            preset.label
        );
        return Ok(());
    }
    state.delete_confirmation_id = None;
    start_delete(state, registry, session, preset)
}

fn apply_to_selected_still_water(
    state: &mut WaterPresetEditorState,
    editor: &mut MapEditorState,
    registry: Option<&WaterPresetRegistry>,
    session: &mut EditorAuthoringSession,
) -> Result<(), String> {
    if session.water_preset_delete_pending {
        return Err("水体预设删除事务尚未完成，暂不能新增引用".into());
    }
    if session.saving {
        return Err("保存期间不能替换静水预设".to_owned());
    }
    if editor.tool != EditorTool::Water {
        return Err("请先在水域对象列表选择一个静水体".to_owned());
    }
    let registry = registry.ok_or_else(|| "水体预设目录尚未加载".to_owned())?;
    let selected_id = editor
        .water_selected_preset_id
        .as_deref()
        .ok_or_else(|| "请先选择一个水体预设".to_owned())?;
    let preset = registry
        .resolve(selected_id)
        .ok_or_else(|| "所选水体预设已不存在".to_owned())?;
    let before = session
        .draft
        .as_ref()
        .and_then(|draft| draft.water.clone())
        .ok_or_else(|| "当前地图没有可编辑水域文件".to_owned())?;
    let mut after = before.clone();
    let layer = after
        .layers
        .get_mut(editor.selected_water_layer)
        .ok_or_else(|| "当前没有可应用预设的静水体".to_owned())?;
    layer.preset_id = preset.id.clone();
    layer.base_color_srgba = preset.base_color_srgba; // 同步兼容回退，使缺目录诊断状态仍保留作者所见基础材质。
    layer.perceptual_roughness = preset.perceptual_roughness;
    if before == after {
        state.status = format!("当前静水体已经使用“{}”", preset.label);
        return Ok(());
    }
    session
        .draft
        .as_mut()
        .ok_or_else(|| "地图作者草稿在应用期间失效".to_owned())?
        .water = Some(after.clone());
    session.push_bounded_water_history(EditorCommand::WaterDefinitionsChanged { before, after });
    session.mark_water_definition_changed(editor);
    state.status = format!("已把“{}”应用到当前静水体；预览正在重建", preset.label);
    Ok(())
}

fn activate_tab(
    state: &mut WaterPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    id: u64,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    if !state.tabs.iter().any(|tab| tab.id == id) {
        return Err("目标水体预设标签已关闭".to_owned());
    }
    clear_input(state, windows);
    workspace.active_tab = PresetWorkspaceTab::Water(id);
    for tab in &mut state.tabs {
        tab.close_confirmation = false;
    }
    touch(state, workspace);
    Ok(())
}

fn close_tab(
    state: &mut WaterPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    id: u64,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    if state.busy() {
        return Err("水体目录保存或热采用期间不能关闭标签".to_owned());
    }
    let index = state
        .tabs
        .iter()
        .position(|tab| tab.id == id)
        .ok_or_else(|| "目标水体预设标签已关闭".to_owned())?;
    if state.tabs[index].draft.dirty() && !state.tabs[index].close_confirmation {
        for tab in &mut state.tabs {
            tab.close_confirmation = tab.id == id;
        }
        state.status = format!(
            "“{}”有未保存修改；再次点击标签关闭按钮确认丢弃",
            state.tabs[index].draft.descriptor.label
        );
        touch(state, workspace);
        return Ok(());
    }
    clear_input(state, windows);
    let active = workspace.active_tab == PresetWorkspaceTab::Water(id);
    let closed = state.tabs.remove(index);
    if active {
        workspace.active_tab = state
            .tabs
            .get(index.saturating_sub(1))
            .map(|tab| PresetWorkspaceTab::Water(tab.id))
            .or_else(|| {
                workspace
                    .tabs
                    .last()
                    .map(|tab| PresetWorkspaceTab::Vegetation(tab.id))
            })
            .unwrap_or(PresetWorkspaceTab::Map);
    }
    state.status = format!("已关闭“{}”水体预设标签", closed.draft.descriptor.label);
    touch(state, workspace);
    Ok(())
}

/// 按钮入口先验证旧字段；这里只换UI分区，不写目录、不换代独立水面样品。
fn switch_section(
    state: &mut WaterPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    section: WaterSection,
) -> Result<(), String> {
    let PresetWorkspaceTab::Water(id) = workspace.active_tab else {
        return Err("当前不是水体预设标签".to_owned());
    };
    let tab = state
        .tabs
        .iter_mut()
        .find(|tab| tab.id == id)
        .ok_or_else(|| "活动水体预设标签已不存在".to_owned())?;
    if tab.section != section {
        tab.section = section;
        touch(state, workspace);
    }
    Ok(())
}

fn focus_field(
    state: &mut WaterPresetEditorState,
    workspace: &mut VegetationPresetEditorState,
    field: WaterPresetField,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    let value = state
        .active_draft(workspace.active_tab)
        .map(|draft| field_value(&draft.descriptor, field))
        .ok_or_else(|| "请先打开一个水体预设标签".to_owned())?;
    state.active_field = Some(field);
    state.input_buffer = value;
    set_ime_enabled(windows, field.accepts_text());
    if !field.accepts_text() {
        touch(state, workspace);
    } // 中文聚焦不销毁官方编辑器实体。
    Ok(())
}

fn save_draft(
    state: &mut WaterPresetEditorState,
    workspace: &VegetationPresetEditorState,
    registry: Option<&WaterPresetRegistry>,
) -> Result<(), String> {
    let registry = registry.ok_or_else(|| "水体预设目录尚未加载".to_owned())?;
    let PresetWorkspaceTab::Water(tab_id) = workspace.active_tab else {
        return Err("当前不是水体预设标签".to_owned());
    };
    let draft = state
        .active_draft(workspace.active_tab)
        .ok_or_else(|| "活动水体预设标签已不存在".to_owned())?;
    start_upsert(
        state,
        registry,
        &draft.mode.clone(),
        draft.descriptor.clone(),
        tab_id,
    )
}

pub(super) fn handle_text_input(
    // 官方组件编辑中文，本系统保留业务提交与数值输入。
    mut keyboard_inputs: MessageReader<KeyboardInput>, // 只处理业务键和数值字符。
    fields: Query<(Entity, &EditableText), With<WaterPresetButton>>, // 当前真实名称控件。
    modal_browser: Option<Res<crate::resources::MapBrowserState>>, // 文件夹模态必须阻止底层字段快捷键。
    numeric: Option<Res<feathrise_editor_ui::ActiveNumericInput>>, // 专业数值弹框独占自己的输入。
    official_inputs: Query<(), With<EditableText>>, // 判断焦点是否已交给其他官方文本域。
    guard: Res<EditorTextInputGuard>,               // 候选 Commit 帧不重复执行提交键。
    mut input_focus: ResMut<InputFocus>,            // 结束会话时释放官方焦点。
    mut state: ResMut<WaterPresetEditorState>,      // 当前字段缓冲与草稿。
    mut workspace: ResMut<VegetationPresetEditorState>, // 共享标签身份。
    editor: Res<MapEditorState>,                    // 检查编辑器是否仍开启。
    mut windows: Query<&mut Window, With<PrimaryWindow>>, // 数值仍沿用原 IME 开关。
) {
    // 每帧处理唯一活动字段。
    let Some(field) = state.active_field else {
        keyboard_inputs.clear();
        return;
    }; // 不重放无焦点字符。
    if !editor.open || state.active_draft(workspace.active_tab).is_none() || state.busy() {
        // 会话失效立即退出。
        if fields
            .iter()
            .any(|(entity, _)| input_focus.get() == Some(entity))
        {
            input_focus.clear();
        } // 只释放本域旧字段，不覆盖其他输入框焦点。
        clear_input(&mut state, &mut windows); // 清除业务缓冲。
        keyboard_inputs.clear(); // 丢弃失效事件。
        return; // 不能把旧候选提交到新标签。
    } // 只有有效草稿能继续输入。
    let mut composing = false; // 数值没有中文组合态。
    if field.accepts_text() {
        // 文字只从官方组件读取。
        let Ok((entity, editable)) = fields.single() else {
            keyboard_inputs.clear();
            return;
        }; // 重建期间不猜测目标。
        state.input_buffer = editable.value().to_string(); // 即使鼠标已切换焦点，也先读回官方本帧刚应用的最终文字。
        if input_focus.get() != Some(entity) {
            keyboard_inputs.clear();
            return;
        } // 焦点交给其他输入框后，本域不得消费它的快捷键。
        composing = editable.is_composing() || guard.block_shortcuts; // 候选帧及确认帧都让位输入法。
    } // 光标、选区与剪贴板均由官方处理。
    if editor.new_map_dialog_open // 新建地图模态独占确认与取消键。
        || modal_browser.as_ref().is_some_and(|browser| browser.dialog.is_some()) // 文件夹模态不能提交底层预设。
        || numeric.as_ref().is_some_and(|input| input.entity.is_some()) // 专业数值输入不得串写旧字段。
        || (!field.accepts_text() && input_focus.get().is_some_and(|entity| official_inputs.contains(entity)))
    // 旧数值字段不消费新中文焦点的快捷键。
    {
        // 保留字段缓冲，等待用户显式返回或取消会话。
        keyboard_inputs.clear(); // 当前消息仅属于上层输入目标。
        return; // 不提交、取消或改写底层草稿。
    } // 无模态且确实持有输入权限时才执行业务键。
    let before = state.input_buffer.clone(); // 数值只在变化时刷新显示。
    let mut submit = false; // 原 Enter、Tab 提交语义。
    let mut cancel = false; // 原 Escape 取消语义。
    for input in keyboard_inputs.read() {
        // 推进独立业务事件游标。
        if !input.state.is_pressed() || composing {
            continue;
        } // 组合过程不触发业务动作。
        match (&input.logical_key, &input.text) {
            // 不再自行拼接中文或普通文本。
            (Key::Enter | Key::Tab, _) => submit = true, // 提交完整字段。
            (Key::Escape, _) => cancel = true,           // 放弃本次修改。
            (Key::Backspace, _) if !field.accepts_text() => {
                state.input_buffer.pop();
            } // 仅数值保持旧退格。
            (_, Some(text)) if !field.accepts_text() => push_number(&mut state.input_buffer, text), // 仅数值保持旧过滤。
            _ => {} // 文字编辑由官方系统接收。
        } // 当前事件完成。
    } // 所有事件已消费。
    if submit {
        // 先验证业务值，再结束输入。
        match commit_pending_input(
            &mut state,
            workspace.active_tab,
            &mut workspace,
            &mut windows,
        ) {
            // 保留非空名称和数值范围检查。
            Ok(()) => {
                if field.accepts_text() {
                    input_focus.clear();
                }
                state.status = "字段已提交到当前草稿".to_owned();
            } // 成功后释放焦点。
            Err(error) => state.status = error, // 错误保留输入供用户修正。
        } // 提交结果处理完成。
    } else if cancel {
        // 取消不修改正式草稿。
        if field.accepts_text() {
            input_focus.clear();
        } // 释放官方焦点。
        clear_input(&mut state, &mut windows); // 丢弃未提交缓冲。
        state.status = "已放弃本次字符修改".to_owned(); // 保留原提示。
        touch(&mut state, &mut workspace); // 重新显示草稿原值。
    } else if !field.accepts_text() && before != state.input_buffer {
        // 中文打字不允许重建整棵表单树。
        touch(&mut state, &mut workspace); // 只刷新原数值输入显示。
    } // 无变化不递增显示修订。
} // 官方文本业务同步结束。

fn commit_pending_input(
    state: &mut WaterPresetEditorState,
    active: PresetWorkspaceTab,
    workspace: &mut VegetationPresetEditorState,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    let Some(field) = state.active_field else {
        return Ok(());
    };
    let input = state.input_buffer.clone();
    let draft = state
        .active_draft_mut(active)
        .ok_or_else(|| "活动水体预设标签已不存在".to_owned())?;
    commit_field(&mut draft.descriptor, field, input.trim())?;
    clear_input(state, windows);
    touch(state, workspace);
    Ok(())
}

fn clear_input(
    state: &mut WaterPresetEditorState,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) {
    state.active_field = None;
    state.input_buffer.clear();
    set_ime_enabled(windows, false);
}

fn push_number(target: &mut String, value: &str) {
    if target.len() >= MAX_NUMBER_CHARS {
        return;
    }
    target.extend(
        value
            .chars()
            .filter(|character| character.is_ascii_digit() || ".-+eE".contains(*character)),
    );
}

fn touch(state: &mut WaterPresetEditorState, workspace: &mut VegetationPresetEditorState) {
    state.touch_ui();
    workspace.touch_ui();
}
