//! 后台加载并在源坐标刷印各成员；真实表面与遮挡过滤后才登记惰性作者层。
use super::super::{
    paint::{interpolated_dab_count, brush_path_dab_count, MAX_INTERPOLATED_DABS_PER_FRAME, paint_density_neighborhood_with_flow},
    terrain_documents::{TerrainCanvas, load_vegetation_canvas},
    vegetation_compile::compile_vegetation_authoring,
};
use super::types::*;
use bevy::{prelude::*, tasks::AsyncComputeTaskPool};
use feathrise_scene_content::*;
use feathrise_scene::{ClipmapGroundHit, TerrainObjectRaySnapshot};
use std::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};

const WORKING_BYTES: usize = 256 * 1024 * 1024;

pub(super) async fn load_and_paint(
    mut transaction: Transaction,
    targets: Vec<Target>,
    dabs: VecDeque<Dab>,
) -> Result<Batch, String> {
    let pixels = transaction
        .group
        .template
        .resolution
        .into_iter()
        .map(|v| v as usize)
        .product::<usize>();
    if pixels
        .saturating_mul(12)
        .saturating_mul(targets.len() + transaction.documents.len())
        > WORKING_BYTES
    {
        return Err("跨网格目标预计像素工作集超过256MiB".into());
    }
    // IO顺序准备，最多一个加载任务；它等待既有CPU几何派生时不占用CPU worker。
    for target in targets {
        let id = target.identity.definition.id.clone();
        let canvas = load_vegetation_canvas(target.identity, target.cached, target.saved)?;
        transaction.documents.insert(id, Arc::new(canvas));
    }
    AsyncComputeTaskPool::get()
        .spawn(async move { paint(transaction, dabs) })
        .await
}

fn check_budget(transaction: &Transaction) -> Result<(), String> {
    let mut bytes = 0_usize;
    for canvas in transaction.documents.values() {
        // 两代作者、编译/冲突浮点工作区和图片/CPU来源留出保守倍数；不按网格数量截断。
        for pixels in canvas
            .vegetation_author_densities
            .values()
            .chain(canvas.vegetation_densities.values())
        {
            bytes = bytes.saturating_add(pixels.len().saturating_mul(8));
        }
        let resolution = transaction.group.template.resolution;
        bytes = bytes.saturating_add(
            (resolution[0] as usize)
                .saturating_mul(resolution[1] as usize)
                .saturating_mul(12),
        );
    }
    if bytes > WORKING_BYTES {
        Err("本笔跨网格植被工作集超过256MiB，请减少绘制范围或画布分辨率".into())
    } else {
        Ok(())
    }
}

pub(super) fn paint(mut transaction: Transaction, dabs: VecDeque<Dab>) -> Result<Batch, String> {
    check_budget(&transaction)?;
    let baseline = transaction.clone();
    let original = dabs.clone(); // 最多64份标量描述；真实缺页时退回整批，不发布半套跨网格结果。
    let (dabs, remaining) = take_prefix(dabs);
    let mut waiting = false;
    let mut changed = Vec::new();
    for (id, source) in &mut transaction.documents {
        let mut canvas = (**source).clone();
        let identity = canvas
            .identity
            .as_ref()
            .ok_or("跨网格画布缺少来源身份")?
            .clone();
        let object = &identity.definition;
        if !object.enabled || object.terrain.kind == SceneTerrainKind::Voxel {
            continue;
        }
        let Some(query) = transaction.ground.object_query(id) else {
            continue;
        };
        let before = VegetationValues::capture(&canvas);
        let layer_id = match transaction.group.members.get(id) {
            Some(member) => member.clone(),
            None if dabs.iter().all(|dab| dab.erase) => continue, // 擦空处不创建作者层。
            None => create_member(
                &mut canvas,
                &transaction.group,
                &identity.folder,
                &transaction.scene_id,
            )?,
        };
        let layer = canvas
            .vegetation
            .as_ref()
            .and_then(|v| v.authoring.as_ref())
            .and_then(|a| a.layers.iter().find(|l| l.id == layer_id))
            .ok_or("绘制组成员作者层已删除，请重建绘制组")?
            .clone();
        if !layer.enabled || layer.locked {
            continue;
        }
        let density = canvas
            .vegetation_author_densities
            .get_mut(&layer_id)
            .ok_or("绘制组作者密度尚未就绪")?;
        let density = Arc::make_mut(density); // 整图写侧分离仅发生在worker，绝不堵塞输入帧。
        let transform = object.transform.as_transform().to_matrix();
        let inverse = transform.inverse();
        let scale = Vec3::from_array(object.transform.scale);
        let min = [object.world_min[0] * scale.x, object.world_min[1] * scale.z];
        let max = [object.world_max[0] * scale.x, object.world_max[1] * scale.z];
        let top = query.mesh_bounds().map_or_else(
            || {
                transform
                    .transform_point3(Vec3::new(
                        0.,
                        object.terrain.base_height + object.terrain.height_scale,
                        0.,
                    ))
                    .y
            },
            |(_, max)| max.y,
        ) + 1.;
        let bottom = transform
            .transform_point3(Vec3::new(0., object.terrain.base_height, 0.))
            .y
            - object.terrain.height_scale.abs() * scale.y
            - 1.;
        let mut any = false;
        for dab in &dabs {
            let count = interpolated_dab_count(dab.from, dab.to, dab.radius);
            for step in 1..=count {
                let center = dab.from.lerp(dab.to, step as f32 / count as f32);
                let local = inverse.transform_point3(center) * scale;
                let mut edits = BTreeMap::new();
                paint_density_neighborhood_with_flow(
                    density,
                    layer.resolution,
                    min,
                    max,
                    local,
                    dab.radius,
                    dab.erase,
                    dab.amount,
                    dab.exposure,
                    &mut edits,
                );
                for (index, old) in edits {
                    let x = index as u32 % layer.resolution[0];
                    let z = index as u32 / layer.resolution[0];
                    let uv = Vec2::new(
                        x as f32 / (layer.resolution[0] - 1) as f32,
                        z as f32 / (layer.resolution[1] - 1) as f32,
                    );
                    let point = transform.transform_point3(Vec3::new(
                        object.world_min[0] + (object.world_max[0] - object.world_min[0]) * uv.x,
                        0.,
                        object.world_min[1] + (object.world_max[1] - object.world_min[1]) * uv.y,
                    ));
                    match eligible_surface(
                        &transaction.ground,
                        &transaction.hidden,
                        id,
                        &query,
                        point,
                        top,
                        bottom,
                        center,
                        dab,
                    ) {
                        SurfaceEligibility::Paint => any = true,
                        SurfaceEligibility::Skip => density[index] = old,
                        SurfaceEligibility::Waiting => {
                            waiting = true;
                            density[index] = old;
                        }
                        SurfaceEligibility::Failed => {
                            return Err("跨网格真实地形高度查询失败，本批刷段未采用".into());
                        }
                    }
                }
            }
        }
        if !any {
            continue;
        } // 整个局部候选丢弃，未命中不留下空成员或正式输出。
        let definition = canvas.vegetation.as_ref().ok_or("植被定义丢失")?;
        let compiled = compile_vegetation_authoring(
            definition,
            &canvas.vegetation_author_densities,
            object.world_min,
            object.world_max,
        )?;
        canvas.vegetation_densities = compiled.outputs;
        canvas.vegetation_nonzero_layers = compiled.nonzero_output_ids;
        canvas.vegetation_density_stats = compiled.output_stats;
        canvas.vegetation_conflicts = compiled.conflicts;
        canvas
            .vegetation_author_dirty_layers
            .insert(layer_id.clone());
        canvas
            .vegetation_dirty_layers
            .extend(canvas.vegetation_densities.keys().cloned());
        canvas.structural_dirty = true;
        transaction.before.entry(id.clone()).or_insert(before);
        transaction.changed.insert(id.clone());
        transaction.group.members.insert(id.clone(), layer_id);
        *source = Arc::new(canvas);
        changed.push(id.clone());
    }
    if waiting {
        return Ok(Batch {
            transaction: baseline,
            prepared: Vec::new(),
            waiting: original,
            remaining: VecDeque::new(),
        });
    }
    let prepared = changed
        .iter()
        .map(|id| prepare_images(id, &transaction.documents[id]))
        .collect::<Result<_, _>>()?;
    Ok(Batch {
        transaction,
        prepared,
        waiting: VecDeque::new(),
        remaining,
    })
}

/// 后台也沿刷宽处理固定前缀；预算只延后真实轨迹，不改变间距或丢弃长段尾部。
fn take_prefix(mut dabs: VecDeque<Dab>) -> (VecDeque<Dab>, VecDeque<Dab>) {
    let mut prefix = VecDeque::new();
    let mut budget = MAX_INTERPOLATED_DABS_PER_FRAME as usize;
    while budget > 0 {
        let Some(dab) = dabs.pop_front() else { break; };
        let count = brush_path_dab_count(dab.from, dab.to, dab.radius);
        let used = count.min(budget);
        let endpoint = dab.from.lerp(dab.to, used as f32 / count as f32);
        prefix.push_back(Dab { to: endpoint, ..dab.clone() });
        budget -= used;
        if used < count {
            dabs.push_front(Dab { from: endpoint, ..dab });
            break;
        }
    }
    (prefix, dabs)
}

#[allow(clippy::too_many_arguments)]
fn eligible_surface(
    ground: &super::super::terrain_ground::TerrainGroundSnapshot,
    hidden: &std::collections::BTreeSet<TerrainId>,
    id: &TerrainId,
    query: &TerrainObjectRaySnapshot,
    point: Vec3,
    top: f32,
    bottom: f32,
    center: Vec3,
    dab: &Dab,
) -> SurfaceEligibility {
    use SurfaceEligibility::*;
    let hit = match query
        .support_below_status(Vec3::new(point.x, top, point.z), (top - bottom).max(1.))
    {
        ClipmapGroundHit::Hit(hit) => hit,
        ClipmapGroundHit::WaitingForHeight => return Waiting,
        ClipmapGroundHit::Failed => return Failed,
        _ => return Skip,
    };
    if (hit.y - center.y).abs() > dab.radius {
        return Skip;
    }
    if let TerrainObjectRaySnapshot::Clipmap(snapshot, frame) = query {
        let local = frame.to_local(hit);
        if snapshot.is_three_dimensional_surface(local) {
            return Skip;
        }
        match snapshot.sample_authoritative_surface(local.xz()) {
            ClipmapGroundHit::Hit(p) if p.distance(local) < 0.05 => {}
            ClipmapGroundHit::WaitingForHeight => return Waiting,
            ClipmapGroundHit::Failed => return Failed,
            _ => return Skip,
        }
    }
    let camera = dab.camera.w_axis.truncate();
    let origin = if dab.orthographic {
        let direction = dab.camera.transform_vector3(Vec3::NEG_Z).normalize();
        hit - direction * ((hit - camera).dot(direction).max(0.) + 0.1)
    } else {
        camera
    };
    let Some(direction) = (hit - origin).try_normalize() else {
        return Skip;
    };
    match ground.ray_visible_checked(origin, direction, hidden) {
        Ok(Some((owner, point))) if owner == *id && point.distance(hit) < 0.05 => Paint,
        Err(ClipmapGroundHit::WaitingForHeight) => Waiting,
        Err(_) => Failed,
        _ => Skip,
    }
}

fn create_member(
    canvas: &mut TerrainCanvas,
    group: &SceneVegetationPaintGroup,
    folder: &str,
    map: &SceneId,
) -> Result<String, String> {
    let vegetation = canvas.vegetation.get_or_insert_with(|| SceneVegetationAsset {
        schema_version: SCENE_SCHEMA_VERSION,
        scene_id: map.clone(),
        layers: Vec::new(),
        authoring: Some(super::super::vegetation_defaults::empty_vegetation_authoring()),
    });
    if super::super::vegetation_defaults::ensure_vegetation_authoring(vegetation, folder) {
        for layer in &vegetation.authoring.as_ref().unwrap().layers {
            let pixels = canvas
                .vegetation_densities
                .get(&layer.output_layer_id)
                .ok_or("旧植被正式密度缺失")?
                .clone();
            canvas
                .vegetation_author_densities
                .insert(layer.id.clone(), pixels);
        }
    }
    if let Some(existing) = vegetation.layers.iter().find(|l| l.id == group.template.id) {
        let mut expected = group.template.clone();
        expected.label = existing.label.clone();
        expected.density_source_path = existing.density_source_path.clone();
        expected.density_texture_path = existing.density_texture_path.clone();
        if *existing != expected {
            return Err("目标网格同ID正式输出规格与绘制组预设不同，请选择一致预设".into());
        }
    } else {
        if vegetation.layers.iter().any(|l| {
            group.template.runtime_layer_id.is_some()
                && l.runtime_layer_id == group.template.runtime_layer_id
                || group.template.prototype_id.is_some()
                    && l.prototype_id == group.template.prototype_id
        }) {
            return Err("目标网格已有同运行时层或原型的其他输出ID，不能重复注册".into());
        }
        let mut output = group.template.clone();
        output.density_source_path = format!("{folder}/vegetation/{}.r8", output.id);
        output.density_texture_path = format!("{folder}/vegetation/{}.ktx2", output.id);
        vegetation.layers.push(output);
    }
    let authoring = vegetation.authoring.as_mut().unwrap();
    let id = format!("paint-{}", group.id);
    if authoring.layers.iter().any(|l| l.id == id) {
        return Err("绘制组成员身份冲突，不能自动合并已有作者层".into());
    }
    if authoring.groups.iter().any(|g| g.id == id) {
        return Err("绘制组的本场景层组身份已被占用".into());
    }
    authoring.groups.push(SceneVegetationAuthorGroup {
        id: id.clone(),
        label: group.label.clone(),
        mode: SceneVegetationAuthorGroupMode::Additive,
        output_merge: SceneVegetationOutputMergeMode::Maximum,
        capacity: 1.,
        claim_threshold: 0.,
        selection_seed: group.template.seed,
        selection_cell_meters: 8.,
        enabled: true,
    });
    // 独立的本地共存组不继承恰好排在首位的隐藏/覆盖规则，也不重排已有作者图层。
    let resolution = group.template.resolution;
    if resolution.iter().any(|v| *v < 2) {
        return Err("跨网格画布分辨率至少为2×2".into());
    }
    authoring.layers.push(SceneVegetationAuthorLayer {
        id: id.clone(),
        label: group.label.clone(),
        group_id: id.clone(),
        output_layer_id: group.template.id.clone(),
        density_source_path: format!("{folder}/vegetation/author/{id}.r8"),
        resolution,
        enabled: true,
        locked: false,
        opacity: 1.,
        priority: 0,
        weight: 1.,
    });
    canvas.vegetation_author_densities.insert(
        id.clone(),
        Arc::new(vec![0; resolution[0] as usize * resolution[1] as usize]),
    );
    Ok(id)
}

pub(super) fn prepare_images(
    id: &TerrainId,
    canvas: &TerrainCanvas,
) -> Result<PreparedCanvas, String> {
    use bevy::{
        asset::RenderAssetUsages,
        image::ImageSampler,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };
    let map = canvas
        .identity
        .as_ref()
        .ok_or("预览缺少画布身份")?
        .manifest
        .id
        .clone();
    let definition = canvas.vegetation.clone().unwrap_or(SceneVegetationAsset {
        scene_id: map,
        schema_version: SCENE_SCHEMA_VERSION,
        ..Default::default()
    });
    let mut result = PreparedCanvas {
        id: id.clone(),
        definition,
        images: VecDeque::new(),
        handles: BTreeMap::new(),
        sources: BTreeMap::new(),
        nonzero: canvas.vegetation_nonzero_layers.clone(),
    };
    for layer in &result.definition.layers {
        let pixels = canvas
            .vegetation_densities
            .get(&layer.id)
            .ok_or("跨网格预览缺少正式密度")?;
        if pixels.len() != layer.resolution[0] as usize * layer.resolution[1] as usize {
            return Err("跨网格密度尺寸不匹配".into());
        }
        let mut image = Image::new(
            Extent3d {
                width: layer.resolution[0],
                height: layer.resolution[1],
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            pixels.as_ref().clone(),
            TextureFormat::R8Unorm,
            RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = ImageSampler::nearest();
        result.images.push_back((layer.id.clone(), image));
        result
            .sources
            .insert(layer.id.clone(), Arc::from(pixels.as_slice()));
    }
    Ok(result)
}
