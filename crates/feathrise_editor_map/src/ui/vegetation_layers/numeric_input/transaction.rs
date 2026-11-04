//! 植被连续参数的稳定作者目标与单次撤销事务。

use std::collections::BTreeMap; // 撤销命令保留作者层ID到R8 Arc的稳定表。
use std::sync::Arc; // 参数拖动不复制任何R8像素。

use bevy::prelude::*; // Entity用于冻结消息根身份。
use feathrise_map_content::{MapId, MapVegetationAuthorGroupMode, MapVegetationAuthoring}; // 作者快照和组规则直接使用正式公共格式。
use feathrise_editor_ui::NumericValue; // I32/F32消息在业务边界做第二次类型和值域复核。

use crate::authoring::{EditorAuthoringSession, EditorCommand, VegetationAuthoringChange}; // 完整前后快照进入既有撤销、保存和single-flight编译链路。
use crate::resources::MapEditorState; // 当前选择、模态状态与dirty门。

use super::super::operations::validate_density_table; // 描述和Arc画布表必须同次完整存在。
use super::{VegetationNumericInputState, VegetationNumericKind}; // 字段规则与唯一事务资源。

/// 一次字符编辑或左拖冻结的完整稳定目标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct VegetationNumericTarget {
    pub(super) map_id: MapId,               // 同名层不能跨地图继续。
    pub(super) layer_id: String,            // 数组重排后仍按稳定作者层ID定位。
    pub(super) group_id: String,            // 共享组参数不会误写到切换后的组。
    pub(super) kind: VegetationNumericKind, // 同一实体事务不可中途换字段。
    pub(super) authoring_revision: u64,     // 只接受本次事务期间未被外部作者动作换代的草稿。
}

/// 中键精确输入聚焦时冻结目标，失焦提交不读取可能已经变化的数组下标。
#[derive(Clone, Debug)]
pub(super) struct VegetationNumericFocus {
    pub(super) entity: Entity,                  // 公共消息返回的业务根。
    pub(super) target: VegetationNumericTarget, // 聚焦首帧的完整稳定身份。
}

/// 左拖第一次产生真实数值变化时建立，松手前不进入撤销栈或后台编译。
pub(super) struct VegetationNumericScrub {
    pub(super) entity: Entity,                        // 唯一活动公共输入根。
    pub(super) target: VegetationNumericTarget,       // 地图、层、组、字段和起始修订。
    before: MapVegetationAuthoring,                   // 小型JSON描述完整快照，失败可原子恢复。
    before_densities: BTreeMap<String, Arc<Vec<u8>>>, // 只复制Arc句柄，不复制R8整图。
    before_value: NumericValue, // 外部修订冲突时只恢复仍由本拖动拥有的单字段。
    latest_value: NumericValue, // 判断当前字段是否仍等于本拖动最后写入值。
}

/// 当前选择的数值、稳定目标和编辑可用性，供输入与Presentation共享。
pub(super) struct SelectedVegetationNumeric {
    pub(super) target: VegetationNumericTarget, // 下一次聚焦或拖动可冻结的身份。
    pub(super) value: NumericValue,             // UI百分比已经完成作者值转换。
    pub(super) editable: bool,                  // 锁定或规则不相关时仍显示但禁止输入。
}

/// 从当前选择解析完整稳定身份；缺失任一正式引用都不建立半有效输入。
pub(super) fn selected_numeric(
    kind: VegetationNumericKind,
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
) -> Option<SelectedVegetationNumeric> {
    let draft = session.draft.as_ref()?; // 草稿未载入时字段统一禁用。
    let authoring = draft.vegetation.as_ref()?.authoring.as_ref()?; // 旧地图未建立作者层时不伪造值。
    let layer = authoring.layers.get(editor.selected_vegetation_layer)?; // UI索引只在此处转成稳定ID。
    let group = authoring
        .groups
        .iter()
        .find(|group| group.id == layer.group_id)?; // 组字段必须绑定层当前引用的真实组。
    let target = VegetationNumericTarget {
        map_id: draft.map_id.clone(),
        layer_id: layer.id.clone(),
        group_id: group.id.clone(),
        kind,
        authoring_revision: session.revision,
    };
    let value = value_from_authoring(authoring, &target)?; // 统一转换百分比和I32。
    let editable = editor.open
        && !editor.close_requested
        && !editor.new_map_dialog_open
        && !session.saving
        && !session.vegetation_cross_busy
        && !layer.locked
        && kind.relevant_for_group(group.mode); // 不相关共享参数明确禁用，避免改了却看不出效果。
    Some(SelectedVegetationNumeric {
        target,
        value,
        editable,
    })
}

/// 复核冻结目标仍对应当前地图、修订、选择层和组。
pub(super) fn target_is_current(
    target: &VegetationNumericTarget,
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
) -> bool {
    if session.revision != target.authoring_revision {
        return false; // 任一外部作者命令都使旧焦点失效。
    }
    selected_numeric(target.kind, editor, session)
        .is_some_and(|selected| selected.target == *target) // 当前选择也必须仍是冻结稳定层和组。
}

/// 返回冻结目标在指定作者描述中的业务显示值。
fn value_from_authoring(
    authoring: &MapVegetationAuthoring,
    target: &VegetationNumericTarget,
) -> Option<NumericValue> {
    let layer = authoring
        .layers
        .iter()
        .find(|layer| layer.id == target.layer_id && layer.group_id == target.group_id)?;
    let group = authoring
        .groups
        .iter()
        .find(|group| group.id == target.group_id)?;
    Some(match target.kind {
        VegetationNumericKind::Priority => NumericValue::I32(layer.priority),
        VegetationNumericKind::Weight => NumericValue::F32(layer.weight),
        VegetationNumericKind::OpacityPercent => NumericValue::F32(layer.opacity * 100.0),
        VegetationNumericKind::GroupCapacityPercent => NumericValue::F32(group.capacity * 100.0),
        VegetationNumericKind::ClaimThresholdPercent => {
            NumericValue::F32(group.claim_threshold * 100.0)
        }
    })
}

/// 把受检公共值写进局部作者描述；返回是否发生真实变化。
fn set_authoring_value(
    authoring: &mut MapVegetationAuthoring,
    target: &VegetationNumericTarget,
    value: NumericValue,
) -> Result<bool, String> {
    let layer_index = authoring
        .layers
        .iter()
        .position(|layer| layer.id == target.layer_id && layer.group_id == target.group_id)
        .ok_or_else(|| "植被数值绑定的作者图层或分组已经失效".to_owned())?;
    if authoring.layers[layer_index].locked {
        return Err(format!(
            "作者图层“{}”已锁定，请先解锁",
            authoring.layers[layer_index].label
        ));
    }
    let group_index = authoring
        .groups
        .iter()
        .position(|group| group.id == target.group_id)
        .ok_or_else(|| "植被数值绑定的作者组已经失效".to_owned())?;
    let group_mode = authoring.groups[group_index].mode;
    if !target.kind.relevant_for_group(group_mode) {
        return Err(match target.kind {
            VegetationNumericKind::GroupCapacityPercent => {
                "组容量只在“共享容量”规则下可编辑".to_owned()
            }
            VegetationNumericKind::ClaimThresholdPercent => {
                "覆盖阈值只在“高优先级覆盖”规则下可编辑".to_owned()
            }
            _ => "当前植被参数与组规则不匹配".to_owned(),
        });
    }
    match target.kind {
        VegetationNumericKind::Priority => {
            let value = value
                .as_i32()
                .ok_or_else(|| "植被优先级控件返回了非I32数值".to_owned())?;
            let slot = &mut authoring.layers[layer_index].priority;
            let changed = *slot != value;
            *slot = value;
            Ok(changed)
        }
        VegetationNumericKind::Weight => {
            let value = require_f32_range(value, 0.05, 16.0, "植被权重")?;
            let slot = &mut authoring.layers[layer_index].weight;
            let changed = *slot != value;
            *slot = value;
            Ok(changed)
        }
        VegetationNumericKind::OpacityPercent => {
            let value = require_f32_range(value, 0.0, 100.0, "植被透明度")? / 100.0;
            let slot = &mut authoring.layers[layer_index].opacity;
            let changed = *slot != value;
            *slot = value;
            Ok(changed)
        }
        VegetationNumericKind::GroupCapacityPercent => {
            debug_assert_eq!(group_mode, MapVegetationAuthorGroupMode::WeightedPartition); // 上方规则门已经验证相关性。
            let value = require_f32_range(value, 0.0, 100.0, "植被组容量")? / 100.0;
            let slot = &mut authoring.groups[group_index].capacity;
            let changed = *slot != value;
            *slot = value;
            Ok(changed)
        }
        VegetationNumericKind::ClaimThresholdPercent => {
            debug_assert_eq!(group_mode, MapVegetationAuthorGroupMode::PriorityOverride); // 上方规则门已经验证相关性。
            let value = require_f32_range(value, 0.0, 100.0, "植被覆盖阈值")? / 100.0;
            let slot = &mut authoring.groups[group_index].claim_threshold;
            let changed = *slot != value;
            *slot = value;
            Ok(changed)
        }
    }
}

/// 公共控件外再拒绝非有限和越界浮点，作者格式不能依赖UI正确性。
fn require_f32_range(
    value: NumericValue,
    minimum: f32,
    maximum: f32,
    label: &str,
) -> Result<f32, String> {
    let value = value
        .as_f32()
        .ok_or_else(|| format!("{label}控件返回了非浮点数值"))?;
    if !value.is_finite() || !(minimum..=maximum).contains(&value) {
        return Err(format!("{label}必须在{minimum}至{maximum}之间"));
    }
    Ok(value)
}

/// 精确输入在局部副本完成修改和完整校验后一次原子采用。
pub(super) fn apply_exact_value(
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    target: &VegetationNumericTarget,
    value: NumericValue,
) -> Result<bool, String> {
    if !target_is_current(target, editor, session) {
        return Err("植被精确输入期间地图、图层、分组或作者修订已经变化".to_owned());
    }
    let (outputs, before, before_densities) = authoring_snapshot(session, &target.map_id)?;
    let mut after = before.clone(); // 参数描述很小，失败不会污染当前草稿。
    if !set_authoring_value(&mut after, target, value)? {
        return Ok(false); // 显式输入同值不制造撤销或后台编译。
    }
    after.validate(&outputs)?; // 正式格式负责全部层引用、组引用和数值规则。
    validate_density_table(&after, &before_densities)?; // 参数修改不能掩盖缺失作者R8。
    adopt_authoring(session, &target.map_id, after.clone())?; // 完整受检值一次替换。
    let after_densities = before_densities.clone(); // 参数事务不改任何像素Arc。
    session
        .undo
        .push(EditorCommand::VegetationAuthoringChanged(Box::new(
            VegetationAuthoringChange {
                before_outputs: outputs.clone(), // 纯参数修改不改变正式输出，但命令仍保存完整原子边界。
                after_outputs: outputs,
                before,
                after,
                before_densities,
                after_densities,
            },
        )));
    session.redo.clear(); // 新参数修改终止旧重做分支。
    session.mark_vegetation_authoring_changed(editor); // 精确输入只启动一次正式编译。
    Ok(true)
}

/// 左拖第一次真实Changed冻结起点，但暂不推进作者修订或dirty门。
pub(super) fn begin_scrub(
    entity: Entity,
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    selected: SelectedVegetationNumeric,
) -> Result<VegetationNumericScrub, String> {
    if !selected.editable {
        return Err("当前植被参数因图层锁定或组规则不匹配而不可编辑".to_owned());
    }
    if !target_is_current(&selected.target, editor, session) {
        return Err("植被拖动开始前作者目标已经换代".to_owned());
    }
    let (_, before, before_densities) = authoring_snapshot(session, &selected.target.map_id)?;
    Ok(VegetationNumericScrub {
        entity,
        target: selected.target,
        before,
        before_densities,
        before_value: selected.value,
        latest_value: selected.value,
    })
}

/// 拖动途中只写一个小型JSON标量；列表文字可实时回显，整图编译留到松手。
pub(super) fn apply_scrub_value(
    editor: &MapEditorState,
    session: &mut EditorAuthoringSession,
    transaction: &mut VegetationNumericScrub,
    value: NumericValue,
) -> Result<bool, String> {
    if !target_is_current(&transaction.target, editor, session) {
        return Err("植被拖动期间地图、选择或作者修订已经变化".to_owned());
    }
    let draft = session
        .draft
        .as_mut()
        .ok_or_else(|| "植被拖动期间作者草稿已经卸载".to_owned())?;
    let authoring = draft
        .vegetation
        .as_mut()
        .and_then(|vegetation| vegetation.authoring.as_mut())
        .ok_or_else(|| "植被拖动期间作者图层已经失效".to_owned())?;
    let changed = set_authoring_value(authoring, &transaction.target, value)?;
    if changed {
        transaction.latest_value = value; // 只保留最终标量，拖中不克隆完整终点。
    }
    Ok(changed)
}

/// 松手时完整校验并形成唯一撤销命令；拖回起点保持地图原dirty状态。
pub(super) fn finish_scrub(
    editor: &mut MapEditorState,
    session: &mut EditorAuthoringSession,
    transaction: VegetationNumericScrub,
) -> Result<bool, String> {
    if !target_is_current(&transaction.target, editor, session) {
        rollback_scrub(session, &transaction); // 只恢复仍由本事务拥有的字段，不覆盖外部结构变化。
        return Err("植被拖动结束时作者目标已经换代，已恢复可安全识别的拖动值".to_owned());
    }
    let (outputs, after, after_densities) =
        authoring_snapshot(session, &transaction.target.map_id)?;
    if after == transaction.before {
        return Ok(false); // 拖回原值没有地图修改、撤销或后台编译。
    }
    if let Err(error) = after
        .validate(&outputs)
        .and_then(|_| validate_density_table(&after, &after_densities))
    {
        adopt_authoring(
            session,
            &transaction.target.map_id,
            transaction.before.clone(),
        )?;
        return Err(format!("植被参数校验失败并已恢复拖动前数值：{error}"));
    }
    session
        .undo
        .push(EditorCommand::VegetationAuthoringChanged(Box::new(
            VegetationAuthoringChange {
                before_outputs: outputs.clone(), // 连续拖动只改变作者标量，正式输出集合逐字保持。
                after_outputs: outputs,
                before: transaction.before,
                after,
                before_densities: transaction.before_densities,
                after_densities,
            },
        )));
    session.redo.clear(); // 一次连续拖动只结束一次重做分支。
    session.mark_vegetation_authoring_changed(editor); // 松手后才启动一次整图single-flight编译。
    Ok(true)
}

/// 只在当前字段仍等于本事务末值时，按稳定层/组身份恢复完整起点中的作者raw标量。
fn restore_owned_scrub_value(
    authoring: &mut MapVegetationAuthoring, // 当前草稿；同层或同组的其他后来字段必须保持不变。
    transaction: &VegetationNumericScrub,   // 完整before提供不经过百分比往返的raw权威值。
) -> Result<bool, String> {
    let before_value = value_from_authoring(&transaction.before, &transaction.target)
        .ok_or_else(|| "植被拖动起点中的稳定字段已经损坏".to_owned())?; // 冻结快照必须仍能证明raw来源。
    if before_value != transaction.before_value {
        return Err("植被拖动起点标量与完整作者快照不一致".to_owned()); // 损坏事务不得猜测恢复值。
    }
    if value_from_authoring(authoring, &transaction.target) != Some(transaction.latest_value) {
        return Ok(false); // 同字段已被后来作者结果接管时绝不覆盖。
    }
    let before_layer_index = transaction
        .before
        .layers
        .iter()
        .position(|layer| {
            layer.id == transaction.target.layer_id && layer.group_id == transaction.target.group_id
        })
        .ok_or_else(|| "植被拖动起点缺少稳定作者图层".to_owned())?; // raw层字段只能从冻结稳定层读取。
    let current_layer_index = authoring
        .layers
        .iter()
        .position(|layer| {
            layer.id == transaction.target.layer_id && layer.group_id == transaction.target.group_id
        })
        .ok_or_else(|| "当前植被草稿缺少拖动绑定的稳定作者图层".to_owned())?; // 不按选择索引串写重排后的其他层。
    let before_group_index = transaction
        .before
        .groups
        .iter()
        .position(|group| group.id == transaction.target.group_id)
        .ok_or_else(|| "植被拖动起点缺少稳定作者组".to_owned())?; // raw组字段只从冻结稳定组读取。
    let current_group_index = authoring
        .groups
        .iter()
        .position(|group| group.id == transaction.target.group_id)
        .ok_or_else(|| "当前植被草稿缺少拖动绑定的稳定作者组".to_owned())?; // 组重排后仍按ID恢复。
    match transaction.target.kind {
        VegetationNumericKind::Priority => {
            authoring.layers[current_layer_index].priority =
                transaction.before.layers[before_layer_index].priority; // I32虽无隐藏尾数，也保持统一owned-field语义。
        }
        VegetationNumericKind::Weight => {
            authoring.layers[current_layer_index].weight =
                transaction.before.layers[before_layer_index].weight; // 直接复制raw f32，避免两位显示精度量化。
        }
        VegetationNumericKind::OpacityPercent => {
            authoring.layers[current_layer_index].opacity =
                transaction.before.layers[before_layer_index].opacity; // 不经乘除100恢复作者归一化raw值。
        }
        VegetationNumericKind::GroupCapacityPercent => {
            authoring.groups[current_group_index].capacity =
                transaction.before.groups[before_group_index].capacity; // 组容量raw只改稳定组自身字段。
        }
        VegetationNumericKind::ClaimThresholdPercent => {
            authoring.groups[current_group_index].claim_threshold =
                transaction.before.groups[before_group_index].claim_threshold; // 覆盖阈值同样位级恢复raw。
        }
    }
    Ok(true) // 当前字段仍归本事务所有，调用方可安全跳过撤销、dirty和编译。
}

/// 共享 UI 控件报告拖回显示起点时，只恢复隐藏raw精度，不形成地图作者提交。
pub(super) fn restore_scrub_raw_origin(
    session: &mut EditorAuthoringSession, // 当前草稿是唯一可写作者权威。
    transaction: &VegetationNumericScrub, // 首个Changed冻结的稳定身份和raw完整起点。
) -> Result<bool, String> {
    let Some(draft) = session
        .draft
        .as_mut()
        .filter(|draft| draft.map_id == transaction.target.map_id)
    else {
        return Ok(false); // 切图或卸载后不把旧字段重新插入当前草稿。
    };
    let Some(authoring) = draft
        .vegetation
        .as_mut()
        .and_then(|vegetation| vegetation.authoring.as_mut())
    else {
        return Ok(false); // 外部删除作者域后保持其权威结果。
    };
    restore_owned_scrub_value(authoring, transaction) // 不调用mark_changed或正式编译入口。
}

/// 异常中止时优先完整恢复同修订草稿；存在外部修订时只回滚仍等于本事务末值的字段。
pub(super) fn rollback_scrub(
    session: &mut EditorAuthoringSession,
    transaction: &VegetationNumericScrub,
) {
    let revision_matches = session.revision == transaction.target.authoring_revision; // 可变借用草稿前冻结冲突判定。
    let Some(draft) = session
        .draft
        .as_mut()
        .filter(|draft| draft.map_id == transaction.target.map_id)
    else {
        return; // 地图已经卸载时旧草稿不存在，也没有值可回滚。
    };
    let Some(authoring) = draft
        .vegetation
        .as_mut()
        .and_then(|vegetation| vegetation.authoring.as_mut())
    else {
        return; // 外部结构删除作者域后不得重新插回旧快照。
    };
    if revision_matches {
        *authoring = transaction.before.clone(); // 无外部作者变化时恢复完整起点最安全。
        draft.vegetation_author_densities = transaction.before_densities.clone(); // Arc表原样恢复。
        return;
    }
    let _ = restore_owned_scrub_value(authoring, transaction); // 冲突分支同样按owned-field复制精确raw且保留其他外部字段。
}

/// 读取小型作者描述、正式输出引用和Arc表；调用方决定局部修改或事务收口。
fn authoring_snapshot(
    session: &EditorAuthoringSession,
    map_id: &MapId,
) -> Result<
    (
        Vec<feathrise_map_content::MapVegetationLayer>,
        MapVegetationAuthoring,
        BTreeMap<String, Arc<Vec<u8>>>,
    ),
    String,
> {
    let draft = session
        .draft
        .as_ref()
        .filter(|draft| &draft.map_id == map_id)
        .ok_or_else(|| "植被参数绑定的地图草稿已经失效".to_owned())?;
    let vegetation = draft
        .vegetation
        .as_ref()
        .ok_or_else(|| "当前地图没有正式植被数据".to_owned())?;
    let authoring = vegetation
        .authoring
        .as_ref()
        .ok_or_else(|| "当前地图没有植被作者图层".to_owned())?;
    Ok((
        vegetation.layers.clone(), // 小型正式输出描述只用于引用校验。
        authoring.clone(),         // 小型JSON参数快照，不复制像素。
        draft.vegetation_author_densities.clone(), // Arc浅复制保持O(图层数)。
    ))
}

/// 把完整受检作者描述放回同一地图草稿。
fn adopt_authoring(
    session: &mut EditorAuthoringSession,
    map_id: &MapId,
    authoring: MapVegetationAuthoring,
) -> Result<(), String> {
    let draft = session
        .draft
        .as_mut()
        .filter(|draft| &draft.map_id == map_id)
        .ok_or_else(|| "植被参数采用时地图草稿已经失效".to_owned())?;
    let vegetation = draft
        .vegetation
        .as_mut()
        .ok_or_else(|| "植被参数采用时正式植被描述已经失效".to_owned())?;
    vegetation.authoring = Some(authoring); // 完整前后值一次替换，不留下部分组参数。
    Ok(())
}

/// 目标中断时由系统取得并清空唯一左拖资源。
pub(super) fn take_scrub(
    state: &mut VegetationNumericInputState,
) -> Option<VegetationNumericScrub> {
    state.scrub.take() // 封装私有字段，系统只通过明确事务函数转移所有权。
}

/// 系统第一次Changed建立唯一左拖事务。
pub(super) fn set_scrub(
    state: &mut VegetationNumericInputState,
    transaction: VegetationNumericScrub,
) {
    state.scrub = Some(transaction); // 共享 UI 控件全局捕获保证同一时间至多一个根。
}

/// 系统以可变借用应用当前Changed，不暴露资源内部结构。
pub(super) fn scrub_mut(
    state: &mut VegetationNumericInputState,
) -> Option<&mut VegetationNumericScrub> {
    state.scrub.as_mut()
}

/// 返回当前事务根和字段，用于拒绝跨根迟到消息。
pub(super) fn scrub_identity(
    state: &VegetationNumericInputState,
) -> Option<(Entity, VegetationNumericKind)> {
    state
        .scrub
        .as_ref()
        .map(|scrub| (scrub.entity, scrub.target.kind))
}

/// 精确输入系统原地更新或清除冻结焦点。
pub(super) fn set_focus(
    state: &mut VegetationNumericInputState,
    focus: Option<VegetationNumericFocus>,
) {
    state.focus = focus; // 只允许当前公共ActiveNumericInput对应一个业务目标。
}

/// 提交时取得焦点所有权，防止同一提交重复采用。
pub(super) fn take_focus(
    state: &mut VegetationNumericInputState,
) -> Option<VegetationNumericFocus> {
    state.focus.take() // 任何Accepted/Canceled/Rejected都会结束本次业务绑定。
}

/// Presentation/输入阶段只读当前焦点身份。
pub(super) fn focus(state: &VegetationNumericInputState) -> Option<&VegetationNumericFocus> {
    state.focus.as_ref() // 不泄露可写字段，切换必须通过set/take。
}

#[cfg(test)]
#[path = "transaction_tests.rs"]
mod tests; // 纯作者值转换测试独立文件，事务实现保持在500行以内。
