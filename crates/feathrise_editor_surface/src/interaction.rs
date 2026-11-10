//! 地表预设列表、中央标签、字段输入和场景局部层分配交互。

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::input::keyboard::Key;
use bevy::input_focus::FocusCause; // 使用主菜单已验证的官方焦点。
use bevy::prelude::*;
use bevy::text::EditableText; // 业务只读取已确认文本。
use bevy::window::{PrimaryWindow, Window};
use feathrise_scene_content::{SurfacePresetDescriptor, SurfacePresetRegistry, SurfaceProjection};

use super::persistence::{start_delete, start_upsert};
use super::types::{
    SurfaceDraft, SurfaceDraftMode, SurfacePresetAction, SurfacePresetEditorState,
    SurfacePresetField, SurfaceSection, SurfaceTextureField,
};
use crate::helpers::set_ime_enabled;
use feathrise_editor_core::{PresetWorkspaceState, PresetWorkspaceTab};
use feathrise_editor_surface_model::{host::SurfaceEditorHost, system_params::*};

const MAX_NUMBER_CHARS: usize = 64;

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_buttons(
    editor: &mut dyn SurfaceEditorHost,
    workspace: &mut PresetWorkspaceState,
    params: ButtonsParams,
) {
    let ButtonsParams {
        buttons,
        fields,
        guard,
        mut input_focus,
        mut state,
        registry,
        mut windows,
    } = params;
    if !editor.is_open() {
        return;
    }
    for (entity, interaction, button) in &buttons {
        // 保留点击实体身份。
        if !interaction.is_clicked() {
            continue;
        }
        let action = button.0;
        if matches!(
            action,
            SurfacePresetAction::Focus(SurfacePresetField::Label)
        ) && state.active_field == Some(SurfacePresetField::Label)
        {
            // 重复点击仅移动官方光标。
            input_focus.set(entity, FocusCause::Navigated); // 保留选区与候选。
            continue; // 不重复提交或重建输入框。
        } // 其他动作继续业务处理。
        if state.active_field == Some(SurfacePresetField::Label)
            && (guard.block_shortcuts || fields.iter().any(EditableText::is_composing))
            && !matches!(
                action,
                SurfacePresetAction::Cancel | SurfacePresetAction::CloseTab(_)
            )
        {
            // 候选不能提前进入草稿。
            state.status = "请先确认输入法候选".to_owned(); // 告知阻止提交的原因。
            editor.set_status(state.status.clone()); // 同步状态栏。
            continue; // 保持当前输入会话。
        } // 取消仍可放弃候选。

        if matches!(action, SurfacePresetAction::SelectPreset(_)) && editor.surface_busy() {
            // 点击即写作者层，因此等待当前地表事务完成。
            state.status = "地表操作正在完成，请稍候再选择预设".to_owned(); // 明确反馈被阻止的创建请求。
            editor.set_status(state.status.clone()); // 全局状态与预设卡保持一致。
            continue; // 不更改所选预设或作者层。
        } // 普通预设表单操作不受该地表结构门影响。
        if state.active_field == Some(SurfacePresetField::Label)
            && let Ok(editable) = fields.single()
        {
            // 点击按钮时直接取官方已确认值，避免焦点先变导致缓冲落后一帧。
            state.input_buffer = editable.value().to_string(); // 保存与数值切换都提交完整名称。
        } // 非名称输入继续使用原缓冲。
        if !matches!(action, SurfacePresetAction::DeleteSelected) {
            state.delete_confirmation_id = None;
        }
        if !matches!(
            action,
            SurfacePresetAction::Cancel | SurfacePresetAction::CloseTab(_)
        ) && let Err(error) =
            commit_pending_input(&mut state, workspace.active_tab, workspace, &mut windows)
        {
            state.status = error;
            editor.set_status(state.status.clone());
            if action == SurfacePresetAction::Save {
                state.record_save_warning(editor, workspace.active_tab);
            }
            continue;
        }
        if !matches!(
            action,
            SurfacePresetAction::Focus(SurfacePresetField::Label)
        ) {
            input_focus.clear();
        } // 验证成功或取消后才释放焦点，错误继续保留名称编辑。
        let result = dispatch_action(
            action,
            &mut state,
            workspace,
            editor,
            registry.as_deref(),
            &mut windows,
        );
        if let Err(error) = result {
            state.status = error;
            if action == SurfacePresetAction::Save {
                editor.set_status(state.status.clone());
                state.record_save_warning(editor, workspace.active_tab);
            }
        }
        if matches!(
            action,
            SurfacePresetAction::Focus(SurfacePresetField::Label)
        ) && state.active_field == Some(SurfacePresetField::Label)
        {
            // 名称成功激活后才设置官方焦点。
            input_focus.set(entity, FocusCause::Navigated); // 聚焦真实可见输入框。
        } // 数值输入不占用官方中文焦点。
        editor.set_status(state.status.clone());
    }
}

#[allow(clippy::too_many_arguments)]
fn dispatch_action(
    action: SurfacePresetAction,
    state: &mut SurfacePresetEditorState,
    workspace: &mut PresetWorkspaceState,
    editor: &mut dyn SurfaceEditorHost,
    registry: Option<&SurfacePresetRegistry>,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    match action {
        SurfacePresetAction::SelectPreset(index) => select_preset(index, state, editor, registry),
        SurfacePresetAction::Create => create_preset(state, workspace, registry),
        SurfacePresetAction::EditSelected => edit_selected(state, workspace, editor, registry),
        SurfacePresetAction::DeleteSelected => delete_selected(state, editor, registry),
        SurfacePresetAction::ActivateTab(id) => activate_tab(state, workspace, id, windows),
        SurfacePresetAction::CloseTab(id) => {
            close_tab(state, workspace, id, windows, editor.fallback_tab())
        }
        SurfacePresetAction::Section(section) => switch_section(state, workspace, section),
        SurfacePresetAction::Focus(field) => focus_field(state, workspace, field, windows),
        SurfacePresetAction::ApplySelectedAsset(kind) => {
            set_texture(state, workspace, editor, kind, false)
        }
        SurfacePresetAction::ClearTexture(kind) => {
            set_texture(state, workspace, editor, kind, true)
        }
        SurfacePresetAction::ToggleNormalY => {
            let draft = state
                .active_draft_mut(workspace.active_tab)
                .ok_or_else(|| "请先打开一个地表预设标签".to_owned())?;
            draft.descriptor.normal_y_flip = !draft.descriptor.normal_y_flip;
            touch(state, workspace);
            Ok(())
        }
        SurfacePresetAction::CycleProjection => {
            let draft = state
                .active_draft_mut(workspace.active_tab)
                .ok_or_else(|| "请先打开一个地表预设标签".to_owned())?;
            draft.descriptor.projection = match draft.descriptor.projection {
                SurfaceProjection::WorldXz => SurfaceProjection::Triplanar,
                SurfaceProjection::Triplanar => SurfaceProjection::WorldXz,
            };
            touch(state, workspace);
            Ok(())
        }
        SurfacePresetAction::Save => save_draft(state, workspace, registry),
        SurfacePresetAction::Cancel => {
            let PresetWorkspaceTab::Surface(id) = workspace.active_tab else {
                return Err("当前是场景或植被预设标签".to_owned());
            };
            close_tab(state, workspace, id, windows, editor.fallback_tab())
        }
    }
}

fn switch_section(
    state: &mut SurfacePresetEditorState,
    workspace: &mut PresetWorkspaceState,
    section: SurfaceSection,
) -> Result<(), String> {
    let PresetWorkspaceTab::Surface(id) = workspace.active_tab else {
        return Err("当前不是地表预设标签".to_owned());
    };
    let tab = state
        .tabs
        .iter_mut()
        .find(|tab| tab.id == id)
        .ok_or_else(|| "活动地表预设标签已不存在".to_owned())?;
    if tab.section != section {
        tab.section = section;
        touch(state, workspace); // 仅换正文；进入此处前仍按原协议验证并提交已确认字段。
    }
    Ok(()) // 不启动源图处理，不改目录基线或场景作者层。
}

fn select_preset(
    index: usize,
    state: &mut SurfacePresetEditorState,
    editor: &mut dyn SurfaceEditorHost,
    registry: Option<&SurfacePresetRegistry>,
) -> Result<(), String> {
    let registry = registry.ok_or_else(|| "PBR 地表预设目录尚未加载".to_owned())?;
    let preset = registry
        .presets()
        .get(index)
        .ok_or_else(|| "地表预设列表已热更，请等待界面同步".to_owned())?;
    state.status = editor.select_preset(preset, registry)?;
    Ok(())
}

fn create_preset(
    state: &mut SurfacePresetEditorState,
    workspace: &mut PresetWorkspaceState,
    registry: Option<&SurfacePresetRegistry>,
) -> Result<(), String> {
    if state.busy() {
        return Err("地表目录任务进行中，暂不能新建".to_owned());
    }
    let registry = registry.ok_or_else(|| "PBR 地表预设目录尚未加载".to_owned())?;
    let id = unique_id(registry);
    let descriptor = SurfacePresetDescriptor {
        id,
        aliases: Vec::new(),
        label: "新地表".to_owned(),
        base_color_srgba: [0.55, 0.48, 0.36, 1.0],
        base_color_texture_path: None,
        normal_texture_path: None,
        arm_height_texture_path: None,
        tile_meters: 4.0,
        normal_strength: 1.0,
        normal_y_flip: false,
        height_blend_strength: 0.0,
        perceptual_roughness: 0.9,
        metallic: 0.0,
        crater_response: 1.0, // 新建预设默认与现有土壤技能行为一致。
        projection: SurfaceProjection::WorldXz,
    };
    let id = state.open_draft(SurfaceDraft::new(SurfaceDraftMode::Create, descriptor));
    workspace.active_tab = PresetWorkspaceTab::Surface(id);
    touch(state, workspace);
    state.status = "已打开新 PBR 地表标签；可从右侧 Asset 浏览器应用三类源图".to_owned();
    Ok(())
}

fn edit_selected(
    state: &mut SurfacePresetEditorState,
    workspace: &mut PresetWorkspaceState,
    editor: &dyn SurfaceEditorHost,
    registry: Option<&SurfacePresetRegistry>,
) -> Result<(), String> {
    if state.busy() {
        return Err("地表目录任务进行中，暂不能编辑".to_owned());
    }
    let registry = registry.ok_or_else(|| "PBR 地表预设目录尚未加载".to_owned())?;
    let id = editor
        .selected_preset()
        .ok_or_else(|| "请先点击左栏一个地表预设".to_owned())?;
    let preset = registry
        .resolve(id)
        .ok_or_else(|| format!("所选地表预设{id}已不存在"))?
        .clone();
    let label = preset.label.clone();
    let tab_id = state.open_draft(SurfaceDraft::new(
        SurfaceDraftMode::Edit {
            original_id: preset.id.clone(),
        },
        preset,
    ));
    workspace.active_tab = PresetWorkspaceTab::Surface(tab_id);
    touch(state, workspace);
    state.status = format!("已在中央打开“{label}”完整 PBR 编辑标签");
    Ok(())
}

fn delete_selected(
    state: &mut SurfacePresetEditorState,
    editor: &dyn SurfaceEditorHost,
    registry: Option<&SurfacePresetRegistry>,
) -> Result<(), String> {
    if state.busy() {
        return Err("地表目录任务进行中，暂不能删除".to_owned());
    }
    let registry = registry.ok_or_else(|| "PBR 地表预设目录尚未加载".to_owned())?;
    let id = editor
        .selected_preset()
        .ok_or_else(|| "请先点击左栏一个地表预设".to_owned())?;
    let preset = registry
        .resolve(id)
        .ok_or_else(|| "所选预设已不存在".to_owned())?;
    if state.tabs.iter().any(|tab| {
        matches!(&tab.draft.mode, SurfaceDraftMode::Edit { original_id } if original_id == &preset.id)
    }) {
        return Err("该地表预设仍在中央编辑标签中；请先关闭标签".to_owned());
    }
    if state.delete_confirmation_id.as_deref() != Some(preset.id.as_str()) {
        state.delete_confirmation_id = Some(preset.id.clone());
        state.status = format!(
            "再次点击删除确认移除「{}」；任何场景引用都会自动阻止",
            preset.label
        );
        return Ok(());
    }
    state.delete_confirmation_id = None;
    start_delete(state, registry, editor, preset)
}

fn activate_tab(
    state: &mut SurfacePresetEditorState,
    workspace: &mut PresetWorkspaceState,
    id: u64,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    if !state.tabs.iter().any(|tab| tab.id == id) {
        return Err("目标地表预设标签已关闭".to_owned());
    }
    clear_input(state, windows);
    workspace.active_tab = PresetWorkspaceTab::Surface(id);
    for tab in &mut state.tabs {
        tab.close_confirmation = false;
    }
    touch(state, workspace);
    Ok(())
}

fn close_tab(
    state: &mut SurfacePresetEditorState,
    workspace: &mut PresetWorkspaceState,
    id: u64,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
    fallback: PresetWorkspaceTab,
) -> Result<(), String> {
    if state.busy() {
        return Err("地表目录保存或热采用期间不能关闭标签".to_owned());
    }
    let index = state
        .tabs
        .iter()
        .position(|tab| tab.id == id)
        .ok_or_else(|| "目标地表预设标签已关闭".to_owned())?;
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
    let active = workspace.active_tab == PresetWorkspaceTab::Surface(id);
    let closed = state.tabs.remove(index);
    if active {
        workspace.active_tab = state
            .tabs
            .get(index.saturating_sub(1))
            .map(|tab| PresetWorkspaceTab::Surface(tab.id))
            .or(Some(fallback))
            .unwrap_or(PresetWorkspaceTab::Scene);
    }
    state.status = format!("已关闭“{}”地表预设标签", closed.draft.descriptor.label);
    touch(state, workspace);
    Ok(())
}

fn focus_field(
    state: &mut SurfacePresetEditorState,
    workspace: &mut PresetWorkspaceState,
    field: SurfacePresetField,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    let value = state
        .active_draft(workspace.active_tab)
        .map(|draft| field_value(&draft.descriptor, field))
        .ok_or_else(|| "请先打开一个地表预设标签".to_owned())?;
    state.active_field = Some(field);
    state.input_buffer = value;
    set_ime_enabled(windows, field.accepts_text());
    if !field.accepts_text() {
        touch(state, workspace);
    } // 中文聚焦不销毁官方编辑器实体。
    Ok(())
}

fn set_texture(
    state: &mut SurfacePresetEditorState,
    workspace: &mut PresetWorkspaceState,
    editor: &dyn SurfaceEditorHost,
    kind: SurfaceTextureField,
    clear: bool,
) -> Result<(), String> {
    let value = if clear {
        None
    } else {
        let path = editor
            .selected_asset()
            .ok_or_else(|| "请先在右侧 Asset 浏览器选择 PNG/JPG/JPEG 贴图".to_owned())?;
        let extension = path.rsplit_once('.').map(|(_, extension)| extension);
        if !extension.is_some_and(|extension| {
            ["png", "jpg", "jpeg"]
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        }) {
            return Err("PBR 作者源只接受可后台解码的 PNG/JPG/JPEG".to_owned());
        }
        Some(path.to_owned())
    };
    let draft = state
        .active_draft_mut(workspace.active_tab)
        .ok_or_else(|| "请先打开一个地表预设标签".to_owned())?;
    match kind {
        SurfaceTextureField::BaseColor => draft.descriptor.base_color_texture_path = value,
        SurfaceTextureField::Normal => draft.descriptor.normal_texture_path = value,
        SurfaceTextureField::ArmHeight => draft.descriptor.arm_height_texture_path = value,
    }
    touch(state, workspace);
    Ok(())
}

fn save_draft(
    state: &mut SurfacePresetEditorState,
    workspace: &PresetWorkspaceState,
    registry: Option<&SurfacePresetRegistry>,
) -> Result<(), String> {
    let registry = registry.ok_or_else(|| "PBR 地表预设目录尚未加载".to_owned())?;
    let PresetWorkspaceTab::Surface(tab_id) = workspace.active_tab else {
        return Err("当前不是地表预设标签".to_owned());
    };
    let draft = state
        .active_draft(workspace.active_tab)
        .ok_or_else(|| "活动地表预设标签已不存在".to_owned())?;
    let mode = draft.mode.clone();
    let descriptor = draft.descriptor.clone();
    start_upsert(state, registry, &mode, descriptor, tab_id)
}

pub(crate) fn handle_text_input(
    editor_open: bool,
    modal_open: bool,
    workspace: &mut PresetWorkspaceState,
    params: TextParams,
) {
    let TextParams {
        mut keyboard_inputs,
        fields,
        numeric,
        official_inputs,
        guard,
        mut input_focus,
        mut state,
        mut windows,
    } = params;
    // 每帧处理唯一活动字段。
    let Some(field) = state.active_field else {
        keyboard_inputs.clear();
        return;
    }; // 不重放无焦点字符。
    if !editor_open || state.active_draft(workspace.active_tab).is_none() || state.busy() {
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
    if modal_open // 新建场景模态独占确认与取消键。
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
        match commit_pending_input(&mut state, workspace.active_tab, workspace, &mut windows) {
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
        touch(&mut state, workspace); // 重新显示草稿原值。
    } else if !field.accepts_text() && before != state.input_buffer {
        // 中文打字不允许重建整棵表单树。
        touch(&mut state, workspace); // 只刷新原数值输入显示。
    } // 无变化不递增显示修订。
} // 官方文本业务同步结束。

fn commit_pending_input(
    state: &mut SurfacePresetEditorState,
    active: PresetWorkspaceTab,
    workspace: &mut PresetWorkspaceState,
    windows: &mut Query<&mut Window, With<PrimaryWindow>>,
) -> Result<(), String> {
    let Some(field) = state.active_field else {
        return Ok(());
    };
    let input = state.input_buffer.clone();
    let draft = state
        .active_draft_mut(active)
        .ok_or_else(|| "活动地表预设标签已不存在".to_owned())?;
    commit_field(&mut draft.descriptor, field, input.trim())?;
    clear_input(state, windows);
    touch(state, workspace);
    Ok(())
}

fn field_value(descriptor: &SurfacePresetDescriptor, field: SurfacePresetField) -> String {
    match field {
        SurfacePresetField::Label => descriptor.label.clone(),
        SurfacePresetField::BaseColor(component) => descriptor
            .base_color_srgba
            .get(component)
            .copied()
            .unwrap_or_default()
            .to_string(), // UI组件由本模块生成，但失效实体也不能用越界索引拖垮F12。
        SurfacePresetField::TileMeters => descriptor.tile_meters.to_string(),
        SurfacePresetField::NormalStrength => descriptor.normal_strength.to_string(),
        SurfacePresetField::HeightBlendStrength => descriptor.height_blend_strength.to_string(),
        SurfacePresetField::Roughness => descriptor.perceptual_roughness.to_string(),
        SurfacePresetField::Metallic => descriptor.metallic.to_string(),
        SurfacePresetField::CraterResponse => descriptor.crater_response.to_string(),
    }
}

fn commit_field(
    descriptor: &mut SurfacePresetDescriptor,
    field: SurfacePresetField,
    value: &str,
) -> Result<(), String> {
    if field == SurfacePresetField::Label {
        if value.is_empty() {
            return Err("显示名称不能为空".to_owned());
        }
        descriptor.label = value.to_owned();
        return Ok(());
    }
    let number = value
        .parse::<f32>()
        .map_err(|error| format!("数值格式无效：{error}"))?;
    if !number.is_finite() {
        return Err("PBR 数值必须有限".to_owned());
    }
    match field {
        SurfacePresetField::BaseColor(component) if (0.0..=1.0).contains(&number) => {
            *descriptor
                .base_color_srgba
                .get_mut(component)
                .ok_or_else(|| "基础色通道索引无效".to_owned())? = number
        }
        SurfacePresetField::TileMeters if number > 0.0 => descriptor.tile_meters = number,
        SurfacePresetField::NormalStrength if number >= 0.0 => descriptor.normal_strength = number,
        SurfacePresetField::HeightBlendStrength if number >= 0.0 => {
            descriptor.height_blend_strength = number
        }
        SurfacePresetField::Roughness if (0.0..=1.0).contains(&number) => {
            descriptor.perceptual_roughness = number
        }
        SurfacePresetField::Metallic if (0.0..=1.0).contains(&number) => {
            descriptor.metallic = number
        }
        SurfacePresetField::CraterResponse if (0.0..=1.0).contains(&number) => {
            descriptor.crater_response = number // 0为不下沉、1为完整技能深度。
        }
        SurfacePresetField::Label => unreachable!(),
        _ => {
            return Err(
                "数值超出允许范围：颜色/粗糙度/金属度/浅坑响应为0..1，其余强度非负".to_owned(),
            );
        }
    }
    Ok(())
}

fn clear_input(
    state: &mut SurfacePresetEditorState,
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

fn unique_id(registry: &SurfacePresetRegistry) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let base = format!("surface-{millis}");
    if registry.resolve(&base).is_none() {
        return base;
    }
    (2_u32..)
        .map(|suffix| format!("{base}-{suffix}"))
        .find(|candidate| registry.resolve(candidate).is_none())
        .expect("u32后缀空间足以形成新地表预设ID")
}

fn touch(state: &mut SurfacePresetEditorState, workspace: &mut PresetWorkspaceState) {
    state.touch_ui();
    workspace.touch_ui(); // 中央共享标签条由植被工作区壳统一持有显示修订。
}

#[cfg(test)]
#[path = "interaction_tests.rs"]
mod tests;
