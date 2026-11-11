//! 自有沈照雪夹具的真窗口验收，走原按钮和鼠标手势，核对落盘及两段动作。

use super::super::{
    AssetPreviewState, AssetPreviewWorkspace, OpenAssetPreview,
    animation::rig_animation_probe,
    rigging::{RigAction, RiggingState},
    types::{AssetPreviewCamera, AssetPreviewCanvas, AssetPreviewContent, AssetPreviewPhase},
};
use super::{edit::Tool, influences::VertexWeights, panel::Action, resources::WeightEditor};
use crate::{
    asset_catalog::{EditorAssetCatalog, request_asset_reveal},
    components::SceneEditorAssetRow,
    resources::SceneEditorState,
    ui::{
        sidebars::{
            EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarState,
            SceneEditorSidebarTabButton,
        },
        vegetation_preset_editor::VegetationPresetEditorState,
    },
    viewport::placement_move::MoveCamera,
};
use bevy::{
    app::AppExit,
    input::{ButtonState, mouse::MouseButtonInput},
    math::DVec2,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    ui::{InteractionDisabled, UiGlobalTransform},
    window::{CursorMoved, PrimaryWindow, WindowEvent},
};
use feathrise_editor_ui::{EditorButtonClick, UiInteraction};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

const MODEL: &str = "人形_已绑骨.glb";
const UNBOUND: &str = "人形.glb";
const IDLE: &str = "01-待机.glb";
const WALK: &str = "02-行走.glb";
const BRUSHES: [Tool; 3] = [Tool::Add, Tool::Subtract, Tool::Smooth];

#[derive(Clone, Copy, Debug)]
enum Phase {
    Locate,
    Open,
    RigLaunch,
    RigReference,
    RigBind,
    RigPublished,
    Enter,
    HideSkeleton,
    ShowSkeleton,
    BoxTool,
    Aim,
    Press,
    Extend,
    Release,
    Set,
    Verify,
    Undo,
    Redo,
    BrushSetup,
    BrushAim,
    BrushPress,
    BrushMove,
    BrushRelease,
    BrushVerify,
    HeatImage,
    Save,
    IdlePicker,
    IdleSelect,
    IdlePlay,
    IdlePose,
    Compare,
    Restore,
    RestoreDraft,
    WalkPicker,
    WalkSelect,
    WalkPlay,
    WalkPose,
    WalkImage,
    OpenCopy,
    Finish,
}

#[derive(Resource)]
struct Run {
    root: PathBuf,
    assets: PathBuf,
    source: &'static str,
    inputs: [Vec<u8>; 3],
    generated: Option<(String, Vec<u8>)>,
    phase: Phase,
    started: Instant,
    phase_started: Instant,
    click: Option<Entity>,
    asset_click: Option<(Entity, bool)>,
    cursor: Vec2,
    selected: usize,
    before: Option<Arc<[VertexWeights]>>,
    after: Option<Arc<[VertexWeights]>>,
    held: u32,
    brush_changes: Vec<usize>,
    frame_samples: Vec<f32>,
    output: Option<String>,
    idle: Option<Value>,
    walk: Option<Value>,
    compare_time: Option<f32>,
    pose: Option<(String, f32)>,
}

pub(in crate::ui::asset_preview) fn install(
    app: &mut App,
    root: PathBuf,
    assets: PathBuf,
) -> Result<(), String> {
    if std::env::var_os("FEATHRISE_RIG_VALIDATION_PROBLEM_FRAME").is_some() {
        return super::problem_validation::install(app, root, assets);
    }
    let hashes = |name| {
        fs::read(assets.join(name))
            .map(|bytes| Sha256::digest(bytes).to_vec())
            .map_err(|error| error.to_string())
    };
    let source = if std::env::var_os("FEATHRISE_RIG_VALIDATION_WEIGHTS_FROM_RIG").is_some() {
        UNBOUND
    } else {
        MODEL
    };
    let inputs = [hashes(source)?, hashes(IDLE)?, hashes(WALK)?];
    if [
        "weights.json",
        "weights-failure.json",
        "weights-heat.png",
        "weights-walk.png",
    ]
    .iter()
    .any(|name| root.join(name).exists())
    {
        return Err("权重验收需使用新夹具".into());
    }
    app.insert_resource(Run {
        root,
        assets,
        source,
        inputs,
        generated: None,
        phase: Phase::Locate,
        started: Instant::now(),
        phase_started: Instant::now(),
        click: None,
        asset_click: None,
        cursor: Vec2::ZERO,
        selected: 0,
        before: None,
        after: None,
        held: 0,
        brush_changes: Vec::new(),
        frame_samples: Vec::new(),
        output: None,
        idle: None,
        walk: None,
        compare_time: None,
        pose: None,
    })
    .add_systems(Update, focus.before(crate::SceneEditorSystemSet::Input))
    .add_systems(Last, drive.after(bevy::gizmos::GizmoMeshSystems));
    Ok(())
}

pub(super) fn focus(mut window: Query<&mut Window, With<PrimaryWindow>>) {
    if let Ok(mut window) = window.single_mut()
        && !window.focused
    {
        window.focused = true;
    }
}

fn click<T: Component>(world: &mut World, run: &mut Run, matches: impl Fn(&T) -> bool) -> bool {
    let entity = world
        .query::<(
            Entity,
            &T,
            &ComputedNode,
            &Node,
            Option<&InteractionDisabled>,
            &EditorButtonClick,
        )>()
        .iter(world)
        .find(|(_, role, computed, node, disabled, _)| {
            matches(role)
                && computed.size().min_element() > 1.0
                && node.display != Display::None
                && disabled.is_none()
        })
        .map(|(entity, ..)| entity);
    let Some(entity) = entity else { return false };
    *world.get_mut::<EditorButtonClick>(entity).unwrap() = EditorButtonClick::triggered();
    run.click = Some(entity);
    true
}

fn button(world: &mut World, run: &mut Run, action: Action) -> bool {
    click(world, run, |button: &Action| *button == action)
}
fn rig(world: &mut World, run: &mut Run, action: RigAction) -> bool {
    click(world, run, |button: &RigAction| *button == action)
}
fn sidebar(world: &mut World, run: &mut Run, tab: EditorRightSidebarTab) -> bool {
    click(world, run, |button: &SceneEditorSidebarTabButton| {
        button.0 == EditorSidebarTab::Right(tab)
    })
}

pub(super) fn move_cursor(world: &mut World, window: Entity, normalized: Vec2) -> Option<()> {
    let (size, center) = world
        .query_filtered::<(&ComputedNode, &UiGlobalTransform), With<AssetPreviewCanvas>>()
        .iter(world)
        .find(|(node, _)| node.size().min_element() > 16.0)
        .map(|(node, transform)| (node.size(), transform.affine().translation))?;
    move_physical_cursor(world, window, (center + normalized * size).as_dvec2())
}

pub(super) fn move_physical_cursor(
    world: &mut World,
    window: Entity,
    physical: DVec2,
) -> Option<()> {
    let logical = {
        let mut window = world.get_mut::<Window>(window)?;
        window.set_physical_cursor_position(Some(physical));
        physical.as_vec2() / window.scale_factor()
    };
    let event = CursorMoved {
        window,
        position: logical,
        delta: None,
    };
    world.write_message(WindowEvent::CursorMoved(event.clone()));
    world.write_message(event);
    Some(())
}

fn choose_reference(world: &mut World, run: &mut Run, window: Entity) -> Option<()> {
    let (entity, down) = match run.asset_click {
        Some(click) => click,
        None => {
            let index = world
                .resource::<EditorAssetCatalog>()
                .entries
                .iter()
                .position(|entry| entry.asset_path == IDLE)?;
            let entity = world
                .query::<(Entity, &SceneEditorAssetRow, &ComputedNode)>()
                .iter(world)
                .find(|(_, row, node)| row.entry_index == Some(index) && !node.is_empty())?
                .0;
            (entity, false)
        }
    };
    let cursor = world.get::<UiGlobalTransform>(entity)?.affine().translation;
    move_physical_cursor(world, window, cursor.as_dvec2())?;
    match (down, world.get::<UiInteraction>(entity)?) {
        (false, UiInteraction::Hovered) => {
            press(world, window, ButtonState::Pressed);
            run.asset_click = Some((entity, true));
        }
        (true, UiInteraction::Pressed) => {
            press(world, window, ButtonState::Released);
            run.asset_click = None;
            return Some(());
        }
        _ => run.asset_click = Some((entity, down)),
    }
    None
}

pub(super) fn press(world: &mut World, window: Entity, state: ButtonState) {
    let event = MouseButtonInput {
        window,
        button: MouseButton::Left,
        state,
    };
    world.write_message(WindowEvent::from(event.clone()));
    world.write_message(event);
}

fn current_weights(world: &World) -> Option<Arc<[VertexWeights]>> {
    let state = world.get_resource::<WeightEditor>()?;
    if state.work.is_some() || state.stroke.is_some() {
        return None;
    }
    let session = state.session.as_ref()?;
    Some(session.drafts[session.surface].weights.clone())
}

fn view(world: &mut World) -> Option<MoveCamera> {
    let (size, scale) = world
        .query_filtered::<&ComputedNode, With<AssetPreviewCanvas>>()
        .iter(world)
        .find(|node| node.size().min_element() > 16.0)
        .map(|node| (node.size(), node.inverse_scale_factor()))?;
    let (projection, transform) = world
        .query_filtered::<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>()
        .single(world)
        .ok()?;
    MoveCamera::from_projection(projection, transform, size, scale)
}

fn visible_point(world: &mut World) -> Option<Vec2> {
    let view = view(world)?;
    let session = world.resource::<WeightEditor>().session.as_ref()?;
    let surface = &session.doc.surfaces[session.surface];
    let draft = &session.drafts[session.surface];
    surface
        .points
        .positions
        .iter()
        .enumerate()
        .step_by(17)
        .filter_map(|(index, &point)| {
            let projected = view.project(point)?;
            if projected.abs().max_element() > 0.30
                || draft.weights[index].weight(session.joint) > 0.90
            {
                return None;
            }
            let ray = view.ray(projected)?;
            session
                .doc
                .occlusion
                .visible(ray, point, 0.001)
                .then_some(projected)
        })
        .min_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))
}

fn observed(world: &World, run: &mut Run, wanted: &str) -> Option<Value> {
    let pose = rig_animation_probe(world)?;
    if pose.source.file_name()?.to_str()? != wanted || pose.joints.len() != 65 {
        return None;
    }
    if !super::preview::drawn(world, pose.root, false) {
        return None;
    }
    match &run.pose {
        Some((old, time)) if old == wanted && (pose.time - *time).abs() >= 0.08 => {
            run.pose = None;
            Some(
                json!({"clip": pose.clip_name, "time": pose.time, "root": format!("{:?}", pose.root)}),
            )
        }
        Some((old, _)) if old == wanted => None,
        _ => {
            run.pose = Some((wanted.to_owned(), pose.time));
            None
        }
    }
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        if let Some(entity) = run.click.take()
            && let Some(mut click) = world.get_mut::<EditorButtonClick>(entity)
        {
            click.invalidate();
        }
        let result = if run.started.elapsed() > Duration::from_secs(150)
            || run.phase_started.elapsed() > Duration::from_secs(18)
        {
            let state = world.resource::<WeightEditor>();
            let preview = world.resource::<AssetPreviewState>();
            let sidebar = world.resource::<SceneEditorSidebarState>();
            Err(format!(
                "权重验收等待 {:?} 超时，{:?} {:?}，草稿 {}，任务 {}，笔画 {}，骨架绘制 {}，{}",
                run.phase,
                sidebar.right,
                preview.selected_path,
                state.session.is_some(),
                state.work.is_some(),
                state.stroke.is_some(),
                super::skeleton::submitted(world),
                state.notice
            ))
        } else {
            advance(world, &mut run)
        };
        match result {
            Ok(Some(report)) => {
                let path = run.root.join("weights.json");
                if fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).is_ok() {
                    world.write_message(AppExit::Success);
                } else {
                    world.write_message(AppExit::error());
                }
            }
            Err(error) => {
                eprintln!("{error}");
                let _ = fs::write(
                    run.root.join("weights-failure.json"),
                    serde_json::to_vec_pretty(
                        &json!({"phase":format!("{:?}",run.phase),"error":error}),
                    )
                    .unwrap(),
                );
                world.write_message(AppExit::error());
            }
            Ok(None) => {}
        }
    });
}

fn advance(world: &mut World, run: &mut Run) -> Result<Option<Value>, String> {
    let Some(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .iter(world)
        .next()
    else {
        return Ok(None);
    };
    let phase = run.phase;
    let step = match phase {
        Phase::Locate if world.resource::<SceneEditorState>().open => {
            world.resource_scope(|world, mut catalog: Mut<EditorAssetCatalog>| {
                world.resource_scope(|world, mut editor: Mut<SceneEditorState>| {
                    request_asset_reveal(
                        &mut catalog,
                        &mut editor,
                        &mut world.resource_mut::<SceneEditorSidebarState>(),
                        run.source,
                    )
                })
            })?;
            Phase::Open
        }
        Phase::Open
            if world
                .resource::<SceneEditorState>()
                .selected_asset
                .as_deref()
                == Some(run.source) =>
        {
            world.write_message(OpenAssetPreview {
                path: run.source.into(),
            });
            if run.source == UNBOUND {
                Phase::RigLaunch
            } else {
                Phase::Enter
            }
        }
        Phase::RigLaunch if rig(world, run, RigAction::Launch) => Phase::RigReference,
        Phase::RigReference => {
            if !world.resource::<RiggingState>().validation_status().open
                || choose_reference(world, run, window).is_none()
            {
                return Ok(None);
            }
            Phase::RigBind
        }
        Phase::RigBind => {
            let status = world.resource::<RiggingState>().validation_status();
            if !status.native
                || status.busy
                || status.reference != Some(IDLE)
                || status.landmarks != 17
                || !rig(world, run, RigAction::Bind)
            {
                return Ok(None);
            }
            Phase::RigPublished
        }
        Phase::RigPublished => {
            let status = world.resource::<RiggingState>().validation_status();
            if let Some(error) = status.error {
                return Err(error.to_owned());
            }
            let Some(path) = status.output.filter(|_| !status.busy) else {
                return Ok(None);
            };
            let bytes = fs::read(run.assets.join(path)).map_err(|error| error.to_string())?;
            run.generated = Some((path.to_owned(), Sha256::digest(bytes).to_vec()));
            Phase::Enter
        }
        Phase::Enter if button(world, run, Action::Open) => Phase::HideSkeleton,
        Phase::HideSkeleton => {
            let expected = run.generated.as_ref().map_or(run.source, |(path, _)| path);
            let Some(session) = world
                .resource::<WeightEditor>()
                .session
                .as_ref()
                .filter(|session| session.key.path == expected)
            else {
                return Ok(None);
            };
            if !session.show_skeleton {
                return Err("进入权重页时骨架没有默认显示".into());
            }
            if !super::skeleton::submitted(world) || !button(world, run, Action::Skeleton) {
                return Ok(None);
            }
            Phase::ShowSkeleton
        }
        Phase::ShowSkeleton
            if world.resource::<WeightEditor>().session.as_ref()
                .is_some_and(|session| !session.show_skeleton)
                && !super::skeleton::submitted(world)
                && button(world, run, Action::Skeleton) =>
        {
            Phase::BoxTool
        }
        Phase::BoxTool
            if world.resource::<WeightEditor>().session.as_ref()
                .is_some_and(|session| session.show_skeleton)
                && super::skeleton::submitted(world)
                && button(world, run, Action::Mode(Tool::Box)) =>
        {
            Phase::Aim
        }
        Phase::Aim => {
            let Some(cursor) = visible_point(world) else {
                return Ok(None);
            };
            run.cursor = cursor;
            move_cursor(world, window, cursor - Vec2::splat(0.035));
            Phase::Press
        }
        Phase::Press => {
            move_cursor(world, window, run.cursor - Vec2::splat(0.035));
            press(world, window, ButtonState::Pressed);
            Phase::Extend
        }
        Phase::Extend => {
            move_cursor(world, window, run.cursor + Vec2::splat(0.035));
            if world.resource::<WeightEditor>().stroke.is_none() {
                return Err("框选未捕获真实视口手势".into());
            }
            Phase::Release
        }
        Phase::Release => {
            press(world, window, ButtonState::Released);
            Phase::Set
        }
        Phase::Set => {
            let Some(before) = current_weights(world) else {
                return Ok(None);
            };
            let session = world.resource::<WeightEditor>().session.as_ref().unwrap();
            let count = session.drafts[session.surface].selection.len();
            if count == 0 || !button(world, run, Action::Apply) {
                return Ok(None);
            }
            run.selected = count;
            run.before = Some(before);
            Phase::Verify
        }
        Phase::Verify => {
            let Some(after) = current_weights(world) else {
                return Ok(None);
            };
            let session = world.resource::<WeightEditor>().session.as_ref().unwrap();
            let selected = &session.drafts[session.surface].selection;
            let before = run.before.as_ref().unwrap();
            if Arc::ptr_eq(before, &after) {
                return Ok(None);
            }
            if before
                .iter()
                .zip(after.iter())
                .enumerate()
                .any(|(index, (a, b))| a != b && selected.binary_search(&(index as u32)).is_err())
            {
                return Err("数值调权越过框选范围".into());
            }
            if !button(world, run, Action::Undo) {
                return Ok(None);
            }
            run.after = Some(after);
            Phase::Undo
        }
        Phase::Undo
            if current_weights(world)
                .is_some_and(|value| Arc::ptr_eq(&value, run.before.as_ref().unwrap()))
                && button(world, run, Action::Redo) =>
        {
            Phase::Redo
        }
        Phase::Redo
            if current_weights(world)
                .is_some_and(|value| Arc::ptr_eq(&value, run.after.as_ref().unwrap())) =>
        {
            Phase::BrushSetup
        }
        Phase::BrushSetup => {
            let Some(weights) = current_weights(world) else {
                return Ok(None);
            };
            let tool = BRUSHES[run.brush_changes.len()];
            let session = world.resource::<WeightEditor>().session.as_ref().unwrap();
            let mirror = run.generated.is_some() && tool == Tool::Subtract;
            if session.mirror != mirror {
                button(world, run, Action::Mirror);
                return Ok(None);
            }
            if session.tool != tool {
                button(world, run, Action::Mode(tool));
                return Ok(None);
            }
            run.after = Some(weights);
            run.held = 0;
            Phase::BrushAim
        }
        Phase::BrushAim => {
            move_cursor(world, window, run.cursor);
            Phase::BrushPress
        }
        Phase::BrushPress => {
            move_cursor(world, window, run.cursor);
            press(world, window, ButtonState::Pressed);
            Phase::BrushMove
        }
        Phase::BrushMove => {
            if world.resource::<WeightEditor>().stroke.is_none() {
                return Err("画笔未捕获真实视口手势".into());
            }
            if let Some(time) = world.get_resource::<Time<Real>>() {
                run.frame_samples.push(time.delta_secs() * 1000.0);
            }
            run.held += 1;
            move_cursor(
                world,
                window,
                run.cursor + Vec2::X * (run.held as f32 * 0.002),
            );
            if run.held >= 18 {
                Phase::BrushRelease
            } else {
                return Ok(None);
            }
        }
        Phase::BrushRelease => {
            press(world, window, ButtonState::Released);
            Phase::BrushVerify
        }
        Phase::BrushVerify => {
            let Some(weights) = current_weights(world) else {
                return Ok(None);
            };
            let session = world.resource::<WeightEditor>().session.as_ref().unwrap();
            let tool = BRUSHES[run.brush_changes.len()];
            let before = run.after.as_ref().unwrap();
            let joint = session.joint;
            let changed = before
                .iter()
                .zip(weights.iter())
                .filter(|(a, b)| {
                    let delta = b.weight(joint) - a.weight(joint);
                    match tool {
                        Tool::Add => delta > 1e-6,
                        Tool::Subtract => delta < -1e-6,
                        Tool::Smooth => delta.abs() > 1e-6,
                        _ => false,
                    }
                })
                .count();
            if changed == 0 || session.undo.len() != run.brush_changes.len() + 2 {
                return Err(format!("{tool:?} 未修改权重或没有以一条历史提交"));
            }
            run.brush_changes.push(changed);
            if run.brush_changes.len() < BRUSHES.len() {
                run.phase_started = Instant::now();
                run.phase = Phase::BrushSetup;
                return Ok(None);
            }
            let path = run.root.join("weights-heat.png");
            world
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
            Phase::HeatImage
        }
        Phase::HeatImage
            if run.root.join("weights-heat.png").is_file() && button(world, run, Action::Save) =>
        {
            Phase::Save
        }
        Phase::Save => {
            let Some(path) = world
                .resource::<WeightEditor>()
                .session
                .as_ref()
                .and_then(|session| session.saved_path.clone())
            else {
                return Ok(None);
            };
            if !sidebar(world, run, EditorRightSidebarTab::Scene) {
                return Ok(None);
            }
            run.output = Some(path);
            Phase::IdlePicker
        }
        Phase::IdlePicker if rig(world, run, RigAction::ChooseProjectMotion) => Phase::IdleSelect,
        Phase::IdleSelect if rig(world, run, RigAction::Choose(0)) => Phase::IdlePlay,
        Phase::IdlePlay
            if world
                .resource::<RiggingState>()
                .validation_status()
                .animation
                == Some(IDLE)
                && rig(world, run, RigAction::Preview) =>
        {
            Phase::IdlePose
        }
        Phase::IdlePose => {
            let Some(idle) = observed(world, run, IDLE) else {
                return Ok(None);
            };
            if !sidebar(world, run, EditorRightSidebarTab::Tools) {
                return Ok(None);
            }
            run.idle = Some(idle);
            Phase::Compare
        }
        Phase::Compare if button(world, run, Action::Compare) => Phase::Restore,
        Phase::Restore => {
            let Some(pose) = rig_animation_probe(world) else {
                return Ok(None);
            };
            if !world
                .resource::<WeightEditor>()
                .session
                .as_ref()
                .is_some_and(|session| session.original)
            {
                return Ok(None);
            }
            if !super::preview::drawn(world, pose.root, true) {
                return Ok(None);
            }
            if let Some(time) = run.compare_time {
                if (pose.time - time).abs() > 0.001 {
                    return Err("A/B 对照期间时间轴发生跳帧".into());
                }
                if !button(world, run, Action::Compare) {
                    return Ok(None);
                }
                Phase::RestoreDraft
            } else {
                run.compare_time = Some(pose.time);
                return Ok(None);
            }
        }
        Phase::RestoreDraft => {
            let Some(pose) = rig_animation_probe(world) else {
                return Ok(None);
            };
            if !super::preview::drawn(world, pose.root, false) {
                return Ok(None);
            }
            if (pose.time - run.compare_time.unwrap()).abs() > 0.001 {
                return Err(format!(
                    "切回草稿时发生跳帧：{} -> {}",
                    run.compare_time.unwrap(),
                    pose.time
                ));
            }
            Phase::WalkPicker
        }
        Phase::WalkPicker if sidebar(world, run, EditorRightSidebarTab::Scene) => Phase::WalkSelect,
        Phase::WalkSelect => {
            if rig(world, run, RigAction::Choose(1)) {
                Phase::WalkPlay
            } else {
                rig(world, run, RigAction::PickFile);
                return Ok(None);
            }
        }
        Phase::WalkPlay
            if world
                .resource::<RiggingState>()
                .validation_status()
                .animation
                == Some(WALK)
                && rig(world, run, RigAction::Preview) =>
        {
            Phase::WalkPose
        }
        Phase::WalkPose => {
            if observed(world, run, WALK).is_none()
                || !sidebar(world, run, EditorRightSidebarTab::Tools)
            {
                return Ok(None);
            }
            Phase::WalkImage
        }
        Phase::WalkImage
            if world.resource::<SceneEditorSidebarState>().right == EditorRightSidebarTab::Weights
                && super::skeleton::submitted(world) =>
        {
            // 首次提交线条后继续等行走姿态推进，让截图覆盖完成渲染的骨架与模型。
            let Some(walk) = observed(world, run, WALK) else {
                return Ok(None);
            };
            run.walk = Some(walk);
            let path = run.root.join("weights-walk.png");
            world
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path));
            Phase::OpenCopy
        }
        Phase::OpenCopy
            if run.root.join("weights-walk.png").is_file()
                && button(world, run, Action::OpenSaved) =>
        {
            Phase::Finish
        }
        Phase::Finish => {
            let output = run.output.as_deref().ok_or("缺少保存回执")?;
            let current = world.resource::<AssetPreviewState>();
            if current.selected_path.as_deref() != Some(output)
                || !matches!(
                    current.content,
                    AssetPreviewContent::Model {
                        phase: AssetPreviewPhase::Ready,
                        ..
                    }
                )
            {
                return Ok(None);
            }
            let active = world.resource::<AssetPreviewWorkspace>();
            let workspace = world.resource::<VegetationPresetEditorState>();
            if super::super::gltf_tree::central_glb(active, workspace.active_tab) != Some(output) {
                return Ok(None);
            }
            for (index, name) in [run.source, IDLE, WALK].into_iter().enumerate() {
                let original =
                    fs::read(run.assets.join(name)).map_err(|error| error.to_string())?;
                if Sha256::digest(original).as_slice() != run.inputs[index] {
                    return Err(format!("输入 GLB 被改写：{name}"));
                }
            }
            if let Some((path, digest)) = &run.generated {
                let generated =
                    fs::read(run.assets.join(path)).map_err(|error| error.to_string())?;
                if Sha256::digest(generated).as_slice() != digest {
                    return Err("自动绑骨输出被改写".into());
                }
            }
            let output_doc =
                gltf::Gltf::open(run.assets.join(output)).map_err(|error| error.to_string())?;
            if output_doc.skins().next().map(|skin| skin.joints().count()) != Some(65) {
                return Err("重开副本缺少 65 关节".into());
            }
            let session = world
                .resource::<WeightEditor>()
                .session
                .as_ref()
                .ok_or("缺少已保存草稿")?;
            for (surface, draft) in session.doc.surfaces.iter().zip(&session.drafts) {
                let primitive = output_doc
                    .meshes()
                    .nth(surface.mesh)
                    .and_then(|mesh| mesh.primitives().nth(surface.primitive))
                    .ok_or("副本缺少目标图元")?;
                let reader = primitive.reader(|_| output_doc.blob.as_deref());
                let joints = reader.read_joints(0).ok_or("副本缺少 JOINTS_0")?.into_u16();
                let weights = reader
                    .read_weights(0)
                    .ok_or("副本缺少 WEIGHTS_0")?
                    .into_f32();
                if joints.len() != draft.weights.len()
                    || weights.len() != draft.weights.len()
                    || joints.zip(weights).zip(draft.weights.iter()).any(
                        |((joints, weights), expected)| {
                            joints != expected.joints || weights != expected.weights
                        },
                    )
                {
                    return Err("副本权重与已保存的草稿不一致".into());
                }
            }
            run.frame_samples.sort_by(f32::total_cmp);
            return Ok(Some(json!({
                "passed": true, "os_input_verified": false, "selected_vertices": run.selected,
                "source": run.source, "generated": run.generated.as_ref().map(|(path, _)| path),
                "output": output, "reopened": true, "idle": run.idle, "walk": run.walk,
                "brush_changes": {"add": run.brush_changes[0], "subtract": run.brush_changes[1], "smooth": run.brush_changes[2]},
                "ab_paused_time": run.compare_time, "brush_frame_ms": run.frame_samples,
                "screenshots": ["weights-heat.png", "weights-walk.png"]
            })));
        }
        _ => return Ok(None),
    };
    eprintln!("权重验收 {:?} -> {step:?}", run.phase);
    run.phase = step;
    run.phase_started = Instant::now();
    Ok(None)
}
