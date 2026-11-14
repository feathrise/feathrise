//! 用原数值消息、属性按钮和视口确认验证模型生命周期；观测真实运行子树与热更回执。

use super::{lifecycle_io::{self, Mutation, Source}, resources::Fixture, types::MODEL};
use crate::{
    authoring::{EditorAuthoringSession, model_vegetation, placement_batch::PlacementBatchState},
    components::{SceneEditorActionButton, SceneEditorViewport},
    resources::{SceneEditorState, SceneEditorViewportState},
    types::{EditorAction, EditorTool, EditorView},
    ui::{gltf_browser::EditorGltfBrowserState, placement_transform::{PlacementTransformAction,
        PlacementTransformField, SceneEditorPlacementTransformActionButton}},
    viewport::{modes::{ViewportMode, ViewportModeState}, multi_select::PlacementSelectionSet},
};
use bevy::{prelude::*, tasks::{IoTaskPool, Task, futures::check_ready}, ui::RelativeCursorPosition};
use feathrise_editor_ui::{EditorButtonClick, NumericInputCommitted, NumericInputCommitOutcome,
    NumericInputCommitSource, NumericValue};
use feathrise_environment::vegetation::blade::GpuBladePatch;
use feathrise_scene::{DynamicModelVegetationDraft, DynamicModelVegetationStatus, ScenePlacementInstance};
use feathrise_scene_content::{ModelSurfaceVegetation, ScenePlacementRecord, SurfaceTarget};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::{Duration, Instant}};

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    #[default] Start, Transform, UndoTransforms, Duplicate, WaitPaste, ConfirmCopy, WaitCopy,
    Delete, WaitDelete, UndoDelete, WaitUndoDelete, RedoDelete, WaitRedoDelete,
    UndoDeleteAgain, WaitUndoDeleteAgain, UndoCopy, WaitUndoCopy, RedoCopy, WaitRedoCopy,
    UndoCopyFinal, WaitUndoCopyFinal, Raise, WaitRaiseIo, WaitRaise,
    Topology, WaitTopologyIo, WaitTopology, Restore, WaitRestoreIo, WaitRestore, Complete,
}

pub(super) struct Lifecycle {
    stage: Stage,
    at: Instant,
    index: usize,
    issued: bool,
    undo: usize,
    original: Option<ScenePlacementRecord>,
    targets: Arc<Vec<Arc<ModelSurfaceVegetation>>>,
    placements: Vec<String>,
    copied: Option<String>,
    copied_targets: Option<Arc<Vec<Arc<ModelSurfaceVegetation>>>>,
    source: Option<Arc<Source>>,
    io: Option<Task<Result<Arc<Source>, String>>>,
    runtime_before: u64,
    error: Option<String>,
    mouse_owned: bool,
    proofs: Vec<Value>,
}
impl Default for Lifecycle {
    fn default() -> Self {
        Self { stage: Stage::Start, at: Instant::now(), index: 0, issued: false, undo: 0,
            original: None, targets: Arc::default(), placements: Vec::new(), copied: None,
            copied_targets: None, source: None, io: None, runtime_before: 0, error: None,
            mouse_owned: false, proofs: Vec::new() }
    }
}
impl Lifecycle {
    fn next(&mut self, stage: Stage) { self.stage = stage; self.at = Instant::now(); self.issued = false; }
}

/// 只发布真实视口坐标和鼠标边沿；粘贴结果必须由原有射线、后台校验和原子采用生成。
pub(super) fn pointer(world: &mut World) {
    let pressed = world.resource::<Fixture>().lifecycle.stage == Stage::ConfirmCopy;
    let owned = world.resource::<Fixture>().lifecycle.mouse_owned;
    if owned && !pressed { world.resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left); }
    world.resource_mut::<Fixture>().lifecycle.mouse_owned = pressed;
    if let Some(mut mode) = world.get_resource_mut::<ViewportModeState>() {
        mode.mode = ViewportMode::Select; mode.pointer_over_controls = false;
    }
    {
        let mut editor = world.resource_mut::<SceneEditorState>();
        editor.tool = EditorTool::Inspect; editor.view = EditorView::Top; editor.orthographic = true;
    }
    if let Some(mut viewport) = world.get_resource_mut::<SceneEditorViewportState>() {
        viewport.center = Vec3::new(12.,1.15,12.); viewport.extent = 8.; viewport.yaw = 0.;
        viewport.pitch = std::f32::consts::FRAC_PI_2;
    }
    for mut cursor in world.query_filtered::<&mut RelativeCursorPosition, With<SceneEditorViewport>>().iter_mut(world) {
        cursor.cursor_over = true; cursor.normalized = Some(Vec2::new(0.2,0.));
    }
    if pressed { world.resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left); }
}

pub(super) fn advance(world: &mut World, state: &mut Lifecycle, pulse: &mut Option<Entity>) -> Result<Option<Value>, String> {
    match step(world, state, pulse) {
        Ok(value) => Ok(value),
        Err(error) if state.source.is_some() && !matches!(state.stage, Stage::Restore | Stage::WaitRestoreIo | Stage::WaitRestore) => {
            // 故障也先恢复本轮源文件；不得在变体仍落盘时把错误直接交给父流程退出。
            state.error = Some(error); state.next(Stage::Restore); Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn step(world: &mut World, state: &mut Lifecycle, pulse: &mut Option<Entity>) -> Result<Option<Value>, String> {
    if state.at.elapsed() > Duration::from_secs(45) && state.io.is_none() {
        return Err(format!("模型生命周期 {:?} 超时：{}；{}",state.stage,world.resource::<SceneEditorState>().status,
            world.resource::<PlacementBatchState>().validation_snapshot()));
    }
    if world.resource::<EditorAuthoringSession>().model_vegetation_busy { return Ok(None); }
    match state.stage {
        Stage::Start => {
            if !ready(world) || output(world, MODEL)?.is_none() { return Ok(None); }
            let session = world.resource::<EditorAuthoringSession>();
            let draft = session.draft.as_ref().ok_or("模型生命周期草稿丢失")?;
            if draft.placements.placements.len() > 16 { return Err("模型生命周期对象超过固定观测预算".into()); }
            state.original = draft.placements.placements.iter().find(|r|r.id == MODEL).cloned();
            state.targets = model_vegetation::root_targets(draft);
            state.placements = draft.placements.placements.iter().map(|r|r.id.clone()).collect();
            state.undo = session.undo.len();
            if state.original.is_none() || state.targets.len() > 4 { return Err("模型生命周期自有目标失效".into()); }
            select(world, MODEL); state.index = 0; state.next(Stage::Transform);
        }
        Stage::Transform => {
            if !state.issued {
                let (field, value) = transform_value(state.index);
                let entity = world.spawn(field).id(); *pulse = Some(entity);
                world.write_message(NumericInputCommitted { entity, value: NumericValue::F32(value),
                    outcome: NumericInputCommitOutcome::Accepted, source: NumericInputCommitSource::Text,
                    changed: true, edited: true });
                state.issued = true;
            } else {
                let expected = transformed(state.original.as_ref().unwrap(), state.index + 1);
                if record(world, MODEL) == Some(&expected) && history(world) == state.undo + state.index + 1
                    && follows(world, MODEL, &expected)?
                {
                    state.index += 1;
                    if state.index == 3 {
                        state.proofs.push(json!({"object_transform":"original_numeric_messages",
                            "translation_x":expected.translation[0],"yaw_degrees":expected.yaw_degrees,"scale_x":expected.scale[0],
                            "actual_vegetation_parent_and_world_transform":true}));
                        state.index = 0; state.next(Stage::UndoTransforms);
                    } else { state.issued = false; }
                }
            }
        }
        Stage::UndoTransforms => {
            if !state.issued { *pulse = Some(action(world, EditorAction::Undo)); state.issued = true; }
            else {
                let count = 2 - state.index;
                let expected = transformed(state.original.as_ref().unwrap(), count);
                if record(world, MODEL) == Some(&expected) && history(world) == state.undo + count
                    && follows(world, MODEL, &expected)?
                {
                    state.index += 1;
                    if state.index == 3 { state.next(Stage::Duplicate); } else { state.issued = false; }
                }
            }
        }
        Stage::Duplicate => {
            if !state.issued { select(world, MODEL); state.issued = true; }
            let session = world.resource::<EditorAuthoringSession>();
            let selection = world.resource::<PlacementSelectionSet>();
            // 原选择域在Tasks阶段把树的单项同步到选集；须等它完成本轮Undo后的正式复核，
            // 不能先清空选集又同帧按复制，让下一次自动发布被正确识别成“选集已变化”。
            if !selection.ready_for(session) || selection.members.len() != 1 || selection.members[0].id != MODEL
                || world.resource::<EditorGltfBrowserState>().placement_record_index(MODEL, session.placement_structure_revision).is_none()
            { return Ok(None); }
            *pulse = Some(object_action(world, PlacementTransformAction::Duplicate));
            state.next(Stage::WaitPaste);
        }
        Stage::WaitPaste => {
            if world.resource::<PlacementBatchState>().grid_reference().is_some() { state.next(Stage::ConfirmCopy); }
            else if state.at.elapsed() > Duration::from_secs(5)
                && world.resource::<PlacementBatchState>().validation_waiting_for_ghost_records()
            {
                return Err(format!("模型复制记录仍未完成正式后台交接：{}；{}",
                    world.resource::<SceneEditorState>().status,
                    world.resource::<PlacementBatchState>().validation_snapshot()));
            }
            else if state.at.elapsed() > Duration::from_millis(500) && !batch_busy(world)
                && ["已取消对象批量操作", "批量操作身份已变化"].iter()
                    .any(|&prefix| world.resource::<SceneEditorState>().status.starts_with(prefix))
            {
                return Err(format!("模型属性复制被原批量域取消：{}", world.resource::<SceneEditorState>().status));
            }
        }
        Stage::ConfirmCopy => { state.next(Stage::WaitCopy); }
        Stage::WaitCopy => {
            if batch_busy(world) || history(world) != state.undo + 1 { return Ok(None); }
            let session = world.resource::<EditorAuthoringSession>();
            let draft = session.draft.as_ref().ok_or("模型副本草稿丢失")?;
            let added: Vec<_> = draft.placements.placements.iter().filter(|r|!state.placements.contains(&r.id)).collect();
            if added.len() != 1 { return Err("属性复制没有且仅有一个新对象ID".into()); }
            let copied = added[0].id.clone();
            let current = model_vegetation::root_targets(draft);
            same_copy(&state.targets, &current, &copied)?;
            if !ready(world) || output(world, &copied)?.is_none() { return Ok(None); }
            state.proofs.push(json!({"duplicate":"original_property_button_and_viewport_confirm",
                "source_id":MODEL,"new_id":copied,"anchors_and_all_layer_densities_preserved":true}));
            state.copied = Some(copied.clone()); state.copied_targets = Some(current);
            select(world, &copied); state.next(Stage::Delete);
        }
        Stage::Delete => {
            *pulse = Some(object_action(world, PlacementTransformAction::Delete)); state.next(Stage::WaitDelete);
        }
        Stage::WaitDelete | Stage::WaitRedoDelete => {
            let copied = state.copied.as_deref().unwrap();
            if record(world,copied).is_none() && history(world) == state.undo + 2 && output(world,copied)?.is_none() {
                if !Arc::ptr_eq(&targets(world)?,state.copied_targets.as_ref().unwrap()) {
                    return Err("删除对象时丢弃了可撤销的模型作者样本".into());
                }
                state.next(if state.stage == Stage::WaitDelete { Stage::UndoDelete } else { Stage::UndoDeleteAgain });
            }
        }
        Stage::UndoDelete | Stage::UndoDeleteAgain => {
            *pulse = Some(action(world,EditorAction::Undo));
            state.next(if state.stage == Stage::UndoDelete { Stage::WaitUndoDelete } else { Stage::WaitUndoDeleteAgain });
        }
        Stage::WaitUndoDelete | Stage::WaitUndoDeleteAgain => {
            let copied = state.copied.as_deref().unwrap();
            if record(world,copied).is_some() && history(world) == state.undo + 1 && ready(world) && output(world,copied)?.is_some() {
                state.next(if state.stage == Stage::WaitUndoDelete { Stage::RedoDelete } else { Stage::UndoCopy });
            }
        }
        Stage::RedoDelete => { *pulse = Some(action(world,EditorAction::Redo)); state.next(Stage::WaitRedoDelete); }
        Stage::UndoCopy | Stage::UndoCopyFinal => {
            *pulse = Some(action(world,EditorAction::Undo));
            state.next(if state.stage == Stage::UndoCopy { Stage::WaitUndoCopy } else { Stage::WaitUndoCopyFinal });
        }
        Stage::WaitUndoCopy | Stage::WaitUndoCopyFinal => {
            let copied = state.copied.as_deref().unwrap();
            if record(world,copied).is_none() && history(world) == state.undo && output(world,copied)?.is_none()
                && Arc::ptr_eq(&targets(world)?,&state.targets) && ready(world)
            {
                if state.stage == Stage::WaitUndoCopy { state.next(Stage::RedoCopy); }
                else {
                    state.proofs.push(json!({"delete_hides_actual_vegetation":true,"undo_delete_restores_actual_vegetation":true,
                        "redo_delete_hides_again":true,"copy_undo_redo_restores_joint_model_data":true,
                        "original_author_arc_preserved":true,"temporary_copy_removed_by_original_undo":true}));
                    select(world,MODEL); state.next(Stage::Raise);
                }
            }
        }
        Stage::RedoCopy => { *pulse = Some(action(world,EditorAction::Redo)); state.next(Stage::WaitRedoCopy); }
        Stage::WaitRedoCopy => {
            let copied = state.copied.as_deref().unwrap();
            if record(world,copied).is_some() && history(world) == state.undo + 1 && ready(world)
                && output(world,copied)?.is_some() && Arc::ptr_eq(&targets(world)?,state.copied_targets.as_ref().unwrap())
            { state.next(Stage::UndoCopyFinal); }
        }
        Stage::Raise => {
            let paths = feathrise_editor_core::editor_project_paths().ok_or("模型热更缺少作者路径")?;
            let (project,assets) = (paths.project_root().to_owned(), paths.asset_root().to_owned());
            let output = PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("模型热更缺少owned输出")?);
            let placement_hash = world.resource::<EditorAuthoringSession>().draft.as_ref()
                .and_then(|draft| draft.placement_glb_hash.clone()).ok_or("模型热更缺少私有 GLB 保存基线")?;
            state.runtime_before = world.resource::<DynamicModelVegetationStatus>().runtime_revision;
            state.io = Some(IoTaskPool::get().spawn(async move {
                let source = lifecycle_io::prepare(project,assets,output,placement_hash)?;
                if let Err(error) = lifecycle_io::apply(&source,Mutation::Raise) {
                    let restored = lifecycle_io::apply(&source,Mutation::Restore);
                    return Err(format!("{error}；源恢复：{restored:?}"));
                }
                Ok(source)
            }));
            state.next(Stage::WaitRaiseIo);
        }
        Stage::WaitRaiseIo | Stage::WaitTopologyIo | Stage::WaitRestoreIo => {
            if let Some(result) = state.io.as_mut().and_then(check_ready) {
                state.io = None; state.source = Some(result?);
                world.resource::<AssetServer>().reload(super::types::SOURCE);
                state.next(match state.stage { Stage::WaitRaiseIo => Stage::WaitRaise,
                    Stage::WaitTopologyIo => Stage::WaitTopology, _ => Stage::WaitRestore });
            }
        }
        Stage::WaitRaise => {
            unchanged(state,world)?;
            if fresh(world,state.runtime_before) && output(world,MODEL)?.is_some_and(|o|o.blade_height.is_some_and(|y|(y-0.35).abs()<0.001)) {
                state.proofs.push(json!({"same_topology_vertex_reload":true,"actual_blade_local_height":0.35,
                    "author_samples_unchanged":true,"runtime_revision":world.resource::<DynamicModelVegetationStatus>().runtime_revision}));
                state.next(Stage::Topology);
            }
        }
        Stage::Topology | Stage::Restore => {
            if state.io.is_some() { return Ok(None); }
            state.runtime_before = world.resource::<DynamicModelVegetationStatus>().runtime_revision;
            let source = state.source.as_ref().ok_or("模型热更没有恢复来源")?.clone();
            let mutation = if state.stage == Stage::Topology { Mutation::Topology } else { Mutation::Restore };
            state.io = Some(IoTaskPool::get().spawn(async move { lifecycle_io::apply(&source,mutation)?; Ok(source) }));
            state.next(if state.stage == Stage::Topology { Stage::WaitTopologyIo } else { Stage::WaitRestoreIo });
        }
        Stage::WaitTopology => {
            unchanged(state,world)?;
            let status = world.resource::<DynamicModelVegetationStatus>().clone();
            if fresh(world,state.runtime_before) && status.detail.contains("重新绑定")
                && status.surface_count == 0 && status.blade_window_count == 0 && status.mesh_batch_count == 0
                && output(world,MODEL)?.is_none()
            {
                state.proofs.push(json!({"topology_reload_invalidates_anchors":true,"author_samples_unchanged":true,
                    "old_runtime_output_removed":true,"detail":status.detail}));
                state.next(Stage::Restore);
            }
        }
        Stage::WaitRestore => {
            unchanged(state,world)?;
            if fresh(world,state.runtime_before) && output(world,MODEL)?.is_some_and(|o|o.blade_height.is_some_and(|y|(y-0.15).abs()<0.001)) {
                state.proofs.push(json!({"owned_source_original_bytes_restored_and_read_back":true,
                    "runtime_reloaded_original_geometry":true,"actual_blade_local_height":0.15}));
                if let Some(error) = state.error.take() { return Err(error); }
                state.next(Stage::Complete);
            }
        }
        Stage::Complete => { return Ok(Some(json!({"kind":"model_lifecycle","checks":state.proofs}))); }
    }
    Ok(None)
}

fn action(world: &mut World, action: EditorAction) -> Entity {
    world.spawn((SceneEditorActionButton(action),EditorButtonClick::triggered())).id()
}
fn object_action(world: &mut World, action: PlacementTransformAction) -> Entity {
    world.spawn((SceneEditorPlacementTransformActionButton(action),EditorButtonClick::triggered())).id()
}
fn select(world: &mut World, id: &str) {
    let selection = world.resource::<PlacementSelectionSet>();
    if selection.members.len() != 1 || selection.members[0].id != id {
        world.resource_mut::<PlacementSelectionSet>().clear();
    }
    let mut browser = world.resource_mut::<EditorGltfBrowserState>();
    browser.selected_placement_id = Some(id.to_owned()); browser.selection = None;
}
fn record<'a>(world: &'a World, id: &str) -> Option<&'a ScenePlacementRecord> {
    world.resource::<EditorAuthoringSession>().draft.as_ref()?.placements.placements.iter().find(|r|r.id==id)
}
fn targets(world: &World) -> Result<Arc<Vec<Arc<ModelSurfaceVegetation>>>, String> {
    Ok(model_vegetation::root_targets(world.resource::<EditorAuthoringSession>().draft.as_ref().ok_or("模型生命周期草稿丢失")?))
}
fn history(world: &World) -> usize { world.resource::<EditorAuthoringSession>().undo.len() }
fn batch_busy(world: &World) -> bool { let batch=world.resource::<PlacementBatchState>(); batch.busy() || batch.owns_mouse() }
fn transform_value(index: usize) -> (PlacementTransformField,f32) {
    [(PlacementTransformField::TranslationX,13.),(PlacementTransformField::Yaw,35.),(PlacementTransformField::ScaleX,1.25)][index]
}
fn transformed(original: &ScenePlacementRecord, count: usize) -> ScenePlacementRecord {
    let mut record=original.clone();
    if count>0 { record.translation[0]=13.; } if count>1 { record.yaw_degrees=35.; } if count>2 { record.scale[0]=1.25; }
    record
}
fn same_copy(before: &[Arc<ModelSurfaceVegetation>], after: &[Arc<ModelSurfaceVegetation>], copied: &str) -> Result<(),String> {
    let find = |values: &[Arc<ModelSurfaceVegetation>],id: &str| values.iter().position(|d|matches!(&d.target,SurfaceTarget::Model{object_id} if object_id==id));
    let source=&before[find(before,MODEL).ok_or("模型副本缺少来源作者样本")?];
    let duplicate=&after[find(after,copied).ok_or("新对象没有重绑模型植被")?];
    if duplicate.topology!=source.topology || duplicate.samples!=source.samples || !duplicate.sample_blocks.is_empty()
        || !Arc::ptr_eq(&after[find(after,MODEL).ok_or("复制后来源样本丢失")?],source)
    { return Err("模型复制改变原样本或漏掉副本锚点/密度".into()); }
    Ok(())
}
fn unchanged(state: &Lifecycle,world: &World) -> Result<(),String> {
    if !Arc::ptr_eq(&targets(world)?,&state.targets) { return Err("几何热更改写了应保留的作者样本Arc".into()); } Ok(())
}
fn ready(world: &World) -> bool {
    let Some(draft)=world.resource::<EditorAuthoringSession>().draft.as_ref() else { return false; };
    let status=world.resource::<DynamicModelVegetationStatus>();
    world.resource::<DynamicModelVegetationDraft>().frozen_for_scene(&draft.scene_id).is_some_and(|(revision,data)|
        status.ready && !status.failed && status.scene_id.as_ref()==Some(&draft.scene_id)
            && status.applied_revision==Some(revision) && status.applied_document==Some(Arc::as_ptr(&data) as usize))
}
fn fresh(world: &World,previous: u64) -> bool { ready(world) && world.resource::<DynamicModelVegetationStatus>().runtime_revision>previous }

struct Output { local: Mat4, blade_height: Option<f32> }
fn follows(world: &mut World,id: &str,record: &ScenePlacementRecord) -> Result<bool,String> {
    let Some(output)=output(world,id)? else { return Ok(false); };
    let expected=Mat4::from_scale_rotation_translation(Vec3::from_array(record.scale),
        Quat::from_rotation_y(record.yaw_degrees.to_radians()),Vec3::from_array(record.translation));
    Ok(matrix_eq(output.local,expected))
}
fn matrix_eq(a: Mat4,b: Mat4) -> bool { a.to_cols_array().iter().zip(b.to_cols_array()).all(|(a,b)|(*a-b).abs()<0.001) }

/// 验收只遍历本轮最多16个源根及最多256个直接孩子，不扫描整份模型或替运行时创建输出。
fn output(world: &mut World,id: &str) -> Result<Option<Output>,String> {
    let candidates: Vec<Entity> = world.query::<(Entity,&ScenePlacementInstance,&InheritedVisibility)>().iter(world)
        .filter(|(_,instance,visible)|instance.id()==id && instance.scene_id().as_str()=="M1-SAMPLE" && instance.is_active() && visible.get())
        .take(17).map(|(entity,_,_)|entity).collect();
    if candidates.len()>16 { return Err("模型生命周期源根超过固定预算".into()); }
    let mut found=None;
    for source in candidates {
        let Some(children)=world.get::<Children>(source) else { continue; };
        if children.len()>256 { return Err("模型生命周期源孩子超过固定预算".into()); }
        for child in children.iter() {
            if world.get::<Name>(child).is_none_or(|n|n.as_str()!=format!("模型植被_{id}"))
                || world.get::<InheritedVisibility>(child).is_none_or(|v|!v.get()) { continue; }
            let source_frame=world.get::<GlobalTransform>(source).ok_or("模型源缺少实际世界变换")?.to_matrix();
            let child_frame=world.get::<GlobalTransform>(child).ok_or("模型植被缺少实际世界变换")?.to_matrix();
            if !matrix_eq(source_frame,child_frame) { return Ok(None); } // 正常层级传播尚未完成时等待下一帧。
            let local=world.get::<Transform>(source).ok_or("模型源缺少局部姿态")?.to_matrix();
            let mut heights=Vec::new();
            if let Some(leaves)=world.get::<Children>(child) {
                if leaves.len()>256 { return Err("模型植被子树超过固定预算".into()); }
                for leaf in leaves.iter() {
                    if world.get::<GpuBladePatch>(leaf).is_some() {
                        heights.push(world.get::<Transform>(leaf).ok_or("实际草窗缺少变换")?.translation.y);
                    }
                }
            }
            if found.is_some() { return Err("同一模型存在两个可见植被输出，疑似旧结果未撤销".into()); }
            let blade_height=heights.first().copied().filter(|height|heights.iter().all(|y|(y-height).abs()<0.001));
            found=Some(Output{local,blade_height});
        }
    }
    Ok(found)
}
