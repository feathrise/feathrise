//! 水网控制点专业数值框的精确提交、左拖事务和右拖设置桥。

use bevy::prelude::*; // MessageReader、Query和资源可变访问。
use feathrise_editor_core::EditorSettings; // 每个逻辑字段独立保存右拖倍率。
use feathrise_editor_ui::{
    ActiveNumericInput, NumericInputChanged, NumericInputCommitOutcome, NumericInputCommitSource,
    NumericInputCommitted, NumericInputScrubSensitivityChanged,
    NumericInputScrubSensitivityCommitted, NumericInputSetValue, NumericValue,
}; // 公共控件发布规范化实际值、事务收口和倍率消息。

use super::{
    WaterNetworkNumericFocus, WaterNetworkNumericInput, WaterNetworkNumericInputState,
    WaterNetworkNumericKind, WaterNetworkNumericScrub, rollback_water_network_scrub_if_owned,
    selected_target, set_target_value, target_value,
}; // 本领域冻结稳定目标并拥有一次左拖完整起点。
use crate::authoring::resources::EditorAuthoringSession; // 当前草稿、修订和统一历史。
use crate::authoring::water_network::connections::sync_water_network_connections; // 任意高度变化后统一维护端点连续性。
use crate::authoring::water_network::helpers::{commit_water_change, validate_intermediate_water}; // 松手或精确提交才校验并推进水预览修订。
use crate::resources::SceneEditorState; // 可编辑上下文和底栏诊断。

/// 按消息根反查水网字段；其他NumericInput由各自业务reader处理。
fn input_kind(
    inputs: &Query<(Entity, &WaterNetworkNumericInput)>,
    entity: Entity,
) -> Option<WaterNetworkNumericKind> {
    inputs.get(entity).ok().map(|(_, marker)| marker.0) // 根实体是唯一消息路由身份。
}

/// 把当前权威值回写公共模型并取消可能残留的字符缓冲。
fn reset_input(
    entity: Entity,
    kind: WaterNetworkNumericKind,
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    set_values: &mut MessageWriter<NumericInputSetValue>,
) {
    let value = selected_target(editor, session)
        .and_then(|target| {
            session
                .draft
                .as_ref()
                .and_then(|draft| draft.water.as_ref())
                .and_then(|water| target_value(water, &target, kind))
        })
        .unwrap_or(0.0); // 目标失效时使用禁用字段的安全占位值。
    set_values.write(NumericInputSetValue {
        entity,                          // 只重置产生异常消息的根。
        value: NumericValue::F32(value), // 回写当前草稿权威值。
        cancel_edit: true,               // 同时清除旧字符缓冲与焦点。
    });
}

/// 中键精确输入获得焦点时冻结场景、特征和点稳定ID；上下文变化立即取消缓冲。
fn maintain_focus(
    active: &ActiveNumericInput,
    inputs: &Query<(Entity, &WaterNetworkNumericInput)>,
    editor: &mut SceneEditorState,
    session: &EditorAuthoringSession,
    state: &mut WaterNetworkNumericInputState,
    set_values: &mut MessageWriter<NumericInputSetValue>,
) {
    let Some(entity) = active.entity else {
        return; // 提交消息仍可能需要上一帧焦点，不能在这里提前清空。
    };
    let Some(kind) = input_kind(inputs, entity) else {
        return; // 其他公共数字框不影响水网焦点资源。
    };
    let current = selected_target(editor, session); // 当前可见选择只用于建立或复核冻结身份。
    let context_invalid =
        !editor.open || editor.close_requested || editor.new_scene_dialog_open || session.saving; // 隐藏、模态和保存期间不得保留作者字符缓冲。
    let stale = state
        .focus
        .as_ref()
        .is_some_and(|focus| focus.entity != entity || current.as_ref() != Some(&focus.target)); // 场景、修订、路径或点任一变化都使旧焦点失效。
    if context_invalid || stale || current.is_none() {
        reset_input(entity, kind, editor, session, set_values); // 公共模型恢复本帧权威值。
        state.focus = None; // 旧稳定身份立即释放。
        editor.status = "水网数值输入因选择、场景或作者状态变化而取消".to_owned(); // 明确说明没有串写新选择。
        return;
    }
    if state.focus.is_none() {
        state.focus = current.map(|target| WaterNetworkNumericFocus { entity, target }); // 首帧焦点冻结完整稳定身份。
        editor.status = format!("正在精确输入{}；Enter或点击空白提交", kind.label(),); // 中键已经由公共控件进入字符模式。
    }
}

/// 立即应用一次精确字符提交，并生成唯一完整水网撤销记录。
fn apply_exact_value(
    entity: Entity,
    kind: WaterNetworkNumericKind,
    value: f32,
    target: super::WaterNetworkNumericTarget,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    set_values: &mut MessageWriter<NumericInputSetValue>,
) {
    let target_is_current = session.revision == target.authoring_revision
        && session
            .draft
            .as_ref()
            .is_some_and(|draft| draft.scene_id == target.scene_id); // 精确输入期间任何外部作者变化都失败关闭。
    if !target_is_current {
        editor.status = "水网数值提交因场景或作者修订变化而取消".to_owned(); // 不把旧缓冲写到新分支。
        reset_input(entity, kind, editor, session, set_values); // 显示恢复当前权威值。
        return;
    }
    let Some(before) = session.draft.as_ref().and_then(|draft| draft.water.clone()) else {
        editor.status = "当前场景没有可编辑水域草稿".to_owned(); // 损坏或换代草稿无副作用拒绝。
        reset_input(entity, kind, editor, session, set_values); // 禁用同步前先收口公共模型。
        return;
    };
    let mut after = before.clone(); // 完整小型水网快照用于可靠撤销。
    match set_target_value(&mut after, &target, kind, value) {
        Ok(false) => {
            editor.status = format!("{}没有变化", kind.label()); // 重输同值不制造伪撤销。
            return;
        }
        Ok(true) => {} // 真实变化继续同步连接并校验。
        Err(error) => {
            editor.status = format!("水网{}未采用：{error}", kind.label()); // 范围、连接或目标失效原因直达底栏。
            reset_input(entity, kind, editor, session, set_values); // 恢复作者权威显示。
            return;
        }
    }
    sync_water_network_connections(&mut after); // 高度提交同事务维护所有稳定连接端点。
    if let Err(error) = validate_intermediate_water(&after, &after.scene_id) {
        editor.status = format!("水网{}未采用：{error}", kind.label()); // 完整宽松校验只在提交边沿执行一次。
        reset_input(entity, kind, editor, session, set_values); // 非法结果不留在公共显示。
        return;
    }
    let actual = target_value(&after, &target, kind).unwrap_or(value); // 连接同步后的真实字段值用于状态回显。
    commit_water_change(
        session,
        editor,
        before,
        after,
        format!("水网{}已设为 {:.2}{}", kind.label(), actual, kind.unit()),
    ); // 精确输入只推进一次撤销、dirty与水预览修订。
}

/// 左拖Changed只改当前内存草稿和Gizmo，不逐帧启动完整水面构建。
fn apply_scrub_changed(
    entity: Entity,
    kind: WaterNetworkNumericKind,
    value: f32,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    state: &mut WaterNetworkNumericInputState,
) {
    if state.scrub.is_none() {
        let Some(target) = selected_target(editor, session) else {
            editor.status = "请先选择一个可编辑水网控制点".to_owned(); // 无目标Changed不得建立半套事务。
            return;
        };
        let Some(before) = session.draft.as_ref().and_then(|draft| draft.water.clone()) else {
            editor.status = "当前场景没有可编辑水域草稿".to_owned(); // 无完整起点不得实时写入。
            return;
        };
        let Some(initial_value) = target_value(&before, &target, kind) else {
            editor.status = "当前水网控制点稳定身份已经失效".to_owned(); // 选择数组已变化时失败关闭。
            return;
        };
        state.scrub = Some(WaterNetworkNumericScrub {
            entity,                      // 冻结公共根。
            kind,                        // 冻结字段。
            target,                      // 冻结场景、特征和点ID。
            before,                      // 冻结一次完整撤销前值。
            before_value: initial_value, // 记录稳定点单字段精确起点。
            latest_value: initial_value, // 尚未应用首个Changed前两者相同。
            aborted: false,              // 新事务允许正常接受Changed。
        });
    }
    let Some(transaction) = state.scrub.as_mut() else {
        return; // 上方失败已经发布明确诊断。
    };
    if transaction.aborted {
        return; // 冲突事务只等待松手清理，不在同一指针段重绑。
    }
    if transaction.entity != entity || transaction.kind != kind {
        rollback_water_network_scrub_if_owned(session, transaction); // 字段仍是latest才定点恢复before。
        transaction.aborted = true; // 防止后续Changed建立新点事务。
        editor.status = "水网数值拖动收到不同字段消息，已拒绝串写".to_owned(); // 全局指针捕获理论上不会发生，仍失败关闭。
        return;
    }
    if session.revision != transaction.target.authoring_revision {
        rollback_water_network_scrub_if_owned(session, transaction); // 外部未改本字段时撤销无历史实时值。
        transaction.aborted = true; // 保留取消态到松手。
        editor.status = "水网数值拖动因并发作者修改而停止".to_owned(); // 未推进修订的实时值不拥有外部分支。
        return;
    }
    let Some(draft) = session
        .draft
        .as_mut()
        .filter(|draft| draft.scene_id == transaction.target.scene_id)
    else {
        rollback_water_network_scrub_if_owned(session, transaction); // 切换场景只会定点尝试原场景原点。
        transaction.aborted = true; // 不在新场景上重绑迟到拖动。
        editor.status = "水网数值拖动因场景切换而停止".to_owned(); // 旧场景稳定ID绝不进入新草稿。
        return;
    };
    let Some(water) = draft.water.as_mut() else {
        transaction.aborted = true; // 水网结构已消失，后续Changed不能重建事务。
        editor.status = "水网数值拖动目标水域已经移除".to_owned(); // 迟到消息无副作用拒绝。
        return;
    };
    match set_target_value(water, &transaction.target, kind, value) {
        Ok(false) => return, // 同一吸附档不更新状态或连接。
        Ok(true) => {}       // 真实变化继续刷新连接和Gizmo。
        Err(error) => {
            rollback_water_network_scrub_if_owned(session, transaction); // 异常中断不遗留未入历史字段。
            transaction.aborted = true; // 失败后等待松手清理。
            editor.status = format!("水网{}拖动未采用：{error}", kind.label()); // 业务门异常直达底栏。
            return;
        }
    }
    sync_water_network_connections(water); // 仅固定预算内小型纯数据同步，不启动后台网格构建。
    transaction.latest_value = target_value(water, &transaction.target, kind).unwrap_or(value); // 保存本事务最近实际拥有值。
    editor.status = format!(
        "正在调整水网{}：{:.2}{}；松手后生成一次撤销并重建水面",
        kind.label(),
        transaction.latest_value,
        kind.unit(),
    ); // 明确Gizmo实时而完整水面在松手更新。
}

/// Scrub松手完整校验一次最终草稿，并只推进一条撤销和一次水预览修订。
fn finish_scrub(
    entity: Entity,
    kind: WaterNetworkNumericKind,
    committed_changed: bool,
    editor: &mut SceneEditorState,
    session: &mut EditorAuthoringSession,
    state: &mut WaterNetworkNumericInputState,
    set_values: &mut MessageWriter<NumericInputSetValue>,
) {
    let Some(transaction) = state.scrub.take() else {
        return; // 未超过拖动阈值时不会建立业务事务。
    };
    if !committed_changed {
        rollback_water_network_scrub_if_owned(session, &transaction); // 拖回显示起点时恢复未量化raw before。
        editor.status = format!("水网{}拖动已回到显示起点，没有产生撤销", kind.label()); // 不推进水面重建。
        reset_input(entity, kind, editor, session, set_values); // 公共值回显精确权威原值。
        return;
    }
    if transaction.entity != entity || transaction.kind != kind || transaction.aborted {
        rollback_water_network_scrub_if_owned(session, &transaction); // 错误收口仅回滚当前仍属于本事务的字段。
        editor.status = "水网数值拖动收口身份不一致，未生成撤销".to_owned(); // 不移动错误字段的完整起点。
        reset_input(entity, kind, editor, session, set_values); // 回显当前权威分支。
        return;
    }
    let continuous = session.revision == transaction.target.authoring_revision
        && session
            .draft
            .as_ref()
            .is_some_and(|draft| draft.scene_id == transaction.target.scene_id); // 只有无外部分支时本事务拥有当前草稿。
    if !continuous {
        rollback_water_network_scrub_if_owned(session, &transaction); // 同场景同点同字段仍为latest才恢复。
        editor.status = "水网数值拖动因场景或作者修订变化而取消；已保留外部结果".to_owned(); // 不拿旧完整快照覆盖新分支。
        reset_input(entity, kind, editor, session, set_values); // 公共显示对齐外部权威。
        return;
    }
    let Some(after) = session.draft.as_ref().and_then(|draft| draft.water.clone()) else {
        rollback_water_network_scrub_if_owned(session, &transaction); // 草稿异常也经过单字段所有权门。
        editor.status = "水网数值拖动结束时水域草稿已经失效".to_owned(); // 迟到松手不创建半套命令。
        reset_input(entity, kind, editor, session, set_values); // 控件同步到安全占位。
        return;
    };
    if transaction.before == after || transaction.before_value == transaction.latest_value {
        rollback_water_network_scrub_if_owned(session, &transaction); // 拖回起点时精确恢复before字段。
        editor.status = format!("水网{}拖动已回到起点，没有产生撤销", kind.label()); // 草稿已经等于完整起点，无需回写。
        return;
    }
    if let Err(error) = validate_intermediate_water(&after, &after.scene_id) {
        rollback_water_network_scrub_if_owned(session, &transaction); // 最终校验失败也只撤销本数值字段。
        editor.status = format!("水网{}最终值无效，已恢复：{error}", kind.label()); // 完整校验失败不推进预览修订。
        reset_input(entity, kind, editor, session, set_values); // 控件回到恢复后的作者值。
        return;
    }
    let value = transaction.latest_value; // 移动完整起点前冻结可见终值。
    commit_water_change(
        session,
        editor,
        transaction.before,
        after,
        format!("水网{}已调整为 {:.2}{}", kind.label(), value, kind.unit()),
    ); // 这里是整次拖动唯一一次昂贵水预览修订。
}

/// 消费四个水网字段的公共消息；各MessageReader独立推进而不影响其他业务域。
#[allow(clippy::too_many_arguments)] // 单系统需要在同一Input阶段按顺序收口焦点、值事务和设置倍率。
pub(crate) fn handle_water_network_numeric_inputs(
    mut changed: MessageReader<NumericInputChanged>, // 左拖途中规范化实际值。
    mut committed: MessageReader<NumericInputCommitted>, // 中键精确提交或左拖松手。
    mut sensitivity_changed: MessageReader<NumericInputScrubSensitivityChanged>, // 右拖即时倍率仅回显。
    mut sensitivity_committed: MessageReader<NumericInputScrubSensitivityCommitted>, // 松手才写设置。
    active: Res<ActiveNumericInput>, // 中键字符焦点用于冻结稳定目标。
    inputs: Query<(Entity, &WaterNetworkNumericInput)>, // 根实体到四项字段身份映射。
    mut editor: ResMut<SceneEditorState>, // 状态栏与当前选择。
    mut session: ResMut<EditorAuthoringSession>, // 草稿、修订、撤销和重做。
    mut settings: ResMut<EditorSettings>, // 独立右拖倍率注册表。
    mut state: ResMut<WaterNetworkNumericInputState>, // 唯一焦点与左拖事务。
    mut set_values: MessageWriter<NumericInputSetValue>, // 异常或取消时恢复公共模型。
) {
    maintain_focus(
        &active,
        &inputs,
        &mut editor,
        &session,
        &mut state,
        &mut set_values,
    ); // 先冻结本帧中键焦点，随后提交消息才能使用旧选择身份。

    for event in changed.read() {
        let Some(kind) = input_kind(&inputs, event.entity) else {
            continue; // 其他公共数字框由各自业务系统处理。
        };
        let Some(value) = event.value.as_f32() else {
            editor.status = "水网数值控件返回了非浮点值".to_owned(); // 类型错配不得写作者数据。
            continue;
        };
        apply_scrub_changed(
            event.entity,
            kind,
            value,
            &mut editor,
            &mut session,
            &mut state,
        ); // Changed只更新小型草稿和Gizmo。
    }

    for event in committed.read() {
        let Some(kind) = input_kind(&inputs, event.entity) else {
            continue; // 非水网提交由其他reader处理。
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
            ); // 松手只在此形成一次完整命令和水预览修订。
            continue;
        }
        let focus = state
            .focus
            .take()
            .filter(|focus| focus.entity == event.entity); // 精确提交必须匹配冻结根。
        match event.outcome {
            NumericInputCommitOutcome::Canceled => {
                editor.status = format!("已取消水网{}精确输入；原值保持不变", kind.label()); // 中键编辑Esc只恢复控件内部基线。
                continue;
            }
            NumericInputCommitOutcome::Rejected => {
                editor.status = format!("水网{}不是有效数字，已恢复原值", kind.label()); // 公共控件已恢复显示，业务不写草稿。
                continue;
            }
            NumericInputCommitOutcome::Accepted if !event.edited => continue, // 未编辑失焦不量化原作者值。
            NumericInputCommitOutcome::Accepted => {} // 有效编辑继续复核稳定目标。
        }
        let Some(value) = event.value.as_f32() else {
            editor.status = "水网数值控件返回了非浮点提交".to_owned(); // 类型错配失败关闭。
            reset_input(event.entity, kind, &editor, &session, &mut set_values); // 恢复权威值。
            continue;
        };
        let target = focus
            .map(|focus| focus.target)
            .or_else(|| selected_target(&editor, &session)); // 同帧建立并提交的极端路径仍使用稳定当前目标。
        let Some(target) = target else {
            editor.status = "水网数值提交时没有可编辑控制点".to_owned(); // 不把值缓存到未来选择。
            reset_input(event.entity, kind, &editor, &session, &mut set_values); // 显示安全占位。
            continue;
        };
        apply_exact_value(
            event.entity,
            kind,
            value,
            target,
            &mut editor,
            &mut session,
            &mut set_values,
        ); // 精确提交立即完成一次受检作者事务。
    }

    for event in sensitivity_changed.read() {
        let Some(kind) = input_kind(&inputs, event.entity) else {
            continue; // 其他字段倍率消息不属于水网。
        };
        editor.status = format!(
            "正在调整水网{}拖动灵敏度：×{:.3}",
            kind.label(),
            event.value
        ); // 实际数值框继续显示业务值。
    }
    for event in sensitivity_committed.read() {
        let Some(kind) = input_kind(&inputs, event.entity) else {
            continue; // 其他字段倍率提交由其业务域保存。
        };
        let value = event.after.clamp(0.01, 100.0); // 持久化前再次限制公共倍率范围。
        let field = kind.sensitivity_field(); // 当前根决定唯一设置槽。
        if settings.scene_editor_scrub_sensitivities.get(field) != value {
            settings.scene_editor_scrub_sensitivities.set(field, value); // 松开右键才触发一次EditorSettings变更与自动保存。
        }
        editor.status = format!("水网{}拖动灵敏度已设为 ×{value:.3}", kind.label()); // 底栏确认字段独立倍率。
    }
}
