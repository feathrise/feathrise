//! 只安装于显式自有夹具的几何统一专项；创建、LOD 拒绝、原指针画笔与历史全部走正式业务。

mod pointer;
mod diagnostic;
mod hover_query;
mod layer; // 第二个独立Blade输出通过原预设创建、保存和热采用门准备。
mod readback;
mod readback_density;
mod resources;
mod screenshot;
mod types;
pub use types::GeometryUnificationProof;

use resources::GeometryFixture;
use types::{Creation, Domain, GeometryStrokeProof, Phase};
use crate::{authoring::{EditorAuthoringSession, TerrainBrushStroke, VegetationBrushStroke,
    terrain_documents::{self, TerrainDocumentTask}},
    components::SceneEditorActionButton, resources::{SceneEditorState, SceneEditorViewportState},
    types::EditorAction, ui::terrain_objects::{self, CreationSource, Mutation, TerrainObjectsEditorState}};
use bevy::{prelude::*, ecs::system::RunSystemOnce, tasks::futures::check_ready};
use feathrise_scene_content::{SceneTerrainObject, TerrainId};
use std::time::{Duration, Instant};

pub(super) fn install(app: &mut App) {
    app.init_resource::<GeometryFixture>()
        .add_systems(Update, pointer::inject.in_set(crate::SceneEditorSystemSet::Cursor)
            .before(crate::viewport::sync_editor_viewport_camera))
        .add_systems(Last, drive);
}

pub(super) fn request(world: &mut World) -> Result<(), String> {
    if !world.contains_resource::<super::FixtureInstalled>() { return Err("几何验收未获得自有工程安装权限".into()); }
    if world.resource::<GeometryFixture>().phase != Phase::Idle { return Err("几何专项不可重复请求".into()); }
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().ok_or("几何验收需要已打开的真实场景")?;
    if draft.scene_id.as_str() != "M1-SAMPLE" || draft.manifest.terrain_objects_for_edit().len() > 1
        || session.saving || world.resource::<TerrainObjectsEditorState>().busy() {
        return Err("几何验收只操作本轮 M1-SAMPLE 小夹具，且须等待原后台槽空闲".into());
    }
    let scene = draft.scene_id.clone();
    let editor = world.resource::<SceneEditorState>();
    let viewport = world.resource::<SceneEditorViewportState>();
    let view = (editor.view, editor.orthographic, viewport.center, viewport.extent, viewport.yaw, viewport.pitch);
    let mut state = world.resource_mut::<GeometryFixture>();
    state.scene = Some(scene.clone()); state.view = Some(view); state.started = Instant::now();
    state.proof.scene_id = scene.as_str().into();
    state.proof.input = "original creation request, LOD mutation, ECS pointer, Undo/Redo; physical mouse not exercised".into();
    state.proof.parent_save_required = true;
    state.advance(Phase::Create);
    Ok(())
}

pub(super) fn result(world: &World) -> Option<GeometryUnificationProof> {
    let state = world.get_resource::<GeometryFixture>()?;
    (state.phase != Phase::Idle).then(|| state.proof.clone())
}

pub(super) fn scene_tools_mesh(world: &World) -> Option<&feathrise_scene_content::VegetationPresetDescriptor> { layer::scene_tools_mesh(world) }

fn object(world: &World, id: &TerrainId) -> Option<SceneTerrainObject> {
    let draft = world.resource::<EditorAuthoringSession>().draft.as_ref()?;
    let objects = draft.manifest.terrains.as_ref()?;
    (objects.len() <= 8).then(|| objects.iter().find(|object| &object.id == id).cloned()).flatten()
}

fn idle(world: &World) -> bool { !world.resource::<TerrainObjectsEditorState>().busy() }

fn mutate(world: &mut World, mutation: Mutation) -> Result<(), String> {
    world.resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
        world.resource_scope(|world, mut selection: Mut<TerrainObjectsEditorState>| {
            terrain_objects::start(&mut session, &mut world.resource_mut::<SceneEditorState>(), &mut selection, mutation);
            selection.preparing().then_some(()).ok_or("原网格事务没有受理验收请求".into())
        })
    })
}

fn pulse(world: &mut World, action: EditorAction) -> Entity {
    world.spawn((SceneEditorActionButton(action), feathrise_editor_ui::EditorButtonClick::triggered())).id()
}

fn focus(world: &mut World, id: &TerrainId) -> Result<(), String> {
    let accepted = world.resource_scope(|world, session: Mut<EditorAuthoringSession>| {
        world.resource_scope(|world, mut task: Mut<TerrainDocumentTask>| {
            terrain_documents::request_focus(&mut task, &mut world.resource_mut::<TerrainObjectsEditorState>(),
                session.draft.as_ref().unwrap(), id)
        })
    });
    if !accepted { return Err("原地形画布未受理专项选择".into()); }
    world.resource_mut::<SceneEditorState>().terrain_canvas_target = Some(id.clone());
    Ok(())
}

fn ready(world: &mut World, state: &GeometryFixture) -> bool {
    if !idle(world) { return false; }
    if world.get_resource::<crate::authoring::sparse_terrain::SparseTerrainAuthoringState>()
        .is_some_and(|sparse| !sparse.document_can_park()) { return false; }
    let session = world.resource::<EditorAuthoringSession>();
    if session.saving { return false; }
    let Some(draft) = session.draft.as_ref() else { return false; };
    let Some(id) = state.target() else { return false; };
    if draft.terrain_documents.loading || draft.terrain_documents.active.as_ref().map(|v| &v.definition.id) != Some(id)
        || draft.terrain_ground.as_ref().filter(|ground| ground.revision == session.terrain_revision)
            .and_then(|ground| ground.object_surface(id)).is_none() { return false; }
    let scene = draft.scene_id.clone(); let id = id.clone();
    if matches!(state.domain, Domain::Height | Domain::HeightVegetation)
        && !world.run_system_once(|session: Res<EditorAuthoringSession>,
            sparse: Res<crate::authoring::sparse_terrain::SparseTerrainAuthoringState>,
            live: Res<feathrise_scene_content::SparseHeightLiveOverride>,
            ground: feathrise_scene::TerrainObjectClipmapGround| {
                sparse.current_height_snapshot(&session, &live, &ground).is_some()
            }).unwrap_or(false) { return false; }
    world.run_system_once(move |queries: feathrise_scene::TerrainObjectQueries| queries.collision_is_ready(&scene, &id)).unwrap_or(false)
}

/// 超时仅记录固定槽身份，不遍历作者像素，也不降低原画布、修订和碰撞门槛。
fn wait_diagnostic(world: &mut World, state: &GeometryFixture) -> String {
    let canvas_busy = world.run_system_once(|idle: terrain_documents::CanvasIdle,
        session: Res<EditorAuthoringSession>| idle.busy(&session)).unwrap_or(true);
    let session = world.resource::<EditorAuthoringSession>();
    let Some(draft) = session.draft.as_ref() else { return "draft=None".into(); };
    let scene = draft.scene_id.clone(); let id = state.target().cloned();
    let identity = format!("target={id:?},active={:?},loading={},objects_busy={},canvas_busy={canvas_busy},ground_revision={:?}/{},ground_target={},vegetation_revision={}/{},author_dirty={},model_busy={},sparse_dirty={},history={}/{}",
        draft.terrain_documents.active.as_ref().map(|active| &active.definition.id), draft.terrain_documents.loading,
        !idle(world), draft.terrain_ground.as_ref().map(|ground| ground.revision), session.terrain_revision,
        draft.terrain_ground.as_ref().is_some_and(|ground| id.as_ref().is_some_and(|id| ground.object_surface(id).is_some())),
        session.vegetation_compiled_revision, session.vegetation_revision, !draft.vegetation_author_dirty_layers.is_empty(),
        session.model_vegetation_busy, session.sparse_terrain_dirty, draft.terrain_documents.scoped_through, session.undo.len());
    let collision = id.is_some_and(|id| world.run_system_once(move |queries: feathrise_scene::TerrainObjectQueries|
        queries.collision_is_ready(&scene, &id)).unwrap_or(false));
    format!("{identity},collision_ready={collision}")
}

fn drive(world: &mut World) {
    let phase = world.resource::<GeometryFixture>().phase;
    if matches!(phase, Phase::Idle | Phase::Complete | Phase::Failed) { return; }
    world.resource_scope(|world, mut state: Mut<GeometryFixture>| {
        if let Some(entity) = state.pulse.take() { let _ = world.despawn(entity); }
        if let Err(error) = advance(world, &mut state) {
            state.proof.error = Some(format!("{:?}: {error}", state.phase));
            pointer::restore(world, &mut state); state.advance(Phase::Failed);
        }
    });
}

fn advance(world: &mut World, state: &mut GeometryFixture) -> Result<(), String> {
    hover_query::poll(world, state);
    if state.started.elapsed() > Duration::from_secs(300) {
        let status = world.resource::<SceneEditorState>().status.clone();
        return Err(format!("原业务等待超时：{status}；{}", wait_diagnostic(world, state)));
    }
    if world.resource::<EditorAuthoringSession>().draft.as_ref().map(|d| &d.scene_id) != state.scene.as_ref() {
        return Err("验收场景换代，拒绝继续操作".into());
    }
    match state.phase {
        Phase::Create if idle(world) => {
            let draft = world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
            state.ids_before = draft.manifest.terrain_objects_for_edit().iter().map(|o| o.id.clone()).collect();
            let (source, mut values) = match state.creation {
                Creation::Tiny => (CreationSource::Clipmap, [0.,0.,0.,0.01,16.,0.01,0.,0.125]),
                Creation::Small => (CreationSource::Clipmap, [0.,0.,0.,4.,16.,4.,0.,0.125]),
                Creation::Huge => (CreationSource::Clipmap, [0.,0.,0.,8192.,16.,8192.,0.,0.125]),
                // 原生体素解析盒体经过正式提取；两个相反表面须落入同一个0.25m作者格。
                // 后续回读检查真实样本，若显示采样抹掉薄板则验收失败，不能以创建参数判成功。
                Creation::Voxel => (CreationSource::Voxel, [0.,0.,0.,4.,0.05,4.,0.,0.25]),
            };
            values[..3].copy_from_slice(&state.creation.origin().to_array());
            terrain_objects::validation_request_creation(world, source, values)?;
            state.advance(Phase::WaitCreate);
        }
        Phase::WaitCreate if idle(world) => {
            let draft = world.resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
            let objects = draft.manifest.terrains.as_ref().ok_or("原创建没有发布对象集合")?;
            let added: Vec<_> = objects.iter().filter(|o| !state.ids_before.contains(&o.id)).collect();
            if added.is_empty() { return Ok(()); }
            if added.len() != 1 { return Err("原创建必须恰好新增一个地形".into()); }
            let id = added[0].id.clone();
            match state.creation {
                Creation::Tiny => { state.tiny = Some(id); state.advance(Phase::TinyReadback); }
                Creation::Small => { state.small = Some(id); state.creation = Creation::Huge; state.advance(Phase::Create); }
                Creation::Huge => { state.huge = Some(id); state.advance(Phase::ReduceLods); }
                Creation::Voxel => { state.voxel = Some(id); state.domain = Domain::Voxel; state.advance(Phase::Focus); }
            }
        }
        Phase::TinyReadback => {
            if state.task.is_none() {
                let tiny = object(world, state.tiny.as_ref().unwrap()).ok_or("微型高度地形丢失")?;
                state.task = Some(readback::start(feathrise_editor_core::workspace_asset_root(), state.scene.clone().unwrap(), tiny, None, None));
            } else if let Some(result) = state.task.as_mut().and_then(check_ready) {
                state.task = None; let proof = result?;
                if proof.cells != [1,1] || proof.world_extent != [0.01,0.01] || proof.display_lod_count != 1 {
                    return Err("0.01米高度地形被扩大或没有保留单层完整来源".into());
                }
                state.proof.tiny = Some(proof); state.creation = Creation::Small; state.advance(Phase::Create);
            }
        }
        Phase::ReduceLods if idle(world) => {
            let huge = object(world, state.huge.as_ref().unwrap()).ok_or("预算夹具已失效")?;
            let count = huge.terrain.display_lods.as_ref().ok_or("预算夹具无显示LOD")?.len();
            if count == 1 { state.advance(Phase::RejectBudget); }
            else { state.lod_count = count; mutate(world, Mutation::RemoveLod(huge.id, true, 0))?; state.advance(Phase::WaitLods); }
        }
        Phase::WaitLods if idle(world) => {
            let count = object(world, state.huge.as_ref().unwrap()).unwrap().terrain.display_lods.unwrap().len();
            if count + 1 != state.lod_count { return Err("原显示LOD删除没有采用完整结果".into()); }
            state.advance(Phase::ReduceLods);
        }
        Phase::RejectBudget if idle(world) => {
            let huge = object(world, state.huge.as_ref().unwrap()).unwrap();
            state.rejection_before = Some((world.resource::<EditorAuthoringSession>().revision, huge.clone()));
            // 单层粗网格合法；把唯一层倍率改为1后必须按完整8192m来源计算并拒绝，不能截断范围。
            mutate(world, Mutation::Value(huge.id, 9, 1., true))?; state.advance(Phase::WaitRejectBudget);
        }
        Phase::WaitRejectBudget if idle(world) => {
            let (revision, previous) = state.rejection_before.as_ref().unwrap();
            let status = world.resource::<SceneEditorState>().status.clone();
            if object(world, &previous.id).as_ref() != Some(previous)
                || world.resource::<EditorAuthoringSession>().revision != *revision
                || !status.contains("失败") || !(status.contains("预算") || status.contains("MiB") || status.contains("上限")) {
                return Err(format!("超预算请求没有明确失败并保留原对象：{status}"));
            }
            state.proof.overbudget_error = Some(status); state.proof.overbudget_source_unchanged = true;
            state.advance(Phase::RemoveHuge);
        }
        Phase::RemoveHuge if idle(world) => {
            mutate(world, Mutation::Remove(state.huge.clone().unwrap()))?; state.advance(Phase::WaitRemoveHuge);
        }
        Phase::WaitRemoveHuge if idle(world) => {
            if object(world, state.huge.as_ref().unwrap()).is_none() { state.advance(Phase::Focus); }
        }
        Phase::Focus if idle(world) => { focus(world, state.target().unwrap())?; state.advance(Phase::WaitFocus); }
        Phase::WaitFocus if ready(world, state) => {
            state.advance(if state.domain == Domain::Height { Phase::Before } else { Phase::CreateLayer });
        }
        Phase::CreateLayer | Phase::WaitLayerPreset | Phase::SaveLayerPreset | Phase::WaitLayerPresetSave
            | Phase::CloseLayerPreset | Phase::WaitLayerPresetClose => layer::advance(world, state)?,
        Phase::WaitLayer => {
            let session = world.resource::<EditorAuthoringSession>();
            if let Some(author) = session.draft.as_ref().unwrap().vegetation.as_ref().and_then(|v| v.authoring.as_ref())
                .filter(|a| a.layers.len() == state.author_count + 1) {
                let index = author.layers.len()-1;
                if state.domain == Domain::VoxelUnderside {
                    state.proof.underside_vegetation_output_layer = Some(author.layers[index].output_layer_id.clone());
                } else if state.domain == Domain::Voxel {
                    state.proof.vegetation_output_layer = Some(author.layers[index].output_layer_id.clone());
                }
                world.resource_mut::<SceneEditorState>().selected_vegetation_layer = index;
                state.advance(Phase::Before);
            }
        }
        Phase::Before | Phase::After | Phase::Undone | Phase::Redone => capture_phase(world, state)?,
        Phase::Hover => {
            let editor = world.resource::<SceneEditorState>();
            if editor.brush_terrain_id.as_ref() == state.target() && editor.brush_cursor_world.is_some()
                && idle(world) && state.phase_started.elapsed() > Duration::from_millis(100) {
                state.hit = editor.brush_cursor_world;
                state.undo_before = world.resource::<EditorAuthoringSession>().undo.len(); state.advance(Phase::Paint);
            }
            if state.phase == Phase::Hover && state.phase_started.elapsed() > Duration::from_secs(10) {
                // 原曲面已经在Before前完成就绪门；稳定小射线十秒仍未命中应立即保留真实原因，不能耗尽整轮300秒。
                return Err(format!("真实悬停十秒未命中目标：{}；{}；后台实查询={}", wait_diagnostic(world, state),
                    diagnostic::hover(world), state.proof.hover_queries.last().unwrap_or(&serde_json::Value::Null)));
            }
        }
        Phase::Paint => {
            // 稀疏雕刻在松笔后才提交整笔历史，不能按住鼠标等待一条尚不应产生的记录。
            if state.phase_started.elapsed() > Duration::from_millis(700) { state.advance(Phase::Release); }
        }
        Phase::Release if !state.mouse_owned && idle(world)
            && !world.resource::<TerrainBrushStroke>().active && !world.resource::<VegetationBrushStroke>().active
            && world.get_resource::<crate::authoring::sparse_terrain::SparseTerrainAuthoringState>()
                .is_none_or(|sparse| sparse.document_can_park()) => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.undo.len() != state.undo_before+1 { return Err("一次原画笔拖动必须只产生一条历史".into()); }
            state.advance(Phase::After);
        }
        Phase::Undo => { state.pulse = Some(pulse(world, EditorAction::Undo)); state.advance(Phase::WaitUndo); }
        Phase::WaitUndo => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.undo.len() == state.undo_before && session.redo.len() == 1 { state.advance(Phase::Undone); }
        }
        Phase::Redo => { state.pulse = Some(pulse(world, EditorAction::Redo)); state.advance(Phase::WaitRedo); }
        Phase::WaitRedo => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.undo.len() == state.undo_before+1 && session.redo.is_empty() { state.advance(Phase::Redone); }
        }
        Phase::Settle => {
            if ready(world, state) && screenshot::runtime_ready(world, state)? {
                state.stable_frames = state.stable_frames.saturating_add(1);
                if state.stable_frames >= 5 && state.phase_started.elapsed() > Duration::from_millis(600) {
                    screenshot::request(world, state)?;
                    state.advance(Phase::Capture);
                }
            } else { state.stable_frames = 0; }
        }
        Phase::Capture => {
            let result = state.capture.try_lock().ok().and_then(|mut slot| slot.take());
            if let Some(result) = result {
                result?; // GPU捕获和后台PNG写盘均完成，才允许改变画布或向宿主报告完成。
                if state.capture_revision != world.resource::<EditorAuthoringSession>().revision {
                    return Err("GPU截图期间作者身份换代".into());
                }
                if state.domain == Domain::Height {
                    state.domain = Domain::HeightVegetation; state.advance(Phase::CreateLayer);
                } else if state.domain == Domain::HeightVegetation {
                    state.proof.verify_height_vegetation()?;
                    state.advance(Phase::SaveHeight);
                } else if state.domain == Domain::Voxel {
                    state.domain = Domain::VoxelUnderside; state.advance(Phase::CreateLayer);
                } else {
                    state.proof.verify_volume_sides()?;
                    state.proof.complete = true; pointer::restore(world, state); state.advance(Phase::Complete);
                }
            }
        }
        Phase::SaveHeight if ready(world, state) => {
            state.save_before = world.resource::<EditorAuthoringSession>().successful_save_sequence;
            state.pulse = Some(pulse(world, EditorAction::Save));
            state.advance(Phase::WaitSaveHeight);
        }
        Phase::WaitSaveHeight => {
            let session = world.resource::<EditorAuthoringSession>();
            let editor = world.resource::<SceneEditorState>();
            if session.successful_save_sequence > state.save_before && !session.saving
                && !session.sparse_terrain_dirty && !editor.save_requested && !editor.dirty {
                state.proof.height_save_sequence = Some(session.successful_save_sequence);
                state.advance(Phase::ReadSavedHeight);
            }
        }
        Phase::ReadSavedHeight => {
            if state.task.is_none() {
                let expected = state.proof.height_vegetation.as_ref().unwrap().after.density.clone().unwrap();
                let object = object(world, state.small.as_ref().unwrap()).ok_or("已保存小Clipmap丢失")?;
                state.task = Some(readback::start_saved_height(feathrise_editor_core::workspace_asset_root(),
                    state.scene.clone().unwrap(), object, expected));
            } else if let Some(result) = state.task.as_mut().and_then(check_ready) {
                state.task = None;
                if result? != state.proof.height_vegetation.as_ref().unwrap().after {
                    return Err("小Clipmap原保存的高度、作者密度或正式R8不同于重做结果".into());
                }
                state.proof.height_density_saved_matches = true;
                state.creation = Creation::Voxel; state.advance(Phase::Create);
            }
        }
        _ => {}
    }
    Ok(())
}

fn capture_phase(world: &mut World, state: &mut GeometryFixture) -> Result<(), String> {
    if !ready(world, state) { return Ok(()); }
    if state.task.is_none() {
        let session = world.resource::<EditorAuthoringSession>();
        if state.domain == Domain::HeightVegetation
            && (session.vegetation_compiled_revision != session.vegetation_revision
                || session.vegetation_compiled_structure_revision != session.vegetation_structure_revision) { return Ok(()); }
        state.capture_revision = session.revision;
        let density = if state.domain == Domain::HeightVegetation {
            Some(readback_density::freeze(session, world.resource::<SceneEditorState>().selected_vegetation_layer)?)
        } else { None };
        let live = if matches!(state.domain, Domain::Height | Domain::HeightVegetation) {
            let live = world.get_resource::<feathrise_scene_content::SparseHeightLiveOverride>()
                .filter(|live| live.scene_id() == state.scene.as_ref()).ok_or("当前高度来源缺少同场景实时快照")?;
            Some(live.snapshot())
        } else { None };
        let object = object(world, state.target().unwrap()).ok_or("原地形已删除")?;
        state.task = Some(readback::start(feathrise_editor_core::workspace_asset_root(), state.scene.clone().unwrap(), object, live, density));
        return Ok(());
    }
    let Some(result) = state.task.as_mut().and_then(check_ready) else { return Ok(()); };
    state.task = None;
    if state.capture_revision != world.resource::<EditorAuthoringSession>().revision { return Err("取证期间作者身份换代".into()); }
    let proof = result?;
    match state.phase {
        Phase::Before => { state.before = Some(proof); state.undo_verified = false; state.advance(Phase::Hover); }
        Phase::After => {
            if proof.sha256 == state.before.as_ref().unwrap().sha256 || proof.nonzero_samples == 0 {
                return Err("原画笔没有形成真实非空作者数据".into());
            }
            if state.domain == Domain::Height && proof.display_lod_count != 1 { return Err("小高度地形未保持单LOD".into()); }
            state.after = Some(proof); state.advance(Phase::Undo);
        }
        Phase::Undone => {
            if proof.sha256 != state.before.as_ref().unwrap().sha256 { return Err("原撤销没有精确恢复来源".into()); }
            state.undo_verified = true; state.advance(Phase::Redo);
        }
        Phase::Redone => {
            if proof.sha256 != state.after.as_ref().unwrap().sha256 { return Err("原重做没有精确恢复来源".into()); }
            let stroke = GeometryStrokeProof { before: state.before.take().unwrap(), after: state.after.take().unwrap(),
                undo_matches_before: state.undo_verified, redo_matches_after: true, collision_ready: true,
                surface_hit: state.hit.unwrap().to_array(), screenshot: screenshot::file_name(state.domain).into() };
            if state.domain == Domain::Height {
                state.proof.small = Some(stroke);
            } else if state.domain == Domain::HeightVegetation {
                state.proof.height_vegetation = Some(stroke);
            } else if state.domain == Domain::Voxel {
                state.proof.voxel = Some(stroke);
            } else {
                state.proof.voxel_underside = Some(stroke);
                state.proof.verify_volume_sides()?;
            }
            state.stable_frames = 0; state.advance(Phase::Settle);
        }
        _ => unreachable!(),
    }
    Ok(())
}

/// 宿主在原保存完成并退出后调用；复用实际源回读，避免另造一套分页/体积摘要口径。
pub(super) fn verify_saved(asset_root: &std::path::Path, proof: &GeometryUnificationProof) -> Result<(), String> {
    readback::verify_saved(asset_root, proof)
}
