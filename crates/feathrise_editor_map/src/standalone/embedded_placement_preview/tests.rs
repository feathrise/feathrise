//! 原手柄、数值Changed/Committed和多选输入驱动实时消息；不伪造手势或作者历史。

use super::*;
use crate::{
    authoring::EditorAuthoringSession,
    viewport::placement_move::{MoveAxis, interaction_tests as original},
};
use feathrise_map_content::embedded_placement_preview::{
    PlacementPreviewCommand as Command, PlacementPreviewKind,
};
use std::time::{Duration, Instant};

fn enable(app: &mut App) {
    install(app);
    let mut state = app.world_mut().resource_mut::<EmbeddedPlacementPreview>();
    state.enabled = true;
    state.session = 1;
    state.generation = 1;
}

fn point(app: &App, origin: Vec3, delta: f32) -> Vec2 {
    let camera = original::camera(app);
    camera
        .project(origin + Vec3::X * (camera.handle_length(origin).unwrap() * 0.65 + delta))
        .unwrap()
}

fn wait_command(app: &mut App, accept: impl Fn(&Command) -> bool) -> Command {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        app.update();
        if let Some(ready) = peek(app.world()) {
            let command: Command = serde_json::from_slice(&ready.bytes).unwrap();
            assert!(!acknowledge(
                app.world_mut(),
                ready.sequence.wrapping_add(1)
            ));
            assert!(acknowledge(app.world_mut(), ready.sequence));
            if accept(&command) {
                return command;
            }
        }
        assert!(
            Instant::now() < deadline,
            "实时命令未产生：{:?}",
            app.world().resource::<EmbeddedPlacementPreview>().error
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn embedded_placement_preview_original_drag_updates_before_release_and_commits_once() {
    let (mut app, root, node) = original::app();
    enable(&mut app);
    let origin = Vec3::from_array(original::record().translation);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    let start = point(&app, origin, 0.0);
    original::step(&mut app, node, start);
    assert!(
        matches!(wait_command(&mut app, |command| matches!(command, Command::Begin { .. })),
        Command::Begin { ids, kind: PlacementPreviewKind::Absolute, .. } if ids == vec!["move-test"])
    );
    for delta in [0.5, 1.0, 1.37] {
        let cursor = point(&app, origin, delta);
        original::step(&mut app, node, cursor);
    }
    let preview = wait_command(
        &mut app,
        |command| matches!(command, Command::Update { transform, .. } if (transform[12] - 6.37).abs() < 0.002),
    );
    assert!(matches!(preview, Command::Update { .. }));
    assert!((app.world().get::<Transform>(root).unwrap().translation.x - 6.37).abs() < 0.002);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements[0].translation[0],
        5.0
    );
    assert!(session.undo.is_empty());
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    let cursor = point(&app, origin, 1.37);
    original::step(&mut app, node, cursor);
    let end = wait_command(&mut app, |command| matches!(command, Command::End { .. }));
    assert!(
        matches!(end, Command::End { cancelled: false, transform: Some(matrix), .. } if (matrix[12] - 6.37).abs() < 0.002)
    );
    match &end {
        Command::End { final_members, .. } => {
            assert_eq!(final_members.len(), 1);
            assert_eq!(final_members[0].id, "move-test");
            assert!((final_members[0].local[12] - 6.37).abs() < 0.002);
        }
        _ => unreachable!(),
    }
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn embedded_placement_preview_numeric_cancel_restores_current_author_without_history() {
    use feathrise_editor_ui::{
        NumericInputChanged, NumericInputCommitOutcome, NumericInputCommitSource,
        NumericInputCommitted, NumericInputScrubSensitivityChanged,
        NumericInputScrubSensitivityCommitted, NumericInputSetValue, NumericValue,
    };
    let (mut app, _, _) = original::app();
    app.init_resource::<feathrise_editor_core::EditorSettings>()
        .add_message::<NumericInputChanged>()
        .add_message::<NumericInputCommitted>()
        .add_message::<NumericInputScrubSensitivityChanged>()
        .add_message::<NumericInputScrubSensitivityCommitted>()
        .add_message::<NumericInputSetValue>()
        .add_systems(
            Update,
            crate::ui::placement_transform::handle_placement_transform_numeric_input,
        );
    let input = app
        .world_mut()
        .spawn(crate::ui::placement_transform::PlacementTransformField::TranslationX)
        .id();
    enable(&mut app);
    for value in [6.0, 7.0] {
        app.world_mut().write_message(NumericInputChanged {
            entity: input,
            value: NumericValue::F32(value),
        });
        app.update();
    }
    wait_command(
        &mut app,
        |command| matches!(command, Command::Update { transform, .. } if transform[12] == 7.0),
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    let revision_before_cancel = app.world().resource::<EditorAuthoringSession>().revision;
    app.world_mut().write_message(NumericInputCommitted {
        entity: input,
        value: NumericValue::F32(7.0),
        outcome: NumericInputCommitOutcome::Canceled,
        source: NumericInputCommitSource::Scrub,
        changed: true,
        edited: true,
    });
    app.update();
    let end = wait_command(&mut app, |command| matches!(command, Command::End { .. }));
    assert!(
        matches!(end, Command::End { cancelled: true, transform: Some(matrix), .. } if matrix[12] == 5.0)
    );
    assert!(matches!(&end, Command::End { final_members, .. }
        if final_members.len() == 1 && final_members[0].local[12] == 5.0));
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(
        session.draft.as_ref().unwrap().placements.placements[0].translation[0],
        5.0
    );
    assert!(session.undo.is_empty());
    assert!(session.revision > revision_before_cancel, "取消必须撤销临时值的完整快照信用");
    assert!(!app.world().resource::<crate::resources::MapEditorState>().dirty, "取消恢复原干净状态");
}

#[test]
fn embedded_placement_preview_original_group_uses_one_delta_and_reliable_cancel() {
    let (mut app, _, node, _) = original::batch_preview_fixture();
    enable(&mut app);
    original::batch_preview_begin(&mut app, node, MoveAxis::X);
    let begin = wait_command(&mut app, |command| matches!(command, Command::Begin { .. }));
    assert!(
        matches!(begin, Command::Begin { ids, kind: PlacementPreviewKind::GroupDelta, .. }
        if ids == vec!["move-test", "move-second"])
    );
    let origin = app
        .world()
        .resource::<crate::viewport::multi_select::PlacementSelectionSet>()
        .center;
    let cursor = point(&app, origin, 1.25);
    original::step(&mut app, node, cursor);
    wait_command(
        &mut app,
        |command| matches!(command, Command::Update { transform, .. } if (transform[12] - 1.25).abs() < 0.002),
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Escape);
    original::step(&mut app, node, cursor);
    let end = wait_command(&mut app, |command| matches!(command, Command::End { .. }));
    assert!(
        matches!(end, Command::End { cancelled: true, transform: Some(matrix), .. } if matrix == Mat4::IDENTITY.to_cols_array())
    );
    assert!(matches!(&end, Command::End { final_members, .. }
        if final_members.len() == 2 && final_members[0].id == "move-test" && final_members[1].id == "move-second"));
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
    // 不安装完整快照插件也能紧接下一笔；前一笔可靠End携带全部真实局部姿态即可归还临时父级。
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Escape);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    for _ in 0..4 {
        original::step(&mut app, node, cursor);
    }
    original::batch_preview_begin(&mut app, node, MoveAxis::X);
    wait_command(&mut app, |command| matches!(command, Command::Begin { .. }));
    let second_cursor = point(&app, origin, 0.5);
    original::step(&mut app, node, second_cursor);
    wait_command(&mut app, |command| {
        matches!(command, Command::Update { transform, .. }
        if (transform[12] - 0.5).abs() < 0.002)
    });
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    original::step(&mut app, node, second_cursor);
    let second_end = wait_command(&mut app, |command| matches!(command, Command::End { .. }));
    assert!(
        matches!(second_end, Command::End { cancelled: false, final_members, .. }
        if final_members.len() == 2 && (final_members[0].local[12]-5.5).abs() < 0.002
        && (final_members[1].local[12]-7.5).abs() < 0.002)
    );
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().undo.len(),
        1
    );
}

#[test]
fn embedded_placement_preview_restart_replays_current_gesture_but_rejects_previous_ack() {
    let (mut app, _, node) = original::app();
    enable(&mut app);
    let origin = Vec3::from_array(original::record().translation);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    let start = point(&app, origin, 0.0);
    original::step(&mut app, node, start);
    wait_command(&mut app, |command| matches!(command, Command::Begin { .. }));
    let cursor = point(&app, origin, 1.0);
    original::step(&mut app, node, cursor);
    let deadline = Instant::now() + Duration::from_secs(3);
    let previous = loop {
        app.update();
        if let Some(ready) = peek(app.world()) {
            break ready;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    };
    app.world_mut()
        .resource_mut::<EmbeddedPlacementPreview>()
        .session = 2;
    assert!(!acknowledge(app.world_mut(), previous.sequence));
    wait_command(&mut app, |command| matches!(command, Command::Begin { .. }));
    wait_command(
        &mut app,
        |command| matches!(command, Command::Update { transform, .. } if (transform[12]-6.0).abs()<0.002),
    );
    assert!(
        app.world()
            .resource::<EditorAuthoringSession>()
            .undo
            .is_empty()
    );
}
