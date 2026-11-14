//! 显式隔离工程验收：原植被预设/建层、真实相机地面查询笔划与原静水笔划，随后由父夹具统一保存。

#[path = "validation_fixture_scene_tools/history.rs"]
mod history;
#[path = "validation_fixture_scene_tools/resources.rs"]
mod resources;
#[path = "validation_fixture_scene_tools/types.rs"]
mod types;
use resources::SceneTools;
pub use types::{SceneToolProof, PaintSourceProof};
use types::{PaintCapture, Phase};

use crate::{
    authoring::{
        EditorAuthoringSession, EditorCommand, VegetationBrushStroke, WaterBrushStroke, WaterNetworkEditState,
        terrain_documents,
    },
    components::{SceneEditorActionButton, SceneEditorViewport},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState},
    types::{EditorAction, EditorTool, EditorView, WaterLevelInputTarget, WaterLevelSetRequest},
    ui::{
        sidebars::{EditorNavigatorTab, EditorSidebarTab, SceneEditorSidebarState, SceneEditorSidebarTabButton},
        terrain_objects::TerrainObjectsEditorState,
        vegetation_preset_editor::{
            PresetEditorAction, PresetEditorButton, VegetationPresetEditorState,
        },
        water_feature_list::WaterSourceSelectionGuard,
    },
    viewport::modes::{ViewportMode, ViewportModeState},
};
use bevy::{
    ecs::system::RunSystemOnce,
    prelude::*,
    tasks::{IoTaskPool, futures::check_ready},
    ui::RelativeCursorPosition,
};
use feathrise_scene_content::VegetationPresetRegistry;
use sha2::{Digest, Sha256};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const WATER_LEVEL: f32 = 0.25;

pub(super) fn install(app: &mut App) {
    if std::env::var("NEW_EDITOR_NATIVE_SMOKE_SCENE_TOOLS").as_deref() != Ok("1") {
        return;
    }
    app.insert_resource(SceneTools {
        phase: Phase::Idle,
        started: Instant::now(),
        phase_started: Instant::now(),
        proof: SceneToolProof {
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
            .in_set(crate::SceneEditorSystemSet::Cursor)
            .before(crate::viewport::sync_editor_viewport_camera),
    );
}

pub(super) fn begin(world: &mut World) -> Result<bool, String> {
    if !world.contains_resource::<SceneTools>() {
        return Ok(false);
    }
    let editor = world.resource::<SceneEditorState>();
    let viewport = world
        .get_resource::<SceneEditorViewportState>()
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
    let mut state = world.resource_mut::<SceneTools>();
    if state.phase != Phase::Idle {
        return Err("原场景工具验收不可重复请求".into());
    }
    state.old_view = Some(old_view);
    state.terrain = Some(terrain.clone());
    state.proof.terrain_id = terrain.as_str().into();
    state.started = Instant::now();
    state.advance(Phase::CreatePreset);
    Ok(true)
}

pub(super) fn report(world: &World) -> Option<SceneToolProof> {
    world
        .get_resource::<SceneTools>()
        .map(|state| state.proof.clone())
}

impl SceneTools {
    fn advance(&mut self, phase: Phase) {
        self.phase = phase;
        self.phase_started = Instant::now();
    }
}

/// 只在显式验收中向原ECS鼠标源投递有限轨迹；命中由真实相机、当前地形Collider/水位平面产生。
/// 不改地面快照、ray身份、作者R8或原笔刷算法，也不声称测试了物理鼠标与UI命中。
fn inject_pointer(world: &mut World) {
    let (phase, tail, release) = {
        let mut state = world.resource_mut::<SceneTools>();
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
    let mut editor = world.resource_mut::<SceneEditorState>();
    editor.tool = if water {
        EditorTool::Water
    } else {
        EditorTool::Vegetation
    };
    editor.view = EditorView::Top;
    editor.orthographic = true;
    editor.brush_radius = 2.;
    editor.brush_strength = 1.;
    if let Some(mut viewport) = world.get_resource_mut::<SceneEditorViewportState>() {
        viewport.center = Vec3::new(8., 0., 8.);
        viewport.extent = 16.;
        viewport.yaw = 0.;
        viewport.pitch = std::f32::consts::FRAC_PI_2;
    }
    for mut cursor in world
        .query_filtered::<&mut RelativeCursorPosition, With<SceneEditorViewport>>()
        .iter_mut(world)
    {
        cursor.cursor_over = true;
        cursor.normalized = Some(Vec2::new(if tail { 0.12 } else { -0.12 }, 0.));
    }
    let mut mouse = world.resource_mut::<ButtonInput<MouseButton>>();
    if pressed {
        mouse.press(MouseButton::Left);
        world.resource_mut::<SceneTools>().mouse_owned = true;
    }
}

fn pulse_scene(world: &mut World, action: EditorAction) -> Entity {
    world
        .spawn((
            SceneEditorActionButton(action),
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

fn open_scene_tree(world: &mut World) -> Result<bool, String> {
    if world.resource::<SceneEditorSidebarState>().navigator == EditorNavigatorTab::SceneTree {
        return Ok(true);
    }
    let mut buttons = world.query::<(Entity, &SceneEditorSidebarTabButton)>();
    let button = buttons.iter(world).find_map(|(entity, button)| {
        (button.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::SceneTree)).then_some(entity)
    }).ok_or("原场景树页签尚未建立")?;
    let mut nodes = world.query::<(&Node, Option<&ChildOf>)>();
    if !crate::viewport::modes::controls_visible(button, &nodes.query(world)) {
        return Err("原场景树页签不可见".into());
    }
    // 验收经真实页签输入打开导航页，等原展示系统生成可见树行后才冻结菜单目标。
    world.entity_mut(button).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Ok(false)
}

fn height_hover_ready(world: &mut World) -> Result<bool, String> {
    world.run_system_once(|session: Res<EditorAuthoringSession>,
        author: Res<crate::authoring::SparseTerrainAuthoringState>,
        live: Res<feathrise_scene_content::SparseHeightLiveOverride>,
        ground: feathrise_scene::TerrainObjectClipmapGround| {
        let Some(draft) = session.draft.as_ref() else { return false; };
        let Some(active) = draft.terrain_documents.active.as_ref() else { return false; };
        let Some(index) = draft.terrain_ground.as_ref() else { return false; };
        let Some(current) = author.current_height_snapshot(&session, &live, &ground) else { return false; };
        // 只等待目标当前高度内容交接。落点附近的作者页仍由实际按压请求，不能预取代替冷起笔。
        active.definition.terrain.kind == feathrise_scene_content::SceneTerrainKind::SparseClipmap
            && index.revision == session.terrain_revision
            && index.clipmap_snapshot_for_validation(&active.definition.id).is_some_and(|snapshot|
                snapshot.ready && snapshot.content_revision_words() == current.content_revision_words())
    }).map_err(|error| error.to_string())
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

/// 原按钮只能点击一次。松笔后还须等当前私有画布的正式密度采用和根水切换门收口。
fn water_creation_ready(world: &mut World) -> Result<bool, String> {
    let ready = world.run_system_once(|source: WaterSourceSelectionGuard,
        session: Res<EditorAuthoringSession>, selection: Res<TerrainObjectsEditorState>,
        stroke: Res<WaterBrushStroke>, network: Res<WaterNetworkEditState>,
        mouse: Res<ButtonInput<MouseButton>>, scrub: Res<SceneEditorNumericScrubMouseGuard>| {
            !mouse.pressed(MouseButton::Left) && !scrub.blocked() && !stroke.active && !network.dragging()
                && !selection.busy() && !session.saving && !session.sparse_terrain_dirty
                && session.vegetation_compiled_revision == session.vegetation_revision
                && session.vegetation_compiled_structure_revision == session.vegetation_structure_revision
                && !source.busy(&session)
        }).map_err(|error| error.to_string())?;
    if !ready { return Ok(false); }
    world.run_system_once(crate::scene_browser::scene_browser_general_input_available)
        .map_err(|error| error.to_string())
}

pub(super) fn advance(world: &mut World) {
    if !world.contains_resource::<SceneTools>() {
        return;
    }
    world.resource_scope(|world, mut state: Mut<SceneTools>| {
        if let Some(entity) = state.pulse.take() { let _ = world.despawn(entity); }
        if matches!(state.phase, Phase::Idle | Phase::Complete | Phase::Failed) { return; }
        let result = (|| -> Result<(), String> {
            if state.started.elapsed() > Duration::from_secs(150) {
                return Err(format!("原场景工具验收超时，阶段{:?}：{}；{}", state.phase, world.resource::<SceneEditorState>().status, history::diagnostic(world, state.undo_before)));
            }
            match state.phase {
                Phase::CreatePreset => {
                    let registry = world.get_resource::<VegetationPresetRegistry>().ok_or("原植被目录未安装")?;
                    if registry.revision == 0 { return Ok(()); }
                    if !registry.presets().is_empty() {
                        let registered_mesh = super::model_surfaces::scene_tools_mesh(world)
                            .or_else(|| super::geometry_unification::scene_tools_mesh(world))
                            .map(|preset| (preset.id.clone(), preset.author_asset_path.clone()));
                        if let Some((preset, source)) = registered_mesh {
                            // 模型专项的原两项，或完整几何专项核验的原两项加本轮新增Blade，才属于这一自有夹具。
                            state.proof.vegetation_preset_source = source;
                            state.proof.vegetation_preset_id = Some(preset);
                            state.advance(Phase::SelectTerrain); return Ok(());
                        }
                        return Err("场景工具夹具只允许空白模板植被库，拒绝覆盖既有作者预设".into());
                    }
                    world.resource_mut::<SceneEditorState>().selected_asset = Some("scene/probe.glb".into());
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
                    if world.resource::<ViewportModeState>().mode != ViewportMode::Select {
                        crate::viewport::modes::validation_press_select(world)?;
                        return Ok(()); // 原模式按钮先完成权限交接，不能让菜单暗换正在编辑的目标。
                    }
                    if !open_scene_tree(world)? { return Ok(()); }
                    let id = state.terrain.as_ref().unwrap().clone();
                    // 原树行菜单统一取消旧模型/多选身份；直接请求画布不能证明工具已取得地形目标。
                    if crate::ui::scene_context_menu::queue_terrain_focus_for_validation(world, &id)? {
                        state.advance(Phase::WaitCanvas);
                    }
                }
                Phase::WaitCanvas => {
                    let session = world.resource::<EditorAuthoringSession>();
                    let editor = world.resource::<SceneEditorState>();
                    let terrain_selected = world.resource::<crate::viewport::LayerCandidateState>().selected.as_ref()
                        .is_some_and(|target| matches!(target, crate::viewport::LayerTarget::TerrainObject(id)
                            if Some(id.as_str()) == state.terrain.as_ref().map(|terrain|terrain.as_str())));
                    let model_selected = world.get_resource::<crate::ui::gltf_browser::EditorGltfBrowserState>()
                        .is_some_and(|browser| crate::authoring::model_vegetation::model_selected(session, browser));
                    if terrain_selected && !model_selected && terrain_documents::canvas_writable(session, editor) && session.draft.as_ref()
                        .and_then(|draft| draft.terrain_documents.active.as_ref()).is_some_and(|active| Some(&active.definition.id) == state.terrain.as_ref()) {
                        state.advance(Phase::CreateVegetation);
                    }
                }
                Phase::CreateVegetation => {
                    state.undo_before = world.resource::<EditorAuthoringSession>().undo.len();
                    let mut editor = world.resource_mut::<SceneEditorState>();
                    editor.vegetation_selected_preset_id = state.proof.vegetation_preset_id.clone(); editor.tool = EditorTool::Vegetation;
                    state.pulse = Some(pulse_scene(world, EditorAction::VegetationAddLayer)); state.advance(Phase::WaitVegetation);
                }
                Phase::WaitVegetation => {
                    let session = world.resource::<EditorAuthoringSession>();
                    let added_id = match session.undo.last().map(core_command) {
                        Some(EditorCommand::VegetationAuthoringChanged(change))
                            if session.undo.len() == state.undo_before + 1 && change.after.layers.len() == change.before.layers.len() + 1 =>
                                change.after.layers.iter().find(|layer| !change.before.layers.iter().any(|old| old.id == layer.id))
                                    .map(|layer| layer.id.as_str()),
                        _ => None,
                    };
                    if let Some(layer) = session.draft.as_ref().and_then(|draft| draft.vegetation.as_ref()).and_then(|vegetation| vegetation.authoring.as_ref())
                        .and_then(|author| author.layers.iter().find(|layer| Some(layer.id.as_str()) == added_id)) {
                        state.vegetation_layer = Some(layer.id.clone());
                        state.tail = false; state.advance(Phase::HoverVegetation);
                    }
                }
                Phase::HoverVegetation => {
                    if state.terrain.as_ref().is_some_and(|terrain|
                        world.resource::<SceneEditorState>().brush_terrain_id.as_ref() == Some(terrain))
                        && world.resource::<SceneEditorState>().brush_cursor_world.is_some()
                        && crate::authoring::terrain_ground::cursor_validation_snapshot(world)["ready"] == true
                        && height_hover_ready(world)? {
                        state.undo_before = world.resource::<EditorAuthoringSession>().undo.len(); state.first = None; state.last = None;
                        state.advance(Phase::PaintVegetation);
                    }
                }
                Phase::PaintVegetation => {
                    if state.mouse_owned && state.proof.vegetation_press.is_none() {
                        // Last读取原系统消费的首个按压帧，只记录一次身份与写侧门，不授予新按压。
                        let press = history::diagnostic(world, state.undo_before);
                        eprintln!("场景工具植被首按：{press}");
                        state.proof.vegetation_press = Some(press);
                    }
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
                    if !water_creation_ready(world)? { return Ok(()); }
                    let session = world.resource::<EditorAuthoringSession>();
                    state.proof.water_creation = Some(serde_json::json!({
                        "authoring_revision_at_click":session.revision,
                        "vegetation_revision_at_click":session.vegetation_revision,
                        "compiled_vegetation_revision_at_click":session.vegetation_compiled_revision,
                        "undo_before":session.undo.len(),
                    }));
                    world.resource_mut::<SceneEditorState>().water_selected_preset_id = Some("calm".into());
                    state.pulse = Some(pulse_scene(world, EditorAction::WaterAddLayer)); state.advance(Phase::WaitWater);
                }
                Phase::WaitWater => {
                    let session = world.resource::<EditorAuthoringSession>();
                    let Some(draft) = session.draft.as_ref().filter(|draft| draft.terrain_documents.active.is_none()) else { return Ok(()); };
                    let Some([layer]) = draft.water.as_ref().map(|water| water.layers.as_slice()) else { return Ok(()); };
                    let Some(EditorCommand::WaterLayersChanged(change)) = session.undo.last().map(core_command) else {
                        return Err("原根水新建缺少正式空白遮罩事务".into());
                    };
                    let capture = state.vegetation.as_ref().ok_or("根水切换缺少植被原笔划快照")?;
                    let parked = draft.terrain_documents.values.get(state.terrain.as_ref().unwrap())
                        .and_then(|canvas| canvas.vegetation_author_densities.get(&capture.layer));
                    let mask = draft.water_masks.get(&layer.id);
                    let proof = state.proof.water_creation.as_ref().ok_or("根水创建缺少原按钮回执")?;
                    if proof["undo_before"].as_u64() != Some(session.undo.len().saturating_sub(1) as u64)
                        || !change.before.layers.is_empty() || change.after.layers.as_slice() != [layer.clone()]
                        || !mask.zip(change.after_masks.get(&layer.id)).is_some_and(|(actual, adopted)| Arc::ptr_eq(actual, adopted))
                        || !parked.is_some_and(|pixels| Arc::ptr_eq(pixels, &capture.pixels))
                    {
                        return Err("原根水创建没有且仅有一条采用同一遮罩的事务，或私有植被作者R8未原样停放".into());
                    }
                    let proof = state.proof.water_creation.as_mut().expect("已验证原按钮回执");
                    proof["root_layer"] = serde_json::json!(layer.id);
                    proof["new_mask_matches_history"] = serde_json::json!(true);
                    proof["parked_vegetation_r8_matches_stroke"] = serde_json::json!(true);
                    state.advance(Phase::WaterLevel);
                }
                Phase::WaterLevel => {
                    let session = world.resource::<EditorAuthoringSession>();
                    let draft = session.draft.as_ref().unwrap();
                    let request = WaterLevelSetRequest { target: WaterLevelInputTarget { scene_id: draft.scene_id.clone(),
                        layer_id: draft.water.as_ref().unwrap().layers[0].id.clone(), authoring_revision: session.revision }, water_level: WATER_LEVEL };
                    world.resource_mut::<SceneEditorState>().water_level_set_requested = Some(request); state.advance(Phase::WaitWaterLevel);
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
                    if world.resource::<SceneEditorState>().brush_cursor_world.is_some() {
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

fn restore_view(world: &mut World, state: &mut SceneTools) {
    // 完成和失败的释放也由下一Cursor清理，仅归还本夹具自己按下的ECS鼠标。
    if let Some((view, orthographic, center, extent, yaw, pitch)) = state.old_view.take() {
        let mut editor = world.resource_mut::<SceneEditorState>();
        editor.view = view;
        editor.orthographic = orthographic;
        if let Some(mut viewport) = world.get_resource_mut::<SceneEditorViewportState>() {
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
