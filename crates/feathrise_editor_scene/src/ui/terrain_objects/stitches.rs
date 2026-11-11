//! 单飞拼接准备：CPU选择完整边，IO仅预取参考边的有限作者页，最终通过原集合事务统一采用。
use bevy::tasks::{AsyncComputeTaskPool, IoTaskPool};
use feathrise_scene_content::*;
use std::{collections::BTreeSet, sync::Arc};

pub(super) async fn prepare(
    map: SceneId,
    objects: Arc<[SceneTerrainObject]>,
    id: TerrainId,
    blocked: BTreeSet<TerrainId>,
    serial: u64,
) -> Result<Vec<SceneTerrainObject>, String> {
    prepare_at(
        crate::authoring::workspace_asset_root(),
        map,
        objects,
        id,
        blocked,
        serial,
    )
    .await
}

pub(super) async fn prepare_at(
    asset_root: std::path::PathBuf,
    map: SceneId,
    objects: Arc<[SceneTerrainObject]>,
    id: TerrainId,
    blocked: BTreeSet<TerrainId>,
    serial: u64,
) -> Result<Vec<SceneTerrainObject>, String> {
    let (mut records, index, peer, edge, peer_edge, pose) = AsyncComputeTaskPool::get()
        .spawn(async move {
            if blocked.contains(&id) {
                return Err("所选网格已隐藏或锁定".to_string());
            }
            let index = objects
                .iter()
                .position(|o| o.id == id)
                .ok_or("所选网格已失效")?;
            let eligible: Vec<_> = objects
                .iter()
                .filter(|o| !blocked.contains(&o.id))
                .cloned()
                .collect();
            let (peer, edge, peer_edge, pose) =
                nearest_clipmap_stitch_pose(&objects[index], &eligible)?;
            let peer_id = &eligible[peer].id;
            let peer = objects
                .iter()
                .position(|o| &o.id == peer_id)
                .ok_or("参考网格已失效")?;
            if objects[index]
                .clipmap_stitches
                .iter()
                .any(|s| s.edge == edge)
                || objects[peer]
                    .clipmap_stitches
                    .iter()
                    .any(|s| s.edge == peer_edge)
            {
                return Err("所选边已有拼接关系，请先解除或恢复原位".to_string());
            }
            Ok::<_, String>((objects.to_vec(), index, peer, edge, peer_edge, pose))
        })
        .await?;
    records[index].transform = pose;
    IoTaskPool::get()
        .spawn(async move {
            let reference = &records[peer];
            let path = reference
                .terrain
                .height_path
                .as_deref()
                .ok_or("参考网格缺少高度作者源")?;
            let namespace = sparse_height_manifest_terrain_id(&map, path)?;
            let mut source = SparseTerrainAuthorSession::load_source(
                &asset_root,
                &map,
                namespace.as_ref(),
                path,
            )?;
            let axis = peer_edge.axis();
            let scale = if axis == 0 {
                reference.transform.scale[0]
            } else {
                reference.transform.scale[2]
            };
            let id = format!(
                "stitch-{}-{serial:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_nanos()
            );
            let (a, b) = clipmap_stitch_pair(
                &records[index],
                edge,
                reference,
                peer_edge,
                id,
                |distance| {
                    let mut point = reference.world_min;
                    if matches!(peer_edge, ClipmapEdge::East) {
                        point[0] = reference.world_max[0];
                    }
                    if matches!(peer_edge, ClipmapEdge::North) {
                        point[1] = reference.world_max[1];
                    }
                    point[axis] += distance / scale;
                    point[axis] =
                        point[axis].clamp(reference.world_min[axis], reference.world_max[axis]);
                    source.prefetch_world_sample_tiles(point[0] as f64, point[1] as f64)?;
                    source
                        .sample_world_height(point[0] as f64, point[1] as f64)?
                        .ok_or("参考边高度尚未就绪".into())
                },
            )?;
            source.verify_source_version()?;
            let mut links = records[index].clipmap_stitches.to_vec();
            links.push(a);
            records[index].clipmap_stitches = links.into();
            let mut links = records[peer].clipmap_stitches.to_vec();
            links.push(b);
            records[peer].clipmap_stitches = links.into();
            for record in [&records[index], &records[peer]] {
                validate_clipmap_stitches(record)?;
            }
            Ok(records)
        })
        .await
}

pub(super) fn remove(records: &mut [SceneTerrainObject], id: &TerrainId) {
    for record in records {
        if record.id == *id {
            record.clipmap_stitches = Default::default();
        } else if record.clipmap_stitches.iter().any(|s| s.peer == *id) {
            record.clipmap_stitches = record
                .clipmap_stitches
                .iter()
                .filter(|s| s.peer != *id)
                .cloned()
                .collect::<Vec<_>>()
                .into();
        }
    }
}

pub(super) fn cancel(
    state: &mut super::resources::TerrainObjectsEditorState,
    preview: &mut feathrise_scene::TerrainObjectTransformPreview,
) {
    if let Some(held) = state.stitch_preview.take() {
        release(state, held);
    }
    state.stitch_apply = false;
    state.stitch_cancel = false;
    preview.pose = None;
}

/// 迟到集合也可能是最后一份大快照；实际释放完成前继续占用原任务门。
pub(super) fn release<T: Send + 'static>(
    state: &mut super::resources::TerrainObjectsEditorState,
    value: T,
) {
    debug_assert!(state.stitch_release.is_none());
    state.stitch_release = Some(AsyncComputeTaskPool::get().spawn(async move {
        drop(value);
    }));
}

pub(super) fn draw_preview(
    state: bevy::prelude::Res<super::resources::TerrainObjectsEditorState>,
    gizmos: Option<bevy::prelude::Gizmos>,
) {
    use bevy::prelude::*;
    let Some(held) = &state.stitch_preview else {
        return;
    };
    let Some(mut gizmos) = gizmos else {
        return;
    };
    let Some(object) = held
        .prepared
        .selected_index
        .and_then(|i| held.prepared.objects.get(i))
    else {
        return;
    };
    let Some(link) = object.clipmap_stitches.last() else {
        return;
    };
    let axis = link.edge.axis();
    let normal = 1 - axis;
    let matrix = object.transform.as_transform().to_matrix();
    let mut previous = None;
    for (i, height) in link.profile.samples.iter().enumerate() {
        let mut p = object.world_min;
        p[axis] = (p[axis] + i as f32 * link.profile.step).min(object.world_max[axis]);
        p[normal] = if link.edge.index() % 2 == 0 {
            object.world_min[normal]
        } else {
            object.world_max[normal]
        };
        let point = matrix.transform_point3(Vec3::new(p[0], *height, p[1]));
        if let Some(before) = previous {
            gizmos.line(before, point, Color::srgb(0.9, 0.65, 0.25));
        }
        previous = Some(point);
    }
}
