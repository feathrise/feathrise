//! 网格水两个数值字段的独立右拖灵敏度回显与持久化。

use bevy::prelude::*; // 按输入根反查字段并消费共享 UI 控件消息。
use feathrise_editor_core::EditorSettings; // 松开右键后只写对应字段倍率。
use feathrise_editor_ui::{
    NumericInputScrubSensitivityChanged, NumericInputScrubSensitivityCommitted,
}; // 途中值与最终值的公共协议。

use super::{PlacementWaterNumericInput, PlacementWaterNumericKind}; // 业务根决定流速或Y偏移的独立设置槽。
use crate::resources::MapEditorState; // 底栏在右拖时只回显倍率，数字框保持实际值。

fn input_kind(
    inputs: &Query<(Entity, &PlacementWaterNumericInput)>,
    entity: Entity,
) -> Option<PlacementWaterNumericKind> {
    inputs.get(entity).ok().map(|(_, marker)| marker.0)
}

/// 正式placement交接在途时消费迟到右拖消息，但不改作者值或设置。
pub(super) fn handle_sensitivity_messages(
    changed: &mut MessageReader<NumericInputScrubSensitivityChanged>,
    committed: &mut MessageReader<NumericInputScrubSensitivityCommitted>,
    inputs: &Query<(Entity, &PlacementWaterNumericInput)>,
    editor: &mut ResMut<MapEditorState>,
    settings: &mut ResMut<EditorSettings>,
    placement_handoff_in_flight: bool,
) {
    for event in changed.read() {
        let Some(kind) = input_kind(inputs, event.entity) else {
            continue; // 其他NumericInput由各自业务域处理。
        };
        if placement_handoff_in_flight {
            editor.status = "正式摆放原子交接完成前，网格水数值与灵敏度暂停编辑".to_owned();
            continue;
        }
        editor.status = format!("正在调整{}拖动灵敏度：×{:.3}", kind.label(), event.value);
    }
    for event in committed.read() {
        let Some(kind) = input_kind(inputs, event.entity) else {
            continue;
        };
        if placement_handoff_in_flight {
            editor.status = "正式摆放原子交接完成前，网格水数值与灵敏度暂停编辑".to_owned();
            continue;
        }
        let current = kind.sensitivity(settings); // 先只读比较；稳定帧和同值提交都不能把全局设置标脏。
        if current != event.after {
            kind.set_sensitivity(settings, event.after); // 只有右拖产生真实倍率变化时才取得EditorSettings可写访问。
        }
        let value = kind.sensitivity(settings);
        editor.status = format!("{}拖动灵敏度已设为 ×{value:.3}", kind.label());
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 使用正式消息处理边界验证Bevy资源变更检测。

    #[derive(Resource, Default)]
    struct ChangeSamples {
        settings: Vec<bool>, // 每帧记录统一设置是否被处理系统误标为已修改。
        editor: Vec<bool>,   // 同时锁定无输入时状态栏资源也保持稳定。
    }

    fn run_idle_sensitivity_handler(
        mut changed: MessageReader<NumericInputScrubSensitivityChanged>,
        mut committed: MessageReader<NumericInputScrubSensitivityCommitted>,
        inputs: Query<(Entity, &PlacementWaterNumericInput)>,
        mut editor: ResMut<MapEditorState>,
        mut settings: ResMut<EditorSettings>,
    ) {
        handle_sensitivity_messages(
            &mut changed,
            &mut committed,
            &inputs,
            &mut editor,
            &mut settings,
            false,
        ); // 复现游戏Update中每帧调用但没有任何右拖消息的稳定状态。
    }

    fn sample_resource_changes(
        settings: Res<EditorSettings>,
        editor: Res<MapEditorState>,
        mut samples: ResMut<ChangeSamples>,
    ) {
        samples.settings.push(settings.is_changed());
        samples.editor.push(editor.is_changed());
    }

    #[test]
    fn idle_sensitivity_handler_does_not_dirty_global_settings() {
        let mut app = App::new(); // 只启动消息与ECS调度，不创建窗口或渲染世界。
        app.init_resource::<EditorSettings>()
            .init_resource::<MapEditorState>()
            .init_resource::<ChangeSamples>()
            .add_message::<NumericInputScrubSensitivityChanged>()
            .add_message::<NumericInputScrubSensitivityCommitted>()
            .add_systems(
                Update,
                (run_idle_sensitivity_handler, sample_resource_changes).chain(),
            ); // 观察器固定在正式处理函数之后读取本帧变更标记。

        app.update(); // 首帧资源刚插入，按Bevy规则应报告Added/Changed。
        app.world_mut().clear_trackers(); // 清除首帧基线，随后模拟无输入的稳定游戏帧。
        app.update();

        let samples = app.world().resource::<ChangeSamples>();
        assert_eq!(samples.settings, vec![true, false]);
        assert_eq!(samples.editor, vec![true, false]);
    }
}
