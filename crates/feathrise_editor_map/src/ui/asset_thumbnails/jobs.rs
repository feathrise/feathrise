//! 复用一个离屏相机串行烘焙；根隔离、完整取景、GPU确认后一次性发布快照。

use super::{components::*, constants::*, resources::*, types::*};
use crate::ui::asset_preview::compatible_texture::{CompatiblePreviewTexture, flat_size, retry_capacity, sample_compatible};
use crate::components::{MapEditorAssetRow, MapEditorOwnedEntity};
use crate::resources::EditorAssetCatalog;
use crate::ui::{
    asset_preview::{frame_thumbnail_camera, include_transformed_aabb},
    sidebars::{EditorNavigatorTab, EditorSidebarTab, MapEditorSidebarPage},
    theme as colors,
};
use bevy::asset::{LoadState, RecursiveDependencyLoadState};
use bevy::camera::{
    Projection, RenderTarget,
    primitives::Aabb,
    visibility::{NoFrustumCulling, RenderLayers},
};
use bevy::core_pipeline::prepass::{DeferredPrepass, DepthPrepass};
use bevy::gltf::GltfAssetLabel;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::NoIndirectDrawing; // 至多256网格的小快照直接绘制，避免额外GPU剔除预热。
use bevy::world_serialization::{WorldAsset, WorldAssetRoot, WorldInstanceReady};
use feathrise_scene_core::view::DeferredRasterAuxiliaryCamera;
use feathrise_editor_core::constants::render_layers::MAP_EDITOR_ASSET_THUMBNAIL_LAYER;
use feathrise_map_content::MapAssetKind;
use std::sync::{Arc, atomic::Ordering};

fn stop_view(commands: &mut Commands, state: &ThumbnailState) {
    if let Some(camera) = state.camera {
        commands.entity(camera).insert((
            Camera {
                order: -5,
                is_active: false,
                clear_color: ClearColorConfig::Custom(colors::MODEL_PREVIEW_BG),
                ..default()
            },
            RenderTarget::None {
                size: UVec2::splat(IMAGE_SIZE),
            },
        )); // 保留目标组件让Bevy提取停用状态并撤销旧View；None不强持有最后一张可淘汰快照。
    }
    if let Some(light) = state.light {
        commands.entity(light).insert(Visibility::Hidden);
    }
}

fn retire(commands: &mut Commands, state: &mut ThumbnailState, job: ThumbnailJob, materials: &mut Assets<StandardMaterial>) {
    // 隐藏根不能阻止RenderApp重新准备材质；先断开我们拥有的图片采样，再进入分帧退役。
    if let Some(mut material) = job.owned_material.as_ref().and_then(|handle| materials.get_mut(handle)) {
        material.base_color_texture = None;
    }
    stop_view(commands, state);
    let stack = job.root.map(NodeCursor::new).into_iter().collect();
    if let Some(root) = job.root {
        commands.entity(root).insert(Visibility::Hidden);
    }
    state.retiring = Some(Retirement { stack, job });
    state.changed();
}

pub(super) fn ensure_view(commands: &mut Commands, state: &mut ThumbnailState) {
    if state.camera.is_none() {
        state.camera = Some(
            commands
                .spawn((
                    Name::new("资产缩略图串行相机"),
                    ThumbnailCamera,
                    MapEditorOwnedEntity,
                    DeferredRasterAuxiliaryCamera,
                    Camera3d::default(),
                    NoIndirectDrawing,
                    Msaa::Off,
                    DepthPrepass,
                    DeferredPrepass, // 从首帧固定兼容布局，不能在同一相机换图时再切MSAA/Deferred绑定。
                    Camera {
                        order: -5,
                        is_active: false,
                        clear_color: ClearColorConfig::Custom(colors::MODEL_PREVIEW_BG),
                        ..default()
                    },
                    RenderTarget::None {
                        size: UVec2::splat(IMAGE_SIZE),
                    },
                    Projection::Perspective(PerspectiveProjection::default()),
                    Transform::default(),
                    RenderLayers::layer(MAP_EDITOR_ASSET_THUMBNAIL_LAYER),
                ))
                .id(),
        );
        state.light = Some(
            commands
                .spawn((
                    Name::new("资产缩略图主光"),
                    MapEditorOwnedEntity,
                    DirectionalLight {
                        illuminance: 13_000.0,
                        shadow_maps_enabled: false,
                        ..default()
                    },
                    Transform::from_rotation(Quat::from_euler(EulerRot::YXZ, -0.75, -0.85, 0.0)),
                    Visibility::Hidden,
                    RenderLayers::layer(MAP_EDITOR_ASSET_THUMBNAIL_LAYER),
                ))
                .id(),
        );
    }
}

fn page_rect(
    pages: &Query<(&MapEditorSidebarPage, &ComputedNode, &UiGlobalTransform)>,
) -> Option<Rect> {
    pages.iter().find_map(|(page, computed, transform)| {
        (page.0 == EditorSidebarTab::Navigator(EditorNavigatorTab::Assets)
            && computed.size().min_element() > 0.0)
            .then(|| Rect::from_center_size(transform.translation, computed.size()))
    })
}

fn begin_render(commands: &mut Commands, state: &ThumbnailState, job: &mut ThumbnailJob) {
    let mut transform = Transform::default();
    let mut projection = Projection::Perspective(PerspectiveProjection::default());
    frame_thumbnail_camera(
        job.minimum,
        job.maximum,
        job.image.is_some(),
        &mut transform,
        &mut projection,
    );
    if let Some(camera) = state.camera {
        commands.entity(camera).insert((
            transform,
            projection,
            RenderTarget::from(job.target.clone()),
            Camera {
                order: -5,
                is_active: true,
                clear_color: ClearColorConfig::Custom(colors::MODEL_PREVIEW_BG),
                ..default()
            },
        ));
    }
    if let Some(root) = job.root {
        commands.entity(root).insert(Visibility::Inherited);
    }
    if let Some(light) = state.light {
        commands.entity(light).insert(Visibility::Inherited);
    }
    job.phase = JobPhase::Rendering;
}

/// 图片也烘焙为小RTT，缓存不会强持有原图的完整CPU像素/GPU贴图。
pub(super) fn drive_jobs(
    mut commands: Commands,
    server: Res<AssetServer>,
    catalog: Res<EditorAssetCatalog>,
    time: Res<Time<Real>>,
    mut state: ResMut<ThumbnailState>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    previews: Option<Res<Assets<CompatiblePreviewTexture>>>,
    rows: Query<(&MapEditorAssetRow, &ComputedNode, &UiGlobalTransform)>,
    pages: Query<(&MapEditorSidebarPage, &ComputedNode, &UiGlobalTransform)>,
) {
    if state.retiring.is_some() {
        return;
    } // 旧场景完整退役前不并发第二个模型。
    if let Some(mut job) = state.active.take() {
        if !state.job_current(&job) {
            retire(&mut commands, &mut state, job, &mut materials);
            return;
        }
        let failure = if time.elapsed_secs_f64() - job.started > TIMEOUT_SECONDS {
            Some("预览超时 · 可刷新重试".to_owned())
        } else if job
            .probes
            .iter()
            .any(|receipt| receipt.failed.load(Ordering::Acquire))
        {
            Some("渲染失败 · 可刷新重试".to_owned())
        } else if job.await_reload {
            None
        } else {
            let id = job
                .scene
                .as_ref()
                .map(|handle| handle.id().untyped())
                .or_else(|| job.image.as_ref().map(|handle| handle.id().untyped()));
            id.and_then(|id| server.get_load_states(id))
                .and_then(|(direct, _, recursive)| {
                    (matches!(direct, LoadState::Failed(_))
                        || matches!(recursive, RecursiveDependencyLoadState::Failed(_)))
                    .then(|| "加载失败 · 请选中查看详情".to_owned())
                })
        };
        if let Some(failure) = failure {
            if let Some(entry) = state.cache.get_mut(&job.path) {
                entry.result = ThumbnailResult::Failed(failure);
            }
            retire(&mut commands, &mut state, job, &mut materials);
            return;
        }
        if job.await_reload || job.refresh_after_load {
            state.active = Some(job);
            return;
        } // 已加载的旧源不能冒充本次磁盘换代。
        if let Some(handle) = &job.texture_preview {
            match server.load_state(handle.id()) {
                LoadState::Failed(error) => {
                    let diagnostic = error.to_string();
                    if retry_capacity(&server, handle, &diagnostic) {
                        state.active = Some(job);
                        return;
                    }
                    if let Some(entry) = state.cache.get_mut(&job.path) {
                        entry.result = ThumbnailResult::Failed(format!("预览失败 · {diagnostic}"));
                    }
                    retire(&mut commands, &mut state, job, &mut materials);
                    return;
                }
                LoadState::Loaded => {
                    if let Some(preview) = previews.as_ref().and_then(|assets| assets.get(handle))
                        && let Some(failure) = &preview.failure
                    {
                        if let Some(entry) = state.cache.get_mut(&job.path) {
                            entry.result = ThumbnailResult::Failed(format!("{} · {failure}", preview.description));
                        }
                        retire(&mut commands, &mut state, job, &mut materials);
                        return;
                    }
                },
                _ if job.phase == JobPhase::Loading => {
                    state.active = Some(job);
                    return;
                }
                _ => {
                    if let Some(entry) = state.cache.get_mut(&job.path) { entry.result = ThumbnailResult::Queued; }
                    retire(&mut commands, &mut state, job, &mut materials);
                    return;
                }
            }
        }
        if job.image.is_some() && job.phase != JobPhase::Loading {
            let compatible = job.owned_material.as_ref().and_then(|handle| materials.get(handle))
                .and_then(|material| material.base_color_texture.as_ref())
                .and_then(|handle| images.get(handle)).is_some_and(sample_compatible);
            if !compatible {
                if let Some(entry) = state.cache.get_mut(&job.path) { entry.result = ThumbnailResult::Queued; }
                retire(&mut commands, &mut state, job, &mut materials);
                return;
            }
        }
        if job.phase == JobPhase::RevealPending {
            // Update启用，随后CameraUpdate与聚簇灯光才能看到正确尺寸；不能在PostUpdate末尾激活零尺寸View。
            begin_render(&mut commands, &state, &mut job);
            state.active = Some(job);
            return;
        }
        if job.phase == JobPhase::Rendering
            && !job.probes.is_empty()
            && job
                .probes
                .iter()
                .all(|receipt| receipt.ready.load(Ordering::Acquire))
        {
            if let Some(entry) = state.cache.get_mut(&job.path) {
                entry.result = ThumbnailResult::Ready(job.target.clone()); // 所有网格和真实GPU绘制完整后才发布。
            }
            retire(&mut commands, &mut state, job, &mut materials);
            return;
        }
        if job.phase == JobPhase::Loading
            && let Some(preview) = job.texture_preview.as_ref().and_then(|handle| previews.as_ref().and_then(|assets| assets.get(handle)))
            && let Some(image) = images.get(&preview.image).filter(|image| sample_compatible(image))
        {
            match flat_image_size(image) {
                Err(failure) => {
                    state.cache.get_mut(&job.path).unwrap().result =
                        ThumbnailResult::Failed(failure);
                    retire(&mut commands, &mut state, job, &mut materials);
                    return;
                }
                Ok(size) => {
                    job.image = Some(preview.image.clone()); // 只持有私有安全输出，不再额外加载原图副本。
                    let size = size / size.max_element(); // 不拉伸源图，方形画布保留留边。
                    let mesh = meshes.add(Rectangle::new(size.x, size.y));
                    let material = materials.add(StandardMaterial {
                        base_color_texture: Some(preview.image.clone()),
                        unlit: true,
                        alpha_mode: AlphaMode::Blend,
                        cull_mode: None,
                        ..default()
                    });
                    let root = commands
                        .spawn((
                            ThumbnailRoot,
                            MapEditorOwnedEntity,
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::default(),
                            Visibility::Hidden,
                            RenderLayers::layer(MAP_EDITOR_ASSET_THUMBNAIL_LAYER),
                        ))
                        .id();
                    job.minimum = Vec3::new(-size.x * 0.5, -size.y * 0.5, 0.0);
                    job.maximum = -job.minimum;
                    job.owned_mesh = Some(mesh.clone());
                    job.owned_material = Some(material.clone());
                    job.root = Some(root);
                    attach_probe(&mut commands, &mut job, root, mesh, material);
                    job.phase = JobPhase::RevealPending;
                }
            }
        }
        if job.phase == JobPhase::Loading
            && job.root.is_none()
            && let Some(scene) = &job.scene
        {
            job.root = Some(
                commands
                    .spawn((
                        Name::new("资产缩略图Scene0"),
                        ThumbnailRoot,
                        MapEditorOwnedEntity,
                        WorldAssetRoot(scene.clone()),
                        Transform::default(),
                        Visibility::Hidden,
                        RenderLayers::layer(MAP_EDITOR_ASSET_THUMBNAIL_LAYER),
                    ))
                    .id(),
            ); // 重载完成后才实例化，避免对旧Scene0取景。
        }
        state.active = Some(job);
        return;
    }
    let Some(identity) = &state.identity else {
        return;
    };
    let Some(rect) = page_rect(&pages) else {
        return;
    };
    let selected = rows
        .iter()
        .filter_map(|(row, computed, transform)| {
            let row_rect = Rect::from_center_size(transform.translation, computed.size());
            if computed.size().min_element() <= 0.0
                || row_rect.intersect(rect).size().min_element() <= 0.0
            {
                return None;
            }
            let index = row.entry_index?;
            // Tasks先于Presentation；换页首帧旧行尚未重绑定，不能启动上一页的模型。
            if index != identity.start + row.slot || row.slot >= identity.count {
                return None;
            }
            let entry = catalog.entries.get(index)?;
            if !state.paths.contains(&entry.asset_path) {
                return None;
            }
            let source = state
                .cache
                .get(&entry.asset_path)
                .filter(|entry| matches!(entry.result, ThumbnailResult::Queued))?
                .source
                .as_ref()
                .ok()?;
            Some((row.slot, entry.asset_path.clone(), entry.kind, source.stamp))
        })
        .min_by_key(|(slot, ..)| *slot);
    let Some((_, path, kind, stamp)) = selected else {
        return;
    }; // 完全滚出资产页的槽位不生成三维View。
    ensure_view(&mut commands, &mut state);
    let target = images.add(Image::new_target_texture(
        IMAGE_SIZE,
        IMAGE_SIZE,
        TextureFormat::Bgra8UnormSrgb,
        None,
    ));
    let mut job = ThumbnailJob {
        epoch: state.epoch,
        path: path.clone(),
        stamp,
        started: time.elapsed_secs_f64(),
        phase: JobPhase::Loading,
        target,
        camera: state.camera.unwrap(),
        await_reload: false,
        refresh_after_load: false,
        root: None,
        scene: None,
        image: None,
        texture_preview: None,
        owned_mesh: None,
        owned_material: None,
        stack: Vec::new(),
        nodes: 0,
        minimum: Vec3::splat(f32::INFINITY),
        maximum: Vec3::splat(f32::NEG_INFINITY),
        probes: Vec::new(),
    };
    let reload = state.reload_paths.remove(&path);
    if kind == MapAssetKind::Scene {
        let asset_path = GltfAssetLabel::Scene(0).from_asset(path);
        let existing = server.get_handle::<WorldAsset>(asset_path.clone());
        job.await_reload = reload && existing.is_some();
        job.scene = Some(existing.unwrap_or_else(|| server.load(asset_path.clone())));
        if job.await_reload {
            server.reload(asset_path);
        }
    } else {
        let existing = server.get_handle::<CompatiblePreviewTexture>(path.clone());
        job.refresh_after_load = reload && existing.is_some(); // 所有旧句柄都先跨过下一轮事件冲刷，再派发显式刷新。
        job.await_reload = job.refresh_after_load && existing.as_ref().is_some_and(|handle| {
            !matches!(server.load_state(handle.id()), LoadState::Loaded)
                || !previews.as_ref().is_some_and(|assets| assets.contains(handle.id()))
        }); // refresh=true/await=false是warm冲刷；true/true必须等旧在途发布回执。
        job.image = Some(Handle::default()); // 保留图片工作项身份；唯一解码请求由私有父资产持有。
        job.texture_preview = Some(existing.unwrap_or_else(|| server.load(path.clone())));
    }
    state.active = Some(job);
    state.changed();
}

pub(super) fn poll_reloaded_assets(
    mut images: feathrise_scene_core::residency::ResidentAssetEventReader<Image>,
    mut scenes: feathrise_scene_core::residency::ResidentAssetEventReader<WorldAsset>,
    mut previews: MessageReader<AssetEvent<CompatiblePreviewTexture>>,
    server: Res<AssetServer>,
    preview_assets: Option<Res<Assets<CompatiblePreviewTexture>>>,
    mut state: ResMut<ThumbnailState>,
) {
    // 无任务也排空消息，下一次点击不能消费之前的旧完成事件。
    for event in images.read() {
        if let Some(job) = state.active.as_mut().filter(|job| job.await_reload && job.texture_preview.is_none())
            && let Some(handle) = &job.image
            && matches!(event, AssetEvent::Added { id } | AssetEvent::Modified { id } | AssetEvent::LoadedWithDependencies { id } if *id == handle.id())
        {
            job.await_reload = false;
        }
    }
    for event in scenes.read() {
        if let Some(job) = state.active.as_mut().filter(|job| job.await_reload)
            && let Some(handle) = &job.scene
            && matches!(event, AssetEvent::Added { id } | AssetEvent::Modified { id } | AssetEvent::LoadedWithDependencies { id } if *id == handle.id())
        {
            job.await_reload = false;
        }
    }
    let mut initial_published = false;
    for event in previews.read() {
        if let Some(job) = state.active.as_mut().filter(|job| job.await_reload)
            && let Some(handle) = &job.texture_preview
        {
            if job.refresh_after_load {
                initial_published |= matches!(event, AssetEvent::Added { id } | AssetEvent::Modified { id } if *id == handle.id());
            } else if matches!(event, AssetEvent::Modified { id } if *id == handle.id()) {
                job.await_reload = false;
            }
        }
    }
    // 必须排空旧发布的整批回执之后再派发；Update看见Assets时Added往往尚未经过PostUpdate。
    if let Some(job) = state.active.as_mut().filter(|job| job.refresh_after_load)
        && let Some(handle) = &job.texture_preview
    {
        match server.load_state(handle.id()) {
            LoadState::Failed(_) | LoadState::NotLoaded => {
                job.refresh_after_load = false;
                job.await_reload = false;
                job.texture_preview = Some(server.load(job.path.clone())); // Failed→Loading同步发生；旧有效值也不能绕过这次加载状态门。
            }
            LoadState::Loaded if preview_assets.as_ref().is_some_and(|assets| assets.contains(handle.id())) => {
                if initial_published || !job.await_reload {
                    job.refresh_after_load = false;
                    job.await_reload = true;
                    server.reload(job.path.clone());
                } // cold必须消费旧发布回执；warm已经跨过创建工作项之后的PostUpdate并排空事件。
            }
            LoadState::Loading => { job.await_reload = true; } // 共享父可能被另一可见客户端从Failed同步重试，转为cold回执门。
            _ => {}
        }
    }
    if let Some(job) = state.active.as_mut().filter(|job| job.await_reload)
        && let Some(handle) = &job.texture_preview
        && matches!(server.load_state(handle.id()), LoadState::Failed(_))
    { job.await_reload = false; } // 真失败交给现有失败/容量重试分支，不等到30秒超时。
}

pub(super) fn flat_image_size(image: &Image) -> Result<Vec2, String> {
    let size = flat_size(image)?;
    let descriptor = &image.texture_descriptor;
    if descriptor.size.width == 0
        || descriptor.size.height == 0
        || u64::from(descriptor.size.width) * u64::from(descriptor.size.height) > MAX_IMAGE_PIXELS
    {
        return Err("图片尺寸无效或过大".into());
    }
    Ok(size)
}

fn attach_probe(
    commands: &mut Commands,
    job: &mut ThumbnailJob,
    entity: Entity,
    mesh: Handle<Mesh>,
    material: Handle<StandardMaterial>,
) {
    let receipt = Arc::new(RenderReceipt::default());
    commands.entity(entity).insert((
        NoFrustumCulling,
        NotShadowCaster,
        ThumbnailProbe {
            camera: job.camera,
            target: job.target.clone(),
            mesh,
            material,
            receipt: receipt.clone(),
        },
    ));
    job.probes.push(receipt);
}

/// 只响应当前根；立即关闭模型自带相机，层级深度已由后台限制到64。
pub(super) fn world_ready(
    ready: On<WorldInstanceReady>,
    mut state: ResMut<ThumbnailState>,
    mut roots: Query<&mut Visibility, With<ThumbnailRoot>>,
    parents: Query<&ChildOf>,
    mut cameras: Query<(Entity, &mut Camera), Without<ThumbnailCamera>>,
) {
    let Some(job) = state
        .active
        .as_mut()
        .filter(|job| job.root == Some(ready.entity))
    else {
        return;
    };
    if let Ok(mut visibility) = roots.get_mut(ready.entity) {
        *visibility = Visibility::Hidden;
    }
    for (entity, mut camera) in &mut cameras {
        let mut ancestor = entity;
        for _ in 0..66 {
            let Ok(parent) = parents.get(ancestor) else {
                break;
            };
            ancestor = parent.parent();
            if ancestor == ready.entity {
                camera.is_active = false;
                break;
            }
        }
    }
    job.phase = JobPhase::Scanning;
    job.stack = vec![NodeCursor::new(ready.entity)];
    job.nodes = 0;
    job.probes.clear();
    job.minimum = Vec3::splat(f32::INFINITY);
    job.maximum = Vec3::splat(f32::NEG_INFINITY);
}

pub(super) fn advance_scene(
    mut commands: Commands,
    mut state: ResMut<ThumbnailState>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    children: Query<&Children>,
    visibility: Query<&Visibility>,
    geometry: Query<(
        &GlobalTransform,
        Option<&Aabb>,
        &Mesh3d,
        &MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let Some(mut job) = state.active.take() else {
        return;
    };
    if !state.job_current(&job) {
        retire(&mut commands, &mut state, job, &mut materials);
        return;
    }
    let mut failure = None;
    if job.phase == JobPhase::Scanning {
        for _ in 0..NODE_STEPS_PER_FRAME {
            let Some(cursor) = job.stack.last_mut() else {
                break;
            };
            if !cursor.entered {
                if job.root != Some(cursor.entity) {
                    cursor.hidden |= visibility
                        .get(cursor.entity)
                        .is_ok_and(|value| *value == Visibility::Hidden);
                }
                if !cursor.hidden
                    && let Ok((transform, bounds, mesh, material)) = geometry.get(cursor.entity)
                {
                    let Some(bounds) = bounds else {
                        break;
                    }; // CPU资产就绪不能代替实际Aabb，非阻塞等待后续帧。
                    if job.probes.len() >= MAX_MESHES {
                        failure = Some("模型较大，请选中后预览");
                        break;
                    }
                    include_transformed_aabb(&mut job.minimum, &mut job.maximum, transform, bounds);
                    let entity = cursor.entity;
                    attach_probe(
                        &mut commands,
                        &mut job,
                        entity,
                        mesh.0.clone(),
                        material.0.clone(),
                    );
                }
                let cursor = job.stack.last_mut().unwrap();
                cursor.entered = true;
                job.nodes += 1;
                if job.nodes > MAX_NODES {
                    failure = Some("模型较大，请选中后预览");
                    break;
                }
                commands
                    .entity(cursor.entity)
                    .insert(RenderLayers::layer(MAP_EDITOR_ASSET_THUMBNAIL_LAYER));
            } else if let Some(child) = children
                .get(cursor.entity)
                .ok()
                .and_then(|children| children.get(cursor.child))
                .copied()
            {
                cursor.child += 1;
                let mut next = NodeCursor::new(child);
                next.hidden = cursor.hidden;
                job.stack.push(next);
            } else {
                job.stack.pop();
            }
        }
        if job.stack.is_empty() && failure.is_none() {
            if job.probes.is_empty() || !job.minimum.is_finite() || !job.maximum.is_finite() {
                failure = Some("无静态网格 · 双击查看"); // 纯动画仍能在正式预览中播放和编辑，缩略图缺Mesh不代表资源不可用。
            } else {
                job.phase = JobPhase::RevealPending;
            }
            state.active = Some(job); // 隔离命令须先落地一帧，不能同帧显示默认层后代。
            if failure.is_none() {
                return;
            }
            job = state.active.take().unwrap();
        }
    }
    if let Some(failure) = failure {
        state.cache.get_mut(&job.path).unwrap().result =
            ThumbnailResult::Failed(failure.to_owned());
        retire(&mut commands, &mut state, job, &mut materials);
    } else {
        state.active = Some(job);
    }
}

/// 后序逐节点移除，禁止根despawn触发整棵大型场景的同步递归析构。
pub(super) fn retire_nodes(
    mut commands: Commands,
    mut state: ResMut<ThumbnailState>,
    children: Query<&Children>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(retiring) = state.retiring.as_mut() else {
        return;
    };
    if let Some(mut material) = retiring.job.owned_material.as_ref().and_then(|handle| materials.get_mut(handle)) {
        material.base_color_texture = None; // 防御测试/旧会话直接构造的退役队列，也不允许其继续持有源采样。
    }
    for _ in 0..NODE_STEPS_PER_FRAME {
        let Some(cursor) = retiring.stack.last_mut() else {
            break;
        };
        let descendants = children.get(cursor.entity).ok();
        let count = descendants.map_or(0, |children| children.len());
        if !cursor.entered {
            cursor.child = count;
            cursor.entered = true;
        }
        cursor.child = cursor.child.min(count);
        if cursor.child > 0 {
            cursor.child -= 1;
            let child = descendants.unwrap()[cursor.child];
            // 从尾部删除使跨帧Children收缩不改变剩余下标，不跳过兄弟再递归删根。
            retiring.stack.push(NodeCursor::new(child));
        } else {
            let entity = cursor.entity;
            retiring.stack.pop();
            commands.entity(entity).try_despawn(); // 子树已逐个为空，linked-spawn只处理一个小节点。
        }
    }
    if retiring.stack.is_empty() {
        let retiring = state.retiring.take().unwrap();
        if let Some(mesh) = retiring.job.owned_mesh {
            meshes.remove(mesh.id());
        }
        if let Some(material) = retiring.job.owned_material {
            materials.remove(material.id());
        }
    }
}
