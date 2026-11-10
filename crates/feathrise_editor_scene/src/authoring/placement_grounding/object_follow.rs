//! 只冻结128条姿态元数据；真实承托查询在后台执行，采用时复核整份输入而不只复核变化项。

use super::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use feathrise_scene_content::{SceneTerrainBinding, TerrainObjectTransform};

/// 水域覆盖和模型目录不参与贴地；避免固定记录的大型附属集合进入主线程克隆路径。
#[derive(Clone, PartialEq)]
struct PoseInput {
    index: usize,
    id: String,
    pose: TerrainObjectTransform,
    mode: ScenePlacementHeightMode,
    offset: f32,
    binding: Option<SceneTerrainBinding>,
    terrain_patch: bool,
}

impl PoseInput {
    fn capture(index: usize, record: &ScenePlacementRecord) -> Self {
        Self {
            index,
            id: record.id.clone(),
            pose: TerrainObjectTransform {
                translation: record.translation,
                yaw_degrees: record.yaw_degrees,
                scale: record.scale,
                ..Default::default()
            },
            mode: record.height_mode,
            offset: record.terrain_offset,
            binding: record.terrain_binding.clone(),
            terrain_patch: record.terrain_patch.is_some(),
        }
    }

    fn matches(&self, draft: &EditorSceneDraft) -> bool {
        draft
            .placements
            .placements
            .get(self.index)
            .is_some_and(|record| self.matches_record(record))
    }

    fn matches_record(&self, record: &ScenePlacementRecord) -> bool {
        self.id == record.id
            && self.mode == record.height_mode
            && self.offset == record.terrain_offset
            && self.pose.translation == record.translation
            && self.pose.yaw_degrees == record.yaw_degrees
            && self.pose.scale == record.scale
            && self.binding == record.terrain_binding
            && self.terrain_patch == record.terrain_patch.is_some()
    }
}

pub(super) struct PoseChange {
    index: usize,
    before: PoseInput,
    after: PoseInput,
}

impl PoseChange {
    /// 只写后台负责的姿态字段；其他作者属性不被迟到快照覆盖。
    pub(super) fn apply(self, records: &mut [ScenePlacementRecord]) -> Option<ScenePlacementRecord> {
        if self.before == self.after {
            return None;
        }
        let record = &mut records[self.index];
        record.translation = self.after.pose.translation;
        record.yaw_degrees = self.after.pose.yaw_degrees;
        record.scale = self.after.pose.scale;
        record.terrain_offset = self.after.offset;
        record.terrain_binding = self.after.binding;
        Some(record.clone())
    }
}

pub(super) type Changes = Vec<PoseChange>;
struct Completed {
    inputs: Vec<PoseInput>,
    result: Result<(usize, Changes), String>,
}

pub(super) struct Work {
    scene_id: SceneId,
    terrain: u64,
    structure: u64,
    content: Option<[u64; 7]>,
    start: usize,
    task: Task<Completed>,
}

pub(super) fn advance(
    state: &mut PlacementTerrainFollowState,
    draft: &EditorSceneDraft,
    sampler: &PlacementTerrainSampler,
    terrain: u64,
    structure: u64,
    content: Option<[u64; 7]>,
    start: usize,
    end: usize,
) -> Option<Result<(usize, Changes), String>> {
    if let Some(completed) = state
        .object_work
        .as_mut()
        .and_then(|work| check_ready(&mut work.task))
    {
        let work = state.object_work.take().unwrap();
        if work.scene_id == draft.scene_id
            && work.terrain == terrain
            && work.structure == structure
            && work.content == content
            && work.start == start
            && completed.inputs.iter().all(|input| input.matches(draft))
        {
            return Some(completed.result);
        }
        // 包括原先无需变化的记录和确定失败都复核原输入，防止覆盖任务期间刚编辑的模式/位置。
    }
    if state.object_work.is_some() {
        return None;
    }
    let inputs = draft.placements.placements[start..end]
        .iter()
        .enumerate()
        .map(|(offset, record)| PoseInput::capture(start + offset, record))
        .collect::<Vec<_>>();
    let sampler = sampler.clone();
    state.object_work = Some(Work {
        scene_id: draft.scene_id.clone(),
        terrain,
        structure,
        content,
        start,
        task: AsyncComputeTaskPool::get().spawn(async move {
            let result = resolve_batch(&inputs, &sampler, start);
            Completed { inputs, result }
        }),
    });
    None
}

fn resolve_batch(
    inputs: &[PoseInput],
    sampler: &PlacementTerrainSampler,
    start: usize,
) -> Result<(usize, Changes), String> {
    let PlacementTerrainSource::Objects { ground, .. } = &sampler.source else {
        return Err("多地形采样器已失效".into());
    };
    let mut changes = Vec::new();
    let mut next = start;
    for before in inputs {
        let mut after = before.clone();
        if !after.terrain_patch
            && let Some(binding) = after.binding.as_ref()
        {
            after.pose = ground.bindings.resolve(binding)?;
            if let Some(offset) = binding.local_height_offset {
                after.offset = offset
                    * ground
                        .bindings
                        .pose(&binding.terrain_id)
                        .ok_or("关联地形已不存在")?
                        .scale[1];
            }
        }
        if after.mode.follows_terrain() && !after.terrain_patch {
            // 离地偏移不能改变楼层选择；同一GLB包含屋顶和洞底时仍采原承托层。
            let reference = after.pose.translation[1] - after.offset + 0.5;
            let hit = if let Some(binding) = after.binding.as_ref() {
                ground.support_on(
                    &binding.terrain_id,
                    after.pose.translation[0],
                    after.pose.translation[2],
                    reference,
                )
            } else {
                ground.support_status(
                    after.pose.translation[0],
                    after.pose.translation[2],
                    Some(reference),
                )
            };
            let height = match hit {
                ClipmapGroundHit::Hit(point) => point.y,
                ClipmapGroundHit::WaitingForHeight => break,
                _ => {
                    return Err(format!(
                        "实例{}的原承托面已不存在，请移动、解除关联或删除该内容",
                        after.id
                    ));
                }
            };
            after.pose.translation[1] = height + after.offset;
            if !after.pose.translation[1].is_finite() {
                return Err("跟随地形的离地偏移或解析高度无效".into());
            }
            after.binding = if let Some(binding) = after.binding.as_ref() {
                Some(ground.bind(&binding.terrain_id, after.pose)?)
            } else {
                sampler.bind_pose(after.pose, Some(reference))?
            };
            if let Some(binding) = after.binding.as_mut() {
                binding.local_height_offset = Some(
                    after.offset
                        / ground
                            .bindings
                            .pose(&binding.terrain_id)
                            .ok_or("关联地形已不存在")?
                            .scale[1],
                );
            }
        }
        changes.push(PoseChange {
            index: before.index,
            before: before.clone(),
            after,
        });
        next = before.index + 1;
    }
    Ok((next, changes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_scene_content::TerrainId;

    #[test]
    fn unchanged_records_and_failed_batches_still_reject_late_pose_or_mode_edits() {
        let mut record = super::super::tests::record();
        let frozen = PoseInput::capture(0, &record);
        assert!(frozen.matches_record(&record));
        record.height_mode = ScenePlacementHeightMode::FollowTerrain;
        assert!(!frozen.matches_record(&record));
        record.height_mode = ScenePlacementHeightMode::FixedHeight;
        record.translation[0] += 1.0;
        assert!(!frozen.matches_record(&record));
    }

    #[test]
    fn vertical_offset_scales_with_its_island_without_selecting_that_islands_roof() {
        let id = TerrainId::parse("stacked-island").unwrap();
        let mut record = super::super::tests::record();
        record.translation = [0.0, 8.0, 0.0];
        record.height_mode = ScenePlacementHeightMode::FollowTerrain;
        record.terrain_offset = 8.0;
        let mut binding = SceneTerrainBinding::capture(
            id.clone(),
            default(),
            TerrainObjectTransform {
                translation: record.translation,
                ..default()
            },
        )
        .unwrap();
        binding.local_height_offset = Some(8.0);
        record.terrain_binding = Some(binding);
        let mut positions = Vec::new();
        for y in [0.0, 6.0] {
            positions.extend([
                Vec3::new(-2.0, y, -2.0),
                Vec3::new(2.0, y, -2.0),
                Vec3::new(2.0, y, 2.0),
                Vec3::new(-2.0, y, 2.0),
            ]);
        }
        let shape = avian3d::prelude::Collider::trimesh(
            positions,
            vec![[0, 2, 1], [0, 3, 2], [4, 6, 5], [4, 7, 6]],
        );
        let pose = TerrainObjectTransform {
            translation: [10.0, 20.0, 0.0],
            yaw_degrees: 45.0,
            scale: [2.0, 3.0, 4.0],
            ..Default::default()
        };
        let ground = super::super::super::terrain_ground::TerrainGroundSnapshot::test_mesh(
            SceneId::parse("MAP-stacked").unwrap(),
            id,
            pose,
            shape,
        );
        let sampler = PlacementTerrainSampler {
            source: PlacementTerrainSource::Objects {
                ground,
                reference: None,
            },
        };
        let (_, changes) = resolve_batch(&[PoseInput::capture(0, &record)], &sampler, 0).unwrap();
        let mut records = vec![record];
        for change in changes {
            let _ = change.apply(&mut records);
        }
        assert_eq!(records[0].translation, [10.0, 44.0, 0.0]); // 原洞底20米+缩放离地量24米；不是38米屋顶+24米。
        assert_eq!(records[0].yaw_degrees, 45.0);
        assert_eq!(records[0].scale, [2.0, 3.0, 4.0]);
        assert_eq!(
            records[0]
                .terrain_binding
                .as_ref()
                .unwrap()
                .local_height_offset,
            Some(8.0)
        );
    }
}
