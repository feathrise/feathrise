//! 作者命令及真实系统回归：精确撤销三次切线、保存点、迟到任务与只读输入门。

use feathrise_editor_ui::UiInteraction;
use super::super::{
    AssetPreviewWorkspace, DocumentState,
    types::{AssetPreviewContent, AssetPreviewModelSession, AssetPreviewPhase, AssetPreviewState},
};
use super::{
    components::*,
    control,
    data::{AnimationDocument, Keyframe, tests::test_document},
    drive,
    resources::*,
};
use crate::{
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState},
    ui::{
        sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
};
use bevy::{input::mouse::MouseWheel, prelude::*, tasks::IoTaskPool};
use feathrise_editor_ui::{
    EditorButtonClick, NumericInputCommitOutcome, NumericInputCommitSource, NumericInputCommitted,
    NumericValue,
};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

fn changed_key(document: &AnimationDocument, value: f32) -> Edit {
    let before = document.clips[0].channels[0].keys[0].clone();
    let mut after = before.clone();
    after.value[0] = value;
    Edit::Key {
        clip: 0,
        channel: 0,
        before: Some(before),
        after: Some(after),
    }
}

fn cubic_document() -> AnimationDocument {
    let mut document = test_document();
    let channel = &mut document.clips[0].channels[0];
    channel
        .set_interpolation("CUBICSPLINE")
        .expect("设置三次插值");
    channel.keys = vec![
        Keyframe {
            time: 0.0,
            value: vec![0.0, 0.0, 0.0],
            in_tangent: vec![9.0, 8.0, 7.0],
            out_tangent: vec![1.0, 2.0, 3.0],
        },
        Keyframe {
            time: 1.0,
            value: vec![2.0, 4.0, 1.0],
            in_tangent: vec![-5.0, 2.0, 1.0],
            out_tangent: vec![4.0, -3.0, 2.0],
        },
        Keyframe {
            time: 2.0,
            value: vec![3.0, 2.0, 4.0],
            in_tangent: vec![1.0, 0.0, 2.0],
            out_tangent: vec![6.0, 5.0, 4.0],
        },
    ];
    document
}

#[test]
fn deleting_and_undoing_a_cubic_key_restores_exact_tangents_and_curve() {
    let mut document = cubic_document();
    let original = document.clips[0].channels[0].clone();
    let edit = Edit::Key {
        clip: 0,
        channel: 0,
        before: Some(original.keys[1].clone()),
        after: None,
    };
    edit.apply(&mut document, true).expect("删除中间三次帧");
    assert_eq!(document.clips[0].channels[0].keys.len(), 2);
    let selected = edit.apply(&mut document, false).expect("撤销删除");
    assert_eq!(selected, Some(1));
    assert_eq!(
        document.clips[0].channels[0], original,
        "恢复关键帧必须采用历史中的原切线"
    );
    for time in [0.25, 0.75, 1.25, 1.75] {
        assert_eq!(
            document.clips[0].channels[0].sample(time),
            original.sample(time)
        );
    }
}

#[test]
fn moving_a_cubic_key_across_neighbors_and_undo_keeps_the_whole_key() {
    let mut document = cubic_document();
    let original = document.clips[0].channels[0].clone();
    let before = original.keys[1].clone();
    let mut after = before.clone();
    after.time = 3.0;
    let edit = Edit::Key {
        clip: 0,
        channel: 0,
        before: Some(before),
        after: Some(after.clone()),
    };
    assert_eq!(edit.apply(&mut document, true).expect("移动"), Some(2));
    assert_eq!(document.clips[0].channels[0].keys[2], after);
    assert_eq!(edit.apply(&mut document, false).expect("撤销移动"), Some(1));
    assert_eq!(document.clips[0].channels[0], original);
}

#[test]
fn an_insert_command_cannot_replace_an_existing_key() {
    let authoritative = Arc::new(test_document());
    let original = authoritative.clips[0].channels[0].clone();
    let mut replacement = original.keys[0].clone();
    replacement.value[0] = 100.0;
    let edit = Edit::Key {
        clip: 0,
        channel: 0,
        before: None,
        after: Some(replacement),
    };
    let mut candidate = (*authoritative).clone();
    assert!(
        edit.apply(&mut candidate, true).is_err(),
        "插入命令不得悄悄变成替换现有帧"
    );
    assert_eq!(authoritative.clips[0].channels[0], original);
}

#[test]
fn failed_candidate_edits_leave_the_authoritative_arc_document_available() {
    let authoritative = Arc::new(cubic_document());
    let original = authoritative.clips[0].channels[0].clone();
    let before = original.keys[1].clone();
    let mut after = before.clone();
    after.time = 2.0;
    let collision = Edit::Key {
        clip: 0,
        channel: 0,
        before: Some(before.clone()),
        after: Some(after),
    };
    let mut candidate = (*authoritative).clone();
    assert!(collision.apply(&mut candidate, true).is_err());
    let invalid = Edit::Key {
        clip: 0,
        channel: 0,
        before: Some(before),
        after: Some(Keyframe {
            time: 1.0,
            value: vec![f32::NAN; 3],
            in_tangent: vec![0.0; 3],
            out_tangent: vec![0.0; 3],
        }),
    };
    assert!(invalid.apply(&mut candidate, true).is_err());
    assert_eq!(authoritative.clips[0].channels[0], original);
    assert_eq!(
        authoritative.clips[0].channels[0].sample(0.5),
        original.sample(0.5)
    );
}

#[test]
fn undoing_quaternion_value_edit_restores_author_values_without_renormalizing_them() {
    let mut document = test_document();
    let channel = &mut document.clips[0].channels[0];
    channel.path = "rotation".into();
    channel.keys[0].value = vec![0.0, 0.0, 0.0, 0.9999999];
    channel.keys[1].value = vec![0.0, 1.0, 0.0, 0.0];
    let original = channel.clone();
    let before = original.keys[0].clone();
    let mut after = before.clone();
    after.value = vec![0.0, 2.0, 0.0, 2.0];
    let edit = Edit::Key {
        clip: 0,
        channel: 0,
        before: Some(before),
        after: Some(after),
    };
    edit.apply(&mut document, true).expect("编辑旋转");
    assert!(
        (document.clips[0].channels[0].keys[0]
            .value
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            - 1.0)
            .abs()
            < 0.00001
    );
    edit.apply(&mut document, false).expect("撤销旋转");
    assert_eq!(document.clips[0].channels[0], original);
}

pub(super) fn app_context() -> (App, PathBuf, PathBuf) {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<DocumentState>()
        .init_resource::<SceneEditorNumericScrubMouseGuard>()
        .init_resource::<VegetationPresetEditorState>()
        .init_resource::<AssetPreviewWorkspace>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_message::<MouseWheel>()
        .add_message::<NumericInputCommitted>();
    let root = app
        .world()
        .resource::<DocumentState>()
        .asset_root()
        .to_owned();
    let alpha = root.join("animation-regression-alpha.glb");
    let beta = root.join("animation-regression-beta.glb");
    let mut editor = SceneEditorState::default();
    editor.open = true;
    editor.selected_asset = Some("animation-regression-alpha.glb".into());
    let mut sidebar = SceneEditorSidebarState::default();
    sidebar.right = EditorRightSidebarTab::Assets;
    let mut state = AnimationEditorState::default();
    state.active = Some(alpha.clone());
    state.visible = true;
    state
        .sessions
        .insert(alpha.clone(), Session::new(test_document()));
    state
        .sessions
        .insert(beta.clone(), Session::new(test_document()));
    app.insert_resource(editor)
        .insert_resource(sidebar)
        .insert_resource(state)
        .insert_resource(AssetPreviewState {
            selected_path: Some("animation-regression-alpha.glb".into()),
            content: AssetPreviewContent::Model {
                handle: Handle::default(),
                root: Entity::PLACEHOLDER,
                phase: AssetPreviewPhase::Ready,
            },
            ..default()
        });
    (app, alpha, beta)
}

fn run_until_idle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        if !app.world().resource::<AnimationEditorState>().busy() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "后台作者任务应能在测试期限内非阻塞完成"
        );
        std::thread::yield_now(); // 仅测试线程等待，不进入生产帧关键路径。
    }
}

#[test]
fn pose_toolbar_click_switches_tools_without_modifying_animation() {
    let (mut app, _, _) = app_context();
    app.add_systems(Update, control::input);
    let document = app.world().resource::<AnimationEditorState>().document_snapshot().unwrap();
    for action in [AnimationAction::RotatePose, AnimationAction::MovePose] {
        let binding = app.world().resource::<AnimationEditorState>().revision;
        let button = app.world_mut().spawn((AnimationButton { action, binding }, EditorButtonClick::triggered())).id();
        app.update();
        let state = app.world().resource::<AnimationEditorState>();
        assert!(state.pose_tool == if action == AnimationAction::MovePose { super::types::PoseTool::Move } else { super::types::PoseTool::Rotate });
        assert!(state.pending.is_none());
        assert!(!state.current().unwrap().dirty());
        assert!(Arc::ptr_eq(&document, &state.document_snapshot().unwrap()));
        app.world_mut().entity_mut(button).despawn();
    }
}

fn queue(app: &mut App, action: PendingAction) {
    app.world_mut()
        .resource_mut::<AnimationEditorState>()
        .pending = Some(action);
    run_until_idle(app);
}

#[test]
fn undo_from_another_clip_returns_to_the_edited_clip_channel_and_key() {
    let (mut app, alpha, _) = app_context();
    app.add_systems(Update, drive::drive);
    let document = test_document();
    let edit = changed_key(&document, 17.0);
    {
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        let session = state.sessions.get_mut(&alpha).unwrap();
        let mut candidate = document.clone();
        edit.apply(&mut candidate, true).unwrap();
        session.document = Arc::new(candidate);
        session.history.push(edit);
        session.cursor = 1;
        session.clip = 1;
        session.channel = Some(0);
        session.key = None;
        session.page = 99;
    }
    queue(&mut app, PendingAction::Undo);
    let state = app.world().resource::<AnimationEditorState>();
    let session = state.sessions.get(&alpha).unwrap();
    assert_eq!(session.clip, 0);
    assert_eq!(session.channel, Some(0));
    assert_eq!(session.key, Some(0));
    assert_eq!(session.page, 0);
    assert_eq!(session.cursor, 0);
    assert_eq!(
        session.document.clips[0].channels[0],
        document.clips[0].channels[0]
    );
    assert_eq!(
        session.document.clips[1].channels[0],
        document.clips[1].channels[0]
    );
}

#[test]
fn branching_after_undo_does_not_reuse_a_saved_history_position() {
    let (mut app, alpha, _) = app_context();
    app.add_systems(Update, drive::drive);
    let edit = changed_key(&test_document(), 17.0);
    queue(&mut app, PendingAction::Edit(edit));
    {
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        let session = state.sessions.get_mut(&alpha).unwrap();
        session.saved_cursor = Some(session.cursor);
        assert!(!session.dirty());
    }
    queue(&mut app, PendingAction::Undo);
    let edit = changed_key(
        app.world()
            .resource::<AnimationEditorState>()
            .active_document()
            .unwrap(),
        23.0,
    );
    queue(&mut app, PendingAction::Edit(edit));
    let state = app.world().resource::<AnimationEditorState>();
    let session = state.sessions.get(&alpha).unwrap();
    assert_eq!(session.cursor, 1);
    assert_eq!(session.history.len(), 1);
    assert_eq!(
        session.saved_cursor, None,
        "分支历史的相同序号不等于原保存内容"
    );
    assert!(session.dirty());
    assert_eq!(session.document.clips[0].channels[0].keys[0].value[0], 23.0);
}

#[test]
fn source_switch_keeps_inflight_credit_and_late_edit_updates_only_its_original_session() {
    let (mut app, alpha, beta) = app_context();
    app.add_systems(Update, drive::drive);
    // Beta 是已停放的脏草稿，切回时必须保留；本测试不发起真实磁盘刷新。
    app.world_mut()
        .resource_mut::<AnimationEditorState>()
        .sessions
        .get_mut(&beta)
        .unwrap()
        .saved_cursor = None;
    let document = test_document();
    let edit = changed_key(&document, 31.0);
    let mut candidate = document.clone();
    let key = edit.apply(&mut candidate, true).unwrap();
    let metadata = Metadata::new(&candidate);
    let (sender, receiver) = std::sync::mpsc::channel();
    app.world_mut().resource_mut::<AnimationEditorState>().work = Some(Work {
        path: alpha.clone(),
        generation: None,
        saving: false,
        task: IoTaskPool::get()
            .spawn(async move { receiver.recv().expect("测试发送真实完成结果") }),
    });
    app.world_mut()
        .resource_mut::<AssetPreviewState>()
        .selected_path = Some("animation-regression-beta.glb".into());
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .selected_asset = Some("animation-regression-beta.glb".into());
    app.update();
    {
        let state = app.world().resource::<AnimationEditorState>();
        assert_eq!(state.active.as_ref(), Some(&beta));
        assert_eq!(
            state.work.as_ref().unwrap().path,
            alpha,
            "换源不得提前释放旧任务槽"
        );
        assert_eq!(state.sessions.get(&beta).unwrap().cursor, 0);
    }
    sender
        .send(TaskResult::Edit {
            document: Ok((candidate, metadata)),
            action: PendingAction::Edit(edit),
            key,
            target: (0, 0),
            selection: None,
        })
        .unwrap_or_else(|_| panic!("单槽后台接收者应仍存活"));
    run_until_idle(&mut app);
    let state = app.world().resource::<AnimationEditorState>();
    assert_eq!(state.active.as_ref(), Some(&beta));
    assert_eq!(
        state.sessions.get(&alpha).unwrap().document.clips[0].channels[0].keys[0].value[0],
        31.0
    );
    assert_eq!(state.sessions.get(&alpha).unwrap().cursor, 1);
    assert_eq!(
        state.sessions.get(&beta).unwrap().document.clips[0].channels[0],
        document.clips[0].channels[0]
    );
    assert_eq!(state.sessions.get(&beta).unwrap().cursor, 0);
    assert!(
        !state.status.contains("草稿已更新"),
        "后台原文件的回执不得冒充当前文件的回执"
    );
}

#[test]
fn failed_save_preserves_the_exact_draft_history_and_savepoint() {
    let (mut app, alpha, _) = app_context();
    app.add_systems(Update, drive::drive);
    queue(
        &mut app,
        PendingAction::Edit(changed_key(&test_document(), 41.0)),
    );
    let snapshot = app
        .world()
        .resource::<AnimationEditorState>()
        .document_snapshot()
        .unwrap();
    app.world_mut().resource_mut::<AnimationEditorState>().work = Some(Work {
        path: alpha.clone(),
        generation: None,
        saving: true,
        task: IoTaskPool::get()
            .spawn(async { TaskResult::Save(Err("磁盘动画文件已被外部修改".into())) }),
    });
    run_until_idle(&mut app);
    let state = app.world().resource::<AnimationEditorState>();
    let session = state.sessions.get(&alpha).unwrap();
    assert!(Arc::ptr_eq(&snapshot, &session.document));
    assert_eq!(session.cursor, 1);
    assert_eq!(session.saved_cursor, Some(0));
    assert!(session.dirty());
    assert!(state.status.contains("保存失败"));
    assert!(state.status.contains("外部修改"));
}

#[test]
fn hot_reloaded_node_indices_reject_a_captured_old_paste_command() {
    use super::editing::{self, Command};
    use super::types::KeySelection;

    let (mut app, alpha, _) = app_context();
    app.add_systems(Update, drive::drive);
    app.world_mut()
        .resource_mut::<AnimationEditorState>()
        .sessions
        .insert(alpha.clone(), Session::new(super::data::tests::clipboard_document(false)));
    queue(
        &mut app,
        PendingAction::Command(Command::Copy {
            clip: 0,
            selection: KeySelection::single(0, 0.0),
            source: alpha.clone(),
        }),
    );
    let clipboard = app
        .world()
        .resource::<AnimationEditorState>()
        .clipboard
        .clone()
        .expect("正式后台复制应发布剪贴板");
    let replacement = super::data::tests::clipboard_document(true);
    app.world_mut().resource_mut::<AnimationEditorState>().work = Some(Work {
        path: alpha.clone(),
        generation: None,
        saving: false,
        task: IoTaskPool::get()
            .spawn(async move { TaskResult::Load(Ok(Session::new(replacement))) }),
    });
    run_until_idle(&mut app);
    let reloaded = app
        .world()
        .resource::<AnimationEditorState>()
        .document_snapshot()
        .unwrap();
    assert!(editing::build(
        &reloaded,
        Command::Paste {
            clip: 0,
            at: 5.0,
            source: alpha.clone(),
            clipboard: clipboard.clone(),
        },
    )
    .is_err());
    // 故意保留并提交换代前已捕获的命令，证明后台入口的保护不依赖清空UI剪贴板。
    queue(
        &mut app,
        PendingAction::Command(Command::Paste {
            clip: 0,
            at: 5.0,
            source: alpha.clone(),
            clipboard: clipboard.clone(),
        }),
    );
    {
        let state = app.world().resource::<AnimationEditorState>();
        let session = state.sessions.get(&alpha).unwrap();
        assert!(Arc::ptr_eq(&reloaded, &session.document));
        assert!(Arc::ptr_eq(&clipboard, state.clipboard.as_ref().unwrap()));
        assert_eq!(session.cursor, 0);
        assert_eq!(session.saved_cursor, Some(0));
        assert!(!session.dirty());
        assert!(state.status.contains("重新复制"));
    }
    queue(
        &mut app,
        PendingAction::Command(Command::Copy {
            clip: 0,
            selection: KeySelection::single(0, 0.0),
            source: alpha.clone(),
        }),
    );
    let clipboard = app
        .world()
        .resource::<AnimationEditorState>()
        .clipboard
        .clone()
        .unwrap();
    queue(
        &mut app,
        PendingAction::Command(Command::Paste {
            clip: 1,
            at: 5.0,
            source: alpha.clone(),
            clipboard,
        }),
    );
    let state = app.world().resource::<AnimationEditorState>();
    let session = state.sessions.get(&alpha).unwrap();
    assert_eq!(session.cursor, 1, "重新复制后仍允许同源跨动作粘贴");
    assert_eq!(session.document.clips[1].channels[0].keys.last().unwrap().time, 5.0);
    assert_eq!(session.document.clips[1].channels[0].node_name, "骨架");
}

#[test]
fn ordinary_static_models_have_no_animation_panel() {
    let (mut app, alpha, _) = app_context();
    let mut document = test_document();
    document.clips.clear();
    app.world_mut()
        .resource_mut::<AnimationEditorState>()
        .sessions
        .insert(alpha, Session::new(document));
    let state = app.world().resource::<AnimationEditorState>();
    let view = state.view();
    assert!(!state.visible(), "普通静态模型也不应生成动画骨架覆盖");
    assert!(!view.visible);
    assert_eq!(view.clip_count, 0);
    assert!(view.rows.is_empty());
}

#[test]
fn readonly_documents_reject_real_numeric_commits_and_edit_buttons() {
    let (mut app, alpha, _) = app_context();
    app.add_systems(Update, control::input);
    let revision;
    {
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        let session = state.sessions.get_mut(&alpha).unwrap();
        let mut document = (*session.document).clone();
        document.read_only = true;
        session.document = Arc::new(document);
        session.key = Some(0);
        revision = state.revision;
    }
    let field = app
        .world_mut()
        .spawn(AnimationNumericInput {
            field: AnimationNumericField::Value(0),
            binding: revision,
        })
        .id();
    app.world_mut().write_message(NumericInputCommitted {
        entity: field,
        value: NumericValue::F32(99.0),
        outcome: NumericInputCommitOutcome::Accepted,
        source: NumericInputCommitSource::Text,
        changed: true,
        edited: true,
    });
    app.world_mut().spawn((
        AnimationButton {
            action: AnimationAction::DeleteKey,
            binding: revision,
        },
        EditorButtonClick::triggered(),
    ));
    app.update();
    let state = app.world().resource::<AnimationEditorState>();
    assert!(
        state.pending.is_none(),
        "只读必须在业务输入层拒绝，不能只依赖灰色表现"
    );
    assert!(!state.busy());
    assert!(!state.current().unwrap().dirty());
    assert_eq!(
        state.current().unwrap().document.clips[0].channels[0].keys[0].value[0],
        0.0
    );
}

fn assert_new_source_generation_survives_older_result(completed: TaskResult) {
    let (mut app, alpha, _) = app_context();
    app.add_systems(Update, drive::drive);
    let snapshot = app
        .world()
        .resource::<AnimationEditorState>()
        .document_snapshot()
        .unwrap();
    app.world_mut().resource_mut::<AssetPreviewState>().model =
        Some(AssetPreviewModelSession::new(2, Handle::default()));
    let task = IoTaskPool::get().spawn(async move { completed });
    let deadline = Instant::now() + Duration::from_secs(5);
    while !task.is_finished() {
        assert!(Instant::now() < deadline, "旧来源任务须在测试期限内完成");
        std::thread::yield_now();
    }
    {
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        state.model_generation = Some((alpha.clone(), 1));
        state.work = Some(Work {
            path: alpha.clone(),
            generation: Some(1),
            saving: false,
            task,
        });
    }
    // 本帧先收到同源 gen2，再消费已完成的 gen1 结果；这是实际事件顺序中的竞态窗口。
    app.update();
    let state = app.world().resource::<AnimationEditorState>();
    let session = state.sessions.get(&alpha).unwrap();
    assert!(session.source_changed, "旧结果不得清除第二次热更新标记");
    assert!(
        Arc::ptr_eq(&snapshot, &session.document),
        "迟到读取不得替换新代校验中的旧有效作者版本"
    );
    let work = state.work.as_ref().expect("新代校验应沿同一任务槽继续");
    assert_eq!(work.path, alpha);
    assert_eq!(work.generation, Some(2));
    assert!(state.failed.is_none(), "旧错误不能阻止新来源再次校验");
}

#[test]
fn newer_hot_reload_survives_a_late_unchanged_result() {
    assert_new_source_generation_survives_older_result(TaskResult::Unchanged);
}

#[test]
fn newer_hot_reload_rejects_a_late_loaded_document() {
    let mut old_document = test_document();
    old_document.clips[0].channels[0].keys[0].value[0] = 99.0;
    assert_new_source_generation_survives_older_result(TaskResult::Load(Ok(Session::new(
        old_document,
    ))));
}

#[test]
fn newer_hot_reload_is_not_blocked_by_a_late_load_failure() {
    assert_new_source_generation_survives_older_result(TaskResult::Load(Err(
        "上一代临时读取失败".into()
    )));
}

#[test]
fn retired_documents_wait_for_the_unique_work_slot_and_drop_on_its_worker() {
    let (mut app, alpha, _) = app_context();
    app.add_systems(Update, drive::drive);
    let main_thread = std::thread::current().id();
    let released_on = Arc::new(std::sync::Mutex::new(None));
    let observed = released_on.clone();
    let document = Arc::new(test_document());
    let weak = Arc::downgrade(&document);
    let (sender, receiver) = std::sync::mpsc::channel();
    let revision;
    {
        let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
        state.status = "原作者回执".into();
        revision = state.revision;
        // 测试探针在同一释放任务内持有真正的最后文档 Arc，记录实际析构线程。
        state.retired = Some(Retired::ReleaseProbe(Box::new(move || {
            drop(document);
            *observed.lock().expect("测试线程记录锁") = Some(std::thread::current().id());
        })));
        state.work = Some(Work {
            path: alpha.clone(),
            generation: None,
            saving: false,
            task: IoTaskPool::get()
                .spawn(async move { receiver.recv().expect("测试释放原工作槽") }),
        });
    }
    app.update();
    {
        let state = app.world().resource::<AnimationEditorState>();
        assert!(state.retired.is_some(), "原任务完成前不得另开释放任务");
        assert_eq!(state.work.as_ref().unwrap().path, alpha);
        assert!(weak.upgrade().is_some());
        assert!(released_on.lock().unwrap().is_none());
    }
    sender
        .send(TaskResult::Released)
        .unwrap_or_else(|_| panic!("唯一原任务仍存活"));
    run_until_idle(&mut app);
    assert!(weak.upgrade().is_none(), "完整释放任务应归还最后一份旧文档");
    assert_ne!(
        *released_on.lock().unwrap(),
        Some(main_thread),
        "大文档析构不能回到 App::update 主线程"
    );
    let state = app.world().resource::<AnimationEditorState>();
    assert_eq!(state.revision, revision, "释放回执不更换作者绑定或历史身份");
    assert_eq!(state.status, "原作者回执");
    assert!(!state.current().unwrap().dirty());
}

#[test]
fn ordinary_text_focus_blocks_timeline_shortcuts_without_an_ime_guard() {
    use bevy::input_focus::{FocusCause, InputFocus};

    for native in [false, true] {
        for key in [KeyCode::KeyZ, KeyCode::Home] {
            let (mut app, _, _) = app_context();
            app.init_resource::<ButtonInput<KeyCode>>()
                .init_resource::<InputFocus>()
                .init_resource::<crate::resources::EditorTextInputGuard>()
                .add_systems(Update, super::timeline::input);
            let binding = {
                let mut state = app.world_mut().resource_mut::<AnimationEditorState>();
                let session = state.current_mut().unwrap();
                session.history.push(Edit::Interpolation {
                    clip: 0,
                    channel: 0,
                    before: "STEP".into(),
                    after: "LINEAR".into(),
                });
                session.cursor = 1;
                session.window_start = 0.5;
                session.window_span = 0.25;
                state.revision
            };
            app.world_mut().spawn((
                super::components::AnimationTrack { row: None, binding },
                ComputedNode {
                    size: Vec2::new(400.0, 24.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
                bevy::ui::RelativeCursorPosition {
                    cursor_over: true,
                    normalized: Some(Vec2::ZERO),
                },
                UiInteraction::Hovered,
            ));
            let field = if native {
                app.world_mut().spawn(bevy::text::EditableText::new("Search")).id()
            } else {
                app.world_mut().spawn(crate::components::EditorTextInput).id()
            };
            app.world_mut().resource_mut::<InputFocus>().set(field, FocusCause::Navigated);
            // 不模拟IME/数值门：普通英文输入仍聚焦，仅移动鼠标到时间轴就必须隔离键盘。
            app.world_mut()
                .resource_mut::<crate::resources::EditorTextInputGuard>()
                .block_shortcuts = false;
            {
                let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                if key == KeyCode::KeyZ {
                    keyboard.press(KeyCode::ControlLeft);
                }
                keyboard.press(key);
            }
            app.update();
            {
                let state = app.world().resource::<AnimationEditorState>();
                assert!(state.pending.is_none(), "普通文字焦点不得发起动画撤销");
                let session = state.current().unwrap();
                assert_eq!(session.cursor, 1);
                assert_eq!(session.window_start, 0.5);
                assert_eq!(session.window_span, 0.25);
                assert!(app.world().resource::<ButtonInput<KeyCode>>().just_pressed(key),
                    "文字控件拥有的按键不能被时间轴消费");
            }
            app.world_mut().resource_mut::<InputFocus>().clear();
            {
                let mut keyboard = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keyboard.reset_all();
                if key == KeyCode::KeyZ {
                    keyboard.press(KeyCode::ControlLeft);
                }
                keyboard.press(key);
            }
            app.update();
            let state = app.world().resource::<AnimationEditorState>();
            if key == KeyCode::KeyZ {
                assert!(matches!(state.pending, Some(PendingAction::Undo)));
            } else {
                assert_eq!(state.current().unwrap().window_start, 0.0);
                assert_eq!(state.current().unwrap().window_span, 2.0);
            }
        }
    }
}
