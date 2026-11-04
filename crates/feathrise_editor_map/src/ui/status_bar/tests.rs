//! 底栏真实回执、错误保留、地图身份、完整复制及鼠标/滚轮隔离回归。

use super::components::*;
use super::helpers::*;
use super::resources::EditorStatusBarState;
use super::systems::*;
use super::types::*;
use crate::authoring::EditorAuthoringSession;
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorSettingsState, MapEditorState};
use crate::types::{EditorStatusKind, EditorTool};
use bevy::prelude::*;
use feathrise_scene::{DynamicWaterPhase, DynamicWaterStatus};

fn app() -> App {
    let mut app = App::new();
    let draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    app.insert_resource(MapEditorState {
        open: true,
        selected_map_id: Some(draft.map_id.clone()),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        ..default()
    })
    .init_resource::<EditorStatusBarState>()
    .add_systems(Update, observe_status);
    app.update(); // 先采用当前地图身份，之后的展开才属于同一已打开的作者上下文。
    app
}

fn controls(app: &mut App) -> Entity {
    app.init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<MapEditorNumericScrubMouseGuard>()
        .init_resource::<Clipboard>()
        .add_message::<bevy::input::mouse::MouseWheel>()
        .add_systems(Update, control_details.before(observe_status));
    app.world_mut()
        .spawn((StatusButton(StatusAction::Toggle), Interaction::None))
        .id()
}

#[test]
fn ordinary_words_do_not_guess_a_save_result() {
    let mut app = app();
    app.world_mut().resource_mut::<MapEditorState>().status =
        "说明：这是保存成功、保存失败字样的示例".into();
    app.update();
    let status = app.world().resource::<EditorStatusBarState>();
    assert_eq!(status.phase, StatusPhase::Ready);
    assert!(status.retained.is_none() && status.latest_result.is_none());
    assert!(status.detail.is_empty());
}

#[test]
fn typed_host_projection_reports_work_without_overwriting_tool_feedback() {
    use feathrise_editor_ui::feedback::{PanelStatusPhase, PanelStatusState};
    let mut app = app();
    app.world_mut().resource_mut::<MapEditorState>().status = "当前工具的操作说明".into();
    let source = app.world_mut().spawn((
        PanelStatusText("3D视口"), Text::new("真实宿主提供的完整诊断"), PanelStatusState::default(),
    )).id();
    for caption in ["工程3D：启动中", "工程3D：同步中", "工程3D：重启中", "工程3D：断开中"] {
        *app.world_mut().get_mut::<PanelStatusState>(source).unwrap() = PanelStatusState {
            phase: PanelStatusPhase::Working, summary: caption,
        };
        app.update();
        let state = app.world().resource::<EditorStatusBarState>();
        assert_eq!(state.phase, StatusPhase::Working);
        assert_eq!(state.task, caption);
        assert_eq!(state.summary, "当前工具的操作说明");
        assert_eq!(app.world().resource::<MapEditorState>().status, "当前工具的操作说明");
    }
    for caption in ["工程3D：已连接", "工程3D：未连接"] {
        *app.world_mut().get_mut::<PanelStatusState>(source).unwrap() = PanelStatusState {
            phase: PanelStatusPhase::Info, summary: caption,
        };
        app.update();
        let state = app.world().resource::<EditorStatusBarState>();
        assert_eq!(state.phase, StatusPhase::Ready, "信息项不能被当作后台任务");
        assert_eq!(state.task, caption);
    }
    *app.world_mut().get_mut::<PanelStatusState>(source).unwrap() = PanelStatusState::default();
    app.update();
    let state = app.world().resource::<EditorStatusBarState>();
    assert_eq!(state.phase, StatusPhase::Ready);
    assert!(state.task.is_empty(), "切回编辑器3D后恢复原底栏判断");
}

#[test]
fn typed_host_failure_is_visible_and_keeps_original_save_error_priority() {
    use feathrise_editor_ui::feedback::{PanelStatusPhase, PanelStatusState};
    let mut app = app();
    app.world_mut().spawn((
        PanelStatusText("3D视口"), Text::new("实际进程返回的失败原因"),
        PanelStatusState { phase: PanelStatusPhase::Failed, summary: "工程3D：连接失败" },
    ));
    app.world_mut().resource_mut::<EditorStatusBarState>().popup_open = true;
    app.update();
    let state = app.world().resource::<EditorStatusBarState>();
    assert_eq!(state.phase, StatusPhase::Error);
    assert_eq!(state.summary, "工程3D：连接失败");
    assert!(state.detail.contains("实际进程返回的失败原因"));
    app.world_mut().resource_mut::<MapEditorState>().cancel_close_with_error("当前地图保存失败");
    app.update();
    let state = app.world().resource::<EditorStatusBarState>();
    assert_eq!(state.phase, StatusPhase::Error);
    assert_eq!(state.summary, "当前地图保存失败");
    assert!(state.task.contains("工程3D：连接失败"));
}

#[test]
fn details_only_occlude_their_rectangle_and_keep_the_existing_mouse_gate() {
    use crate::standalone::viewport_projection::{ViewportHostProjection, install as install_projection};
    let mut app = app();
    let toggle = controls(&mut app);
    app.init_resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
        .add_systems(PreUpdate, track_pointer)
        .add_systems(Update, sync_presentation.after(observe_status))
        .add_systems(Update, crate::ui::numeric_scrub_mouse_guard::update_map_editor_numeric_scrub_mouse_guard.after(control_details));
    install_projection(&mut app);
    app.world_mut().spawn((crate::components::MapEditorViewport, Node::default(),
        ComputedNode { size: Vec2::new(800., 600.), ..default() },
        UiGlobalTransform::from_xy(500., 350.), InheritedVisibility::VISIBLE));
    let popup = app.world_mut().spawn((StatusPopup, Node { display: Display::None, ..default() },
        ComputedNode { size: Vec2::new(400., 280.), ..default() },
        UiGlobalTransform::from_xy(350., 450.), InheritedVisibility::VISIBLE,
        bevy::ui::RelativeCursorPosition::default(), GlobalZIndex(425))).id();
    app.update();
    let layout = app.world().resource::<ViewportHostProjection>().layout;
    assert!(!app.world().resource::<ViewportHostProjection>().obscured);
    app.world_mut().entity_mut(toggle).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    let projection = app.world().resource::<ViewportHostProjection>();
    assert!(!projection.obscured && projection.error.is_none(), "详情打开不隐藏整个游戏child");
    assert_eq!(projection.layout, layout);
    assert_eq!(projection.excluded_rects, vec![Rect::new(150., 310., 550., 590.)]);
    assert!(app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked(), "打开点击仍由原输入门隔离");
    // 阅读正文时继续调用原指针所有权流程，不能为了保留游戏画面而解除点击或滚轮门。
    app.world_mut().get_mut::<bevy::ui::RelativeCursorPosition>(popup).unwrap().cursor_over = true;
    app.update();
    assert!(app.world().resource::<EditorStatusBarState>().owns_mouse());
    assert!(app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked());
    assert!(!app.world().resource::<ViewportHostProjection>().obscured);
    app.world_mut().entity_mut(toggle).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    let projection = app.world().resource::<ViewportHostProjection>();
    assert!(!projection.obscured && projection.excluded_rects.is_empty());
    assert_eq!(projection.layout, layout);
    assert!(app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked(), "关闭点击仍须等待静默帧");
    app.update();
    assert!(!app.world().resource::<MapEditorNumericScrubMouseGuard>().blocked());
}

#[test]
fn native_plugin_failure_is_visible_and_clears_only_after_its_own_success() {
    use crate::types::{EditorStatusOperation, EditorStatusTarget};
    let mut app = app();
    let target = EditorStatusTarget::new("fixture.equipment:1", "装备·新装备");
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.status = "保存装备失败：缺少模板模型，草稿保留".into();
        editor.record_operation_result(
            EditorStatusOperation::NativePlugin,
            &target,
            EditorStatusKind::Error,
        );
    }
    app.update();
    let status = app.world().resource::<EditorStatusBarState>();
    assert_eq!(status.phase, StatusPhase::Error);
    assert!(status.summary.contains("保存装备失败"));
    // 插件文档独立于地图；换图或保存另一文档均不能解除本次失败。
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.selected_map_id = None;
        editor.status = "另一文档已保存".into();
        editor.record_operation_result(
            EditorStatusOperation::NativePlugin,
            &EditorStatusTarget::new("fixture.equipment:2", "另一装备"),
            EditorStatusKind::Success,
        );
    }
    app.update();
    assert_eq!(
        app.world().resource::<EditorStatusBarState>().phase,
        StatusPhase::Error
    );
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.status = "装备已原子保存".into();
        editor.record_operation_result(
            EditorStatusOperation::NativePlugin,
            &target,
            EditorStatusKind::Success,
        );
    }
    app.update();
    let status = app.world().resource::<EditorStatusBarState>();
    assert_eq!(status.phase, StatusPhase::Success);
    assert!(status.retained.is_none());
}

#[test]
fn panel_feedback_keeps_full_detail_without_occupying_the_form_or_guessing_phase() {
    let mut app = app();
    app.add_systems(Update, hide_panel_status_texts.before(observe_status));
    let message = format!("加载失败：{}", "完整中文资源路径/".repeat(40));
    let source = app.world_mut().spawn((PanelStatusText("资源目录"), Text::new(&message), Node::default())).id();
    app.world_mut().resource_mut::<EditorStatusBarState>().popup_open = true;
    app.update();
    assert_eq!(app.world().get::<Node>(source).unwrap().display, Display::None);
    let status = app.world().resource::<EditorStatusBarState>();
    assert!(status.detail.contains(&message), "详情保留完整原因，不能只剩底栏截断摘要");
    assert_eq!(status.phase, StatusPhase::Ready, "正式阶段由类型证据决定，不能解析说明里的失败字样");
    let retained = status.panel_status[&source].1.clone();
    app.update();
    assert!(std::sync::Arc::ptr_eq(&retained, &app.world().resource::<EditorStatusBarState>().panel_status[&source].1));
    app.world_mut().get_mut::<Text>(source).unwrap().0.clear();
    app.update();
    assert!(!app.world().resource::<EditorStatusBarState>().detail.contains(&message));
    app.world_mut().get_mut::<Text>(source).unwrap().0 = message.clone();
    app.update();
    app.world_mut().resource_mut::<MapEditorState>().selected_map_id = None;
    app.update();
    app.world_mut().resource_mut::<EditorStatusBarState>().popup_open = true;
    app.update();
    assert!(!app.world().resource::<EditorStatusBarState>().detail.contains(&message), "换图不能重新采用未更新的旧面板文字");
}

#[test]
fn file_feedback_survives_map_switch_until_its_own_source_clears() {
    let mut app = app();
    let source = app.world_mut().spawn((
        PanelStatusText("文档"), feathrise_editor_ui::feedback::GlobalPanelStatus,
        Text::new("JSON第8行：缺少结束括号"), Node::default(),
    )).id();
    app.update();
    let retained = app.world().resource::<EditorStatusBarState>().panel_status[&source].1.clone();
    app.world_mut().resource_mut::<MapEditorState>().selected_map_id = None;
    app.update();
    app.world_mut().resource_mut::<EditorStatusBarState>().popup_open = true;
    app.update();
    let state = app.world().resource::<EditorStatusBarState>();
    assert!(state.detail.contains("JSON第8行：缺少结束括号"));
    assert!(std::sync::Arc::ptr_eq(&retained, &state.panel_status[&source].1), "切图沿用原文件诊断，不重新复制或冒充新回执");
    app.world_mut().get_mut::<Text>(source).unwrap().0.clear();
    app.update();
    assert!(!app.world().resource::<EditorStatusBarState>().detail.contains("JSON第8行"));
}

#[test]
fn saving_and_waiting_are_not_completion_and_only_a_real_receipt_is_success() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = true;
    app.world_mut().resource_mut::<MapEditorState>().status = "已登记后台任务".into();
    app.update();
    assert_eq!(
        app.world().resource::<EditorStatusBarState>().phase,
        StatusPhase::Working
    );
    assert_eq!(
        app.world().resource::<EditorStatusBarState>().task,
        "保存中"
    );
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .saving = false;
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .save_requested = true;
    app.update();
    assert_eq!(
        app.world().resource::<EditorStatusBarState>().task,
        "等待保存"
    );
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.save_requested = false;
        editor.status = "正式成功回执".into();
        editor.record_status_result(EditorStatusKind::Success);
    }
    app.update();
    let status = app.world().resource::<EditorStatusBarState>();
    assert_eq!(status.phase, StatusPhase::Success);
    assert!(status.task.is_empty());
}

#[test]
fn a_save_error_survives_same_frame_and_later_preview_messages() {
    let mut app = app();
    let full = format!(
        "无法写入中文路径\n{}\n完整尾部证据",
        "目录/文件节点 ".repeat(300)
    );
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.cancel_close_with_error(full.clone());
        editor.status = "普通预览已同步".into(); // 同一帧的后续系统也不能抹掉真实失败。
    }
    app.world_mut()
        .resource_mut::<EditorStatusBarState>()
        .popup_open = true;
    app.update();
    let status = app.world().resource::<EditorStatusBarState>();
    assert_eq!(status.phase, StatusPhase::Error);
    assert_eq!(status.retained.as_ref().unwrap().1.as_ref(), full);
    assert!(status.detail.contains(&full));
    assert!(status.summary.chars().count() <= 89);
    let revision = status.scroll_revision;
    app.world_mut().resource_mut::<MapEditorState>().status = "第二次普通预览".into();
    app.update();
    let status = app.world().resource::<EditorStatusBarState>();
    assert_eq!(status.phase, StatusPhase::Error);
    assert_eq!(status.scroll_revision, revision); // 后台进度变化不会把作者正在读的长诊断滚回顶部。
}

#[test]
fn a_valid_save_retry_resolves_save_failure_but_not_an_independent_runtime_failure() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .cancel_close_with_error("保存失败");
    app.update();
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.status = "已完整保存".into();
        editor.record_status_result(EditorStatusKind::Success);
    }
    app.update();
    assert!(
        app.world()
            .resource::<EditorStatusBarState>()
            .retained
            .is_none()
    );
    assert_eq!(
        app.world().resource::<EditorStatusBarState>().phase,
        StatusPhase::Success
    );
    let id = app
        .world()
        .resource::<MapEditorState>()
        .selected_map_id
        .clone();
    app.insert_resource(DynamicWaterStatus {
        phase: DynamicWaterPhase::Failed,
        map_id: id,
        detail: "水网校验失败".into(),
        ..default()
    });
    app.update();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .record_status_result(EditorStatusKind::Success);
    app.update();
    let state = app.world().resource::<EditorStatusBarState>();
    assert_eq!(state.phase, StatusPhase::Error);
    assert!(state.retained.as_ref().unwrap().1.contains("水网校验失败"));
}

#[test]
fn dismissing_a_result_does_not_replay_it_but_a_new_result_is_retained() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .cancel_close_with_error("原始错误");
    app.update();
    let button = controls(&mut app);
    app.world_mut()
        .entity_mut(button)
        .insert(StatusButton(StatusAction::Dismiss));
    app.world_mut()
        .resource_mut::<EditorStatusBarState>()
        .popup_open = true;
    app.world_mut()
        .entity_mut(button)
        .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(
        app.world()
            .resource::<EditorStatusBarState>()
            .retained
            .is_none()
    );
    app.update();
    assert!(
        app.world()
            .resource::<EditorStatusBarState>()
            .retained
            .is_none()
    );
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .cancel_close_with_error("原始错误");
    app.update();
    assert_eq!(
        app.world()
            .resource::<EditorStatusBarState>()
            .retained
            .as_ref()
            .unwrap()
            .1
            .as_ref(),
        "原始错误"
    );
}

#[test]
fn switching_maps_clears_retained_diagnostics_and_old_receipts_never_return() {
    let mut app = app();
    let original = app
        .world()
        .resource::<MapEditorState>()
        .selected_map_id
        .clone();
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .cancel_close_with_error("原地图错误");
    app.update();
    app.world_mut()
        .resource_mut::<EditorStatusBarState>()
        .popup_open = true;
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = None;
    app.update();
    let status = app.world().resource::<EditorStatusBarState>();
    assert!(status.retained.is_none() && !status.popup_open);
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .selected_map_id = original;
    app.update();
    assert!(
        app.world()
            .resource::<EditorStatusBarState>()
            .retained
            .is_none()
    );
}

#[test]
fn runtime_failures_are_map_scoped_and_recovery_does_not_erase_unread_diagnostics() {
    let mut app = app();
    let id = app
        .world()
        .resource::<MapEditorState>()
        .selected_map_id
        .clone();
    app.insert_resource(DynamicWaterStatus {
        phase: DynamicWaterPhase::Failed,
        map_id: None,
        detail: "其他地图失败".into(),
        ..default()
    });
    app.update();
    assert!(
        app.world()
            .resource::<EditorStatusBarState>()
            .retained
            .is_none()
    );
    app.world_mut().resource_mut::<DynamicWaterStatus>().map_id = id;
    app.update();
    assert_eq!(
        app.world().resource::<EditorStatusBarState>().phase,
        StatusPhase::Error
    );
    app.world_mut().resource_mut::<DynamicWaterStatus>().phase = DynamicWaterPhase::Ready;
    app.update();
    assert!(
        app.world()
            .resource::<EditorStatusBarState>()
            .retained
            .is_some()
    );
}

#[test]
fn opening_and_closing_details_do_not_change_author_state_and_capture_release() {
    let mut app = app();
    let button = controls(&mut app);
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    app.world_mut()
        .entity_mut(button)
        .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(app.world().resource::<EditorStatusBarState>().popup_open);
    assert!(app.world().resource::<EditorStatusBarState>().owns_mouse());
    assert!(
        app.world()
            .resource::<MapEditorNumericScrubMouseGuard>()
            .blocked()
    );
    let editor = app.world().resource::<MapEditorState>();
    assert!(
        !editor.save_requested && !editor.undo_requested && !editor.redo_requested && !editor.dirty
    );
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().revision,
        revision
    );
    app.world_mut()
        .entity_mut(button)
        .insert((StatusButton(StatusAction::Close), Interaction::None));
    app.update();
    app.world_mut()
        .entity_mut(button)
        .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(!app.world().resource::<EditorStatusBarState>().popup_open);
    assert!(
        app.world()
            .resource::<EditorStatusBarState>()
            .captured_this_frame
    );
}

#[test]
fn blocked_button_edges_are_consumed_instead_of_replayed_after_a_modal_closes() {
    let mut app = app();
    let button = controls(&mut app);
    app.insert_resource(MapEditorSettingsState {
        open: true,
        ..default()
    });
    app.world_mut()
        .entity_mut(button)
        .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(!app.world().resource::<EditorStatusBarState>().popup_open);
    app.world_mut()
        .resource_mut::<MapEditorSettingsState>()
        .open = false;
    app.update();
    assert!(!app.world().resource::<EditorStatusBarState>().popup_open);
}

#[test]
fn copy_adapter_preserves_full_unicode_and_reports_real_clipboard_errors() {
    let detail = format!("诊断 首行\n{}\n尾部 α", "含空格/中文路径 ".repeat(400));
    let mut calls = 0;
    let (failed, message) = copy_detail(&detail, |actual| {
        calls += 1;
        assert_eq!(actual, detail);
        Ok(())
    });
    assert_eq!(calls, 1);
    assert!(!failed && message == "完整详情已复制");
    let (failed, message) = copy_detail(&detail, |_| {
        Err(bevy::clipboard::ClipboardError::ClipboardOccupied)
    });
    assert!(failed && message.contains("ClipboardOccupied"));
}

#[test]
fn loading_details_remain_readable_and_dismissing_does_not_unlock_failed_map() {
    let mut app = app();
    let button = controls(&mut app);
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    let mut progress = feathrise_scene_core::state::LoadingProgress::for_transition(
        &feathrise_scene_core::state::PendingSceneTransition::editor_map(feathrise_scene_core::state::ScenePhase::Active, "测试加载地图"),
    );
    progress.stage = feathrise_scene_core::state::LoadingStage::Failed;
    progress.label = format!("地形资源读取失败：{}", "完整中文目录/".repeat(40));
    let error = progress.label.clone();
    app.insert_resource(progress)
        .add_systems(Update, crate::ui::map_loading::sync_map_loading.before(observe_status));
    let source = app.world_mut().spawn((
        crate::components::MapEditorMapLoadingDetail, PanelStatusText("地图加载"),
        Text::default(), TextColor::default(), Node::default(),
    )).id();
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.map_transition_in_progress = true;
        editor.pending_map_transition = editor.selected_map_id.clone();
    }
    app.world_mut().entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    app.update();
    let status = app.world().resource::<EditorStatusBarState>();
    assert!(status.popup_open, "加载与待采用请求不再屏蔽只读详情");
    assert_eq!(status.phase, StatusPhase::Error);
    // 请求尚未消费时显示的是排队状态；正式请求被消费后，详情必须采用完整加载错误。
    app.world_mut().resource_mut::<MapEditorState>().pending_map_transition = None;
    app.update();
    let status = app.world().resource::<EditorStatusBarState>();
    assert!(status.detail.contains(&error), "错误长路径必须完整进入详情");
    app.world_mut().entity_mut(button).insert((StatusButton(StatusAction::Dismiss), feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(!app.world().resource::<EditorStatusBarState>().popup_open);
    assert_eq!(app.world().resource::<EditorStatusBarState>().phase, StatusPhase::Error);
    assert!(app.world().resource::<MapEditorState>().map_transition_in_progress);
    assert_eq!(app.world().resource::<feathrise_scene_core::state::LoadingProgress>().stage, feathrise_scene_core::state::LoadingStage::Failed);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, revision);
    app.world_mut().entity_mut(button).insert((StatusButton(StatusAction::Toggle), feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(app.world().resource::<EditorStatusBarState>().popup_open);
    app.insert_resource(MapEditorSettingsState { open: true, ..default() });
    app.update();
    assert!(!app.world().resource::<EditorStatusBarState>().popup_open, "真实管理模态仍独占输入");
    app.world_mut().resource_mut::<MapEditorState>().map_transition_in_progress = false;
    app.update();
    assert!(app.world().get::<Text>(source).unwrap().is_empty(), "完成后不残留上一次地图加载详情");
}

#[test]
fn layout_and_numeric_owners_close_details_without_canceling_their_original_input() {
    let mut app = app();
    let button = controls(&mut app);
    app.insert_resource(crate::resources::MapEditorLayoutState {
        owns_mouse_this_frame: true,
        ..default()
    });
    app.world_mut()
        .entity_mut(button)
        .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(!app.world().resource::<EditorStatusBarState>().popup_open);
    app.world_mut()
        .resource_mut::<crate::resources::MapEditorLayoutState>()
        .owns_mouse_this_frame = false;
    app.update();
    assert!(!app.world().resource::<EditorStatusBarState>().popup_open);
    app.world_mut().entity_mut(button).insert(Interaction::None);
    app.update();
    app.world_mut()
        .entity_mut(button)
        .insert((Interaction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
    app.update();
    assert!(app.world().resource::<EditorStatusBarState>().popup_open);
    let numeric = app.world_mut().spawn_empty().id();
    let mut scrub = feathrise_editor_ui::ActiveNumericInputScrub::default();
    scrub.entity = Some(numeric);
    app.insert_resource(scrub);
    app.update();
    assert!(!app.world().resource::<EditorStatusBarState>().popup_open);
    assert_eq!(
        app.world()
            .resource::<feathrise_editor_ui::ActiveNumericInputScrub>()
            .entity,
        Some(numeric)
    );
}

#[test]
fn tool_parameters_are_only_visible_for_the_applicable_tool() {
    let mut editor = MapEditorState::default();
    assert!(!tool_context(&editor).contains("半径"));
    editor.terrain_sculpt_speed = 6.5;
    for tool in [EditorTool::Raise, EditorTool::Lower, EditorTool::Smooth, EditorTool::Flatten] {
        editor.tool = tool;
        let context = tool_context(&editor);
        assert!(context.contains("半径") && context.contains("6.5m/s"));
    }
    for tool in [EditorTool::VolumeDig, EditorTool::VolumeFill, EditorTool::VolumeSmooth,
        EditorTool::VolumeFlatten, EditorTool::Surface, EditorTool::Puddles,
        EditorTool::Vegetation, EditorTool::Water] {
        editor.tool = tool;
        assert!(!tool_context(&editor).contains("m/s"), "非高度工具不能显示未消费的雕刻速度");
    }
    editor.tool = EditorTool::Portal;
    assert!(!tool_context(&editor).contains("半径"));
    editor.tool = EditorTool::Domain("fixture.domain");
    editor.domain_brush_active = true;
    assert!(tool_context(&editor).contains("间距") && !tool_context(&editor).contains("m/s"));
}
