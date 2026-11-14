//! 串行驱动原领域入口，等待真实采用后再核对一笔Undo/Redo，不实现替代保存或绘制。

use super::{BLADE_SOURCE, capture, pointer, resources::TerrainWorkflow, types::*};
use crate::{
    authoring::{
        EditorAuthoringSession, SurfaceBrushStroke, TerrainBrushStroke, VegetationBrushStroke,
        sparse_terrain::SparseTerrainAuthoringState,
        terrain_documents::{self, TerrainDocumentTask},
    },
    components::SceneEditorActionButton,
    resources::SceneEditorState,
    types::{EditorAction, EditorTool},
    ui::{
        terrain_objects::TerrainObjectsEditorState,
        vegetation_preset_editor::{
            PresetEditorAction, PresetEditorButton, VegetationPresetEditorState,
        },
    },
};
use bevy::{prelude::*, tasks::futures::check_ready};
use feathrise_editor_surface_model::{
    components::SurfacePresetButton, resources::SurfacePresetEditorState,
    types::SurfacePresetAction,
};
use feathrise_scene_content::{SurfacePresetRegistry, VegetationPresetRegistry};
use std::time::Duration;

fn pulse_scene(world: &mut World, action: EditorAction) -> Entity {
    world
        .spawn((
            SceneEditorActionButton(action),
            feathrise_editor_ui::EditorButtonClick::triggered(),
        ))
        .id()
}
fn pulse_surface(world: &mut World, action: SurfacePresetAction) -> Entity {
    world
        .spawn((
            SurfacePresetButton(action),
            feathrise_editor_ui::EditorButtonClick::triggered(),
        ))
        .id()
}
fn pulse_blade(world: &mut World, action: PresetEditorAction) -> Entity {
    world
        .spawn((
            PresetEditorButton(action),
            feathrise_editor_ui::EditorButtonClick::triggered(),
        ))
        .id()
}

pub(super) fn advance(world: &mut World, state: &mut TerrainWorkflow) -> Result<(), String> {
    if state.clock.start.elapsed() > Duration::from_secs(240) {
        let preview_ready = capture::ready(world, state);
        return Err(format!(
            "原地形工具验收超时：{:?}/{:?}；{}；undo={}/{}；preview={}；首末刷印={:?}/{:?}",
            state.domain,
            state.phase,
            world.resource::<SceneEditorState>().status,
            world.resource::<EditorAuthoringSession>().undo.len(),
            state.undo_before,
            preview_ready,
            state.first,
            state.last,
        ));
    }
    if world
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .map(|draft| &draft.scene_id)
        != state.scene_id.as_ref()
    {
        return Err("验收中原场景身份发生变化，拒绝串写".into());
    }
    match state.phase {
        Phase::Focus => {
            let id = state.terrain.as_ref().unwrap().clone();
            let accepted = world.resource_scope(|world, session: Mut<EditorAuthoringSession>| {
                world.resource_scope(|world, mut task: Mut<TerrainDocumentTask>| {
                    terrain_documents::request_focus(
                        &mut task,
                        &mut world.resource_mut::<TerrainObjectsEditorState>(),
                        session.draft.as_ref().unwrap(),
                        &id,
                    )
                })
            });
            if !accepted {
                return Err("原地形画布选择未受理".into());
            }
            world.resource_mut::<SceneEditorState>().terrain_canvas_target = Some(id);
            state.advance(Phase::WaitFocus);
        }
        Phase::WaitFocus => {
            let session = world.resource::<EditorAuthoringSession>();
            if terrain_documents::canvas_writable(session, world.resource::<SceneEditorState>())
                && session
                    .draft
                    .as_ref()
                    .and_then(|draft| draft.terrain_documents.active.as_ref())
                    .is_some_and(|active| Some(&active.definition.id) == state.terrain.as_ref())
            {
                state.advance(Phase::Prepare);
            }
        }
        Phase::Prepare => {
            state.before = None;
            state.after = None;
            state.first = None;
            state.last = None;
            state.tail = false;
            state.undo_verified = false;
            state.preset_id = None;
            state.tab = None;
            match state.domain {
                Domain::Height => state.advance(Phase::Before),
                Domain::Surface => {
                    state.pulse = Some(pulse_surface(world, SurfacePresetAction::Create));
                    state.advance(Phase::WaitPreset);
                }
                Domain::Blade => {
                    world.resource_mut::<SceneEditorState>().selected_asset =
                        Some(BLADE_SOURCE.into());
                    state.pulse = Some(pulse_blade(world, PresetEditorAction::CreateBlade));
                    state.advance(Phase::WaitPreset);
                }
            }
        }
        Phase::WaitPreset => {
            let tab = match state.domain {
                Domain::Surface => world
                    .resource::<SurfacePresetEditorState>()
                    .tabs
                    .last()
                    .map(|tab| (tab.id, Some(tab.draft.descriptor.id.clone()))),
                Domain::Blade => world
                    .resource::<VegetationPresetEditorState>()
                    .tabs
                    .last()
                    .map(|tab| (tab.id, None)),
                Domain::Height => unreachable!(),
            };
            if let Some((tab, id)) = tab {
                state.tab = Some(tab);
                state.preset_id = id;
                state.advance(Phase::SavePreset);
            }
        }
        Phase::SavePreset => {
            state.pulse = Some(match state.domain {
                Domain::Surface => pulse_surface(world, SurfacePresetAction::Save),
                Domain::Blade => pulse_blade(world, PresetEditorAction::Save),
                Domain::Height => unreachable!(),
            });
            state.advance(Phase::WaitPresetSave);
        }
        Phase::WaitPresetSave => {
            let ready = match state.domain {
                Domain::Surface => {
                    let editor = world.resource::<SurfacePresetEditorState>();
                    !editor.busy()
                        && editor
                            .tabs
                            .iter()
                            .find(|tab| Some(tab.id) == state.tab)
                            .is_some_and(|tab| !tab.draft.dirty())
                        && state.preset_id.as_ref().is_some_and(|id| {
                            world
                                .resource::<SurfacePresetRegistry>()
                                .resolve(id)
                                .is_some()
                        })
                }
                Domain::Blade => {
                    let registry = world.resource::<VegetationPresetRegistry>();
                    let editor = world.resource::<VegetationPresetEditorState>();
                    if !editor.saved_play_pending()
                        && editor.exit_blocker().is_none()
                        && let Some(preset) = registry
                            .presets()
                            .iter()
                            .find(|preset| preset.author_asset_path == BLADE_SOURCE)
                    {
                        state.preset_id = Some(preset.id.clone());
                        true
                    } else {
                        false
                    }
                }
                Domain::Height => unreachable!(),
            };
            if ready {
                state.advance(Phase::ClosePreset);
            }
        }
        Phase::ClosePreset => {
            state.pulse = Some(match state.domain {
                Domain::Surface => {
                    pulse_surface(world, SurfacePresetAction::CloseTab(state.tab.unwrap()))
                }
                Domain::Blade => pulse_blade(
                    world,
                    PresetEditorAction::ClosePresetTab(state.tab.unwrap()),
                ),
                Domain::Height => unreachable!(),
            });
            state.advance(Phase::WaitPresetClose);
        }
        Phase::WaitPresetClose => {
            let closed = match state.domain {
                Domain::Surface => !world
                    .resource::<SurfacePresetEditorState>()
                    .tabs
                    .iter()
                    .any(|tab| Some(tab.id) == state.tab),
                Domain::Blade => !world
                    .resource::<VegetationPresetEditorState>()
                    .tabs
                    .iter()
                    .any(|tab| Some(tab.id) == state.tab),
                Domain::Height => unreachable!(),
            };
            if closed {
                state.advance(Phase::CreateLayer);
            }
        }
        Phase::CreateLayer => create_layer(world, state)?,
        Phase::WaitLayer => wait_layer(world, state)?,
        Phase::Before | Phase::After | Phase::Undone | Phase::Redone => hash_phase(world, state)?,
        Phase::Hover => {
            let editor = world.resource::<SceneEditorState>();
            if editor.brush_terrain_id.as_ref() == state.terrain.as_ref()
                && editor.brush_cursor_world.is_some()
                && !world.resource::<TerrainObjectsEditorState>().busy()
                && state.clock.phase.elapsed() > Duration::from_millis(80)
            {
                let session = world.resource::<EditorAuthoringSession>();
                state.undo_before = session.undo.len();
                state.redo_before = session.redo.len();
                state.advance(Phase::Paint);
            }
        }
        Phase::Paint => {
            // 持续按压中提前形成历史说明原笔已被取消；立即保留失败原因，不能空等后再冒充完整拖动。
            if world.resource::<EditorAuthoringSession>().undo.len() != state.undo_before {
                return Err(format!(
                    "{:?}持续按压时原笔提前收口；{}；首末刷印={:?}/{:?}",
                    state.domain,
                    world.resource::<SceneEditorState>().status,
                    state.first,
                    state.last,
                ));
            }
            let point = match state.domain {
                // 局部页先采用，松手后才形成整笔历史。移到下一点时只认已正式发布的非空刷印。
                Domain::Height => world.resource::<SparseTerrainAuthoringState>().validation_applied_sculpt_point(),
                Domain::Surface => {
                    let stroke = world.resource::<SurfaceBrushStroke>();
                    (stroke.active
                        && (!stroke.before_weights.is_empty() || !stroke.before_indices.is_empty()))
                    .then_some(stroke.last_world)
                    .flatten()
                }
                Domain::Blade => {
                    let stroke = world.resource::<VegetationBrushStroke>();
                    (stroke.active && !stroke.before.is_empty())
                        .then_some(stroke.last_world)
                        .flatten()
                }
            };
            if let Some(point) = point {
                if state.first.is_none() {
                    state.first = Some(point);
                    state.tail = true;
                }
                state.last = Some(point);
            }
            if state
                .first
                .zip(state.last)
                .is_some_and(|(a, b)| a.xz().distance(b.xz()) > 1.)
                && state.clock.phase.elapsed() > Duration::from_millis(650)
            {
                state.advance(Phase::Release);
            }
        }
        Phase::Release => {
            if state.mouse_owned
                || world.resource::<TerrainObjectsEditorState>().busy()
                || world.resource::<TerrainBrushStroke>().active
                || (state.domain == Domain::Height
                    && !world.resource::<SparseTerrainAuthoringState>().document_can_park())
                || world.resource::<SurfaceBrushStroke>().active
                || world.resource::<VegetationBrushStroke>().active
                || world
                    .get_resource::<crate::authoring::vegetation_paint_scope::VegetationPaintState>(
                    )
                    .is_some_and(|scope| scope.busy())
            {
                return Ok(());
            }
            let session = world.resource::<EditorAuthoringSession>();
            if session.undo.len() != state.undo_before + 1 || !session.redo.is_empty() {
                return Err(format!("{:?}一次拖动未恰好形成一条新历史", state.domain));
            }
            (state.history_kind, state.changed_samples) = capture::history(world, state)?;
            state.advance(Phase::After);
        }
        Phase::Undo => {
            state.pulse = Some(pulse_scene(world, EditorAction::Undo));
            state.advance(Phase::WaitUndo);
        }
        Phase::WaitUndo => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.undo.len() == state.undo_before
                && session.redo.len() == 1
                && !world.resource::<SceneEditorState>().undo_requested
            {
                state.advance(Phase::Undone);
            }
        }
        Phase::Redo => {
            state.pulse = Some(pulse_scene(world, EditorAction::Redo));
            state.advance(Phase::WaitRedo);
        }
        Phase::WaitRedo => {
            let session = world.resource::<EditorAuthoringSession>();
            if session.undo.len() == state.undo_before + 1
                && session.redo.is_empty()
                && !world.resource::<SceneEditorState>().redo_requested
            {
                state.advance(Phase::Redone);
            }
        }
        Phase::Restore => {
            if !state.mouse_owned {
                pointer::restore(world, state);
                state.advance(Phase::Complete);
            }
        }
        Phase::Idle | Phase::Complete | Phase::Failed => {}
    }
    Ok(())
}

fn create_layer(world: &mut World, state: &mut TerrainWorkflow) -> Result<(), String> {
    match state.domain {
        Domain::Surface => {
            world
                .resource_mut::<SceneEditorState>()
                .surface_selected_preset_id = state.preset_id.clone();
            world.resource_mut::<SceneEditorState>().tool = EditorTool::Surface;
            let index =
                world.resource_scope(|world, mut session: Mut<EditorAuthoringSession>| {
                    world.resource_scope(|world, registry: Mut<SurfacePresetRegistry>| {
                        crate::authoring::create_surface_layer(
                            &mut world.resource_mut::<SceneEditorState>(),
                            &mut session,
                            Some(&registry),
                        )
                    })
                })?;
            state.surface_layer = world
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .and_then(|draft| draft.surface.as_ref())
                .and_then(|surface| surface.layers.get(index))
                .map(|layer| layer.id.clone());
            state.advance(Phase::Before);
        }
        Domain::Blade => {
            state.author_count = world
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .and_then(|draft| draft.vegetation.as_ref())
                .and_then(|vegetation| vegetation.authoring.as_ref())
                .map_or(0, |author| author.layers.len());
            let mut editor = world.resource_mut::<SceneEditorState>();
            editor.vegetation_selected_preset_id = state.preset_id.clone();
            editor.tool = EditorTool::Vegetation;
            state.pulse = Some(pulse_scene(world, EditorAction::VegetationAddLayer));
            state.advance(Phase::WaitLayer);
        }
        Domain::Height => unreachable!(),
    }
    Ok(())
}

fn wait_layer(world: &mut World, state: &mut TerrainWorkflow) -> Result<(), String> {
    let session = world.resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let Some(vegetation) = draft.vegetation.as_ref() else {
        return Ok(());
    };
    let Some(author) = vegetation
        .authoring
        .as_ref()
        .filter(|author| author.layers.len() == state.author_count + 1)
    else {
        return Ok(());
    };
    let Some(output) = vegetation
        .layers
        .iter()
        .find(|layer| layer.runtime_profile.as_ref() == state.preset_id.as_ref())
    else {
        return Ok(());
    };
    if output.kind != feathrise_scene_content::SceneVegetationKind::Blade {
        return Err("原CreateBlade生成了错误的正式后端".into());
    }
    let Some(index) = author
        .layers
        .iter()
        .position(|layer| layer.output_layer_id == output.id)
    else {
        return Ok(());
    };
    state.blade_layer = Some(author.layers[index].id.clone());
    state.blade_output = Some(output.id.clone());
    world.resource_scope(|world, session: Mut<EditorAuthoringSession>| {
        crate::ui::actions::activate_vegetation_layer(
            &mut world.resource_mut::<SceneEditorState>(),
            &session,
            index,
            None,
        );
    });
    state.advance(Phase::Before);
    Ok(())
}

fn hash_phase(world: &mut World, state: &mut TerrainWorkflow) -> Result<(), String> {
    if !capture::ready(world, state) {
        return Ok(());
    }
    if state.task.is_none() {
        state.capture_revision = world.resource::<EditorAuthoringSession>().revision;
        state.task = Some(capture::start(world, state)?);
        return Ok(());
    }
    let Some(result) = state.task.as_mut().and_then(check_ready) else {
        return Ok(());
    };
    state.task = None;
    if state.capture_revision != world.resource::<EditorAuthoringSession>().revision {
        return Err("后台取证时作者修订发生意外变化".into());
    }
    let proof = result?;
    match state.phase {
        Phase::Before => {
            state.before = Some(proof);
            state.advance(Phase::Hover);
        }
        Phase::After => {
            if state.before.as_ref().unwrap().sha256 == proof.sha256 || proof.nonzero_samples == 0 {
                return Err(format!("{:?}原笔划没有非空真实作者修改", state.domain));
            }
            state.after = Some(proof);
            state.advance(Phase::Undo);
        }
        Phase::Undone => {
            if proof.sha256 != state.before.as_ref().unwrap().sha256 {
                return Err(format!("{:?}原Undo没有精确恢复全部作者源", state.domain));
            }
            state.undo_verified = true;
            state.advance(Phase::Redo);
        }
        Phase::Redone => {
            if proof.sha256 != state.after.as_ref().unwrap().sha256 {
                return Err(format!("{:?}原Redo没有精确恢复全部作者源", state.domain));
            }
            state.proofs.push(StrokeProof {
                tool: match state.domain {
                    Domain::Height => "Raise",
                    Domain::Surface => "Surface",
                    Domain::Blade => "Blade",
                },
                center: state.domain.center().to_array(),
                start: state.first.unwrap().to_array(),
                end: state.last.unwrap().to_array(),
                history_kind: state.history_kind.clone(),
                changed_samples: state.changed_samples,
                history_entries: 1,
                before: state.before.take().unwrap(),
                after: state.after.take().unwrap(),
                undo_matches_before: state.undo_verified,
                redo_matches_after: true,
                live_preview_ready: true,
            });
            state.domain = match state.domain {
                Domain::Height => Domain::Surface,
                Domain::Surface => Domain::Blade,
                Domain::Blade => {
                    state.advance(Phase::Restore);
                    return Ok(());
                }
            };
            state.advance(Phase::Prepare);
        }
        _ => unreachable!(),
    }
    Ok(())
}
