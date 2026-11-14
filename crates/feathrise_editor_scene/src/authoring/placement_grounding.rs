//! 显式摆放实例的内存地形采样、作者高度模式与分帧重贴地。
//!
//! 【功能设计】跟随型实例把`terrain_offset`作为作者不变量，`translation.y`只保存当前
//! 地形修订下的已解析世界高度；地形变化后每帧最多处理固定数量记录，保存必须等待整批完成。

use std::collections::BTreeMap; // 建筑批次只冻结有限查询点，不保留整图或全部分页。
use std::sync::Arc; // 地形采样器只克隆内存高度场强句柄，不复制完整高度数据。

use bevy::prelude::*; // Resource、Commands、Query和世界范围类型。
use feathrise_scene_content::{
    SceneId, ScenePlacementHeightMode, ScenePlacementRecord, SceneTerrainKind,
}; // 作者与运行时共享高度模式、记录格式和正式三角形采样。
use feathrise_scene::{
    ClipmapGroundHit, ClipmapGroundSnapshot, DynamicClipmapGroundSurface, ScenePlacementInstance,
}; // Clipmap普通摆放与植被、水域读取同一已采用三角面快照；分帧更新只定向当前活动或作者预览根。

use super::{EditorAuthoringSession, EditorSceneDraft, TerrainBrushStroke, WaterBrushStroke}; // 当前草稿、修订和两类可能改写地形高度的活动笔划。
use crate::resources::SceneEditorState; // F12生命周期决定是否继续作者重贴地。
use crate::ui::placement_transform::sync_placement_record_world; // 已解析记录复用单根预览同步链路。
use crate::ui::placement_water::PlacementInstanceIndex; // 稳定ID避免扫描完整世界实体集合。
mod object_follow; // 多地形真实三角批次移出主线程，采用仍沿用同一记录与保存门。

/// 每帧最多检查并采用的摆放记录数；与正式摆放规划预算保持一致。
pub(crate) const PLACEMENT_GROUNDING_RECORD_BUDGET_PER_FRAME: usize = 128;

/// 一次O(1)克隆得到的不可变地形采样输入。
#[derive(Clone)]
enum PlacementTerrainSource {
    Objects { ground: Arc<super::terrain_ground::TerrainGroundSnapshot>, reference: Option<f32> },
    VolumeLayer {
        ground: ClipmapGroundSnapshot,
        reference: f32,
    },
    #[cfg(test)]
    TestHeightfield {
        heights: Arc<Vec<u16>>, // 当前作者修订的完整量化高度，只共享不复制。
        resolution: [u32; 2],   // 与地形Mesh完全相同的顶点分辨率。
        world_min: [f32; 2],    // 规则高度场世界XZ最小角。
        world_max: [f32; 2],    // 最大边界允许精确采样末行末列。
        base_height: f32,       // 量化零值映射的世界Y。
        height_scale: f32,      // 完整量化范围映射的米数。
    },
    SparseClipmap(ClipmapGroundSnapshot), // 共享当前已采用局部CPU页和正式环带拓扑，不物化整图高度。
    SampledClipmap {
        content: [u64; 7],
        heights: Arc<BTreeMap<(u32, u32), f32>>,
        volume_ground: Option<ClipmapGroundSnapshot>, // 建筑五点与院落装饰原点的同代精确结果。
    },
}

/// 普通摆放重贴地明确区分局部权威页等待；合法场景内的位置不再受当前视图覆盖限制。
pub(crate) enum PlacementTerrainSample {
    Height(f32),      // 当前同代基础作者三角面的世界Y。
    WaitingForHeight, // 非空真实页尚未采用，禁止伪装成Y=0。
}

#[derive(Clone)]
pub(crate) struct PlacementTerrainSampler {
    source: PlacementTerrainSource, // 两条地形路线只在这里分支，调用方共享高度模式语义。
}

impl PlacementTerrainSampler {
    /// 点击所选高度决定承托层；洞底五点采样不能回到同XZ的屋顶/外部高度。
    pub(crate) fn for_selected_surface(mut self, point: Vec3) -> Self {
        if let PlacementTerrainSource::Objects { reference, .. } = &mut self.source {
            *reference = Some(point.y + 0.5);
            return self;
        }
        let ground = match &self.source {
            PlacementTerrainSource::SparseClipmap(ground) => Some(ground),
            PlacementTerrainSource::VolumeLayer { ground, .. } => Some(ground),
            PlacementTerrainSource::SampledClipmap { volume_ground, .. } => volume_ground.as_ref(),
            _ => None,
        };
        if let Some(ground) = ground {
            if point.is_finite()
                && (ground.is_three_dimensional_surface(point)
                    || ground
                        .volume_footprints()
                        .iter()
                        .any(|hole| hole.contains(point.xz())))
            {
                // 保存地基采用五点最高值，可高于中心地面；重载仍须按其洞层采样，不能要求根Y恰好落在中心面上。
                self.source = PlacementTerrainSource::VolumeLayer {
                    ground: ground.clone(),
                    reference: point.y + 0.5,
                };
            } else if matches!(self.source, PlacementTerrainSource::VolumeLayer { .. }) {
                self.source = PlacementTerrainSource::SparseClipmap(ground.clone()); // 同一保存审计包含室外建筑时恢复其高度域。
            }
        }
        self
    }
    pub(crate) fn is_volume_layer(&self) -> bool {
        matches!(self.source, PlacementTerrainSource::VolumeLayer { .. })
    }
    pub(crate) fn is_object_surface(&self) -> bool {
        matches!(self.source, PlacementTerrainSource::Objects { .. })
    }
    pub(crate) fn object_pose(&self, id: &feathrise_scene_content::TerrainId) -> Option<feathrise_scene_content::TerrainObjectTransform> {
        let PlacementTerrainSource::Objects { ground, .. } = &self.source else { return None; };
        ground.bindings.pose(id)
    }
    /// 两个占地之间存在实体洞顶时净空按楼层区分，不能靠单纯Y偏移绕过校验。
    pub(crate) fn separated_by_terrain(&self, other_floor: f32, xz: Vec2) -> bool {
        let PlacementTerrainSource::VolumeLayer { ground, reference } = &self.source else {
            return false;
        };
        let floor = *reference - 0.5;
        let lower = floor.min(other_floor);
        let upper = floor.max(other_floor);
        if !upper.is_finite() || !lower.is_finite() || upper - lower <= 0.1 {
            return false;
        }
        ground
            .three_dimensional_hit(
                Vec3::new(xz.x, upper - 0.02, xz.y),
                -Vec3::Y,
                upper - lower - 0.04,
            )
            .is_some_and(|(_, normal)| normal.y < -0.05)
    }
    /// 冻结已完成的真实地形承托面；普通模型通过独立表面查询提供种植能力。
    pub(crate) fn from_draft(draft: &EditorSceneDraft) -> Result<Self, String> {
        if draft.manifest.terrains.is_some() {
            return draft.terrain_ground.as_ref().map(|ground| Self {
                source: PlacementTerrainSource::Objects { ground: ground.clone(), reference: None }
            }).ok_or_else(|| "多地形真实承托面尚未完整就绪".into());
        }
        // 单元测试以有限内存高度场覆盖通用高度语义；生产作者线不再读取旧高度网格。
        #[cfg(test)]
        if let Some(heights) = draft.heights.clone() {
            return Ok(Self {
            source: PlacementTerrainSource::TestHeightfield {
                heights,                                           // O(1)共享当前不可变高度代次。
                resolution: draft.manifest.terrain.resolution,     // 采样网格必须与派生GLB一致。
                world_min: draft.manifest.world_min,               // X/Z边界来自同一场景清单。
                world_max: draft.manifest.world_max,               // 最大边界允许精确采样末行末列。
                base_height: draft.manifest.terrain.base_height,   // 解码参数与正式植被贴地一致。
                height_scale: draft.manifest.terrain.height_scale, // 不建立第二套高度换算。
            },
            });
        }
        Err("真实地形承托面尚未完整就绪，不能启用跟随地形".into())
    }

    /// 普通摆放在Clipmap场景中冻结当前正式地表快照；GLB仍沿用完整内存高度场。
    pub(crate) fn from_sources(
        draft: &EditorSceneDraft,
        clipmap: Option<&DynamicClipmapGroundSurface>,
        terrain_revision: u64,
    ) -> Result<Self, String> {
        if draft.manifest.terrains.is_some() {
            let ground = draft.terrain_ground.as_ref().filter(|ground| ground.scene_id == draft.scene_id && ground.revision == terrain_revision)
                .ok_or("多地形真实承托面尚未完整采用当前作者修订")?;
            return Ok(Self { source: PlacementTerrainSource::Objects { ground: ground.clone(), reference: None } });
        }
        match draft.manifest.terrain.kind {
            SceneTerrainKind::SparseClipmap => {
                let snapshot = clipmap
                    .and_then(|surface| surface.snapshot.as_ref())
                    .filter(|snapshot| snapshot.scene_id == draft.scene_id)
                    .ok_or_else(|| "当前Clipmap地表快照尚未就绪".to_owned())?;
                if snapshot.authoring_height_revision() != Some(terrain_revision) {
                    return Err(format!(
                        "当前Clipmap地表仍在采用旧作者高度修订，期望{terrain_revision}，实际{:?}",
                        snapshot.authoring_height_revision(),
                    )); // 雕刻后的新页正式采用前，禁止用旧三角面推进摆放或保存完成态。
                }
                Ok(Self {
                    source: PlacementTerrainSource::SparseClipmap(snapshot.clone()), // Arc浅克隆，不复制高度页。
                })
            }
            SceneTerrainKind::Gltf => Err("当前旧场景地形没有可用高度源".to_owned()),
            SceneTerrainKind::Empty | SceneTerrainKind::Voxel => Err("此位置没有当前地形的有效承托面".into()),
        }
    }

    /// 分帧任务只关心与相机无关的地表内容换代，视角吸附不能让全图记录反复重扫。
    pub(crate) fn clipmap_content_revision(&self) -> Option<[u64; 7]> {
        match &self.source {
            PlacementTerrainSource::Objects { ground, .. } => Some([ground.revision, ground.generation, 0, 0, 0, 0, 0]),
            PlacementTerrainSource::SparseClipmap(snapshot)
            | PlacementTerrainSource::VolumeLayer {
                ground: snapshot, ..
            } => Some(snapshot.content_revision_words()),
            #[cfg(test)]
            PlacementTerrainSource::TestHeightfield { .. } => None,
            PlacementTerrainSource::SampledClipmap { content, .. } => Some(*content),
        }
    }

    /// 只有基础页发生变化才重新启动缺页批次，避免稳定等待帧反复调度CPU任务。
    pub(crate) fn authoritative_page_revision(&self) -> Option<(Option<u64>, bool)> {
        match &self.source {
            PlacementTerrainSource::SparseClipmap(snapshot) => {
                Some((snapshot.authoritative_page_revision(), snapshot.ready))
            } // 页绑定和正式采用完成门分别参与身份，不能只等页号变化。
            _ => None,
        }
    }

    /// 将完整有限点集换成只读采样器；调用方必须在采用前复核同一内容身份。
    pub(crate) fn from_clipmap_samples(
        content: [u64; 7],
        heights: BTreeMap<(u32, u32), f32>,
    ) -> Self {
        Self {
            source: PlacementTerrainSource::SampledClipmap {
                content,
                heights: Arc::new(heights),
                volume_ground: None,
            },
        }
    }

    pub(crate) fn retain_volume_ground(mut self, source: &Self) -> Self {
        if let PlacementTerrainSource::SampledClipmap { volume_ground, .. } = &mut self.source {
            *volume_ground = match &source.source {
                PlacementTerrainSource::SparseClipmap(g) => Some(g.clone()),
                PlacementTerrainSource::SampledClipmap { volume_ground, .. } => {
                    volume_ground.clone()
                }
                _ => None,
            };
        }
        self
    }
    /// 返回当前同代三角面高度；Clipmap基础作者页缺失时保留为明确等待状态。
    pub(crate) fn sample_status(
        &self,
        world_x: f32,
        world_z: f32,
    ) -> Result<PlacementTerrainSample, String> {
        match &self.source {
            PlacementTerrainSource::Objects { ground, reference } => match ground.support_status(world_x,world_z,*reference) {
                ClipmapGroundHit::Hit(point)=>Ok(PlacementTerrainSample::Height(point.y)),
                ClipmapGroundHit::WaitingForHeight=>Ok(PlacementTerrainSample::WaitingForHeight),
                _=>Err("此位置没有真实地形承托面".into()),
            },
            PlacementTerrainSource::VolumeLayer { ground, reference } => {
                if !ground.ready {
                    return Ok(PlacementTerrainSample::WaitingForHeight);
                }
                match ground.sample_support_below(Vec3::new(world_x, *reference, world_z), 32.0) {
                    ClipmapGroundHit::Hit(p) => Ok(PlacementTerrainSample::Height(p.y)),
                    ClipmapGroundHit::WaitingForHeight => {
                        Ok(PlacementTerrainSample::WaitingForHeight)
                    }
                    _ => Err("建筑地基没有命中所选三维承托层".into()),
                } // 跨体积边缘的五点仍允许同高度外表面承托，洞顶背面继续拒绝。
            }
            #[cfg(test)]
            PlacementTerrainSource::TestHeightfield {
                heights,
                resolution,
                world_min,
                world_max,
                base_height,
                height_scale,
            } => feathrise_scene_content::sample_heightfield_world(
                heights.as_slice(), // 共享当前完整量化高度。
                *resolution,        // 正式顶点分辨率。
                [world_x, world_z], // 实例原点的世界XZ。
                *world_min,         // 清单世界最小角。
                *world_max,         // 清单世界最大角。
                *base_height,       // 量化高度解码基线。
                *height_scale,      // 量化高度解码量程。
            )
            .map(PlacementTerrainSample::Height)
            .ok_or_else(|| format!("位置{world_x:.3},{world_z:.3}无法从当前内存地形采样")),
            PlacementTerrainSource::SparseClipmap(snapshot) => {
                match snapshot.sample_authoritative_surface(Vec2::new(world_x, world_z)) {
                    ClipmapGroundHit::Hit(point) => Ok(PlacementTerrainSample::Height(point.y)),
                    ClipmapGroundHit::WaitingForHeight => {
                        Ok(PlacementTerrainSample::WaitingForHeight)
                    }
                    ClipmapGroundHit::Miss => Err(format!(
                        "位置{world_x:.3},{world_z:.3}位于当前Clipmap场景范围外"
                    )),
                    ClipmapGroundHit::Failed => Err(format!(
                        "位置{world_x:.3},{world_z:.3}无法从当前Clipmap三角面采样"
                    )),
                }
            }
            PlacementTerrainSource::SampledClipmap { heights, .. } => heights
                .get(&(world_x.to_bits(), world_z.to_bits()))
                .copied()
                .map(PlacementTerrainSample::Height)
                .ok_or_else(|| "建筑批次缺少已冻结的精确地基样本".to_owned()),
        }
    }

    /// 按正式地形三角剖分取得指定世界X/Z的世界Y。
    pub(crate) fn sample(&self, world_x: f32, world_z: f32) -> Result<f32, String> {
        match self.sample_status(world_x, world_z)? {
            PlacementTerrainSample::Height(height) => Ok(height),
            PlacementTerrainSample::WaitingForHeight => {
                Err("当前Clipmap高度页尚未完整采用，请稍后重试".to_owned())
            }
        } // 显式编辑必须拿到真实高度；不能把等待或场景外解释为平地。
    }
    /// 只能在后台采样任务中调用；完整位置、朝向和尺寸绑定实际所选层。
    pub(crate) fn bind_pose(&self,pose:feathrise_scene_content::TerrainObjectTransform,reference:Option<f32>)
        ->Result<Option<feathrise_scene_content::SceneTerrainBinding>,String> {
        let PlacementTerrainSource::Objects {ground,reference:selected}=&self.source else {return Ok(None);};
        let Some((id,_))=ground.support_identity(pose.translation[0],pose.translation[2],reference.or(*selected))? else {return Err("内容落点没有真实地形承托面".into());};
        ground.bind(&id,pose).map(Some)
    }
    /// 属性/手柄只改内容姿态时更新源局部记录；O(log N)，无需重新执行网格射线。
    pub(crate) fn refresh_record_binding(&self,record:&mut ScenePlacementRecord)->Result<(),String> {
        let (PlacementTerrainSource::Objects{ground,..},Some(binding))=(&self.source,&record.terrain_binding) else {return Ok(());};
        let mut updated=ground.bind(&binding.terrain_id,feathrise_scene_content::TerrainObjectTransform {
            translation:record.translation,yaw_degrees:record.yaw_degrees,scale:record.scale,
            ..Default::default()
        })?;
        if record.height_mode.follows_terrain() {updated.local_height_offset=Some(record.terrain_offset/ground.bindings.pose(&binding.terrain_id).ok_or("关联地形已不存在")?.scale[1]);}
        record.terrain_binding=Some(updated);Ok(())
    }
    fn sample_record(&self,record:&ScenePlacementRecord)->Result<f32,String> {
        if let (PlacementTerrainSource::Objects{ground,..},Some(binding))=(&self.source,&record.terrain_binding) {
            let original=ground.bindings.resolve(binding)?;
            return match ground.support_on(&binding.terrain_id,record.translation[0],record.translation[2],original.translation[1]-record.terrain_offset+0.5) {
                ClipmapGroundHit::Hit(point)=>Ok(point.y),ClipmapGroundHit::WaitingForHeight=>Err("原承托页尚未就绪".into()),
                _=>Err("此位置没有原地形的实际承托面".into()),
            };
        }
        self.clone().for_selected_surface(Vec3::from_array(record.translation)).sample(record.translation[0],record.translation[2])
    }
}

/// 切换高度模式但保持实例当前世界Y不跳变。
pub(crate) fn set_height_mode_preserving_world_y(
    record: &mut ScenePlacementRecord,           // 当前完整作者记录。
    mode: ScenePlacementHeightMode,              // 用户明确选择的新模式。
    sampler: Option<&PlacementTerrainSampler>, // 只有切入跟随模式才要求当前地形采样器。
) -> Result<bool, String> {
    if record.height_mode == mode && !(mode==ScenePlacementHeightMode::FixedHeight&&record.terrain_binding.is_some()) {
        return Ok(false); // 重复点击已选模式不制造撤销或保存噪音。
    }
    match mode {
        ScenePlacementHeightMode::FollowTerrain => {
            let sampler = sampler.ok_or_else(|| "当前场景没有可用地形高度".to_owned())?; // 跟随模式不能借基准平面伪造高度。
            let ground = sampler.sample_record(record)?; // 以当前真实层切换，洞底不能借同XZ上层算离地偏移。
            let offset = record.translation[1] - ground; // 以离地偏移抵消模式变化，画面高度保持不动。
            if !offset.is_finite() {
                return Err(format!("实例{}的离地偏移无效", record.id)); // 非有限作者值不得进入JSON。
            }
            record.terrain_offset = offset; // 后续地形变化只改变地表项，不改变作者偏移。
        }
        ScenePlacementHeightMode::FixedHeight => {
            record.terrain_offset = 0.0; // 固定模式只认translation.y，清除无效残留避免JSON噪音。
            record.terrain_binding = None; // 明确切为世界固定内容，同时脱离原地形姿态。
        }
    }
    record.height_mode = mode; // 所有前置采样成功后才原子切换语义。
    if let Some(sampler)=sampler {sampler.refresh_record_binding(record)?;}
    Ok(true)
}

/// 跟随模式下，把当前世界Y解释为新的作者离地偏移。
pub(crate) fn update_follow_offset_from_world_y(
    record: &mut ScenePlacementRecord, // 数值输入已经写入新世界Y的完整记录。
    sampler: &PlacementTerrainSampler, // 当前地形高度修订采样器。
) -> Result<(), String> {
    if !record.height_mode.follows_terrain() {
        return Ok(()); // 固定高度的Y就是作者权威，不产生隐藏偏移。
    }
    let ground = sampler.sample_record(record)?; // 关联地形及承托层保持同一身份。
    let offset = record.translation[1] - ground; // 用户输入的世界Y转换为未来可稳定保持的离地量。
    if !offset.is_finite() {
        return Err(format!("实例{}的离地偏移无效", record.id)); // 防御极端输入溢出。
    }
    record.terrain_offset = offset; // translation.y保持用户刚输入的可见世界高度。
    sampler.refresh_record_binding(record)?;
    Ok(())
}

/// 跟随模式下按当前X/Z和作者偏移更新缓存世界Y。
pub(crate) fn reconcile_record_with_terrain(
    record: &mut ScenePlacementRecord,   // 只会修改派生translation.y。
    sampler: &PlacementTerrainSampler, // 当前地形高度修订采样器。
) -> Result<bool, String> {
    if !record.height_mode.follows_terrain() {
        return Ok(false); // 固定高度记录完全不响应地形变化。
    }
    if !record.terrain_offset.is_finite() {
        return Err(format!("实例{}的离地偏移无效", record.id)); // 公共变换校验前给出精确高度诊断。
    }
    let ground = sampler.sample_record(record)?; // 复用原对象与承托层的真实三角面。
    let resolved_y = ground + record.terrain_offset; // 唯一跟随公式：地表世界Y加作者偏移。
    if !resolved_y.is_finite() {
        return Err(format!("实例{}解析后的世界高度无效", record.id)); // 溢出不能写Transform。
    }
    if record.translation[1] == resolved_y {
        sampler.refresh_record_binding(record)?;
        return Ok(false); // 同一地形高度结果不重复写草稿或ECS。
    }
    record.translation[1] = resolved_y; // 保存文件同时保留当前修订的可直接运行世界高度。
    sampler.refresh_record_binding(record)?;
    Ok(true)
}

/// 保存入口读取的整批跟随高度完成状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PlacementGroundingReadiness {
    Ready,                                      // 当前场景、地形修订和摆放结构已经完整处理。
    Waiting { processed: usize, total: usize }, // 保留保存请求并按固定预算继续。
    Failed(String),                             // 损坏高度场或记录必须零IO停止保存。
}

/// 当前场景跟随型摆放的分帧游标与确定失败结果。
#[derive(Resource, Default)]
pub(crate) struct PlacementTerrainFollowState {
    scene_id: Option<SceneId>,                      // 当前游标所属场景。
    terrain_revision: u64,                      // 任一地形高度变化都让旧前缀失效。
    clipmap_content_revision: Option<[u64; 7]>, // 根、清单、作者高度、浅坑或拓扑换代都重跑当前可见记录。
    structure_revision: u64,                    // 增删摆放后数组下标整体重新解释。
    total: usize,                               // 冻结本轮数组长度供保存门复核。
    cursor: usize,                              // 下一条待检查记录下标。
    failure: Option<String>,                    // 同身份确定失败不在稳定帧重复工作。
    object_work: Option<object_follow::Work>,
}

impl PlacementTerrainFollowState {
    /// 读取指定完整身份的保存完成态；身份不匹配时视为尚未开始。
    pub(crate) fn readiness_for(
        &self,
        scene_id: &SceneId,          // 当前保存场景。
        terrain_revision: u64,   // 当前地形高度修订。
        structure_revision: u64, // 当前摆放数组结构修订。
        total: usize,            // 当前摆放记录总数。
    ) -> PlacementGroundingReadiness {
        if self.scene_id.as_ref() != Some(scene_id)
            || self.terrain_revision != terrain_revision
            || self.structure_revision != structure_revision
            || self.total != total
        {
            return PlacementGroundingReadiness::Waiting {
                processed: 0,
                total,
            }; // 下一次任务系统从新身份首项开始。
        }
        if let Some(error) = self.failure.as_ref() {
            return PlacementGroundingReadiness::Failed(error.clone()); // 保存边沿需要拥有可见诊断字符串。
        }
        if self.cursor >= self.total {
            PlacementGroundingReadiness::Ready // 完整数组前缀已经采用到草稿和当前可见根。
        } else {
            PlacementGroundingReadiness::Waiting {
                processed: self.cursor, // 已完成前缀可直接展示进度。
                total: self.total,      // 冻结总量防止百分比倒退。
            }
        }
    }

    /// 草稿、地形高度或摆放数组换代时O(1)重置游标，不析构大型临时集合。
    fn prepare(
        &mut self,
        scene_id: &SceneId,
        terrain_revision: u64,
        clipmap_content_revision: Option<[u64; 7]>,
        structure_revision: u64,
        total: usize,
    ) {
        if self.scene_id.as_ref() == Some(scene_id)
            && self.terrain_revision == terrain_revision
            && self.clipmap_content_revision == clipmap_content_revision
            && self.structure_revision == structure_revision
            && self.total == total
        {
            return; // 稳定帧继续当前前缀，不重复从零扫描。
        }
        self.scene_id = Some(scene_id.clone()); // 新场景身份只克隆一个短稳定ID。
        self.terrain_revision = terrain_revision; // 冻结本轮地形高度版本。
        self.clipmap_content_revision = clipmap_content_revision; // 只绑定内容，不把相机中心当高度修改。
        self.structure_revision = structure_revision; // 冻结本轮数组下标语义。
        self.total = total; // 保存门随后必须看到同一长度。
        self.cursor = 0; // 新身份从首条记录开始。
        self.failure = None; // 作者修复或新地形修订允许重新尝试。
    }

    /// 没有作者草稿时释放旧身份，下一个场景不能继承Ready信用。
    fn clear(&mut self) {
        let work=self.object_work.take();
        *self = Self::default(); self.object_work=work; // 取消也保留任务槽，真实结束后才回收。
    }
}

/// 在地形笔划稳定后按固定记录预算更新所有跟随型实例及其当前可见根。
#[allow(clippy::too_many_arguments)]
pub(crate) fn reconcile_following_placements(
    mut commands: Commands,                  // 每条变化只写一个定向根Transform。
    editor: Res<SceneEditorState>,             // F12关闭后不再生成作者预览。
    terrain_stroke: Res<TerrainBrushStroke>, // 连续雕刻期间合并到最终地形修订。
    water_stroke: Res<WaterBrushStroke>,     // 复合静水笔刷同样可能修改地形河床。
    clipmap: Option<Res<DynamicClipmapGroundSurface>>, // Clipmap只共享当前正式CPU页和环带快照。
    asset_server: Res<AssetServer>,          // 正式根缺失时分帧补Scene0作者预览。
    root_index: Res<PlacementInstanceIndex>, // 按稳定ID定向少量并存根。
    roots: Query<(Entity, &ScenePlacementInstance)>, // 根索引结果仍复核活动身份。
    mut session: ResMut<EditorAuthoringSession>, // 当前草稿与两项专用修订。
    mut state: ResMut<PlacementTerrainFollowState>, // 唯一固定预算前向游标。
) {
    let Some(draft) = session.draft.as_ref() else {
        state.clear(); // 切换场景加载空窗不能保留上一个场景完成信用。
        return;
    };
    let scene_id = draft.scene_id.clone(); // 结束只读借用后允许更新记录和预览所有权。
    let terrain_revision = session.terrain_revision; // 本轮只接受唯一地形高度修订。
    let structure_revision = session.placement_structure_revision; // 数组增删使旧游标失效。
    let total = draft.placements.placements.len(); // 最多8192，仅读取常数时间长度。
    let sampler =
        PlacementTerrainSampler::from_sources(draft, clipmap.as_deref(), terrain_revision); // 两条路线均只O(1)冻结不可变高度输入，并严格等待当前作者高度正式采用。
    let clipmap_content_revision = sampler
        .as_ref()
        .ok()
        .and_then(PlacementTerrainSampler::clipmap_content_revision); // 旧或缺失快照用None撤销旧Ready信用。
    state.prepare(
        &scene_id,
        terrain_revision,
        clipmap_content_revision,
        structure_revision,
        total,
    ); // 身份变化只重置轻量游标。
    if !editor.open || session.saving || terrain_stroke.active || water_stroke.active {
        return; // 连续笔划和冻结保存期间不逐帧追赶中间高度。
    }
    if state.failure.is_some() || state.cursor >= total {
        return; // 确定失败或完整Ready稳定帧保持O(1)。
    }

    if (draft.manifest.terrain.kind == SceneTerrainKind::SparseClipmap || draft.manifest.terrains.is_some()) && sampler.is_err() {
        return; // 场景页与正式环带仍在换代时保持等待，不能把启动空窗固化成保存失败。
    }
    let bounds = draft.manifest.world_bounds(); // 变化记录同步ECS前复用同一受检XZ范围。
    let start = state.cursor; // 当前帧固定前缀起点。
    let end = start
        .saturating_add(PLACEMENT_GROUNDING_RECORD_BUDGET_PER_FRAME)
        .min(total); // 最坏每帧只访问公开预算数量的记录。
    let mut changed_records = Vec::new(); // 容量严格受单帧预算限制，不随全图记录数增长。
    let mut failure = None; // 首个确定错误停止本轮，避免发布后续半合法记录。
    let mut next_cursor = start; // Clipmap缺页时只提交已完成前缀，当前项留待后续帧重试。
    if draft.manifest.terrains.is_some() {
        let Some(sampler)=sampler.as_ref().ok() else { return; };
        let Some(result)=object_follow::advance(&mut state,draft,sampler,terrain_revision,structure_revision,clipmap_content_revision,start,end) else { return; };
        match result {
            Ok((next,changes))=> { next_cursor=next;
                let draft=session.draft.as_mut().unwrap();
                for change in changes {
                    if let Some(after) = change.apply(&mut draft.placements.placements) {
                        changed_records.push(after);
                    }
                }
            }
            Err(error)=>failure=Some(error),
        }
    } else {
        let draft = session.draft.as_mut().expect("上方已经确认作者草稿存在");
        for record_index in start..end {
            let record = &mut draft.placements.placements[record_index]; // 冻结结构修订保证下标语义稳定。
            if !record.height_mode.follows_terrain() {
                next_cursor = record_index + 1; // 固定高度记录已经完成本轮检查。
                continue; // 固定高度只承担一次轻量分支，不访问内存高度场。
            }
            let sampler = match sampler.as_ref() {
                Ok(value) => value, // 当前高度场可按正式规则采样。
                Err(error) => {
                    failure = Some(format!("实例{}无法跟随地形：{error}", record.id));
                    break; // 同身份后续帧直接返回Failed。
                }
            };
            match sampler.sample_status(record.translation[0], record.translation[2]) {
                Ok(PlacementTerrainSample::Height(ground)) => {
                    if !record.terrain_offset.is_finite() {
                        failure = Some(format!("实例{}的离地偏移无效", record.id));
                        break; // 坏作者值不能进入世界Transform或保存快照。
                    }
                    let resolved_y = ground + record.terrain_offset; // 与公共单记录跟随公式一致。
                    if !resolved_y.is_finite() {
                        failure = Some(format!("实例{}解析后的世界高度无效", record.id));
                        break;
                    }
                    if record.translation[1] != resolved_y {
                        record.translation[1] = resolved_y;
                        changed_records.push(record.clone()); // 单帧最多克隆128条小型完整记录供借用外同步。
                    }
                    next_cursor = record_index + 1; // 同代真实高度已经完整采用。
                }
                Ok(PlacementTerrainSample::WaitingForHeight) => {
                    break; // 保留当前记录，页采用后从同一项继续，绝不写零平面。
                }
                Err(error) => {
                    failure = Some(error); // 坏偏移或坏采样禁止保存。
                    break;
                }
            }
        }
    }
    if !changed_records.is_empty() {
        // 前缀可能已改值而随后失败；先发布实际采用代次，旧选集和后台结果不能继续沿用原姿态。
        session.placement_derived_pose_revision = session.placement_derived_pose_revision.wrapping_add(1);
    }
    if let Some(error) = failure {
        state.failure = Some(error); // 保持当前游标，作者换代后才重新尝试。
        return;
    }
    state.cursor = next_cursor; // 只推进真实完成或明确不受当前局部页影响的前缀。
    if changed_records.is_empty() {
        return; // 固定记录或同高度结果不产生Commands。
    }
    let Some(bounds) = bounds else {
        state.failure = Some("当前场景清单世界范围无效，无法同步跟随型资产".to_owned());
        return; // 草稿Y已经由有效内存高度解析，但坏清单不得进入Transform或保存。
    };
    for record in changed_records {
        if let Err(error) = sync_placement_record_world(
            &mut commands, // 应用当前解析世界高度。
            &asset_server, // 缺根时补作者Scene0。
            &mut session,  // 维护精确预览所有权。
            &root_index,   // 稳定ID定向根。
            &roots,        // 复核活动批次身份。
            &record,       // 当前完整派生记录。
            bounds,        // 当前场景正式XZ范围。
            true,          // 跟随结果保持可见。
            true,          // F12内缺根时必须补预览。
        ) {
            state.failure = Some(format!("实例{}重贴地预览失败：{error}", record.id));
            return; // 保存门将阻止当前半同步结果落盘，下一作者修订可重试。
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 纯高度语义回归不构造完整Bevy世界。

    pub(super) fn record() -> ScenePlacementRecord {
        ScenePlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: "placement-grounding".to_owned(), // 稳定测试身份。
            asset_path: "gltf/test.glb".to_owned(), // 纯采样不加载Scene0。
            asset_target: Default::default(),     // 高度测试不改变文件内目标。
            building_preset_id: None,             // 普通贴地夹具没有建筑目录语义。
            courtyard: None,                      // 独立摆放记录不属于任何院落副本。
            translation: [1.0, 12.0, 1.0],        // 当前世界Y高于测试地表两米。
            height_mode: ScenePlacementHeightMode::FixedHeight, // 模拟旧记录兼容起点。
            terrain_offset: 0.0,                  // 固定模式没有有效偏移。
            yaw_degrees: 0.0,                     // 姿态与本测试无关。
            scale: [1.0, 1.0, 1.0],               // 单位缩放保持记录合法。
            water_surfaces: Vec::new(),           // 高度语义不修改网格水。
        }
    }

    fn sampler(heights: [u16; 4]) -> PlacementTerrainSampler {
        PlacementTerrainSampler {
            source: PlacementTerrainSource::TestHeightfield {
                heights: Arc::new(heights.to_vec()), // 2×2测试高度场拥有独立Arc。
                resolution: [2, 2],                  // 单个正式地形单元。
                world_min: [0.0, 0.0],               // 西北角世界坐标。
                world_max: [1.0, 1.0],               // 东南角世界坐标。
                base_height: 0.0,                    // 零样本对应零米。
                height_scale: 10.0,                  // 最大样本对应十米。
            },
        }
    }

    #[test]
    fn switching_modes_preserves_world_height_and_following_keeps_offset() {
        let high = sampler([u16::MAX; 4]); // 当前平坦地表为十米。
        let mut value = record(); // 实例当前位于十二米。

        assert!(
            set_height_mode_preserving_world_y(
                &mut value,
                ScenePlacementHeightMode::FollowTerrain,
                Some(&high),
            )
            .expect("合法内存高度场应允许切到跟随模式")
        );
        assert_eq!(value.translation[1], 12.0); // 切换按钮不能让画面瞬移。
        assert_eq!(value.terrain_offset, 2.0); // 当前两米离地量成为作者不变量。

        let low = sampler([0; 4]); // 后续地形整体降到零米。
        assert!(reconcile_record_with_terrain(&mut value, &low).expect("跟随采样应成功"));
        assert_eq!(value.translation[1], 2.0); // 世界Y随地表下降并继续保留两米偏移。

        assert!(
            set_height_mode_preserving_world_y(
                &mut value,
                ScenePlacementHeightMode::FixedHeight,
                None,
            )
            .expect("固定高度不依赖内存高度场")
        );
        assert_eq!(value.translation[1], 2.0); // 切回固定仍保持当前画面高度。
        assert_eq!(value.terrain_offset, 0.0); // 无效偏移被规范化清除。
    }

    #[test]
    fn world_y_edit_updates_follow_offset_without_moving_again() {
        let terrain = sampler([u16::MAX; 4]); // 地表固定十米。
        let mut value = record();
        value.height_mode = ScenePlacementHeightMode::FollowTerrain; // 模拟已经启用跟随。
        value.translation[1] = 13.5; // 用户在数值框输入新的世界高度。

        update_follow_offset_from_world_y(&mut value, &terrain).expect("合法世界Y应转换为偏移");
        assert_eq!(value.terrain_offset, 3.5); // 后续地形变化保持新的离地量。
        assert_eq!(value.translation[1], 13.5); // 本次输入画面高度不被二次量化。
    }

    #[test]
    fn readiness_is_bound_to_scene_terrain_structure_and_length() {
        let scene_id = SceneId::parse("MAP-grounding-test").expect("测试场景ID合法");
        let mut state = PlacementTerrainFollowState::default(); // 初始没有任何场景完成信用。
        state.prepare(&scene_id, 7, None, 3, 2); // 建立指定地形高度和数组身份。
        assert_eq!(
            state.readiness_for(&scene_id, 7, 3, 2),
            PlacementGroundingReadiness::Waiting {
                processed: 0,
                total: 2
            },
        );
        state.cursor = 2; // 模拟完整处理两条记录。
        assert_eq!(
            state.readiness_for(&scene_id, 7, 3, 2),
            PlacementGroundingReadiness::Ready
        );
        assert_eq!(
            state.readiness_for(&scene_id, 8, 3, 2),
            PlacementGroundingReadiness::Waiting {
                processed: 0,
                total: 2
            },
        ); // 新地形修订不能借用旧Ready。

        state.prepare(&scene_id, 7, Some([1, 2, 7, 1, 0, 0, 3]), 3, 2);
        state.cursor = 2; // 模拟当前Clipmap内容已经完整处理。
        state.prepare(&scene_id, 7, Some([1, 2, 7, 1, 0, 0, 4]), 3, 2);
        assert_eq!(
            state.readiness_for(&scene_id, 7, 3, 2),
            PlacementGroundingReadiness::Waiting {
                processed: 0,
                total: 2,
            },
        ); // 同一作者修订下拓扑或其他正式内容换代也必须撤销旧完成前缀。
    }
}
