//! RenderApp在绘制提交后确认实际GPU资产与已编译管线，不按固定等待帧盲拍。

use super::components::ThumbnailProbe;
use bevy::core_pipeline::{
    core_3d::{AlphaMask3d, Opaque3d, Transparent3d},
    deferred::{AlphaMask3dDeferred, Opaque3dDeferred},
};
use bevy::material::OpaqueRendererMethod;
use bevy::pbr::{
    PreparedMaterial, RenderMaterialInstances, SpecializedMaterialPipelineCache,
    SpecializedPrepassMaterialPipelineCache, Transmissive3d, deferred::DeferredLightingPipeline,
};
use bevy::prelude::*;
use bevy::render::render_phase::{
    BinnedPhaseItem, BinnedRenderPhase, PhaseItem, ViewBinnedRenderPhases, ViewSortedRenderPhases,
};
use bevy::render::{
    Render, RenderApp, RenderSystems,
    erased_render_asset::ErasedRenderAssets,
    extract_component::ExtractComponentPlugin,
    mesh::RenderMesh,
    render_asset::RenderAssets,
    render_resource::{CachedPipelineState, PipelineCache},
    sync_world::MainEntity,
    texture::GpuImage,
};
use std::sync::atomic::Ordering;

pub(super) fn install(app: &mut App) {
    app.add_plugins(ExtractComponentPlugin::<ThumbnailProbe>::default());
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render.add_systems(Render, observe_render.in_set(RenderSystems::Cleanup));
    } // 逻辑测试没有RenderApp时不会虚构GPU成功。
}

#[allow(clippy::too_many_arguments)] // 六种正式绘制阶段只读取唯一256px相机的至多256网格。
fn observe_render(
    probes: Query<(&MainEntity, &ThumbnailProbe)>,
    images: Res<RenderAssets<GpuImage>>,
    meshes: Res<RenderAssets<RenderMesh>>,
    materials: Res<ErasedRenderAssets<PreparedMaterial>>,
    instances: Res<RenderMaterialInstances>,
    pipelines: Res<SpecializedMaterialPipelineCache>,
    prepass: Res<SpecializedPrepassMaterialPipelineCache>,
    cache: Res<PipelineCache>,
    lighting: Query<(&MainEntity, &DeferredLightingPipeline)>,
    opaque: Res<ViewBinnedRenderPhases<Opaque3d>>,
    masked: Res<ViewBinnedRenderPhases<AlphaMask3d>>,
    deferred: Res<ViewBinnedRenderPhases<Opaque3dDeferred>>,
    deferred_masked: Res<ViewBinnedRenderPhases<AlphaMask3dDeferred>>,
    transparent: Res<ViewSortedRenderPhases<Transparent3d>>,
    transmissive: Res<ViewSortedRenderPhases<Transmissive3d>>,
) {
    for (entity, probe) in &probes {
        if probe.receipt.ready.load(Ordering::Acquire) {
            continue;
        }
        let material = materials.get(probe.material.id().untyped());
        let mut complete = images.get(&probe.target).is_some()
            && meshes.get(&probe.mesh).is_some()
            && material.is_some()
            && instances
                .instances
                .get(entity)
                .is_some_and(|instance| instance.asset_id == probe.material.id().untyped());
        let mut found = false;
        let use_deferred = material.is_some_and(|material| {
            material.properties.render_method == OpaqueRendererMethod::Deferred
                && matches!(
                    material.properties.alpha_mode,
                    AlphaMode::Opaque | AlphaMode::Mask(_)
                )
        });
        let mut queued = false;
        for (key, view) in pipelines
            .iter()
            .filter(|(key, _)| key.main_entity.id() == probe.camera)
        {
            if let Some(id) = view.get(entity) {
                if !use_deferred {
                    found = true;
                    complete &= compiled(&cache, *id, probe);
                }
            }
            queued |= opaque
                .get(key)
                .is_some_and(|phase| binned_contains(phase, *entity))
                || masked
                    .get(key)
                    .is_some_and(|phase| binned_contains(phase, *entity))
                || transparent.get(key).is_some_and(|phase| {
                    phase
                        .items
                        .values()
                        .any(|item| item.main_entity() == *entity)
                })
                || transmissive.get(key).is_some_and(|phase| {
                    phase
                        .items
                        .values()
                        .any(|item| item.main_entity() == *entity)
                });
        }
        if use_deferred {
            for (key, view) in prepass
                .iter()
                .filter(|(key, _)| key.main_entity.id() == probe.camera)
            {
                if let Some((_, id, _)) = view.get(entity) {
                    found = true;
                    complete &= compiled(&cache, *id, probe);
                    queued |= deferred
                        .get(key)
                        .is_some_and(|phase| binned_contains(phase, *entity))
                        || deferred_masked
                            .get(key)
                            .is_some_and(|phase| binned_contains(phase, *entity));
                }
            }
            complete &= lighting
                .iter()
                .find(|(entity, _)| entity.id() == probe.camera)
                .is_some_and(|(_, pipeline)| compiled(&cache, pipeline.pipeline_id, probe));
        }
        if complete && found && queued {
            let observed = probe.receipt.frames.fetch_add(1, Ordering::AcqRel);
            if observed >= 1 {
                probe.receipt.ready.store(true, Ordering::Release);
            }
        } else {
            probe.receipt.frames.store(0, Ordering::Release);
        }
    }
}

fn compiled(
    cache: &PipelineCache,
    id: bevy::render::render_resource::CachedRenderPipelineId,
    probe: &ThumbnailProbe,
) -> bool {
    match cache.get_render_pipeline_state(id) {
        CachedPipelineState::Ok(_) => true,
        CachedPipelineState::Err(_) => {
            probe.receipt.failed.store(true, Ordering::Release);
            false
        }
        _ => false,
    }
}

fn binned_contains<P: BinnedPhaseItem>(phase: &BinnedRenderPhase<P>, entity: MainEntity) -> bool {
    // 相机显式NoIndirectDrawing，不依赖私有multidraw缓存；检查真实实体的当前绘制队列。
    phase
        .batchable_meshes
        .values()
        .any(|bin| bin.entities().contains_key(&entity))
        || phase
            .unbatchable_meshes
            .values()
            .any(|bin| bin.entities.contains_key(&entity))
        || phase
            .non_mesh_items
            .values()
            .any(|bin| bin.entities.contains_key(&entity))
}
