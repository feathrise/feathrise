//! 天空盒环境实时拖动的单字段所有权回滚门。

use bevy::prelude::{Entity, MessageWriter}; // 异常回显通过公共消息桥完成。
use feathrise_scene_content::SceneEnvironmentDescriptor; // 纯函数仅读写小型环境描述。
use feathrise_editor_ui::{NumericInputSetValue, NumericValue}; // 不直接篡改活动字符缓冲。

use super::{EnvironmentNumericKind, EnvironmentNumericScrub, EnvironmentNumericScrubStart}; // 事务区分持久天空与纯预览。
use crate::authoring::resources::EditorAuthoringSession; // ECS包装只定位当前草稿。

/// 从当前作者或F12预览读取UI显示值。
pub(super) fn current_value(
    kind: EnvironmentNumericKind,     // 目标环境或预览字段。
    session: &EditorAuthoringSession, // 提供当前场景环境。
) -> Option<f32> {
    let environment = session
        .draft
        .as_ref()
        .map(|draft| &draft.manifest.environment); // 持久字段读取当前完整清单环境。
    kind.displayed_value(environment)
}

/// 当前权威值回写公共模型并取消失效字符缓冲。
pub(super) fn reset_input(
    entity: Entity,                                       // 产生异常的公共根。
    kind: EnvironmentNumericKind,                         // 需恢复的逻辑字段。
    session: &EditorAuthoringSession,                     // 当前场景环境权威。
    set_values: &mut MessageWriter<NumericInputSetValue>, // 公共值模型消息桥。
) {
    let value = current_value(kind, session).unwrap_or(0.0); // 无环境草稿时禁用字段显示安全占位。
    set_values.write(NumericInputSetValue {
        entity,                          // 只恢复异常来源根。
        value: NumericValue::F32(value), // 使用当前作者或预览权威值。
        cancel_edit: true,               // 清除隐藏或过期字符缓冲。
    });
}

/// 读取持久环境字段的未归一化精确值。
pub(super) const fn authored_field_value(
    environment: &SceneEnvironmentDescriptor, // 当前或起点环境快照。
    kind: EnvironmentNumericKind,           // 四个持久天空字段可读。
) -> Option<f32> {
    match kind {
        EnvironmentNumericKind::SkyboxBrightness => Some(environment.skybox_brightness), // 亮度原值。
        EnvironmentNumericKind::EnvironmentDiffuseIntensity => {
            Some(environment.environment_diffuse_intensity)
        } // 漫反射补光原值。
        EnvironmentNumericKind::EnvironmentSpecularIntensity => {
            Some(environment.environment_specular_intensity)
        } // 镜面反射原值。
        EnvironmentNumericKind::SkyboxYaw => Some(environment.skybox_yaw_degrees), // 保留超周或负角原值。
    }
}

/// 只当持久字段仍等于事务latest时恢复精确before。
fn rollback_owned_authored_field(
    environment: &mut SceneEnvironmentDescriptor, // 当前权威环境。
    kind: EnvironmentNumericKind,               // 事务拥有的唯一字段。
    before: f32,                                // 拖动前的raw原值。
    latest: f32,                                // 事务最后写入的raw值。
) -> bool {
    if authored_field_value(environment, kind) != Some(latest) {
        return false; // 外部已改同字段时失去回滚权。
    }
    match kind {
        EnvironmentNumericKind::SkyboxBrightness => environment.skybox_brightness = before, // 只恢复亮度。
        EnvironmentNumericKind::EnvironmentDiffuseIntensity => {
            environment.environment_diffuse_intensity = before
        } // 只恢复PBR漫反射补光。
        EnvironmentNumericKind::EnvironmentSpecularIntensity => {
            environment.environment_specular_intensity = before
        } // 只恢复PBR镜面反射。
        EnvironmentNumericKind::SkyboxYaw => environment.skybox_yaw_degrees = before, // 只恢复原始方向。
    }
    true // 其他环境字段保持不变。
}

/// 异常收口只撤销当前仍由事务拥有的那一个标量。
pub(super) fn rollback_environment_scrub_if_owned(
    session: &mut EditorAuthoringSession,  // 当前草稿可能已换代。
    transaction: &EnvironmentNumericScrub, // 冻结before/latest和字段身份。
) -> bool {
    match &transaction.start {
        EnvironmentNumericScrubStart::Authored {
            target,
            before_value,
            ..
        } => {
            let Some(environment) = session
                .draft
                .as_mut()
                .filter(|draft| draft.scene_id == target.scene_id)
                .map(|draft| &mut draft.manifest.environment)
            else {
                return false; // 切换场景后不写新场景。
            };
            rollback_owned_authored_field(
                environment,              // 当前同场景环境。
                transaction.kind,         // 只撤销本数值框字段。
                *before_value,            // 恢复raw原值。
                transaction.latest_value, // latest仍在才写入。
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 纯回滚门不启动天空渲染。

    #[test]
    fn rollback_requires_environment_field_to_still_equal_owned_latest() {
        let mut environment = SceneEnvironmentDescriptor::default(); // 合法默认环境。
        environment.skybox_brightness = 1000.0; // 模拟拖回整数显示起点的latest。
        assert!(rollback_owned_authored_field(
            &mut environment,
            EnvironmentNumericKind::SkyboxBrightness,
            1000.125,
            1000.0
        )); // changed=false时恢复高精度raw。
        assert_eq!(environment.skybox_brightness, 1000.125); // 高精度before不被显示步长量化。
        environment.skybox_brightness = 2000.0; // 外部已改同字段。
        assert!(!rollback_owned_authored_field(
            &mut environment,
            EnvironmentNumericKind::SkyboxBrightness,
            1000.125,
            1000.0
        )); // 旧事务失去所有权。
        assert_eq!(environment.skybox_brightness, 2000.0); // 外部值保留。
    }

    #[test]
    fn diffuse_and_specular_intensities_change_without_rewriting_each_other_or_background() {
        let mut environment = SceneEnvironmentDescriptor::default(); // 默认背景1000、补光300、反射1000。
        assert!(
            EnvironmentNumericKind::EnvironmentDiffuseIntensity
                .set_environment_value(&mut environment, 220.0)
                .expect("有限非负补光强度应可写入")
        ); // 模拟F12继续降低人物天空染色。
        assert_eq!(environment.environment_diffuse_intensity, 220.0); // PBR补光采用用户值。
        assert_eq!(environment.environment_specular_intensity, 1_000.0); // 水面反射不得被补光联动改写。
        assert_eq!(environment.skybox_brightness, 1_000.0); // 天空盒独立亮度不得被IBL补光联动改写。
        assert!(
            EnvironmentNumericKind::EnvironmentSpecularIntensity
                .set_environment_value(&mut environment, 1_250.0)
                .expect("有限非负反射强度应可写入")
        ); // 模拟F12单独增强斜视水面天空反射。
        assert_eq!(environment.environment_diffuse_intensity, 220.0); // 反射调整不得反向抬高人物补光。
        assert_eq!(environment.environment_specular_intensity, 1_250.0); // 镜面通道采用用户值。
        assert!(rollback_owned_authored_field(
            &mut environment,                                    // 回滚当前完整环境。
            EnvironmentNumericKind::EnvironmentDiffuseIntensity, // 事务只拥有补光字段。
            300.0,                                               // 拖动前精确作者值。
            220.0,                                               // 当前仍等于事务最后写值。
        )); // 失焦或隐藏时只恢复补光字段。
        assert_eq!(environment.environment_diffuse_intensity, 300.0); // 补光字段恢复起点。
        assert_eq!(environment.environment_specular_intensity, 1_250.0); // 反射字段保留独立修改。
        assert_eq!(environment.skybox_brightness, 1_000.0); // 背景字段全过程保持不变。
    }
}
