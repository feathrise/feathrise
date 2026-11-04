//! 复制快照、独立身份、网格水预算、地基及历史计费均在唯一后台CPU槽完成。
use super::{terrain_patch_pose, types::*};
use crate::authoring::buildings::{BuildingGroundQuery, validate_courtyard_batch};
use crate::authoring::{CourtyardBatchChange, update_follow_offset_from_world_y};
use bevy::prelude::*;
use feathrise_map_content::{MAX_MAP_PLACEMENT_WATER_SURFACES_PER_MAP, MapPlacementRecord};
use std::{collections::BTreeMap, sync::Arc};

#[cfg(test)]
pub(super) fn capture(capture: Capture) -> BatchOutput {
    let source = (capture.action == PlacementBatchAction::Delete)
        .then(|| crate::authoring::placement_deletion::PlacementDeletionIntents::default().source());
    capture_with_deletions(capture, source)
}

/// 删除元数据和完整历史在同一CPU槽准备，主线程不遍历八千项票据表。
pub(super) fn capture_with_deletions(capture: Capture,
    deletion_source: Option<crate::authoring::placement_deletion::PlacementDeletionSource>) -> BatchOutput {
    if capture.records.iter().any(|(_, record)| terrain_patch_pose::is_automatic_volume(record)) {
        let error = "自动三维雕刻层不能普通复制或删除，请使用其专用作者入口".into();
        return if capture.action == PlacementBatchAction::Delete {
            BatchOutput::Deleted(Err(error))
        } else {
            BatchOutput::Captured(Err(error), capture.action == PlacementBatchAction::Duplicate)
        }; // 先于Delete与复制分支统一核对完整捕获，混合组不能部分采用或绕过拓扑回收责任。
    }
    if capture.action == PlacementBatchAction::Delete {
        let deletion =
            CourtyardBatchChange::new_removed(capture.records, capture.length).and_then(|history| {
                let prepared = deletion_source.ok_or("批量删除缺少逻辑票据来源")?.prepare(
                    history.removed.iter().map(|(_, record)| record.id.as_str()),
                    std::iter::empty::<&str>(),
                )?;
                let updates = history
                    .removed
                    .iter()
                    .map(|(index, record)| (*index, record.clone(), false))
                    .collect();
                Ok((history, updates, prepared))
            });
        return BatchOutput::Deleted(deletion);
    }
    let result = (|| {
        let mut bytes = capture
            .records
            .iter()
            .map(|(_, record)| {
                crate::authoring::commands::placement_record_heap_bytes(record)
                    + std::mem::size_of::<MapPlacementRecord>()
            })
            .sum::<usize>();
        if capture.records.iter().any(|(_, record)| record.terrain_patch.is_some()) {
            bytes += capture.records.len() * std::mem::size_of::<Option<feathrise_map_content::TerrainObjectTransform>>();
        } // 完整源框架也计入剪贴板上限，不能仅给作者字段计费。
        if bytes > MAX_CLIPBOARD_BYTES {
            return Err("选中对象的完整配置超过16 MiB，请分批复制".into());
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut records = Vec::with_capacity(capture.records.len());
        let mut patch_frames = Vec::with_capacity(capture.records.len());
        let mut builtin_only = true;
        let mut lowest_bottom = f32::INFINITY;
        for (_, record) in &capture.records {
            let source = if let Some(patch) = record.terrain_patch.as_ref() {
                let id = patch.terrain_id.clone().unwrap_or_else(feathrise_map_content::TerrainId::legacy);
                Some(capture.source_objects.iter().find(|object| object.id == id && object.enabled
                    && object.terrain.kind == feathrise_map_content::MapTerrainKind::SparseClipmap)
                    .ok_or("三维地形所属的Clipmap不存在或未启用，复制未开始")?)
            } else { None };
            let bounds = source.map_or(capture.bounds, |object|
                Rect::from_corners(object.world_min.into(), object.world_max.into()));
            record.validated_transform(&capture.identity.map, bounds)?;
            if !record.asset_target.is_builtin() {
                crate::authoring::safe_asset_path(
                    &crate::authoring::workspace_asset_root(),
                    &record.asset_path,
                )?;
            } // 内置几何没有磁盘资产；尺寸和材质仍由正式记录校验，复制不请求空路径。
            let frame = source.map(|object| object.transform);
            let position = if let Some(frame) = frame {
                terrain_patch_pose::source_to_world(record_pose(record), frame)?.transform_point3(Vec3::ZERO)
            } else { Vec3::from_array(record.translation) };
            if let Some(geometry) = record.asset_target.editable_geometry() {
                let local_bottom = geometry.vertices().iter().map(|point| point[1]).min().unwrap() as f32 * 0.0005;
                lowest_bottom = lowest_bottom.min(position.y + local_bottom * record.scale[1]);
            } else {
                builtin_only = false;
            }
            min = min.min(position);
            max = max.max(position);
            records.push(record.clone());
            patch_frames.push(frame);
        }
        // 纯立方体使用实际最低底面，兼容独立尺寸与Y缩放；共同位移只算一次，组内高差保持不变。
        // 其他资产没有可证明的参数化边界，继续以最低作者原点作为整组锚点。
        let origin = Vec3::new((min.x + max.x) * 0.5, if builtin_only { lowest_bottom } else { min.y }, (min.z + max.z) * 0.5);
        if patch_frames.iter().all(Option::is_none) { patch_frames = Vec::new(); }
        Ok(Arc::new(ClipboardData {
            records,
            patch_frames: Arc::new(patch_frames),
            origin,
            source_map: capture.identity.map.clone(),
        }))
    })();
    BatchOutput::Captured(result, capture.action == PlacementBatchAction::Duplicate)
}

pub(super) fn ghost_records(
    source: &ClipboardData,
    prefix: &str,
) -> Result<Arc<Vec<MapPlacementRecord>>, String> {
    let mut groups = BTreeMap::<String, String>::new();
    let mut records = source.records.clone();
    for (ordinal, record) in records.iter_mut().enumerate() {
        record.id = format!("{prefix}-{ordinal}");
        let metadata = Arc::make_mut(&mut record.editor);
        metadata.hidden = false; metadata.locked = false; // 副本保留名称和分组，独立解除源对象的编辑器保护。
        if let Some(group) = record.courtyard.as_mut() {
            let count = groups.len();
            let next = groups
                .entry(group.instance_id.clone())
                .or_insert_with(|| format!("{prefix}-g{count}"));
            group.instance_id = next.clone(); // 每次粘贴的院落副本也独立，不能继续指向源地图的组合。
        }
    }
    Ok(Arc::new(records))
}

pub(super) fn run_paste(mut work: PasteWork, base_len: usize) -> BatchOutput {
    if work.source.is_empty() || work.source.len() != work.presets.len() {
        return BatchOutput::Paste(Err("粘贴成员与预设快照不完整".into()));
    }
    if work.work_plane && (work.validator.is_some() || work.source.iter().any(|record|
        !record.asset_target.is_builtin())) {
        return BatchOutput::Paste(Err("空地图工作平面仅用于内置网格组".into()));
    }
    let validation_bounds = match patch_bounds(&work) {
        Ok(bounds) => bounds,
        Err(error) => return BatchOutput::Paste(Err(error)),
    };
    if work.records.is_empty() {
        work.records = work.source.as_ref().clone(); // 完整数组及水绑定只在worker复制，主线程只传Arc。
        for (ordinal, (record, preset)) in work.records.iter_mut().zip(work.presets.iter()).enumerate() {
            if record.terrain_patch.is_some() {
                let context = work.patch.as_ref().expect("patch_bounds已证明完整地形上下文");
                let frame = context.frames[ordinal].expect("patch_bounds已证明源父变换存在");
                let pose = terrain_patch_pose::source_to_world(record_pose(record), frame)
                    .and_then(|world| terrain_patch_pose::world_to_source(
                        Mat4::from_translation(work.delta) * world, context.target.object.transform));
                let pose = match pose {
                    Ok(pose) => pose,
                    Err(error) => return BatchOutput::Paste(Err(format!("三维地形{}无法粘贴：{error}", record.id))),
                };
                record.translation = pose.translation;
                record.yaw_degrees = pose.yaw_degrees;
                record.scale = pose.scale;
                record.terrain_patch.as_mut().unwrap().terrain_id =
                    (!context.target.legacy).then(|| context.target.object.id.clone());
                record.terrain_offset = 0.0;
            } else {
                record.translation = (Vec3::from_array(record.translation) + work.delta).to_array();
            }
            record.terrain_binding = None; // 副本进入目标地图；源TerrainId与源局部TRS绝不能沿用。
            if work.work_plane {
                record.height_mode = feathrise_map_content::MapPlacementHeightMode::FixedHeight;
                record.terrain_offset = 0.0; // 空图没有承托地形；实际中心Y已由底面锚点共同位移，不能再加半高或旧离地偏移。
            }
            if let Some(group) = record.courtyard.as_mut() {
                group.origin = (Vec3::from_array(group.origin) + work.delta).to_array();
            }
            if let Some(preset) = preset {
                record.building_preset_id = Some(preset.id.clone());
            }
        }
    }
    let prepared = (|| -> Result<bool, String> {
        if let Some(sampler) = work.sampler.as_ref().filter(|sampler| sampler.is_object_surface()) {
            for record in work.records.iter_mut().filter(|record| record.terrain_patch.is_none()) {
                record.terrain_binding = sampler.bind_pose(feathrise_map_content::TerrainObjectTransform {
                    translation: record.translation, yaw_degrees: record.yaw_degrees, scale: record.scale,
                    ..Default::default()
                }, Some(record.translation[1] + 0.5))?;
                if record.height_mode.follows_terrain() {
                    record.terrain_offset = 0.0; // 已换目标层；旧偏移不能把后续参考射线推到新地面下方，完整偏移由目标真实高度重新计算。
                }
            } // 在同一后台快照中重新捕获真实目标层；固定高度副本也保留地形整体变换归属。
        }
        if let Some(validator) = work.validator.as_mut() {
            let candidates = work
                .records
                .iter()
                .zip(work.presets.iter())
                .filter(|(record, _)| record.terrain_patch.is_none())
                .map(|(record, preset)| (record.clone(), preset.clone()))
                .collect::<Vec<_>>();
            if !validator.prepare_ground(&candidates, &mut work.query)? {
                return Ok(false);
            }
            work.sampler = validator.sampler.clone();
        } else if let Some(sampler) = work
            .sampler
            .as_ref()
            .filter(|sampler| !sampler.is_object_surface() && sampler.clipmap_content_revision().is_some())
        {
            let query = work.query.get_or_insert_with(|| {
                BuildingGroundQuery::new(
                    sampler,
                    work.records
                        .iter()
                        .filter(|record| record.height_mode.follows_terrain())
                        .map(|record| Vec2::new(record.translation[0], record.translation[2])),
                )
            });
            let Some(frozen) = query.prepare(sampler)? else {
                work.sampler = None;
                return Ok(false);
            };
            work.sampler = Some(frozen);
            work.query = None;
        }
        for record in &mut work.records {
            if record.building_preset_id.is_none() && record.height_mode.follows_terrain() {
                update_follow_offset_from_world_y(
                    record,
                    work.sampler.as_ref().ok_or("目标地图地形高度尚未就绪")?,
                )?;
            } // 普通资产保持共同三维位移，以偏移表达真实高度；建筑仍使用五点地基。
            record.validated_transform(&work.map, validation_bounds)?;
        }
        if let Some(validator) = &work.validator {
            // 三维地形已按所属源局部范围校验，不能再交给使用容器范围的建筑校验器。
            // 普通成员仍完整经过地基/占地检查，结果回填原槽，保持混合组与整笔历史的顺序。
            let validated = validate_courtyard_batch(
                validator,
                work.records
                    .iter()
                    .zip(work.presets.iter())
                    .filter(|(record, _)| record.terrain_patch.is_none())
                    .map(|(record, preset)| (record.clone(), preset.clone()))
                    .collect(),
                &Default::default(),
            )?;
            for (record, validated) in work.records.iter_mut()
                .filter(|record| record.terrain_patch.is_none()).zip(validated) {
                *record = validated;
            }
        }
        Ok(true)
    })();
    match prepared {
        Ok(false) => return BatchOutput::Waiting(Box::new(work)),
        Err(error) => return BatchOutput::Paste(Err(error)),
        Ok(true) => {}
    }
    let mut prepared = prepare_paste_records(
        work.records,
        base_len,
        &work.map,
        validation_bounds,
    );
    if let (Ok(prepared), Some(context)) = (&mut prepared, &work.patch) {
        for member in &mut prepared.members {
            if prepared.apply[member.index - base_len].terrain_patch.is_some() {
                member.position = context.target.object.transform.as_transform().to_matrix()
                    .transform_point3(member.position);
            }
        } // 选集使用世界锚点；完整作者记录继续保留所属源局部坐标。
    }
    BatchOutput::Paste(prepared)
}

fn record_pose(record: &MapPlacementRecord) -> feathrise_map_content::TerrainObjectTransform {
    feathrise_map_content::TerrainObjectTransform { translation: record.translation,
        yaw_degrees: record.yaw_degrees, scale: record.scale, ..Default::default() }
}

/// 确认射线所属对象、完整源父变换和全图容量后才可开始构造任何副本。
fn patch_bounds(work: &PasteWork) -> Result<Rect, String> {
    let added = work.source.iter().fold(PatchBudget::default(), |mut budget, record| {
        if let Some(patch) = &record.terrain_patch {
            budget.roots += 1;
            budget.holes += usize::from(patch.cutout.is_some()) + patch.cutouts.len();
        }
        budget
    });
    if added.roots == 0 { return Ok(work.bounds); }
    let context = work.patch.as_ref().ok_or("三维地形需要命中启用的Clipmap，粘贴未应用")?;
    let target = &context.target.object;
    if !target.enabled || target.terrain.kind != feathrise_map_content::MapTerrainKind::SparseClipmap
        || (context.target.legacy && target.id != feathrise_map_content::TerrainId::legacy()) {
        return Err("三维地形只能粘贴到启用的Clipmap".into());
    }
    if context.frames.len() != work.source.len() || work.source.iter().enumerate().any(|(index, record)|
        record.terrain_patch.is_some() && context.frames[index].is_none()) {
        return Err("三维地形的源姿态快照不完整".into());
    }
    if work.source.iter().any(terrain_patch_pose::is_automatic_volume) {
        return Err("自动三维雕刻层不能普通复制，请使用其专用作者入口".into());
    }
    if context.existing.roots.saturating_add(added.roots) > feathrise_map_content::MAX_TERRAIN_CUTOUTS {
        return Err("目标地图的三维地形实例超过32项预算，整批未应用".into());
    }
    if context.existing.holes.saturating_add(added.holes) > feathrise_map_content::MAX_TERRAIN_CUTOUTS {
        return Err("目标地图的三维地形洞口超过32项预算，整批未应用".into());
    }
    Ok(Rect::from_corners(target.world_min.into(), target.world_max.into()))
}

pub(super) fn prepare_paste_records(
    records: Vec<MapPlacementRecord>,
    base_len: usize,
    map: &feathrise_map_content::MapId,
    bounds: Rect,
) -> Result<PreparedPaste, String> {
    let mut ids = std::collections::BTreeSet::new();
    let mut water_count = 0;
    for record in &records {
        if !ids.insert(record.id.as_str()) {
            return Err("粘贴对象身份重复".into());
        }
        record.validated_transform(map, bounds)?;
        water_count += record.water_surfaces.len();
    }
    if water_count > MAX_MAP_PLACEMENT_WATER_SURFACES_PER_MAP {
        return Err("粘贴对象的网格水绑定超过单图预算".into());
    }
    let history = CourtyardBatchChange::new(records.clone(), Vec::new())?;
    let members = records
        .iter()
        .enumerate()
        .filter(|(_, record)| record.building_preset_id.is_none())
        .map(
            |(i, record)| crate::viewport::multi_select::SelectedMember {
                id: record.id.clone(),
                index: base_len + i,
                position: Vec3::from_array(record.translation),
            },
        )
        .collect();
    Ok(PreparedPaste {
        apply: records,
        history,
        members,
        water_count,
    })
}
