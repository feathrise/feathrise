//! 沈照雪问题帧验收。计时从权重页已载入同一行走帧开始，通过真实窗口输入执行修权。

use feathrise_editor_glb_model::weights::{Geometry, Tool, VertexWeights};
use super::{
    super::{
        AssetPreviewNavigation, AssetPreviewState, OpenAssetPreview,
        animation::{
            AnimationEditorState, rig_animation_probe, workflow_validation::timeline_ruler,
        },
        rigging::RigAction,
        rigging_validation::{animation_clip_button, animation_file_button},
        types::{AssetPreviewCamera, AssetPreviewCanvas},
        view_math::AssetPreviewView,
        view_toolbar::Action as ViewAction,
    },
    panel::Action,
    preview::{self, WeightPreview},
    resources::WeightEditor,
    validation::{focus, move_cursor, move_physical_cursor, press},
};
use crate::{
    SceneEditorSystemSet,
    asset_catalog::{EditorAssetCatalog, request_asset_reveal},
    components::SceneEditorActionButton,
    resources::SceneEditorState,
    types::{EditorAction, EditorView},
    ui::sidebars::{
        EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarState,
        SceneEditorSidebarTabButton,
    },
    viewport::placement_move::MoveCamera,
};
use bevy::{
    app::AppExit,
    input::ButtonState,
    mesh::skinning::SkinnedMesh,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    ui::{InteractionDisabled, UiGlobalTransform},
    window::PrimaryWindow,
};
use feathrise_editor_ui::UiInteraction;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

const MODEL: &str = "人形_已绑骨.glb";
const WALK: &str = "02-行走.glb";
const INPUT_GAP: Duration = Duration::from_millis(180);

#[derive(Clone, Copy, Debug)]
enum Step {
    Locate,
    Open,
    Enter,
    Edit,
    Front,
    Ortho,
    Properties,
    Picker,
    Choose,
    Play,
    Seek,
    Tools,
    Prepare,
    BoxTool,
    Aim,
    Down,
    Move,
    Up,
    CheckRest,
    Side,
    Remove,
    CheckEdit,
    Return,
    CheckReturn,
    ShowOriginal,
    CheckOriginal,
    ShowDraft,
    CheckDraft,
    Save,
    CheckSave,
    Finish,
}

#[derive(Resource)]
struct Run {
    root: PathBuf,
    assets: PathBuf,
    source_hash: Vec<u8>,
    motion_hash: Vec<u8>,
    step: Step,
    step_started: Instant,
    click: Option<(Entity, bool)>,
    last_release: Option<Instant>,
    timed: Option<Instant>,
    input: Vec<Value>,
    seek: f32,
    view: Option<AssetPreviewView>,
    camera: Option<MoveCamera>,
    pose_root: Option<Entity>,
    joint: u16,
    start: Vec2,
    end: Vec2,
    benchmark_box: Option<[Vec2; 2]>,
    selected: Arc<[u32]>,
    before: Option<Arc<[VertexWeights]>>,
    changed: usize,
    outside_rest_box: usize,
    report: Option<Value>,
}

pub(super) fn install(app: &mut App, root: PathBuf, assets: PathBuf) -> Result<(), String> {
    let hash = |name| -> Result<_, String> {
        let bytes = fs::read(assets.join(name)).map_err(|error| error.to_string())?;
        Ok(Sha256::digest(bytes).to_vec())
    };
    if ["problem.json", "problem-failure.json", "problem-pose.json"]
        .iter()
        .any(|name| root.join(name).exists())
    {
        return Err("问题帧验收需要新的独占夹具".into());
    }
    let box_path = root.join("problem-box.json");
    let benchmark_box = if box_path.is_file() {
        let bytes = fs::read(box_path).map_err(|error| error.to_string())?;
        let [min, max]: [[f32; 2]; 2] =
            serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
        let [min, max] = [Vec2::from_array(min), Vec2::from_array(max)];
        if !min.is_finite()
            || !max.is_finite()
            || min.abs().max_element() >= 0.5
            || max.abs().max_element() >= 0.5
            || !(max - min).cmpgt(Vec2::splat(0.01)).all()
        {
            return Err("对照选区必须在当前画布内".into());
        }
        Some([min, max])
    } else {
        None
    };
    app.insert_resource(Run {
        source_hash: hash(MODEL)?,
        motion_hash: hash(WALK)?,
        root,
        assets,
        step: Step::Locate,
        step_started: Instant::now(),
        click: None,
        last_release: None,
        timed: None,
        input: Vec::new(),
        seek: 0.0,
        view: None,
        camera: None,
        pose_root: None,
        joint: 0,
        start: Vec2::ZERO,
        end: Vec2::ZERO,
        benchmark_box,
        selected: Arc::from([]),
        before: None,
        changed: 0,
        outside_rest_box: 0,
        report: None,
    })
    .add_systems(Update, focus.before(SceneEditorSystemSet::Input))
    .add_systems(Last, drive);
    Ok(())
}

impl Run {
    fn ready_for_input(&self) -> bool {
        self.last_release
            .is_none_or(|last| last.elapsed() >= INPUT_GAP)
    }

    fn released(&mut self, kind: &str) {
        self.last_release = Some(Instant::now());
        if let Some(start) = self.timed {
            self.input.push(json!({"step": format!("{:?}", self.step), "kind": kind, "seconds": start.elapsed().as_secs_f64()}));
        }
    }
}

fn click_entity(world: &mut World, run: &mut Run, window: Entity, entity: Entity) -> bool {
    let Some(transform) = world.get::<UiGlobalTransform>(entity) else {
        return false;
    };
    let center = transform.affine().translation;
    if run.click.is_some() || run.ready_for_input() {
        // 真窗口可能在等待悬停或按下的帧间重报系统光标。
        move_physical_cursor(world, window, center.as_dvec2());
    }
    match run.click {
        None if run.ready_for_input() => {
            run.click = Some((entity, false));
        }
        Some((held, false))
            if held == entity
                && world.get::<UiInteraction>(entity) == Some(&UiInteraction::Hovered) =>
        {
            press(world, window, ButtonState::Pressed);
            run.click = Some((entity, true));
        }
        Some((held, true))
            if held == entity
                && world.get::<UiInteraction>(entity) == Some(&UiInteraction::Pressed) =>
        {
            press(world, window, ButtonState::Released);
            run.click = None;
            run.released("click");
            return true;
        }
        _ => {}
    }
    false
}

fn click<T: Component + PartialEq>(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    role: T,
) -> bool {
    let entity = run.click.map(|(entity, _)| entity).or_else(|| {
        world
            .query::<(Entity, &T, &ComputedNode, &Node, Has<InteractionDisabled>)>()
            .iter(world)
            .find(|(_, button, computed, node, disabled)| {
                **button == role
                    && !computed.is_empty()
                    && node.display != Display::None
                    && !disabled
            })
            .map(|(entity, ..)| entity)
    });
    entity.is_some_and(|entity| click_entity(world, run, window, entity))
}

fn click_header(world: &mut World, run: &mut Run, window: Entity, action: EditorAction) -> bool {
    let entity = run.click.map(|(entity, _)| entity).or_else(|| {
        world
            .query::<(Entity, &SceneEditorActionButton, &ComputedNode, Has<InteractionDisabled>)>()
            .iter(world)
            .find(|(_, button, node, disabled)| button.0 == action && !node.is_empty() && !disabled)
            .map(|(entity, ..)| entity)
    });
    entity.is_some_and(|entity| click_entity(world, run, window, entity))
}

fn click_sidebar(
    world: &mut World,
    run: &mut Run,
    window: Entity,
    tab: EditorRightSidebarTab,
) -> bool {
    let entity = run.click.map(|(entity, _)| entity).or_else(|| {
        world
            .query::<(Entity, &SceneEditorSidebarTabButton, &ComputedNode, &Node, Has<InteractionDisabled>)>()
            .iter(world)
            .find(|(_, button, computed, node, disabled)| {
                button.0 == EditorSidebarTab::Right(tab)
                    && !computed.is_empty()
                    && node.display != Display::None
                    && !disabled
            })
            .map(|(entity, ..)| entity)
    });
    entity.is_some_and(|entity| click_entity(world, run, window, entity))
}

fn projection(world: &mut World) -> Option<(MoveCamera, Mat4, Vec2)> {
    let (size, scale) = world
        .query_filtered::<&ComputedNode, With<AssetPreviewCanvas>>()
        .iter(world)
        .find(|node| !node.is_empty())
        .map(|node| (node.size(), node.inverse_scale_factor()))?;
    let (projection, global) = world
        .query_filtered::<(&Projection, &GlobalTransform), With<AssetPreviewCamera>>()
        .single(world)
        .ok()?;
    let Projection::Orthographic(orthographic) = projection else {
        return None;
    };
    let view = MoveCamera::from_projection(projection, global, size, scale)?;
    Some((view, global.to_matrix(), orthographic.area.size()))
}

fn prepare_pose(world: &mut World, run: &mut Run) -> Result<bool, String> {
    let Some((view, camera, area)) = projection(world) else {
        return Ok(false);
    };
    let Some(pose) = rig_animation_probe(world).filter(|pose| (pose.time - run.seek).abs() < 0.001)
    else {
        return Ok(false);
    };
    let editor = world.resource::<WeightEditor>();
    let Some(session) = editor.session.as_ref().filter(|_| !editor.busy()) else {
        return Ok(false);
    };
    let draw = world.resource::<WeightPreview>();
    let Some((_, parts)) = draw.pose_parts(&session.key, pose.root) else {
        return Ok(false);
    };
    let Some((entity, _)) = parts
        .iter()
        .find(|(_, part)| *part == Geometry::Skin(session.surface))
    else {
        return Ok(false);
    };
    let skin = world
        .get::<SkinnedMesh>(*entity)
        .ok_or("行走预览缺少实际蒙皮")?;
    let source = session.doc.surface(session.surface);
    let joint = session.joint as usize;
    let world_joint = world
        .get::<GlobalTransform>(skin.joints[joint])
        .ok_or("缺少当前骨骼")?
        .translation();
    let center =
        view.project(world_joint).ok_or("当前骨骼不在可见画布内")? + Vec2::new(0.0, -0.025);
    [run.start, run.end] = run.benchmark_box.unwrap_or([
        center - Vec2::new(0.035, 0.045),
        center + Vec2::new(0.035, 0.045),
    ]);
    let transforms = source
        .skin
        .joints
        .iter()
        .zip(&skin.joints)
        .map(|(joint, entity)| {
            let local = world.get::<Transform>(*entity).ok_or("缺少骨骼局部姿态")?;
            Ok(
                json!({"node":joint.node, "name":joint.name, "t":local.translation.to_array(),
            "r":local.rotation.to_array(), "s":local.scale.to_array()}),
            )
        })
        .collect::<Result<Vec<_>, &str>>()?;
    let pose_file = json!({
        "model": MODEL, "action": WALK, "clip": pose.clip_name, "time": pose.time,
        "camera_cols":camera.to_cols_array(), "camera_area":area.to_array(), "canvas":view.size.to_array(),
        "box_min":run.start.to_array(), "box_max":run.end.to_array(),
        "joint": source.skin.joints[joint].name, "joints":transforms,
    });
    fs::write(
        run.root.join("problem-pose.json"),
        serde_json::to_vec_pretty(&pose_file).unwrap(),
    )
    .map_err(|error| error.to_string())?;
    run.view = Some(world.resource::<AssetPreviewNavigation>().view);
    run.camera = Some(view);
    run.seek = pose.time;
    run.pose_root = Some(pose.root);
    run.joint = session.joint;
    run.before = Some(session.drafts[session.surface].weights.clone());
    Ok(true)
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut run: Mut<Run>| {
        let result = if run.step_started.elapsed() > Duration::from_secs(18) {
            Err(format!(
                "等待 {:?} 超时 · {}",
                run.step,
                world.resource::<WeightEditor>().notice
            ))
        } else {
            advance(world, &mut run)
        };
        match result {
            Ok(None) => {}
            Ok(Some(report)) => {
                let file = run.root.join("problem.json");
                let ok = fs::write(file, serde_json::to_vec_pretty(&report).unwrap()).is_ok();
                world.write_message(if ok {
                    AppExit::Success
                } else {
                    AppExit::error()
                });
            }
            Err(error) => {
                eprintln!("{error}");
                let _ = fs::write(
                    run.root.join("problem-failure.json"),
                    serde_json::to_vec_pretty(
                        &json!({"step":format!("{:?}",run.step), "error":error, "input":run.input}),
                    )
                    .unwrap(),
                );
                world.write_message(AppExit::error());
            }
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
    let step = run.step;
    let next = match step {
        Step::Locate if world.resource::<SceneEditorState>().open => {
            world.resource_scope(|world, mut catalog: Mut<EditorAssetCatalog>| {
                world.resource_scope(|world, mut editor: Mut<SceneEditorState>| {
                    request_asset_reveal(
                        &mut catalog,
                        &mut editor,
                        &mut world.resource_mut::<SceneEditorSidebarState>(),
                        MODEL,
                    )
                })
            })?;
            Step::Open
        }
        Step::Open
            if world
                .resource::<SceneEditorState>()
                .selected_asset
                .as_deref()
                == Some(MODEL) =>
        {
            world.write_message(OpenAssetPreview { path: MODEL.into() });
            Step::Enter
        }
        Step::Enter if click(world, run, window, Action::Open) => Step::Edit,
        Step::Edit if click(world, run, window, Action::Edit) => Step::Front,
        Step::Front if click(world, run, window, ViewAction::Direction(EditorView::Front)) => {
            Step::Ortho
        }
        Step::Ortho if click(world, run, window, ViewAction::Orthographic) => Step::Properties,
        Step::Properties if click_sidebar(world, run, window, EditorRightSidebarTab::Scene) => {
            Step::Picker
        }
        Step::Picker if click(world, run, window, RigAction::ChooseProjectMotion) => Step::Choose,
        Step::Choose => {
            let entity = run.click.map(|(entity, _)| entity).or_else(|| animation_file_button(world, WALK));
            if !entity.is_some_and(|entity| click_entity(world, run, window, entity)) {
                return Ok(None);
            }
            Step::Play
        }
        Step::Play => {
            let entity = run.click.map(|(entity, _)| entity).or_else(|| animation_clip_button(world, 0));
            if !entity.is_some_and(|entity| click_entity(world, run, window, entity)) {
                return Ok(None);
            }
            Step::Seek
        }
        Step::Seek => {
            if rig_animation_probe(world).is_none() {
                return Ok(None);
            }
            let Some((ruler, time)) = timeline_ruler(world) else {
                return Ok(None);
            };
            if !click_entity(world, run, window, ruler) {
                return Ok(None);
            }
            run.seek = time;
            Step::Tools
        }
        Step::Tools if click_sidebar(world, run, window, EditorRightSidebarTab::Tools) => {
            Step::Prepare
        }
        Step::Prepare => {
            if !run.ready_for_input() || !prepare_pose(world, run)? {
                return Ok(None);
            }
            run.timed = Some(Instant::now());
            Step::BoxTool
        }
        Step::BoxTool if click(world, run, window, Action::Mode(Tool::Box)) => Step::Aim,
        Step::Aim if run.ready_for_input() => {
            move_cursor(world, window, run.start);
            Step::Down
        }
        Step::Down => {
            move_cursor(world, window, run.start);
            press(world, window, ButtonState::Pressed);
            Step::Move
        }
        Step::Move => {
            if world
                .resource::<WeightEditor>()
                .stroke
                .as_ref()
                .is_none_or(|stroke| stroke.problem.is_none())
            {
                return Err("问题帧框选未从实际动作容器捕获".into());
            }
            move_cursor(world, window, run.end);
            Step::Up
        }
        Step::Up => {
            let stroke = world
                .resource::<WeightEditor>()
                .stroke
                .as_ref()
                .ok_or("框选松键前丢失了手势")?;
            let pixels = stroke.view.size * stroke.view.inverse_scale;
            let reached = ((stroke.latest - run.end) * pixels).length() < 2.0;
            // 真窗口可能在重放间隙刷新光标，等画布消费了末端位置再发送松键。
            move_cursor(world, window, run.end);
            if !reached {
                return Ok(None);
            }
            press(world, window, ButtonState::Released);
            run.released("drag");
            Step::CheckRest
        }
        Step::CheckRest => {
            let state = world.resource::<WeightEditor>();
            let Some(problem) = state.problem.filter(|_| !state.busy()) else {
                return Ok(None);
            };
            let session = state.session.as_ref().ok_or("丢失调权会话")?;
            if !world
                .resource::<AnimationEditorState>()
                .weight_rest_active()
                || session.joint != run.joint
                || problem.frame.time != run.seek
            {
                return Err("静置修权未保留骨骼和精确动作帧".into());
            }
            let root = world
                .resource::<AssetPreviewState>()
                .scene_root()
                .ok_or("缺少静置预览")?;
            if world.get::<Visibility>(root) != Some(&Visibility::Inherited)
                || rig_animation_probe(world).is_some()
            {
                return Ok(None);
            }
            run.selected = session.drafts[session.surface].selection.clone();
            if run.selected.is_empty() {
                return Err("问题帧没有选中顶点".into());
            }
            let points = session.doc.rest_positions(session.surface);
            let camera = run.camera.unwrap();
            run.outside_rest_box = run
                .selected
                .iter()
                .filter(|&&index| {
                    camera.project(points[index as usize]).is_none_or(|point| {
                        !point.cmpge(run.start).all() || !point.cmple(run.end).all()
                    })
                })
                .count();
            if run.outside_rest_box == 0 {
                return Err("验收框选未区分动作姿态与静置位置".into());
            }
            Step::Side
        }
        Step::Side if click(world, run, window, ViewAction::Direction(EditorView::Side)) => {
            Step::Remove
        }
        Step::Remove if click(world, run, window, Action::Remove) => Step::CheckEdit,
        Step::CheckEdit => {
            let state = world.resource::<WeightEditor>();
            let session = state.session.as_ref().ok_or("丢失调权会话")?;
            let draft = &session.drafts[session.surface];
            let before = run.before.as_ref().unwrap();
            if state.busy() || Arc::ptr_eq(before, &draft.weights) {
                return Ok(None);
            }
            if draft.selection != run.selected
                || session.joint != run.joint
                || world.resource::<AssetPreviewNavigation>().view == run.view.unwrap()
            {
                return Err("静置期间选区、骨骼或测试视角不符".into());
            }
            for (index, (_, b)) in before
                .iter()
                .zip(draft.weights.iter())
                .enumerate()
                .filter(|(_, (a, b))| a != b)
            {
                if run.selected.binary_search(&(index as u32)).is_err()
                    || b.weight(run.joint) > 1e-6
                {
                    return Err("移除影响改变了选区外权重或遗漏当前骨骼".into());
                }
                run.changed += 1;
            }
            Step::Return
        }
        Step::Return if click(world, run, window, Action::Rest) => Step::CheckReturn,
        Step::CheckReturn => {
            let Some(pose) = rig_animation_probe(world) else {
                return Ok(None);
            };
            if Some(pose.root) != run.pose_root
                || pose.time != run.seek
                || world.resource::<AssetPreviewNavigation>().view != run.view.unwrap()
            {
                return Err("返回问题帧没有复用原动作帧和视角".into());
            }
            if !preview::drawn(world, pose.root, false) {
                return Ok(None);
            }
            Step::ShowOriginal
        }
        Step::ShowOriginal if click(world, run, window, Action::Compare) => Step::CheckOriginal,
        Step::CheckOriginal => {
            let Some(pose) = rig_animation_probe(world) else {
                return Ok(None);
            };
            if !preview::drawn(world, pose.root, true) {
                return Ok(None);
            }
            if pose.time != run.seek {
                return Err("查看原始权重时发生跳帧".into());
            }
            Step::ShowDraft
        }
        Step::ShowDraft if click(world, run, window, Action::Compare) => Step::CheckDraft,
        Step::CheckDraft => {
            let Some(pose) = rig_animation_probe(world) else {
                return Ok(None);
            };
            if !preview::drawn(world, pose.root, false) {
                return Ok(None);
            }
            if pose.time != run.seek {
                return Err("查看草稿权重时发生跳帧".into());
            }
            Step::Save
        }
        Step::Save if click_header(world, run, window, EditorAction::Save) => Step::CheckSave,
        Step::CheckSave => {
            let state = world.resource::<WeightEditor>();
            let Some(session) = state.session.as_ref().filter(|_| !state.busy()) else {
                return Ok(None);
            };
            let Some(path) = session.saved_path.clone() else {
                return Ok(None);
            };
            let seconds = run.timed.unwrap().elapsed().as_secs_f64();
            let output =
                gltf::Gltf::open(run.assets.join(&path)).map_err(|error| error.to_string())?;
            let surface = session.doc.surface(session.surface);
            let primitive = output
                .meshes()
                .nth(surface.mesh)
                .and_then(|mesh| mesh.primitives().nth(surface.primitive))
                .ok_or("输出 GLB 缺少图元")?;
            let reader = primitive.reader(|_| output.blob.as_deref());
            let weights = reader
                .read_weights(0)
                .ok_or("输出 GLB 缺少权重")?
                .into_f32();
            if weights.len() != session.drafts[session.surface].weights.len()
                || weights
                    .zip(session.drafts[session.surface].weights.iter())
                    .any(|(saved, draft)| saved != draft.weights)
            {
                return Err("输出 GLB 权重与草稿不一致".into());
            }
            for (name, digest) in [(MODEL, &run.source_hash), (WALK, &run.motion_hash)] {
                let bytes = fs::read(run.assets.join(name)).map_err(|error| error.to_string())?;
                if Sha256::digest(bytes).as_slice() != digest {
                    return Err(format!("输入被改写 {name}"));
                }
            }
            world
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(run.root.join("problem-returned.png")));
            run.report = Some(json!({
                "passed":true, "app":"Feathrise", "seconds":seconds, "pacing_ms":INPUT_GAP.as_millis(),
                "clicks":run.input.iter().filter(|event| event["kind"] == "click").count(),
                "drags":run.input.iter().filter(|event| event["kind"] == "drag").count(), "keys":0,
                "source":MODEL, "action":WALK, "pose_seconds":run.seek, "joint":run.joint,
                "selected": run.selected.as_ref(), "changed":run.changed, "output":path,
                "selected_outside_rest_box":run.outside_rest_box,
                "input":"window_event_replay", "events":run.input,
            }));
            Step::Finish
        }
        Step::Finish
            if run
                .root
                .join("problem-returned.png")
                .metadata()
                .is_ok_and(|file| file.len() > 0) =>
        {
            return Ok(run.report.take());
        }
        _ => return Ok(None),
    };
    eprintln!("问题帧验收 {:?} -> {next:?}", run.step);
    run.step = next;
    run.step_started = Instant::now();
    Ok(None)
}
