//! 隔离工程验收：通过原动画控件和数值提交编辑两根骨骼，保存后在新编辑器进程读取。

use super::validation_fixture::ui_visible;
use crate::{
    asset_catalog::{EditorAssetCatalog, request_asset_reveal},
    resources::SceneEditorState,
    ui::{
        asset_preview::{
            AnimationEditorState, AssetPreviewState, AssetPreviewWorkspace, OpenAssetPreview,
            animation::workflow_validation as bridge,
        },
        file_manager_validation::pointer_click,
        sidebars::{EditorRightSidebarTab, SceneEditorSidebarState},
    },
};
use bevy::{
    app::AppExit,
    math::DVec2,
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready},
    ui::UiGlobalTransform,
    window::PrimaryWindow,
};
use bridge::{AnimationCommand as Action, KeyIdentity};
use feathrise_editor_animation_model::{
    media::{AssetPreviewContent, AssetPreviewPhase},
    preview::PreviewRuntime,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

const MODEL: &str = "roles/nested/hero.glb";
const POSE_TIME: f32 = 0.5;
const HIP_X: f32 = 0.25;
const ARM_DEGREES_X: f32 = 35.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Reveal,
    Open,
    Create,
    Enter,
    Seek,
    ManualKeys,
    Hips,
    SetHips,
    Arm,
    SetArm,
    Insert,
    Undo,
    Redo,
    Save,
    Receipt,
    Reopen,
    ReadSavedFrame,
    Capture,
    Finish,
}

#[derive(Resource)]
struct Probe {
    assets: PathBuf,
    output: PathBuf,
    reopen: bool,
    phase: Phase,
    started: Instant,
    click: Option<(DVec2, u8)>,
    pulse: Option<Entity>,
    report: Value,
}

#[derive(Resource)]
struct ImageWrite(Task<Result<bool, String>>);

pub fn install(app: &mut App, output: PathBuf, assets: PathBuf, reopen: bool) {
    app.insert_resource(Probe {
        assets,
        output,
        reopen,
        phase: Phase::Reveal,
        started: Instant::now(),
        click: None,
        pulse: None,
        report: Value::Null,
    })
    .add_systems(Last, drive);
}

fn drive(world: &mut World) {
    world.resource_scope(|world, mut probe: Mut<Probe>| {
        if let Some(pulse) = probe.pulse.take() {
            let _ = world.despawn(pulse);
        }
        let result = if probe.started.elapsed() > Duration::from_secs(120) {
            Err(format!(
                "姿势组验收超时于 {:?}：{}",
                probe.phase,
                bridge::diagnostic(world)
            ))
        } else {
            advance(world, &mut probe)
        };
        match result {
            Ok(false) => {}
            Ok(true) => finish(world, &probe, probe.report.clone()),
            Err(error) => finish(
                world,
                &probe,
                json!({
                    "passed":false,"phase":format!("{:?}",probe.phase),"error":error
                }),
            ),
        }
    });
}

fn finish(world: &mut World, probe: &Probe, result: Value) {
    let written = serde_json::to_vec_pretty(&result)
        .map_err(|error| error.to_string())
        .and_then(|data| {
            std::fs::write(probe.output.join("pose-group.json"), data)
                .map_err(|error| error.to_string())
        });
    if let Err(error) = &written {
        error!("无法写入姿势组验收报告：{error}");
    }
    world.write_message(if written.is_ok() && result["passed"] == true {
        AppExit::Success
    } else {
        AppExit::error()
    });
}

fn advance(world: &mut World, probe: &mut Probe) -> Result<bool, String> {
    match probe.phase {
        Phase::Reveal => {
            if !world.resource::<SceneEditorState>().open {
                return Ok(false);
            }
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
            probe.phase = Phase::Open;
        }
        Phase::Open => {
            let workspace = world.resource::<AssetPreviewWorkspace>();
            if workspace.central_active
                && workspace
                    .expanded
                    .as_ref()
                    .is_some_and(|tab| tab.path == MODEL)
            {
                let preview = world.resource::<AssetPreviewState>();
                if preview.selected_path.as_deref() == Some(MODEL)
                    && preview.render_visible
                    && matches!(
                        &preview.content,
                        AssetPreviewContent::Model {
                            phase: AssetPreviewPhase::Ready,
                            ..
                        }
                    )
                {
                    probe.phase = if probe.reopen {
                        Phase::Reopen
                    } else {
                        Phase::Create
                    };
                }
            } else if world
                .resource::<SceneEditorState>()
                .selected_asset
                .as_deref()
                == Some(MODEL)
                && world.resource::<SceneEditorSidebarState>().right
                    == EditorRightSidebarTab::Assets
            {
                world.write_message(OpenAssetPreview { path: MODEL.into() });
            }
        }
        Phase::Create => {
            // 指针按松不代表操作已受理。等首个片段成为正式选中项，错过布局或输入门时可重试。
            let cursor = history_cursor(world);
            if probe.click.is_some() || cursor == Some(0) {
                click_action(world, probe, Action::CreateClip)?;
            } else if cursor == Some(1)
                && world.resource::<AnimationEditorState>().can_edit_selected()
            {
                probe.phase = Phase::Enter;
            }
        }
        Phase::Enter => {
            if probe.click.is_none() && world.resource::<AnimationEditorState>().editing_animation() {
                probe.phase = Phase::Seek;
            } else {
                click_action(world, probe, Action::ToggleEditing)?;
            }
        }
        Phase::Seek => {
            let editor = world.resource::<AnimationEditorState>();
            if !editor.editing_animation() {
                return Ok(false);
            }
            if click_midpoint(world, probe)? {
                probe.phase = Phase::ManualKeys;
            }
        }
        Phase::ManualKeys => {
            let editor = world.resource::<AnimationEditorState>();
            if !editor.auto_key && probe.click.is_none() {
                if (editor.position() - POSE_TIME).abs() > 0.02 {
                    return Err("原时间尺未将播放头定位到中间姿势".into());
                }
                probe.phase = Phase::Hips;
            } else if click_action(world, probe, Action::ToggleAutoKey)? {
                probe.phase = Phase::Hips;
            }
        }
        Phase::Hips | Phase::Arm => {
            if !bridge::input_ready(world) {
                return Ok(false);
            }
            let (name, property) = if probe.phase == Phase::Hips {
                ("mixamorig:Hips", "translation")
            } else {
                ("mixamorig:LeftArm", "rotation")
            };
            let Some(channel) = channel(world, name, property) else {
                return Ok(false);
            };
            probe.pulse = Some(bridge::select_channel(
                world,
                KeyIdentity {
                    clip: 0,
                    channel,
                    key: 0,
                },
            )?);
            probe.phase = if probe.phase == Phase::Hips {
                Phase::SetHips
            } else {
                Phase::SetArm
            };
        }
        Phase::SetHips | Phase::SetArm => {
            if !bridge::input_ready(world) {
                return Ok(false);
            }
            let hips = probe.phase == Phase::SetHips;
            let expected = if hips {
                channel(world, "mixamorig:Hips", "translation")
            } else {
                channel(world, "mixamorig:LeftArm", "rotation")
            };
            let editor = world.resource::<AnimationEditorState>();
            if expected.is_none()
                || editor.current().and_then(|session| session.channel) != expected
            {
                return Ok(false);
            }
            if editor.auto_key || (editor.position() - POSE_TIME).abs() > 0.02 {
                return Err("调整两个骨骼时须保留暂停的同一播放帧".into());
            }
            bridge::commit_x(world, if hips { HIP_X } else { ARM_DEGREES_X })?;
            probe.phase = if hips { Phase::Arm } else { Phase::Insert };
        }
        Phase::Insert => {
            let Some(session) = world.resource::<AnimationEditorState>().current() else {
                return Ok(false);
            };
            if (bridge::draft_bone_count(world) != 2 || session.cursor != 1)
                && probe.click.is_none()
            {
                return Ok(false);
            }
            if click_action(world, probe, Action::InsertPose)? {
                probe.phase = Phase::Undo;
            }
        }
        Phase::Undo => {
            if (!has_pair(world) || history_cursor(world) != Some(2)) && probe.click.is_none() {
                return Ok(false);
            }
            if click_action(world, probe, Action::Undo)? {
                probe.phase = Phase::Redo;
            }
        }
        Phase::Redo => {
            if (history_cursor(world) != Some(1)
                || keys(world)?.0.len() != 2
                || keys(world)?.1.len() != 2)
                && probe.click.is_none()
            {
                return Ok(false);
            }
            if click_action(world, probe, Action::Redo)? {
                probe.phase = Phase::Save;
            }
        }
        Phase::Save => {
            if (!has_pair(world) || history_cursor(world) != Some(2)) && probe.click.is_none() {
                return Ok(false);
            }
            if click_action(world, probe, Action::Save)? {
                probe.phase = Phase::Receipt;
            }
        }
        Phase::Receipt => {
            let editor = world.resource::<AnimationEditorState>();
            let Some((path, receipt)) = &editor.project_save_receipt else {
                return Ok(false);
            };
            if path != &probe.assets.join(MODEL) {
                return Err("保存回执没有指向本轮角色 GLB".into());
            }
            receipt.as_ref().map_err(Clone::clone)?;
            if editor.busy() {
                return Ok(false);
            }
            if editor.current().is_none_or(|session| session.dirty()) || !has_pair(world) {
                return Err("保存后仍有草稿或关键帧已失效".into());
            }
            probe.report = pair_report(world, false)?;
            probe.phase = Phase::Capture;
        }
        Phase::Reopen => {
            if !has_pair(world) {
                return Ok(false);
            }
            let editor = world.resource::<AnimationEditorState>();
            let media = world.resource::<AssetPreviewWorkspace>();
            if editor.editing_animation()
                || media.expanded.as_ref().is_none_or(|tab| tab.path != MODEL)
            {
                return Err("重新打开的作者标签身份或只读状态异常".into());
            }
            probe.report = pair_report(world, true)?;
            probe.phase = Phase::ReadSavedFrame;
        }
        Phase::ReadSavedFrame => {
            if click_midpoint(world, probe)? {
                probe.phase = Phase::Capture;
            }
        }
        Phase::Capture => {
            let editor = world.resource::<AnimationEditorState>();
            let preview = world.resource::<AssetPreviewState>();
            let runtime = world.resource::<PreviewRuntime>();
            if !runtime.document_ready(editor, preview)
                || runtime
                    .published_pose
                    .as_ref()
                    .is_none_or(|pose| (pose.time - POSE_TIME).abs() > 0.02)
            {
                return Ok(false);
            }
            let output = probe.output.join("pose-group.png");
            world.spawn(Screenshot::primary_window()).observe(
                move |event: On<ScreenshotCaptured>, mut commands: Commands| {
                    let image = event.image.clone();
                    let output = output.clone();
                    commands.insert_resource(ImageWrite(IoTaskPool::get().spawn(async move {
                        let image = image
                            .try_into_dynamic()
                            .map_err(|error| error.to_string())?
                            .to_rgb8();
                        let (width, height) = image.dimensions();
                        // 角色夹具的橙色躯干须出现在实际画布；读到 ECS 姿势时 GPU 可能仍是上一张空画面。
                        let model_visible = image
                            .enumerate_pixels()
                            .filter(|(x, y, pixel)| {
                                let [red, green, blue] = pixel.0;
                                *x > width / 3
                                    && *x < width * 2 / 3
                                    && *y > height / 5
                                    && *y < height / 2
                                    && red > 150
                                    && red.saturating_sub(green) > 40
                                    && green.saturating_sub(blue) > 25
                            })
                            .take(128)
                            .count()
                            == 128;
                        if model_visible {
                            image.save(output).map_err(|error| error.to_string())?;
                        }
                        Ok(model_visible)
                    })));
                },
            );
            probe.phase = Phase::Finish;
        }
        Phase::Finish => {
            let Some(result) = world
                .get_resource_mut::<ImageWrite>()
                .and_then(|mut task| check_ready(&mut task.0))
            else {
                return Ok(false);
            };
            // GPU 画面未到齐时重试截图，已完成的写图任务不再进入下一轮轮询。
            world.remove_resource::<ImageWrite>();
            if result? {
                return Ok(true);
            }
            probe.phase = Phase::Capture;
        }
    }
    Ok(false)
}

fn click_midpoint(world: &mut World, probe: &mut Probe) -> Result<bool, String> {
    if let Some((position, _)) = probe.click {
        return click_at(world, probe, position);
    }
    let Some((ruler, midpoint)) = bridge::timeline_ruler(world) else {
        return Ok(false);
    };
    if (midpoint - POSE_TIME).abs() > 0.02 {
        return Err("原时间尺中心与夹具的目标帧不一致".into());
    }
    click_entity(world, probe, ruler)
}

fn click_action(world: &mut World, probe: &mut Probe, action: Action) -> Result<bool, String> {
    if let Some((position, _)) = probe.click {
        return click_at(world, probe, position);
    }
    if !bridge::input_ready(world) {
        return Ok(false);
    }
    let Some(entity) = bridge::original_button(world, action) else {
        return Ok(false);
    };
    click_entity(world, probe, entity)
}

fn click_entity(world: &mut World, probe: &mut Probe, entity: Entity) -> Result<bool, String> {
    if !ui_visible(world, entity) {
        return Ok(false);
    }
    let Some(transform) = world.get::<UiGlobalTransform>(entity) else {
        return Ok(false);
    };
    click_at(world, probe, transform.translation.as_dvec2())
}

fn click_at(world: &mut World, probe: &mut Probe, at: DVec2) -> Result<bool, String> {
    let window = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
        .map_err(|error| error.to_string())?;
    Ok(pointer_click(
        world,
        &mut probe.click,
        window,
        at,
        MouseButton::Left,
    ))
}

fn channel(world: &World, name: &str, property: &str) -> Option<usize> {
    world
        .resource::<AnimationEditorState>()
        .active_document()?
        .clips
        .first()?
        .channels
        .iter()
        .position(|channel| channel.node_name == name && channel.path == property)
}

fn keys(
    world: &World,
) -> Result<
    (
        &[feathrise_editor_animation_model::data::Keyframe],
        &[feathrise_editor_animation_model::data::Keyframe],
    ),
    String,
> {
    let hips = channel(world, "mixamorig:Hips", "translation").ok_or("角色缺少骨盆位置通道")?;
    let arm = channel(world, "mixamorig:LeftArm", "rotation").ok_or("角色缺少左臂旋转通道")?;
    let doc = world
        .resource::<AnimationEditorState>()
        .active_document()
        .ok_or("角色 GLB 动画尚未读取")?;
    Ok((
        &doc.clips[0].channels[hips].keys,
        &doc.clips[0].channels[arm].keys,
    ))
}

fn has_pair(world: &World) -> bool {
    let Ok((hips, arm)) = keys(world) else {
        return false;
    };
    let ([_, hips, _], [_, arm, _]) = (hips, arm) else {
        return false;
    };
    let rotation = Quat::from_rotation_x(ARM_DEGREES_X.to_radians());
    hips.time == arm.time
        && (hips.time - POSE_TIME).abs() < 0.02
        && (hips.value[0] - HIP_X).abs() < 1e-5
        && arm
            .value
            .as_slice()
            .try_into()
            .is_ok_and(|value| (Quat::from_array(value).dot(rotation)).abs() > 0.9999)
}

fn history_cursor(world: &World) -> Option<usize> {
    world
        .resource::<AnimationEditorState>()
        .current()
        .map(|session| session.cursor)
}

fn pair_report(world: &World, reopened: bool) -> Result<Value, String> {
    let (hips, arm) = keys(world)?;
    Ok(
        json!({"passed": true, "reopened": reopened, "asset":MODEL, "bones":2, "time":hips[1].time,
        "hips_translation":hips[1].value,"arm_rotation":arm[1].value,"history_cursor":history_cursor(world)}),
    )
}
