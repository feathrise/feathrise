//! 真实原动画输入与公共释放门回归；连续动作等待静默，不通过改Session伪造选择。

use super::super::{control, editor_tests};
use super::*;
use crate::resources::MapEditorNumericScrubMouseGuard;
use crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard;

#[test]
fn next_key_waits_for_the_original_channel_button_release_gate_before_a_fresh_pulse() {
    let (mut app, _, _) = editor_tests::app_context();
    app.init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .add_systems(
            Update,
            (control::input, update_map_editor_numeric_scrub_mouse_guard).chain(),
        );
    let binding = app.world().resource::<AnimationEditorState>().revision;
    let channel = app
        .world_mut()
        .spawn((
            AnimationChannelButton {
                slot: 0,
                binding,
                index: Some(0),
            },
            EditorButtonClick::triggered(),
        ))
        .id();
    app.update();
    assert!(
        app.world()
            .resource::<AnimationEditorState>()
            .current()
            .unwrap()
            .key
            .is_none()
    );
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    assert!(
        !input_ready(app.world()),
        "新动作必须等待原上一点击的释放门"
    );
    app.world_mut().despawn(channel);

    // 真实复现旧夹具：紧接着投递会被原门消费，解除阻断也不会重放这笔Changed。
    let binding = app.world().resource::<AnimationEditorState>().revision;
    let too_early = app
        .world_mut()
        .spawn((
            AnimationButton {
                action: AnimationAction::NextKey,
                binding,
            },
            EditorButtonClick::triggered(),
        ))
        .id();
    app.update();
    assert!(
        app.world()
            .resource::<AnimationEditorState>()
            .current()
            .unwrap()
            .key
            .is_none()
    );
    assert!(input_ready(app.world()));
    app.world_mut().despawn(too_early);

    let binding = app.world().resource::<AnimationEditorState>().revision;
    app.world_mut().spawn((
        AnimationButton {
            action: AnimationAction::NextKey,
            binding,
        },
        EditorButtonClick::triggered(),
    ));
    app.update();
    let session = app
        .world()
        .resource::<AnimationEditorState>()
        .current()
        .unwrap();
    assert_eq!(session.channel, Some(0));
    assert_eq!(session.key, Some(0));
    assert_eq!(
        session.position,
        session.document.clips[0].channels[0].keys[0].time
    );
    assert_eq!(session.cursor, 0, "定位关键帧不产生动画编辑历史");
}
