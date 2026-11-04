//! 地表笔划的贴地柔边覆盖；共享植被覆盖的透明材质表现，正式PBR就绪后退出。

use crate::authoring::{
    EditorAuthoringSession, SurfaceBrushStroke, SurfacePreviewState, TerrainPreviewState,
}; // 只读取当前权威作者和完整预览身份。
use crate::resources::MapEditorState; // 编辑器关闭或切图立即撤回辅助覆盖。
use bevy::asset::RenderAssetUsages; // 保留唯一主世界像素以便局部更新。
use bevy::camera::visibility::RenderLayers; // 辅助覆盖仅供F12相机显示。
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor}; // 线性过滤消除低分辨率覆盖的锯齿。
use bevy::light::NotShadowCaster; // 辅助色不参与正式世界阴影。
use bevy::prelude::*; // ECS资源和透明覆盖材质。
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat}; // 固定预算的RGBA8覆盖。
use feathrise_map_content::{MapId, TerrainId}; // 不允许跨地图或同图另一对象复用路径。
use feathrise_scene::{
    ClipmapSurfaceCoverage, ClipmapSurfaceOverlayIdentity, DynamicClipmapSurfaceOverlay,
};

const DIM: usize = 256; // 与地图大小、拖动时长无关；变化帧上传最多256KiB。
const PIXELS: usize = DIM * DIM; // CPU栅格与GPU图像具有相同固定容量。

#[derive(Resource, Default)] // 保留原输入接口，但不再绘制线框或保留历史圆圈。
pub(crate) struct SurfaceStrokeTrail {
    map_id: Option<MapId>,                      // 当前覆盖的稳定地图身份。
    terrain_id: Option<TerrainId>,              // 同图独立画布不能共享旧覆盖像素。
    author_revision: u64,                       // 外部撤销与结构换代清除旧覆盖。
    terrain_revision: u64,                      // 地形变更后旧空间投影不再有效。
    target_revision: u64,                       // 整张覆盖等包含最新笔划的完整PBR采用。
    pixels: Vec<[u8; 4]>,                       // 固定栅格保留整笔，不因长笔淘汰前端。
    touched: Vec<usize>,                        // 清理只访问实际触及像素；每像素最多登记一次。
    dirty: Vec<usize>,                          // 上传只写发生变化的位置。
    dirty_flags: Vec<bool>,                     // 去重确保dirty队列不会随笔划增长。
    image: Option<Handle<Image>>,               // 跨帧复用同一图像，禁止每帧重新建图。
    material: Option<Handle<StandardMaterial>>, // 透明材质随图像复用。
    entity: Option<Entity>,                     // 仅一个共享地形网格的覆盖实体。
    pending_keys: [Option<(Vec3, Vec3, f32, bool, f32)>; 2], // 冻结待补几何没有变化时不重算覆盖。
    pending_revision: u64,                      // 作者换代后相同几何仍需重新受理。
    projection: Option<SurfaceTrailSource>, // 只读投影复用本系统真实show与源姿态，不能自行猜测显隐。
    projection_versions: [u64; 16], // 每16行只记录原脏像素版本；IPC不每帧扫描整图。
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SurfaceTrailSource {
    pub map: MapId, pub terrain: Option<TerrainId>, pub terrain_revision: u64,
    pub transform: [f32;16], pub kind: feathrise_map_content::MapTerrainKind,
}

mod raster; // 局部栅格与渲染资产职责分离。

impl SurfaceStrokeTrail {
    pub(crate) fn projection_source(&self) -> Option<&SurfaceTrailSource> { self.projection.as_ref() }
    pub(crate) fn projection_versions(&self) -> &[u64;16] { &self.projection_versions }
    pub(crate) fn projection_block(&self, block:usize) -> Option<Vec<u8>> {
        if block >= 16 || self.pixels.len() != PIXELS { return None; }
        // 单次固定16KiB拷贝，不随地图尺寸、笔划长度或脏像素总量增长。
        Some(self.pixels[block*DIM*16..(block+1)*DIM*16].iter().flatten().copied().collect())
    }
    pub(crate) fn has_visible_coverage(&self) -> bool {
        !self.touched.is_empty() // 选中覆盖让位给尚未由PBR接管的即时绿色笔迹。
    }
}

#[derive(Component)] // 与植被覆盖隔离，不改变正式地形或存档。
pub(crate) struct SurfaceStrokeOverlay; // 插件注册系统需能访问查询中的标记类型。

/// 地表通道冻结当前画布；正式Clipmap负责真实高度和缺页，编辑器仅提供已有RGBA图。
pub(super) fn clipmap_surface_identity(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    pose: Option<&feathrise_scene::TerrainObjectTransformPreview>,
) -> Option<ClipmapSurfaceOverlayIdentity> {
    let draft = session.draft.as_ref()?;
    if !editor.open
        || editor.close_requested
        || !matches!(
            editor.tool,
            crate::types::EditorTool::Surface | crate::types::EditorTool::Inspect
        )
        || editor.selected_map_id.as_ref() != Some(&draft.map_id)
        || !crate::authoring::terrain_documents::canvas_visible(session, editor)
        || draft.terrain_manifest().terrain.kind != feathrise_map_content::MapTerrainKind::SparseClipmap
    {
        return None;
    }
    let canvas = draft
        .terrain_documents
        .active
        .as_ref()
        .map(|active| &active.definition.id);
    let terrain_id = if draft.manifest.terrains.is_none() {
        // 旧单地形的primary认领只改变作者寻址，正式场景仍使用根资源；异常非primary不得借根。
        if canvas.is_some_and(|id| id != &TerrainId::legacy()) {
            return None;
        }
        None
    } else {
        canvas.cloned()
    }; // 显式对象即使名为legacy也必须保留Some，不获得旧根绘制资格。
    Some(ClipmapSurfaceOverlayIdentity {
        map_id: draft.map_id.clone(),
        terrain_id,
        author_revision: session.revision,
        terrain_revision: session.terrain_revision,
        surface_revision: session.surface_revision,
        source_to_world: crate::authoring::terrain_documents::canvas_transform(draft, pose)
            .to_matrix(),
    })
}

pub(crate) fn draw_surface_stroke_trail(
    mut commands: Commands,               // 只在第一次显示时创建覆盖实体。
    editor: Res<MapEditorState>,          // F12和地图选择门。
    session: Res<EditorAuthoringSession>, // 当前权威作者版本。
    stroke: Res<SurfaceBrushStroke>,      // 作者轨迹有界排队，辅助覆盖仍只检查两个固定签名。
    preview: Res<SurfacePreviewState>,    // 只有真实完整PBR回执才收起覆盖。
    terrain: Res<TerrainPreviewState>,    // 共享当前带UV的地形Mesh。
    pose: Option<Res<feathrise_scene::TerrainObjectTransformPreview>>,
    mut clipmap: Option<ResMut<DynamicClipmapSurfaceOverlay>>, // 独立通道不受植被工具清空。
    mut trail: ResMut<SurfaceStrokeTrail>,                     // 固定栅格与持久资产。
    mut images: ResMut<Assets<Image>>, // 在原Image局部写入，绝不深拷贝整图。
    mut materials: ResMut<Assets<StandardMaterial>>, // 仅首次创建透明材质。
    mut source_and_entities: ParamSet<(
        feathrise_scene::TerrainObjectQueries,
        Query<(&mut Visibility, &mut Mesh3d, &mut Transform), With<SurfaceStrokeOverlay>>,
    )>, // 正式源只读与辅助实体写入分时借用，不能靠不相交的假定规避B0001。
) {
    trail.reconcile(&session); // 即使输入因门禁跳过也必须清理过期身份。
    let context = editor.open
        && crate::authoring::terrain_documents::canvas_visible(&session, &editor)
        && editor.selected_map_id == session.draft.as_ref().map(|draft| draft.map_id.clone()); // 覆盖不进入游戏相机或另一张地图。
    let failed = preview
        .failed_identity
        .as_ref()
        .is_some_and(|(map, id, revision, _)| {
            Some(map) == editor.selected_map_id.as_ref()
                && crate::authoring::same_surface_canvas(
                    &session,
                    id.as_ref(),
                    session
                        .draft
                        .as_ref()
                        .and_then(|draft| draft.terrain_documents.active.as_ref())
                        .map(|active| &active.definition.id),
                )
                && *revision == session.surface_revision
        }); // 明确失败不永久遮住地形。
    let clean = !stroke.active
        && session
            .draft
            .as_ref()
            .is_none_or(|draft| !draft.surface_dirty); // 空白擦除没有PBR任务也必须收口。
    if !context || failed || clean {
        // 取消、保存或失败同步撤回辅助色。
        trail.clear(); // 实际PBR接管和未保存游戏预览完全不受影响。
    } else if stroke.active
        && stroke.map_id == editor.selected_map_id
        && stroke.expected_revision == session.revision
        && stroke.expected_surface_revision == session.surface_revision
    {
        for (slot, sample) in [stroke.pending_sample, stroke.queued_samples.back().copied()]
            .into_iter()
            .enumerate()
        {
            let key = sample.map(|sample| {
                (
                    sample.from,
                    sample.to,
                    sample.radius,
                    sample.erase,
                    sample.amount,
                )
            }); // 小型冻结签名，无像素复制。
            if trail.pending_revision == session.revision && trail.pending_keys[slot] == key {
                continue;
            } // 等待独立画布的稳定帧不扫描栅格。
            trail.pending_keys[slot] = key; // 保存当前受理段的真实参数。
            let Some(sample) = sample else { continue }; // 待补段消费后清理签名即可。
            // 只画当前段及最新受理真段；中间队列随作者消费时record，不把256段栅格工作塞进一帧。
            trail.record(
                &session,
                sample.from,
                sample.to,
                sample.radius,
                sample.erase,
                sample.amount,
            ); // 独立写侧没准备好时也显示柔边填色。
        }
        trail.pending_revision = session.revision; // 两段都基于同一版本完成核对。
    } else if preview.applied_map_id == trail.map_id
        && crate::authoring::same_surface_canvas(
            &session,
            preview.applied_terrain_id.as_ref(),
            trail.terrain_id.as_ref(),
        )
        && preview.material.is_some()
        && preview.applied_revision >= trail.target_revision
    {
        trail.clear(); // 新笔存在时target更高，迟到旧PBR回执不清覆盖。
    }
    let clipmap_identity = (!failed)
        .then(|| clipmap_surface_identity(&editor, &session, pose.as_deref()))
        .flatten();
    let source = crate::authoring::surface_canvas_mesh(
        &session,
        &terrain,
        pose.as_deref(),
        &source_and_entities.p0(),
    )
    .filter(|source| source.kind != feathrise_map_content::MapTerrainKind::Voxel);
    let is_clipmap = session.draft.as_ref().is_some_and(|draft| {
        draft.terrain_manifest().terrain.kind == feathrise_map_content::MapTerrainKind::SparseClipmap
    });
    let show = context
        && !trail.touched.is_empty()
        && if is_clipmap {
            clipmap_identity.is_some()
        } else {
            source.is_some()
        };
    let transform = source
        .as_ref()
        .map_or(Transform::IDENTITY, |source| source.transform);
    trail.projection = if show {
        session.draft.as_ref().map(|draft| SurfaceTrailSource {
            map:draft.map_id.clone(), terrain:if draft.manifest.terrains.is_none() {None} else {draft.terrain_documents.active.as_ref().map(|active|active.definition.id.clone())},
            terrain_revision:session.terrain_revision,
            transform:clipmap_identity.as_ref().map_or_else(||transform.to_matrix().to_cols_array(),|identity|identity.source_to_world.to_cols_array()),
            kind:draft.terrain_manifest().terrain.kind,
        })
    } else { None };
    if show && trail.image.is_none() {
        // 图像和材质只创建一次。
        let mut image = Image::new_fill(
            Extent3d {
                width: DIM as u32,
                height: DIM as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[0; 4],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        ); // 唯一固定256KiB图像。
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            ..default()
        }); // 与植被密度覆盖相同的柔滑采样。
        let handle = images.add(image); // 保存唯一强句柄。
        trail.material = Some(materials.add(StandardMaterial {
            base_color: Color::WHITE,                 // RGB直接来自低饱和覆盖颜色。
            base_color_texture: Some(handle.clone()), // 始终复用同一小图。
            alpha_mode: AlphaMode::Blend,             // 柔边透明覆盖真实地形。
            unlit: true,                              // 与植被覆盖一样不受光照闪变。
            depth_bias: 4.0,                          // 与共享地形面避免深度争抢。
            ..default()                               // 其他参数沿用植被覆盖默认值。
        }));
        trail.image = Some(handle); // 后续只局部更新像素。
    }
    if !trail.dirty.is_empty()
        && let Some(handle) = trail.image.clone()
        && let Some(mut image) = images.get_mut(&handle)
        && let Some(data) = image.data.as_mut()
    {
        while let Some(index) = trail.dirty.pop() {
            // 不clone整图；每个脏像素仅复制四字节。
            data[index * 4..index * 4 + 4].copy_from_slice(&trail.pixels[index]); // 变化帧GPU仍至多上传固定256KiB。
            trail.dirty_flags[index] = false; // 下帧允许同一点再次更新。
        }
    }
    if let Some(clipmap) = clipmap.as_mut() {
        clipmap.current = clipmap_identity.clone();
        clipmap.trail = show
            .then(|| {
                clipmap_identity
                    .zip(trail.image.clone())
                    .map(|(identity, image)| ClipmapSurfaceCoverage { identity, image })
            })
            .flatten();
    } // Clipmap只发布既有RGBA图；场景统一按正式高度绘制，不能创建平面副本。
    if let Some(entity) = trail.entity {
        // 已有实体只更新显隐和必要的地形句柄。
        if let Ok((mut visibility, mut mesh, mut current_transform)) =
            source_and_entities.p1().get_mut(entity)
        {
            if *current_transform != transform {
                *current_transform = transform;
            }
            let next_visibility = if show && !is_clipmap {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            }; // 关闭F12即隐藏，不泄露给游戏画面。
            if *visibility != next_visibility {
                *visibility = next_visibility;
            } // 稳定帧不制造Changed<Visibility>。
            if let Some(current) = source.as_ref().map(|source| &source.mesh)
                && mesh.0 != *current
            {
                mesh.0 = current.clone();
            } // 高度网格采用后复用其新几何。
        } else {
            trail.entity = None;
        } // 场景回收后允许下次重新创建唯一实体。
    } else if show
        && !is_clipmap
        && let (Some(mesh), Some(material)) = (
            source.as_ref().map(|source| &source.mesh),
            trail.material.as_ref(),
        )
    {
        trail.entity = Some(
            commands
                .spawn((
                    Name::new("地图编辑器地表柔边覆盖"), // 可辨识的作者辅助实体。
                    SurfaceStrokeOverlay,                // 不混入正式地形查询。
                    Mesh3d(mesh.clone()),                // 直接共享地形拓扑，山坡上不会悬浮圆圈。
                    MeshMaterial3d(material.clone()),    // 跨帧复用透明覆盖材质。
                    transform,                           // 源顶点只在此处应用一次对象姿态。
                    Visibility::Inherited,               // 已满足当前地图与地形身份门。
                    RenderLayers::layer(super::MAP_EDITOR_BRUSH_RENDER_LAYER), // 仅F12相机可见。
                    NotShadowCaster,                     // 不给游戏世界投射辅助阴影。
                ))
                .id(),
        ); // 只有一个实体，与笔划长度无关。
    }
}

#[cfg(test)] // 纯栅格和生命周期回归无需GPU。
#[path = "surface_trail_tests.rs"]
mod tests;
