//! 地形专项只在真实运行时采用稳定后捕获GPU画面；后台写盘回执控制父工作流推进。

use super::{resources::GeometryFixture, types::Domain};
use bevy::{prelude::*, render::view::screenshot::{Screenshot, ScreenshotCaptured}, tasks::IoTaskPool};
use feathrise_scene::{DynamicSceneRuntimePhase, DynamicSceneRuntimeStatus, TerrainObjectQueries, TerrainObjectResource};
use feathrise_environment::vegetation::blade::{GpuBladeDensityMap, GpuBladeGpuReadbackStats, GpuBladeReadbackConfig, GpuBladeRenderStats};
use std::path::PathBuf;
use std::time::{Duration, Instant};
mod volume; // 原生体素按自己的完整三维采用代和目标草窗回读，不能借二维或模型草的总数。

/// 只读诊断请求另有票据；同源对象的另一实例、旧密度或旧几何窗口不能给本次截图信用。
#[derive(Clone, Debug, PartialEq)]
struct WindowIdentity {
    entity: Entity,
    source_revision: u64,
    height_revision: [u64; 3],
    density: GpuBladeDensityMap,
    source_to_world: [f32; 16],
    origin: Vec3,
    focus: Vec3,
}

#[derive(Clone, Debug, PartialEq)]
struct HeightCaptureIdentity {
    scene: String,
    terrain: String,
    instance: Entity,
    author_revision: u64,
    layout_revision: u64,
    density_revision: u64,
    camera: Entity,
    camera_pose: [f32; 16],
    projection: [f32; 16],
    windows: Vec<WindowIdentity>,
}

#[derive(Resource)]
struct HeightGpuGate {
    identity: HeightCaptureIdentity,
    ticket: u64,
    baseline: u64,
    started: Instant,
    last_diagnostic: Option<Instant>,
    was_ready: bool,
    previous: GpuBladeReadbackConfig,
}

pub(super) fn file_name(domain: Domain) -> &'static str {
    match domain { Domain::Height => "geometry-small.png", Domain::HeightVegetation => "geometry-clipmap-vegetation.png", Domain::Voxel => "geometry-voxel.png",
        Domain::VoxelUnderside => "geometry-voxel-underside.png" }
}

pub(super) fn runtime_ready(world: &mut World, state: &mut GeometryFixture) -> Result<bool, String> {
    use bevy::ecs::system::RunSystemOnce;
    let scene = state.scene.clone().ok_or("截图场景身份缺失")?;
    let terrain = state.target().cloned().ok_or("截图地形身份缺失")?;
    if state.domain == Domain::HeightVegetation && !height_vegetation_ready(world, state)? { return Ok(false); }
    let voxel = matches!(state.domain, Domain::Voxel | Domain::VoxelUnderside);
    if voxel && !volume::ready(world, state)? { return Ok(false); }
    world.run_system_once(move |queries: TerrainObjectQueries, runtimes: TerrainObjectResource<DynamicSceneRuntimeStatus>| {
        let Some(instance) = queries.instance(&scene, &terrain) else { return false; };
        // 原生体素由voxel_objects共同采用显示/字段/碰撞，不属于Clipmap洞口的volume_stream窗口。
        queries.render_mesh(&scene, &terrain).is_some() && (!voxel || runtimes.for_saved_object(&scene, &terrain)
            .is_some_and(|(active, status)| active == instance && status.scene_id.as_ref() == Some(&scene) && status.phase == DynamicSceneRuntimePhase::Ready))
    }).map_err(|error| error.to_string())
}

/// 二维正式页发布后，必须由同一地形活动实例采用同代布局和密度，且真实GPU窗口已绑定。
fn height_vegetation_ready(world: &mut World, state: &mut GeometryFixture) -> Result<bool, String> {
    use bevy::ecs::system::SystemState;
    use feathrise_environment::vegetation::{VegetationBladeLayer,
        blade::{GpuBladeClipmapHeight, GpuBladeDensityMap, GpuBladePatch, GpuBladeSourceRevision}};
    use feathrise_scene::{DynamicBladeDraft, DynamicVegetationPhase, DynamicVegetationStatus};
    let session = world.resource::<crate::authoring::EditorAuthoringSession>();
    let Some(draft) = session.draft.as_ref() else { return Ok(false); };
    if session.vegetation_compiled_revision != session.vegetation_revision
        || session.vegetation_compiled_structure_revision != session.vegetation_structure_revision { return Ok(false); }
    let Some(live) = world.get_resource::<DynamicBladeDraft>() else { return Ok(false); };
    let Some(snapshot) = live.for_scene(&draft.scene_id) else { return Ok(false); };
    let Some(density) = state.proof.height_vegetation.as_ref().and_then(|proof| proof.after.density.as_ref()) else { return Ok(false); };
    if snapshot.source_revision != session.vegetation_revision || !snapshot.densities.contains_key(&density.output_layer) { return Ok(false); }
    let Some(runtime_layer) = snapshot.definition.layers.iter().find(|layer| layer.id == density.output_layer)
        .and_then(|layer| layer.runtime_layer_id) else { return Ok(false); };
    let author_revision = session.vegetation_revision;
    let scene = draft.scene_id.clone(); let Some(terrain) = state.target() else { return Ok(false); };
    let terrain = terrain.as_str().to_owned();
    let layout = live.layout_revision_for_scene(&scene); let density_revision = live.revision_for_scene(&scene);
    let mut access = SystemState::<TerrainObjectResource<DynamicVegetationStatus>>::new(world);
    let Ok(statuses) = access.get(world) else { return Ok(false); };
    let Some((instance, status)) = statuses.for_saved_object(&scene, state.target().unwrap()) else { return Ok(false); };
    if status.scene_id.as_ref() != Some(&scene) || status.phase != DynamicVegetationPhase::Ready
        || status.applied_draft_revision != layout || status.applied_density_revision != density_revision
        || status.blade_layer_count != 1 || status.blade_window_count == 0 { return Ok(false); }
    let expected = status.blade_window_count;
    let mut bindings = Vec::new();
    let mut windows = Vec::new();
    let mut visible_windows = 0;
    for (entity, layer, binding) in world.query::<(Entity, &VegetationBladeLayer, &GpuBladeDensityMap)>().iter(world) {
        if layer.0.0 == runtime_layer && visible_descendant(world, entity, instance) {
            bindings.push(binding.clone());
            if bindings.len() > expected { return Ok(false); }
            let (Some(height), Some(source), Some(patch), Some(local), Some(global)) = (
                world.get::<GpuBladeClipmapHeight>(entity), world.get::<GpuBladeSourceRevision>(entity),
                world.get::<GpuBladePatch>(entity), world.get::<Transform>(entity), world.get::<GlobalTransform>(entity),
            ) else { return Ok(false); };
            if !height.ready || Some(source.0) != density_revision || source.0 == 0 { return Ok(false); }
            if world.get::<ViewVisibility>(entity).is_some_and(|value| value.get()) { visible_windows += 1; }
            windows.push(WindowIdentity { entity, source_revision: source.0, height_revision: height.revision,
                density: binding.clone(), source_to_world: (global.to_matrix() * local.to_matrix().inverse()).to_cols_array(),
                origin: patch.origin(), focus: patch.focus() });
        }
    }
    if bindings.len() != expected || !bindings.iter().all(|binding| binding == &bindings[0]) { return Ok(false); }
    windows.sort_unstable_by_key(|window| window.entity.to_bits());
    let camera = world.resource::<crate::resources::SceneEditorViewportState>().camera;
    let Some(camera_pose) = world.get::<GlobalTransform>(camera).map(|value| value.to_matrix().to_cols_array()) else { return Ok(false); };
    let Some(projection) = world.get::<Camera>(camera).map(|value| value.clip_from_view().to_cols_array()) else { return Ok(false); };
    height_gpu_ready(world, state, HeightCaptureIdentity { scene: scene.as_str().into(), terrain, instance,
        author_revision, layout_revision: layout.unwrap_or(0), density_revision: density_revision.unwrap_or(0),
        camera, camera_pose, projection, windows }, visible_windows)
}

/// 真实Compute计数按窗口筛选，CPU绘制计数仅作附加门；最终仍必须独立查看实际GPU截图。
fn height_gpu_ready(world: &mut World, state: &mut GeometryFixture, identity: HeightCaptureIdentity,
    visible_windows: usize) -> Result<bool, String> {
    let gpu = world.get_resource::<GpuBladeGpuReadbackStats>().copied().unwrap_or_default();
    let render = world.get_resource::<GpuBladeRenderStats>().copied().unwrap_or_default();
    let config = world.get_resource::<GpuBladeReadbackConfig>().copied().ok_or("真实GPU回读插件未安装")?;
    let mut gate = world.remove_resource::<HeightGpuGate>().unwrap_or_else(|| HeightGpuGate {
        identity: identity.clone(), ticket: config.selection_ticket().wrapping_add(1).max(1), baseline: gpu.sample_sequence,
        started: Instant::now(), last_diagnostic: None, was_ready: false, previous: config,
    });
    if gate.identity != identity {
        gate.identity = identity; gate.ticket = gate.ticket.wrapping_add(1).max(1);
        gate.baseline = gpu.sample_sequence; gate.started = Instant::now(); gate.last_diagnostic = None; gate.was_ready = false;
    }
    let entities: Vec<_> = gate.identity.windows.iter().map(|window| window.entity).collect();
    let request = GpuBladeReadbackConfig::for_windows(1, gate.ticket, &entities).ok_or("截图GPU目标超过固定32窗或含重复身份")?;
    world.insert_resource(request);
    let current_sample = selected_sample_matches(&gpu, gate.ticket, gate.baseline,
        &entities, gate.identity.density_revision);
    let ready = current_sample && gpu.rendered_instances > 0 && gpu.surface_survivors > 0
        && gpu.density_survivors > 0 && gpu.scene_errors == 0 && gpu.overflow_instances == 0
        && visible_windows > 0 && render.target_views > 0 && render.prepared_bind_groups >= entities.len() as u32
        && render.compute_pipeline_ready_stages == 4 && render.compute_pipeline_waiting_stages == 0
        && render.compute_pipeline_error_stages == 0 && render.pipeline_error_draws == 0
        && render.pipeline_waiting_draws == 0 && render.pipeline_ready_draws > 0 && render.drawn_draws > 0;
    let timeout = gate.started.elapsed() > Duration::from_secs(60);
    let print = (ready && !gate.was_ready) || (!ready && (timeout
        || gate.last_diagnostic.is_none_or(|last| last.elapsed() >= Duration::from_secs(2))));
    if ready || print {
        let diagnostic = gpu_diagnostic(&gate, &gpu, &render, visible_windows, current_sample, ready);
        if print { eprintln!("Clipmap植被实际GPU等待：{diagnostic}"); }
        state.proof.height_vegetation_gpu = Some(diagnostic.clone());
        if print { gate.last_diagnostic = Some(Instant::now()); }
        if timeout && !ready {
            world.insert_resource(gate);
            return Err(format!("Clipmap同代GPU绘制六十秒未就绪：{diagnostic}"));
        }
    }
    gate.was_ready = ready;
    world.insert_resource(gate);
    Ok(ready)
}

fn selected_sample_matches(gpu: &GpuBladeGpuReadbackStats, ticket: u64, baseline: u64,
    entities: &[Entity], revision: u64) -> bool {
    if ticket == 0 || revision == 0 || gpu.selection_ticket != ticket || gpu.sample_sequence <= baseline
        || gpu.source_revision != revision || gpu.truncated_windows != 0 || entities.is_empty()
        || entities.len() > gpu.sampled_main_entities.len() || gpu.sampled_windows as usize != entities.len() {
        return false;
    }
    let count = entities.len();
    let expected_mask = if count == 32 { u32::MAX } else { (1u32 << count) - 1 };
    if gpu.sampled_output_valid_mask != expected_mask { return false; }
    let mut actual = gpu.sampled_main_entities[..count].to_vec(); actual.sort_unstable();
    let mut expected: Vec<_> = entities.iter().map(|entity| entity.to_bits()).collect(); expected.sort_unstable();
    actual == expected && gpu.sampled_source_revisions[..count].iter().all(|source| *source == revision)
}

fn gpu_diagnostic(gate: &HeightGpuGate, gpu: &GpuBladeGpuReadbackStats, render: &GpuBladeRenderStats,
    visible_windows: usize, current_sample: bool, ready: bool) -> serde_json::Value {
    use serde_json::json;
    let count = (gpu.sampled_windows as usize).min(gpu.sampled_main_entities.len());
    json!({"scene":gate.identity.scene,"terrain":gate.identity.terrain,"instance":gate.identity.instance.to_bits(),
        "author_revision":gate.identity.author_revision,"layout_revision":gate.identity.layout_revision,
        "density_revision":gate.identity.density_revision,"ticket":gate.ticket,"sample_baseline":gate.baseline,
        "current_sample":current_sample,"ready_for_capture":ready,"wait_seconds":gate.started.elapsed().as_secs_f32(),
        "camera":{"entity":gate.identity.camera.to_bits(),"pose":gate.identity.camera_pose,"projection":gate.identity.projection},
        "windows":gate.identity.windows.iter().map(|window|json!({"entity":window.entity.to_bits(),
            "source_revision":window.source_revision,"height_revision":window.height_revision,"density":format!("{:?}",window.density),
            "source_to_world":window.source_to_world,"origin":window.origin.to_array(),"focus":window.focus.to_array()})).collect::<Vec<_>>(),
        "gpu":{"ticket":gpu.selection_ticket,"sample_sequence":gpu.sample_sequence,"source_revision":gpu.source_revision,
            "sampled_windows":gpu.sampled_windows,"truncated_windows":gpu.truncated_windows,
            "main_entities":&gpu.sampled_main_entities[..count],"source_revisions":&gpu.sampled_source_revisions[..count],
            "output_valid_mask":gpu.sampled_output_valid_mask,"candidate_count":gpu.candidate_count,"active_tiles":gpu.active_tiles,
            "distance_survivors":gpu.distance_survivors,"density_survivors":gpu.density_survivors,
            "surface_survivors":gpu.surface_survivors,"rendered_instances":gpu.rendered_instances,
            "rendered_instances_by_lod":gpu.rendered_instances_by_lod,"overflow_instances":gpu.overflow_instances,"scene_errors":gpu.scene_errors},
        "render":{"visible_target_windows":visible_windows,"target_views":render.target_views,"extracted_windows":render.extracted_windows,
            "input_ready_windows":render.input_ready_windows,"height_waiting_windows":render.height_waiting_windows,
            "density_waiting_windows":render.density_waiting_windows,"prepared_bind_groups":render.prepared_bind_groups,
            "compute_ready":render.compute_pipeline_ready_stages,"compute_waiting":render.compute_pipeline_waiting_stages,
            "compute_errors":render.compute_pipeline_error_stages,"pipeline_ready":render.pipeline_ready_draws,
            "pipeline_waiting":render.pipeline_waiting_draws,"pipeline_errors":render.pipeline_error_draws,
            "queued_draws":render.queued_draws,"drawn_draws":render.drawn_draws}})
}

pub(super) fn restore(world: &mut World) {
    volume::restore(world);
    if let Some(gate) = world.remove_resource::<HeightGpuGate>() { world.insert_resource(gate.previous); }
}

fn visible_descendant(world: &World, mut entity: Entity, instance: Entity) -> bool {
    for _ in 0..16 {
        if world.get::<Visibility>(entity) == Some(&Visibility::Hidden) { return false; }
        if entity == instance { return true; }
        let Some(parent) = world.get::<ChildOf>(entity) else { return false; };
        entity = parent.parent();
    }
    false
}

pub(super) fn request(world: &mut World, state: &mut GeometryFixture) -> Result<(), String> {
    let path = PathBuf::from(std::env::var_os("NEW_EDITOR_NATIVE_SMOKE_DIR").ok_or("几何截图缺少自有输出根")?)
        .join(file_name(state.domain));
    if !path.is_absolute() { return Err("几何截图输出必须为绝对路径".into()); }
    state.capture_revision = world.resource::<crate::authoring::EditorAuthoringSession>().revision;
    let completion = state.capture.clone();
    let domain = state.domain;
    let report = match domain {
        Domain::Height => None,
        Domain::HeightVegetation => state.proof.height_vegetation_gpu.clone(),
        Domain::Voxel => state.proof.voxel_gpu.clone(),
        Domain::VoxelUnderside => state.proof.voxel_underside_gpu.clone(),
    };
    world.spawn(Screenshot::primary_window()).observe(move |captured: On<ScreenshotCaptured>,
        mut config: Option<ResMut<GpuBladeReadbackConfig>>, mut gate: Option<ResMut<HeightGpuGate>>,
        volume_gate: Option<Res<volume::VolumeGpuGate>>| {
        // 像素已由真实帧捕获才恢复原诊断范围；不会临时隐藏或改动任何绘制窗口。
        if report.is_some() && let Some(config) = config.as_mut() {
            if domain == Domain::HeightVegetation && let Some(gate) = gate.as_mut() {
                **config = gate.previous;
            } else if matches!(domain, Domain::Voxel | Domain::VoxelUnderside) && let Some(gate) = volume_gate.as_ref() {
                **config = gate.previous;
            }
        }
        let image = captured.image.clone(); let path = path.clone(); let completion = completion.clone();
        let report = report.clone();
        IoTaskPool::get().spawn(async move {
            let result = image.try_into_dynamic().map_err(|error| error.to_string()).and_then(|image| {
                image.to_rgb8().save(&path).map_err(|error| error.to_string())?;
                if let Some(report) = report {
                    std::fs::write(path.with_extension("gpu.json"), serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?)
                        .map_err(|error| error.to_string())?;
                }
                Ok(())
            });
            if let Ok(mut slot) = completion.lock() { *slot = Some(result); }
        }).detach();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_gpu_sample_rejects_late_ticket_replaced_entity_and_mixed_sources() {
        let mut world = World::new(); let entity = world.spawn_empty().id(); let other = world.spawn_empty().id();
        let mut gpu = GpuBladeGpuReadbackStats { sample_sequence: 11, selection_ticket: 5, source_revision: 7,
            sampled_windows: 1, sampled_output_valid_mask: 1, ..default() };
        gpu.sampled_main_entities[0] = entity.to_bits(); gpu.sampled_source_revisions[0] = 7;
        assert!(selected_sample_matches(&gpu, 5, 10, &[entity], 7));
        assert!(!selected_sample_matches(&gpu, 6, 10, &[entity], 7), "切换目标后迟到旧票据不能通过");
        assert!(!selected_sample_matches(&gpu, 5, 11, &[entity], 7), "修改前的完成样本不能通过");
        assert!(!selected_sample_matches(&gpu, 5, 10, &[other], 7), "同源另一实例不能借用计数");
        gpu.sampled_source_revisions[0] = 6;
        assert!(!selected_sample_matches(&gpu, 5, 10, &[entity], 7));
        gpu.sampled_source_revisions[0] = 7; gpu.sampled_output_valid_mask = 0;
        assert!(!selected_sample_matches(&gpu, 5, 10, &[entity], 7), "未采用完整输出不能通过");
    }
}
