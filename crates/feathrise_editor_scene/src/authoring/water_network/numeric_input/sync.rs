//! 水网控制点四个公共数值框的显示值、禁用态和灵敏度同步。

use bevy::prelude::*; // Commands、Query和资源变更读取。
use feathrise_editor_core::EditorSettings; // 下一次左右拖动读取已持久倍率。
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputDisabled, NumericInputScrubConfig,
    NumericInputSetValue, NumericInputValue, NumericValue,
}; // 公共模型桥避免直接篡改字符缓冲。

use super::{
    WaterNetworkNumericInput, WaterNetworkNumericInputState, WaterNetworkNumericKind,
    rollback_water_network_scrub_if_owned, selected_target, target_height_is_derived, target_value,
}; // 当前选择派生四个字段的权威值和连接禁用规则。
use crate::authoring::resources::EditorAuthoringSession; // 当前水网草稿和保存门。
use crate::resources::SceneEditorState; // F12显隐、模态与控制点选择。

/// 同步选择、撤销、重做、连接变化和设置恢复；活动输入期间绝不覆盖公共缓冲。
#[allow(clippy::too_many_arguments)] // 公共模型、禁用marker、倍率和业务冻结状态需在同一Presentation快照下同步。
pub(crate) fn sync_water_network_numeric_inputs(
    mut commands: Commands,                           // 增删公共禁用marker。
    editor: Res<SceneEditorState>,                      // 当前工具和选择。
    mut session: ResMut<EditorAuthoringSession>, // 当前草稿；异常隐藏时可定点回滚未入历史字段。
    settings: Res<EditorSettings>,                 // 四个字段各自的持久倍率。
    active: Res<ActiveNumericInput>,             // 中键字符缓冲期间不回写值。
    active_scrub: Res<ActiveNumericInputScrub>,  // 左右拖动捕获期间不回写值或倍率。
    mut state: ResMut<WaterNetworkNumericInputState>, // F12关闭后释放冻结身份。
    mut inputs: Query<(
        Entity,
        &WaterNetworkNumericInput,
        &NumericInputValue,
        Has<NumericInputDisabled>,
        &mut NumericInputScrubConfig,
    )>, // 四个固定根的小型查询。
    mut set_values: MessageWriter<NumericInputSetValue>, // 外部作者值经公共消息桥回写。
) {
    if state.scrub.as_ref().is_some_and(|transaction| {
        transaction.aborted && active_scrub.entity != Some(transaction.entity)
    }) {
        state.scrub = None; // 取消锁存到对应指针松手即释放，防止丢失Committed后永久阻塞。
    }
    if !editor.open && active.entity.is_none() && active_scrub.entity.is_none() {
        if let Some(transaction) = state.scrub.as_ref() {
            rollback_water_network_scrub_if_owned(&mut session, transaction); // 丢失松手消息的关闭也不遗留无历史水网值。
        }
        state.clear(); // 隐藏稳定帧不保留跨会话控制点身份。
    }
    let target = selected_target(&editor, &session); // 一次派生供四个字段共享同一稳定点身份。
    let water = session
        .draft
        .as_ref()
        .and_then(|draft| draft.water.as_ref()); // 当前小型水域只读权威。
    let context_enabled =
        editor.open && !editor.close_requested && !editor.new_scene_dialog_open && !session.saving; // 隐藏、关闭等待、模态和保存期间全部字段禁用。

    for (entity, marker, current, disabled, mut scrub) in &mut inputs {
        let kind = marker.0; // 当前根唯一字段身份。
        let value = target
            .as_ref()
            .zip(water)
            .and_then(|(target, water)| target_value(water, target, kind)); // 稳定ID读取实际作者值。
        let height_derived = kind == WaterNetworkNumericKind::Height
            && target
                .as_ref()
                .zip(water)
                .is_some_and(|(target, water)| target_height_is_derived(water, target)); // 连接拥有Y时只禁用高度。
        let should_enable = context_enabled && value.is_some() && !height_derived; // 其余三个字段仍可编辑。
        if should_enable && disabled {
            commands.entity(entity).remove::<NumericInputDisabled>(); // 恢复中键和左右拖动。
        } else if !should_enable && !disabled {
            commands.entity(entity).insert(NumericInputDisabled); // 无目标或连接派生高度拒绝全部输入。
        }
        let desired = value.unwrap_or(0.0); // 无目标根使用禁用占位零。
        if active.entity != Some(entity)
            && active_scrub.entity != Some(entity)
            && current.0.as_f32() != Some(desired)
        {
            set_values.write(NumericInputSetValue {
                entity,                            // 只同步当前字段根。
                value: NumericValue::F32(desired), // 使用当前草稿权威值。
                cancel_edit: true,                 // 外部作者变化取消旧缓冲。
            });
        }
        if active_scrub.entity != Some(entity) {
            let sensitivity = kind.sensitivity(&settings); // 设置加载、重置或外部修改只影响下一次拖动。
            if scrub.sensitivity != sensitivity {
                scrub.sensitivity = sensitivity; // 不在右拖途中覆盖即时倍率。
            }
        }
    }
}
