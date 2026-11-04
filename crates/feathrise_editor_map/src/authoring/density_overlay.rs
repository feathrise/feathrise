//! 当前植被密度层的地形覆盖预览。
//!
//! 【功能设计】完整切层或撤销结果在后台降采样；连续笔划只更新圆形局部。编辑器因此能像画图工具一样
//! 立即看到落笔痕迹，同时稳定帧不扫描全图、不创建临时实体。

use std::sync::Arc; // 覆盖后台任务共享当前R8修订，启动帧不复制整张作者图。

use bevy::asset::RenderAssetUsages; // 生成图保留主世界像素供局部更新并上传RenderWorld。
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*; // ECS、PBR、Image和后台任务句柄。
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::tasks::AsyncComputeTaskPool; // 全层降采样不进入帧关键路径。
use bevy::tasks::Task;
use bevy::tasks::futures::check_ready;
use feathrise_map_content::MapTerrainKind; // Clipmap 使用同高地形覆盖，GLB 保持原实体。
use feathrise_map_content::{MapId, MapVegetationLayer, TerrainId};
use feathrise_scene::{ClipmapVegetationDensity, DynamicClipmapVegetationOverlay}; // 共享现有密度图片而不复制像素。

use super::resources::{
    EditorAuthoringSession, TerrainPreviewState, VegetationBrushStroke, WaterBrushStroke,
};
use super::selected_vegetation_layer; // 作者层、组规则与固定正式输出使用同一选择映射。
use crate::components::EditorVegetationDensityOverlay;
use crate::resources::MapEditorState; // 当前笔刷作者选择。
use crate::types::{EditorTool, VegetationPreviewMode};
use crate::viewport::{LayerCandidateState, LayerTarget}; // 统一选择与临时悬停身份。

const MAX_OVERLAY_DIMENSION: u32 = 512; // 2048²作者图只生成固定上限覆盖图，实际保存仍保持原分辨率。

type OverlayTaskResult = (
    MapId,
    Option<TerrainId>,
    String,
    VegetationPreviewMode,
    u64,
    [u32; 2],
    Result<Vec<u8>, String>,
);

/// 一张可直接交给RGBA8 Image的密度覆盖结果。
#[derive(Resource, Default)]
pub(crate) struct VegetationDensityOverlayState {
    desired_map_id: Option<MapId>,
    desired_terrain_id: Option<TerrainId>,
    desired_layer_id: Option<String>,
    desired_mode: VegetationPreviewMode,
    desired_revision: u64,
    applied_map_id: Option<MapId>,
    applied_terrain_id: Option<TerrainId>,
    applied_layer_id: Option<String>,
    applied_mode: VegetationPreviewMode,
    applied_revision: u64,
    resolution: [u32; 2],
    task: Option<Task<OverlayTaskResult>>,
    image: Option<Handle<Image>>,
    material: Option<Handle<StandardMaterial>>,
    mesh: Option<Handle<Mesh>>,
    entity: Option<Entity>,
}

/// 当前覆盖模式解析出的只读R8输入；借用草稿避免稳定帧克隆字符串或像素。
struct OverlaySelection<'a> {
    map_id: &'a MapId,                 // 当前草稿稳定地图身份。
    terrain_id: Option<&'a TerrainId>, // 同MapId不同对象的相同层ID不能串覆盖。
    id: &'a str,                       // 作者层、正式层或组ID，与模式共同构成身份。
    revision: u64,                     // 原图使用作者修订，最终/冲突使用编译完成修订。
    resolution: [u32; 2],              // 当前R8输入宽高。
    density: &'a Arc<Vec<u8>>,         // 完整只读R8像素共享所有权。
    color: [u8; 3],                    // 当前profile或冲突诊断综合色。
}

fn overlay_resolution(source: [u32; 2]) -> [u32; 2] {
    let scale = (source[0].max(source[1]) as f32 / MAX_OVERLAY_DIMENSION as f32).max(1.0);
    [
        (source[0] as f32 / scale).round().max(1.0) as u32,
        (source[1] as f32 / scale).round().max(1.0) as u32,
    ]
}

fn layer_color(layer: &MapVegetationLayer) -> [u8; 3] {
    // 用工程稳定 ID 选择固定高对比配色；不按地图、游戏名或目录顺序推断植物种类。
    const COLORS: [[u8; 3]; 6] = [[72, 245, 92], [255, 190, 38], [238, 118, 34],
        [38, 220, 255], [52, 235, 112], [211, 112, 255]];
    let id = layer.runtime_profile.as_deref().unwrap_or(&layer.id);
    let hash = id.bytes().fold(0xcbf29ce484222325u64,
        |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
    COLORS[(hash % COLORS.len() as u64) as usize]
}

#[path = "density_overlay_selection.rs"]
mod selection; // 模式与作者身份解析独立于像素后台工作。
use selection::{overlay_context_active, overlay_mode, selected_overlay}; // 共享解析避免开始与显示身份不一致。

fn rgba_for_density(value: u8, color: [u8; 3]) -> [u8; 4] {
    let alpha = if value == 0 {
        0
    } else {
        48u8.saturating_add((value as f32 * 0.70).round() as u8)
    };
    [color[0], color[1], color[2], alpha]
}

fn sample_density(density: &[u8], source: [u32; 2], x: u32, z: u32) -> u8 {
    density[(z * source[0] + x) as usize]
}

/// 把一个覆盖图像素映射到不重叠的源像素区间；降采样时取最大值，保证细笔刷不会落进抽样缝隙。
fn source_partition(index: u32, output_count: u32, source_count: u32) -> (u32, u32) {
    let start = (u64::from(index) * u64::from(source_count) / u64::from(output_count)) as u32;
    let end_exclusive =
        (u64::from(index + 1) * u64::from(source_count) / u64::from(output_count)) as u32;
    (
        start.min(source_count - 1),
        end_exclusive
            .saturating_sub(1)
            .max(start)
            .min(source_count - 1),
    )
}

fn max_density_for_output_pixel(
    density: &[u8],
    source: [u32; 2],
    output: [u32; 2],
    output_x: u32,
    output_z: u32,
) -> u8 {
    let (min_x, max_x) = source_partition(output_x, output[0], source[0]);
    let (min_z, max_z) = source_partition(output_z, output[1], source[1]);
    let mut maximum = 0u8;
    for z in min_z..=max_z {
        for x in min_x..=max_x {
            maximum = maximum.max(sample_density(density, source, x, z));
        }
    }
    maximum
}

fn build_overlay(
    density: &[u8],
    source: [u32; 2],
    output: [u32; 2],
    color: [u8; 3],
) -> Result<Vec<u8>, String> {
    if density.len() != source[0] as usize * source[1] as usize {
        return Err("植被密度长度与声明分辨率不一致".to_owned());
    }
    let mut pixels = Vec::with_capacity(output[0] as usize * output[1] as usize * 4);
    for z in 0..output[1] {
        for x in 0..output[0] {
            pixels.extend_from_slice(&rgba_for_density(
                max_density_for_output_pixel(density, source, output, x, z),
                color,
            ));
        }
    }
    Ok(pixels)
}

/// 只在切层、载入、撤销或完成笔划后提交一次完整覆盖图后台任务。
pub(crate) fn start_density_overlay_rebuild(
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    viewport_selection: Res<LayerCandidateState>, // 悬停优先正式选择，保持作者层身份。
    stroke: Res<VegetationBrushStroke>,
    water_stroke: Res<WaterBrushStroke>,
    mut overlay: ResMut<VegetationDensityOverlayState>,
) {
    let active_stroke = if editor.tool == EditorTool::Water {
        water_stroke.active
    } else {
        stroke.active
    };
    if !editor.open
        || !overlay_context_active(&editor, &session, &viewport_selection)
        || active_stroke
    {
        if !active_stroke {
            overlay.desired_layer_id = None;
        } // 取消、切类别或关闭后迟到结果不能登记资产。
        return;
    }
    let Some(selected) = selected_overlay(&editor, &session, &viewport_selection) else {
        // 与显示使用相同临时目标。
        overlay.desired_layer_id = None;
        overlay.desired_terrain_id = None;
        return;
    };
    let map_id = selected.map_id.clone(); // 只在真正考虑任务时克隆稳定身份。
    let terrain_id = selected.terrain_id.cloned();
    let layer_id = selected.id.to_owned(); // 模式与ID共同拒绝切换后的迟到结果。
    let mode = overlay_mode(&editor); // 选择模式始终展示独立作者源。
    let revision = selected.revision; // 原图或编译完成修订。
    let source_resolution = selected.resolution; // 完整R8输入规格。
    let density = Arc::clone(selected.density); // 主线程只克隆Arc，整图读取留在后台任务。
    let color = selected.color; // 当前profile或冲突诊断颜色。
    overlay.desired_map_id = Some(map_id.clone());
    overlay.desired_terrain_id = terrain_id.clone();
    overlay.desired_layer_id = Some(layer_id.clone());
    overlay.desired_mode = mode;
    overlay.desired_revision = revision;
    if overlay.task.is_some()
        || (overlay.applied_map_id.as_ref() == Some(&map_id)
            && overlay.applied_terrain_id == terrain_id
            && overlay.applied_layer_id.as_deref() == Some(layer_id.as_str())
            && overlay.applied_mode == mode
            && overlay.applied_revision == revision)
    {
        return;
    }
    let resolution = overlay_resolution(source_resolution);
    overlay.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let pixels = build_overlay(density.as_slice(), source_resolution, resolution, color);
        (
            map_id, terrain_id, layer_id, mode, revision, resolution, pixels,
        )
    }));
}

/// 采用身份仍有效的完整图像；过期任务直接丢弃并由下一帧为最新修订重启。
pub(crate) fn poll_density_overlay_rebuild(
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut overlay: ResMut<VegetationDensityOverlayState>,
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    viewport_selection: Res<LayerCandidateState>,
) {
    let Some(task) = overlay.task.as_mut() else {
        return;
    };
    let Some((map_id, terrain_id, layer_id, mode, revision, resolution, result)) =
        check_ready(task)
    else {
        return;
    };
    overlay.task = None;
    if overlay.desired_map_id.as_ref() != Some(&map_id)
        || overlay.desired_terrain_id != terrain_id
        || overlay.desired_layer_id.as_deref() != Some(layer_id.as_str())
        || overlay.desired_mode != mode
        || overlay.desired_revision != revision
        || !editor.open
        || !overlay_context_active(&editor, &session, &viewport_selection)
        || selected_overlay(&editor, &session, &viewport_selection).is_none_or(|selected| {
            selected.map_id != &map_id
                || selected.terrain_id != terrain_id.as_ref()
                || selected.id != layer_id
                || selected.revision != revision
                || overlay_mode(&editor) != mode
        })
    {
        return;
    }
    let Ok(pixels) = result else {
        return;
    };
    let mut image = Image::new(
        Extent3d {
            width: resolution[0],
            height: resolution[1],
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Nearest,
        ..default()
    });
    let image = images.add(image);
    let material = if let Some(handle) = overlay.material.as_ref() {
        if let Some(mut material) = materials.get_mut(handle) {
            material.base_color_texture = Some(image.clone());
        }
        handle.clone()
    } else {
        materials.add(StandardMaterial {
            base_color: Color::WHITE,
            base_color_texture: Some(image.clone()),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            depth_bias: 4.0,
            ..default()
        })
    };
    if let Some(old) = overlay.image.replace(image)
        && old.id() != overlay.image.as_ref().expect("已写入覆盖图").id()
    {
        images.remove(old.id());
    }
    overlay.material = Some(material);
    overlay.resolution = resolution;
    overlay.applied_map_id = Some(map_id);
    overlay.applied_terrain_id = terrain_id;
    overlay.applied_layer_id = Some(layer_id);
    overlay.applied_mode = mode;
    overlay.applied_revision = revision;
}

/// 连续笔划只重采样覆盖圆附近的固定小区域，当前帧落笔当前帧即可看见。
fn update_local_overlay(
    image: &mut Image,
    density: &[u8],
    source: [u32; 2],
    output: [u32; 2],
    world_min: [f32; 2],
    world_max: [f32; 2],
    from: Vec3,
    to: Vec3,
    radius: f32,
    color: [u8; 3],
) -> bool {
    let Some(pixels) = image.data.as_mut() else {
        return false;
    };
    if pixels.len() != output[0] as usize * output[1] as usize * 4 {
        return false;
    }
    let to_pixel = |world: f32, axis: usize, size: u32| {
        ((world - world_min[axis]) / (world_max[axis] - world_min[axis]) * (size - 1) as f32)
            .clamp(0.0, (size - 1) as f32)
    };
    let from_x = to_pixel(from.x, 0, output[0]);
    let from_z = to_pixel(from.z, 1, output[1]);
    let to_x = to_pixel(to.x, 0, output[0]);
    let to_z = to_pixel(to.z, 1, output[1]);
    let radius_x = radius / (world_max[0] - world_min[0]) * (output[0] - 1) as f32;
    let radius_z = radius / (world_max[1] - world_min[1]) * (output[1] - 1) as f32;
    let min_x = (from_x.min(to_x) - radius_x - 1.0).floor().max(0.0) as u32;
    let max_x = (from_x.max(to_x) + radius_x + 1.0)
        .ceil()
        .min((output[0] - 1) as f32) as u32;
    let min_z = (from_z.min(to_z) - radius_z - 1.0).floor().max(0.0) as u32;
    let max_z = (from_z.max(to_z) + radius_z + 1.0)
        .ceil()
        .min((output[1] - 1) as f32) as u32;
    for z in min_z..=max_z {
        for x in min_x..=max_x {
            let rgba = rgba_for_density(
                max_density_for_output_pixel(density, source, output, x, z),
                color,
            );
            let offset = ((z * output[0] + x) * 4) as usize;
            pixels[offset..offset + 4].copy_from_slice(&rgba);
        }
    }
    true
}

/// 绑定共享地形网格、同步显隐，并在活动笔划期间执行局部即时更新。
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_density_overlay(
    mut commands: Commands,
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    viewport_selection: Res<LayerCandidateState>, // 悬停结束自动回到正式选择。
    stroke: Res<VegetationBrushStroke>,
    water_stroke: Res<WaterBrushStroke>,
    terrain: Res<TerrainPreviewState>,
    mut images: ResMut<Assets<Image>>,
    mut overlay: ResMut<VegetationDensityOverlayState>,
    mut clipmap: Option<ResMut<DynamicClipmapVegetationOverlay>>, // 仅发布 Clipmap 专属覆盖输入。
    pose: Option<Res<feathrise_scene::TerrainObjectTransformPreview>>,
    mut entities: Query<
        (&mut Visibility, &mut Mesh3d, &mut Transform),
        With<EditorVegetationDensityOverlay>,
    >,
) {
    if let Some(clipmap) = clipmap.as_mut() {
        clipmap.density = None;
    } // 关闭、切图和切层先撤销旧覆盖。
    let selected = selected_overlay(&editor, &session, &viewport_selection); // 选择模式优先悬停作者源。
    let draft = session.draft.as_ref();
    let transform = draft.map_or_else(Transform::default, |draft| {
        super::terrain_documents::canvas_transform(draft, pose.as_deref())
    });
    let metric = draft.map(super::terrain_documents::PaintMetricSpace::from_draft);
    let metric_cursor = editor
        .brush_cursor_world
        .zip(draft)
        .zip(metric)
        .map(|((cursor, draft), metric)| metric.point(draft.terrain_point(cursor)));
    let metric_start = metric
        .map(|metric| {
            let point = if editor.tool == EditorTool::Water {
                water_stroke.segment_start
            } else {
                stroke.segment_start
            };
            point.map(|point| metric.point(point))
        })
        .flatten();
    let metric_end = metric
        .map(|metric| {
            let point = if editor.tool == EditorTool::Water {
                water_stroke.last_world
            } else {
                stroke.last_world
            };
            point.map(|point| metric.point(point))
        })
        .flatten();
    let source_stroke = if editor.tool == EditorTool::Water {
        water_stroke.active
    } else {
        stroke.active && editor.vegetation_preview_mode == VegetationPreviewMode::Source
    };
    if source_stroke
        && selected
            .as_ref()
            .is_some_and(|selected| overlay.applied_revision != selected.revision)
        && let (Some(selected), Some(cursor), Some(mut image)) = (
            selected.as_ref(),
            metric_end.or(metric_cursor),
            overlay
                .image
                .as_ref()
                .and_then(|handle| images.get_mut(handle)),
        )
        && overlay.applied_map_id.as_ref() == Some(selected.map_id)
        && overlay.applied_terrain_id.as_ref() == selected.terrain_id
        && overlay.applied_layer_id.as_deref() == Some(selected.id)
        && overlay.applied_mode == VegetationPreviewMode::Source
        && update_local_overlay(
            &mut image,
            selected.density.as_slice(),
            selected.resolution,
            overlay.resolution,
            metric.expect("覆盖选择与当前草稿同源").bounds(
                draft
                    .expect("覆盖选择与当前草稿同源")
                    .terrain_manifest()
                    .world_min,
            ),
            metric.expect("覆盖选择与当前草稿同源").bounds(
                draft
                    .expect("覆盖选择与当前草稿同源")
                    .terrain_manifest()
                    .world_max,
            ),
            metric_start.unwrap_or(cursor),
            cursor,
            editor.brush_radius,
            selected.color,
        )
    {
        overlay.applied_revision = selected.revision; // 局部刷印已经追到当前作者修订。
    }
    let identity_matches = selected.as_ref().is_some_and(|selected| {
        overlay.applied_map_id.as_ref() == Some(selected.map_id)
            && overlay.applied_terrain_id.as_ref() == selected.terrain_id
            && overlay.applied_layer_id.as_deref() == Some(selected.id)
            && overlay.applied_mode == overlay_mode(&editor) // 不能把正式编译覆盖误认作作者层高亮。
    });
    let should_show = editor.open
        && overlay_context_active(&editor, &session, &viewport_selection)
        && identity_matches; // 查看与笔刷工具共享完全相同的身份匹配和显隐实体。
    if let Some(draft) = session
        .draft
        .as_ref()
        .filter(|draft| draft.terrain_manifest().terrain.kind == MapTerrainKind::SparseClipmap)
    {
        // 新图没有旧 GLB TerrainPreviewState。
        if should_show
            && let (Some(clipmap), Some(image)) = (clipmap.as_mut(), overlay.image.as_ref())
        {
            clipmap.density = Some(ClipmapVegetationDensity {
                map_id: draft.terrain_manifest().id.clone(), // 同图身份防止热切图残留。
                image: image.clone(),                        // 连续笔画仍使用上方已有局部即时更新。
            }); // 场景按世界 XZ 投影到实际位移后的共享网格。
        }
        if let Some(entity) = overlay.entity
            && let Ok((mut visibility, _, _)) = entities.get_mut(entity)
        {
            *visibility = Visibility::Hidden;
        } // 之前 GLB 的覆盖实体不进入 Clipmap。
        return; // 不创建平面副本；原 GLB 分支以下保持不变。
    }
    if overlay.entity.is_none()
        && let (Some(mesh), Some(material)) = (terrain.mesh.as_ref(), overlay.material.as_ref())
    {
        overlay.entity = Some(
            commands
                .spawn((
                    Name::new("地图编辑器植被密度即时覆盖"),
                    EditorVegetationDensityOverlay,
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    transform, // 源网格仅应用一次对象姿态，UV不再额外缩放。
                    bevy::camera::visibility::RenderLayers::layer(
                        crate::viewport::MAP_EDITOR_BRUSH_RENDER_LAYER,
                    ),
                    bevy::light::NotShadowCaster,
                    if should_show {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    },
                ))
                .id(),
        );
        overlay.mesh = Some(mesh.clone());
        return;
    }
    let Some(entity) = overlay.entity else {
        return;
    };
    let Ok((mut visibility, mut mesh3d, mut current_transform)) = entities.get_mut(entity) else {
        overlay.entity = None;
        return;
    };
    if *current_transform != transform {
        *current_transform = transform;
    }
    let target_visibility = if should_show {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if *visibility != target_visibility {
        *visibility = target_visibility;
    }
    if let Some(mesh) = terrain.mesh.as_ref()
        && overlay
            .mesh
            .as_ref()
            .is_none_or(|old| old.id() != mesh.id())
    {
        mesh3d.0 = mesh.clone();
        overlay.mesh = Some(mesh.clone());
    }
}

#[cfg(test)]
#[path = "density_overlay_tests.rs"]
mod tests; // 透明度、固定覆盖预算和最大池化保细节回归独立存放。
