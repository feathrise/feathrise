//! 院落整批摆放复用建筑 BVH、五点地基和双向门前净空的后台校验。
//!
//! 【功能设计】整个院落只有一次作者事务。后台读取当前不可变 BVH 与共享高度场，
//! 逐栋排除旧组、检查新组内相互关系，再把完整结果交给院落作者域原子采用。

use std::collections::BTreeSet; // 旧组排除ID只在显式整体操作边沿构造。
use std::sync::Arc; // 原有8192项建筑索引只克隆强句柄。

use bevy::prelude::*; // Rect、Vec2、Vec3与公共四边形使用同一数学语义。
use feathrise_scene_content::{
    BuildingPresetDescriptor, BuildingPresetRegistry, BuildingWorldQuad, SceneId,
    ScenePlacementHeightMode, ScenePlacementRecord,
};
use feathrise_scene::DynamicClipmapGroundSurface; // 一整组仍由正式建筑预设和普通摆放记录构成。

use super::index::current_identity; // 快照只能匹配当前作者、结构和建筑目录修订。
use super::resources::BuildingAuthoringState; // 复用唯一已采用空间索引。
use super::types::{BuildingGroundQuery, BuildingSpatialIndex, union_rect}; // BVH宽相位与单栋落点共用。
use crate::authoring::{EditorAuthoringSession, PlacementTerrainSampler}; // 地形只共享Arc量化高度。

const QUARTER_TURN_EPSILON: f32 = 0.001; // 与单栋建筑允许的九十度档位一致。
const UNIT_SCALE_EPSILON: f32 = 0.001; // 地基与碰撞只接受单位缩放建筑。

/// 一次院落操作冻结的建筑空间和地形输入；不借用主世界或作者草稿。
#[derive(Clone)]
pub(in crate::authoring) struct CourtyardBuildingValidator {
    index: Arc<BuildingSpatialIndex>, // 旧建筑完整BVH在新组校验期间保持不可变。
    pub(in crate::authoring) sampler: Option<PlacementTerrainSampler>, // 普通固定高度资产允许在旧非高度场场景上使用。
    scene_id: SceneId, // 记录变换失败需明确指向同一场景。
    bounds: Rect,  // 所有成员原点、建筑占地和门区共同服从场景范围。
}

/// 从已完成的同代建筑索引取得后台可用快照；在途单栋校验时拒绝整组操作。
pub(in crate::authoring) fn courtyard_validator_snapshot(
    session: &EditorAuthoringSession,  // 当前草稿和作者修订。
    registry: &BuildingPresetRegistry, // 建筑目录修订与BVH必须一致。
    state: &BuildingAuthoringState,    // 只读取已原子采用的索引。
    clipmap: Option<&DynamicClipmapGroundSurface>,
) -> Result<CourtyardBuildingValidator, String> {
    let identity = current_identity(session, registry)
        .ok_or_else(|| "建筑目录或场景草稿尚未就绪".to_owned())?; // 不能根据未加载目录猜占地。
    if state.validation_task.is_some() || state.validation_waiting.is_some() {
        return Err("一处单栋建筑落点仍在校验，请稍后操作院落".to_owned()); // 避免两个作者任务竞争同一草稿。
    }
    if let Some((_, error)) = state
        .index_error
        .as_ref()
        .filter(|(failed, _)| failed == &identity)
    {
        return Err(format!("建筑空间索引构建失败：{error}")); // 确定失败不能伪装成仍在准备。
    }
    if state.active_identity.as_ref() != Some(&identity) {
        return Err("建筑空间索引正在准备，请就绪后重新点击视口盖印".to_owned()); // 新目录或作者改动必须等待同代BVH。
    }
    let index = state
        .active_index
        .as_ref()
        .cloned()
        .ok_or_else(|| "建筑空间索引尚未就绪".to_owned())?; // O(1)共享旧建筑。
    let draft = session
        .draft
        .as_ref()
        .ok_or_else(|| "当前场景作者草稿不存在".to_owned())?; // 身份确认后取得同一场景。
    let bounds = draft
        .manifest
        .world_bounds()
        .ok_or_else(|| "当前场景范围无效".to_owned())?; // 不对坏范围落印。
    let sampler = if draft.manifest.terrain.kind == feathrise_scene_content::SceneTerrainKind::SparseClipmap {
        Some(PlacementTerrainSampler::from_sources(
            draft,
            clipmap,
            session.terrain_revision,
        )?)
    } else {
        PlacementTerrainSampler::from_draft(draft).ok()
    }; // 仅含固定普通资产时可不需要高度场。
    Ok(CourtyardBuildingValidator {
        index,
        sampler,
        scene_id: draft.scene_id.clone(),
        bounds,
    }) // 小型纯值进入CPU任务。
}

impl CourtyardBuildingValidator {
    /// 院落最多256成员；点集在worker生成，装饰保留原点跟随，建筑保留独立五点。
    pub(in crate::authoring) fn prepare_ground(
        &mut self,
        candidates: &[(ScenePlacementRecord, Option<BuildingPresetDescriptor>)],
        query: &mut Option<BuildingGroundQuery>,
    ) -> Result<bool, String> {
        let Some(sampler) = self.sampler.as_ref() else {
            return Ok(true);
        };
        if sampler.is_volume_layer() || sampler.is_object_surface() || sampler.clipmap_content_revision().is_none() {
            return Ok(true);
        } // GLB不创建分页点集或增加一次采样。
        let ground = query.get_or_insert_with(|| {
            BuildingGroundQuery::new(
                sampler,
                candidates.iter().flat_map(|(record, preset)| {
                    if let Some(preset) = preset {
                        preset
                            .world_footprint(record.translation, record.yaw_degrees, record.scale)
                            .foundation_samples()
                            .to_vec()
                    } else if record.height_mode.follows_terrain() {
                        vec![Vec2::new(record.translation[0], record.translation[2])]
                    } else {
                        Vec::new()
                    }
                }),
            )
        });
        let prepared = ground.prepare(sampler).map_err(|error| {
            let failed_point = ground.next_point_index();
            let mut point_offset = 0;
            let member = candidates.iter().find_map(|(record, preset)| {
                point_offset += if preset.is_some() {
                    5
                } else {
                    usize::from(record.height_mode.follows_terrain())
                };
                (failed_point < point_offset).then_some(record.id.as_str())
            }); // 点集与成员顺序相同，采样损坏或越界也要指出具体成员。
            member.map_or_else(
                || error.clone(),
                |id| format!("院落成员{id}地基采样失败：{error}"),
            )
        })?;
        match prepared {
            Some(frozen) => {
                self.sampler = Some(frozen);
                *query = None;
                Ok(true)
            }
            None => {
                self.sampler = None;
                Ok(false)
            } // 等待仅保留有限点，释放旧分页Arc。
        }
    }
}

#[cfg(test)]
#[path = "courtyard/clipmap_tests.rs"]
mod clipmap_tests;

/// 后台完整校验并解析一组候选的最终世界高度。
pub(in crate::authoring) fn validate_courtyard_batch(
    validator: &CourtyardBuildingValidator, // 已冻结同代旧建筑索引与地形。
    mut candidates: Vec<(ScenePlacementRecord, Option<BuildingPresetDescriptor>)>, // 小型完整候选，预设由当前目录明确解析。
    excluded_ids: &BTreeSet<String>, // 移动旧院落时只排除原组成员，复制或新落印为空。
) -> Result<Vec<ScenePlacementRecord>, String> {
    let mut accepted_buildings = Vec::<(String, BuildingWorldQuad, BuildingWorldQuad)>::new(); // 同组新建筑互相检查。
    for (record, preset) in &mut candidates {
        if let Some(preset) = preset {
            if record.building_preset_id.as_deref() != Some(preset.id.as_str())
                || record.asset_path != preset.asset_path
                || record.asset_target != preset.asset_target
            {
                return Err(format!("院落成员{}的模型与建筑预设不一致", record.id)); // 不允许套错占地与碰撞。
            }
            if record.terrain_binding.is_none() && record
                .scale
                .iter()
                .any(|value| (*value - 1.0).abs() > UNIT_SCALE_EPSILON)
            {
                return Err(format!("建筑{}首期只支持单位缩放", record.id)); // 与单体校验口径一致。
            }
            let quarter_turn = (record.yaw_degrees / 90.0).round() * 90.0; // 组合整体和成员都必须形成合法档位。
            if record.terrain_binding.is_none() && (record.yaw_degrees - quarter_turn).abs() > QUARTER_TURN_EPSILON {
                return Err(format!("建筑{}旋转必须使用九十度档位", record.id)); // 门区与地基不能接受任意旋转。
            }
            let footprint =
                preset.world_footprint(record.translation, record.yaw_degrees, record.scale); // 与正式索引同公式。
            let entrance = preset.world_entrance_clearance(
                record.translation,
                record.yaw_degrees,
                record.scale,
            ); // 门区独立于实体碰撞。
            let sampler = validator
                .sampler
                .as_ref()
                .ok_or_else(|| "当前场景没有建筑地基高度场".to_owned())?; // 建筑不在未知旧地形猜Y。
            let sampler = sampler.clone().for_selected_surface(Vec3::from_array(record.translation));
            let mut minimum = f32::INFINITY; // 五点最低地面供坡差诊断。
            let mut maximum = f32::NEG_INFINITY; // 五点最高地面决定最终根Y。
            for point in footprint.foundation_samples() {
                let height = sampler
                    .sample(point.x, point.y)
                    .map_err(|error| format!("院落成员{}地基采样失败：{error}", record.id))?; // 与单栋摆放复用正式三角采样。
                minimum = minimum.min(height); // 保留本栋最低值。
                maximum = maximum.max(height); // 保留本栋最高值。
            }
            if maximum - minimum > preset.foundation.max_ground_height_delta {
                return Err(format!("建筑{}地基高差超过预设限制", record.id)); // 一栋失败整院落不落印。
            }
            record.translation[1] = maximum - preset.foundation.model_base_y * record.scale[1]; // 建筑竖直并贴合最高地面。
            record.height_mode = ScenePlacementHeightMode::FixedHeight; // 五点地基不退化成原点跟随。
            record.terrain_offset = 0.0; // 固定建筑不保存失效离地偏移。
            record.terrain_binding = sampler.bind_pose(feathrise_scene_content::TerrainObjectTransform {
                translation: record.translation, yaw_degrees: record.yaw_degrees, scale: record.scale,
                ..Default::default()
            }, Some(maximum + 0.5))?;
            let query_bounds = union_rect(footprint.aabb(), entrance.aabb()); // 一次BVH查询覆盖双向规则。
            for candidate in validator.index.query(query_bounds) {
                let Some(existing) = validator.index.entry(candidate) else {
                    continue;
                }; // 损坏内部索引不授权放置。
                if excluded_ids.contains(&existing.id) {
                    continue;
                } // 移动整组时排除旧位置的成员。
                let overlap = footprint.aabb().intersect(existing.footprint.aabb());
                if sampler.separated_by_terrain(
                    existing.translation[1] + existing.foundation.model_base_y * existing.scale[1],
                    overlap.center(),
                ) {
                    continue;
                }
                if footprint.overlaps(existing.footprint)
                    || entrance.overlaps(existing.footprint)
                    || footprint.overlaps(existing.entrance_clearance)
                {
                    return Err(format!(
                        "建筑{}与现有建筑{}占地或门前空间冲突",
                        record.id, existing.id
                    )); // 保留具体双方身份。
                }
            }
            for (prior_id, prior_footprint, prior_entrance) in &accepted_buildings {
                if footprint.overlaps(*prior_footprint)
                    || entrance.overlaps(*prior_footprint)
                    || footprint.overlaps(*prior_entrance)
                {
                    return Err(format!(
                        "院落内建筑{}与{}占地或门前空间冲突",
                        record.id, prior_id
                    )); // 组内也必须完整合法。
                }
            }
            accepted_buildings.push((record.id.clone(), footprint, entrance)); // 后续成员只检查此前已受检项。
        } else if record.building_preset_id.is_some() {
            return Err(format!("院落成员{}引用了未知建筑预设", record.id)); // 目录缺失不得当普通GLB跳过规则。
        } else if record.height_mode.follows_terrain() {
            let sampler = validator
                .sampler
                .as_ref()
                .ok_or_else(|| "当前场景没有可用于普通装饰的高度场".to_owned())?; // 跟随型只用真实地形。
            record.translation[1] = sampler.sample(record.translation[0], record.translation[2])?
                + record.terrain_offset; // 在新位置恢复原离地量。
            if sampler.is_volume_layer() {
                record.height_mode = ScenePlacementHeightMode::FixedHeight;
                record.terrain_offset = 0.0; // 三维院落装饰冻结所选洞层，重载不能回到外部高度。
            }
            record.terrain_binding = sampler.bind_pose(feathrise_scene_content::TerrainObjectTransform {
                translation: record.translation, yaw_degrees: record.yaw_degrees, scale: record.scale,
                ..Default::default()
            }, Some(record.translation[1] - record.terrain_offset + 0.5))?;
            sampler.refresh_record_binding(record)?;
        } else if record.terrain_binding.is_some() {
            if let Some(sampler) = validator.sampler.as_ref() { sampler.refresh_record_binding(record)?; }
        }
        record.validated_transform(&validator.scene_id, validator.bounds)?; // 最终成员的数值和场景范围与正式运行时共享。
    }
    Ok(candidates.into_iter().map(|(record, _)| record).collect()) // 全组成功后才交给主线程作者事务。
}
