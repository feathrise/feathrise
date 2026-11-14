//! 原生体素截图以实际三维worker采用代、字段与精确草窗取得GPU信用。

use super::super::{resources::GeometryFixture,
    types::{Domain, VolumeCaptureIdentity, VolumeFieldIdentity, VolumeWindowIdentity}};
pub(super) use super::super::resources::VolumeGpuGate;
use bevy::{ecs::system::SystemState, prelude::*};
use feathrise_environment::vegetation::{VegetationBladeLayer, blade::{
    GpuBladeDensityMap, GpuBladeGpuReadbackStats, GpuBladePatch, GpuBladeReadbackConfig,
    GpuBladeRenderStats, GpuBladeSourceRevision, GpuBladeVolumeSource,
}};
use feathrise_scene::{DynamicBladeDraft, DynamicVegetationPhase, DynamicVegetationStatus,
    TerrainObjectQueries, TerrainObjectResource, VolumeSurfaceFields};
use std::{sync::Arc, time::{Duration, Instant}};

pub(super) fn ready(world: &mut World, state: &mut GeometryFixture) -> Result<bool, String> {
    let scene = state.scene.clone().ok_or("体素GPU截图场景身份缺失")?;
    let terrain = state.target().cloned().ok_or("体素GPU截图地形身份缺失")?;
    let layer = match state.domain {
        Domain::Voxel => state.proof.vegetation_output_layer.as_ref(),
        Domain::VoxelUnderside => state.proof.underside_vegetation_output_layer.as_ref(),
        _ => return Err("体素GPU门不能验证其他截图目标".into()),
    }.cloned().ok_or("体素GPU截图缺少真实种植层")?;
    let mut access = SystemState::<(TerrainObjectResource<DynamicVegetationStatus>, TerrainObjectQueries)>::new(world);
    let Ok((statuses, queries)) = access.get(world) else { return Ok(false); };
    let Some((instance, status)) = statuses.for_saved_object(&scene, &terrain) else { return Ok(false); };
    let Some((active, _, frame)) = queries.instance_source(&scene, &terrain) else { return Ok(false); };
    if instance != active { return Ok(false); }
    let runtime = queries.voxel_diagnostic_snapshot(&scene, &terrain).map(|(_, _, value)| value);
    let status = status.clone();
    let (author_revision, density_revision, runtime_layer, authors_ready) = {
        let session = world.resource::<crate::authoring::EditorAuthoringSession>();
        let live = world.get_resource::<DynamicBladeDraft>();
        let snapshot = live.and_then(|live| live.for_scene(&scene));
        let density = live.and_then(|live| live.revision_for_scene(&scene));
        let layout = live.and_then(|live| live.layout_revision_for_scene(&scene));
        let runtime_layer = snapshot.and_then(|snapshot| snapshot.definition.layers.iter()
            .find(|definition| definition.id == layer)).and_then(|definition| definition.runtime_layer_id);
        let ready = session.vegetation_compiled_revision == session.vegetation_revision
            && session.vegetation_compiled_structure_revision == session.vegetation_structure_revision
            && snapshot.is_some_and(|snapshot| snapshot.source_revision == session.vegetation_revision)
            && status.scene_id.as_ref() == Some(&scene) && status.phase == DynamicVegetationPhase::Ready
            && status.applied_draft_revision == layout && status.applied_density_revision == density
            && density.is_some() && layout.is_some() && runtime_layer.is_some();
        (session.vegetation_revision, density.unwrap_or(0), runtime_layer, ready)
    };
    let mut fields = Vec::new();
    for (entity, field) in world.query::<(Entity, &VolumeSurfaceFields)>().iter(world) {
        if super::visible_descendant(world, entity, instance) {
            fields.push(VolumeFieldIdentity { entity, snapshot: Arc::as_ptr(&field.0) as usize,
                geometry: field.0.geometry_identity(), cell_size: field.0.cell_size,
                primary_samples: field.0.samples.len(), additional_cells: field.0.additional_samples.len() });
        }
    }
    fields.sort_unstable_by_key(|field| field.entity.to_bits());
    let mut adopted_fields = status.volume_fields.clone();
    adopted_fields.sort_unstable_by_key(|(entity, _)| entity.to_bits());
    let current_fields: Vec<_> = fields.iter().map(|field| (field.entity, field.snapshot)).collect();
    let source = status.volume_sources.get(&layer).cloned();
    let source_count = status.volume_source_counts.get(&layer).copied().unwrap_or(0);
    let positive_source_count = status.volume_source_positive_counts.get(&layer).copied().unwrap_or(0);
    let expected_windows = status.volume_window_counts.get(&layer).copied().unwrap_or(0);
    let mut windows = Vec::new();
    let mut visible_windows = 0;
    let mut bindings_complete = true;
    for (entity, binding, volume) in world.query::<(Entity, &VegetationBladeLayer, &GpuBladeVolumeSource)>().iter(world) {
        if Some(binding.0.0) != runtime_layer || !status.volume_root
            .is_some_and(|root| super::visible_descendant(world, entity, root)) { continue; }
        let (Some(density), Some(revision), Some(patch), Some(local), Some(global)) = (
            world.get::<GpuBladeDensityMap>(entity), world.get::<GpuBladeSourceRevision>(entity),
            world.get::<GpuBladePatch>(entity), world.get::<Transform>(entity), world.get::<GlobalTransform>(entity),
        ) else { bindings_complete = false; continue; };
        let source_to_world = global.to_matrix() * local.to_matrix().inverse();
        bindings_complete &= revision.0 == status.volume_generation && source.as_ref() == Some(&volume.0)
            && patch.instance_count() > 0 && source_to_world.abs_diff_eq(frame.source_to_world, 0.001);
        if world.get::<ViewVisibility>(entity).is_some_and(|visibility| visibility.get()) { visible_windows += 1; }
        windows.push(VolumeWindowIdentity { entity, source_revision: revision.0, source: volume.0.clone(),
            density: density.clone(), source_to_world: source_to_world.to_cols_array(), lod: patch.lod_level(),
            candidates: patch.instance_count(), origin: patch.origin(), focus: patch.focus() });
    }
    windows.sort_unstable_by_key(|window| window.entity.to_bits());
    let mut lods: Vec<_> = windows.iter().map(|window| window.lod).collect();
    lods.sort_unstable(); lods.dedup();
    let cpu_ready = authors_ready && status.volume_ready && status.volume_generation != 0
        && status.volume_root.is_some_and(|root| super::visible_descendant(world, root, instance))
        && !fields.is_empty() && current_fields == adopted_fields && source.is_some()
        && positive_source_count > 0 && positive_source_count <= source_count
        && expected_windows > 0 && windows.len() == expected_windows && lods.len() == expected_windows && bindings_complete;
    let camera = world.resource::<crate::resources::SceneEditorViewportState>().camera;
    let Some(camera_pose) = world.get::<GlobalTransform>(camera).map(|value| value.to_matrix().to_cols_array()) else { return Ok(false); };
    let Some(projection) = world.get::<Camera>(camera).map(|value| value.clip_from_view().to_cols_array()) else { return Ok(false); };
    gpu_ready(world, state, VolumeCaptureIdentity { domain: state.domain, scene: scene.as_str().into(),
        terrain: terrain.as_str().into(), layer, instance, author_revision, density_revision,
        generation: status.volume_generation, root: status.volume_root, adopted_fields, fields,
        ground: status.volume_ground_revision, source, source_count, positive_source_count, expected_windows,
        camera, camera_pose, projection, windows, cpu_ready }, visible_windows, runtime)
}

fn gpu_ready(world: &mut World, state: &mut GeometryFixture, identity: VolumeCaptureIdentity,
    visible_windows: usize, runtime: Option<serde_json::Value>) -> Result<bool, String> {
    let gpu = world.get_resource::<GpuBladeGpuReadbackStats>().copied().unwrap_or_default();
    let render = world.get_resource::<GpuBladeRenderStats>().copied().unwrap_or_default();
    let config = world.get_resource::<GpuBladeReadbackConfig>().copied().ok_or("真实GPU回读插件未安装")?;
    let mut gate = world.remove_resource::<VolumeGpuGate>().unwrap_or_else(|| VolumeGpuGate {
        identity: identity.clone(), ticket: config.selection_ticket().wrapping_add(1).max(1), baseline: gpu.sample_sequence,
        started: Instant::now(), last_diagnostic: None, was_ready: false, previous: config,
    });
    if gate.identity != identity {
        gate.identity = identity; gate.ticket = gate.ticket.wrapping_add(1).max(1);
        gate.baseline = gpu.sample_sequence; gate.started = Instant::now(); gate.last_diagnostic = None; gate.was_ready = false;
    }
    let entities: Vec<_> = gate.identity.windows.iter().map(|window| window.entity).collect();
    if !entities.is_empty() {
        world.insert_resource(GpuBladeReadbackConfig::for_windows(1, gate.ticket, &entities)
            .ok_or("体素截图GPU目标超过固定32窗或含重复身份")?);
    }
    let current_sample = super::selected_sample_matches(&gpu, gate.ticket, gate.baseline,
        &entities, gate.identity.generation);
    let ready = gate.identity.cpu_ready && current_sample && gpu.rendered_instances > 0
        && gpu.surface_survivors > 0 && gpu.density_survivors > 0 && gpu.scene_errors == 0 && gpu.overflow_instances == 0
        && visible_windows > 0 && render.target_views > 0 && render.prepared_bind_groups >= entities.len() as u32
        && render.compute_pipeline_ready_stages == 4 && render.compute_pipeline_waiting_stages == 0
        && render.compute_pipeline_error_stages == 0 && render.pipeline_error_draws == 0
        && render.pipeline_waiting_draws == 0 && render.pipeline_ready_draws > 0 && render.drawn_draws > 0;
    let timeout = gate.started.elapsed() > Duration::from_secs(60);
    let print = (ready && !gate.was_ready) || (!ready && (timeout
        || gate.last_diagnostic.is_none_or(|last| last.elapsed() >= Duration::from_secs(2))));
    if ready || print {
        let diagnostic = diagnostic(&gate, &gpu, &render, visible_windows, runtime, current_sample, ready);
        if print { eprintln!("体素目标植被实际GPU等待：{diagnostic}"); gate.last_diagnostic = Some(Instant::now()); }
        match state.domain {
            Domain::Voxel => state.proof.voxel_gpu = Some(diagnostic.clone()),
            Domain::VoxelUnderside => state.proof.voxel_underside_gpu = Some(diagnostic.clone()),
            _ => {}
        }
        if timeout && !ready {
            world.insert_resource(gate);
            return Err(format!("体素同代目标GPU绘制六十秒未就绪：{diagnostic}"));
        }
    }
    gate.was_ready = ready;
    world.insert_resource(gate);
    Ok(ready)
}

fn diagnostic(gate: &VolumeGpuGate, gpu: &GpuBladeGpuReadbackStats, render: &GpuBladeRenderStats,
    visible_windows: usize, runtime: Option<serde_json::Value>, current_sample: bool, ready: bool) -> serde_json::Value {
    use serde_json::json;
    let identity = &gate.identity;
    let count = (gpu.sampled_windows as usize).min(gpu.sampled_main_entities.len());
    json!({"domain":format!("{:?}",identity.domain),"scene":identity.scene,"terrain":identity.terrain,
        "layer":identity.layer,"instance":identity.instance.to_bits(),"author_revision":identity.author_revision,
        "density_revision":identity.density_revision,"volume_generation":identity.generation,
        "cpu_ready":identity.cpu_ready,"ready_for_capture":ready,"current_sample":current_sample,
        "ticket":gate.ticket,"sample_baseline":gate.baseline,"wait_seconds":gate.started.elapsed().as_secs_f32(),
        "native_runtime":runtime,"worker":{"root":identity.root.map(Entity::to_bits),
            "ground":identity.ground,"source":format!("{:?}",identity.source),"source_samples":identity.source_count,
            "positive_source_samples":identity.positive_source_count,
            "expected_windows":identity.expected_windows,"adopted_fields":identity.adopted_fields.iter()
                .map(|(entity,snapshot)|json!({"entity":entity.to_bits(),"snapshot":snapshot})).collect::<Vec<_>>()},
        "fields":identity.fields.iter().map(|field|json!({"entity":field.entity.to_bits(),"snapshot":field.snapshot,
            "geometry":field.geometry,"cell_size":field.cell_size,"primary_samples":field.primary_samples,
            "additional_cells":field.additional_cells})).collect::<Vec<_>>(),
        "camera":{"entity":identity.camera.to_bits(),"pose":identity.camera_pose,"projection":identity.projection},
        "windows":identity.windows.iter().map(|window|json!({"entity":window.entity.to_bits(),"lod":window.lod,
            "source_revision":window.source_revision,"source":format!("{:?}",window.source),
            "density":format!("{:?}",window.density),"source_to_world":window.source_to_world,
            "candidates":window.candidates,"origin":window.origin.to_array(),"focus":window.focus.to_array()})).collect::<Vec<_>>(),
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
    if let Some(gate) = world.remove_resource::<VolumeGpuGate>() { world.insert_resource(gate.previous); }
}
