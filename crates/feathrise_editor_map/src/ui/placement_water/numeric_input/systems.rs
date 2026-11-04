//! 网格水专业数值框的作者绑定、精确提交和拖动事务。

use bevy::prelude::*; // ECS查询、Commands、消息和资源变更检测。
use feathrise_map_content::MapPlacementRecord; // 数值编辑始终以完整摆放记录收口。
use feathrise_scene::{DynamicWaterMeshOverrideNumericPatch, MapPlacementInstance}; // 拖中只发单字段补丁，离散操作仍重施完整覆盖。
use feathrise_editor_ui::{NumericInputChanged, NumericInputCommitSource, NumericInputCommitted}; // 共享 UI 控件发布字符输入、左拖、右拖和外部同步的唯一公共协议。

use super::transaction::{
    begin_numeric_scrub, input_context_valid, numeric_target_is_current,
    restore_numeric_scrub_raw_origin, settle_interrupted_numeric_scrub,
}; // 左拖的连续收口与异常恢复集中维护，系统只编排消息和预览。
use super::{
    PlacementWaterNumericInput, PlacementWaterNumericInputState, PlacementWaterNumericKind,
    PlacementWaterNumericScrub, PlacementWaterNumericTarget, set_record_numeric_value,
    set_record_numeric_value_during_scrub,
}; // 本模块拥有稳定绑定和一次拖动事务的全部业务状态。
use crate::authoring::{
    EditorAuthoringSession, EditorCommand, WaterFileHandoff, }; // 所有离散或拖动历史都进入统一水命令硬门。
use crate::resources::MapEditorState; // F12生命周期、dirty门和用户可见状态。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 当前已摆放Scene0网格稳定选择。
use crate::ui::placement_water::{
    PlacementInstanceIndex, preview_placement_numeric_patch, preview_placement_overrides,
}; // 与类别按钮共享根索引、完整异常重施和O(1)拖中补丁桥。

mod exact_input; // 中键字符提交、焦点冻结与右拖灵敏度独立编排，拖动事务留在本文件。

pub(crate) use exact_input::handle_placement_water_numeric_input; // 保持父模块原有系统注册路径和crate内可见性不变。

type AdoptedPlacementRoots<'w, 's> = Query<'w, 's, (Entity, &'static MapPlacementInstance)>; // 实际写入由共享活动门原子隔离候选与退役根。

/// 数值失败与同帧保存请求共用出口，避免旧值继续落盘后掩盖真正错误。
fn reject_numeric_edit(editor: &mut MapEditorState, reason: impl Into<String>) {
    let reason = reason.into(); // 调用方可传静态文本或带身份的动态诊断。
    editor.status = if std::mem::take(&mut editor.save_requested) {
        format!("{reason}；本次保存已取消") // 同帧保存不能越过失败的用户编辑意图。
    } else {
        reason // 没有保存请求时直接显示准确失败原因。
    };
}

/// 按输入实体反查两个业务字段之一，其他NumericInput消息由各自模块处理。
fn input_kind(
    inputs: &Query<(Entity, &PlacementWaterNumericInput)>, // 两个固定业务根的小型查询。
    entity: Entity,                                        // 共享 UI 控件消息携带的根实体。
) -> Option<PlacementWaterNumericKind> {
    inputs.get(entity).ok().map(|(_, marker)| marker.0) // 关系换代时安全忽略迟到消息。
}

/// 精确字符提交按冻结身份写入完整记录，并立即发布一次运行时接管验证。
fn apply_exact_numeric_value(
    commands: &mut Commands,                         // 发布当前完整未保存覆盖。
    placement_roots: &AdoptedPlacementRoots<'_, '_>, // 隐藏保存候选必须保持发布快照。
    root_index: &PlacementInstanceIndex,             // 稳定ID只命中少量并存根。
    editor: &mut MapEditorState,                     // 更新dirty门和用户状态。
    session: &mut EditorAuthoringSession,            // 修改草稿、历史与作者修订。
    target: &PlacementWaterNumericTarget,            // 聚焦时冻结的稳定作者身份。
    kind: PlacementWaterNumericKind,                 // 当前输入根字段身份。
    value: f32,                                      // 已通过共享 UI 控件解析的候选浮点值。
) -> Result<Option<MapPlacementRecord>, String> {
    if target.authoring_revision != session.revision {
        return Err(format!(
            "{}输入基于作者修订{}，当前已是{}；过期提交已丢弃",
            kind.label(),
            target.authoring_revision,
            session.revision,
        )); // 字符编辑期间切图、撤销或其他作者动作不得被旧缓冲覆盖。
    }
    if target.placement_structure_revision != session.placement_structure_revision {
        return Err(format!(
            "{}输入绑定的摆放结构已经变化；过期提交已丢弃",
            kind.label(),
        )); // 即使普通作者修订数巧合，旧数组下标也不能跨结构代次复用。
    }
    let (before, after) = {
        let draft = session
            .draft
            .as_mut()
            .ok_or_else(|| "地图作者草稿尚未就绪".to_owned())?; // 先取得当前唯一作者文档。
        if draft.map_id != target.map_id {
            return Err(format!(
                "{}输入属于地图{}，当前草稿属于{}；过期提交已丢弃",
                kind.label(),
                target.map_id,
                draft.map_id,
            )); // 地图身份变化即使placement同名也必须失败关闭。
        }
        let record = draft
            .placements
            .placements
            .get_mut(target.record_index)
            .filter(|record| record.id == target.placement_id)
            .ok_or_else(|| "输入绑定的摆放实例已经不存在".to_owned())?; // 稳定ID必须仍唯一存在。
        let before = record.clone(); // 完整记录是唯一撤销起点。
        if !set_record_numeric_value(record, target, kind, value)? {
            return Ok(None); // 显式重输相同位级值不制造作者命令。
        }
        (before, record.clone()) // 借用结束后再重施ECS覆盖与修改历史。
    };
    preview_placement_overrides(commands, session, root_index, placement_roots, &after); // 同帧提交完整覆盖，运行时随后发布Pending/Ready/Failed。
    let session = &mut *session; // 显式拆借两个历史字段供统一硬门使用。
    session.push_bounded_water_history(EditorCommand::PlacementChanged {
        record_index: target.record_index, // 精确输入冻结的结构下标供撤销/重做O(1)复用。
        before,                            // 精确输入前完整placement快照。
        after: after.clone(),              // 精确输入后已经受检的完整快照。
    }); // 所有整网格水历史都受统一命令数和堆内存前缀裁剪。
    session.mark_changed(editor); // placements JSON进入保存门并推进作者修订。
    Ok(Some(after)) // 调用方使用最终记录生成精确成功提示。
}

/// 任一异常出口都先处理已经实时写入的事务，再清除活动状态并报告原始原因。
fn reject_interrupted_numeric_scrub(
    commands: &mut Commands, // 回滚或外部接管后重施当前完整覆盖。
    placement_roots: &AdoptedPlacementRoots<'_, '_>, // 只匹配已采用正式根，编辑器预览根由session精确定位。
    root_index: &PlacementInstanceIndex,             // 异常恢复也不能退回全图扫描。
    editor: &mut MapEditorState,                     // 合并恢复说明并取消同帧保存请求。
    session: &mut EditorAuthoringSession,            // 安全入栈或撤销未入栈实时字段。
    transaction: PlacementWaterNumericScrub,         // 调用方已经从资源take并转移的唯一活动事务。
    reason: impl Into<String>,                       // 触发异常中止的准确原始诊断。
) {
    let reason = reason.into(); // 在可变借用前统一取得用户可见原因所有权。
    match settle_interrupted_numeric_scrub(editor, session, transaction) {
        Ok(resolution) => {
            if let Some(record) = resolution.preview_record.as_ref() {
                preview_placement_overrides(commands, session, root_index, placement_roots, record); // 回滚或外部权威记录立即清掉旧拖动画面。
            }
            reject_numeric_edit(editor, format!("{reason}；{}", resolution.summary)); // 状态同时说明异常与可撤销性处理结果。
        }
        Err(recovery_error) => reject_numeric_edit(
            editor,
            format!("{reason}；拖动异常恢复失败：{recovery_error}"),
        ), // 构造不变量损坏时明确失败，不静默声称已经产生撤销或回滚。
    }
}

/// 消费左拖Changed实时发布单字段补丁，松开Scrub提交只压入一个有界撤销命令。
pub(crate) fn apply_placement_water_numeric_scrub(
    mut commands: Commands, // 每个新吸附档即时发布一个Copy数值补丁。
    mut changed: MessageReader<NumericInputChanged>, // 共享 UI 控件左拖途中的规范化实际值。
    mut committed: MessageReader<NumericInputCommitted>, // Scrub松开只用于事务收口。
    inputs: Query<(Entity, &PlacementWaterNumericInput)>, // 消息实体到字段身份映射。
    browser: Res<EditorGltfBrowserState>, // 第一次变化与后续帧复核稳定选择。
    root_index: Res<PlacementInstanceIndex>, // 连续拖动每档按同图稳定ID定位根。
    placement_roots: AdoptedPlacementRoots, // 补丁只触及当前活动正式根或精确作者预览根。
    mut state: ResMut<PlacementWaterNumericInputState>, // 唯一左拖事务。
    mut editor: ResMut<MapEditorState>, // dirty门和用户状态。
    mut session: ResMut<EditorAuthoringSession>, // 实时修改草稿并推进作者修订。
    handoff: Res<WaterFileHandoff>, // 正式placement交接在途时异常收口旧拖动并拒绝新值。
) {
    let placement_handoff_in_flight = session
        .draft
        .as_ref()
        .is_some_and(|draft| handoff.placement_handoff_in_flight_for(&draft.map_id));
    if state.scrub.is_some() && !input_context_valid(&editor, &session, placement_handoff_in_flight)
    {
        let transaction = state.scrub.take().expect("上方已确认活动左拖存在"); // F12关闭或保存抢占前先取得唯一完整事务。
        state.abort_scrub_until_release(transaction.entity, transaction.kind); // 共享 UI 控件尚未松键时不允许后续Changed重建事务。
        reject_interrupted_numeric_scrub(
            &mut commands,
            &placement_roots,
            &root_index,
            &mut editor,
            &mut session,
            transaction,
            "当前界面或保存状态不能继续网格水拖动",
        ); // Presentation清理冻结状态前保证已有实时变化已入栈或回滚。
    }
    for event in changed.read() {
        let Some(kind) = input_kind(&inputs, event.entity) else {
            continue; // 水位和笔刷拖动消息由各自作者域处理。
        };
        if state.scrub_is_aborted() {
            continue; // 异常回滚后同一次物理拖动的迟到Changed必须全部丢弃，不能重新begin覆盖外部结果。
        }
        let Some(value) = event.value.as_f32() else {
            reject_numeric_edit(&mut editor, "网格水拖动产生了非浮点值");
            continue;
        };
        let value = match kind.validate_value(value) {
            Ok(value) => value, // 共享 UI 控件范围外再做一次公共作者域复核。
            Err(error) => {
                reject_numeric_edit(&mut editor, error);
                continue;
            }
        };
        if state.scrub.is_none() {
            match begin_numeric_scrub(
                event.entity,
                kind,
                &browser,
                &editor,
                &session,
                placement_handoff_in_flight,
            ) {
                Ok(transaction) => state.scrub = Some(transaction), // 第一次真实变化才冻结完整起点。
                Err(error) => {
                    state.abort_scrub_until_release(event.entity, kind); // 首次Changed失败后也等到对应松键再允许新事务。
                    reject_numeric_edit(&mut editor, error);
                    continue;
                }
            }
        }
        let Some(transaction) = state.scrub.as_ref() else {
            continue; // 防御事务建立失败后的空状态。
        };
        if transaction.entity != event.entity || transaction.kind != kind {
            let transaction = state.scrub.take().expect("上方已确认活动左拖存在"); // 输入根异常换代前保留已有完整起终点。
            state.abort_scrub_until_release(event.entity, kind); // 当前共享 UI 控件捕获根保持拒绝到本次物理松键。
            reject_interrupted_numeric_scrub(
                &mut commands,
                &placement_roots,
                &root_index,
                &mut editor,
                &mut session,
                transaction,
                "网格水拖动输入根在事务中途发生变化",
            ); // 连续实时变化先入栈，不能因迟到的其他根消息丢失撤销。
            continue;
        }
        if !input_context_valid(&editor, &session, placement_handoff_in_flight) {
            let transaction = state.scrub.take().expect("上方已确认活动左拖存在"); // 生命周期异常前冻结已有实时终点。
            state.abort_scrub_until_release(transaction.entity, transaction.kind); // 回滚或收口后禁止余下拖动帧重建事务。
            reject_interrupted_numeric_scrub(
                &mut commands,
                &placement_roots,
                &root_index,
                &mut editor,
                &mut session,
                transaction,
                "当前界面或保存状态不能继续网格水拖动",
            ); // 保存或关闭抢占也不能留下无历史草稿变化。
            continue;
        }
        if session.revision != transaction.expected_revision {
            let reason = format!(
                "{}拖动期望作者修订{}，当前已是{}；实时拖动已停止",
                kind.label(),
                transaction.expected_revision,
                session.revision,
            ); // take事务前先复制全部诊断字段，结束共享借用。
            let transaction = state.scrub.take().expect("上方已确认活动左拖存在"); // 修订冲突必须进入保外部变化的恢复路径。
            state.abort_scrub_until_release(transaction.entity, transaction.kind); // 外部权威接管后吞掉余下Changed直至松键。
            reject_interrupted_numeric_scrub(
                &mut commands,
                &placement_roots,
                &root_index,
                &mut editor,
                &mut session,
                transaction,
                reason,
            ); // 同字段外部值优先，仍归本拖动所有的实时字段安全回滚。
            continue;
        }
        if !numeric_target_is_current(&browser, &session, &transaction.target) {
            let transaction = state.scrub.take().expect("上方已确认活动左拖存在"); // 选择失效不能直接丢弃已有终点快照。
            state.abort_scrub_until_release(transaction.entity, transaction.kind); // 选择换代后的同次物理拖动不得绑定新选择。
            reject_interrupted_numeric_scrub(
                &mut commands,
                &placement_roots,
                &root_index,
                &mut editor,
                &mut session,
                transaction,
                "网格水拖动期间选择或绑定已经失效",
            ); // 草稿仍连续时正常入栈，真正外部换代时才安全回滚。
            continue;
        }
        let result = (|| {
            let transaction = state
                .scrub
                .as_ref()
                .ok_or_else(|| "网格水拖动事务在字段写入前已经结束".to_owned())?; // 当前帧只借用冻结目标，不复制其最多六十四段节点路径。
            let target = &transaction.target; // 两个作者数组下标和稳定身份全程借用比较。
            let draft = session
                .draft
                .as_mut()
                .ok_or_else(|| "地图作者草稿尚未就绪".to_owned())?; // 每帧复核当前草稿身份。
            if draft.map_id != target.map_id {
                return Err("网格水拖动不能跨地图继续".to_owned()); // 同名placement也不允许跨图。
            }
            let record = draft
                .placements
                .placements
                .get_mut(target.record_index)
                .filter(|record| record.id == target.placement_id)
                .ok_or_else(|| "拖动绑定的摆放实例已经不存在".to_owned())?; // 稳定ID必须仍存在。
            if !set_record_numeric_value_during_scrub(record, target, kind, value)? {
                return Ok(None); // 同一吸附档不推进修订或重复重施运行时材质。
            }
            Ok::<_, String>(Some(())) // 拖中只发布“当前字段已改变”，不克隆完整placement终点。
        })();
        match result {
            Ok(Some(())) => {}    // 当前档产生真实变化，继续借用当前草稿重施预览。
            Ok(None) => continue, // 重复档保持现有事务状态。
            Err(error) => {
                let transaction = state.scrub.take().expect("写入失败时活动左拖仍存在"); // 保留失败前已经实时应用的完整终点。
                state.abort_scrub_until_release(transaction.entity, transaction.kind); // 失败后剩余物理拖动不再写作者草稿。
                reject_interrupted_numeric_scrub(
                    &mut commands,
                    &placement_roots,
                    &root_index,
                    &mut editor,
                    &mut session,
                    transaction,
                    error,
                ); // 当前失败档未写入，先前成功档仍必须可撤销或已回滚。
                continue;
            }
        }
        {
            let transaction = state.scrub.as_ref().expect("当前拖动事务仍存在"); // 只借用冻结的两个O(1)作者下标。
            let patch = match kind {
                PlacementWaterNumericKind::FlowSpeed => {
                    DynamicWaterMeshOverrideNumericPatch::flow_speed(
                        transaction.target.surface_index,
                        value,
                    ) // scene侧再校验范围与冻结结构下标，只修改一个f32。
                }
                PlacementWaterNumericKind::HeightOffset => {
                    DynamicWaterMeshOverrideNumericPatch::height_offset(
                        transaction.target.surface_index,
                        value,
                    ) // 世界Y偏移档同样不复制整批路径或水绑定数组。
                }
            };
            preview_placement_numeric_patch(
                &mut commands,
                &session,
                &root_index,
                &placement_roots,
                &transaction.target.placement_id,
                patch,
            ); // 每个吸附档只插入Copy补丁，完整覆盖仅留给精确提交和异常回滚边沿。
        }
        session.mark_changed(&mut editor); // placements草稿推进修订并保持dirty。
        let transaction = state.scrub.as_mut().expect("当前拖动事务仍存在"); // 更新最终标量与连续修订证据。
        transaction.latest_value = value; // 每档只保存一个f32，最终完整快照推迟到松键。
        transaction.expected_revision = session.revision; // 下一Changed只接受本事务刚推进的修订。
        editor.status = format!(
            "正在拖动{}：{value:.precision$}",
            kind.label(),
            precision = usize::from(kind.decimals()),
        ); // 拖动中明确显示实际业务值而不是灵敏度。
    }

    for event in committed.read() {
        let Some(kind) = input_kind(&inputs, event.entity) else {
            continue; // 其他数字框提交不影响当前事务。
        };
        if event.source != NumericInputCommitSource::Scrub {
            continue; // 字符输入由精确提交系统处理。
        }
        if state.release_aborted_scrub(event.entity, kind) {
            continue; // 异常事务早已owned-field恢复；本次松键只清拒绝门，绝不二次提交。
        }
        let Some(transaction) = state.scrub.take() else {
            continue; // 未跨四像素或未产生真实Changed的拖动没有作者事务。
        };
        if transaction.entity != event.entity || transaction.kind != kind {
            reject_interrupted_numeric_scrub(
                &mut commands,
                &placement_roots,
                &root_index,
                &mut editor,
                &mut session,
                transaction,
                "网格水拖动结束消息与活动事务不匹配",
            ); // 错根提交也先保存或撤销真正活动事务的实时变化。
            continue;
        }
        if !event.changed {
            match restore_numeric_scrub_raw_origin(&mut editor, &mut session, &transaction) {
                Ok((true, preview_record)) => {
                    if let Some(record) = preview_record.as_ref() {
                        preview_placement_overrides(
                            &mut commands,
                            &session,
                            &root_index,
                            &placement_roots,
                            record,
                        ); // 显示精度掩盖的raw尾数恢复后，同帧把运行时水材质还原到作者精确值。
                    }
                    editor.status = format!("{}已拖回显示起点；作者原始精度保持不变", kind.label()); // no-op松键不进入PlacementChanged、redo清理或新dirty状态。
                }
                Ok((false, _)) => {
                    editor.status = format!(
                        "{}拖动结束前当前字段已被后来作者状态接管；未覆盖其结果",
                        kind.label()
                    ); // owned-field检查失败时只结束旧事务，不覆盖后写值。
                }
                Err(error) => reject_numeric_edit(
                    &mut editor,
                    format!("{}拖回显示起点时恢复作者精度失败：{error}", kind.label()),
                ), // 损坏的冻结起点必须明确报告，不能伪称无修改。
            }
            continue; // 共享 UI 控件已证明控件回到显示起点，本次绝不进入正常撤销收口。
        }
        match settle_interrupted_numeric_scrub(&mut editor, &mut session, transaction) {
            Ok(resolution) => {
                if let Some(record) = resolution.preview_record.as_ref() {
                    preview_placement_overrides(
                        &mut commands,
                        &session,
                        &root_index,
                        &placement_roots,
                        record,
                    ); // 最终完整校验失败或外部冲突时重施恢复后的当前权威记录。
                }
                if resolution.recovered {
                    reject_numeric_edit(&mut editor, resolution.summary); // 普通松键未能形成历史时明确显示恢复结论。
                }
            }
            Err(error) => reject_numeric_edit(
                &mut editor,
                format!("网格水拖动结束与异常恢复都失败：{error}"),
            ), // 损坏事务不能静默保留未入栈实时值。
        }
    }
}
