//! 显式隔离工程验收：原植被预设/建层、真实相机地面查询笔划与原静水笔划，随后由父夹具统一保存。

#[path = "validation_fixture_map_tools/history.rs"]
mod history;
#[path = "validation_fixture_map_tools/resources.rs"]
mod resources;
#[path = "validation_fixture_map_tools/types.rs"]
mod types;
use resources::MapTools;
pub use types::{MapToolProof, PaintSourceProof};
use types::{PaintCapture, Phase};

use crate::{
    authoring::{
        EditorAuthoringSession, EditorCommand, VegetationBrushStroke, WaterBrushStroke,
        terrain_documents::{self, TerrainDocumentTask},
    },
    components::{MapEditorActionButton, MapEditorViewport},
    resources::{MapEditorState, MapEditorViewportState},
    types::{EditorAction, EditorTool, EditorView, WaterLevelInputTarget, WaterLevelSetRequest},
    ui::{
        terrain_objects::TerrainObjectsEditorState,
        vegetation_preset_editor::{
            PresetEditorAction, PresetEditorButton, VegetationPresetEditorState,
        },
    },
    viewport::modes::{ViewportMode, ViewportModeState},
};
use bevy::{
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
    ui::RelativeCursorPosition,
};
use feathrise_map_content::VegetationPresetRegistry;
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const WATER_LEVEL: f32 = 0.25;

pub(super) fn install(app: &mut App) {
    if std::env::var("NEW_EDITOR_NATIVE_SMOKE_MAP_TOOLS").as_deref() != Ok("1") {
        return;
    }
    app.insert_resource(MapTools {
        phase: Phase::Idle,
        started: Instant::now(),
        phase_started: Instant::now(),
        proof: MapToolProof {
            scope: "original_project_mesh_vegetation_and_root_water_brush_save".into(),
            vegetation_preset_source: "scene/probe.glb".into(),
            ..default()
        },
        pulse: None,
        tab: None,
        terrain: None,
        undo_before: 0,
        vegetation_layer: None,
        mouse_owned: false,
        tail: false,
        first: None,
        last: None,
        vegetation: None,
        water: None,
        pixels: None,
        old_view: None,
    })
    .add_systems(
        Update,
        inject_pointer
            .in_set(crate::MapEditorSystemSet::Cursor)
            .before(crate::viewport::sync_editor_viewport_camera),
    );
}

pub(super) fn begin(world: &mut World) -> Result<bool, String> {
    if !world.contains_resource::<MapTools>() {
        return Ok(false);
    }
    let editor = world.resource::<MapEditorState>();
    let viewport = world
        .get_resource::<MapEditorViewportState>()
        .ok_or("原作者相机尚未生成")?;
    let old_view = (
        editor.view,
        editor.orthographic,
        viewport.center,
        viewport.extent,
        viewport.yaw,
        viewport.pitch,
    );
    let terrain = world
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .and_then(|draft| draft.manifest.terrains.as_ref())
        .and_then(|objects| objects.first())
        .map(|object| object.id.clone())
        .ok_or("验收工程缺少真实初始网格")?;
    let mut state = world.resource_mut::<MapTools>();
    if state.phase != Phase::Idle {
        return Err("原地图工具验收不可重复请求".into());
    }
    state.old_view = Some(old_view);
    state.terrain = Some(terrain.clone());
    state.proof.terrain_id = terrain.as_str().into();
    state.started = Instant::now();
    state.advance(Phase::CreatePreset);
    Ok(true)
}

pub(super) fn report(world: &World) -> Option<MapToolProof> {
    world
        .get_resource::<MapTools>()
        .map(|state| state.proof.clone())
}

impl MapTools {
    fn advance(&mut self, phase: Phase) {
        self.phase = phase;
        self.phase_started = Instant::now();
    }
}

/// 只在显式验收中向原ECS鼠标源投递有限轨迹；命中由真实相机、当前地形Collider/水位平面产生。
/// 不改地面快照、ray身份、作者R8或原笔刷算法，也不声称测试了物理鼠标与UI命中。
fn inject_pointer(world: &mut World) {
    let (phase, tail, release) = {
        let mut state = world.resource_mut::<MapTools>();
        let pressed = matches!(state.phase, Phase::PaintVegetation | Phase::PaintWater);
        let release = state.mouse_owned && !pressed;
        if release {
            state.mouse_owned = false;
        }
        (state.phase, state.tail, release)
    };
    if release {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
    }
    if !matches!(
        phase,
        Phase::HoverVegetation
            | Phase::PaintVegetation
            | Phase::ReleaseVegetation
            | Phase::HoverWater
            | Phase::PaintWater
            | Phase::ReleaseWater
    ) {
        return;
    }
    let water = matches!(
        phase,
        Phase::HoverWater | Phase::PaintWater | Phase::ReleaseWater
    );
    let pressed = matches!(phase, Phase::PaintVegetation | Phase::PaintWater);
    if let Some(mut mode) = world.get_resource_mut::<ViewportModeState>() {
        mode.mode = ViewportMode::Edit;
        mode.pointer_over_controls = false;
    }
    let mut editor = world.resource_mut::<MapEditorState>();
    editor.tool = if water {
        EditorTool::Water
    } else {
        EditorTool::Vegetation
    };
    editor.view = EditorView::Top;
    editor.orthographic = true;
    editor.brush_radius = 2.;
    editor.brush_strength = 1.;
    if let Some(mut viewport) = world.get_resource_mut::<MapEditorViewportState>() {
        viewport.center = Vec3::new(8., 0., 8.);
        viewport.extent = 16.;
        viewport.yaw = 0.;
        viewport.pitch = std::f32::consts::FRAC_PI_2;
    }
    for mut cursor in world
        .query_filtered::<&mut RelativeCursorPosition, With<MapEditorViewport>>()
        .iter_mut(world)
    {
        cursor.cursor_over = true;
        cursor.normalized = Some(Vec2::new(if tail { 0.12 } else { -0.12 }, 0.));
    }
    let mut mouse = world.resource_mut::<ButtonInput<MouseButton>>();
    if pressed {
        mouse.press(MouseButton::Left);
        world.resource_mut::<MapTools>().mouse_owned = true;
    }
}

fn pulse_map(world: &mut World, action: EditorAction) -> Entity {
    world
        .spawn((
            MapEditorActionButton(action),
            feathrise_editor_ui::EditorButtonClick::triggered(),
        ))
        .id()
}
fn pulse_preset(world: &mut World, action: PresetEditorAction) -> Entity {
    world
        .spawn((
            PresetEditorButton(action),
            feathrise_editor_ui::EditorButtonClick::triggered(),
        ))
        .id()
}
fn core_command(mut command: &EditorCommand) -> &EditorCommand {
    for _ in 0..8 {
        command = match command {
            EditorCommand::TerrainCanvas { command, .. } | EditorCommand::RootWater(command) => {
                command
            }
            _ => return command,
        };
    }
    command
}

pub(super) fn advance(world: &mut World) {
    if !world.contains_resource::<MapTools>() {
        return;
    }
    world.resource_scope(|world, mut state: Mut<MapTools>| {
        if let Some(entity) = state.pulse.take() { let _ = world.despawn(entity); }
        if matches!(state.phase, Phase::Idle | Phase::Complete | Phase::Failed) { return; }
        let result = (|| -> Result<(), String> {
            if state.started.elapsed() > Duration::from_secs(150) {
                return Err(format!("原地图工具验收超时，阶段{:?}：{}；{}", state.phase, world.resource::<MapEditorState>().status, history::diagnostic(world, state.undo_before)));
            }
            match state.phase {
                Phase::CreatePreset => {
                    let registry = world.get_resource::<VegetationPresetRegistry>().ok_or("原植被目录未安装")?;
                    if registry.revision == 0 { return Ok(()); }
                    if !registry.presets().is_empty() { return Err("地图工具夹具只允许空白模板植被库，拒绝覆盖既有作者预设".into()); }
                    world.resource_mut::<MapEditorState>().selected_asset = Some("scene/probe.glb".into());
                    state.pulse = Some(pulse_preset(world, PresetEditorAction::CreateMesh)); state.advance(Phase::WaitPreset);
                }
                Phase::WaitPreset => {
                    if let Some(tab) = world.resource::<VegetationPresetEditorState>().tabs.first() {
                        state.tab = Some(tab.id); state.advance(Phase::SavePreset);
                    }
                }
                Phase::SavePreset => {
                    state.pulse = Some(pulse_preset(world, PresetEditorAction::Save)); state.advance(Phase::WaitPresetSave);
                }
                Phase::WaitPresetSave => {
                    let registry = world.resource::<VegetationPresetRegistry>();
                    let editor = world.resource::<VegetationPresetEditorState>();
                    if registry.presets().len() == 1 && !editor.saved_play_pending() && editor.exit_blocker().is_none() {
                        let preset = &registry.presets()[0];
                        if preset.author_asset_path != "scene/probe.glb" { return Err("原植被预设来源不属于本工程选中GLB".into()); }
                        state.proof.vegetation_preset_id = Some(preset.id.clone()); state.advance(Phase::ClosePreset);
                    }
                }
                Phase::ClosePreset => {
                    state.pulse = Some(pulse_preset(world, PresetEditorAction::ClosePresetTab(state.tab.unwrap())));
                    state.advance(Phase::WaitPresetClose);
                }
                Phase::WaitPresetClose => {
                    if world.resource::<VegetationPresetEditorState>().tabs.is_empty() { state.advance(Phase::SelectTerrain); }
                }
                Phase::SelectTerrain => {
                    let id = state.terrain.as_ref().unwrap().clone();
                    let accepted = world.resource_scope(|world, session: Mut<EditorAuthoringSession>| {
                        world.resource_scope(|world, mut task: Mut<TerrainDocumentTask>| {
                            let mut selection = world.resource_mut::<TerrainObjectsEditorState>();
                            terrain_documents::request_focus(&mut task, &mut selection, session.draft.as_ref().unwrap(), &id)
                        })
                    });
                    if !accepted { return Err("原网格画布选择请求未受理".into()); }
                    world.resource_mut::<MapEditorState>().terrain_canvas_target = Some(id);
                    state.advance(Phase::WaitCanvas);
                }
                Phase::WaitCanvas => {
                    let session = world.resource::<EditorAuthoringSession>();
                    let editor = world.resource::<MapEditorState>();
                    if terrain_documents::canvas_writable(session, editor) && session.draft.as_ref()
                        .and_then(|draft| draft.terrain_documents.active.as_ref()).is_some_and(|active| Some(&active.definition.id) == state.terrain.as_ref()) {
                        state.advance(Phase::CreateVegetation);
                    }
                }
                Phase::CreateVegetation => {
                    let mut editor = world.resource_mut::<MapEditorState>();
                    editor.vegetation_selected_preset_id = state.proof.vegetation_preset_id.clone(); editor.tool = EditorTool::Vegetation;
                    state.pulse = Some(pulse_map(world, EditorAction::VegetationAddLayer)); state.advance(Phase::WaitVegetation);
                }
                Phase::WaitVegetation => {
                    let session = world.resource::<EditorAuthoringSession>();
                    if let Some(layer) = session.draft.as_ref().and_then(|draft| draft.vegetation.as_ref()).and_then(|vegetation| vegetation.authoring.as_ref())
                        .filter(|author| author.layers.len() == 1).and_then(|author| author.layers.first()) {
                        state.vegetation_layer = Some(layer.id.clone());
                        state.tail = false; state.advance(Phase::HoverVegetation);
                    }
                }
                Phase::HoverVegetation => {
                    let editor = world.resource::<MapEditorState>();
                    if editor.brush_terrain_id.as_ref() == state.terrain.as_ref() && editor.brush_cursor_world.is_some() {
                        state.undo_before = world.resource::<EditorAuthoringSession>().undo.len(); state.first = None; state.last = None;
                        state.advance(Phase::PaintVegetation);
                    }
                }
                Phase::PaintVegetation => {
                    let stroke = world.resource::<VegetationBrushStroke>();
                    if stroke.active && !stroke.before.is_empty() && let Some(point) = stroke.last_world {
                        if state.first.is_none() { state.first = Some(point); state.tail = true; }
                        state.last = Some(point);
                    }
                    if state.first.zip(state.last).is_some_and(|(first,last)| first.distance(last) > 1.)
                        && state.phase_started.elapsed() > Duration::from_millis(600) {
                        // Last只换阶段；真正松手留到下一Cursor，与原相机/笔刷同帧消费释放边沿。
                        state.advance(Phase::ReleaseVegetation);
                    }
                }
                Phase::ReleaseVegetation => {
                    if world.resource::<VegetationBrushStroke>().active { return Ok(()); }
                    if world.get_resource::<crate::authoring::vegetation_paint_scope::VegetationPaintState>().is_some_and(|scope| scope.busy()) { return Ok(()); }
                    if state.phase_started.elapsed() < Duration::from_millis(50) { return Ok(()); }
                    let session = world.resource::<EditorAuthoringSession>();
                    if session.undo.len() != state.undo_before + 1 { return Err(format!("原植被轨迹没有形成且仅形成一条非空撤销记录；{}", history::diagnostic(world, state.undo_before))); }
                    let draft = session.draft.as_ref().unwrap();
                    let layer_id = state.vegetation_layer.as_ref().ok_or("原植被按下前未冻结层身份")?;
                    let layer = draft.vegetation.as_ref().unwrap().authoring.as_ref().unwrap().layers.iter().find(|layer| &layer.id == layer_id).ok_or("原植被笔划层身份不一致")?;
                    let pixels = draft.vegetation_author_densities.get(layer_id).ok_or("原植被笔划缺少R8源")?.clone();
                    let history_kind = history::validate_vegetation(session.undo.last().ok_or("原植被笔划缺少撤销记录")?, state.terrain.as_ref().unwrap(), layer_id, &pixels)?;
                    state.vegetation = Some(PaintCapture { layer: layer_id.clone(), path: layer.density_source_path.clone(), pixels,
                        undo: 1, history_kind: history_kind.into(), start: state.first.unwrap(), end: state.last.unwrap() }); state.advance(Phase::CreateWater);
                }
                Phase::CreateWater => {
                    world.resource_mut::<MapEditorState>().water_selected_preset_id = Some("calm".into());
                    state.pulse = Some(pulse_map(world, EditorAction::WaterAddLayer)); state.advance(Phase::WaitWater);
                }
                Phase::WaitWater => {
                    if world.resource::<EditorAuthoringSession>().draft.as_ref().filter(|draft| draft.terrain_documents.active.is_none())
                        .and_then(|draft| draft.water.as_ref()).is_some_and(|water| water.layers.len() == 1) { state.advance(Phase::WaterLevel); }
                }
                Phase::WaterLevel => {
                    let session = world.resource::<EditorAuthoringSession>();
                    let draft = session.draft.as_ref().unwrap();
                    let request = WaterLevelSetRequest { target: WaterLevelInputTarget { map_id: draft.map_id.clone(),
                        layer_id: draft.water.as_ref().unwrap().layers[0].id.clone(), authoring_revision: session.revision }, water_level: WATER_LEVEL };
                    world.resource_mut::<MapEditorState>().water_level_set_requested = Some(request); state.advance(Phase::WaitWaterLevel);
                }
                Phase::WaitWaterLevel => {
                    let session = world.resource::<EditorAuthoringSession>();
                    if session.draft.as_ref().and_then(|draft| draft.water.as_ref()).and_then(|water| water.layers.first())
                        .is_some_and(|layer| layer.water_level == WATER_LEVEL)
                        && matches!(session.undo.last().map(core_command), Some(EditorCommand::WaterLevelChanged { after, .. }) if *after == WATER_LEVEL) {
                        state.proof.water_level = Some(WATER_LEVEL); state.tail = false; state.first = None; state.last = None;
                        state.advance(Phase::HoverWater);
                    }
                }
                Phase::HoverWater => {
                    if world.resource::<MapEditorState>().brush_cursor_world.is_some() {
                        state.undo_before = world.resource::<EditorAuthoringSession>().undo.len(); state.advance(Phase::PaintWater);
                    }
                }
                Phase::PaintWater => {
                    let stroke = world.resource::<WaterBrushStroke>();
                    if stroke.active && (stroke.snapshot_changed || !stroke.mask_before.is_empty()) && let Some(point) = stroke.last_world {
                        if state.first.is_none() { state.first = Some(point); state.tail = true; } state.last = Some(point);
                    }
                    if state.first.zip(state.last).is_some_and(|(first,last)| first.distance(last) > 1.)
                        && state.phase_started.elapsed() > Duration::from_millis(600) {
                        state.advance(Phase::ReleaseWater);
                    }
                }
                Phase::ReleaseWater => {
                    if world.resource::<WaterBrushStroke>().active { return Ok(()); }
                    let session = world.resource::<EditorAuthoringSession>();
                    let Some(EditorCommand::WaterStrokeSnapshot(stroke)) = session.undo.last().map(core_command) else { return Ok(()); };
                    if session.undo.len() != state.undo_before + 1 || Arc::ptr_eq(&stroke.before_mask, &stroke.after_mask) { return Err("原静水轨迹没有形成且仅形成一条有效撤销记录".into()); }
                    let draft = session.draft.as_ref().unwrap();
                    let layer = draft.water.as_ref().unwrap().layers.iter().find(|layer| layer.id == stroke.layer_id).ok_or("原静水笔划层身份不一致")?;
                    state.water = Some(PaintCapture { layer: layer.id.clone(), path: layer.mask_source_path.clone(), pixels: stroke.after_mask.clone(),
                        undo: 1, history_kind: "WaterStrokeSnapshot".into(), start: state.first.unwrap(), end: state.last.unwrap() });
                    let vegetation = state.vegetation.clone().unwrap(); let water = state.water.clone().unwrap();
                    state.pixels = Some(IoTaskPool::get().spawn(async move { (pixel_proof(vegetation), pixel_proof(water)) })); state.advance(Phase::Pixels);
                }
                Phase::Pixels => {
                    if let Some((vegetation,water)) = state.pixels.as_mut().and_then(check_ready) {
                        state.pixels = None;
                        if vegetation.source_nonzero == 0 || water.source_nonzero == 0 { return Err("原笔划回执没有留下非零作者R8".into()); }
                        state.proof.vegetation = Some(vegetation); state.proof.water = Some(water); state.proof.complete = true;
                        restore_view(world, &mut state); state.advance(Phase::Complete);
                    }
                }
                _ => {}
            }
            Ok(())
        })();
        if let Err(error) = result { state.proof.error = Some(error); restore_view(world, &mut state); state.advance(Phase::Failed); }
    });
}

fn restore_view(world: &mut World, state: &mut MapTools) {
    // 完成和失败的释放也由下一Cursor清理，仅归还本夹具自己按下的ECS鼠标。
    if let Some((view, orthographic, center, extent, yaw, pitch)) = state.old_view.take() {
        let mut editor = world.resource_mut::<MapEditorState>();
        editor.view = view;
        editor.orthographic = orthographic;
        if let Some(mut viewport) = world.get_resource_mut::<MapEditorViewportState>() {
            viewport.center = center;
            viewport.extent = extent;
            viewport.yaw = yaw;
            viewport.pitch = pitch;
        }
    }
}

fn pixel_proof(capture: PaintCapture) -> PaintSourceProof {
    PaintSourceProof {
        layer_id: capture.layer,
        source_path: capture.path,
        source_bytes: capture.pixels.len(),
        source_nonzero: capture.pixels.iter().filter(|&&byte| byte != 0).count(),
        source_sha256: Sha256::digest(capture.pixels.as_slice())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>(),
        undo_records: capture.undo,
        history_kind: capture.history_kind,
        start_world: capture.start.to_array(),
        end_world: capture.end.to_array(),
    }
}
