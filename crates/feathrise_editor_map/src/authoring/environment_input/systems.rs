//! 环境专业数值框的精确提交、左拖事务与右拖设置桥。

use bevy::prelude::*; // MessageReader、Query和资源可变访问。
use feathrise_editor_core::EditorSettings; // 四个逻辑字段独立保存右拖倍率。
use feathrise_editor_ui::{
    ActiveNumericInput, NumericInputChanged, NumericInputCommitOutcome, NumericInputCommitSource,
    NumericInputCommitted, NumericInputScrubSensitivityChanged,
    NumericInputScrubSensitivityCommitted, NumericInputSetValue,
}; // 公共控件发布规范化实际值、事务收口和倍率消息。

use super::transaction::{authored_field_value, current_value, reset_input}; // 权威读值、回显和raw事务辅助由同一模块集中实现。
use super::{
    EnvironmentNumericFocus, EnvironmentNumericInput, EnvironmentNumericInputState,
    EnvironmentNumericKind, EnvironmentNumericScrub, EnvironmentNumericScrubStart,
    EnvironmentNumericTarget, authored_target, rollback_environment_scrub_if_owned,
}; // 本领域只处理可持久化的地图天空环境事务。
use crate::authoring::environment::commit_environment_change; // 持久字段共享唯一完整环境撤销入口。
use crate::authoring::resources::EditorAuthoringSession; // 当前草稿、修订和统一历史。
use crate::resources::MapEditorState; // 编辑上下文与底栏诊断。

/// 按消息根反查四个环境字段之一。
fn input_kind(
    inputs: &Query<(Entity, &EnvironmentNumericInput)>,
    entity: Entity,
) -> Option<EnvironmentNumericKind> {
    inputs.get(entity).ok().map(|(_, marker)| marker.0) // 根实体是唯一消息路由身份。
}

/// 返回天空字段当前稳定地图目标。
fn target_for_kind(
    _kind: EnvironmentNumericKind,
    session: &EditorAuthoringSession,
) -> Option<EnvironmentNumericTarget> {
    authored_target(session) // 只有动态地图草稿提供持久环境目标。
}

/// 中键焦点首次出现时冻结根、字段和可选地图目标；上下文变化立即取消。
fn maintain_focus(
    active: &ActiveNumericInput,
    inputs: &Query<(Entity, &EnvironmentNumericInput)>,
    editor: &mut MapEditorState,
    session: &EditorAuthoringSession,
    state: &mut EnvironmentNumericInputState,
    set_values: &mut MessageWriter<NumericInputSetValue>,
) {
    let Some(entity) = active.entity else {
        return; // 提交消息仍需要上一帧焦点，不能提前清空。
    };
    let Some(kind) = input_kind(inputs, entity) else {
        return; // 其他公共数字框不影响环境焦点。
    };
    let target = target_for_kind(kind, session); // 当前可用性与稳定身份一次派生。
    let context_invalid =
        !editor.open || editor.close_requested || editor.new_map_dialog_open || session.saving;
    let stale = state.focus.as_ref().is_some_and(|focus| {
        focus.entity != entity || focus.kind != kind || target.as_ref() != Some(&focus.target)
    }); // 根、字段、地图或修订变化都使旧焦点失效。
    if context_invalid || stale || target.is_none() {
        reset_input(entity, kind, session, set_values); // 公共模型恢复当前权威显示。
        state.focus = None; // 释放旧地图身份。
        editor.status = format!("{}输入因地图或作者状态变化而取消", kind.label()); // 不串写新草稿。
        return;
    }
    if state.focus.is_none() {
        state.focus = target.map(|target| EnvironmentNumericFocus {
            entity, // 冻结公共根。
            kind,   // 冻结字段身份。
            target, // 冻结稳定地图与作者修订。
        });
        editor.status = format!("正在精确输入{}；Enter或点击空白提交", kind.label()); // 中键字符行为由公共控件拥有。
    }
}

/// 应用一次中键精确提交并生成一条地图环境撤销。
fn apply_exact(
    entity: Entity,
    kind: EnvironmentNumericKind,
    value: f32,
    target: EnvironmentNumericTarget,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    set_values: &mut MessageWriter<NumericInputSetValue>,
) {
    if session.revision != target.authoring_revision
        || !session
            .draft
            .as_ref()
            .is_some_and(|draft| draft.map_id == target.map_id)
    {
        editor.status = format!("{}提交因地图或作者修订变化而取消", kind.label()); // 旧缓冲不写新分支。
        reset_input(entity, kind, session, set_values); // 显示回到当前权威值。
        return;
    }
    let Some(before) = session
        .draft
        .as_ref()
        .map(|draft| draft.manifest.environment.clone())
    else {
        editor.status = "地图环境草稿尚未就绪".to_owned(); // 迟到提交无副作用拒绝。
        reset_input(entity, kind, session, set_values); // 控件回到安全占位。
        return;
    };
    let mut after = before.clone(); // 完整环境快照用于唯一撤销。
    match kind.set_environment_value(&mut after, value) {
        Ok(false) => {
            editor.status = format!("{}没有变化", kind.label()); // 重输同值不进历史。
            return;
        }
        Ok(true) => {} // 真实变化继续完整环境校验。
        Err(error) => {
            editor.status = format!("{}未采用：{error}", kind.label()); // 非有限或错字段失败关闭。
            reset_input(entity, kind, session, set_values); // 显示恢复旧权威值。
            return;
        }
    }
    if let Err(error) = after.validate() {
        editor.status = format!("{}未采用：{error}", kind.label()); // 完整校验只在提交边沿执行一次。
        reset_input(entity, kind, session, set_values); // 非法环境不进入草稿。
        return;
    }
    let actual = kind.displayed_value(Some(&after)).unwrap_or(value); // Yaw使用归一后的真实角度回显。
    commit_environment_change(
        session,
        editor,
        before,
        after,
        format!("{}已设为 {:.1}{}", kind.label(), actual, kind.unit()),
    ); // 精确提交只推进一次环境修订和撤销。
}

/// 左拖 Changed 实时应用小型地图环境，但不创建历史或昂贵换代。
fn apply_scrub_changed(
    entity: Entity,
    kind: EnvironmentNumericKind,
    value: f32,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    state: &mut EnvironmentNumericInputState,
) {
    if state.scrub.is_none() {
        let Some(initial_value) = current_value(kind, session) else {
            editor.status = format!("当前没有可调整的{}", kind.label()); // 无显示值不建立半套事务。
            return;
        };
        let Some(target) = authored_target(session) else {
            editor.status = format!("当前地图没有可编辑的{}", kind.label());
            return;
        };
        let Some(before) = session
            .draft
            .as_ref()
            .map(|draft| draft.manifest.environment.clone())
        else {
            editor.status = "地图环境草稿尚未就绪".to_owned();
            return;
        };
        let before_value =
            authored_field_value(&before, kind).expect("持久天空字段必须可读取原始值");
        let start = EnvironmentNumericScrubStart::Authored {
            target,
            before,
            before_value,
        };
        let latest_value = before_value;
        state.scrub = Some(EnvironmentNumericScrub {
            entity,         // 冻结公共根。
            kind,           // 冻结字段身份。
            start,          // 冻结完整地图环境起点。
            initial_value,  // 保存净变化起点。
            latest_value,   // 保存本事务最近一次拥有的字段值。
            aborted: false, // 新事务允许接受Changed。
        });
    }
    let Some(transaction) = state.scrub.as_mut() else {
        return; // 建立失败已经发布诊断。
    };
    if transaction.aborted {
        return; // 冲突事务只等待松手，不在同一指针段重绑。
    }
    if transaction.entity != entity || transaction.kind != kind {
        rollback_environment_scrub_if_owned(session, transaction); // 只恢复仍等于latest的原字段。
        transaction.aborted = true; // 后续Changed忽略到松手。
        editor.status = "环境数值拖动收到不同字段消息，已拒绝串写".to_owned(); // 全局指针异常失败关闭。
        return;
    }
    match &transaction.start {
        EnvironmentNumericScrubStart::Authored { target, .. } => {
            if session.revision != target.authoring_revision {
                rollback_environment_scrub_if_owned(session, transaction); // 外部未改同字段时撤销无历史实时值。
                transaction.aborted = true; // 保留取消态到松手。
                editor.status = format!("{}拖动因并发作者修改而停止", kind.label()); // 未入栈实时值不覆盖外部分支。
                return;
            }
            let Some(environment) = session
                .draft
                .as_mut()
                .filter(|draft| draft.map_id == target.map_id)
                .map(|draft| &mut draft.manifest.environment)
            else {
                rollback_environment_scrub_if_owned(session, transaction); // 换图后只尝试原稳定地图。
                transaction.aborted = true; // 不在新地图重建迟到事务。
                editor.status = format!("{}拖动因地图切换而停止", kind.label()); // 旧环境不进入新草稿。
                return;
            };
            match kind.set_environment_value(environment, value) {
                Ok(false) => return, // 同一吸附档不重复写资源。
                Ok(true) => {}       // 离屏和匹配玩法相机同帧读取新环境值。
                Err(error) => {
                    rollback_environment_scrub_if_owned(session, transaction); // 异常中断不遗留无历史天空值。
                    transaction.aborted = true; // 失败后不继续沿用事务。
                    editor.status = format!("{}拖动未采用：{error}", kind.label()); // 异常消息失败关闭。
                    return;
                }
            }
            transaction.latest_value = authored_field_value(environment, kind).unwrap_or(value); // 保存实际raw值作为所有权比较门。
            let displayed = kind.displayed_value(Some(environment)).unwrap_or(value); // Yaw状态栏仍显示归一角。
            editor.status = format!(
                "正在调整{}：{:.1}{}；松手后生成一次环境撤销",
                kind.label(),
                displayed,
                kind.unit(),
            ); // 天空视觉实时，作者修订延迟到松手。
        }
    }
}

/// 左拖松手收口，整次地图环境操作只推进一次修订。
fn finish_scrub(
    entity: Entity,
    kind: EnvironmentNumericKind,
    committed_changed: bool,
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    state: &mut EnvironmentNumericInputState,
    set_values: &mut MessageWriter<NumericInputSetValue>,
) {
    let Some(transaction) = state.scrub.take() else {
        return; // 未超过阈值时没有业务事务。
    };
    if !committed_changed {
        rollback_environment_scrub_if_owned(session, &transaction); // 显示值回起点时恢复raw before。
        editor.status = format!("{}拖动已回到显示起点，没有产生作者操作", kind.label()); // 量化不留历史。
        reset_input(entity, kind, session, set_values); // 公共值回显精确权威值。
        return;
    }
    if transaction.entity != entity || transaction.kind != kind || transaction.aborted {
        rollback_environment_scrub_if_owned(session, &transaction); // 错误收口仅撤销仍属本事务的字段。
        editor.status = "环境数值拖动收口身份不一致，未生成作者操作".to_owned(); // 不移动错误字段起点。
        reset_input(entity, kind, session, set_values); // 显示回到当前权威。
        return;
    }
    match &transaction.start {
        EnvironmentNumericScrubStart::Authored { target, before, .. } => {
            let continuous = session.revision == target.authoring_revision
                && session
                    .draft
                    .as_ref()
                    .is_some_and(|draft| draft.map_id == target.map_id); // 只有连续同一作者分支可移动完整起点。
            if !continuous {
                rollback_environment_scrub_if_owned(session, &transaction); // 同地图且当前字段仍为latest才回滚。
                editor.status = format!(
                    "{}拖动因地图或作者修订变化而取消；已保留外部结果",
                    kind.label()
                ); // 冲突时不用旧环境快照覆盖新分支。
                reset_input(entity, kind, session, set_values); // 显示对齐外部权威。
                return;
            }
            let Some(after) = session
                .draft
                .as_ref()
                .map(|draft| draft.manifest.environment.clone())
            else {
                rollback_environment_scrub_if_owned(session, &transaction); // 草稿异常也经过单字段所有权门。
                editor.status = "环境拖动结束时草稿已经失效".to_owned(); // 迟到松手不创建半套命令。
                reset_input(entity, kind, session, set_values); // 控件安全收口。
                return;
            };
            let displayed_after = kind.displayed_value(Some(&after)); // Yaw用归一显示角判定回到起点。
            if *before == after || displayed_after == Some(transaction.initial_value) {
                rollback_environment_scrub_if_owned(session, &transaction); // 恢复未归一化raw before。
                editor.status = format!("{}拖动已回到起点，没有产生撤销", kind.label()); // 环境修订保持不变。
                return;
            }
            if let Err(error) = after.validate() {
                rollback_environment_scrub_if_owned(session, &transaction); // 最终校验失败也只撤销本天空字段。
                editor.status = format!("{}最终值无效，已恢复：{error}", kind.label()); // 不推进环境修订。
                reset_input(entity, kind, session, set_values); // 显示恢复后的权威值。
                return;
            }
            let value = displayed_after.unwrap_or(transaction.latest_value); // 状态栏显示归一值，撤销仍保留raw快照。
            commit_environment_change(
                session,
                editor,
                before.clone(),
                after,
                format!("{}已调整为 {:.1}{}", kind.label(), value, kind.unit()),
            ); // 整次拖动唯一一次environment_revision和撤销登记。
        }
    }
}

/// 消费四个环境字段的公共消息。
#[allow(clippy::too_many_arguments)] // 单系统需在同一Input阶段顺序收口焦点、值事务和设置倍率。
pub(crate) fn handle_environment_numeric_inputs(
    mut changed: MessageReader<NumericInputChanged>, // 左拖规范化实际值。
    mut committed: MessageReader<NumericInputCommitted>, // 中键精确或Scrub松手提交。
    mut sensitivity_changed: MessageReader<NumericInputScrubSensitivityChanged>, // 右拖即时倍率仅回显。
    mut sensitivity_committed: MessageReader<NumericInputScrubSensitivityCommitted>, // 松手才持久化倍率。
    active: Res<ActiveNumericInput>, // 中键焦点用于冻结稳定环境目标。
    inputs: Query<(Entity, &EnvironmentNumericInput)>, // 五个根实体到字段映射。
    mut editor: ResMut<MapEditorState>, // 编辑上下文和状态栏。
    mut session: ResMut<EditorAuthoringSession>, // 环境草稿、修订与撤销。
    mut settings: ResMut<EditorSettings>, // 四个独立倍率槽。
    mut state: ResMut<EnvironmentNumericInputState>, // 唯一焦点与左拖事务。
    mut set_values: MessageWriter<NumericInputSetValue>, // 异常路径恢复公共模型。
) {
    maintain_focus(
        &active,
        &inputs,
        &mut editor,
        &session,
        &mut state,
        &mut set_values,
    ); // 先冻结本帧中键目标，失焦提交才能使用旧地图身份。

    for event in changed.read() {
        let Some(kind) = input_kind(&inputs, event.entity) else {
            continue; // 其他公共数字框由各自领域处理。
        };
        let Some(value) = event.value.as_f32() else {
            editor.status = format!("{}控件返回了非浮点值", kind.label()); // 类型错配失败关闭。
            continue;
        };
        apply_scrub_changed(
            event.entity,
            kind,
            value,
            &mut editor,
            &mut session,
            &mut state,
        ); // Changed 只更新小型地图环境快照。
    }

    for event in committed.read() {
        let Some(kind) = input_kind(&inputs, event.entity) else {
            continue; // 非环境提交由其他reader处理。
        };
        if event.source == NumericInputCommitSource::Scrub {
            finish_scrub(
                event.entity,
                kind,
                event.changed,
                &mut editor,
                &mut session,
                &mut state,
                &mut set_values,
            ); // 松手按事务类型收口一次。
            continue;
        }
        let focus = state
            .focus
            .take()
            .filter(|focus| focus.entity == event.entity && focus.kind == kind); // 精确提交必须匹配冻结根和字段。
        match event.outcome {
            NumericInputCommitOutcome::Canceled => {
                editor.status = format!("已取消{}精确输入；原值保持不变", kind.label()); // 公共控件已恢复显示。
                continue;
            }
            NumericInputCommitOutcome::Rejected => {
                editor.status = format!("{}不是有效数字，已恢复原值", kind.label()); // 业务资源未修改。
                continue;
            }
            NumericInputCommitOutcome::Accepted if !event.edited => continue, // 未编辑失焦不量化高精度作者值。
            NumericInputCommitOutcome::Accepted => {} // 有效字符继续复核稳定目标。
        }
        let Some(value) = event.value.as_f32() else {
            editor.status = format!("{}控件返回了非浮点提交", kind.label()); // 类型错配失败关闭。
            reset_input(event.entity, kind, &session, &mut set_values); // 恢复权威显示。
            continue;
        };
        let target = focus
            .map(|focus| focus.target)
            .or_else(|| target_for_kind(kind, &session)); // 极端同帧提交仍按当前稳定目标处理。
        let Some(target) = target else {
            editor.status = format!("{}提交时没有可用目标", kind.label()); // 不缓存值等待未来地图。
            reset_input(event.entity, kind, &session, &mut set_values); // 显示恢复权威值。
            continue;
        };
        apply_exact(
            event.entity,
            kind,
            value,
            target,
            &mut editor,
            &mut session,
            &mut set_values,
        ); // 精确输入一次完成地图环境事务。
    }

    for event in sensitivity_changed.read() {
        let Some(kind) = input_kind(&inputs, event.entity) else {
            continue; // 其他字段倍率消息不属于环境。
        };
        editor.status = format!("正在调整{}拖动灵敏度：×{:.3}", kind.label(), event.value); // 数值框保持显示实际值。
    }
    for event in sensitivity_committed.read() {
        let Some(kind) = input_kind(&inputs, event.entity) else {
            continue; // 其他倍率提交由其业务域保存。
        };
        let value = event
            .after
            .clamp(super::SCRUB_SENSITIVITY_MIN, super::SCRUB_SENSITIVITY_MAX); // 持久化前再次收口公共范围。
        let field = kind.sensitivity_field(); // 当前根决定唯一设置键。
        if settings.map_editor_scrub_sensitivities.get(field) != value {
            settings.map_editor_scrub_sensitivities.set(field, value); // 松开右键只触发一次EditorSettings变化与自动保存。
        }
        editor.status = format!("{}拖动灵敏度已设为 ×{value:.3}", kind.label()); // 确认字段独立倍率。
    }
}
