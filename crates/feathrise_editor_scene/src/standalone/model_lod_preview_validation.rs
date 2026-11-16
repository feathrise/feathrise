//! 独占夹具的模型 LOD 实窗验收。用真实细分源和属性按钮取得两张三角线截图及同镜头帧时间。

use super::{LodAction, action, reveal_lod, ui_visible};
use crate::{
    authoring::{EditorAuthoringSession, mesh_edit::{MeshEditAction, MeshEditState}},
    components::SceneEditorActionButton,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState},
    types::EditorAction,
    ui::{placement_transform::SceneEditorPlacementTransformActionButton,
        sidebars::{EditorRightSidebarTab, EditorSidebarTab, SceneEditorSidebarTabButton}},
};
use bevy::{prelude::*, render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready}, window::{PresentMode, PrimaryWindow},
    winit::{UpdateMode, WinitSettings}};
use feathrise_scene::{MeshLodInspection, MeshLodInspectionSample, author_view::ShadingView};
use feathrise_scene_content::ScenePlacementRecord;
use serde_json::{Value, json};
use std::path::PathBuf;

const DENSE_TRIANGLES: usize = 6 * 101 * 101 * 2;
const WARM_FRAMES: usize = 30;
const MEASURED_FRAMES: usize = 90;
const LOD_WIRE: &str = "model-lod-simplified.png";
const ORIGINAL_WIRE: &str = "model-lod-original.png";

#[derive(Clone, Copy, Debug, Default)]
enum Phase {
    #[default] Start, SetCuts, WaitDense, LeaveEdit, Reveal, Enable,
    LodWire, OriginalWire, DisableWire, OriginalTime, LodTime, OriginalRepeat,
    Close, Undo, Reenter,
}

#[derive(Default)]
pub(super) struct PreviewFixture {
    phase: Phase,
    source: Option<ScenePlacementRecord>,
    camera: Option<GlobalTransform>,
    viewport_size: Option<UVec2>,
    revision: u64,
    history: usize,
    observations: usize,
    frame_times: Vec<f64>,
    proof: Value,
}

#[derive(Resource)]
struct Capture(Task<Result<(), String>>);

impl PreviewFixture {
    pub(super) fn stage(&self) -> String { format!("{:?}", self.phase) }

    pub(super) fn advance(
        &mut self, world: &mut World, current: &ScenePlacementRecord, pulse: &mut Option<Entity>,
    ) -> Result<Option<Value>, String> {
        use Phase::*;
        let edit = world.resource::<MeshEditState>();
        let session = world.resource::<EditorAuthoringSession>();
        let triangles = current.asset_target.editable_geometry().map_or(0, |mesh| mesh.triangles().len());
        let camera = world.resource::<SceneEditorViewportState>().camera;
        let sample = world.resource::<MeshLodInspection>().sample;
        let request = world.resource::<MeshLodInspection>().request;
        let triangle_view = world.get::<ShadingView>(camera).is_some_and(|view| view.wire.z > 0.5);
        let actual = sample.filter(|sample| !sample.pending && sample.original_triangles == DENSE_TRIANGLES);
        match self.phase {
            Start if edit.selected_count() == 6 => {
                if triangles != 12 { return Err("密集模型LOD验收必须从原立方体开始".into()); }
                self.source = Some(current.clone());
                self.history = session.undo.len();
                self.proof = json!({"source_triangles": DENSE_TRIANGLES, "measurement":"same editor viewport, shaded, A/B/A"});
                action(world, MeshEditAction::Subdivide);
                self.phase = SetCuts;
            }
            SetCuts if edit.subdivision_preview_for_validation().is_some() => {
                // 原参数业务接受刀数增量。一次跳到100切，避免99次无关中间候选。
                action(world, MeshEditAction::SubdivisionCutsDelta(99));
                self.phase = WaitDense;
            }
            WaitDense => {
                if let Some((options, candidate, true)) = edit.subdivision_preview_for_validation()
                    && options.cuts == 100
                    && candidate.asset_target.editable_geometry().is_some_and(|mesh| mesh.triangles().len() == DENSE_TRIANGLES)
                    && edit.embedded_preview.as_ref().is_some_and(|value| value.target == candidate.asset_target)
                {
                    action(world, MeshEditAction::Confirm);
                    self.phase = LeaveEdit;
                }
            }
            LeaveEdit if triangles == DENSE_TRIANGLES && !edit.moving() => {
                if session.undo.len() != self.history + 1 { return Err("100切未以单条原建模历史采用".into()); }
                if edit.owns_mouse() {
                    if !edit.releasing_mouse() && !mouse_blocked(world) { action(world, MeshEditAction::Toggle); }
                } else if !mouse_blocked(world) {
                    self.revision = session.revision;
                    let center = Vec3::from_array(current.translation);
                    let mut view = world.resource_mut::<SceneEditorViewportState>();
                    (view.yaw, view.pitch, view.extent) = (0.65, 0.35, 8.0);
                    let (_, pose) = crate::viewport::editor_camera_pose(false, &view);
                    view.center = center - pose.back().as_vec3() * 2.6;
                    world.resource_mut::<SceneEditorState>().orthographic = false;
                    let tab = world.query::<(Entity, &SceneEditorSidebarTabButton)>().iter(world)
                        .find_map(|(entity, button)| (button.0 == EditorSidebarTab::Right(EditorRightSidebarTab::Scene)).then_some(entity))
                        .ok_or("缺少原属性页签")?;
                    world.entity_mut(tab).insert(feathrise_editor_ui::EditorButtonClick::triggered());
                    *pulse = Some(tab);
                    self.phase = Reveal;
                }
            }
            Reveal => {
                reveal_lod(world)?;
                self.phase = Enable;
            }
            Enable => {
                if click(world, LodAction::ToggleLodPreview, pulse) { self.phase = LodWire; }
            }
            LodWire if triangle_view && actual.is_some_and(reduced) => {
                self.camera = self.camera.or_else(|| world.get::<GlobalTransform>(camera).copied());
                self.viewport_size = self.viewport_size.or_else(|| viewport_size(world, camera));
                self.check_view(world, camera)?;
                self.proof["lod_triangles"] = json!(actual.unwrap().displayed_triangles);
                self.proof["lod_levels"] = json!(actual.unwrap().levels);
                if capture_ready(world, LOD_WIRE, &mut self.observations)? {
                    if !click(world, LodAction::CompareLodOriginal, pulse) {
                        return Err("原始对照按钮已离开属性视区".into());
                    }
                    self.observations = 0;
                    self.phase = OriginalWire;
                }
            }
            OriginalWire if triangle_view && request.is_some_and(|request| request.original)
                && actual.is_some_and(|sample| sample.displayed_triangles == DENSE_TRIANGLES && sample.levels[0]) =>
            {
                self.check_view(world, camera)?;
                if capture_ready(world, ORIGINAL_WIRE, &mut self.observations)? {
                    if !click(world, LodAction::ToggleLodTriangles, pulse) {
                        return Err("三角线按钮已离开属性视区".into());
                    }
                    self.observations = 0;
                    self.phase = DisableWire;
                }
            }
            DisableWire if !triangle_view && request.is_some_and(|request| !request.triangles) => {
                self.proof["captures"] = json!([LOD_WIRE, ORIGINAL_WIRE]);
                self.proof["viewport_pixels"] = json!(self.viewport_size.map(|size| size.to_array()));
                self.phase = OriginalTime;
            }
            OriginalTime | LodTime | OriginalRepeat => {
                self.check_view(world, camera)?;
                if !benchmark_ready(world)? {
                    self.observations = 0;
                    self.frame_times.clear();
                    return Ok(None);
                }
                let wants_original = !matches!(self.phase, LodTime);
                if !triangle_view && request.is_some_and(|request| request.original == wants_original && !request.triangles)
                    && actual.is_some_and(|sample| if wants_original { sample.displayed_triangles == DENSE_TRIANGLES }
                        else { sample.displayed_triangles == self.proof["lod_triangles"].as_u64().unwrap_or_default() as usize })
                {
                    let key = match self.phase { LodTime => "lod", OriginalRepeat => "original_repeat", _ => "original" };
                    if let Some(stats) = self.measure(world)? {
                        self.proof[key] = stats;
                        let button = if matches!(self.phase, OriginalRepeat) { LodAction::ToggleLodPreview } else { LodAction::CompareLodOriginal };
                        if !click(world, button, pulse) { return Err("性能对照按钮离开了实际属性视区".into()); }
                        self.phase = match self.phase { OriginalTime => LodTime, LodTime => OriginalRepeat, _ => Close };
                    }
                }
            }
            Close if request.is_none() && !triangle_view => {
                if session.revision != self.revision || session.undo.len() != self.history + 1 {
                    return Err("LOD预览和原始对照修改了作者记录或撤销历史".into());
                }
                let button = world.query::<(Entity, &SceneEditorActionButton)>().iter(world)
                    .find_map(|(entity, action)| (action.0 == EditorAction::Undo && ui_visible(world, entity)).then_some(entity))
                    .ok_or("没有原撤销按钮")?;
                world.entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
                *pulse = Some(button);
                self.phase = Undo;
            }
            Undo if self.source.as_ref() == Some(current) => {
                self.phase = Reenter;
            }
            Reenter if edit.active() => {
                action(world, MeshEditAction::Kind(crate::authoring::mesh_edit::MeshElementKind::Face));
                self.proof["complete"] = true.into();
                return Ok(Some(self.proof.clone()));
            }
            Reenter if !edit.owns_mouse() && !mouse_blocked(world) => action(world, MeshEditAction::Toggle),
            _ => {}
        }
        Ok(None)
    }

    fn check_view(&self, world: &World, camera: Entity) -> Result<(), String> {
        if self.camera.as_ref() != world.get::<GlobalTransform>(camera)
            || self.viewport_size.is_none() || self.viewport_size != viewport_size(world, camera)
        {
            return Err("LOD A/B 没有保持相同实际相机和分辨率".into());
        }
        Ok(())
    }

    fn measure(&mut self, world: &World) -> Result<Option<Value>, String> {
        self.observations += 1;
        if self.observations <= WARM_FRAMES { return Ok(None); }
        let milliseconds = world.resource::<Time<Real>>().delta_secs_f64() * 1000.0;
        if !milliseconds.is_finite() || milliseconds <= 0.0 {
            return Err("性能对照缺少有效帧时间".into());
        }
        self.frame_times.push(milliseconds);
        if self.frame_times.len() < MEASURED_FRAMES { return Ok(None); }
        self.frame_times.sort_by(f64::total_cmp);
        let median = (self.frame_times[44] + self.frame_times[45]) * 0.5;
        let result = json!({"frames":MEASURED_FRAMES,"median_ms":median,
            "p95_ms":self.frame_times[85],"median_fps":1000.0 / median,"continuous_no_vsync":true});
        self.frame_times.clear();
        self.observations = 0;
        Ok(Some(result))
    }
}

fn viewport_size(world: &World, camera: Entity) -> Option<UVec2> {
    world.get::<Camera>(camera)?.physical_viewport_size()
}

fn benchmark_ready(world: &mut World) -> Result<bool, String> {
    let window = world.query_filtered::<&Window, With<PrimaryWindow>>().single(world)
        .map_err(|_| "模型LOD帧时间缺少独占真实窗口")?;
    if window.present_mode != PresentMode::AutoNoVsync {
        return Err("模型LOD帧时间要求夹具窗口关闭垂直同步".into());
    }
    let pacing = world.resource::<WinitSettings>();
    let active_mode = if window.focused { pacing.focused_mode } else { pacing.unfocused_mode };
    // 自动编译会让编辑器大幅降帧，等调度恢复连续模式后重新预热与计时。
    Ok(matches!(active_mode, UpdateMode::Continuous))
}

fn reduced(sample: MeshLodInspectionSample) -> bool {
    sample.displayed_triangles < DENSE_TRIANGLES && sample.levels.iter().skip(1).any(|&active| active)
}

fn mouse_blocked(world: &World) -> bool {
    world.resource::<SceneEditorNumericScrubMouseGuard>().blocked()
}

fn click(world: &mut World, action: LodAction, pulse: &mut Option<Entity>) -> bool {
    let button = world.query::<(Entity, &SceneEditorPlacementTransformActionButton)>().iter(world)
        .find_map(|(entity, button)| (button.0 == action && ui_visible(world, entity)).then_some(entity));
    let Some(button) = button else { return false };
    world.entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    *pulse = Some(button);
    true
}

fn capture_ready(world: &mut World, name: &'static str, frames: &mut usize) -> Result<bool, String> {
    *frames += 1;
    if *frames == 24 {
        let path = PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR")
            .ok_or("模型LOD截图缺少专属输出目录")?).join(name);
        world.spawn(Screenshot::primary_window()).observe(move |captured: On<ScreenshotCaptured>, mut commands: Commands| {
            let image = captured.image.clone();
            let path = path.clone();
            commands.insert_resource(Capture(IoTaskPool::get().spawn(async move {
                image.try_into_dynamic().map_err(|error| error.to_string())?.save(path).map_err(|error| error.to_string())
            })));
        });
    }
    if let Some(mut capture) = world.get_resource_mut::<Capture>()
        && let Some(result) = check_ready(&mut capture.0)
    {
        world.remove_resource::<Capture>();
        result?;
        return Ok(true);
    }
    Ok(false)
}
