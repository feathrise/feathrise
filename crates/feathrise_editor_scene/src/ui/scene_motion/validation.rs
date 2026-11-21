//! 独占工程通过原视口手柄、时间轴和保存入口验收主 GLB 动画。

use bevy::{
    app::AppExit, math::DVec2, prelude::*, render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready},
    ui::{InteractionDisabled, RelativeCursorPosition, UiGlobalTransform}, window::PrimaryWindow,
};
use feathrise_editor_animation_model::data::AnimationDocument;
use feathrise_editor_ui::{EditorUiSystemSet, NumericInputCommitOutcome, NumericInputCommitSource,
    NumericInputCommitted, NumericInputDisabled, NumericInputValue, NumericValue};
use serde_json::{Value, json};
use std::{path::{Path, PathBuf}, time::{Duration, Instant}};
use crate::{
    SceneEditorSystemSet, authoring::EditorAuthoringSession, components::SceneEditorViewport,
    resources::{SceneEditorState, SceneEditorViewportState},
    standalone::{author_status, preview_ready_for, request_open_scene,
        scene_format_validation::{action_button, visible_placement_row}},
    types::{EditorAction, EditorView},
    ui::{
        file_manager_validation::pointer_click,
        gltf_browser::{EditorGltfBrowserState, MainSceneGlbRow, PlacementTreeExpander,
            SceneEditorGltfBrowserRow, SceneEditorGltfBrowserRowText, SceneMeshObjectsHeading},
        placement_transform::PlacementTransformField,
        sidebars::{EditorNavigatorTab, EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarTabButton,
            SceneEditorSidebarState},
    },
    viewport::{modes::{ViewportMode, ViewportModeState, validation_press_mode},
        placement_move::{validation_animation_drag_active, validation_animation_drag_points}},
};
pub use super::{SceneMotionButton, SceneMotionField};
use super::{Channel, Workbench};

const SCENE: &str = "M1-SAMPLE";
const PLATFORM: &str = "validation-elevator";
const REFERENCED_MODEL: &str = "sample-model";
const END_FRAME: f32 = 120.0;
const PEAK_FRAME: f32 = 105.0;
const HEIGHT: f32 = 5.0;
const TURN: f32 = 60.0;
const TURN_X_ON_TRIAL: f32 = 12.0;

#[derive(Debug, Clone, Copy)]
enum Step {
    Enter, Tree, MainGlb, MainGlbExpanded, Select, FoldSelected, UnfoldSelected, Properties,
    Open, Add, Length, Loop, PingPong, AutoPlay,
    KeyOrigin, SeekLow, KeyLow, SeekPeak, MoveTool, BeginMove, Move, ReleaseMove, PositionTrack, KeyPosition,
    RotateTool, BeginTurn, Turn, ReleaseTurn, EditRotationProperty, RestoreRotationProperty,
    RotationTrack, KeyRotation, Undo, Redo,
    First, Play, Playback, Review, Verify, Save, Receipt, Capture, Finish,
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
    scene_glb_tree: Option<Value>,
    property_pose_verified: bool,
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
        drag_end: Vec2::ZERO, save_before: 0, scene_glb_tree: None, property_pose_verified: false,
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
        let result = if run.started.elapsed() > Duration::from_secs(120) {
            Err(format!("场景动画验收超时于 {:?}：{}", run.step, author_status(world).unwrap_or_default()))
        } else { advance(world, &mut run) };
        let report = match result {
            Ok(false) => return,
            Ok(true) => run.report.clone(),
            Err(error) => json!({"passed":false,"reopened":run.reopen,"placement_id":PLATFORM,
                "phase":format!("{:?}",run.step),"error":error,"observed":probe(world,PLATFORM)}),
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
                run.step = Tree;
            } else if world.resource::<SceneEditorState>().open { let _ = request_open_scene(world, SCENE); }
        }
        Tree => {
            let tab = sidebar(world, EditorSidebarTab::Navigator(EditorNavigatorTab::SceneTree));
            if (world.resource::<SceneEditorSidebarState>().scene_tree_visible() && run.click.is_none())
                || click(world, run, tab)? { run.step = MainGlb; }
        }
        MainGlb => {
            let arrow = main_glb_arrow(world);
            if run.click.is_none() {
                if arrow.is_none() { return Ok(false); }
                let scene = feathrise_scene_content::SceneId::parse(SCENE)?;
                if visible_placement_row(world, &scene, REFERENCED_MODEL).is_none() { return Ok(false); }
                if visible_placement_row(world, &scene, PLATFORM).is_some()
                    || main_glb_row(world, MainSceneGlbRow::Animation).is_some()
                {
                    return Err("主场景 GLB 折叠时只能隐藏所属立方体和动画，独立引用应仍可见".into());
                }
            }
            // 只点击树行的正式箭头，展开结果须由生产场景树实际呈现。
            if click(world, run, arrow)? { run.step = MainGlbExpanded; }
        }
        MainGlbExpanded => {
            if let Some(proof) = inspect_main_glb_tree(world, run.reopen)? {
                run.scene_glb_tree = Some(proof);
                run.step = Select;
            }
        }
        Select => {
            let scene = feathrise_scene_content::SceneId::parse(SCENE)?;
            let row = visible_placement_row(world, &scene, PLATFORM).map(|(row, _)| row);
            if click(world, run, row)? { run.step = if run.reopen { FoldSelected } else { Properties }; }
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
                    || visible_placement_row(world, &scene, REFERENCED_MODEL).is_none()
                {
                    return Ok(false);
                }
                if !platform_selected(world) {
                    return Err("折叠主 GLB 后应保留独立引用与立方体的当前选择".into());
                }
            }
            let arrow = main_glb_arrow(world);
            if click(world, run, arrow)? { run.step = Properties; }
        }
        Properties => {
            if !platform_selected(world) { return Ok(false); }
            if run.reopen && let Some(proof) = &mut run.scene_glb_tree {
                let scene = feathrise_scene_content::SceneId::parse(SCENE)?;
                if visible_placement_row(world, &scene, PLATFORM).is_none() { return Ok(false); }
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
                run.step = Open;
            }
        }
        Open => if click_button(world, run, Action::Open)? { run.step = if run.reopen { Review } else { Add }; },
        Add => if click_button(world, run, Action::Add)? { run.step = Length; },
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
                else { ((3,2), EditorAction::Redo, First) };
            if key_counts(world) != count && run.click.is_none() { return Ok(false); }
            let entity = action_button(world, action);
            if click(world, run, entity)? { run.step = next; }
        }
        First => if key_counts(world) == (3,3) && click_button(world, run, Action::First)? { run.step = Play; },
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
            if !at_frame(world, PEAK_FRAME) || !at_peak(world, true) { return Ok(false); }
            if run.reopen { run.report = verify_report(world, run)?; run.step = Capture; }
            else { run.step = Save; }
        }
        Save => {
            let entity = action_button(world, EditorAction::Save);
            if click(world, run, entity)? { run.step = Receipt; }
        }
        Receipt => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.saving || session.successful_save_sequence <= run.save_before { return Ok(false); }
            run.report = verify_report(world, run)?;
            run.step = Capture;
        }
        Capture => {
            if !preview_ready_for(world, SCENE) || !at_peak(world, true) { return Ok(false); }
            let output = run.directory.join("scene-motion.png");
            world.spawn(Screenshot::primary_window()).observe(move |shot: On<ScreenshotCaptured>, mut commands: Commands| {
                let image = shot.image.clone(); let output = output.clone();
                commands.insert_resource(ImageWrite(IoTaskPool::get().spawn(async move {
                    image.try_into_dynamic().map_err(|error| error.to_string())?.save(output).map_err(|error| error.to_string())
                })));
            });
            run.step = Finish;
        }
        Finish => {
            let Some(result) = world.get_resource_mut::<ImageWrite>().and_then(|mut task| check_ready(&mut task.0)) else {
                return Ok(false);
            };
            result?;
            return Ok(true);
        }
    }
    Ok(false)
}

fn key_counts(world: &World) -> (usize, usize) {
    world.resource::<EditorAuthoringSession>().draft.as_ref().and_then(|draft| draft.motion.get(PLATFORM))
        .map_or((0,0), |track| (track.position.len(), track.rotation.len()))
}

fn key_tracks(world: &World) -> (bool, bool) {
    let state = world.resource::<Workbench>();
    (state.keys_channel(Channel::Position), state.keys_channel(Channel::Rotation))
}

fn clip_settings(world: &World) -> (f32, &'static str, bool) {
    use crate::authoring::scene_motion::SceneMotionPlayback::*;
    world.resource::<EditorAuthoringSession>().draft.as_ref().map_or((0.0, "", false), |draft| {
        let timing = draft.motion.timing();
        let playback = match timing.playback { Once => "once", Loop => "loop", PingPong => "ping_pong" };
        (timing.duration_seconds, playback, timing.auto_play)
    })
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
    let pose = preview_transform(world).ok_or("场景动画画面缺少对象")?;
    Ok(json!({"passed":true,"reopened":run.reopen,"scene_id":draft.scene_id,"placement_id":PLATFORM,
        "track":track,"duration_seconds":4.0,"playback":"ping_pong","auto_play":true,"translation":record.translation,
        "preview_translation":pose.translation(),"preview_rotation":pose.rotation().to_array(),
        "save_sequence":session.successful_save_sequence,"screenshot":"scene-motion.png",
        "scene_glb_tree":run.scene_glb_tree,"property_pose_verified":run.property_pose_verified}))
}

fn main_glb_row(world: &mut World, kind: MainSceneGlbRow) -> Option<Entity> {
    world.query::<(Entity, &SceneEditorGltfBrowserRow, &ComputedNode, &InheritedVisibility)>()
        .iter(world)
        .find_map(|(entity, row, node, v)| (row.main_glb.as_ref() == Some(&kind) && visible(node, v)).then_some(entity))
}

fn tree_arrow(world: &World, row: Entity) -> Option<Entity> {
    world.get::<Children>(row)?.iter().find(|&child| world.get::<PlacementTreeExpander>(child).is_some())
}

fn main_glb_arrow(world: &mut World) -> Option<Entity> {
    let row = main_glb_row(world, MainSceneGlbRow::File)?;
    tree_arrow(world, row).filter(|&arrow| world.get::<InteractionDisabled>(arrow).is_none())
}

fn platform_selected(world: &World) -> bool {
    world.resource::<EditorGltfBrowserState>().selected_placement_id.as_deref() == Some(PLATFORM)
}

fn tree_label(world: &World, row: Entity) -> Option<&str> {
    world.get::<Children>(row)?.iter().filter_map(|child| world.get::<Children>(child))
        .flat_map(|children| children.iter())
        .find(|&child| world.get::<SceneEditorGltfBrowserRowText>(child).is_some())
        .and_then(|child| world.get::<Text>(child))
        .map(|text| text.as_str())
}

fn ui_position(world: &World, entity: Entity) -> Option<Vec2> {
    Some(world.get::<UiGlobalTransform>(entity)?.translation)
}

fn inspect_main_glb_tree(world: &mut World, reopened: bool) -> Result<Option<Value>, String> {
    let heading = world.query_filtered::<Entity, With<SceneMeshObjectsHeading>>().single(world).ok();
    let main = main_glb_row(world, MainSceneGlbRow::File);
    let animation = main_glb_row(world, MainSceneGlbRow::Animation);
    if reopened && animation.is_none() { return Ok(None); }
    let scene = feathrise_scene_content::SceneId::parse(SCENE)?;
    let platform = visible_placement_row(world, &scene, PLATFORM).map(|(row, _)| row);
    let reference = visible_placement_row(world, &scene, REFERENCED_MODEL).map(|(row, _)| row);
    let proof = || {
        let (main, platform, reference) = (main?, platform?, reference?);
        let rows = [heading?, main, platform, reference];
        let ys = rows.map(|row| ui_position(world, row).map(|position| position.y));
        let [Some(heading_y), Some(main_y), Some(platform_y), Some(reference_y)] = ys else {
            return None;
        };
        // 叶子行的箭头列仍占位，比较实际按钮列以避开文件名长度带来的文字中心差异。
        let arrow_x = |row| ui_position(world, tree_arrow(world, row)?).map(|position| position.x);
        let (main_x, platform_x, reference_x) = (arrow_x(main)?, arrow_x(platform)?, arrow_x(reference)?);
        let animation = if let Some(row) = animation {
            let y = ui_position(world, row)?.y;
            let same_depth = (arrow_x(row)? - platform_x).abs() < 1.1;
            Some((tree_label(world, row)?, main_y < y && y < reference_y && same_depth))
        } else { None };
        Some((heading_y < main_y && main_y < platform_y && platform_y < reference_y,
            platform_x > main_x + 1.0, (reference_x - main_x).abs() < 1.1, tree_label(world, main)?, animation))
    };
    let Some((ordered, cuboid_indented, reference_at_root, main_label, animation)) = proof() else {
        return Ok(None);
    };
    if !(ordered && cuboid_indented && reference_at_root && main_label.contains("scene.glb")
        && animation.is_none_or(|(label, inside)| label == "场景动画" && inside))
    {
        return Err(format!("主 GLB 树验证失败，顺序 {ordered}，立方体缩进 {cuboid_indented}，引用根层 {reference_at_root}，动画 {animation:?}"));
    }
    Ok(Some(json!({"main_glb":main_label,"animation":animation.map(|(label, _)| label),"first_mesh_row":ordered,
        "cuboid_indented":cuboid_indented,"animation_indented":animation.map(|(_, inside)| inside),
        "reference_at_root":reference_at_root})))
}

pub fn inspect_saved_glb(path: &Path, id: &str) -> Result<Value, String> {
    let saved = AnimationDocument::load(path)?;
    let clip = saved.clips.iter().find(|clip| saved.clip_extra(clip.source_index, "feathrise_scene_animation")
        .and_then(Value::as_bool) == Some(true)).ok_or("主场景 GLB 缺少场景动画片段")?;
    let auto_play = saved.clip_extra(clip.source_index, "feathrise_autoplay").and_then(Value::as_bool);
    if auto_play != Some(true) { return Err("主场景 GLB 未保存自动播放设置".into()); }
    let channel = |property| clip.channels.iter().find(|channel| channel.path == property
        && saved.node_extra(channel.node,"feathrise_id").and_then(Value::as_str) == Some(id));
    let position = channel("translation").ok_or("主 GLB 没有目标对象的位置通道")?;
    let rotation = channel("rotation").ok_or("主 GLB 没有目标对象的旋转通道")?;
    if rotation.keys.iter().any(|key| key.value.len() != 4) { return Err("主 GLB 旋转通道未保存标准四元数".into()); }
    Ok(json!({"position_keys":position.keys.len(),"rotation_keys":rotation.keys.len(),"auto_play":auto_play,
        "position_times":position.keys.iter().map(|key|key.time).collect::<Vec<_>>(),
        "rotation_times":rotation.keys.iter().map(|key|key.time).collect::<Vec<_>>(),
        "end_position":position.keys.last().map(|key| &key.value),
        "end_rotation":rotation.keys.last().map(|key| &key.value)}))
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
    let position = run.click.map(|(position, _)| position)
        .or_else(|| Some(world.get::<UiGlobalTransform>(entity?)?.translation.as_dvec2()));
    let Some(position) = position else { return Ok(false); };
    let window = world.query_filtered::<Entity, With<PrimaryWindow>>().single(world).map_err(|error| error.to_string())?;
    Ok(pointer_click(world, &mut run.click, window, position, MouseButton::Left))
}
