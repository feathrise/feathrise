//! 天空盒环境四个公共数值框的作者值、禁用态和倍率同步。

use bevy::prelude::*; // Commands、Query和资源读取。
use feathrise_editor_core::EditorSettings; // 下一次左右拖动读取持久倍率。
use feathrise_editor_ui::{
    ActiveNumericInput, ActiveNumericInputScrub, NumericInputDisabled, NumericInputScrubConfig,
    NumericInputSetValue, NumericInputValue, NumericValue,
}; // 公共模型桥避免直接覆盖字符缓冲。

use super::{
    EnvironmentNumericInput, EnvironmentNumericInputState, authored_target,
    rollback_environment_scrub_if_owned,
}; // 字段身份决定场景作者值与可用性。
use crate::authoring::resources::EditorAuthoringSession; // 当前环境草稿和保存门。
use crate::resources::SceneEditorState; // F12 作者上下文。

/// 选择、撤销、重做、切换场景和设置恢复后同步四个公共根。
#[allow(clippy::too_many_arguments)] // 公共值、禁用marker、倍率和冻结状态必须读取同一Presentation快照。
pub(crate) fn sync_environment_numeric_inputs(
    mut commands: Commands,                          // 增删公共禁用marker。
    editor: Res<SceneEditorState>,                     // F12上下文决定天空作者字段可用性。
    mut session: ResMut<EditorAuthoringSession>,     // 当前草稿；异常隐藏只回滚本天空字段。
    settings: Res<EditorSettings>,                     // 四个字段各自持久倍率。
    active: Res<ActiveNumericInput>,                 // 中键字符缓冲期间不回写值。
    active_scrub: Res<ActiveNumericInputScrub>,      // 左右拖动期间不回写值或倍率。
    mut state: ResMut<EnvironmentNumericInputState>, // 隐藏稳定帧释放旧身份。
    mut inputs: Query<(
        Entity,
        &EnvironmentNumericInput,
        &NumericInputValue,
        Has<NumericInputDisabled>,
        &mut NumericInputScrubConfig,
    )>, // 四个固定根的小型查询。
    mut set_values: MessageWriter<NumericInputSetValue>, // 外部权威值经公共消息桥同步。
) {
    if state.scrub.as_ref().is_some_and(|transaction| {
        transaction.aborted && active_scrub.entity != Some(transaction.entity)
    }) {
        state.scrub = None; // 冲突锁存在对应指针松手后释放，丢失Committed也不影响下次输入。
    }
    if !editor.open && active.entity.is_none() && active_scrub.entity.is_none() {
        if let Some(transaction) = state.scrub.as_ref() {
            rollback_environment_scrub_if_owned(&mut session, transaction); // 丢失Committed时也不遗留无历史实时值。
        }
        *state = EnvironmentNumericInputState::default(); // 不保留跨F12会话环境身份。
    }
    let environment = session
        .draft
        .as_ref()
        .map(|draft| &draft.manifest.environment); // 三根共享一次当前完整环境借用。
    let authored_enabled = authored_target(&session).is_some()
        && editor.open
        && !editor.close_requested
        && !editor.new_scene_dialog_open
        && !session.saving; // 持久环境只允许动态场景正常作者上下文。
    for (entity, marker, current, disabled, mut scrub) in &mut inputs {
        let kind = marker.0; // 当前根唯一字段身份。
        let value = kind.displayed_value(environment); // 场景环境原单位显示。
        let should_enable = authored_enabled && value.is_some(); // 固定场景与空草稿禁用天空作者值。
        if should_enable && disabled {
            commands.entity(entity).remove::<NumericInputDisabled>(); // 恢复中键和左右拖动。
        } else if !should_enable && !disabled {
            commands.entity(entity).insert(NumericInputDisabled); // 无有效作者目标拒绝输入。
        }
        let desired = value.unwrap_or(0.0); // 无环境草稿时禁用根显示安全占位。
        if active.entity != Some(entity)
            && active_scrub.entity != Some(entity)
            && current.0.as_f32() != Some(desired)
        {
            set_values.write(NumericInputSetValue {
                entity,                            // 只同步当前字段根。
                value: NumericValue::F32(desired), // 当前作者或预览权威值。
                cancel_edit: true,                 // 外部作者变化取消旧缓冲。
            });
        }
        if active_scrub.entity != Some(entity) {
            let sensitivity = kind.sensitivity(&settings); // 设置加载或外部重置只影响下一次拖动。
            if scrub.sensitivity != sensitivity {
                scrub.sensitivity = sensitivity; // 不在右拖途中覆盖即时倍率。
            }
        }
    }
}
