//! 原预设建层、原相机鼠标笔刷与原Undo/Redo的串行验收；最终保存交回父夹具。

use super::{resources::Fixture, types::*};
use crate::{
    authoring::{
        EditorAuthoringSession, EditorCommand, model_vegetation,
        placement_preview_ownership::components::PlacementPreviewSuppression,
    },
    components::{EditorPlacedAsset, SceneEditorActionButton, SceneEditorViewport},
    resources::{SceneEditorState, SceneEditorViewportState},
    types::{EditorAction, EditorTool, EditorView},
    ui::{
        gltf_browser::EditorGltfBrowserState,
        vegetation_preset_editor::{
            PresetEditorAction, PresetEditorButton, VegetationPresetEditorState,
        },
    },
};
use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured},
    tasks::IoTaskPool,
    ui::RelativeCursorPosition,
};
use feathrise_scene::{ScenePlacementInstance, ScenePlacementRuntimePhase, ScenePlacementRuntimeStatus};
use feathrise_scene_content::{
    ModelSurfaceVegetation, SceneId, ScenePlacementAssetTarget, SceneVegetationKind,
    VegetationPresetDescriptor, VegetationPresetRegistry, VegetationPresetRuntime,
};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};

fn private_placement_proof(world: &mut World, scene_id: &SceneId) -> Result<Option<Value>, String> {
    let mut instances = world.query::<(Entity, &ScenePlacementInstance)>();
    let active: Vec<_> = instances
        .iter(world)
        .filter(|(_, instance)| {
            instance.is_active() && instance.scene_id() == scene_id && instance.id() == MODEL
        })
        .take(3)
        .collect();
    if active.is_empty() {
        return Ok(None);
    }
    let preview = world.resource::<EditorAuthoringSession>().placement_previews.get(MODEL).copied();
    let visible = |root| world.get::<InheritedVisibility>(root).is_some_and(|state| state.get());
    let is_preview = |root| world.get::<EditorPlacedAsset>(root).is_some();
    let suppression = |root| world.get::<PlacementPreviewSuppression>(root).map(|state| state.owner);
    let failure = |reason| {
        let roots: Vec<_> = active
            .iter()
            .map(|(root, instance)| {
                format!(
                    "{root:?} source={} target={:?} direct={:?} inherited={:?} preview={} owner={:?}",
                    instance.asset_path(), instance.asset_target(), world.get::<Visibility>(*root),
                    world.get::<InheritedVisibility>(*root), is_preview(*root), suppression(*root),
                )
            })
            .collect();
        format!("模型截图私有 GLB 根校验失败（{reason}），草稿预览 {preview:?}，活动根 {roots:?}")
    };
    let target = ScenePlacementAssetTarget::Primitive {
        mesh: 0,
        primitive: 0,
        material: Some(0),
        collision: false,
    };
    if active.len() > 2 || active.iter().any(|(_, instance)| {
        instance.asset_path() != SOURCE || instance.asset_target() != target
    }) {
        return Err(failure("活动根数量或私有无碰撞网格不符"));
    }
    let mut visible_roots = active.iter().map(|(root, _)| *root).filter(|root| visible(*root));
    let root = visible_roots.next().ok_or_else(|| failure("没有实际可见根"))?;
    if visible_roots.next().is_some()
        || world.get::<Visibility>(root).is_none_or(|state| *state == Visibility::Hidden)
        || suppression(root).is_some()
    {
        return Err(failure("实际可见根不唯一或仍受遮蔽"));
    }
    let hidden = active.iter().map(|(entity, _)| *entity).find(|entity| *entity != root);
    // 编辑草稿的预览根可与正式根共存，必须由当前预览直接遮蔽且只有预览实际可见。
    let formal = match (is_preview(root), preview, hidden) {
        (false, None, None) => root,
        (true, Some(owner), Some(formal))
            if owner == root
                && !is_preview(formal)
                && world.get::<Visibility>(formal) == Some(&Visibility::Hidden)
                && world.get::<InheritedVisibility>(formal).is_some_and(|state| !state.get())
                && suppression(formal) == Some(root) => formal,
        _ => return Err(failure("预览与遮蔽的正式根没有同一所有者")),
    };
    Ok(Some(json!({
        "visible_root": format!("{root:?}"),
        "formal_root": format!("{formal:?}"),
        "visible_editor_preview": is_preview(root),
        "formal_root_hidden_by_preview": formal != root,
        "active_root_count": active.len(),
    })))
}

pub(super) fn begin(world: &mut World) -> Result<bool, String> {
    if !world.contains_resource::<Fixture>() {
        return Ok(false);
    }
    if world.resource::<Fixture>().phase != Phase::Idle {
        return Err("模型表面专项拒绝重复开始".into());
    }
    let paths = feathrise_editor_core::editor_project_paths().ok_or("模型表面验收没有自有工程")?;
    let owned =
        PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("缺少自有验收根")?);
    // 输出与工程是同一独占work根下的兄弟目录，不能误要求工程位于截图目录内。
    let work = owned.parent().ok_or("验收输出缺少本轮工作根")?;
    let owner: Value = serde_json::from_slice(&std::fs::read(work.join(".feathrise-native-validation-owner.json"))
        .map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    if !paths.project_root().starts_with(work)
        || owner["schema"] != 1 || owner["purpose"] != "ordinary-author-boundary-validation"
        || owner["work_root"] != json!(work)
    {
        return Err("模型表面专项只接受本轮验收根内的工程".into());
    }
    let draft = world
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .ok_or("模型表面缺少原草稿")?;
    if draft.scene_id.as_str() != "M1-SAMPLE"
        || draft.placements_path() != SOURCE
        || draft.placements.placements.len() > 16
        || !draft.placements.placements.iter().any(|record| {
            record.id == MODEL
                && record.asset_path == SOURCE
                && matches!(record.asset_target, ScenePlacementAssetTarget::Primitive {
                    mesh: 0, primitive: 0, collision: false, ..
                })
                && record.translation == [12., 1., 12.]
                && record.terrain_patch.is_none()
        })
    {
        return Err("模型表面专项缺少自建双层scene.glb，拒绝操作其他作者对象".into());
    }
    let scene = draft.scene_id.clone();
    let mut fixture = world.resource_mut::<Fixture>();
    fixture.scene = Some(scene);
    fixture.started = std::time::Instant::now();
    fixture.next(Phase::CreatePreset);
    Ok(true)
}

pub(super) fn lifecycle_input(world: &mut World) {
    // 批量复制在Input读取视口；必须在原输入前注入，不能等到供植被绘制使用的Cursor阶段。
    if world.resource::<Fixture>().phase == Phase::Lifecycle { super::lifecycle::pointer(world); }
}

pub(super) fn pointer(world: &mut World) {
    let phase = world.resource::<Fixture>().phase;
    if phase == Phase::Lifecycle { return; }
    let active = matches!(
        phase,
        Phase::Hover
            | Phase::Plant
            | Phase::PlantRelease
            | Phase::Erase
            | Phase::EraseRelease
            | Phase::UndoErase
            | Phase::WaitUndoErase
            | Phase::UndoPlant
            | Phase::WaitUndoPlant
            | Phase::RedoPlant
            | Phase::WaitRedoPlant
            | Phase::Settle
            | Phase::Capture
    );
    let pressed = matches!(phase, Phase::Plant | Phase::Erase);
    if world.resource::<Fixture>().mouse_owned && !pressed {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        world.resource_mut::<Fixture>().mouse_owned = false;
    }
    if phase == Phase::Erase {
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ShiftLeft);
    } else {
        world
            .resource_mut::<ButtonInput<KeyCode>>()
            .release(KeyCode::ShiftLeft);
    }
    if !active {
        return;
    }
    let showcase = matches!(phase, Phase::Settle | Phase::Capture);
    if let Some(mut mode) = world.get_resource_mut::<crate::viewport::modes::ViewportModeState>() {
        mode.mode = if showcase { crate::viewport::modes::ViewportMode::Select }
            else { crate::viewport::modes::ViewportMode::Edit };
        mode.pointer_over_controls = false;
    }
    {
        let mut editor = world.resource_mut::<SceneEditorState>();
        editor.tool = if showcase { EditorTool::Inspect } else { EditorTool::Vegetation };
        editor.view = if showcase { EditorView::Perspective } else { EditorView::Top };
        editor.orthographic = !showcase;
        editor.brush_radius = 0.7;
        editor.brush_strength = 1.;
    }
    if let Some(mut viewport) = world.get_resource_mut::<SceneEditorViewportState>() {
        // 只在所有业务及源恢复后转斜视，截图展示实际草叶/实例高度；原两后端Ready门保持不变。
        viewport.center = Vec3::new(12., if showcase { 1.25 } else { 1.15 }, 12.);
        viewport.extent = if showcase { 6. } else { 8. };
        viewport.yaw = if showcase { -0.72 } else { 0. };
        viewport.pitch = if showcase { 0.55 } else { std::f32::consts::FRAC_PI_2 };
    }
    let tail = phase == Phase::Plant
        && world.resource::<Fixture>().phase_started.elapsed() > Duration::from_millis(180);
    for mut cursor in world
        .query_filtered::<&mut RelativeCursorPosition, With<SceneEditorViewport>>()
        .iter_mut(world)
    {
        cursor.cursor_over = !showcase;
        cursor.normalized = Some(Vec2::new(if tail { 0.06 } else { -0.06 }, 0.));
    }
    if pressed {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        world.resource_mut::<Fixture>().mouse_owned = true;
    }
}

fn action(world: &mut World, action: EditorAction) -> Entity {
    world
        .spawn((
            SceneEditorActionButton(action),
            feathrise_editor_ui::EditorButtonClick::triggered(),
        ))
        .id()
}
fn preset_action(world: &mut World, action: PresetEditorAction) -> Entity {
    world
        .spawn((
            PresetEditorButton(action),
            feathrise_editor_ui::EditorButtonClick::triggered(),
        ))
        .id()
}
fn current(world: &World) -> Result<Arc<Vec<Arc<ModelSurfaceVegetation>>>, String> {
    Ok(model_vegetation::root_targets(
        world
            .resource::<EditorAuthoringSession>()
            .draft
            .as_ref()
            .ok_or("验收草稿已丢失")?,
    ))
}
fn command(mut command: &EditorCommand) -> &EditorCommand {
    for _ in 0..4 {
        command = match command {
            EditorCommand::RootWater(inner)
            | EditorCommand::TerrainCanvas { command: inner, .. } => inner,
            _ => return command,
        };
    }
    command
}
fn desired_kind(fixture: &Fixture) -> SceneVegetationKind {
    if fixture.round == 0 { SceneVegetationKind::Blade } else { SceneVegetationKind::Mesh }
}
/// 同类预设不能借给本专项：作者来源、每档网格及Mesh材质都必须属于独立植被原型。
fn isolated_preset(preset: &VegetationPresetDescriptor, kind: SceneVegetationKind) -> bool {
    preset_uses_source(preset, kind, VEGETATION_SOURCE)
}
pub(super) fn preset_uses_source(preset: &VegetationPresetDescriptor, kind: SceneVegetationKind, source: &str) -> bool {
    let from_source = |path: &str| path.split_once('#')
        .is_some_and(|(asset, label)| asset == source && !label.is_empty());
    preset.runtime.kind() == kind && !source.is_empty() && preset.author_asset_path == source
        && match &preset.runtime {
            VegetationPresetRuntime::Blade { lods, .. } => !lods.is_empty()
                && lods.iter().all(|lod| from_source(&lod.mesh_path)),
            VegetationPresetRuntime::Mesh { lods, .. } => !lods.is_empty()
                && lods.iter().all(|lod| !lod.parts.is_empty() && lod.parts.iter()
                    .all(|part| from_source(&part.mesh_path) && from_source(&part.material_path))),
        }
}
fn unchanged_registered(registry: &VegetationPresetRegistry, registered: &[VegetationPresetDescriptor]) -> bool {
    registered.len() <= 2 && registered.iter().enumerate().all(|(index, preset)| {
        let kind = if index == 0 { SceneVegetationKind::Blade } else { SceneVegetationKind::Mesh };
        isolated_preset(preset, kind) && registry.resolve(&preset.id) == Some(preset)
    })
}
fn creation_ready(registry: &VegetationPresetRegistry, fixture: &Fixture) -> bool {
    fixture.round < 2 && fixture.registered_presets.len() == fixture.round
        && registry.presets().len() == fixture.round && unchanged_registered(registry, &fixture.registered_presets)
}
fn adopted_preset<'a>(registry: &'a VegetationPresetRegistry, fixture: &Fixture)
    -> Result<Option<&'a VegetationPresetDescriptor>, String> {
    if registry.revision <= fixture.preset_revision { return Ok(None); }
    if fixture.round >= 2 || fixture.registered_presets.len() != fixture.round
        || registry.presets().len() != fixture.round + 1 || !unchanged_registered(registry, &fixture.registered_presets) {
        return Err("模型专项原预设保存改变了已登记目录或新增数量不符".into());
    }
    let preset = registry.presets().iter().find(|preset|
        fixture.registered_presets.iter().all(|before| before.id != preset.id))
        .filter(|preset| isolated_preset(preset, desired_kind(fixture))
            && fixture.registered_presets.iter().all(|before| before.output_id != preset.output_id))
        .ok_or("模型专项原预设保存没有采用独立来源的完整新增输出")?;
    Ok(Some(preset))
}

/// 完成的模型专项只借出真正经过原保存/目录采用的两项完整描述；同路径预设不能冒认归属。
pub(super) fn registered_presets(world: &World) -> Option<&[VegetationPresetDescriptor]> {
    let fixture = world.get_resource::<Fixture>()?;
    let scene = &world.get_resource::<EditorAuthoringSession>()?.draft.as_ref()?.scene_id;
    let registry = world.get_resource::<VegetationPresetRegistry>()?;
    if fixture.phase != Phase::Complete || !matches!(fixture.result.as_ref(), Some(Ok(_)))
        || fixture.scene.as_ref() != Some(scene) || fixture.registered_presets.len() != 2
        || registry.revision <= fixture.preset_revision || !(2..=3).contains(&registry.presets().len())
        || !unchanged_registered(registry, &fixture.registered_presets) {
        return None;
    }
    Some(&fixture.registered_presets)
}
fn statistics(
    targets: &[Arc<ModelSurfaceVegetation>],
    layer: &str,
) -> Result<(usize, u64), String> {
    if targets.len() > 4 {
        return Err("模型专项超出固定目标预算".into());
    }
    let data = targets
        .iter()
        .find(|data| {
            matches!(&data.target,
        feathrise_scene_content::SurfaceTarget::Model { object_id } if object_id == MODEL)
        })
        .ok_or("真实模型笔刷没有生成作者目标")?;
    if data.samples.is_empty() || data.samples.len() > 4096 {
        return Err("模型专项样本为空或超过固定观测预算".into());
    }
    let mut sum = 0;
    for sample in &data.samples {
        if sample.anchor.primitive != 0 || !(2..4).contains(&sample.anchor.triangle) {
            return Err("15厘米双层模型植被串入桥下或非命中表面".into());
        }
        sum += sample
            .layers
            .iter()
            .filter(|(id, _)| id == layer)
            .map(|(_, value)| u64::from(*value))
            .sum::<u64>();
    }
    Ok((data.samples.len(), sum))
}

pub(super) fn advance(world: &mut World) -> Option<Result<Value, String>> {
    if !world.contains_resource::<Fixture>() {
        return None;
    }
    world.resource_scope(|world, mut fixture: Mut<Fixture>| {
        if let Some(entity) = fixture.pulse.take() {
            let _ = world.despawn(entity);
        }
        if fixture.phase == Phase::Idle {
            return None;
        }
        if fixture.result.is_some() {
            return fixture.result.clone();
        }
        if let Err(error) = step(world, &mut fixture) {
            fixture.next(Phase::Failed);
            fixture.result = Some(Err(error));
        }
        fixture.result.clone()
    })
}

fn step(world: &mut World, fixture: &mut Fixture) -> Result<(), String> {
    if fixture.started.elapsed() > Duration::from_secs(240) {
        return Err(format!(
            "模型表面专项超时 {:?}: {}",
            fixture.phase,
            world.resource::<SceneEditorState>().status
        ));
    }
    let busy = world
        .resource::<EditorAuthoringSession>()
        .model_vegetation_busy;
    match fixture.phase {
        Phase::CreatePreset => {
            let registry = world.resource::<VegetationPresetRegistry>();
            if registry.revision == 0 {
                return Ok(());
            }
            if !creation_ready(registry, fixture) {
                return Err("模型专项只允许空白模板及本轮原保存登记的预设，拒绝复用既有作者库".into());
            }
            let editor = world.resource::<VegetationPresetEditorState>();
            if !editor.tabs.is_empty() { return Err("模型专项创建前仍有未关闭的预设标签".into()); }
            if editor.exit_blocker().is_some() { return Ok(()); }
            fixture.preset_revision = registry.revision;
            world.resource_mut::<SceneEditorState>().selected_asset = Some(VEGETATION_SOURCE.into());
            fixture.pulse = Some(preset_action(world, if fixture.round == 0 {
                PresetEditorAction::CreateBlade
            } else { PresetEditorAction::CreateMesh }));
            fixture.next(Phase::WaitPreset);
        }
        Phase::WaitPreset => {
            if let Some(tab) = world.resource::<VegetationPresetEditorState>().tabs.first() {
                if world.resource::<VegetationPresetEditorState>().tabs.len() != 1 {
                    return Err("模型专项原创建必须恰好生成一个预设标签".into());
                }
                fixture.tab = Some(tab.id);
                fixture.next(Phase::SavePreset);
            }
        }
        Phase::SavePreset => {
            fixture.pulse = Some(preset_action(world, PresetEditorAction::Save));
            fixture.next(Phase::WaitPresetSave);
        }
        Phase::WaitPresetSave => {
            let editor = world.resource::<VegetationPresetEditorState>();
            if !editor.saved_play_pending() && editor.exit_blocker().is_none()
                && let Some(preset) = adopted_preset(world.resource::<VegetationPresetRegistry>(), fixture)? {
                // 冻结真实热采用后的完整内容，后续组合夹具只接受这份不可变登记。
                fixture.preset = Some(preset.id.clone());
                fixture.registered_presets.push(preset.clone());
                fixture.next(Phase::ClosePreset);
            }
        }
        Phase::ClosePreset => {
            fixture.pulse = Some(preset_action(
                world,
                PresetEditorAction::ClosePresetTab(fixture.tab.unwrap()),
            ));
            fixture.next(Phase::WaitPresetClose);
        }
        Phase::WaitPresetClose => {
            if world
                .resource::<VegetationPresetEditorState>()
                .tabs
                .is_empty()
            {
                fixture.next(Phase::Focus);
            }
        }
        Phase::Focus => {
            world
                .resource_mut::<EditorGltfBrowserState>()
                .selected_placement_id = Some(MODEL.into());
            world.resource_mut::<SceneEditorState>().tool = EditorTool::Vegetation;
            fixture.next(Phase::WaitRoot);
        }
        Phase::WaitRoot => {
            let session = world.resource::<EditorAuthoringSession>();
            let browser = world.resource::<EditorGltfBrowserState>();
            if session
                .draft
                .as_ref()
                .is_some_and(|draft| draft.terrain_documents.active.is_none())
                && browser
                    .placement_record_index(MODEL, session.placement_structure_revision)
                    .is_some()
            {
                fixture.before = current(world)?;
                fixture.next(Phase::AddLayer);
            }
        }
        Phase::AddLayer => {
            fixture.layers_before = world.resource::<EditorAuthoringSession>().draft.as_ref()
                .and_then(|draft| draft.vegetation.as_ref()).and_then(|data| data.authoring.as_ref())
                .map_or(0, |author| author.layers.len());
            world
                .resource_mut::<SceneEditorState>()
                .vegetation_selected_preset_id = fixture.preset.clone();
            fixture.pulse = Some(action(world, EditorAction::VegetationAddLayer));
            fixture.next(Phase::WaitLayer);
        }
        Phase::WaitLayer => {
            let editor = world.resource::<SceneEditorState>();
            let session = world.resource::<EditorAuthoringSession>();
            if let Some(layer) = session
                .draft
                .as_ref()
                .and_then(|draft| draft.vegetation.as_ref())
                .and_then(|vegetation| vegetation.authoring.as_ref())
                .filter(|authoring| authoring.layers.len() == fixture.layers_before + 1)
                .and_then(|authoring| authoring.layers.get(editor.selected_vegetation_layer))
            {
                let output = session.draft.as_ref().and_then(|draft| draft.vegetation.as_ref())
                    .and_then(|vegetation| vegetation.layers.iter().find(|output| output.id == layer.output_layer_id))
                    .ok_or("模型专项新作者层缺少正式输出")?;
                if output.runtime_profile.as_ref() != fixture.preset.as_ref()
                    || output.asset_path != VEGETATION_SOURCE || output.kind != desired_kind(fixture)
                {
                    return Err("模型专项新图层未绑定独立植被预设来源".into());
                }
                fixture.layer = Some(layer.id.clone());
                fixture.next(Phase::Hover);
            }
        }
        Phase::Hover => {
            if world
                .resource::<model_vegetation::ModelVegetationState>()
                .cursor_hit(MODEL)
                .is_some_and(|hit| (hit.y - 1.15).abs() < 0.001)
            {
                fixture.undo_before = world.resource::<EditorAuthoringSession>().undo.len();
                fixture.next(Phase::Plant);
            }
        }
        Phase::Plant => {
            if fixture.phase_started.elapsed() > Duration::from_millis(450) {
                fixture.next(Phase::PlantRelease);
            }
        }
        Phase::PlantRelease => {
            if !busy && fixture.phase_started.elapsed() > Duration::from_millis(80) {
                let session = world.resource::<EditorAuthoringSession>();
                if session.undo.len() != fixture.undo_before + 1
                    || !matches!(session.undo.last().map(command),
                Some(EditorCommand::ModelVegetationChanged {object_id, ..}) if object_id == MODEL)
                {
                    return Err("真实模型整笔没有且仅有一条历史".into());
                }
                let planted = current(world)?;
                (fixture.samples, fixture.planted_sum) =
                    statistics(&planted, fixture.layer.as_deref().unwrap())?;
                if fixture.planted_sum == 0 {
                    return Err("真实模型笔刷没有生成非零密度".into());
                }
                fixture.planted = Some(planted);
                fixture.next(Phase::Erase);
            }
        }
        Phase::Erase => {
            if fixture.phase_started.elapsed() > Duration::from_millis(400) {
                fixture.next(Phase::EraseRelease);
            }
        }
        Phase::EraseRelease => {
            if !busy && fixture.phase_started.elapsed() > Duration::from_millis(80) {
                fixture.erased_sum =
                    statistics(&current(world)?, fixture.layer.as_deref().unwrap())?.1;
                if fixture.erased_sum >= fixture.planted_sum {
                    return Err("原Shift模型擦除未降低命中面密度".into());
                }
                if world.resource::<EditorAuthoringSession>().undo.len() != fixture.undo_before + 2
                {
                    return Err("原Shift擦除未独立形成一笔历史".into());
                }
                fixture.next(Phase::UndoErase);
            }
        }
        Phase::UndoErase => {
            fixture.pulse = Some(action(world, EditorAction::Undo));
            fixture.next(Phase::WaitUndoErase);
        }
        Phase::WaitUndoErase => {
            if !busy && Arc::ptr_eq(&current(world)?, fixture.planted.as_ref().unwrap()) {
                fixture.next(Phase::UndoPlant);
            }
        }
        Phase::UndoPlant => {
            fixture.pulse = Some(action(world, EditorAction::Undo));
            fixture.next(Phase::WaitUndoPlant);
        }
        Phase::WaitUndoPlant => {
            if !busy && Arc::ptr_eq(&current(world)?, &fixture.before) {
                fixture.next(Phase::RedoPlant);
            }
        }
        Phase::RedoPlant => {
            fixture.pulse = Some(action(world, EditorAction::Redo));
            fixture.next(Phase::WaitRedoPlant);
        }
        Phase::WaitRedoPlant => {
            if !busy && Arc::ptr_eq(&current(world)?, fixture.planted.as_ref().unwrap()) {
                let preset = world.resource::<VegetationPresetRegistry>()
                    .resolve(fixture.preset.as_deref().ok_or("模型专项缺少当前预设ID")?)
                    .filter(|preset| fixture.registered_presets.get(fixture.round) == Some(*preset))
                    .ok_or("模型专项预设来源在笔划期间变化")?;
                fixture.layer_proofs.push(json!({"kind":if fixture.round == 0 {"blade"} else {"mesh"},
                    "layer":fixture.layer,"samples":fixture.samples,"planted_density_sum":fixture.planted_sum,
                    "erased_density_sum":fixture.erased_sum,"undo_redo":true,
                    "preset_id":preset.id,"preset_source":preset.author_asset_path,"preset_runtime":preset.runtime}));
                if fixture.round == 0 {
                    fixture.round = 1;
                    fixture.preset = None;
                    fixture.layer = None;
                    fixture.tab = None;
                    fixture.next(Phase::CreatePreset);
                } else { fixture.next(Phase::Lifecycle); }
            }
        }
        Phase::Lifecycle => {
            if let Some(proof) = super::lifecycle::advance(world, &mut fixture.lifecycle, &mut fixture.pulse)? {
                fixture.layer_proofs.push(proof);
                fixture.next(Phase::Settle);
            }
        }
        Phase::Settle => {
            let session = world.resource::<EditorAuthoringSession>();
            let scene_id = session.draft.as_ref().ok_or("模型截图草稿丢失")?.scene_id.clone();
            let published = world.resource::<feathrise_scene::DynamicModelVegetationDraft>()
                .frozen_for_scene(&scene_id);
            let status = world.resource::<feathrise_scene::DynamicModelVegetationStatus>();
            if status.failed { return Err(format!("模型正式植被生成失败：{}",status.detail)); }
            let adopted = published.as_ref().is_some_and(|(revision, data)| status.ready
                && status.scene_id.as_ref() == Some(&scene_id)
                && status.applied_revision == Some(*revision)
                && status.applied_document == Some(Arc::as_ptr(data) as usize)
                && status.blade_window_count > 0 && status.mesh_batch_count > 0);
            // 作者样本完成不能代替GPU/实例采用；必须等待同一份发布Arc的完整运行时回执。
            if adopted && fixture.phase_started.elapsed() > Duration::from_millis(600) {
                let placements = world.resource::<ScenePlacementRuntimeStatus>();
                if placements.scene_id.as_ref() != Some(&scene_id) { return Ok(()); }
                if placements.phase == ScenePlacementRuntimePhase::Failed {
                    return Err(format!("私有 GLB 模型截图前摆放未采用：{}", placements.detail));
                }
                if placements.phase != ScenePlacementRuntimePhase::Ready { return Ok(()); }
                let placement_revision = placements.revision;
                let status = status.clone();
                let Some(placement) = private_placement_proof(world, &scene_id)? else { return Ok(()); };
                fixture.runtime_proof = Some(json!({"ready":status.ready,"scene_id":status.scene_id,
                    "runtime_revision":status.runtime_revision,"applied_revision":status.applied_revision,
                    "blade_windows":status.blade_window_count,"mesh_batches":status.mesh_batch_count,
                    "private_placement_source":SOURCE,"private_placement_revision":placement_revision,
                    "private_placement_visibility":placement,
                    "instance_path":format!("{:?}",status.instance_path),"detail":status.detail}));
                let path = PathBuf::from(
                    std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR")
                        .ok_or("模型截图缺少自有输出根")?,
                )
                .join("model-surface.png");
                let completion = fixture.capture.clone();
                let author_targets = current(world)?;
                world.spawn(Screenshot::primary_window()).observe(
                    move |captured: On<ScreenshotCaptured>| {
                        let image = captured.image.clone();
                        let path = path.clone();
                        let completion = completion.clone();
                        let author_targets = author_targets.clone();
                        IoTaskPool::get()
                            .spawn(async move {
                                let result = (|| {
                                    // 同一后台任务冻结完整目标摘要；输入帧只复制Arc，不序列化样本。
                                    if author_targets.len() > 4 { return Err("模型截图目标超过固定预算".into()); }
                                    let target = author_targets.iter().find(|target| matches!(&target.target,
                                        feathrise_scene_content::SurfaceTarget::Model { object_id } if object_id == MODEL))
                                        .ok_or("模型截图缺少作者目标")?;
                                    if target.samples.is_empty() || target.samples.len() > 4096 || !target.sample_blocks.is_empty() {
                                        return Err("模型截图作者样本为空、超预算或尚未完整读取".into());
                                    }
                                    let bytes = serde_json::to_vec(target.as_ref()).map_err(|error| error.to_string())?;
                                    let digest = feathrise_scene_content::authoring_io::bytes_hash(&bytes);
                                    image.try_into_dynamic().map_err(|error| error.to_string())?
                                        .to_rgb8().save(&path).map_err(|error| error.to_string())?;
                                    Ok(digest)
                                })();
                                if let Ok(mut slot) = completion.lock() {
                                    *slot = Some(result);
                                }
                            })
                            .detach();
                    },
                );
                fixture.next(Phase::Capture);
            }
        }
        Phase::Capture => {
            let result = fixture
                .capture
                .try_lock()
                .ok()
                .and_then(|mut slot| slot.take());
            if let Some(result) = result {
                let author_target_sha256 = result?; // GPU捕获、后台摘要及PNG写盘完成后才报告专项完成。
                fixture.result = Some(Ok(
                    json!({"scope":"original_camera_static_scene_glb_layered_surface_brush",
                "object_id":MODEL,"source":SOURCE,"collision_required":false,"layer":fixture.layer,
                "vegetation_source":VEGETATION_SOURCE,
                "layers":fixture.layer_proofs,"runtime":fixture.runtime_proof,
                "author_target_sha256":author_target_sha256,
                "samples":fixture.samples,"planted_density_sum":fixture.planted_sum,"erased_density_sum":fixture.erased_sum,
                "only_hit_triangles":[2,3],"layer_separation_meters":0.15,"undo_redo":true,
                "save_owner":"parent_original_scene_save","screenshot":"model-surface.png","complete":true}),
                ));
                fixture.next(Phase::Complete);
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_presets() -> Vec<VegetationPresetDescriptor> {
        let catalog: feathrise_scene_content::VegetationPresetCatalogAsset = serde_json::from_str(
            include_str!("../../../tests/fixtures/vegetation_presets.vegetation_presets.json"),
        ).unwrap();
        [SceneVegetationKind::Blade, SceneVegetationKind::Mesh].into_iter().map(|kind| {
            let mut preset = catalog.presets.iter().find(|preset| preset.kind() == kind).unwrap().clone();
            preset.author_asset_path = VEGETATION_SOURCE.into();
            match &mut preset.runtime {
                VegetationPresetRuntime::Blade { lods, .. } => {
                    for lod in lods { lod.mesh_path = format!("{VEGETATION_SOURCE}#Mesh0/Primitive0"); }
                }
                VegetationPresetRuntime::Mesh { lods, .. } => {
                    for part in lods.iter_mut().flat_map(|lod| &mut lod.parts) {
                        part.mesh_path = format!("{VEGETATION_SOURCE}#Mesh0/Primitive0");
                        part.material_path = format!("{VEGETATION_SOURCE}#Material0");
                    }
                }
            }
            preset
        }).collect()
    }
    fn registry(presets: Vec<VegetationPresetDescriptor>, revision: u64) -> VegetationPresetRegistry {
        let catalog = feathrise_scene_content::VegetationPresetCatalogAsset {
            schema_version: feathrise_scene_content::VEGETATION_PRESET_SCHEMA_VERSION, presets,
        };
        VegetationPresetRegistry { catalog: Handle::default(), revision,
            resolved: feathrise_scene_content::ResolvedVegetationPresetCatalog::build(&catalog).unwrap(), last_error: None }
    }

    #[test]
    fn registration_requires_empty_library_then_unchanged_original_save_adoption() {
        let pair = fixture_presets();
        let mut fixture = Fixture::default();
        assert!(creation_ready(&registry(Vec::new(), 1), &fixture));
        assert!(!creation_ready(&registry(vec![pair[0].clone()], 1), &fixture), "同来源既有作者预设不能冒认本轮创建");
        fixture.preset_revision = 1;
        assert!(adopted_preset(&registry(vec![pair[0].clone()], 1), &fixture).unwrap().is_none(), "保存请求不能代替目录热采用");
        let first = registry(vec![pair[0].clone()], 2);
        let adopted = adopted_preset(&first, &fixture).unwrap().unwrap().clone();
        fixture.registered_presets.push(adopted);
        fixture.round = 1;
        assert!(creation_ready(&first, &fixture));
        let mut changed = pair[0].clone(); changed.label.push_str("已修改");
        assert!(!creation_ready(&registry(vec![changed.clone()], 3), &fixture));
        fixture.preset_revision = 2;
        assert!(adopted_preset(&registry(vec![changed, pair[1].clone()], 3), &fixture).is_err(), "原完整登记变化必须拒绝");
        let complete = registry(pair.clone(), 3);
        assert_eq!(adopted_preset(&complete, &fixture).unwrap(), Some(&pair[1]));
        let mut extra = pair[0].clone(); extra.id = "extra-blade".into(); extra.output_id = "extra-output".into(); extra.aliases.clear();
        let mut oversized = pair; oversized.push(extra);
        assert!(adopted_preset(&registry(oversized, 3), &fixture).is_err(), "一次原保存只能登记一个新增项");
    }

    #[test]
    fn registration_rejects_hot_reload_lod_even_with_unchanged_author_source() {
        let mut pair = fixture_presets();
        let mut fixture = Fixture::default(); fixture.round = 1; fixture.preset_revision = 2;
        fixture.registered_presets.push(pair[0].clone());
        if let VegetationPresetRuntime::Mesh { lods, .. } = &mut pair[1].runtime {
            lods.last_mut().unwrap().parts.last_mut().unwrap().material_path = "scene/probe.glb#Material0".into();
        }
        assert!(adopted_preset(&registry(pair, 3), &fixture).is_err());
    }

    #[test]
    fn isolated_presets_reject_other_author_sources_and_any_hot_reload_lod() {
        let catalog: feathrise_scene_content::VegetationPresetCatalogAsset = serde_json::from_str(
            include_str!("../../../tests/fixtures/vegetation_presets.vegetation_presets.json"),
        ).unwrap();
        for kind in [SceneVegetationKind::Blade, SceneVegetationKind::Mesh] {
            let mut good = catalog.presets.iter().find(|preset| preset.kind() == kind).unwrap().clone();
            good.author_asset_path = VEGETATION_SOURCE.into();
            match &mut good.runtime {
                VegetationPresetRuntime::Blade { lods, .. } => {
                    for lod in lods { lod.mesh_path = format!("{VEGETATION_SOURCE}#Mesh0/Primitive0"); }
                }
                VegetationPresetRuntime::Mesh { lods, .. } => {
                    for part in lods.iter_mut().flat_map(|lod| &mut lod.parts) {
                        part.mesh_path = format!("{VEGETATION_SOURCE}#Mesh0/Primitive0");
                        part.material_path = format!("{VEGETATION_SOURCE}#Material0");
                    }
                }
            }
            assert!(isolated_preset(&good, kind));
            let mut foreign = good.clone();
            foreign.author_asset_path = "scene/probe.glb".into();
            assert!(!isolated_preset(&foreign, kind), "同类预设也不能复用热更新专项的来源");
            let mut mixed = good.clone();
            match &mut mixed.runtime {
                VegetationPresetRuntime::Blade { lods, .. } => {
                    lods.last_mut().unwrap().mesh_path = "scene/probe.glb#Mesh0/Primitive0".into();
                }
                VegetationPresetRuntime::Mesh { lods, .. } => {
                    lods.last_mut().unwrap().parts.last_mut().unwrap().mesh_path = "scene/probe.glb#Mesh0/Primitive0".into();
                }
            }
            assert!(!isolated_preset(&mixed, kind), "作者路径正确也不能掩盖任一LOD仍被其他专项修改");
            if let VegetationPresetRuntime::Mesh { lods, .. } = &mut good.runtime {
                lods.last_mut().unwrap().parts.last_mut().unwrap().material_path = "scene/probe.glb#Material0".into();
                assert!(!isolated_preset(&good, kind), "材质也必须从独立GLB读取相邻原贴图");
            }
        }
    }
}
