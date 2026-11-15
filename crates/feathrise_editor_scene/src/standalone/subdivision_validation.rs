//! 验收正式细分工具的实时参数、超限保护及单次确认，不接管建模数据或预览实体。

use crate::{
    authoring::{EditorAuthoringSession, mesh_edit::{MeshEditAction, MeshEditState}},
    components::SceneEditorActionButton,
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorViewportState},
    types::EditorAction,
    viewport::{modes::validation_mesh_button, shading},
};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use feathrise_scene_content::ScenePlacementRecord;
use std::path::PathBuf;

pub(super) const SCREENSHOT: &str = "mesh-subdivide-two-cuts.png";
pub(super) const OBJECT_SCREENSHOT: &str = "mesh-subdivide-object-mode.png";

#[derive(Clone, Copy, Debug, Default)]
enum Phase {
    #[default]
    Open,
    Preview,
    OneCut,
    TwoCuts,
    Screenshot,
    ThreeCuts,
    RecoverTwoCuts,
    RestoreThreeCuts,
    AdoptDense,
    ObjectMode,
    ObjectScreenshot,
    UndoDense,
    Reenter,
    Adopt,
}

#[derive(Default)]
pub(super) struct SubdivisionFixture {
    phase: Phase,
    source: Option<(ScenePlacementRecord, u64, usize, usize)>,
    one_cut: Option<ScenePlacementRecord>,
    two_cuts: Option<ScenePlacementRecord>,
    three_cuts: Option<ScenePlacementRecord>,
    original_shading: Option<u8>,
    verified_object: bool,
    original_view: Option<(Vec3, f32, f32, f32)>,
}

#[derive(Resource)]
struct Capture(Task<Result<(), String>>);

impl SubdivisionFixture {
    pub(super) fn stage(&self) -> String {
        format!("{:?}", self.phase)
    }

    pub(super) fn advance(
        &mut self,
        world: &mut World,
        current: &ScenePlacementRecord,
        pulse: &mut Option<Entity>,
    ) -> Result<Option<serde_json::Value>, String> {
        use Phase::*;
        let session = world.resource::<EditorAuthoringSession>();
        let (revision, undo, redo) = (session.revision, session.undo.len(), session.redo.len());
        if matches!(self.phase, Open) && self.source.is_none() {
            if mesh_counts(current)? != [8, 12, 6] {
                return Err("细分专项的起点必须是原创建的完整立方体".into());
            }
            self.source = Some((current.clone(), revision, undo, redo));
            let center = Vec3::from_array(current.translation);
            let mut view = world.resource_mut::<SceneEditorViewportState>();
            self.original_view = Some((view.center, view.extent, view.yaw, view.pitch));
            view.extent = 8.0;
            view.yaw = 0.72;
            view.pitch = 0.48;
            let (_, pose) = crate::viewport::editor_camera_pose(false, &view);
            // 只改验收取景。视口实际采用本帧姿态后才启动冻结候选。
            view.center = center + pose.back().as_vec3() * (4.8 - 8.0);
            return Ok(None);
        }
        let (source, source_revision, source_undo, source_redo) = self.source.as_ref().ok_or("细分专项未冻结原记录")?;
        if !matches!(self.phase, Adopt | AdoptDense | ObjectMode | ObjectScreenshot | UndoDense)
            && (current != source || (revision, undo, redo) != (*source_revision, *source_undo, *source_redo)) {
            return Err("细分参数预览改写了作者记录、修订或历史".into());
        }
        let edit = world.resource::<MeshEditState>();
        let preview = edit.subdivision_preview_for_validation()
            .map(|(options, record, valid)| (options, record.clone(), valid));
        match self.phase {
            Open if edit.selected_count() == 6 => {
                if click(world, None, pulse) {
                    self.phase = Preview;
                }
            }
            Preview => {
                if click(world, Some(MeshEditAction::Subdivide), pulse) {
                    self.phase = OneCut;
                }
            }
            OneCut => {
                if let Some((options, candidate, true)) = preview {
                    if options.cuts != 1 || mesh_counts(&candidate)? != [26, 48, 24] || !adopted_preview(edit, &candidate) {
                        return Err("完整立方体的默认一刀没有生成已发布的 26 点／24 面网格".into());
                    }
                    if self.verified_object {
                        if self.one_cut.as_ref() != Some(&candidate) {
                            return Err("撤销后重开一切未得到同一个预览".into());
                        }
                        if click(world, Some(MeshEditAction::Confirm), pulse) {
                            self.phase = Adopt;
                        }
                    } else if click(world, Some(MeshEditAction::SubdivisionCutsDelta(1)), pulse) {
                        self.one_cut = Some(candidate);
                        self.phase = TwoCuts;
                    }
                }
            }
            TwoCuts => {
                if let Some((options, candidate, valid)) = preview
                    && options.cuts == 2
                {
                    if !valid || mesh_counts(&candidate)? != [56, 108, 54] || !adopted_preview(edit, &candidate) {
                        return Err("两刀没有从冻结来源重新生成已发布的 56 点／54 面网格".into());
                    }
                    if validation_mesh_button(world, Some(MeshEditAction::SubdivisionCutsDelta(1))).is_some_and(|entity| {
                        super::ui_visible(world, entity)
                    }) {
                        self.two_cuts = Some(candidate);
                        capture(world, SCREENSHOT)?;
                        self.phase = Screenshot;
                    }
                }
            }
            Screenshot => {
                let result = world.get_resource_mut::<Capture>().and_then(|mut capture| check_ready(&mut capture.0));
                if let Some(result) = result {
                    world.remove_resource::<Capture>();
                    result?;
                    let Some((options, candidate, true)) = preview else {
                        return Err("回读期间失去真实细分候选".into());
                    };
                    if options.cuts != 2 || self.two_cuts.as_ref() != Some(&candidate) {
                        return Err("回读期间细分候选被换代".into());
                    }
                    if !click(world, Some(MeshEditAction::SubdivisionCutsDelta(1)), pulse) {
                        return Err("两刀预览缺少正向刀数调整入口".into());
                    }
                    self.phase = ThreeCuts;
                }
            }
            ThreeCuts => {
                if let Some((options, candidate, true)) = preview
                    && options.cuts == 3
                {
                    if mesh_counts(&candidate)? != [98, 192, 96] || !adopted_preview(edit, &candidate) {
                        return Err("三切未发布完整 98 点／96 面网格".into());
                    }
                    if click(world, Some(MeshEditAction::SubdivisionCutsDelta(-1)), pulse) {
                        self.three_cuts = Some(candidate);
                        self.phase = RecoverTwoCuts;
                    }
                }
            }
            RecoverTwoCuts => {
                if let Some((options, candidate, true)) = preview
                    && options.cuts == 2
                {
                    if self.two_cuts.as_ref() != Some(&candidate) || !adopted_preview(edit, &candidate) {
                        return Err("调回两切后候选没有精确回到原值".into());
                    }
                    if click(world, Some(MeshEditAction::SubdivisionCutsDelta(1)), pulse) {
                        self.phase = RestoreThreeCuts;
                    }
                }
            }
            RestoreThreeCuts => {
                if let Some((options, candidate, true)) = preview
                    && options.cuts == 3
                {
                    if self.three_cuts.as_ref() != Some(&candidate) || !adopted_preview(edit, &candidate) {
                        return Err("恢复三切没有从冻结源产生精确同一结果".into());
                    }
                    if click(world, Some(MeshEditAction::Confirm), pulse) {
                        self.phase = AdoptDense;
                    }
                }
            }
            AdoptDense if !edit.moving() => {
                if self.three_cuts.as_ref() != Some(current) || undo != source_undo + 1 || redo != 0 || revision == *source_revision {
                    return Err("三切确认未一次性写入完整网格和一条历史".into());
                }
                if !edit.owns_mouse() {
                    self.phase = ObjectMode;
                } else if !edit.releasing_mouse() && !world.resource::<SceneEditorNumericScrubMouseGuard>().blocked() {
                    click(world, Some(MeshEditAction::Toggle), pulse);
                }
            }
            ObjectMode if !edit.owns_mouse() => {
                if !displayed_object(world, current) {
                    return Ok(None);
                }
                let camera = world.resource::<SceneEditorViewportState>().camera;
                let Some(shading) = world.get::<feathrise_scene::author_view::ShadingView>(camera) else { return Ok(None) };
                let mode = shading.options.x as u8;
                if self.original_shading.is_none() && mode != 1 {
                    self.original_shading = Some(mode);
                    *pulse = Some(shading::validation_press_mode(world, 1)?);
                } else if mode == 1 {
                    capture(world, OBJECT_SCREENSHOT)?;
                    self.phase = ObjectScreenshot;
                }
            }
            ObjectScreenshot => {
                let result = world.get_resource_mut::<Capture>().and_then(|mut capture| check_ready(&mut capture.0));
                if let Some(result) = result {
                    world.remove_resource::<Capture>();
                    result?;
                    if world.resource::<MeshEditState>().active() || !displayed_object(world, current) {
                        return Err("对象截图期间实际网格不可见".into());
                    }
                    if let Some(shading) = self.original_shading.take() {
                        *pulse = Some(shading::validation_press_mode(world, shading)?);
                    }
                    self.verified_object = true;
                }
                if self.verified_object && pulse.is_none() && undo_button(world, pulse) {
                    self.phase = UndoDense;
                }
            }
            UndoDense if current == source && undo == *source_undo && redo == 1 => {
                self.source = Some((source.clone(), revision, undo, redo));
                self.phase = Reenter;
            }
            Reenter => {
                if edit.active() {
                    self.phase = Open;
                } else if !edit.owns_mouse() && !world.resource::<SceneEditorNumericScrubMouseGuard>().blocked() {
                    click(world, Some(MeshEditAction::Toggle), pulse);
                }
            }
            Adopt if !edit.moving() => {
                if self.one_cut.as_ref() != Some(current) || undo != source_undo + 1 || redo != 0 || revision == *source_revision {
                    return Err("细分确认没有一次性采用真实候选并只新增一条撤销".into());
                }
                let (center, extent, yaw, pitch) = self.original_view.ok_or("没有冻结原相机位置")?;
                let mut view = world.resource_mut::<SceneEditorViewportState>();
                (view.center, view.extent, view.yaw, view.pitch) = (center, extent, yaw, pitch);
                return Ok(Some(serde_json::json!({
                    "source": [8, 12, 6], "one_cut": [26, 48, 24], "two_cuts": [56, 108, 54],
                    "three_cuts": [98, 192, 96], "object_mode_visible": self.verified_object,
                    "object_screenshot": OBJECT_SCREENSHOT, "preview_history_unchanged": true,
                    "recomputed_exact": true, "confirmed_one_undo": true, "screenshot": SCREENSHOT,
                })));
            }
            _ => {}
        }
        Ok(None)
    }
}

fn mesh_counts(record: &ScenePlacementRecord) -> Result<[usize; 3], String> {
    let mesh = record.asset_target.editable_geometry().ok_or("细分候选缺少完整作者网格")?;
    Ok([mesh.vertices().len(), mesh.edges().len(), mesh.faces().len()])
}

fn adopted_preview(edit: &MeshEditState, candidate: &ScenePlacementRecord) -> bool {
    edit.embedded_preview.as_ref().is_some_and(|applied| {
        applied.placement_id == candidate.id && applied.target == candidate.asset_target
    }) // 发布态只由原预览绘制入口采用后产生，可以证明看到的是正式候选。
}

fn click(world: &mut World, action: Option<MeshEditAction>, pulse: &mut Option<Entity>) -> bool {
    let Some(entity) = validation_mesh_button(world, action).filter(|&entity| super::ui_visible(world, entity)) else {
        return false;
    };
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    *pulse = Some(entity);
    true
}

fn undo_button(world: &mut World, pulse: &mut Option<Entity>) -> bool {
    let Some(entity) = world.query::<(Entity, &SceneEditorActionButton)>().iter(world)
        .find_map(|(entity, button)| (button.0 == EditorAction::Undo && super::ui_visible(world, entity)).then_some(entity))
    else { return false; };
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    *pulse = Some(entity);
    true
}

fn displayed_object(world: &mut World, record: &ScenePlacementRecord) -> bool {
    use bevy::camera::primitives::Aabb;
    let count = record.asset_target.editable_geometry().map_or(0, |geometry| geometry.triangles().len() * 3);
    let root = world.query::<(Entity, &feathrise_scene::ScenePlacementInstance)>().iter(world)
        .find_map(|(root, instance)| (instance.is_active() && instance.id() == record.id
            && instance.asset_target() == record.asset_target
            && feathrise_scene::builtin_placement_visual_ready(world, root)).then_some(root));
    root.and_then(|root| world.get::<Children>(root)).is_some_and(|children| {
        let meshes = world.resource::<Assets<Mesh>>();
        children.iter().any(|entity| world.get::<InheritedVisibility>(entity).is_some_and(|value| value.get())
            && world.get::<Aabb>(entity).is_some() && world.get::<Mesh3d>(entity)
                .and_then(|mesh| meshes.get(&mesh.0)).is_some_and(|mesh| mesh.count_vertices() == count))
    })
}

fn capture(world: &mut World, name: &'static str) -> Result<(), String> {
    let directory = PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("细分截图目录未隔离")?);
    world.spawn(Screenshot::primary_window()).observe(move |captured: On<ScreenshotCaptured>, mut commands: Commands| {
        let path = directory.join(name);
        let image = captured.image.clone();
        commands.insert_resource(Capture(IoTaskPool::get().spawn(async move {
            image.try_into_dynamic().map_err(|error| error.to_string())?
                .save(path).map_err(|error| error.to_string())
        })));
    });
    Ok(())
}
