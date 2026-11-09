//! 建筑预设目录选择、中央草稿输入和专用摆放参数交互。

use feathrise_editor_ui::UiInteraction;
use std::time::{SystemTime, UNIX_EPOCH}; // 新建预设使用不依赖目录顺序的会话外稳定候选 ID。

use bevy::input::keyboard::{Key, KeyboardInput}; // 数值字段只接受有限 ASCII 语法。
use bevy::input_focus::{FocusCause, InputFocus}; // 中文名称复用 Bevy 官方编辑焦点。
use bevy::prelude::*; // ECS 系统参数、窗口与 UI 交互。
use bevy::text::EditableText; // 中文、选区、剪贴板和输入法由官方组件处理。
use bevy::window::{PrimaryWindow, Window}; // 数值与中文字段切换时同步 IME 开关。
use feathrise_map_content::{
    BuildingConnectionPointDescriptor, BuildingInteriorKind, BuildingPresetCategory,
    BuildingPresetDescriptor, BuildingPresetRegistry, MAX_BUILDING_CONNECTION_POINTS,
}; // 交互只编辑公共强类型目录格式。

use super::fields::{commit_field, field_value}; // 字段解析与范围校验只有一个实现。
use super::persistence::{start_delete, start_upsert}; // 目录修改统一走乐观并发与原子发布。
use super::types::*; // 本域动作、草稿、资源与文字 marker。
use crate::authoring::EditorAuthoringSession; // 删除时同时保护当前未保存地图引用。
use crate::resources::MapEditorState; // 右侧资产选择、建筑选择与全局状态栏。
use crate::types::EditorTool; // “开始摆放”进入专用建筑工具。
use crate::ui::actions::set_ime_enabled; // 与其余 F12 文本域共用窗口 IME 仲裁。
use crate::ui::asset_preview::AssetPreviewState; // 保存 Scene0 或当前静态 primitive 精确目标。
use crate::ui::text_input::EditorTextInputGuard; // 输入法候选确认帧不能顺带执行按钮事务。
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState}; // 建筑加入唯一中央工作区。

const MAX_NUMBER_CHARS: usize = 64; // 足够表达 f32，拒绝异常长数字缓冲。

mod draft_actions; // 目录、标签、草稿和摆放控制按钮动作。
mod numeric_input; // 中文与数值字段输入、提交和共享小型帮助函数。

use draft_actions::dispatch_action; // 按钮入口保持精简，只负责输入门和动作分派。
use numeric_input::commit_pending_input; // 任一非取消动作先提交当前字段。
pub(super) use numeric_input::handle_text_input; // 保持建筑插件原有系统注册路径。

/// 处理建筑列表、标签、字段和摆放控制按钮。
#[allow(clippy::too_many_arguments)] // 每个资源分别证明资产、目录、地图和工作区身份仍有效。
pub(super) fn handle_buttons(
    buttons: Query<(Entity, &feathrise_editor_ui::EditorButtonClick, &BuildingPresetButton), Changed<feathrise_editor_ui::EditorButtonClick>>,
    fields: Query<(&EditableText, &BuildingPresetButton)>, // 当前名称或连接技术字段的官方文本源。
    guard: Res<EditorTextInputGuard>,                      // 输入法组合与确认期间阻止提前保存。
    mut input_focus: ResMut<InputFocus>,                   // 建筑名称或连接技术字段占用官方焦点。
    mut state: ResMut<BuildingPresetEditorState>,          // 草稿、输入与目录任务状态。
    mut workspace: ResMut<VegetationPresetEditorState>,    // 中央活动标签与显示修订。
    mut editor: ResMut<MapEditorState>,                    // 当前资产、建筑选择、工具和状态栏。
    registry: Option<Res<BuildingPresetRegistry>>,         // 首载前按钮会给出明确等待反馈。
    preview: Res<AssetPreviewState>,                       // 新建时冻结作者当前选择的模型目标。
    session: Res<EditorAuthoringSession>,                  // 删除保护包含未保存 placements。
    mut controls: ResMut<BuildingPlacementControls>, // 九十度旋转和网格吸附属于当前 F12 会话。
    mut windows: Query<&mut Window, With<PrimaryWindow>>, // 焦点切换同步 IME 状态。
) {
    if !editor.open {
        return; // F12 关闭后不消费隐藏界面的迟到 Interaction。
    }
    for (entity, interaction, button) in &buttons {
        if !interaction.is_clicked() {
            continue; // 悬停只由表现系统处理。
        }
        let action = button.0; // Copy 动作不会借用即将重建的 UI 实体。
        if let BuildingPresetAction::Focus(field) = action
            && field.accepts_text()
            && state.active_field == Some(field)
        {
            input_focus.set(entity, FocusCause::Navigated); // 重复点击只移动光标，不销毁候选。
            continue;
        }
        if state
            .active_field
            .is_some_and(BuildingPresetField::accepts_text)
            && (state
                .active_field
                .is_some_and(BuildingPresetField::accepts_ime)
                && guard.block_shortcuts
                || state.active_field.is_some_and(|field| {
                    fields.iter().any(|(editable, button)| {
                        button.0 == BuildingPresetAction::Focus(field) && editable.is_composing()
                    })
                }))
            && !matches!(
                action,
                BuildingPresetAction::Cancel | BuildingPresetAction::CloseTab(_)
            )
        {
            state.status = "请先确认建筑名称的输入法候选".to_owned();
            editor.status = state.status.clone();
            continue; // 未确认候选不能进入持久草稿。
        }
        if let Some(field) = state.active_field.filter(|field| field.accepts_text())
            && let Some((editable, _)) = fields
                .iter()
                .find(|(_, button)| button.0 == BuildingPresetAction::Focus(field))
        {
            state.input_buffer = editable.value().to_string(); // 按钮同帧读取官方已确认文本。
        }
        if !matches!(action, BuildingPresetAction::DeleteSelected) {
            state.delete_confirmation_id = None; // 删除二次确认只对连续两次同动作有效。
        }
        if !matches!(
            action,
            BuildingPresetAction::Cancel | BuildingPresetAction::CloseTab(_)
        ) && let Err(error) = commit_pending_input(
            &mut state,
            workspace.active_tab,
            &mut workspace,
            &mut windows,
        ) {
            state.status = error;
            editor.status = state.status.clone();
            continue; // 旧字段无效时不能继续切换或保存。
        }
        if !matches!(action, BuildingPresetAction::Focus(field) if field.accepts_text()) {
            input_focus.clear(); // 离开中文字段后释放官方选择和光标。
        }
        if let Err(error) = dispatch_action(
            action,
            &mut state,
            &mut workspace,
            &mut editor,
            registry.as_deref(),
            &preview,
            &session,
            &mut controls,
            &mut windows,
        ) {
            state.status = error; // 所有失败都保留旧有效草稿、目录和地图。
        }
        if let BuildingPresetAction::Focus(field) = action
            && field.accepts_text()
            && state.active_field == Some(field)
        {
            input_focus.set(entity, FocusCause::Navigated); // 业务焦点成功后再交给官方文本系统。
        }
        editor.status = state.status.clone(); // 左栏和全局状态栏显示同一具体结果。
    }
}

/// 原位同步左栏选择摘要、目录任务状态和摆放控制值。
pub(super) fn sync_panel_text(
    editor: Res<MapEditorState>,
    state: Res<BuildingPresetEditorState>,
    controls: Res<BuildingPlacementControls>,
    registry: Option<Res<BuildingPresetRegistry>>,
    mut texts: Query<
        (
            &mut Text,
            Option<&BuildingPresetSummaryText>,
            Option<&BuildingPresetStatusText>,
            Option<&BuildingPlacementControlText>,
            Option<&super::components::BuildingPlacementOptionText>,
        ),
        Or<(
            With<BuildingPresetSummaryText>,
            With<BuildingPresetStatusText>,
            With<BuildingPlacementControlText>,
            With<super::components::BuildingPlacementOptionText>,
        )>,
    >,
) {
    let summary = editor
        .building_selected_preset_id
        .as_deref()
        .and_then(|id| registry.as_deref()?.resolve(id))
        .map_or_else(
            || "当前建筑：尚未选择".to_owned(),
            |preset| {
                format!(
                    "已选建筑：{}\n{} · {}",
                    preset.label,
                    preset.category.label(),
                    preset.interior.label(),
                )
            },
        );
    let status = match registry.as_deref() {
        None => "建筑预设目录加载中…".to_owned(),
        Some(registry) if registry.last_error.is_some() => format!(
            "{}\n目录更新失败，仍沿用上一有效快照：{}",
            state.status,
            registry.last_error.as_deref().unwrap_or_default(),
        ),
        Some(_) => state.status.clone(),
    };
    let grid = if controls.grid_step_meters <= 0.0 {
        "关闭".to_owned()
    } else {
        format!("{:.1} m", controls.grid_step_meters)
    };
    let leveling = if controls.level_terrain {
        "开启"
    } else {
        "关闭"
    };
    let connection_snap = if controls.connection_snap {
        "开启"
    } else {
        "关闭"
    };
    let tool = if editor.tool == EditorTool::Building {
        "单栋摆放"
    } else {
        "选择"
    };
    let control = format!("当前：{tool} · 朝向 {:.0}°", controls.yaw_degrees);
    let grid_label = format!("网格：{grid} · 切换");
    let connection_label = format!("连接吸附：{connection_snap}");
    let leveling_label = format!("单栋整平：{leveling}");
    for (mut text, summary_marker, status_marker, control_marker, option_marker) in &mut texts {
        let value = if summary_marker.is_some() {
            &summary
        } else if status_marker.is_some() {
            &status
        } else if control_marker.is_some() {
            &control
        } else if let Some(marker) = option_marker {
            match marker.0 {
                BuildingPresetAction::CycleGrid => &grid_label,
                BuildingPresetAction::ToggleConnectionSnap => &connection_label,
                BuildingPresetAction::ToggleTerrainLeveling => &leveling_label,
                _ => continue, // 管理和旋转按钮保留静态文案，只更新三个有界参数值。
            }
        } else {
            continue; // Or 过滤已保证不可达，保留防御分支。
        };
        if text.0 != *value {
            text.0.clone_from(value); // 稳定帧不重复标记 Text Changed。
        }
    }
}

/// 只访问九个固定侧栏按钮；参数变化不增加任何目录规模相关的表现工作。
pub(super) fn sync_sidebar_button_styles(
    editor: Res<MapEditorState>,
    controls: Res<BuildingPlacementControls>,
    mut buttons: Query<
        (
            &UiInteraction,
            &BuildingPresetButton,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        With<super::components::BuildingSidebarActionButton>,
    >,
) {
    use crate::ui::theme as colors;
    for (interaction, button, mut background, mut border) in &mut buttons {
        let selected = match button.0 {
            BuildingPresetAction::PlaceSelected => editor.tool == EditorTool::Building,
            BuildingPresetAction::CycleGrid => controls.grid_step_meters > 0.0,
            BuildingPresetAction::ToggleConnectionSnap => controls.connection_snap,
            BuildingPresetAction::ToggleTerrainLeveling => controls.level_terrain,
            _ => false,
        };
        let fill = match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (UiInteraction::None, true) => colors::SELECT_BG,
            (UiInteraction::None, false) => colors::BUTTON_DEV_SMALL_NORMAL,
        };
        let outline = BorderColor::all(if selected {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
        if background.0 != fill {
            background.0 = fill;
        }
        if *border != outline {
            *border = outline;
        }
    }
}
