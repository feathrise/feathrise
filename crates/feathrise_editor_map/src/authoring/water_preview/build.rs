//! 水域预览联合修订的 single-flight 后台构建与样条预算校验。

use bevy::asset::LoadState; // 微法线必须完成异步加载后才能提交GPU候选。
use bevy::prelude::*;
use bevy::tasks::AsyncComputeTaskPool;
use feathrise_map_content::{MapTerrainKind, WaterPresetRegistry}; // Clipmap水网绕开GLB整图高度，静水外观仍解析同一正式目录。
use feathrise_scene::{
    BuiltWaterSurface, DynamicClipmapWaterDraft, DynamicWaterPhase, DynamicWaterStatus,
    chunk_water_surfaces, PaintedWaterTerrainInput, WaterSurfaceDescriptor,
    build_painted_water_mesh_with_depth, build_river_water_surface, build_waterfall_water_surface,
};

use super::super::resources::{EditorAuthoringSession, WaterBrushStroke, WaterPreviewState};
use super::super::water_network::validate_water_network_edit;
use crate::resources::MapEditorState;

/// 编辑期只构建已经拥有至少两个点的样条；空路径和单点路径是合法的逐点绘制中间态。
pub(super) fn append_completed_spline_builds(
    water: &feathrise_map_content::MapWaterAsset,
    builds: &mut Vec<BuiltWaterSurface>,
) -> Result<(), String> {
    let mut triangle_count = 0usize;
    for river in water.rivers.iter().filter(|river| river.points.len() >= 2) {
        let build = build_river_water_surface(river)?;
        triangle_count = triangle_count
            .checked_add(build.mesh.triangle_count())
            .ok_or_else(|| "地图样条水三角形总数溢出".to_owned())?;
        builds.push(build);
    }
    for waterfall in water
        .waterfalls
        .iter()
        .filter(|waterfall| waterfall.points.len() >= 2)
    {
        let build = build_waterfall_water_surface(waterfall)?;
        triangle_count = triangle_count
            .checked_add(build.mesh.triangle_count())
            .ok_or_else(|| "地图样条水三角形总数溢出".to_owned())?;
        builds.push(build);
    }
    Ok(())
}

/// 当前联合修订没有在途、已采用或已失败结果时，提交一次全层后台构建。
pub(crate) fn start_water_preview_rebuild(
    mut editor: ResMut<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    stroke: Res<WaterBrushStroke>,
    asset_server: Res<AssetServer>,
    water_presets: Option<Res<WaterPresetRegistry>>,
    clipmap_draft: Res<DynamicClipmapWaterDraft>,
    runtime_status: feathrise_scene::TerrainObjectResource<DynamicWaterStatus>,
    live: Option<Res<super::super::live_water_dynamic::LiveClipmapWaterState>>,
    mut preview: ResMut<WaterPreviewState>,
) {
    if !editor.open || stroke.active || editor.undo_requested || editor.redo_requested {
        return;
    }
    let Some(draft) = session
        .draft
        .as_ref()
        .filter(|draft| editor.selected_map_id.as_ref() == Some(&draft.map_id))
    else {
        return;
    };

    let map_id = draft.map_id.clone();
    let water_revision = session.water_revision;
    let terrain_revision = session.terrain_revision;
    let preset_revision = water_presets.as_deref().map_or(0, |value| value.revision); // 只有已保存正式目录换代才重建当前地图水网。
    preview.desired_map_id = Some(map_id.clone());
    preview.desired_water_revision = water_revision;
    preview.desired_terrain_revision = terrain_revision;
    preview.desired_preset_revision = preset_revision;
    preview.waiting_detail = None;
    let independent_root = draft.terrain_documents.active.is_none() && draft.manifest.terrain.is_empty();
    if draft.terrain_manifest().terrain.kind == MapTerrainKind::SparseClipmap || independent_root {
        // Clipmap作者预览就是正式共享环带后端；这里只把其原子采用证据映射到既有保存门。
        preview.applied_map_id = None; // 只撤销当前Ready信用；旧有效水面的实体/图片仍由正式运行时保留。
        let runtime_status = if independent_root {
            runtime_status.for_root(&map_id)
        } else if draft.manifest.terrains.is_some() {
            draft.terrain_documents.active.as_ref().and_then(|canvas|
                runtime_status.for_object(&map_id, &canvas.definition.id))
        } else {
            Some(&*runtime_status)
        }; // 容器根与其他网格的同号修订都不能替当前画布提供Ready信用。
        if let Some(detail) = live.as_deref().and_then(|live| if independent_root {
            live.root_failure_for(&map_id, water_revision)
        } else { live.failure_for(&map_id, water_revision) }) {
            if !preview.failed_for(&map_id, water_revision, terrain_revision) {
                editor.status = format!("Clipmap水网预览失败：{detail}");
            }
            preview.failed_map_id = Some(map_id);
            preview.failed_water_revision = water_revision;
            preview.failed_terrain_revision = terrain_revision;
            preview.failed_preset_revision = preset_revision;
            return;
        }
        let Some(runtime_status) = runtime_status else {
            preview.waiting_detail = Some("正在等待当前水域来源的运行状态与作者画布完成交接".into());
            return;
        };
        let published = if independent_root { clipmap_draft.root_for_map(&map_id) } else { clipmap_draft.for_map(&map_id) };
        let draft_revision = if independent_root { clipmap_draft.root_revision_for_map(&map_id) } else { clipmap_draft.revision_for_map(&map_id) };
        let same_author_revision =
            published.is_some_and(|snapshot| snapshot.source_revision == water_revision);
        let same_runtime_batch = runtime_status.map_id.as_ref() == Some(&map_id)
            && runtime_status.input_draft_revision == draft_revision;
        if same_author_revision
            && same_runtime_batch
            && runtime_status.phase == DynamicWaterPhase::Ready
            && runtime_status.desired_generation == runtime_status.applied_generation
        {
            preview.applied_map_id = Some(map_id);
            preview.applied_water_revision = water_revision;
            preview.applied_terrain_revision = terrain_revision;
            preview.applied_preset_revision = preset_revision;
            preview.failed_map_id = None;
            preview.failed_water_revision = 0;
            preview.failed_terrain_revision = 0;
            preview.failed_preset_revision = 0;
        } else if same_author_revision
            && same_runtime_batch
            && runtime_status.phase == DynamicWaterPhase::Failed
        {
            if !preview.failed_for(&map_id, water_revision, terrain_revision) {
                editor.status = format!("Clipmap水网预览失败：{}", runtime_status.detail);
            } // 失败只核对输入身份；旧有效applied_generation本来就不会等于失败的新一代。
            preview.failed_map_id = Some(map_id.clone());
            preview.failed_water_revision = water_revision;
            preview.failed_terrain_revision = terrain_revision;
            preview.failed_preset_revision = preset_revision;
        } else {
            // 【保存设计】保留真正缺失的信用，底栏直接显示发布、实例采用或GPU准备的具体卡点。
            preview.waiting_detail = Some(if !same_author_revision {
                format!("正在准备当前静水作者修订{water_revision}的完整遮罩与水位")
            } else if !same_runtime_batch {
                format!("正在等待当前水域来源采用静水作者修订{water_revision}（发布代次{draft_revision:?}，运行时代次{:?}）", runtime_status.input_draft_revision)
            } else if !runtime_status.detail.is_empty() {
                runtime_status.detail.clone()
            } else {
                format!("正在等待Clipmap静水完整采用（{:?}，已采用{}，期望{}）", runtime_status.phase,
                    runtime_status.applied_generation, runtime_status.desired_generation)
            });
        }
        return; // 禁止等待GLB派生整图高度或为每层重建CPU水面网格。
    }
    if preview.task.is_some()
        || preview.pending.is_some()
        || preview.ready_for(&map_id, water_revision, terrain_revision)
        || preview.failed_for(&map_id, water_revision, terrain_revision)
    {
        return;
    }
    let Some(source) = super::suppression::current_source(&session) else { return; };

    let water = draft.water.clone();
    let mut layer_inputs = Ok(Vec::new());
    if let Some(water) = water.as_ref() {
        for layer in &water.layers {
            let mask = match draft.water_masks.get(&layer.id).cloned() {
                Some(mask) => mask,
                None => {
                    layer_inputs = Err(format!("静水体{}缺少R8作者源", layer.id));
                    break;
                }
            };
            let descriptor = if layer.preset_id.is_empty() {
                WaterSurfaceDescriptor::from_layer(layer)
            } else {
                let Some(registry) = water_presets.as_deref().filter(|value| value.revision > 0)
                else {
                    return; // 目录首次加载完成后本系统会以同一水/地形修订自动重试。
                };
                let Some(formal_preset) = registry.resolve(&layer.preset_id) else {
                    layer_inputs = Err(format!(
                        "静水体{}引用不存在的水体预设{}",
                        layer.id, layer.preset_id
                    ));
                    break;
                };
                let Some(handles) = registry.image_handles(&formal_preset.id).cloned() else {
                    layer_inputs = Err(format!("水体预设{}缺少微法线句柄快照", formal_preset.id));
                    break;
                };
                for handle in handles.iter().flatten() {
                    if matches!(asset_server.load_state(handle.id()), LoadState::Failed(_)) {
                        layer_inputs = Err(format!("水体预设{}的微法线加载失败", formal_preset.id));
                        break;
                    }
                    if !asset_server.is_loaded_with_dependencies(handle.id()) {
                        return; // 强句柄由注册表持有，下一帧继续非阻塞检查。
                    }
                }
                if layer_inputs.is_err() {
                    break;
                }
                WaterSurfaceDescriptor::from_layer_with_preset(layer, formal_preset, handles)
            };
            layer_inputs
                .as_mut()
                .expect("只有成功输入才会追加静水层")
                .push((layer.clone(), mask, descriptor));
        }
    }
    let heights = draft.heights.clone();
    let terrain_resolution = draft.terrain_manifest().terrain.resolution;
    let world_min = draft.terrain_manifest().world_min;
    let world_max = draft.terrain_manifest().world_max;
    let base_height = draft.terrain_manifest().terrain.base_height;
    let height_scale = draft.terrain_manifest().terrain.height_scale;

    preview.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let result = (|| {
            let Some(water) = water else {
                return Ok(Vec::new());
            };
            validate_water_network_edit(&water, &map_id)?;
            let layer_inputs = layer_inputs?;
            let mut builds = Vec::with_capacity(
                layer_inputs.len() + water.rivers.len() + water.waterfalls.len(),
            );
            if !layer_inputs.is_empty() {
                let heights = heights.ok_or_else(|| {
                    "当前地图缺少GLB派生内存高度，不能构建静水真实预览".to_owned()
                })?;
                let terrain = PaintedWaterTerrainInput::from_shared_vec(
                    heights,
                    terrain_resolution,
                    world_min,
                    world_max,
                    base_height,
                    height_scale,
                )?;
                for (layer, mask, descriptor) in layer_inputs {
                    let minimum_depth = descriptor.minimum_stable_depth();
                    let mesh = build_painted_water_mesh_with_depth(
                        mask.as_ref(),
                        layer.resolution,
                        world_min,
                        world_max,
                        &terrain,
                        layer.water_level,
                        minimum_depth,
                    )?;
                    builds.push(BuiltWaterSurface { descriptor, mesh });
                }
            }
            append_completed_spline_builds(&water, &mut builds)?;
            chunk_water_surfaces(builds)
        })();
        (
            source,
            water_revision,
            terrain_revision,
            preset_revision,
            result,
        )
    }));
}
