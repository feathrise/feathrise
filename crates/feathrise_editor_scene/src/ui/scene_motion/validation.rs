//! 独占工程通过原视口手柄、时间轴和保存入口验收主 GLB 动画。

mod triggers;
pub use triggers::{prepare_unbound_clip, read_saved_trigger_regions};

use bevy::{
    app::AppExit, input_focus::InputFocus, math::DVec2, prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready}, text::{EditableText, TextEdit},
    ui::{InteractionDisabled, RelativeCursorPosition, UiGlobalTransform}, window::PrimaryWindow,
};
use feathrise_editor_core::PresetWorkspaceTab;
use feathrise_editor_animation_model::data::AnimationDocument;
use feathrise_editor_ui::{EditorUiSystemSet, NumericInputCommitOutcome, NumericInputCommitSource,
    NumericInputCommitted, NumericInputDisabled, NumericInputDisplayText, NumericInputValue, NumericValue};
use serde_json::{Value, json};
use std::{path::{Path, PathBuf}, time::{Duration, Instant}};
use crate::{
    SceneEditorSystemSet, authoring::{EditorAuthoringSession, scene_motion::{SCENE_ANIMATION_DRAFT_EXTRA, SceneMotionTiming}},
    components::SceneEditorViewport,
    resources::{SceneEditorState, SceneEditorViewportState},
    standalone::{author_status, preview_ready_for, request_open_scene,
        scene_format_validation::{action_button, tree_branch_arrow, tree_icon_offset, tree_row_glyph, visible_placement_row}},
    types::{EditorAction, EditorView},
    ui::{
        file_manager_validation::pointer_click,
        gltf_browser::{EditorGltfBrowserState, MainSceneGlbRow,
            SceneEditorGltfBrowserRow, SceneEditorGltfBrowserRowText},
        placement_transform::PlacementTransformField,
        scene_context_menu::{SceneMenuAction, visible_action_for_validation, visible_rename_controls_for_validation},
        sidebars::{EditorNavigatorTab, EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarTabButton,
            SceneEditorSidebarState},
        vegetation_preset_editor::VegetationPresetEditorState,
    },
    viewport::{modes::{ViewportMode, ViewportModeState, validation_press_mode},
        placement_move::{validation_animation_drag_active, validation_animation_drag_points}},
};
pub use super::{SceneMotionButton, SceneMotionField};
use super::{Channel, Workbench};

const SCENE: &str = "M1-SAMPLE";
const PLATFORM: &str = "validation-elevator";
const PLATFORM_NAME: &str = "验收电梯";
const OVERVIEW_SCREENSHOT: &str = "scene-motion-overview.png";
const REFERENCED_MODEL: &str = "sample-model";
const CLIP_NAME: &str = "电梯往返验收";
const SECOND_CLIP_NAME: &str = "门闩复位验收";
const SECOND_DURATION: f32 = 3.0;
const END_FRAME: f32 = 120.0;
const PEAK_FRAME: f32 = 105.0;
const HEIGHT: f32 = 5.0;
const TURN: f32 = 60.0;
const TURN_X_ON_TRIAL: f32 = 12.0;
const END_SCALE_X: f32 = 1.25;

#[derive(Debug, Clone, Copy)]
enum Step {
    Enter, Tree, MainGlbExpanded, NewAnimationMenu, OpenCreation, NewAnimation, SelectAnimationForRename, RenameMenu, RenameAnimation,
    EnterName, ConfirmName, VerifyRename, SecondLength, PlayEmpty, SelectPrimaryClip, SwitchPrimaryClip, VerifyPrimaryClip,
    FirstAnimationClick, SecondAnimationClick, SceneTimeline, CaptureOverview, AwaitOverviewCapture,
    PickOverviewPlacement, OverviewPlacementSelected,
    Select, FoldSelected, UnfoldSelected, Properties, Length, Loop, PingPong, AutoPlay,
    KeyOrigin, SeekLow, KeyLow, SeekPeak, MoveTool, BeginMove, Move, ReleaseMove, PositionTrack, KeyPosition,
    RotateTool, BeginTurn, Turn, ReleaseTurn, EditRotationProperty, RestoreRotationProperty,
    RotationTrack, KeyRotation, Undo, Redo, ScaleTrack, ScaleOrigin, KeyScaleOrigin, SeekScalePeak,
    ScaleTool, BeginScale, DragScale, ReleaseScale, KeyScale,
    First, Play, Playback, Review, Verify, Triggers, Save, ReleaseSave, Receipt, Capture, Finish, FailureImage,
}

#[derive(Resource)]
struct Run {
    directory: PathBuf,
    reopen: bool,
    started: Instant,
    step: Step,
    click: Option<(DVec2, u8)>,
    drag_end: Vec2,
    save_before: u64,
    clip_to_rename: &'static str,
    scene_glb_tree: Option<Value>,
    property_pose_verified: bool,
    triggers: triggers::Check,
    report: Value,
}

#[derive(Resource, Default)]
struct Pointer {
    cursor: Option<Vec2>,
    edge: Option<bool>,
}

#[derive(Resource)]
struct ImageWrite(Task<Result<(), String>>);

pub fn install(app: &mut App, directory: PathBuf, reopen: bool) {
    app.insert_resource(Run {
        directory, reopen, started: Instant::now(), step: Step::Enter, click: None,
        drag_end: Vec2::ZERO, save_before: 0, clip_to_rename: CLIP_NAME,
        scene_glb_tree: None, property_pose_verified: false, triggers: default(),
        report: Value::Null,
    }).init_resource::<Pointer>()
        .add_systems(Update, inject_pointer.after(EditorUiSystemSet::NumericInput).before(SceneEditorSystemSet::Input))
        .add_systems(Last, drive);
}

/// 只提供原视口的坐标和按键边沿，实际姿态、插帧及保存全部由原编辑系统处理。
fn inject_pointer(
    mut planned: ResMut<Pointer>,
    mut viewports: Query<&mut RelativeCursorPosition, With<SceneEditorViewport>>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
) {
    let Some(normalized) = planned.cursor else { return; };
    for mut viewport in &mut viewports {
        viewport.cursor_over = true;
        viewport.normalized = Some(normalized);
    }
    match planned.edge.take() {
        Some(true) => buttons.press(MouseButton::Left),
        Some(false) => { buttons.release(MouseButton::Left); planned.cursor = None; }
        None => {}
    }
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        let result = if run.started.elapsed() > Duration::from_secs(120) && !matches!(run.step, Step::FailureImage) {
            Err(format!("场景动画验收超时于 {:?}：{}", run.step, author_status(world).unwrap_or_default()))
        } else { advance(world, &mut run) };
        let report = match result {
            Ok(false) => return,
            Ok(true) => run.report.clone(),
            Err(error) => {
                let failed = json!({"passed":false,"reopened":run.reopen,"placement_id":PLATFORM,
                    "phase":format!("{:?}",run.step),"trigger_step":format!("{:?}",run.triggers),"error":error,"observed":probe(world,PLATFORM)});
                if triggers::enabled() && !matches!(run.step, Step::FailureImage) {
                    run.report=failed; run.step=Step::FailureImage;
                    request_screenshot(world,run.directory.join("trigger-failure.png"));
                    return;
                }
                failed
            }
        };
        let written = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())
            .and_then(|bytes| std::fs::write(run.directory.join("scene-motion.json"), bytes).map_err(|error| error.to_string()));
        if let Err(error) = &written { error!("无法写入场景动画验收报告：{error}"); }
        world.write_message(if written.is_ok() && report["passed"] == true { AppExit::Success } else { AppExit::error() });
    });
}

fn advance(world: &mut World, run: &mut Run) -> Result<bool, String> {
    use Step::*;
    use SceneMotionButton as Action;
    match run.step {
        Enter => {
            if preview_ready_for(world, SCENE) {
                run.save_before = world.resource::<EditorAuthoringSession>().successful_save_sequence;
                run.step = if triggers::enabled() { Triggers } else { Tree };
            } else if world.resource::<SceneEditorState>().open { let _ = request_open_scene(world, SCENE); }
        }
        Tree => {
            let tab = sidebar(world, EditorSidebarTab::Navigator(EditorNavigatorTab::SceneTree));
            if (world.resource::<SceneEditorSidebarState>().scene_tree_visible() && run.click.is_none())
                || click(world, run, tab)? { run.step = MainGlbExpanded; }
        }
        MainGlbExpanded => {
            if let Some(proof) = inspect_main_glb_tree(world, run.reopen)? {
                run.scene_glb_tree = Some(proof);
                run.step = if run.reopen { FirstAnimationClick } else { NewAnimationMenu };
            }
        }
        NewAnimationMenu => {
            let row = main_glb_row(world, MainSceneGlbRow::File);
            if click_with_button(world, run, row, MouseButton::Right)? { run.step = OpenCreation; }
        }
        OpenCreation => {
            let item = visible_action_for_validation(world, SceneMenuAction::NewGlbContent);
            if click(world, run, item)? { run.step = NewAnimation; }
        }
        NewAnimation => {
            let item = visible_action_for_validation(world, SceneMenuAction::NewSceneAnimation);
            if click(world, run, item)? { run.step = SelectAnimationForRename; }
        }
        SelectAnimationForRename => {
            if run.click.is_none() && (!timeline_without_placement(world)? || button(world, Action::Close).is_none()) {
                return Ok(false);
            }
            let row = active_animation_row(world);
            if click(world, run, row)? { run.step = RenameMenu; }
        }
        RenameMenu => {
            let row = active_animation_row(world);
            if !row.is_some_and(|row| tree_animation_selected(world, row)) { return Ok(false); }
            if click_with_button(world, run, row, MouseButton::Right)? {
                if let Some(proof) = &mut run.scene_glb_tree {
                    proof["context_create_opened_without_placement"] = json!(true);
                    proof["selected_animation_before_rename"] = json!(true);
                }
                run.step = RenameAnimation;
            }
        }
        RenameAnimation => {
            let item = visible_action_for_validation(world, SceneMenuAction::Rename);
            if click(world, run, item)? { run.step = EnterName; }
        }
        EnterName => {
            let Some((input, _)) = visible_rename_controls_for_validation(world) else { return Ok(false); };
            if world.resource::<InputFocus>().get() != Some(input) { return Ok(false); }
            let mut text = world.get_mut::<EditableText>(input).ok_or("名称输入未提供原文本编辑组件")?;
            text.queue_edit(TextEdit::SelectAll);
            text.queue_edit(TextEdit::Insert(run.clip_to_rename.into()));
            run.step = ConfirmName;
        }
        ConfirmName => {
            let controls = visible_rename_controls_for_validation(world);
            if run.click.is_none() && !controls.and_then(|(input, _)| world.get::<EditableText>(input))
                .is_some_and(|text| text.value().to_string() == run.clip_to_rename)
            {
                return Ok(false);
            }
            if click(world, run, controls.map(|(_, confirm)| confirm))? { run.step = VerifyRename; }
        }
        VerifyRename => {
            if !timeline_without_placement(world)? { return Ok(false); }
            let row = active_animation_row(world);
            if let Some(name) = row.filter(|&row| tree_animation_selected(world, row))
                .and_then(|row| tree_label(world, row)).filter(|&name| name == run.clip_to_rename) {
                if name == CLIP_NAME {
                    if let Some(proof) = &mut run.scene_glb_tree {
                        proof["animation"] = json!(name);
                        proof["renamed_without_placement"] = json!(true);
                    }
                    // 在同一个主 GLB 再走一遍右键新建和改名，验证首片段存在时仍可添加。
                    run.clip_to_rename = SECOND_CLIP_NAME;
                    run.step = NewAnimationMenu;
                } else {
                    run.step = SecondLength;
                }
            }
        }
        SecondLength => {
            if timeline_without_placement(world)? && commit(world, SceneMotionField::EndFrame, SECOND_DURATION * super::FPS as f32) {
                run.step = PlayEmpty;
            }
        }
        PlayEmpty => {
            if clip_settings(world).0 == SECOND_DURATION && click_button(world, run, Action::TogglePlay)? {
                run.step = SelectPrimaryClip;
            }
        }
        SelectPrimaryClip => {
            if run.click.is_none() {
                // 没有对象轨道的片段仍应响应真实播放按钮和计时器。
                if world.resource::<Workbench>().frame_time() <= 0.0 { return Ok(false); }
                if let Some(proof) = &mut run.scene_glb_tree {
                    proof["empty_clip_played_with_requested_duration"] = json!(true);
                }
            }
            let row = named_animation_row(world, CLIP_NAME);
            if click(world, run, row)? { run.step = SwitchPrimaryClip; }
        }
        SwitchPrimaryClip => {
            if run.click.is_none() {
                let row = named_animation_row(world, CLIP_NAME);
                if !row.is_some_and(|row| tree_animation_selected(world, row)) { return Ok(false); }
                if active_clip_name(world) != Some(SECOND_CLIP_NAME) {
                    return Err("单击电梯片段时，已打开的门闩时间轴应继续保留".into());
                }
            }
            if click_button(world, run, Action::Open)? { run.step = VerifyPrimaryClip; }
        }
        VerifyPrimaryClip => {
            if !timeline_without_placement(world)? || active_clip_name(world) != Some(CLIP_NAME) { return Ok(false); }
            if world.resource::<Workbench>().clip_id.as_deref() != clip_id(world, CLIP_NAME)
                || clip_settings(world).0 != SceneMotionTiming::default().duration_seconds
            {
                return Err("从属性栏切回电梯片段时应恢复它自己的时间轴和时长".into());
            }
            if let Some(proof) = &mut run.scene_glb_tree {
                proof["secondary_animation"] = json!(SECOND_CLIP_NAME);
                proof["single_click_kept_open_clip"] = json!(true);
                proof["switched_to_primary_from_inspector"] = json!(true);
            }
            run.step = Select;
        }
        FirstAnimationClick | SecondAnimationClick => {
            let first = matches!(run.step, FirstAnimationClick);
            let row = named_animation_row(world, CLIP_NAME);
            if run.click.is_none() {
                if world.resource::<Workbench>().scene.is_some()
                    || world.resource::<EditorGltfBrowserState>().selected_placement_id.is_some()
                {
                    return Err("动画行需要双击，并可在未选摆放对象时打开当前场景时间轴".into());
                }
                if !first {
                    if !row.is_some_and(|row| tree_animation_selected(world, row)) { return Ok(false); }
                    if let Some(proof) = &mut run.scene_glb_tree {
                        proof["single_click_selected_with_timeline_closed"] = json!(true);
                    }
                }
            }
            // 同一实际树行走两次原窗口按松，由生产场景树判定双击。
            if click(world, run, row)? {
                run.step = if first { SecondAnimationClick } else { SceneTimeline };
            }
        }
        SceneTimeline => {
            if !timeline_without_placement(world)? { return Ok(false); }
            let Some((row, markers)) = super::overview::visible_row_for_validation(world, PLATFORM) else {
                return Ok(false);
            };
            let label = world.get::<Children>(row)
                .and_then(|children| children.iter().find_map(|child| world.get::<Text>(child)));
            if label.is_none_or(|label| label.as_str() != PLATFORM_NAME) || markers != 3 {
                return Ok(false); // 位置、旋转、缩放的同一时刻应合并为一个标记。
            }
            if button(world, Action::TogglePlay).is_some() && commit(world, SceneMotionField::Frame, PEAK_FRAME) {
                if let Some(proof) = &mut run.scene_glb_tree {
                    proof["double_click_opened_without_placement"] = json!(true);
                    proof["overview_object_name"] = json!(PLATFORM_NAME);
                    proof["overview_key_markers"] = json!(markers);
                }
                run.step = CaptureOverview;
            }
        }
        CaptureOverview => {
            if !timeline_without_placement(world)? || !at_frame(world, PEAK_FRAME)
                || super::overview::visible_row_for_validation(world, PLATFORM).is_none_or(|(_, markers)| markers != 3)
            { return Ok(false); }
            // 播放头和共享数值控件分阶段更新，等到控件的可见数字也追上第 105 帧。
            let Some(frame) = field(world, SceneMotionField::Frame) else { return Ok(false); };
            if world.get::<NumericInputValue>(frame).and_then(|value| value.0.as_f32()) != Some(PEAK_FRAME)
                || !world.get::<Children>(frame).is_some_and(|children| children.iter().any(|child| {
                    world.get::<NumericInputDisplayText>(child).is_some_and(|text| text.root == frame)
                        && world.get::<InheritedVisibility>(child).is_some_and(|visibility| visibility.get())
                        && world.get::<Text>(child).is_some_and(|text| text.as_str() == "105")
                }))
            { return Ok(false); }
            // 截图提交到渲染后先等到写盘，再点名称进入对象明细。
            request_screenshot(world, run.directory.join(OVERVIEW_SCREENSHOT));
            run.step = AwaitOverviewCapture;
        }
        AwaitOverviewCapture => {
            let Some(result) = screenshot_result(world) else { return Ok(false); };
            result?;
            if let Some(proof) = &mut run.scene_glb_tree {
                proof["overview_screenshot"] = json!(OVERVIEW_SCREENSHOT);
            }
            run.step = PickOverviewPlacement;
        }
        PickOverviewPlacement => {
            if run.click.is_none() && !at_frame(world, PEAK_FRAME) { return Ok(false); }
            let row = super::overview::visible_row_for_validation(world, PLATFORM).map(|(entity, _)| entity);
            if click(world, run, row)? { run.step = OverviewPlacementSelected; }
        }
        OverviewPlacementSelected => {
            if !platform_selected(world) { return Ok(false); }
            if !current_scene_timeline(world) || !at_frame(world, PEAK_FRAME)
                || world.resource::<Workbench>().clip_id.as_deref() != clip_id(world, CLIP_NAME)
            {
                return Err("从时间轴对象总览进入轨道明细应保留当前片段和第 105 帧".into());
            }
            if ![Action::SelectPosition, Action::SelectRotation, Action::SelectScale].into_iter()
                .all(|action| button(world, action).is_some())
                || super::overview::visible_row_for_validation(world, PLATFORM).is_some()
            { return Ok(false); }
            if let Some(proof) = &mut run.scene_glb_tree {
                proof["selected_from_timeline_overview"] = json!(true);
                proof["overview_opened_transform_tracks"] = json!(true);
                proof["overview_selection_frame"] = json!(PEAK_FRAME);
            }
            run.step = FoldSelected;
        }
        Select => {
            let scene = feathrise_scene_content::SceneId::parse(SCENE)?;
            let row = visible_placement_row(world, &scene, PLATFORM).map(|(row, _)| row);
            if click(world, run, row)? { run.step = Properties; }
        }
        FoldSelected => {
            if run.click.is_none() && !platform_selected(world) { return Ok(false); }
            let arrow = main_glb_arrow(world);
            if click(world, run, arrow)? { run.step = UnfoldSelected; }
        }
        UnfoldSelected => {
            if run.click.is_none() {
                let scene = feathrise_scene_content::SceneId::parse(SCENE)?;
                if visible_placement_row(world, &scene, PLATFORM).is_some()
                    || visible_placement_row(world, &scene, REFERENCED_MODEL).is_some()
                {
                    return Ok(false);
                }
                if !platform_selected(world) {
                    return Err("折叠主 GLB 后应保留立方体的当前选择".into());
                }
            }
            let arrow = main_glb_arrow(world);
            if click(world, run, arrow)? { run.step = Properties; }
        }
        Properties => {
            if !platform_selected(world) { return Ok(false); }
            if !current_scene_timeline(world) { return Err("选择立方体时应保留已打开的场景时间轴".into()); }
            if run.reopen && let Some(proof) = &mut run.scene_glb_tree {
                let scene = feathrise_scene_content::SceneId::parse(SCENE)?;
                if visible_placement_row(world, &scene, PLATFORM).is_none() { return Ok(false); }
                if !current_scene_timeline(world) || !at_frame(world, PEAK_FRAME) {
                    return Err("选择立方体并折叠展开时应保留已打开的场景时间轴和播放头".into());
                }
                if button(world, Action::TogglePlay).is_none() { return Ok(false); }
                proof["playhead_retained_after_selection"] = json!(true);
                proof["selection_retained_after_fold"] = json!(true);
            }
            let tab = sidebar(world, EditorSidebarTab::Right(EditorRightSidebarTab::Scene));
            if (world.resource::<SceneEditorSidebarState>().right == EditorRightSidebarTab::Scene && run.click.is_none())
                || click(world, run, tab)?
            {
                let mut editor = world.resource_mut::<SceneEditorState>();
                editor.view = EditorView::Perspective;
                editor.orthographic = false;
                let mut view = world.resource_mut::<SceneEditorViewportState>();
                view.center = Vec3::new(0.0, 2.75, 0.0);
                view.extent = 13.0;
                view.yaw = 0.72;
                view.pitch = 0.56;
                run.step = if run.reopen { Verify } else { Length };
            }
        }
        Length => if commit(world, SceneMotionField::EndFrame, END_FRAME) { run.step = Loop; },
        Loop => if clip_settings(world).0 == 4.0 && click_button(world, run, Action::CyclePlayback)? { run.step = PingPong; },
        PingPong => if click_button(world, run, Action::CyclePlayback)? { run.step = AutoPlay; },
        AutoPlay => {
            if run.click.is_none() {
                let (_, playback, auto_play) = clip_settings(world);
                if playback != "ping_pong" { return Ok(false); }
                if auto_play { return Err("新建的场景动画应默认手动播放".into()); }
            }
            if click_button(world, run, Action::ToggleAutoPlay)? { run.step = KeyOrigin; }
        }
        KeyOrigin => {
            if clip_settings(world) == (4.0, "ping_pong", true) && key_tracks(world) == (true, true)
                && click_button(world, run, Action::InsertKey)?
            {
                run.step = SeekLow;
            }
        }
        SeekLow => if key_counts(world) == (1,1) && commit(world, SceneMotionField::Frame, 15.0) { run.step = KeyLow; },
        KeyLow => if at_frame(world, 15.0) && click_button(world, run, Action::InsertKey)? { run.step = SeekPeak; },
        SeekPeak => if key_counts(world) == (2,2) && commit(world, SceneMotionField::Frame, PEAK_FRAME) { run.step = MoveTool; },
        MoveTool => if at_frame(world, PEAK_FRAME) {
            validation_press_mode(world, ViewportMode::Move)?;
            run.step = BeginMove;
        },
        RotateTool => {
            if key_counts(world) != (3, 2) { return Ok(false); }
            validation_press_mode(world, ViewportMode::Rotate)?;
            run.step = BeginTurn;
        }
        BeginMove | BeginTurn => {
            let moving = matches!(run.step, BeginMove);
            let (mode, delta) = if moving { (ViewportMode::Move, HEIGHT) } else { (ViewportMode::Rotate, TURN) };
            if world.resource::<ViewportModeState>().mode == mode
                && let Some((from, to)) = validation_animation_drag_points(world, mode, delta)
            {
                run.drag_end = to;
                let mut pointer = world.resource_mut::<Pointer>();
                pointer.cursor = Some(from);
                pointer.edge = Some(true);
                run.step = if moving { Move } else { Turn };
            }
        }
        Move | Turn => if validation_animation_drag_active(world) {
            world.resource_mut::<Pointer>().cursor = Some(run.drag_end);
            run.step = if matches!(run.step, Move) { ReleaseMove } else { ReleaseTurn };
        },
        ReleaseMove | ReleaseTurn => {
            world.resource_mut::<Pointer>().edge = Some(false);
            run.step = if matches!(run.step, ReleaseMove) { PositionTrack } else { EditRotationProperty };
        }
        EditRotationProperty => {
            if validation_animation_drag_active(world) || !at_peak(world, true)
                || !property_display(world, PlacementTransformField::TranslationY, 0.25 + HEIGHT)
                || !property_display(world, PlacementTransformField::RotationY, TURN)
            {
                return Ok(false);
            }
            let input = property_field(world, PlacementTransformField::RotationX).map(|(entity, _)| entity);
            if commit_numeric(world, input, TURN_X_ON_TRIAL) { run.step = RestoreRotationProperty; }
        }
        RestoreRotationProperty => {
            let expected = Quat::from_euler(EulerRot::XYZ, TURN_X_ON_TRIAL.to_radians(), TURN.to_radians(), 0.0);
            let input = property_field(world, PlacementTransformField::RotationX)
                .filter(|(_, value)| (*value - TURN_X_ON_TRIAL).abs() < 0.05).map(|(entity, _)| entity);
            // 先核对属性输入确实改变视口当前帧，再还原夹具姿势，原载人游戏验证跑同一往返关键帧。
            if preview_transform(world).is_some_and(|pose| pose.rotation().angle_between(expected) < 0.01)
                && commit_numeric(world, input, 0.0)
            {
                run.property_pose_verified = true;
                run.step = RotationTrack;
            }
        }
        PositionTrack | RotationTrack => {
            let position = matches!(run.step, PositionTrack);
            let action = if position { Action::SelectPosition } else { Action::SelectRotation };
            if click_button(world, run, action)? {
                run.step = if position { KeyPosition } else { KeyRotation };
            }
        }
        KeyPosition => {
            if validation_animation_drag_active(world) || !at_peak(world, false) || key_tracks(world) != (true, false) {
                return Ok(false);
            }
            if click_button(world, run, Action::InsertKey)? { run.step = RotateTool; }
        }
        KeyRotation => {
            if validation_animation_drag_active(world) || !at_peak(world, true) || key_tracks(world) != (false, true)
                || !property_display(world, PlacementTransformField::RotationX, 0.0)
            {
                return Ok(false);
            }
            if click_button(world, run, Action::InsertKey)? { run.step = Undo; }
        }
        Undo | Redo => {
            let (count, action, next) = if matches!(run.step, Undo) { ((3,3), EditorAction::Undo, Redo) }
                else { ((3,2), EditorAction::Redo, ScaleTrack) };
            if key_counts(world) != count && run.click.is_none() { return Ok(false); }
            let entity = action_button(world, action);
            if click(world, run, entity)? { run.step = next; }
        }
        ScaleTrack => if key_counts(world) == (3,3) && click_button(world, run, Action::SelectScale)? { run.step = ScaleOrigin; },
        ScaleOrigin => if world.resource::<Workbench>().keys_channel(Channel::Scale)
            && click_button(world, run, Action::First)? { run.step = KeyScaleOrigin; },
        KeyScaleOrigin => if at_frame(world, 0.0) && click_button(world, run, Action::InsertKey)? { run.step = SeekScalePeak; },
        SeekScalePeak => if scale_key_count(world) == 1 && commit(world, SceneMotionField::Frame, PEAK_FRAME) { run.step = ScaleTool; },
        ScaleTool => if at_frame(world, PEAK_FRAME) {
            validation_press_mode(world, ViewportMode::Transform)?;
            run.step = BeginScale;
        },
        BeginScale => {
            let distance = feathrise_editor_animation_model::gizmo::SCALE_DOUBLE_LOGICAL_PIXELS * END_SCALE_X.log2();
            if world.resource::<ViewportModeState>().mode == ViewportMode::Transform
                && let Some((from, to)) = validation_animation_drag_points(world, ViewportMode::Transform, distance)
            {
                run.drag_end = to;
                let mut pointer = world.resource_mut::<Pointer>();
                pointer.cursor = Some(from);
                pointer.edge = Some(true);
                run.step = DragScale;
            }
        }
        DragScale => if validation_animation_drag_active(world) {
            world.resource_mut::<Pointer>().cursor = Some(run.drag_end);
            run.step = ReleaseScale;
        },
        ReleaseScale => {
            world.resource_mut::<Pointer>().edge = Some(false);
            run.step = KeyScale;
        }
        KeyScale => if !validation_animation_drag_active(world) && at_scale_peak(world)
            && click_button(world, run, Action::InsertKey)? { run.step = First; },
        First => if scale_key_count(world) == 2 && click_button(world, run, Action::First)? { run.step = Play; },
        Play => {
            if (run.click.is_some() || at_frame(world, 0.0))
                && click_button(world, run, Action::TogglePlay)?
            {
                run.step = Playback;
            }
        }
        Playback => {
            if run.click.is_none() && preview_transform(world).is_none_or(|pose| pose.translation().y < 0.40) {
                return Ok(false);
            }
            if click_button(world, run, Action::TogglePlay)? { run.step = Review; }
        }
        Review => if commit(world, SceneMotionField::Frame, PEAK_FRAME) { run.step = Verify; },
        Verify => {
            if !at_frame(world, PEAK_FRAME) || !at_peak(world, true) || !at_scale_peak(world) { return Ok(false); }
            if run.reopen { run.report = verify_report(world, run)?; run.step = Capture; }
            else { run.step = Save; }
        }
        Triggers => if triggers::advance(world, run)? {
            if run.reopen { run.report = triggers::report(world,run)?; return Ok(true); }
            else { run.step = Save; }
        },
        Save => {
            if triggers::enabled() {
                triggers::save_shortcut(world,true)?;
                run.step=ReleaseSave;
            } else {
                let entity = action_button(world, EditorAction::Save);
                if click(world, run, entity)? { run.step = Receipt; }
            }
        }
        ReleaseSave => { triggers::save_shortcut(world,false)?; run.step=Receipt; }
        Receipt => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.saving || session.successful_save_sequence <= run.save_before { return Ok(false); }
            if triggers::enabled() { run.report=triggers::report(world,run)?; return Ok(true); }
            run.report = verify_report(world, run)?;
            run.step = Capture;
        }
        Capture => {
            if !preview_ready_for(world, SCENE) || !at_peak(world, true) { return Ok(false); }
            request_screenshot(world, run.directory.join("scene-motion.png"));
            run.step = Finish;
        }
        Finish => {
            let Some(result) = screenshot_result(world) else { return Ok(false); };
            result?;
            return Ok(true);
        }
        FailureImage => if let Some(result) = screenshot_result(world) { result?; return Ok(true); },
    }
    Ok(false)
}

fn request_screenshot(world: &mut World, output: PathBuf) {
    world.spawn(Screenshot::primary_window()).observe(move |shot: On<ScreenshotCaptured>, mut commands: Commands| {
        let image = shot.image.clone(); let output = output.clone();
        commands.insert_resource(ImageWrite(IoTaskPool::get().spawn(async move {
            image.try_into_dynamic().map_err(|error| error.to_string())?.save(output).map_err(|error| error.to_string())
        })));
    });
}

fn screenshot_result(world: &mut World) -> Option<Result<(), String>> {
    let result = check_ready(&mut world.get_resource_mut::<ImageWrite>()?.0)?;
    world.remove_resource::<ImageWrite>();
    Some(result)
}

fn key_counts(world: &World) -> (usize, usize) {
    world.resource::<EditorAuthoringSession>().draft.as_ref().and_then(|draft| draft.motion.get(PLATFORM))
        .map_or((0,0), |track| (track.position.len(), track.rotation.len()))
}

fn key_tracks(world: &World) -> (bool, bool) {
    let state = world.resource::<Workbench>();
    (state.keys_channel(Channel::Position), state.keys_channel(Channel::Rotation))
}

fn scale_key_count(world: &World) -> usize {
    world.resource::<EditorAuthoringSession>().draft.as_ref().and_then(|draft| draft.motion.get(PLATFORM))
        .map_or(0, |track| track.scale.len())
}

fn at_scale_peak(world: &World) -> bool {
    preview_transform(world).is_some_and(|pose| pose.to_scale_rotation_translation().0
        .abs_diff_eq(Vec3::new(END_SCALE_X, 1.0, 1.0), 0.025))
}

fn clip_settings(world: &World) -> (f32, &'static str, bool) {
    use crate::authoring::scene_motion::SceneMotionPlayback::*;
    world.resource::<EditorAuthoringSession>().draft.as_ref().map_or((0.0, "", false), |draft| {
        let timing = draft.motion.timing();
        let playback = match timing.playback { Once => "once", Loop => "loop", PingPong => "ping_pong" };
        (timing.duration_seconds, playback, timing.auto_play)
    })
}

fn active_clip_name(world: &World) -> Option<&str> {
    world.resource::<EditorAuthoringSession>().draft.as_ref()?.motion.clip_name()
}

fn clip_id<'a>(world: &'a World, name: &str) -> Option<&'a str> {
    world.resource::<EditorAuthoringSession>().draft.as_ref()?.motion.clips()
        .find_map(|(id, item)| (item == name).then_some(id))
}

fn at_frame(world: &World, frame: f32) -> bool {
    (world.resource::<Workbench>().frame_time() * 30.0 - f64::from(frame)).abs() < 0.015
}

fn preview_transform(world: &World) -> Option<&GlobalTransform> {
    let entity = world.get_resource::<Workbench>()?.pose.as_ref()?.entity;
    world.get::<GlobalTransform>(entity)
}

fn at_peak(world: &World, check_turn: bool) -> bool {
    preview_transform(world).is_some_and(|pose| {
        (pose.translation().y - (0.25 + HEIGHT)).abs() < 0.12
            && (!check_turn || (pose.rotation().angle_between(Quat::IDENTITY).to_degrees() - TURN).abs() < 1.0)
    })
}

fn verify_report(world: &World, run: &Run) -> Result<Value, String> {
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().ok_or("当前场景草稿丢失")?;
    let track = draft.motion.get(PLATFORM).ok_or("当前场景没有已保存的对象动画")?;
    let record = draft.placements.placements.iter().find(|record| record.id == PLATFORM).ok_or("缺少对象动画夹具")?;
    let times = |keys: &[_]| keys.iter().map(|key: &crate::authoring::scene_motion::SceneMotionKey| key.time).collect::<Vec<_>>();
    if draft.motion.dirty || world.resource::<SceneEditorState>().dirty
        || times(&track.position) != [0.0, 0.5, 3.5] || times(&track.rotation) != [0.0, 0.5, 3.5]
        || clip_settings(world) != (4.0, "ping_pong", true) || record.translation != [0.0, 0.25, 0.0] || record.yaw_degrees != 0.0
    {
        return Err("位置与旋转关键帧、片段时长或原静态摆放不符合夹具编辑结果".into());
    }
    let primary_id = clip_id(world, CLIP_NAME).ok_or("场景草稿缺少电梯片段")?;
    let secondary_id = clip_id(world, SECOND_CLIP_NAME).ok_or("场景草稿缺少独立的门闩片段")?;
    if draft.motion.clip_count() != 2 || primary_id == secondary_id || draft.motion.active_clip_id() != Some(primary_id) {
        return Err("保存和重开需保留两个独立片段及电梯时间轴".into());
    }
    let secondary_timing = SceneMotionTiming { duration_seconds: SECOND_DURATION, ..default() };
    if draft.motion.timing_for(secondary_id) != Some(secondary_timing) || draft.motion.get_for(secondary_id, PLATFORM).is_some() {
        return Err("门闩草稿的时长未保留或混入了电梯轨道".into());
    }
    let pose = preview_transform(world).ok_or("场景动画画面缺少对象")?;
    Ok(json!({"passed":true,"reopened":run.reopen,"scene_id":draft.scene_id,"placement_id":PLATFORM,
        "track":track,"duration_seconds":4.0,"playback":"ping_pong","auto_play":true,"translation":record.translation,
        "primary_clip_id":primary_id,"secondary_clip_id":secondary_id,
        "preview_translation":pose.translation(),"preview_rotation":pose.rotation().to_array(),
        "save_sequence":session.successful_save_sequence,"screenshot":"scene-motion.png",
        "scene_glb_tree":run.scene_glb_tree,"property_pose_verified":run.property_pose_verified}))
}

fn main_glb_row(world: &mut World, kind: MainSceneGlbRow) -> Option<Entity> {
    world.query::<(Entity, &SceneEditorGltfBrowserRow, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .find_map(|(entity, row, node, v)| (row.main_glb.as_ref() == Some(&kind) && visible(node, v)).then_some(entity))
}

fn active_animation_row(world: &mut World) -> Option<Entity> {
    let id = world.resource::<EditorAuthoringSession>().draft.as_ref()?.motion.active_clip_id()?.to_owned();
    animation_row(world, &id)
}

fn named_animation_row(world: &mut World, name: &str) -> Option<Entity> {
    let id = clip_id(world, name)?.to_owned();
    animation_row(world, &id)
}

fn animation_row(world: &mut World, id: &str) -> Option<Entity> {
    world.query::<(Entity, &SceneEditorGltfBrowserRow, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .find_map(|(entity, row, node, v)| {
            (matches!(&row.main_glb, Some(MainSceneGlbRow::Animation { id: row_id, .. }) if row_id == id)
                && visible(node, v)).then_some(entity)
        })
}

fn main_glb_arrow(world: &mut World) -> Option<Entity> {
    let row = main_glb_row(world, MainSceneGlbRow::File)?;
    tree_branch_arrow(world, row)
}

fn platform_selected(world: &World) -> bool {
    world.resource::<EditorGltfBrowserState>().selected_placement_id.as_deref() == Some(PLATFORM)
}

fn current_scene_timeline(world: &World) -> bool {
    world.resource::<Workbench>().scene.as_ref().is_some_and(|scene| scene.as_str() == SCENE)
        && world.resource::<SceneEditorState>().selected_scene_id.as_ref().is_some_and(|scene| scene.as_str() == SCENE)
        && world.resource::<VegetationPresetEditorState>().active_tab == PresetWorkspaceTab::Scene
}

fn timeline_without_placement(world: &World) -> Result<bool, String> {
    let workbench = world.resource::<Workbench>();
    if workbench.scene.is_none() { return Ok(false); }
    if !current_scene_timeline(world) || workbench.target.is_some()
        || world.resource::<EditorGltfBrowserState>().selected_placement_id.is_some()
    {
        return Err("场景动画应在原场景标签打开时间轴，并保留未选中摆放的状态".into());
    }
    Ok(true)
}

fn tree_caption_child<T: Component>(world: &World, row: Entity) -> Option<Entity> {
    world.get::<Children>(row)?.iter().filter_map(|child| world.get::<Children>(child))
        .flat_map(|children| children.iter())
        .find(|&child| world.get::<T>(child).is_some())
}

fn tree_label(world: &World, row: Entity) -> Option<&str> {
    world.get::<Text>(tree_caption_child::<SceneEditorGltfBrowserRowText>(world, row)?)
        .map(|text| text.as_str())
}

fn tree_animation_selected(world: &World, row: Entity) -> bool {
    tree_caption_child::<SceneEditorGltfBrowserRowText>(world, row)
        .and_then(|label| world.get::<TextColor>(label))
        .is_some_and(|color| color.0 == crate::ui::theme::TEXT_ACCENT)
}

fn ui_position(world: &World, entity: Entity) -> Option<Vec2> {
    Some(world.get::<UiGlobalTransform>(entity)?.translation)
}

fn inspect_main_glb_tree(world: &mut World, reopened: bool) -> Result<Option<Value>, String> {
    let main = main_glb_row(world, MainSceneGlbRow::File);
    let animation_row_entity = named_animation_row(world, CLIP_NAME);
    let secondary = named_animation_row(world, SECOND_CLIP_NAME);
    if reopened && (animation_row_entity.is_none() || secondary.is_none()) { return Ok(None); }
    let scene = feathrise_scene_content::SceneId::parse(SCENE)?;
    let platform = visible_placement_row(world, &scene, PLATFORM).map(|(row, _)| row);
    let reference = visible_placement_row(world, &scene, REFERENCED_MODEL).map(|(row, _)| row);
    let proof = || {
        let (main, platform, reference) = (main?, platform?, reference?);
        let rows = [main, platform, reference];
        let ys = rows.map(|row| ui_position(world, row).map(|position| position.y));
        let [Some(main_y), Some(platform_y), Some(reference_y)] = ys else {
            return None;
        };
        // 主根与正文相差一级，正文中的立方体、动画和引用共用类型图标列。
        let root = tree_row_glyph(world, main)?;
        let cuboid = tree_row_glyph(world, platform)?;
        let cuboid_indented = (tree_icon_offset(world, root, cuboid)? - 14.0).abs() < 1.1;
        let aligned = |row| tree_icon_offset(world, cuboid, tree_row_glyph(world, row)?)
            .map(|offset| offset.abs() < 1.1);
        let animation = |row| -> Option<(&str, bool)> {
            let y = ui_position(world, row)?.y;
            Some((tree_label(world, row)?, main_y < y && y < platform_y && aligned(row)?))
        };
        Some((main_y < platform_y && platform_y < reference_y,
            cuboid_indented, aligned(reference)?, tree_label(world, main)?,
            if let Some(row) = animation_row_entity { Some(animation(row)?) } else { None },
            if let Some(row) = secondary { Some(animation(row)?) } else { None }))
    };
    let Some((ordered, cuboid_indented, reference_icon_aligned, main_label, animation, secondary)) = proof() else {
        return Ok(None);
    };
    if !(ordered && cuboid_indented && reference_icon_aligned && main_label.contains("scene.glb")
        && animation.is_none_or(|(label, inside)| label == CLIP_NAME && inside)
        && secondary.is_none_or(|(label, inside)| label == SECOND_CLIP_NAME && inside))
    {
        return Err(format!("主 GLB 树验证失败，顺序 {ordered}，立方体缩进 {cuboid_indented}，引用图标对齐 {reference_icon_aligned}，动画 {animation:?} 和 {secondary:?}"));
    }
    Ok(Some(json!({"main_glb":main_label,"animation":animation.map(|(label, _)| label),"root_above_meshes":ordered,
        "cuboid_indented_14_px":cuboid_indented,"animation_icon_aligned":animation.map(|(_, inside)| inside),
        "secondary_animation":secondary.map(|(label, _)| label),"secondary_animation_icon_aligned":secondary.map(|(_, inside)| inside),
        "reference_icon_aligned":reference_icon_aligned})))
}

pub fn inspect_saved_glb(path: &Path, id: &str) -> Result<Value, String> {
    let saved = AnimationDocument::load(path)?;
    let clip = saved.clips.iter().find(|clip| clip.name == CLIP_NAME
        && saved.clip_extra(clip.source_index, "feathrise_scene_animation").and_then(Value::as_bool) == Some(true))
        .ok_or("主场景 GLB 缺少已命名的电梯片段")?;
    let primary_id = saved.clip_extra(clip.source_index, "feathrise_clip_id")
        .and_then(Value::as_str).filter(|id| !id.is_empty()).ok_or("主场景 GLB 缺少电梯片段 ID")?;
    // 未插帧片段保存在场景根元数据，文件里仍只有含通道的标准 GLB 动画。
    let secondary_id = saved.inspect_glb(|json, _| {
        json["nodes"].as_array()?.iter().filter(|node| node["extras"]["feathrise_scene_id"].as_str() == Some(SCENE))
            .find_map(|node| {
                let entry = &node["extras"][SCENE_ANIMATION_DRAFT_EXTRA];
                if entry["name"].as_str() != Some(SECOND_CLIP_NAME)
                    || entry["duration_seconds"].as_f64() != Some(f64::from(SECOND_DURATION))
                { return None; }
                entry["id"].as_str().filter(|id| !id.is_empty()).map(str::to_owned)
            })
    }).filter(|id| id != primary_id).ok_or("主场景 GLB 缺少门闩片段的独立 ID 或时长")?;
    let auto_play = saved.clip_extra(clip.source_index, "feathrise_autoplay").and_then(Value::as_bool);
    if auto_play != Some(true) { return Err("主场景 GLB 未保存自动播放设置".into()); }
    let channel = |property| clip.channels.iter().find(|channel| channel.path == property
        && saved.node_extra(channel.node,"feathrise_id").and_then(Value::as_str) == Some(id));
    let position = channel("translation").ok_or("主 GLB 没有目标对象的位置通道")?;
    let rotation = channel("rotation").ok_or("主 GLB 没有目标对象的旋转通道")?;
    let scale = channel("scale").ok_or("主 GLB 没有目标对象的缩放通道")?;
    if scale.keys.len() != 2 || scale.keys[1].value.first().is_none_or(|value| (*value - END_SCALE_X).abs() > 0.025) {
        return Err("主 GLB 的两个缩放关键帧与视口手柄不一致".into());
    }
    if rotation.keys.iter().any(|key| key.value.len() != 4) { return Err("主 GLB 旋转通道未保存标准四元数".into()); }
    Ok(json!({"clip_name":clip.name,"position_keys":position.keys.len(),"rotation_keys":rotation.keys.len(),"scale_keys":scale.keys.len(),"auto_play":auto_play,
        "primary_clip_id":primary_id,"secondary_clip_id":secondary_id,
        "position_times":position.keys.iter().map(|key|key.time).collect::<Vec<_>>(),
        "rotation_times":rotation.keys.iter().map(|key|key.time).collect::<Vec<_>>(),
        "end_position":position.keys.last().map(|key| &key.value),
        "end_rotation":rotation.keys.last().map(|key| &key.value), "end_scale":scale.keys.last().map(|key| &key.value)}))
}

fn commit(world: &mut World, slot: SceneMotionField, value: f32) -> bool {
    let entity = field(world, slot);
    commit_numeric(world, entity, value)
}

fn commit_numeric(world: &mut World, entity: Option<Entity>, value: f32) -> bool {
    let Some(entity) = entity else { return false; };
    world.write_message(NumericInputCommitted { entity, value: NumericValue::F32(value),
        outcome: NumericInputCommitOutcome::Accepted, source: NumericInputCommitSource::Text,
        changed: true, edited: true });
    true
}

fn property_field(world: &mut World, field: PlacementTransformField) -> Option<(Entity, f32)> {
    world.query_filtered::<(Entity, &PlacementTransformField, &NumericInputValue, &ComputedNode, &InheritedVisibility),
        Without<NumericInputDisabled>>()
        .iter(world).find_map(|(entity, candidate, value, node, v)| {
            if *candidate != field || !visible(node, v) { return None; }
            value.0.as_f32().map(|value| (entity, value))
        })
}

fn property_display(world: &mut World, field: PlacementTransformField, expected: f32) -> bool {
    property_field(world, field).is_some_and(|(_, value)| (value - expected).abs() < 0.05)
}

fn visible(node: &ComputedNode, visibility: &InheritedVisibility) -> bool {
    visibility.get() && node.size().min_element() > 2.0
}

pub fn button(world: &mut World, action: SceneMotionButton) -> Option<Entity> {
    world.query_filtered::<(Entity, &SceneMotionButton, &ComputedNode, &InheritedVisibility), Without<InteractionDisabled>>()
        .iter(world).find_map(|(entity, button, node, v)| (*button == action && visible(node, v)).then_some(entity))
}

pub fn field(world: &mut World, slot: SceneMotionField) -> Option<Entity> {
    world.query_filtered::<(Entity, &SceneMotionField, &ComputedNode, &InheritedVisibility), Without<NumericInputDisabled>>()
        .iter(world).find_map(|(entity, field, node, v)| (*field == slot && visible(node, v)).then_some(entity))
}

pub fn probe(world: &World, placement_id: &str) -> Option<Value> {
    let state = world.get_resource::<Workbench>()?;
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session.draft.as_ref()?;
    Some(json!({"dirty":draft.motion.dirty,"track":draft.motion.get(placement_id),"timing":clip_settings(world),
        "keyTracks":key_tracks(world),
        "previewPlaying":state.playing,"previewSeconds":state.frame_time(),"dragActive":validation_animation_drag_active(world),
        "previewTranslation":preview_transform(world).map(GlobalTransform::translation),
        "previewRotation":preview_transform(world).map(GlobalTransform::rotation)}))
}

fn sidebar(world: &mut World, tab: EditorSidebarTab) -> Option<Entity> {
    world.query::<(Entity, &SceneEditorSidebarTabButton, &ComputedNode, &InheritedVisibility)>()
        .iter(world).find_map(|(entity, button, node, v)| (button.0 == tab && visible(node, v)).then_some(entity))
}

fn click_button(world: &mut World, run: &mut Run, action: SceneMotionButton) -> Result<bool, String> {
    let entity = button(world, action);
    click(world, run, entity)
}

fn click(world: &mut World, run: &mut Run, entity: Option<Entity>) -> Result<bool, String> {
    click_with_button(world, run, entity, MouseButton::Left)
}

fn click_with_button(world: &mut World, run: &mut Run, entity: Option<Entity>, button: MouseButton) -> Result<bool, String> {
    let position = run.click.map(|(position, _)| position)
        .or_else(|| Some(world.get::<UiGlobalTransform>(entity?)?.translation.as_dvec2()));
    let Some(position) = position else { return Ok(false); };
    let window = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world).map_err(|error| error.to_string())?;
    Ok(pointer_click(world, &mut run.click, window, position, button))
}
