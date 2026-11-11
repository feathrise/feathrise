//! 全图取景读取当前驻留内容；有界捕获真实网格包围盒，稀疏地形在单飞后台统计高度。
use super::{sparse, types::*};
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use crate::{
    authoring::{EditorAuthoringSession, WaterNetworkEditState},
    resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState},
    types::EditorViewportFraming,
};
use bevy::{
    camera::{primitives::Aabb, visibility::{NoFrustumCulling, RenderLayers}},
    ecs::entity_disabling::Disabled,
    prelude::*,
    tasks::{AsyncComputeTaskPool, futures::check_ready},
};
use feathrise_scene_content::{SceneTerrainKind, SparseHeightLiveOverride};
use feathrise_scene::{
    ResidentEntityIndex, SubsceneActive, SubsceneInstance, TerrainObjectAuthoringTarget,
    TerrainObjectInstance,
};
use feathrise_scene_core::residency::{InstanceResourceTable, SceneSourceFrame, SceneResidencyActivity};

const VISITS_PER_FRAME: usize = 256;

/// 世界实体和作者辅助内容可取景；天空、天气、资产样品等专用离屏层不是场景尺寸。
fn content_layers(application: Option<&feathrise_editor_core::EditorApplicationViewLayers>) -> RenderLayers {
    use feathrise_scene_core::constants::render_layers::PORTAL_EFFECT_LAYER;
    let layers = RenderLayers::from_layers(&[
        0, PORTAL_EFFECT_LAYER,
        super::super::SCENE_EDITOR_BRUSH_RENDER_LAYER,
    ]);
    if let Some(application) = application { layers.union(&application.framing) } else { layers }
}

fn identity(world: &World) -> Option<Identity> {
    let editor = world.get_resource::<SceneEditorState>()?;
    let session = world.get_resource::<EditorAuthoringSession>()?;
    let draft = session.draft.as_ref()?;
    let view = world.get_resource::<SceneEditorViewportState>()?;
    if !editor.open
        || editor.close_requested
        || editor.new_scene_dialog_open
        || editor.scene_transition_in_progress
        || editor.pending_scene_transition.is_some()
        || editor.scene_switch_confirmation.is_some()
        || world
            .get_resource::<SceneEditorNumericScrubMouseGuard>()
            .is_some_and(|g| g.blocked())
        || world
            .get_resource::<WaterNetworkEditState>()
            .is_some_and(|g| g.dragging())
        || world
            .get_resource::<VegetationPresetEditorState>()
            .is_some_and(|s| s.active_tab != PresetWorkspaceTab::Scene)
        || editor.selected_scene_id.as_ref() != Some(&draft.scene_id)
        || view.framed_scene_id.as_ref() != Some(&draft.scene_id)
    {
        return None;
    }
    let activity = world.get_resource::<SceneResidencyActivity>();
    if activity.is_some_and(|a| a.switching) {
        return None;
    }
    let token = activity.and_then(|a| a.active);
    let index_revision = token
        .zip(world.get_resource::<ResidentEntityIndex>())
        .map_or(0, |(t, index)| index.snapshot(t).0);
    Some(Identity {
        scene_id: draft.scene_id.clone(),
        revision: session.revision,
        terrain_revision: session.terrain_revision,
        token,
        index_revision,
        content_revision: [
            world.get_resource::<feathrise_scene::DynamicSceneRuntimeStatus>()
                .filter(|s| s.scene_id.as_ref() == Some(&draft.scene_id)).map_or(0, |s| s.revision),
            world.get_resource::<feathrise_scene_content::SceneInstanceGraph>()
                .filter(|s| s.root_scene_id.as_ref() == Some(&draft.scene_id)).map_or(0, |s| s.revision),
            world.get_resource::<feathrise_scene::SubsceneRuntimeStatus>()
                .filter(|s| s.root_scene_id.as_ref() == Some(&draft.scene_id)).map_or(0, |s| s.applied_revision),
            world.get_resource::<feathrise_scene::ScenePlacementRuntimeStatus>()
                .filter(|s| s.scene_id.as_ref() == Some(&draft.scene_id)).map_or(0, |s| s.revision),
        ],
        content_layers: content_layers(world.get_resource::<feathrise_editor_core::EditorApplicationViewLayers>()),
        camera: (
            view.center,
            view.extent,
            view.yaw,
            view.pitch,
            view.render_size,
            editor.orthographic,
        ),
    })
}

fn live<'a>(world: &'a World, source: &SparseSource) -> Option<&'a SparseHeightLiveOverride> {
    let active = world
        .get_resource::<TerrainObjectAuthoringTarget>()
        .and_then(|t| t.instance());
    let result = if source.entity.is_none() || source.entity == active {
        world.get_resource::<SparseHeightLiveOverride>()
    } else {
        world
            .get_resource::<InstanceResourceTable<SparseHeightLiveOverride>>()?
            .values
            .get(&source.entity?)
    }?;
    (result.scene_id() == Some(&source.scene_id)).then_some(result)
}

fn begin(world: &World, identity: Identity) -> Scan {
    let mut scan = Scan {
        identity,
        cursor: 0,
        bounds: None,
        sources: Vec::new(),
        capturing: false,
    };
    let draft = world
        .resource::<EditorAuthoringSession>()
        .draft
        .as_ref()
        .unwrap();
    if draft.manifest.terrains.is_none()
        && draft.manifest.terrain.kind == SceneTerrainKind::SparseClipmap
        && let Some(path) = &draft.manifest.terrain.height_path
    {
        scan.sources.push(SparseSource {
            entity: None,
            scene_id: draft.scene_id.clone(),
            path: path.clone(),
            frame: Mat4::IDENTITY,
            live: Default::default(),
            last: None,
        });
        scan.capturing = true;
    }
    scan
}

/// 一个索引/覆盖页只占一次预算。忽略相机可见性裁剪，屏幕外内容仍计入全图。
fn advance(world: &World, scan: &mut Scan) -> bool {
    let layers = &scan.identity.content_layers; // 使用本次捕获票据冻结的集合，不随实体重复分配。
    for _ in 0..VISITS_PER_FRAME {
        if scan.capturing {
            let source = scan.sources.last_mut().unwrap();
            if let Some((key, tile)) =
                live(world, source).and_then(|live| live.tile_after(source.last))
            {
                source.live.insert(key, tile.clone());
                source.last = Some(key);
                continue;
            }
            scan.capturing = false;
        }
        let entity = scan
            .identity
            .token
            .zip(world.get_resource::<ResidentEntityIndex>())
            .and_then(|(token, index)| index.snapshot(token).1.get(scan.cursor))
            .copied();
        let Some(entity) = entity else {
            return true;
        };
        scan.cursor += 1;
        if world.get::<Disabled>(entity).is_some()
            || world.get::<feathrise_environment::terrain_surface::TerrainSurfaceWarmup>(entity).is_some()
            || world.get::<RenderLayers>(entity).is_some_and(|entity_layers| !entity_layers.intersects(layers))
            || world
                .get::<InheritedVisibility>(entity)
                .is_some_and(|v| !v.get())
            || world.get::<Visibility>(entity) == Some(&Visibility::Hidden)
        {
            continue;
        }
        // GLB根先出现，网格后代随后才发布；等正式World票据完成后再继续有界遍历。
        if world
            .get::<bevy::world_serialization::WorldAssetRoot>(entity)
            .is_some()
            && !world
                .get::<bevy::world_serialization::WorldInstance>(entity)
                .is_some_and(|instance| {
                    world
                        .get_resource::<bevy::world_serialization::WorldInstanceSpawner>()
                        .is_some_and(|spawner| spawner.instance_is_ready(**instance))
                })
        {
            scan.cursor -= 1;
            return false;
        }
        if let (Some(object), Some(instance), Some(frame)) = (
            world.get::<TerrainObjectInstance>(entity),
            world.get::<SubsceneInstance>(entity),
            world.get::<SceneSourceFrame>(entity),
        ) {
            if world.get::<SubsceneActive>(instance.batch).is_some()
                && object.definition.enabled
                && object.definition.terrain.kind == SceneTerrainKind::SparseClipmap
                && let Some(path) = &object.definition.terrain.height_path
            {
                scan.sources.push(SparseSource {
                    entity: Some(entity),
                    scene_id: instance.source_scene_id.clone(),
                    path: path.clone(),
                    frame: frame.source_to_world,
                    live: Default::default(),
                    last: None,
                });
                scan.capturing = true;
            }
        } else if let (Some(instance), Some(frame)) = (
            world.get::<SubsceneInstance>(entity),
            world.get::<SceneSourceFrame>(entity),
        ) {
            // 子场景的旧单地形没有TerrainObjectInstance，仍按它自己的来源和完整姿态取景。
            if !instance.path.instances.is_empty()
                && world.get::<SubsceneActive>(instance.batch).is_some()
            {
                let manifest = world
                    .get_resource::<feathrise_scene_content::SceneContentRegistry>()
                    .and_then(|registry| registry.handles(&instance.source_scene_id))
                    .and_then(|handles| {
                        world
                            .get_resource::<Assets<feathrise_scene_content::SceneManifestAsset>>()?
                            .get(&handles.manifest)
                    });
                if let Some(manifest) = manifest.filter(|m| {
                    m.terrains.is_none() && m.terrain.kind == SceneTerrainKind::SparseClipmap
                }) && let Some(path) = &manifest.terrain.height_path
                {
                    scan.sources.push(SparseSource {
                        entity: Some(entity),
                        scene_id: instance.source_scene_id.clone(),
                        path: path.clone(),
                        frame: frame.source_to_world,
                        live: Default::default(),
                        last: None,
                    });
                    scan.capturing = true;
                }
            }
        }
        if let Some(surface) = world.get::<feathrise_scene::ClipmapWaterSurface>(entity) {
            if surface.is_published() {
                let matrix = world
                    .get::<MeshMaterial3d<feathrise_scene::ClipmapWaterMaterial>>(entity)
                    .and_then(|material| {
                        world
                            .get_resource::<Assets<feathrise_scene::ClipmapWaterMaterial>>()?
                            .get(&material.0)
                    })
                    .map_or(Mat4::IDENTITY, |material| material.extension.source_frame());
                let (min, max) = surface.world_coverage_bounds();
                Bounds::merge(
                    &mut scan.bounds,
                    Bounds::transformed(
                        Vec3::new(min.x, surface.water_level, min.y),
                        Vec3::new(max.x, surface.water_level, max.y),
                        matrix,
                    ),
                );
            }
            continue; // 水面的共享环带AABB不参与取景。
        }
        // GPU环带、天气及辅助覆盖不具有真实CPU AABB；Clipmap由上面的作者源统计。
        if world
            .get::<feathrise_scene::DynamicSparseClipmapPrimitive>(entity)
            .is_some()
            // GPU草窗的AABB覆盖相机附近的整档候选区，可能远大于真实种植范围。
            // 实际承托地形已经参与取景，不能再把随相机移动的窗口当成场景内容。
            || world.get::<feathrise_environment::vegetation::blade::GpuBladePatch>(entity).is_some()
            || (world.get::<NoFrustumCulling>(entity).is_some()
                && world.get::<Aabb>(entity).is_none())
            || world.get::<Mesh3d>(entity).is_none()
        {
            continue;
        }
        if let (Some(aabb), Some(transform)) = (
            world.get::<Aabb>(entity),
            world.get::<GlobalTransform>(entity),
        ) {
            Bounds::merge(
                &mut scan.bounds,
                Bounds::transformed(aabb.min().into(), aabb.max().into(), transform.to_matrix()),
            );
        } else if world
            .get::<bevy::camera::visibility::NoAutoAabb>(entity)
            .is_none()
        {
            scan.cursor -= 1;
            return false; // 异步GLB尚未发布包围盒；不能将“未就绪”误当空场景采用。
        }
    }
    false
}

/// 排在普通聚焦分派前消费Map请求；新导航和切换场景立即撤销信用，后台槽完成后再接下一次。
pub(crate) fn frame_content(world: &mut World, mut state: Local<ContentFraming>) {
    let request = world
        .resource::<SceneEditorState>()
        .viewport_framing_requested;
    let scene_request = matches!(
        request,
        Some(EditorViewportFraming::Scene | EditorViewportFraming::RefreshDepth)
    );
    if !scene_request && !state.requested && state.task.is_none() && state.scan.is_none() {
        return;
    }
    if scene_request {
        world
            .resource_mut::<SceneEditorState>()
            .viewport_framing_requested = None;
        for_cancel_focus(world);
        state.requested = true;
        state.refocus = request == Some(EditorViewportFraming::Scene);
    } else if request.is_some() {
        state.requested = false;
    }
    let current = identity(world);
    if current.is_none() {
        state.requested = false;
    }
    let previous = state
        .scan
        .as_ref()
        .map(|s| &s.identity)
        .or(state.running.as_ref());
    if let (Some(old), Some(new)) = (previous, current.as_ref()) {
        if old.scene_id != new.scene_id || old.camera != new.camera {
            state.requested = scene_request;
        }
    }
    if let Some(result) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        let valid = state.requested && state.running.as_ref() == current.as_ref();
        state.running = None;
        if valid {
            state.requested = false;
            match result {
                Ok(bounds) => apply(world, bounds, state.refocus),
                Err(error) => {
                    world.resource_mut::<SceneEditorState>().status =
                        format!("查看全图未完成：{error}")
                }
            }
        }
    }
    if state
        .scan
        .as_ref()
        .is_some_and(|s| !state.requested || Some(&s.identity) != current.as_ref())
    {
        let retired = state.scan.take();
        // 覆盖Arc和作者列表可能很大；失效捕获同样在唯一后台槽释放。
        state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
            drop(retired);
            Ok(None)
        }));
        state.running = None;
    }
    if !state.requested || state.task.is_some() {
        return;
    }
    if world
        .get_resource::<SceneResidencyActivity>()
        .is_some_and(|a| a.enabled)
        && world
            .get_resource::<ResidentEntityIndex>()
            .is_some_and(|i| i.registration_pending())
    {
        return; // 每帧登记预算独立推进，不能采用只有根节点的半份场景。
    }
    if world
        .get_resource::<feathrise_scene::DynamicSceneRuntimeStatus>()
        .is_some_and(|s| {
            current
                .as_ref()
                .is_some_and(|id| s.scene_id.as_ref() == Some(&id.scene_id))
                && matches!(
                    s.phase,
                    feathrise_scene::DynamicSceneRuntimePhase::Waiting
                        | feathrise_scene::DynamicSceneRuntimePhase::BuildingCollision
                )
        })
        || world
            .get_resource::<feathrise_scene::SubsceneRuntimeStatus>()
            .is_some_and(|s| {
                current
                    .as_ref()
                    .is_some_and(|id| s.root_scene_id.as_ref() == Some(&id.scene_id))
                    && (!s.ready || world.get_resource::<feathrise_scene_content::SceneInstanceGraph>()
                        .is_some_and(|graph| graph.root_scene_id == s.root_scene_id && graph.revision != s.applied_revision))
                    && s.last_error.is_none()
            })
    {
        return;
    } // 首次加载等待完整活动内容，不能把候选根或半份子场景取景。
    if state.scan.is_none() {
        state.scan = current.map(|id| begin(world, id));
        world.resource_mut::<SceneEditorState>().status = "正在按实际内容计算全图范围…".into();
    }
    let Some(scan) = state.scan.as_mut() else {
        return;
    };
    if !advance(world, scan) {
        return;
    }
    let scan = state.scan.take().unwrap();
    if scan.sources.is_empty() {
        state.requested = false;
        apply(world, scan.bounds, state.refocus);
        return;
    }
    state.running = Some(scan.identity);
    let root = crate::authoring::workspace_asset_root();
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let mut bounds = scan.bounds;
        for source in scan.sources {
            Bounds::merge(&mut bounds, sparse::bounds(&root, source)?);
        }
        Ok(bounds)
    }));
}

fn for_cancel_focus(world: &mut World) {
    if let Some(mut s) = world.get_resource_mut::<super::super::SelectedPlacementViewportState>() {
        s.cancel_focus();
    }
    if let Some(mut s) = world.get_resource_mut::<super::super::SelectedWaterViewportState>() {
        s.cancel_focus();
    }
    if let Some(mut s) = world.get_resource_mut::<super::super::SelectedPortalViewportState>() {
        s.cancel_focus();
    }
}

fn apply(world: &mut World, bounds: Option<Bounds>, refocus: bool) {
    let orthographic = world.resource::<SceneEditorState>().orthographic;
    let Some(mut view) = world.get_resource_mut::<SceneEditorViewportState>() else {
        return;
    };
    let bounds = bounds.unwrap_or(Bounds {
        min: Vec3::new(-16.0, 0.0, -16.0),
        max: Vec3::new(16.0, 0.0, 16.0),
    });
    let center = (bounds.min + bounds.max) * 0.5;
    let half = (bounds.max - bounds.min) * 0.5;
    let extent = super::super::camera_math::framing_extent(
        half,
        view.yaw,
        view.pitch,
        view.render_size,
        orthographic,
    );
    if !center.is_finite() || !half.is_finite() || !extent.is_finite() {
        return;
    }
    view.scene_center = center;
    view.scene_half_extents = half;
    if refocus {
        view.center = center;
        view.extent = extent;
        world.resource_mut::<SceneEditorState>().status = "已按实际内容查看全图".into();
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
