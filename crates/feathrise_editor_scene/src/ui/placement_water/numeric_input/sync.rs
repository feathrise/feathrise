//! 网格水专业数值框的显示值、禁用态和持久灵敏度同步。

use bevy::prelude::*; // ECS查询、命令、资源和消息写入。
use feathrise_scene::ScenePlacementInstance; // orphaned拖动回滚后只重施当前活动或精确预览根。
use feathrise_editor_core::EditorSettings; // 外部设置加载后刷新两个字段各自的拖动倍率。
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputDisabled, NumericInputScrubConfig,
    NumericInputSetValue, NumericInputValue, NumericValue, round_to_decimals,
}; // 公共数值框继续拥有字符缓冲、指针捕获与权威显示同步。

use super::transaction::{
    input_context_valid, numeric_target_is_current, restore_numeric_scrub_raw_origin,
}; // Presentation兜底同样按稳定身份owned-field恢复，不能直接丢live事务。
use super::{PlacementWaterNumericInput, PlacementWaterNumericInputState}; // 同一业务域共享冻结状态。
use crate::authoring::EditorAuthoringSession; // 当前完整水绑定和值来自作者草稿。
use crate::authoring::WaterFileHandoff; // 保存候选原子采用前仅冻结两个网格水数值根。
use crate::resources::SceneEditorState; // F12生命周期与模态状态决定可编辑性。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 当前Scene0网格及失效绑定诊断。
use crate::ui::placement_water::{
    PlacementInstanceIndex, PlacementWaterSelectionCache, preview_placement_overrides,
}; // 三个属性消费者共享选择缓存；异常raw恢复按根索引重施当前覆盖。

/// 选择、撤销、设置恢复或控件换代后同步数值、禁用态和下一次拖动灵敏度。
pub(crate) fn sync_placement_water_numeric_inputs(
    mut commands: Commands,                      // 增删NumericInputDisabled标记。
    mut editor: ResMut<SceneEditorState>,          // F12生命周期与模态状态决定可编辑性。
    browser: Res<EditorGltfBrowserState>,        // 当前Scene0网格及失效绑定诊断。
    mut session: ResMut<EditorAuthoringSession>, // 当前完整水绑定和值来源；orphaned事务需定向恢复raw起点。
    selection_cache: Res<PlacementWaterSelectionCache>, // 稳定帧不再线性查找最多8192条placements。
    handoff: Res<WaterFileHandoff>,              // 正式placement交接期间对公共控件插入禁用标记。
    active: Res<ActiveNumericInput>,             // 字符缓冲期间作者同步不能覆盖显示。
    active_scrub: Res<ActiveNumericInputScrub>,  // 左右拖动捕获期间不能回写字段或倍率。
    root_index: Res<PlacementInstanceIndex>, // owned-field恢复按当前场景稳定placement ID定位少量根。
    placement_roots: Query<(Entity, &ScenePlacementInstance)>, // 重施恢复后的完整网格水覆盖，不扫描World。
    settings: Res<EditorSettings>, // 外部加载或恢复设置后同步下一次拖动手感。
    mut state: ResMut<PlacementWaterNumericInputState>, // 关闭F12时释放冻结作者身份。
    mut inputs: Query<(
        Entity,                       // 公共NumericInput业务根。
        &PlacementWaterNumericInput,  // 流速或高度字段身份。
        &NumericInputValue,           // 当前已规范化显示值。
        Has<NumericInputDisabled>,    // 当前公共禁用标记是否存在。
        &mut NumericInputScrubConfig, // 下一次左右拖动配置。
    )>,
    mut set_values: MessageWriter<NumericInputSetValue>, // 外部作者值通过公共桥同步并取消旧缓冲。
) {
    let placement_handoff_in_flight = session
        .draft
        .as_ref()
        .is_some_and(|draft| handoff.placement_handoff_in_flight_for(&draft.scene_id));
    let context_is_editable = input_context_valid(&editor, &session, placement_handoff_in_flight); // 保存、交接、关闭和模态弹框统一禁用。
    let scrub_needs_fallback = state.scrub.as_ref().is_some_and(|transaction| {
        !context_is_editable
            || session.revision != transaction.expected_revision
            || active_scrub.entity != Some(transaction.entity)
            || !inputs
                .get(transaction.entity)
                .ok()
                .is_some_and(|(_, marker, _, _, _)| marker.0 == transaction.kind)
            || !numeric_target_is_current(&browser, &session, &transaction.target)
    }); // UI根、场景、选择、修订或物理捕获任一换代都必须在清状态前恢复owned字段。
    if scrub_needs_fallback {
        let transaction = state
            .scrub
            .take()
            .expect("上方已确认存在orphaned网格水拖动"); // 先取得完整raw起点，绝不直接drop live事务。
        let captured_root_still_current = active_scrub.entity == Some(transaction.entity)
            && inputs
                .get(transaction.entity)
                .ok()
                .is_some_and(|(_, marker, _, _, _)| marker.0 == transaction.kind); // 无MouseMotion的外部换代仍可能发生在原左键按住期间。
        match restore_numeric_scrub_raw_origin(&mut editor, &mut session, &transaction) {
            Ok((owned, preview_record)) => {
                if let Some(record) = preview_record.as_ref() {
                    preview_placement_overrides(
                        &mut commands,
                        &session,
                        &root_index,
                        &placement_roots,
                        record,
                    ); // 当前字段仍归旧拖动所有时，运行时覆盖与草稿raw同帧恢复。
                }
                if editor.open && owned {
                    editor.status = format!(
                        "{}拖动目标已换代；未提交的实时值已恢复",
                        transaction.kind.label()
                    ); // Presentation兜底只说明回滚，不形成撤销或dirty。
                }
            }
            Err(error) => {
                editor.status = format!(
                    "{}拖动目标换代且raw恢复失败：{error}",
                    transaction.kind.label()
                ); // 损坏事务必须显式失败，仍释放旧稳定身份避免跨场景写入。
            }
        }
        if captured_root_still_current {
            state.abort_scrub_until_release(transaction.entity, transaction.kind); // Presentation已回滚但物理拖动仍活动，后续Changed必须吞到原松键。
        } else {
            state.aborted_scrub = None; // 根或捕获真正消失时旧Committed不会到达，直接释放拒绝门。
        }
    }
    if !editor.open {
        state.clear(); // owned-field兜底完成后才释放跨会话placement、焦点和拒绝门身份。
    } else if let Some((entity, kind)) = state.aborted_scrub {
        let aborted_root_is_current = active_scrub.entity == Some(entity)
            && inputs
                .get(entity)
                .ok()
                .is_some_and(|(_, marker, _, _, _)| marker.0 == kind); // 只有原物理捕获和业务根都仍存在时继续等对应松键。
        if !aborted_root_is_current {
            state.aborted_scrub = None; // 根换代后旧Committed不会到达，安全释放拒绝门。
        }
    }
    let surface = selection_cache.surface(); // 即使失效绑定也允许灰显旧作者数值供诊断。
    let selection_is_editable = browser
        .selection
        .as_ref()
        .is_some_and(|selection| selection.valid && !selection.orphaned_water_binding); // 失效或生成名选择只保留取消按钮。
    if placement_handoff_in_flight && !state.placement_handoff_frozen && editor.open {
        editor.status =
            "正式摆放正在原子交接；网格水流速与Y偏移已暂时禁用，其他编辑仍可继续".to_owned();
    }
    if state.placement_handoff_frozen != placement_handoff_in_flight {
        state.placement_handoff_frozen = placement_handoff_in_flight; // 只在边沿写Resource，稳定帧不制造虚假Changed。
    } // 完成边沿不覆盖poll系统已发布的正式交接结果。
    for (entity, marker, current, disabled, mut scrub) in &mut inputs {
        let kind = marker.0; // 当前根字段身份。
        let should_enable = context_is_editable && selection_is_editable && surface.is_some(); // 必须存在有效水绑定。
        if should_enable == disabled {
            if should_enable {
                commands.entity(entity).remove::<NumericInputDisabled>(); // 恢复中键精确输入和左右拖动。
            } else {
                commands.entity(entity).insert(NumericInputDisabled); // 普通或失效绑定拒绝全部数值编辑。
            }
        }
        if active_scrub.entity != Some(entity) {
            let sensitivity = kind.sensitivity(&settings); // 读取本字段独立持久倍率。
            if scrub.sensitivity != sensitivity {
                scrub.sensitivity = sensitivity; // 只更新下一次拖动，不干扰正在捕获的手势。
            }
        }
        if active.entity == Some(entity) || active_scrub.entity == Some(entity) {
            continue; // 共享 UI 控件拥有字符缓冲或指针捕获时不得覆盖当前显示。
        }
        let target = surface.map_or(0.0, |surface| {
            round_to_decimals(kind.surface_value(surface), kind.decimals()) // 显示值服从字段精度但不改作者高精度源。
        });
        if current.0.as_f32() != Some(target) {
            set_values.write(NumericInputSetValue {
                entity,                           // 只同步当前字段根。
                value: NumericValue::F32(target), // 选择、撤销、重做和草稿换代后的最终值。
                cancel_edit: true,                // 外部权威变化必须清空任何旧字符缓冲。
            });
        }
    }
}
