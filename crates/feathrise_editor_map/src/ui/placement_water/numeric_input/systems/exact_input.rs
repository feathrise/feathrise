//! 网格水中键字符提交、焦点冻结与右拖灵敏度编排。

use bevy::prelude::*; // ECS查询、Commands、消息与资源变更检测。
use feathrise_editor_core::EditorSettings; // 右拖松开只写字段独立灵敏度槽。
use feathrise_editor_ui::{
    ActiveNumericInput, NumericInputCommitOutcome, NumericInputCommitSource, NumericInputCommitted,
    NumericInputScrubSensitivityChanged, NumericInputScrubSensitivityCommitted,
    NumericInputSetValue, NumericValue,
}; // 共享 UI 控件提供字符提交、右拖灵敏度和外部取消协议。

use super::{AdoptedPlacementRoots, apply_exact_numeric_value, input_kind, reject_numeric_edit}; // 复用父系统模块的稳定作者写入与公共失败出口。
use crate::authoring::{EditorAuthoringSession, WaterFileHandoff}; // 精确输入冻结并复核当前作者修订和交接门。
use crate::resources::MapEditorState; // F12生命周期、dirty门和用户可见状态。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 当前已摆放Scene0网格稳定选择。
use crate::ui::placement_water::numeric_input::transaction::{
    input_context_valid, selected_editable_target,
}; // 字符焦点与提交共享同一作者上下文和稳定目标检查。
use crate::ui::placement_water::numeric_input::{
    PlacementWaterNumericFocus, PlacementWaterNumericInput, PlacementWaterNumericInputState,
};
use crate::ui::placement_water::{
    PlacementInstanceIndex, numeric_input::sensitivity::handle_sensitivity_messages,
}; // 精确提交定向重施根，右拖写独立设置槽。 // 本系统只拥有字符焦点，不处理左拖事务。

/// 消费中键字符提交与右拖灵敏度消息；左拖Changed由独立事务系统处理。
pub(crate) fn handle_placement_water_numeric_input(
    mut commands: Commands, // 精确输入完成后发布未保存实时覆盖。
    mut committed: MessageReader<NumericInputCommitted>, // 只消费两个字段的Text提交。
    mut sensitivity_changed: MessageReader<NumericInputScrubSensitivityChanged>, // 右拖途中只回显倍率。
    mut sensitivity_committed: MessageReader<NumericInputScrubSensitivityCommitted>, // 右拖松开才写设置。
    active: Res<ActiveNumericInput>, // 中键字符焦点用于冻结稳定作者身份。
    inputs: Query<(Entity, &PlacementWaterNumericInput)>, // 两个业务根实体到字段映射。
    browser: Res<EditorGltfBrowserState>, // 当前Scene0选择。
    root_index: Res<PlacementInstanceIndex>, // 精确提交按稳定ID定位当前活动根。
    placement_roots: AdoptedPlacementRoots, // 精确输入即时重施已采用目标根。
    mut state: ResMut<PlacementWaterNumericInputState>, // 字符焦点与左拖事务公共状态。
    mut editor: ResMut<MapEditorState>, // 生命周期、dirty和状态栏。
    mut settings: ResMut<EditorSettings>, // 右拖提交更新独立倍率并触发统一自动保存。
    mut session: ResMut<EditorAuthoringSession>, // 精确输入修改草稿和历史。
    handoff: Res<WaterFileHandoff>,  // 正式placement交接未完成时两个数值根都失效。
    mut set_values: MessageWriter<NumericInputSetValue>, // 目标失效时取消旧字符缓冲。
) {
    let placement_handoff_in_flight = session
        .draft
        .as_ref()
        .is_some_and(|draft| handoff.placement_handoff_in_flight_for(&draft.map_id));
    for event in committed.read() {
        let Some(kind) = input_kind(&inputs, event.entity) else {
            continue; // 水位和笔刷等其他NumericInput由各自reader处理。
        };
        if event.source == NumericInputCommitSource::Scrub {
            continue; // 左拖松开只由一次事务系统收口，不能再形成精确命令。
        }
        let bound_target = state
            .focus
            .as_ref()
            .filter(|focus| focus.entity == event.entity)
            .map(|focus| focus.target.clone()); // 先复制本次焦点冻结身份。
        if state
            .focus
            .as_ref()
            .is_some_and(|focus| focus.entity == event.entity)
        {
            state.focus = None; // 任一字符提交结果都会结束当前业务绑定。
        }
        match event.outcome {
            NumericInputCommitOutcome::Canceled => {
                editor.status = format!("已恢复{}输入前的原值", kind.label()); // Esc仍只属于字符缓冲恢复，不参与拖动。
                continue;
            }
            NumericInputCommitOutcome::Rejected => {
                reject_numeric_edit(
                    &mut editor,
                    format!("{}不是有效数字，已恢复原值", kind.label()),
                );
                continue;
            }
            NumericInputCommitOutcome::Accepted if !event.edited => {
                continue; // 未编辑失焦不得量化或重写高精度作者值。
            }
            NumericInputCommitOutcome::Accepted => {} // 有效字符提交继续进入作者域复核。
        }
        if !input_context_valid(&editor, &session, placement_handoff_in_flight) {
            reject_numeric_edit(&mut editor, "当前界面或保存状态不能提交网格水数值");
            continue;
        }
        let target = bound_target.or_else(|| selected_editable_target(&browser, &session)); // 防御同帧首次焦点消息尚未被业务状态观察。
        let Some(target) = target else {
            reject_numeric_edit(&mut editor, "当前选择没有可编辑的有效网格水绑定");
            continue;
        };
        let Some(value) = event.value.as_f32() else {
            reject_numeric_edit(&mut editor, "网格水数值控件返回了非浮点值");
            continue;
        };
        let value = match kind.validate_value(value) {
            Ok(value) => value, // 公共范围复核成功才修改完整记录。
            Err(error) => {
                reject_numeric_edit(&mut editor, error);
                continue;
            }
        };
        match apply_exact_numeric_value(
            &mut commands,
            &placement_roots,
            &root_index,
            &mut editor,
            &mut session,
            &target,
            kind,
            value,
        ) {
            Ok(Some(_)) => {
                editor.status = format!(
                    "{}已设为{value:.precision$}；正在验证Scene0与动态水材质",
                    kind.label(),
                    precision = usize::from(kind.decimals()),
                ); // 这里只声明已提交，成功仍等待公开Ready回执。
            }
            Ok(None) => {
                editor.status = format!(
                    "{}保持{value:.precision$}",
                    kind.label(),
                    precision = usize::from(kind.decimals()),
                ); // 同值显式输入不制造历史或验证换代。
            }
            Err(error) => reject_numeric_edit(&mut editor, error), // 目标或修订失效时明确拒绝。
        }
    }

    handle_sensitivity_messages(
        &mut sensitivity_changed,
        &mut sensitivity_committed,
        &inputs,
        &mut editor,
        &mut settings,
        placement_handoff_in_flight,
    );

    let active_kind = active
        .entity
        .and_then(|entity| input_kind(&inputs, entity).map(|kind| (entity, kind))); // 只关心两个网格水根的字符焦点。
    let Some((active_entity, active_kind)) = active_kind else {
        state.focus = None; // 字符焦点已结束时不保留旧网格身份。
        return;
    };
    let current_target = selected_editable_target(&browser, &session); // 聚焦期间每帧复核选择和作者修订。
    if !input_context_valid(&editor, &session, placement_handoff_in_flight)
        || current_target.is_none()
    {
        set_values.write(NumericInputSetValue {
            entity: active_entity,         // 只取消当前网格水字符根。
            value: NumericValue::F32(0.0), // 下一次Presentation会同步真实或诊断值。
            cancel_edit: true,             // 丢弃过期字符缓冲并释放键盘焦点。
        });
        state.focus = None; // 不允许隐藏或失效目标继续持有作者身份。
        return;
    }
    let current_target = current_target.expect("上方已经确认当前目标存在"); // 取得当前完整稳定身份。
    let focus_is_current = state.focus.as_ref().is_some_and(|focus| {
        focus.entity == active_entity && focus.target == current_target // 作者修订或任何稳定身份变化都令旧焦点失效。
    });
    if !focus_is_current {
        state.focus = Some(PlacementWaterNumericFocus {
            entity: active_entity,  // 冻结本次中键精确输入根。
            target: current_target, // 冻结地图、网格和作者修订。
        });
        editor.status = format!(
            "正在精确输入{}；Enter或点击空白提交，范围由字段自动校验",
            active_kind.label(),
        ); // 拖动不需要Esc，中键继续保留给外部系统。
    }
}
