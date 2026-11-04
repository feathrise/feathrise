//! 网格水数值左拖的稳定身份、正常收口与异常恢复事务。

use bevy::prelude::Entity; // 事务起点冻结公共NumericInput根实体。
use feathrise_map_content::MapPlacementRecord; // 撤销、回滚与预览都使用完整摆放记录快照。

use super::{
    PlacementWaterNumericKind, PlacementWaterNumericScrub, PlacementWaterNumericTarget,
    set_record_numeric_value_during_scrub,
}; // 复用字段范围、稳定网格身份和唯一单字段修改门。
use crate::authoring::{EditorAuthoringSession, EditorCommand}; // 正常收口仍进入统一有界水历史。
use crate::resources::MapEditorState; // 事务冻结并恢复dirty状态，异常恢复保留外部作者dirty。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 第一次真实变化冻结当前Scene0选择。
use crate::ui::placement_water::surface_index; // 稳定节点路径和网格名定位唯一水绑定。

/// 异常结束的安全处理结果；只有回滚或外部权威重施才需要再次刷新运行时覆盖。
pub(super) struct InterruptedNumericScrubResolution {
    pub(super) preview_record: Option<MapPlacementRecord>, // 回滚后或外部覆盖后的当前权威完整记录。
    pub(super) summary: String, // 调用系统附加到原始异常原因后的用户可见说明。
    pub(super) recovered: bool, // true表示没有正常提交，普通松键必须显示恢复诊断。
}

/// 正常收口失败时连同原事务所有权一起返回，异常恢复无需重新克隆完整起点。
#[derive(Debug)] // 测试与上层诊断可直接展开失败，同时保留完整事务内容。
pub(super) struct NumericScrubFinishError {
    pub(super) reason: String, // 记录完整校验、地图或稳定身份失败的准确原因。
    pub(super) transaction: PlacementWaterNumericScrub, // 唯一起点快照原样交还恢复路径。
}

/// 保存、关闭、模态弹框或F12隐藏期间不能开始或继续网格水数值编辑。
pub(super) fn input_context_valid(
    editor: &MapEditorState,           // F12与模态生命周期门。
    session: &EditorAuthoringSession,  // 正在保存时完整placements快照已经冻结。
    placement_handoff_in_flight: bool, // 已保存隐藏候选未原子采用前不产生后续网格水草稿。
) -> bool {
    editor.open
        && !editor.close_requested
        && !editor.new_map_dialog_open
        && !session.saving
        && !placement_handoff_in_flight // 交接失败会释放该门，作者仍能继续修复。
}

/// 只有当前Ready GLB中的有效、非失效且已赋水网格才能形成可写目标。
pub(super) fn selected_editable_target(
    browser: &EditorGltfBrowserState, // 当前Scene0选择必须仍可持久化。
    session: &EditorAuthoringSession, // 地图身份与作者修订从当前草稿冻结。
) -> Option<PlacementWaterNumericTarget> {
    let selection = browser.selection.as_ref()?; // 未选择网格时保持输入禁用。
    if !selection.valid || selection.orphaned_water_binding {
        return None; // 名称不稳定或目标已经消失时只保留“取消水体”。
    }
    let draft = session.draft.as_ref()?; // 草稿缺失时不能冻结地图身份。
    let record_index = browser.placement_record_index(
        &selection.placement_id,
        session.placement_structure_revision,
    )?; // 只接受已经完整发布的当前结构索引；在途换代期间暂时禁用写入。
    let record = draft.placements.placements.get(record_index)?;
    if record.id != selection.placement_id || !record.editor.selectable() {
        return None; // 防御结构索引与草稿没有同批换代，绝不串写另一实例。
    }
    let surface_index = surface_index(record, selection)?; // 普通Blender网格尚未赋水时两个数值框禁用，并冻结唯一数组槽。
    Some(PlacementWaterNumericTarget {
        map_id: draft.map_id.clone(),                 // 同名实例不能跨地图串写。
        placement_id: selection.placement_id.clone(), // 完整记录稳定ID。
        record_index,  // 后续每个拖动档直接索引，不再扫描最多8192项。
        surface_index, // 拖中直接复核并写此槽，不再扫描同placement其余绑定。
        node_path: selection.node_path.clone(), // Blender对象显式路径。
        mesh_name: selection.mesh_name.clone(), // Blender Mesh datablock显式名。
        authoring_revision: session.revision, // 字符输入期间任何外部作者变化都令绑定过期。
        placement_structure_revision: session.placement_structure_revision, // 数组结构变化立即令下标过期。
    })
}

/// 拖中只用借用比较复核当前选择与冻结目标，不再每个吸附档复制完整节点路径。
pub(super) fn numeric_target_is_current(
    browser: &EditorGltfBrowserState, // 当前Scene0选择与结构索引均为上一完整发布代。
    session: &EditorAuthoringSession, // 当前草稿、地图和结构修订提供作者权威。
    target: &PlacementWaterNumericTarget, // 第一次真实Changed冻结的稳定身份与两个数组下标。
) -> bool {
    if session.placement_structure_revision != target.placement_structure_revision {
        return false; // 绑定增删、重排、类别或来源变化都让冻结surface下标失效。
    }
    let Some(selection) = browser.selection.as_ref() else {
        return false; // 选择被清空时停止旧拖动。
    };
    if !selection.valid
        || selection.orphaned_water_binding
        || selection.placement_id != target.placement_id
        || selection.node_path != target.node_path
        || selection.mesh_name != target.mesh_name
    {
        return false; // 借用比较全部稳定身份，不构造第二份目标路径。
    }
    if browser.placement_record_index(&target.placement_id, session.placement_structure_revision)
        != Some(target.record_index)
    {
        return false; // 浏览器完整结构索引必须仍指向同一placement槽。
    }
    let Some(draft) = session
        .draft
        .as_ref()
        .filter(|draft| draft.map_id == target.map_id)
    else {
        return false; // 同名placement不能跨地图继承拖动。
    };
    draft
        .placements
        .placements
        .get(target.record_index)
        .is_some_and(|record| target.surface(record).is_some()) // 冻结surface槽仍须命中同一路径和网格名。
}

/// 第一次真实Changed冻结当前完整记录，四像素以内的左键无动作单击不会进入这里。
pub(super) fn begin_numeric_scrub(
    entity: Entity,                    // 当前共享 UI 控件左拖根。
    kind: PlacementWaterNumericKind,   // 事务期间不能从流速切到高度。
    browser: &EditorGltfBrowserState,  // 第一次变化时读取最终Scene0选择。
    editor: &MapEditorState,           // 冻结拖动前dirty门。
    session: &EditorAuthoringSession,  // 冻结地图、修订和完整记录起点。
    placement_handoff_in_flight: bool, // 保存候选在途时禁止建立新事务。
) -> Result<PlacementWaterNumericScrub, String> {
    if !input_context_valid(editor, session, placement_handoff_in_flight) {
        return Err("当前界面或保存状态不能拖动网格水数值".to_owned()); // 隐藏或保存期间失败关闭。
    }
    let target = selected_editable_target(browser, session)
        .ok_or_else(|| "当前选择没有可编辑的有效网格水绑定".to_owned())?; // 普通或失效绑定不开始事务。
    let draft = session
        .draft
        .as_ref()
        .ok_or_else(|| "地图作者草稿尚未就绪".to_owned())?; // 目标构造成功时草稿仍必须存在。
    let record = draft
        .placements
        .placements
        .get(target.record_index)
        .filter(|record| record.id == target.placement_id)
        .ok_or_else(|| "拖动绑定的摆放实例已经不存在".to_owned())?; // O(1)下标仍复核稳定ID。
    let initial_value = target
        .surface(record)
        .map(|surface| kind.surface_value(surface))
        .ok_or_else(|| "拖动绑定的网格水数组槽已经失效".to_owned())?; // 构造时冻结唯一目标字段起点。
    Ok(PlacementWaterNumericScrub {
        entity,                              // 后续Changed和Committed必须来自同一根。
        kind,                                // 后续值使用同一范围与字段写入。
        target,                              // 冻结地图和稳定网格身份。
        before: record.clone(),              // 唯一一次完整起点克隆只发生在拖动边沿。
        initial_value,                       // 无净变化和异常恢复只比较这个标量。
        latest_value: initial_value,         // 第一次实际写入后才更新，不保存第二份完整记录。
        expected_revision: session.revision, // 第一次变化前的作者修订。
        dirty_before: editor.dirty,          // 拖回起点时恢复原保存门。
    })
}

/// 对已经按当前地图和稳定下标定位的记录执行松键完整校验，并形成至多一条历史。
fn prepare_numeric_scrub_record(
    editor: &mut MapEditorState,             // 恢复dirty或发布最终状态。
    revision: u64,                           // 调用边沿冻结的当前作者修订。
    transaction: PlacementWaterNumericScrub, // 消费事务后可把完整起点直接移入历史。
    record: &MapPlacementRecord,             // 当前草稿冻结下标处的权威最终记录。
) -> Result<Option<EditorCommand>, NumericScrubFinishError> {
    let validation = (|| -> Result<bool, String> {
        if revision != transaction.expected_revision {
            return Err(format!(
                "{}拖动结束前作者修订已变化；未合并为撤销命令",
                transaction.kind.label(),
            )); // 外部作者动作介入时绝不把不连续快照伪装成一次拖动。
        }
        let before_value = transaction
            .target
            .surface(&transaction.before)
            .map(|surface| transaction.kind.surface_value(surface))
            .ok_or_else(|| "拖动起点缺少冻结下标对应的网格水绑定".to_owned())?; // 完整起点必须保留同一稳定槽。
        if before_value != transaction.initial_value {
            return Err("拖动事务的完整起点与冻结标量不一致".to_owned()); // 损坏事务不能生成伪造撤销边界。
        }
        let final_value = transaction
            .target
            .surface(record)
            .map(|surface| transaction.kind.surface_value(surface))
            .ok_or_else(|| "拖动结束时网格水绑定已经失效".to_owned())?; // 冻结surface槽必须仍命中同一稳定身份。
        if final_value != transaction.latest_value {
            return Err("拖动结束时当前字段已被其他作者状态替代".to_owned()); // 未推进修订的异常写入也不能伪装成拖动终点。
        }
        record.validate_water_surfaces()?; // 整段拖动只在松键边沿建立一次完整唯一性和格式证据。
        Ok(transaction.initial_value != transaction.latest_value) // 位级起终点差异决定是否需要历史终点。
    })();
    let changed = match validation {
        Ok(changed) => changed, // 完整记录有效后才允许克隆终点或改写历史。
        Err(reason) => {
            return Err(NumericScrubFinishError {
                reason,      // 失败原因与唯一事务一起交给异常恢复。
                transaction, // 起点所有权尚未移动，可安全定向回滚当前字段。
            });
        }
    };
    if !changed {
        editor.dirty = transaction.dirty_before; // 拖回完整起点不制造新的未保存状态。
        editor.status = format!(
            "{}保持{value:.precision$}",
            transaction.kind.label(),
            value = transaction.latest_value,
            precision = usize::from(transaction.kind.decimals()),
        ); // 明确说明本次拖动没有净变化。
        return Ok(None); // 没有净变化，不生成任何历史候选。
    }
    let after = record.clone(); // 完整校验通过且存在净变化后只克隆一次终点记录。
    let PlacementWaterNumericScrub {
        kind,         // 状态栏在事务消费后继续使用冻结字段身份。
        target,       // 历史命令直接取得冻结的O(1)记录下标。
        before,       // 开始拖动时的唯一完整克隆直接移入撤销命令。
        latest_value, // 状态栏显示最终已校验标量。
        ..            // 其余连续性与恢复字段完成收口后自然释放。
    } = transaction;
    let command = EditorCommand::PlacementChanged {
        record_index: target.record_index, // 左拖起点已冻结并持续复核的结构下标。
        before,                            // 不再为松键额外深克隆起点记录。
        after,                             // 松开完整校验后唯一一次克隆的终点记录。
    }; // 先归还作者记录借用，再由会话统一采用历史并调整画布归属游标。
    editor.status = format!(
        "{}已拖到{value:.precision$}；本次拖动形成一个可撤销操作",
        kind.label(),
        value = latest_value,
        precision = usize::from(kind.decimals()),
    ); // 松开后说明单次撤销语义。
    Ok(Some(command)) // 调用方取得唯一完整候选，不在记录核心修改历史栈。
}

/// 纯记录回归保留原Vec接口；生产收口始终通过会话入口维护画布归属。
#[cfg(test)]
pub(super) fn finish_numeric_scrub_record(
    editor: &mut MapEditorState,
    undo: &mut Vec<EditorCommand>,
    redo: &mut Vec<EditorCommand>,
    revision: u64,
    transaction: PlacementWaterNumericScrub,
    record: &MapPlacementRecord,
) -> Result<bool, NumericScrubFinishError> {
    let Some(command) = prepare_numeric_scrub_record(editor, revision, transaction, record)? else {
        return Ok(false);
    };
    crate::authoring::push_bounded_water_history(undo, redo, command);
    Ok(true)
}

/// 松开左键后按当前MapId和两个冻结下标借用唯一记录，再调用纯记录收口核心。
pub(super) fn finish_numeric_scrub(
    editor: &mut MapEditorState,             // 恢复dirty或发布最终状态。
    session: &mut EditorAuthoringSession,    // 拆借当前草稿、修订和两侧历史。
    transaction: PlacementWaterNumericScrub, // 成功时把唯一完整起点所有权移入撤销命令。
) -> Result<bool, NumericScrubFinishError> {
    let Some(draft) = session.draft
        .as_ref()
        .filter(|draft| draft.map_id == transaction.target.map_id)
    else {
        return Err(NumericScrubFinishError {
            reason: "拖动结束时当前地图草稿已经失效".to_owned(), // endpoint必须来自同一地图当前权威。
            transaction, // 地图失败时仍保留唯一完整起点供切图安全恢复结论。
        });
    };
    let Some(record) = draft
        .placements
        .placements
        .get(transaction.target.record_index)
        .filter(|record| record.id == transaction.target.placement_id)
    else {
        return Err(NumericScrubFinishError {
            reason: "拖动结束时摆放实例已经失效".to_owned(), // 不按ID扫描最多8192项。
            transaction, // 外部删除分支仍需用原事务生成准确恢复说明。
        });
    };
    let Some(command) =
        prepare_numeric_scrub_record(editor, session.revision, transaction, record)?
    else {
        return Ok(false);
    };
    session.push_bounded_water_history(command); // 完整记录借用结束后，统一裁剪并移动历史归属游标。
    Ok(true)
}

/// 修订冲突时只撤销仍由本拖动拥有的字段；合法外部同记录修改必须原样保留。
pub(super) fn restore_interrupted_record(
    current: &mut MapPlacementRecord, // 当前草稿中的外部权威完整记录。
    transaction: &PlacementWaterNumericScrub, // 本拖动的完整起点与初始/最新字段标量。
) -> Result<bool, String> {
    let before_value = transaction
        .target
        .surface(&transaction.before)
        .map(|surface| transaction.kind.surface_value(surface))
        .ok_or_else(|| "异常恢复缺少拖动起点水绑定".to_owned())?; // 构造期保证冻结槽存在，仍防御损坏事务。
    if before_value != transaction.initial_value {
        return Err("异常恢复的完整起点与冻结标量不一致".to_owned()); // 不使用来源不明的回滚值。
    }
    let Some(current_value) = transaction
        .target
        .surface(current)
        .map(|surface| transaction.kind.surface_value(surface))
    else {
        return Ok(false); // 外部操作已删除绑定，本拖动的可见效果已经不存在。
    };
    if transaction.initial_value == transaction.latest_value
        || current_value != transaction.latest_value
    {
        return Ok(false); // 无净变化或外部已经改写同一字段时绝不覆盖外部结果。
    }
    set_record_numeric_value_during_scrub(
        current,
        &transaction.target,
        transaction.kind,
        transaction.initial_value,
    ) // 无论是否夹带外部变化，都只撤销本事务拥有的一个字段。
}

/// 共享 UI 控件判定拖动回到“控件显示起点”时，定向恢复作者记录中可能更高精度的原始值。
pub(super) fn restore_numeric_scrub_raw_origin(
    editor: &mut MapEditorState, // 只有连续事务才能恢复进入拖动前的dirty门。
    session: &mut EditorAuthoringSession, // 当前草稿提供稳定placement与水绑定槽。
    transaction: &PlacementWaterNumericScrub, // 冻结raw起点、末次显示档和完整稳定身份。
) -> Result<(bool, Option<MapPlacementRecord>), String> {
    let revision_matches = session.revision == transaction.expected_revision; // 外部作者修订存在时保留其dirty状态。
    let Some(draft) = session
        .draft
        .as_mut()
        .filter(|draft| draft.map_id == transaction.target.map_id)
    else {
        return Ok((false, None)); // 切图或卸载后旧事务不得重新插回原地图值。
    };
    let Some(record) = draft
        .placements
        .placements
        .get_mut(transaction.target.record_index)
        .filter(|record| record.id == transaction.target.placement_id)
    else {
        return Ok((false, None)); // 稳定placement已被删除或换槽时不扫描并覆盖其他实例。
    };
    let current_value = transaction
        .target
        .surface(record)
        .map(|surface| transaction.kind.surface_value(surface)); // 只读取冻结surface槽的当前业务值。
    if current_value != Some(transaction.latest_value) {
        return Ok((false, None)); // 同字段已被后来作者值接管时绝不覆盖其结果。
    }
    let restored = restore_interrupted_record(record, transaction)?; // owned-field恢复保留同记录上的其他外部修改。
    let preview_record = restored.then(|| record.clone()); // 只有raw尾数真实恢复时才重施一次完整运行时覆盖。
    if revision_matches {
        editor.dirty = transaction.dirty_before; // 纯拖离再回起点不留下未保存提示。
    }
    Ok((true, preview_record)) // owned=true表示本次无净变化可安全跳过撤销收口。
}

/// 修订冲突或松键完整校验失败时定向撤销本拖动字段，并保留外部其他作者变化。
fn recover_interrupted_numeric_scrub(
    editor: &mut MapEditorState, // 真正回滚会推进作者修订并保持外部dirty。
    session: &mut EditorAuthoringSession, // 当前草稿是异常恢复唯一可写权威。
    transaction: &PlacementWaterNumericScrub, // 只读取完整起点和初始/最新两个标量。
) -> Result<InterruptedNumericScrubResolution, String> {
    let recovery = {
        let Some(draft) = session
            .draft
            .as_mut()
            .filter(|draft| draft.map_id == transaction.target.map_id)
        else {
            return Ok(InterruptedNumericScrubResolution {
                preview_record: None, // 切图或卸载后的旧根由地图生命周期负责清理。
                summary: "外部作者状态已替代旧地图，未覆盖其结果".to_owned(), // 旧草稿已不再可见。
                recovered: true,      // 普通松键也必须报告本事务没有形成历史。
            });
        };
        let Some(record) = draft
            .placements
            .placements
            .get_mut(transaction.target.record_index)
            .filter(|record| record.id == transaction.target.placement_id)
        else {
            return Ok(InterruptedNumericScrubResolution {
                preview_record: None, // 外部删除实例后没有可重施目标。
                summary: "外部作者状态已移除旧摆放，未覆盖其结果".to_owned(), // 删除优先于旧拖动恢复。
                recovered: true, // 普通松键必须明确说明没有形成历史。
            });
        };
        let restored = restore_interrupted_record(record, transaction)?; // 只撤销当前值仍等于latest的事务字段。
        (restored, record.clone()) // 异常边沿才克隆当前权威记录供运行时重施。
    };
    if recovery.0 {
        session.mark_changed(editor); // 回滚本身换代当前草稿，迟到后台结果不得覆盖恢复值。
        Ok(InterruptedNumericScrubResolution {
            preview_record: Some(recovery.1), // 立即把恢复后的完整记录重施到正式或预览根。
            summary: "未入栈的实时变化已回滚，合法外部作者变化保持不变".to_owned(),
            recovered: true, // 调用方应把恢复结论展示给作者。
        })
    } else {
        Ok(InterruptedNumericScrubResolution {
            preview_record: Some(recovery.1), // 重施当前外部权威记录，清掉可能残留的旧拖动画面。
            summary: "外部作者状态已接管当前字段，未覆盖其结果".to_owned(),
            recovered: true, // 当前拖动没有正常形成历史。
        })
    }
}

/// 异常中止必须把已有实时变化安全入栈，或撤销仍由本事务拥有的草稿效果。
pub(super) fn settle_interrupted_numeric_scrub(
    editor: &mut MapEditorState, // 连续时正常收口，冲突回滚时保留外部dirty。
    session: &mut EditorAuthoringSession, // 当前作者修订、草稿与历史。
    transaction: PlacementWaterNumericScrub, // 调用方take后转移唯一事务，正常收口移动完整起点。
) -> Result<InterruptedNumericScrubResolution, String> {
    if session.revision != transaction.expected_revision {
        return recover_interrupted_numeric_scrub(editor, session, &transaction); // 外部修订冲突只撤销仍由本事务拥有的字段。
    }
    match finish_numeric_scrub(editor, session, transaction) {
        Ok(changed) => Ok(InterruptedNumericScrubResolution {
            preview_record: None, // 最后实时快照已经重施，无需重复排队。
            summary: if changed {
                "已有实时变化已安全收口为一个可撤销操作".to_owned() // 异常只停止后续输入，不丢历史。
            } else {
                "拖动已回到起点，没有产生额外历史".to_owned() // 无净变化恢复进入前dirty。
            },
            recovered: false, // 完整校验与历史提交已经正常完成。
        }),
        Err(error) => {
            let mut resolution =
                recover_interrupted_numeric_scrub(editor, session, &error.transaction)?; // 松键校验失败时借用被交还的唯一事务，不复制完整起点。
            resolution.summary = format!(
                "拖动最终完整校验失败：{}；{}",
                error.reason, resolution.summary,
            ); // 一次说明正式拒绝原因与实际恢复结果。
            Ok(resolution)
        }
    }
}
