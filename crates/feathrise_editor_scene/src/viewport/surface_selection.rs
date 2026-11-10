//! 地表作者层的精确贡献覆盖；整图提取在单一后台任务中完成，F12只显示当前选择。
use super::SurfaceStrokeTrail;
use super::{LayerCandidateState, LayerTarget}; // 视口悬停与正式选择共用稳定作者身份。
use crate::authoring::{EditorAuthoringSession, SurfaceBrushStroke, TerrainPreviewState}; // 权威作者输入。
use crate::resources::SceneEditorState; // 当前显式选层。
use crate::types::EditorTool; // 地表工具才显示辅助覆盖。
use bevy::asset::RenderAssetUsages; // 覆盖仅保留固定小图。
use bevy::camera::visibility::RenderLayers; // 只供编辑相机显示。
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor}; // 柔边采样。
use bevy::light::NotShadowCaster; // 辅助图不投影。
use bevy::prelude::*; // ECS和材质资产。
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat}; // RGBA8小图。
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready}; // 非阻塞single-flight。
use feathrise_scene_content::{SceneId, SurfacePresetRegistry, TerrainId}; // 完整场景、对象与预设选择身份。
use std::sync::Arc; // 与笔刷写侧共享不可变配对像素。 // 即时笔迹优先。

#[derive(Clone, PartialEq, Eq, Debug)] // 结果必须匹配所有输入身份。
struct Identity {
    scene_id: SceneId,                 // 场景身份。
    terrain: Option<TerrainId>, // 同图同名层也属于独立网格画布。
    layer: String,              // 稳定作者层ID，绝不能使用预设ID合并。
    index: usize,               // 配对索引中的局部位置。
    revision: u64,              // 当前完整地表修订。
}
impl Identity {
    fn same_layer(&self, other: &Self) -> bool {
        // 同层修订允许保留上一完整覆盖。
        self.scene_id == other.scene_id
            && self.terrain == other.terrain
            && self.layer == other.layer
            && self.index == other.index // 删除换位也失效。
    }
}
#[derive(Resource, Default)] // 一项在途任务和一项完整结果。
pub(crate) struct SurfaceSelectionOverlay {
    desired: Option<Identity>,                                     // 最新选择。
    applied: Option<Identity>,                                     // 当前覆盖。
    failed: Option<Identity>,                                      // 相同失败输入不重复整图计算。
    task: Option<Task<(Identity, Result<(Image, bool), String>)>>, // 任务结束之前不释放并发槽。
    image: Option<Handle<Image>>,                                  // 替换时释放旧小图。
    material: Option<Handle<StandardMaterial>>,                    // 跨修订复用。
    entity: Option<Entity>,                                        // 唯一共享地形实体。
    nonempty: bool,                                                // 零贡献禁止显示。
}
#[derive(Component)] // 与正式地表查询隔离。
pub(crate) struct SurfaceSelectionMesh;

fn selection(
    editor: &SceneEditorState,
    session: &EditorAuthoringSession,
    presets: Option<&SurfacePresetRegistry>,
    candidates: &LayerCandidateState, // 临时高亮不能覆盖侧栏正式选层。
) -> Option<Identity> {
    // 只做常数成本候选检查。
    if !editor.open || !matches!(editor.tool, EditorTool::Surface | EditorTool::Inspect) {
        // 统一选择模式也提供地表整层高亮。
        return None;
    } // 离开工具即时隐藏。
    let draft = session.draft.as_ref()?; // 草稿必须有效。
    if editor.selected_scene_id.as_ref() != Some(&draft.scene_id) {
        return None;
    } // 拒绝旧图。
    if !crate::authoring::terrain_documents::canvas_visible(session, editor) {
        return None;
    } // 换槽、隐藏或停用时撤销辅助色，不能把旧对象的同名层带到新对象。
    let surface = draft.surface.as_ref()?; // 无地表没有作者层。
    let index = if editor.tool == EditorTool::Inspect {
        if !candidates.author_canvas_matches(draft) { return None; }
        // 候选悬停只控制辅助覆盖。
        let LayerTarget::Surface(id) =
            candidates.hover.as_ref().or(candidates.selected.as_ref())?
        else {
            return None;
        }; // 悬停其他类别即时隐藏。
        let index = candidates.surface_index(id)?; // 后台候选记录作者索引，主线程常数查找。
        if surface.layers.get(index)?.id != *id {
            return None;
        } // 删除或换位后拒绝旧缓存。
        index // 精确作者层，不能按相同预设合并。
    } else {
        // 编辑模式沿用侧栏当前选择。
        crate::ui::surface_layers::selected_index(surface, editor, presets)? // 保留预设别名与作者索引规则。
    }; // 共同生成后台覆盖身份。
    Some(Identity {
        scene_id: draft.scene_id.clone(),
        terrain: draft
            .terrain_documents
            .active
            .as_ref()
            .map(|active| active.definition.id.clone()),
        layer: surface.layers[index].id.clone(),
        index,
        revision: session.surface_revision,
    }) // 字符串很小，无像素复制。
}

pub(crate) fn sync_surface_selection_overlay(
    mut commands: Commands,                      // 只在首次创建实体。
    mut editor: ResMut<SceneEditorState>,          // 选择和一次性结果提示。
    session: Res<EditorAuthoringSession>,        // 作者配对图。
    presets: Option<Res<SurfacePresetRegistry>>, // 正式别名解析。
    candidates: Res<LayerCandidateState>,        // 独立悬停覆盖目标。
    stroke: Res<SurfaceBrushStroke>,             // 松笔才重建。
    trail: Res<SurfaceStrokeTrail>,              // 绿色笔迹优先。
    terrain: Res<TerrainPreviewState>,           // 共享当前UV网格。
    preview: Option<Res<crate::authoring::SurfacePreviewState>>, // PBR失败时撤回辅助色，不冒充新材质已采用。
    pose: Option<Res<feathrise_scene::TerrainObjectTransformPreview>>,
    mut clipmap: Option<ResMut<feathrise_scene::DynamicClipmapSurfaceOverlay>>, // 与绿色笔迹独立发布。
    mut state: ResMut<SurfaceSelectionOverlay>,                           // 有界缓存。
    mut images: ResMut<Assets<Image>>,                                    // 完整小图登记。
    mut materials: ResMut<Assets<StandardMaterial>>,                      // 复用透明材质。
    mut source_and_entities: ParamSet<(
        feathrise_scene::TerrainObjectQueries,
        Query<(&mut Visibility, &mut Mesh3d, &mut Transform), With<SurfaceSelectionMesh>>,
    )>, // 几何读取与辅助实例写入分时借用。
) {
    let desired = selection(&editor, &session, presets.as_deref(), &candidates); // 每帧核对悬停优先的当前身份。
    state.desired = desired.clone(); // 切层后旧结果在本帧隐藏。
    let ready = state.task.as_mut().and_then(check_ready); // 关闭时也轮询，防止槽位泄漏。
    if let Some((identity, result)) = ready {
        // 只有已结束任务才释放槽。
        state.task = None; // 不假设drop会抢占CPU。
        if desired.as_ref() == Some(&identity) {
            // 迟到结果零资产副作用。
            match result {
                Ok((image, nonempty)) => {
                    let image = images.add(image); // 一次最多512² RGBA8上传。
                    if let Some(handle) = state.material.as_ref() {
                        // 稳定材质避免反复创建。
                        if let Some(mut material) = materials.get_mut(handle) {
                            material.base_color_texture = Some(image.clone());
                        } // 原子替换完整图。
                    } else {
                        state.material = Some(materials.add(StandardMaterial {
                            base_color: Color::WHITE,                // 使用清晰青色原图。
                            base_color_texture: Some(image.clone()), // 唯一小图。
                            alpha_mode: AlphaMode::Blend,            // 柔边透明覆盖。
                            unlit: true,                             // 光照不影响辨识。
                            depth_bias: 3.0,                         // 避免与共享地形争抢深度。
                            ..default()                              // 沿用植被透明覆盖参数。
                        }));
                    }
                    if let Some(old) = state.image.replace(image) {
                        images.remove(old.id());
                    } // 及时释放旧小图。
                    state.applied = Some(identity); // 完整资产一起采用。
                    state.nonempty = nonempty; // 空层不生成假范围。
                    state.failed = None; // 新修订恢复成功。
                    if !nonempty {
                        editor.status = "该作者层尚未绘制地表；可直接绘制".to_owned();
                    } // 每次完整结果只提示一次。
                }
                Err(error) => {
                    state.failed = Some(identity);
                    editor.status = format!("地表作者层选中效果不可用：{error}");
                } // 不重试同一坏输入。
            }
        }
    }
    if !stroke.active
        && state.task.is_none()
        && desired.is_some()
        && state.applied != desired
        && state.failed != desired
    {
        if let Some(draft) = session.draft.as_ref()
            && let (Some(surface), Some(indices), Some(weights)) = (
                &draft.surface,
                &draft.surface_indices,
                &draft.surface_weights,
            )
        {
            let identity = desired.clone().expect("已经确认有效选择"); // 冻结唯一任务身份。
            let resolution = surface.resolution; // 冻结尺寸。
            let indices = Arc::clone(indices); // 不深拷贝作者源。
            let weights = Arc::clone(weights); // 同修订配对快照。
            state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
                let result = build_overlay(&indices, &weights, resolution, identity.index); // 完整扫描只在worker。
                (identity, result) // 返回完整候选。
            }));
        }
    }
    let show = desired
        .as_ref()
        .zip(state.applied.as_ref())
        .is_some_and(|(a, b)| a.same_layer(b))
        && state.nonempty
        && !stroke.active
        && !trail.has_visible_coverage(); // 绿色轨迹退出后恢复青色选中面。
    let failed = session.draft.as_ref().is_some_and(|draft| {
        // 当前提取失败不能留下同层旧覆盖冒充新值。
        state.failed.as_ref().is_some_and(|failed| {
            failed.scene_id == draft.scene_id
                && failed.revision == session.surface_revision
                && failed.terrain.as_ref()
                    == draft
                        .terrain_documents
                        .active
                        .as_ref()
                        .map(|active| &active.definition.id)
        }) || preview
            .as_ref()
            .and_then(|preview| preview.failed_identity.as_ref())
            .is_some_and(|(map, terrain, revision, _)| {
                map == &draft.scene_id
                    && *revision == session.surface_revision
                    && crate::authoring::same_surface_canvas(
                        &session,
                        terrain.as_ref(),
                        draft
                            .terrain_documents
                            .active
                            .as_ref()
                            .map(|active| &active.definition.id),
                    )
            })
    });
    let source = crate::authoring::surface_canvas_mesh(
        &session,
        &terrain,
        pose.as_deref(),
        &source_and_entities.p0(),
    )
    .filter(|source| source.kind != feathrise_scene_content::SceneTerrainKind::Voxel);
    let is_clipmap = session.draft.as_ref().is_some_and(|draft| {
        draft.terrain_manifest().terrain.kind == feathrise_scene_content::SceneTerrainKind::SparseClipmap
    });
    let clipmap_identity = (!failed)
        .then(|| super::surface_trail::clipmap_surface_identity(&editor, &session, pose.as_deref()))
        .flatten();
    let show = show
        && !failed
        && if is_clipmap {
            clipmap_identity.is_some()
        } else {
            source.is_some()
        };
    if let Some(clipmap) = clipmap.as_mut() {
        clipmap.current = clipmap_identity.clone();
        clipmap.selection = show
            .then(|| {
                clipmap_identity
                    .zip(state.image.clone())
                    .map(|(identity, image)| feathrise_scene::ClipmapSurfaceCoverage { identity, image })
            })
            .flatten();
    } // 同一场景高度覆盖通道仲裁青面/绿迹，不新增平面和额外绘制。
    let transform = source
        .as_ref()
        .map_or(Transform::IDENTITY, |source| source.transform);
    if let Some(entity) = state.entity {
        if let Ok((mut visibility, mut mesh, mut pose)) = source_and_entities.p1().get_mut(entity) {
            let target = if show && !is_clipmap {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            }; // 选择取消即时收起。
            if *visibility != target {
                *visibility = target;
            } // 稳定帧不触发变化。
            if *pose != transform {
                *pose = transform;
            } // 源几何只应用一次完整对象姿态。
            if let Some(current) = source.as_ref().map(|source| &source.mesh)
                && mesh.0 != *current
            {
                mesh.0 = current.clone();
            } // 跟随新地形。
        } else {
            state.entity = None;
        } // 场景回收后重新建唯一实体。
    } else if show
        && !is_clipmap
        && let (Some(mesh), Some(material)) = (
            source.as_ref().map(|source| &source.mesh),
            state.material.as_ref(),
        )
    {
        state.entity = Some(
            commands
                .spawn((
                    Name::new("场景编辑器选中地表作者层"), // 世界诊断名称。
                    SurfaceSelectionMesh,                  // 独立辅助实体。
                    Mesh3d(mesh.clone()),                  // 共用地形UV与高度。
                    MeshMaterial3d(material.clone()),      // 同一透明材质。
                    transform,
                    Visibility::Inherited, // 当前有效选中。
                    RenderLayers::layer(super::SCENE_EDITOR_BRUSH_RENDER_LAYER), // F12专用31层。
                    NotShadowCaster,       // 不污染世界阴影。
                ))
                .id(),
        ); // 无额外相机与网格。
    }
}

fn build_overlay(
    indices: &[u8],
    weights: &[u8],
    source: [u32; 2],
    selected: usize,
) -> Result<(Image, bool), String> {
    let count = (source[0] as usize)
        .checked_mul(source[1] as usize)
        .ok_or("地表尺寸溢出")?; // 防御非法源。
    if count == 0
        || count.checked_mul(16) != Some(indices.len())
        || count.checked_mul(4) != Some(weights.len())
    {
        return Err("地表索引和权重尺寸不匹配".to_owned());
    } // 必须配对完整。
    let selected = u32::try_from(selected).map_err(|_| "作者层索引溢出")?; // 不截断局部索引。
    let scale = source[0].max(source[1]).div_ceil(512).max(1); // 最大512²，保持窄图比例。
    let output = [source[0].div_ceil(scale), source[1].div_ceil(scale)]; // 覆盖最后边界像素。
    let mut coverage = vec![0u8; output[0] as usize * output[1] as usize]; // 固定至多256KiB。
    for y in 0..source[1] {
        // 全部源只读取一次。
        for x in 0..source[0] {
            let pixel = (y as usize * source[0] as usize) + x as usize; // 避免u32乘法溢出。
            let mut contribution = 0u16; // 防御重复槽位贡献累加。
            for channel in 0..4 {
                // 正式top-four配对格式。
                let offset = pixel * 16 + channel * 4; // RGBA32Uint小端。
                let layer =
                    u32::from_le_bytes(indices[offset..offset + 4].try_into().expect("已验证长度")); // 绝不能按预设身份合并。
                if layer == selected {
                    contribution += weights[pixel * 4 + channel] as u16;
                } // 零权重槽不会产生覆盖。
            }
            let target = (y / scale) as usize * output[0] as usize + (x / scale) as usize; // 最大池化保留细窄地皮。
            coverage[target] = coverage[target].max(contribution.min(255) as u8); // 混合权重决定强度。
        }
    }
    let nonempty = coverage.iter().any(|value| *value > 0); // 空作者层不高亮。
    let pixels = coverage
        .into_iter()
        .flat_map(|weight| {
            [
                24,
                220,
                255,
                if weight == 0 {
                    0
                } else {
                    48 + (u16::from(weight) * 112 / 255) as u8
                },
            ]
        })
        .collect(); // 青色48..160透明度，明显且仍看见纹理。
    let mut image = Image::new(
        Extent3d {
            width: output[0],
            height: output[1],
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ); // 整张小图在后台构造。
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge, // 场景边缘不重复。
        address_mode_v: ImageAddressMode::ClampToEdge, // 场景边缘不重复。
        mag_filter: ImageFilterMode::Linear,           // 连续柔边。
        min_filter: ImageFilterMode::Linear,           // 缩小平滑。
        ..default()                                    // 单层纹理无需mip。
    });
    Ok((image, nonempty)) // 主线程只登记完整结果。
}

#[cfg(test)]
#[path = "surface_selection_tests.rs"]
mod tests; // 权重、身份与固定预算回归。
