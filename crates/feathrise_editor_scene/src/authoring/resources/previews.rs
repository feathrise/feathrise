//! 地形、地表、植被与混合水网的有界预览任务和采用状态。

use std::collections::{BTreeMap, VecDeque}; // 预览资产确定排序，水候选逐帧弹出一张完成Mesh。
#[cfg(any(test, feature = "runtime_test_support"))]
use std::time::Duration; // 退休有限网格夹具仍验证文件交接的真实时间截止。

use avian3d::prelude::Collider; // 未保存高度场后台完成值直接携带正式物理形状。
use bevy::prelude::*; // 预览资源、实体、资产句柄和变换。
use bevy::tasks::Task; // 每类重计算保持至多一个在途后台任务。
use feathrise_scene_content::{SceneId, TerrainId}; // 全部采用门同时绑定稳定场景与对象身份。
use feathrise_scene::{
    BuiltWaterSurface, PaintedWaterSurfaceSpawn, StagedWaterSurface, WaterGpuUploadTicket,
}; // 作者预览复用正式GPU水面完成值、候选资产与回执身份。

/// 地形结果绑定作者槽与采样输入，不能仅凭同图修订把A对象的结果采用到B对象。
type TerrainPreviewTaskResult = (TerrainPreviewIdentity, Result<TerrainPreviewBuild, String>);

#[derive(Clone)]
pub(crate) struct TerrainPreviewIdentity {
    pub(crate) scene_id: SceneId,
    pub(crate) terrain_id: Option<feathrise_scene_content::TerrainId>,
    pub(crate) revision: u64,
    pub(crate) spec: feathrise_terrain::TerrainSampleSpec,
    pub(crate) source_path: String,
    pub(crate) source_sha256: Option<[u8; 32]>,
    /// 只比较仍由草稿持有的分配地址，不解引用或持有Arc/Weak；Weak也会阻止Arc::get_mut。
    /// 地址同时绑定对象、源路径/摘要和作者修订，不单独作为内容身份。
    pub(crate) mesh_identity: usize,
}

impl PartialEq for TerrainPreviewIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.scene_id == other.scene_id
            && self.terrain_id == other.terrain_id
            && self.revision == other.revision
            && self.source_path == other.source_path
            && self.source_sha256 == other.source_sha256
            && self.mesh_identity == other.mesh_identity
            && self.spec.resolution == other.spec.resolution
            // 损坏的NaN参数也有稳定输入身份；同一失败不能因为NaN != NaN而每帧重试。
            && self.spec.world_min.map(f32::to_bits) == other.spec.world_min.map(f32::to_bits)
            && self.spec.world_max.map(f32::to_bits) == other.spec.world_max.map(f32::to_bits)
            && self.spec.base_height.to_bits() == other.spec.base_height.to_bits()
            && self.spec.height_scale.to_bits() == other.spec.height_scale.to_bits()
    }
}

/// 一个GLB草稿修订在后台一次完成的画面、物理与内存高度派生值。
pub(crate) struct TerrainPreviewBuild {
    #[cfg(test)]
    pub(crate) mesh: Mesh,         // 保持原拓扑且带重算法线的正式预览Mesh。
    #[cfg(test)]
    pub(crate) collider: Collider, // 与上方Mesh同顶点和索引的Avian Trimesh。
    #[cfg(test)]
    pub(crate) heights: Vec<u16>,  // 水域、植被与边界共享的规则CPU查询网格。
    #[cfg(test)]
    pub(crate) height_cells_rgba16: Vec<u8>, // GPU Blade使用的NW/NE/SW/SE内存Image字节。
}

/// 地表综合色后台结果携带场景与修订身份。
type SurfacePreviewTaskResult = (SceneId, Option<TerrainId>, u64, u64, Result<SurfacePreviewImage, String>);

/// 植被散布后台结果携带场景、作者层与修订身份。
type VegetationPreviewTaskResult = (
    SceneId,
    String,
    u64,
    u64,
    u64, // 工程预设目录修订；热更新后的旧下扎结果不能采用。
    u64, // 该目录实际就绪的原型登记修订，禁止新目录借旧模型安装。
    Result<VegetationPreviewBuild, String>,
); // 场景、植被编译修订和建筑净空修订共同决定完整预览身份。

/// 水网外观身份只保存已采用的正式水体预设目录修订。
pub(crate) type WaterPreviewPresetRevision = u64; // 未保存预设只渲染独立样品，不再换代整个场景。

/// 只有实际生成GLB作者水面的来源才有替代正式水面的权限；根水与Clipmap的Ready映射不拥有它。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WaterPreviewSource {
    pub(crate) scene_id: SceneId,
    /// None只表示旧版单GLB场景的无owner根；独立根水不会创建本身份。
    pub(crate) terrain_id: Option<TerrainId>,
}

/// 来源稳定、实例可热换代；每次按完整已采用路径解析当前owner，不能把兄弟实例一起隐藏。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaterPreviewRuntimeTarget {
    Root,
    Instance(Entity),
}

/// 水网后台结果把同一来源、同一水域/地形/外观修订的全部水体作为一个原子批次返回。
type WaterPreviewTaskResult = (
    WaterPreviewSource,                     // 冻结真实替代来源，迟到结果不能借用新的属性焦点。
    u64,                                    // 水域JSON与R8作者修订。
    u64,                                    // GLB地形派生高度修订。
    WaterPreviewPresetRevision,             // 已采用正式目录的稳定外观身份。
    Result<Vec<BuiltWaterSurface>, String>, // 完整水网完成值或整批失败原因。
);

/// GLB 地形实时预览的有界后台重建状态。
#[derive(Resource, Default)]
pub(crate) struct TerrainPreviewState {
    pub(crate) desired_identity: Option<TerrainPreviewIdentity>,
    pub(crate) applied_identity: Option<TerrainPreviewIdentity>,
    #[cfg(test)]
    pub(crate) failed_identity: Option<TerrainPreviewIdentity>,
    #[cfg(test)]
    pub(crate) desired_scene_id: Option<SceneId>,
    #[cfg(test)]
    pub(crate) desired_revision: u64,
    pub(crate) applied_scene_id: Option<SceneId>,
    pub(crate) applied_revision: u64,
    pub(crate) task: Option<Task<TerrainPreviewTaskResult>>,
    #[cfg(test)]
    pub(crate) retirement: Option<Task<()>>, // 迟到大结果最多占一个回收槽，结束前不再启动下一次派生。
    #[cfg(test)]
    pub(crate) entity: Option<Entity>,
    pub(crate) mesh: Option<Handle<Mesh>>,
    pub(crate) height_image: Option<Handle<Image>>, // 当前作者修订的内存RGBA16高度单元强句柄。
    /// 作者白模可见时暂存正式地形原显隐，避免降低后的新网格被旧表面深度遮挡。
    #[cfg(test)]
    pub(crate) hidden_runtime_entities: Vec<(Entity, Visibility)>,
}

impl TerrainPreviewState {
    /// 保存只能读取与当前GLB作者修订完全相同的一整套Mesh、Trimesh和内存高度派生值。
    pub(crate) fn ready_for(&self, scene_id: &SceneId, revision: u64) -> bool {
        self.applied_scene_id.as_ref() == Some(scene_id)
            && self.applied_revision == revision
            && self.applied_identity == self.desired_identity
            && self.mesh.is_some()
            && self.height_image.is_some() // 两个句柄只在后台全套派生成功后一起登记。
    }
}

/// 退休有限网格夹具等待运行时同一GLB代次的完整派生结果取得所有权。
#[cfg(any(test, feature = "runtime_test_support"))]
#[derive(Clone, Debug)]
pub(crate) struct TerrainFileHandoff {
    pub(crate) scene_id: SceneId, // 文件结果只能交给本次保存的场景，切换场景后立即失效。
    pub(crate) terrain_revision: u64, // 新笔划会换代并取消这次旧文件交接。
    pub(crate) generation_baseline: u64, // 只接受保存发布后出现并完整采用的新GLB派生代次。
    pub(crate) started_at: Duration, // 真实时间有界截止，丢失资产事件也不能永久锁住F12。
}

/// 未保存GLB草稿对正式动态地形画面与物理的联合接管状态。
#[derive(Resource, Default)]
pub(crate) struct TerrainLiveApplyState {
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub(crate) desired_scene_id: Option<SceneId>, // 最新等待应用的场景身份，迟到任务必须匹配。
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub(crate) desired_revision: u64,         // 最新等待应用的地形专用修订号。
    pub(crate) active_scene_id: Option<SceneId>,  // 当前已经拥有完整Mesh与Collider候选的场景。
    pub(crate) active_revision: u64,          // 当前候选对应的地形修订。
    pub(crate) active_mesh: Option<Handle<Mesh>>, // 与碰撞同修订的编辑器预览Mesh强句柄。
    pub(crate) active_collider: Option<Collider>, // 可在GLB热重载覆盖后重新安装的实际拓扑Trimesh。
    pub(crate) ready: bool, // 正式primitive已同时观察到正确Mesh、Collider和作者标记。
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub(crate) failed_scene_id: Option<SceneId>, // 最近一次确定失败所属场景，避免稳定帧无限重试。
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub(crate) failed_revision: Option<u64>, // 最近一次确定失败修订；新笔划会自然解除失败门。
    pub(crate) candidate_epoch: u64, // 同图同修订候选失效重建后，外部消费者仍能识别新的采用生命周期。
    /// 保存成功后保留内存候选强句柄，直到运行时同一新代次全套派生值就绪。
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub(crate) file_handoff: Option<TerrainFileHandoff>,
}

impl TerrainLiveApplyState {
    /// 当前是否已经持有指定作者修订的完整候选，尚待ECS确认时也不重复构建。
    pub(crate) fn has_candidate(&self, scene_id: &SceneId, revision: u64) -> bool {
        self.active_scene_id.as_ref() == Some(scene_id) // 候选必须属于同一个场景。
            && self.active_revision == revision // 候选必须与当前高度修订完全一致。
            && self.active_mesh.is_some() // 画面与碰撞缺一不可。
            && self.active_collider.is_some() // 物理候选必须已经完整构建。
    }

    /// 未保存地形只有在同一修订的画面与碰撞都落到正式primitive后才允许关闭F12。
    pub(crate) fn ready_for(&self, scene_id: &SceneId, revision: u64) -> bool {
        self.ready // ECS已跨帧确认组件实际存在。
            && self.has_candidate(scene_id, revision) // 不能拿上一笔划的完整结果冒充当前草稿。
    }

    /// 当前地形修订已经确定构建失败时，保存请求必须停止等待并归还作者输入。
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub(crate) fn failed_for(&self, scene_id: &SceneId, revision: u64) -> bool {
        self.failed_scene_id.as_ref() == Some(scene_id) && self.failed_revision == Some(revision)
    }

    /// 仅退休有限网格夹具建立GLB交接；正式高度地形已统一采用Clipmap分页源。
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub(crate) fn begin_file_handoff(
        &mut self,
        scene_id: SceneId,
        terrain_revision: u64,
        generation_baseline: u64,
        started_at: Duration,
    ) -> Result<(), String> {
        if !self.ready || !self.has_candidate(&scene_id, terrain_revision) {
            return Err("保存完成时未保存地形的Mesh/Trimesh尚未形成可验证联合候选".to_owned());
        } // IO启动门通常已经等待ready；这里继续阻止未来调度改动绕过原子候选门。
        self.file_handoff = Some(TerrainFileHandoff {
            scene_id,              // 冻结保存成功的场景身份。
            terrain_revision,    // 冻结与派生GLB完全相同的高度修订。
            generation_baseline, // 后续必须观察严格更新且完整采用的运行时代次。
            started_at,          // F12暂停期间仍推进的真实时间起点。
        });
        Ok(())
    }

    /// 当前关闭请求是否仍需等待同一个场景的文件权威交接。
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub(crate) fn file_handoff_pending_for(&self, scene_id: &SceneId, revision: u64) -> bool {
        self.file_handoff.as_ref().is_some_and(|handoff| {
            &handoff.scene_id == scene_id && handoff.terrain_revision == revision
        })
    }

    /// 释放旧联合候选但保留最新目标与在途任务，供下一笔划single-flight结果继续完成。
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub(crate) fn discard_candidate(&mut self) {
        self.active_scene_id = None; // 旧候选不得继续覆盖正式地形。
        self.active_mesh = None; // 释放旧预览Mesh强句柄。
        self.active_collider = None; // 释放旧高度场碰撞共享形状。
        self.ready = false; // 下一候选必须重新完成跨帧联合确认。
        self.candidate_epoch = self.candidate_epoch.wrapping_add(1); // 轻量生命周期票据不持有或复制整图数据。
    }

    /// 场景生命周期结束时撤销接管身份。
    #[cfg(any(test, feature = "runtime_test_support"))]
    pub(crate) fn invalidate(&mut self) {
        self.desired_scene_id = None; // 迟到任务完成后因身份不匹配直接丢弃。
        self.discard_candidate(); // 旧场景联合候选不得进入下一张动态场景。
        self.file_handoff = None; // 旧Scene的迟到Modified事件不得影响下一个场景。
        self.failed_scene_id = None; // 旧场景失败不阻止新场景首次任务。
        self.failed_revision = None; // 清除旧失败修订。
    }
}

#[cfg(test)]
mod terrain_live_apply_tests {
    use super::*; // 只验证联合候选身份门，不启动渲染或物理世界。

    #[test] // F12关闭绝不能把上一笔划的完整结果当成当前修订。
    fn ready_requires_the_same_scene_and_terrain_revision() {
        let scene_id = SceneId::parse("MAP-LIVE-TEST").expect("测试场景ID应有效"); // 使用正式SceneId校验路径。
        let mut state = TerrainLiveApplyState {
            // 构造已经拥有画面与碰撞的候选。
            active_scene_id: Some(scene_id.clone()), // 候选属于测试场景。
            active_revision: 7,                  // 候选绑定第七次高度变化。
            active_mesh: Some(Handle::default()), // 强句柄存在即可满足联合候选门。
            active_collider: Some(Collider::cuboid(1.0, 1.0, 1.0)), // 测试无需构造真实Trimesh。
            ready: true,                         // 模拟正式primitive已经跨帧确认。
            ..default()                          // 其他任务与失败字段保持空。
        };
        assert!(state.ready_for(&scene_id, 7)); // 完全相同身份才允许关闭。
        assert!(!state.ready_for(&scene_id, 8)); // 新笔划必须等待自己的碰撞。
        let other = SceneId::parse("MAP-LIVE-OTHER").expect("第二个测试场景ID应有效"); // 构造切换场景身份。
        assert!(!state.ready_for(&other, 7)); // 相同数字修订也不能跨场景复用。
        state.failed_scene_id = Some(scene_id.clone());
        state.failed_revision = Some(8);
        assert!(state.failed_for(&scene_id, 8)); // 确定失败同样严格绑定场景和修订。
        assert!(!state.failed_for(&scene_id, 7)); // 旧失败不得阻止另一高度修订。
        state.invalidate(); // 生命周期结束应释放联合候选。
        assert!(!state.ready_for(&scene_id, 7)); // 失效后旧完成证据不得残留。
    }

    #[test]
    fn preview_ready_requires_current_mesh_and_height_image() {
        let scene_id = SceneId::parse("MAP-PREVIEW-READY").expect("测试场景ID应有效");
        let mut preview = TerrainPreviewState {
            applied_scene_id: Some(scene_id.clone()),
            applied_revision: 3,
            mesh: Some(Handle::default()),
            height_image: Some(Handle::default()),
            ..default()
        };

        assert!(preview.ready_for(&scene_id, 3));
        assert!(!preview.ready_for(&scene_id, 2));
        preview.height_image = None;
        assert!(!preview.ready_for(&scene_id, 3));
    }

    #[test] // 保存后的文件交接必须同时绑定场景、地形修订和运行时派生代次。
    fn file_handoff_is_revision_scoped_and_invalidated_with_the_scene() {
        let scene_id = SceneId::parse("MAP-HANDOFF-TEST").expect("测试场景ID应有效");
        let live_mesh = Handle::<Mesh>::default(); // 保存门必须冻结当前已采用的编辑器临时Mesh身份。
        let mut state = TerrainLiveApplyState {
            active_scene_id: Some(scene_id.clone()),
            active_revision: 11,
            active_mesh: Some(live_mesh.clone()),
            active_collider: Some(Collider::cuboid(1.0, 1.0, 1.0)),
            ready: true,
            ..default()
        };

        state
            .begin_file_handoff(scene_id.clone(), 11, 29, Duration::from_secs(3))
            .expect("完整实时候选应允许建立文件交接");

        assert!(state.file_handoff_pending_for(&scene_id, 11)); // 完全相同身份必须阻止提前关闭。
        assert!(!state.file_handoff_pending_for(&scene_id, 12)); // 新笔划不能继承旧文件完成门。
        let handoff = state.file_handoff.as_ref().expect("交接身份应完整登记");
        assert_eq!(handoff.generation_baseline, 29); // 保存前旧Ready代次不得冒充新文件结果。
        assert_eq!(handoff.started_at, Duration::from_secs(3)); // 截止基于真实时间而不是虚拟时间。
        state.invalidate();
        assert!(!state.file_handoff_pending_for(&scene_id, 11)); // 切换场景后旧Scene事件不再拥有采用资格。
    }

    #[test] // 保存完成不能在实时画面与碰撞尚未原子落地时建立一个不可证明的文件门。
    fn file_handoff_requires_a_ready_live_mesh_candidate() {
        let scene_id = SceneId::parse("MAP-HANDOFF-NOT-READY").expect("测试场景ID应有效");
        let mut state = TerrainLiveApplyState::default();
        let result = state.begin_file_handoff(scene_id, 3, 0, Duration::ZERO);
        assert!(result.is_err()); // 缺少临时Mesh身份时不能靠任意后续Trimesh提前完成。
        assert!(state.file_handoff.is_none()); // 失败不得留下永久阻止F12的半套事务。
    }
}

/// 后台派生正式四槽索引与权重纹理，完整GPU候选就绪后才发布PBR材质。
#[derive(Resource, Default)]
pub(crate) struct SurfacePreviewState {
    pub(crate) desired_scene_id: Option<SceneId>,
    pub(crate) desired_terrain_id: Option<TerrainId>,
    pub(crate) desired_revision: u64,
    pub(crate) desired_preset_revision: u64,
    pub(crate) applied_scene_id: Option<SceneId>,
    pub(crate) applied_terrain_id: Option<TerrainId>,
    pub(crate) applied_revision: u64,
    pub(crate) applied_preset_revision: u64,
    pub(crate) task: Option<Task<SurfacePreviewTaskResult>>,
    pub(crate) image: Option<Handle<Image>>,
    pub(crate) material:
        Option<Handle<feathrise_environment::terrain_surface::TerrainSurfaceMaterial>>, // 当前完整PBR材质。
    pub(crate) crater_response: Option<std::sync::Arc<feathrise_scene_content::SurfaceCraterResponse>>, // 与当前完整PBR候选同代采用。
    pub(crate) candidate: Option<SurfacePreviewCandidate>, // 唯一待GPU确认候选，与CPU任务共同实施背压。
    pub(crate) failed_identity: Option<(SceneId, Option<TerrainId>, u64, u64)>, // 同一对象错误修订只尝试一次，避免连续帧重算。
}

/// 后台完成配对Mip编码和GPU层表编码的拥有结果。
pub(crate) struct SurfacePreviewImage {
    pub(crate) crater_response: Option<std::sync::Arc<feathrise_scene_content::SurfaceCraterResponse>>, // 复用作者Arc的只读玩法响应，绝不在发布帧复制画布。
    pub(crate) indices: Image, // 整数索引不能退化为综合色。
    pub(crate) weights: Image, // 与索引具有同一完整Mip链。
    pub(crate) layer_table: bevy::render::storage::ShaderBuffer, // 任意层表的CPU编码也在后台完成。
    pub(crate) layer_count: u32, // 零层用GPU哨兵，作者层数仍保持零。
    pub(crate) array_paths: [String; 3], // 与表切片一致的全局三数组路径。
    pub(crate) world_min: Vec2, // 权重UV复用正式世界范围。
    pub(crate) world_max: Vec2, // 不使用编辑器缩略比例改变世界平铺。
}

/// 登记完成而尚未对视口发布的唯一PBR候选。
pub(crate) struct SurfacePreviewCandidate {
    pub(crate) crater_response: Option<std::sync::Arc<feathrise_scene_content::SurfaceCraterResponse>>, // GPU就绪前与候选一起隐藏。
    pub(crate) scene_id: SceneId,        // 精确场景身份。
    pub(crate) terrain_id: Option<TerrainId>, // 同图另一个画布不能借用旧候选或GPU证据。
    pub(crate) revision: u64,        // 精确作者修订。
    pub(crate) preset_revision: u64, // 全局数组及PBR参数修订。
    pub(crate) material: Handle<feathrise_environment::terrain_surface::TerrainSurfaceMaterial>, // 候选绑定组。
    pub(crate) images: Vec<Handle<Image>>, // 完整索引、权重、三数组及零槽。
    pub(crate) layer_table: Handle<bevy::render::storage::ShaderBuffer>, // 完整局部层表。
    pub(crate) ready: std::sync::Arc<std::sync::atomic::AtomicBool>, // 只由候选RenderApp证据发布。
    pub(crate) failed: std::sync::Arc<std::sync::atomic::AtomicBool>, // 管线明确失败时解除等待但保持旧画面。
    pub(crate) started: std::time::Instant, // 失败超时保持上一个有效材质。
}

/// 当前植被层的确定性有限模型预览。
#[derive(Resource, Default)]
pub(crate) struct VegetationPreviewState {
    pub(crate) desired_scene_id: Option<SceneId>,
    pub(crate) desired_layer_id: Option<String>,
    pub(crate) desired_revision: u64,
    pub(crate) desired_clearance_revision: u64, // 建筑移动、删除和撤销也必须逐层换版当前预览。
    pub(crate) task: Option<Task<VegetationPreviewTaskResult>>,
    pub(crate) install: Option<VegetationPreviewInstall>, // 大结果在隐藏父根下按帧安装，绝不一次生成全部ECS根。
    /// 已经编辑过的每个Mesh密度层各自保留正式多档预览。
    pub(crate) layers: BTreeMap<String, VegetationMeshLayerPreview>,
}

/// 一个Mesh密度层当前会话的完整可见接管值。
pub(crate) struct VegetationMeshLayerPreview {
    pub(crate) scene_id: SceneId,
    pub(crate) runtime_layer_id: u64,
    pub(crate) applied_revision: u64,
    pub(crate) clearance_revision: u64, // 当前可见层使用的完整建筑净空修订。
    pub(crate) preset_revision: u64, // 与后台散布使用的工程目录同代。
    pub(crate) prototype_revision: u64, // 同 ID 模型/材质重新登记时必须再次安装。
    /// 当前完整可见预览的精确根数量；F4直接读取，不逐棵扫描ECS。
    pub(crate) instance_count: usize,
    pub(crate) batch_count: usize, // 动态场景非空128m实例块数。
    pub(crate) render_node_count: usize, // 批次LOD/部件承接节点总数，证明ECS不再随逐株数量增长。
    pub(crate) container: Entity,  // 一次显隐或回收整层预览，避免逐根切换造成半套画面。
    /// 该层第一次建立密度预览时隐藏的原正式根，切换场景时必须原样恢复。
    pub(crate) hidden_entities: Vec<(Entity, Visibility)>,
}

/// 后台植被抽样只返回变换与正式技术身份，主线程再从注册表取得完整LOD/风材质原型。
pub(crate) struct VegetationPreviewBuild {
    pub(crate) runtime_profile: String,
    pub(crate) runtime_layer_id: u64,
    pub(crate) prototype_id: u64,
    pub(crate) instance_ids: Vec<u64>, // 净空过滤后仍保留过滤前的稳定正式实例身份。
    pub(crate) transforms: Vec<Transform>,
}

/// 一个已经完成CPU散布、正在隐藏父根下分帧生成正式LOD节点的F12层结果。
pub(crate) struct VegetationPreviewInstall {
    pub(crate) scene_id: SceneId,    // 安装期间切换场景会按身份立即丢弃整个隐藏父根。
    pub(crate) layer_id: String, // 完成后只替换同一正式输出层。
    pub(crate) revision: u64,    // 新笔划出现后旧安装结果不得取得可见性。
    pub(crate) clearance_revision: u64, // 新建筑净空出现后旧安装结果不得取得可见性。
    pub(crate) preset_revision: u64, // 目录变化使隐藏候选失效。
    pub(crate) prototype_revision: u64, // 隐藏安装只能采用冻结时的精确原型。
    pub(crate) build: VegetationPreviewBuild, // 完整作者变换只保留一份并由前向游标消费。
    pub(crate) container: Entity, // 全部实例排完后只切换这一父根为可见。
    pub(crate) cursor: usize,    // 每帧从唯一未排入位置继续。
}

/// 混合水网作者预览的single-flight后台构建和正式GPU资产所有权。
pub(crate) struct PendingWaterPreviewBatch {
    pub(crate) source: WaterPreviewSource,
    pub(crate) water_revision: u64,
    pub(crate) terrain_revision: u64,
    pub(crate) preset_revision: WaterPreviewPresetRevision, // 正式目录换代后旧GPU候选不得采用。
    pub(crate) remaining: VecDeque<BuiltWaterSurface>,
    pub(crate) staged: Vec<StagedWaterSurface>,
    pub(crate) warmup_entities: Vec<Entity>,
    pub(crate) triangle_count: usize,
    pub(crate) ticket: Option<WaterGpuUploadTicket>,
}

#[derive(Resource, Default)]
pub(crate) struct WaterPreviewState {
    pub(crate) desired_scene_id: Option<SceneId>,
    pub(crate) desired_water_revision: u64,
    pub(crate) desired_terrain_revision: u64,
    pub(crate) desired_preset_revision: WaterPreviewPresetRevision,
    /// 保存预检沿用当前联合身份的真实等待原因，不能把独立Clipmap的GPU等待误报成GLB构建。
    pub(crate) waiting_detail: Option<String>,
    pub(crate) applied_scene_id: Option<SceneId>,
    pub(crate) applied_water_revision: u64,
    pub(crate) applied_terrain_revision: u64,
    pub(crate) applied_preset_revision: WaterPreviewPresetRevision,
    pub(crate) failed_scene_id: Option<SceneId>,
    pub(crate) failed_water_revision: u64,
    pub(crate) failed_terrain_revision: u64,
    pub(crate) failed_preset_revision: WaterPreviewPresetRevision,
    pub(crate) task: Option<Task<WaterPreviewTaskResult>>,
    pub(crate) pending: Option<PendingWaterPreviewBatch>,
    pub(crate) spawns: Vec<PaintedWaterSurfaceSpawn>,
    /// 与Ready信用分开记录真实GLB替代；空水网仍须隐藏自己来源的旧正式水。
    pub(crate) replacement_source: Option<WaterPreviewSource>,
    pub(crate) replacement_target: Option<WaterPreviewRuntimeTarget>,
    /// 作者预览托管当前来源时隐藏的正式热加载水面，清理预览后必须恢复。
    pub(crate) hidden_runtime_entities: Vec<(Entity, Visibility)>,
}

impl WaterPreviewState {
    /// 切换期望身份并清空可见结果身份；在途CPU任务继续占住唯一后台槽直到自然结束。
    pub(crate) fn retarget(
        &mut self,
        scene_id: Option<SceneId>,
        water_revision: u64,
        terrain_revision: u64,
    ) {
        self.desired_scene_id = scene_id; // 后续轮询只允许采用最新场景和联合修订。
        self.desired_water_revision = water_revision; // 旧水数据完成值由身份门拒收。
        self.desired_terrain_revision = terrain_revision; // 旧河床完成值同样不得进入当前预览。
        self.waiting_detail = None; // 新图或新修订不能继承上一批等待诊断。
        self.applied_scene_id = None; // 切换场景或离图后不再把已回收GPU资产视为就绪。
        self.applied_water_revision = 0; // 清除上一批可见水域修订。
        self.applied_terrain_revision = 0; // 清除上一批可见河床修订。
        self.applied_preset_revision = 0; // 新场景不能沿用上一图正式目录外观证据。
        self.replacement_source = None; // 调用方已恢复正式水和回收旧预览，不能遗留替代权限。
        self.replacement_target = None;
        self.failed_scene_id = None; // 新目标可以在旧任务释放槽后重新尝试。
        self.failed_water_revision = 0; // 失败缓存只属于旧联合身份。
        self.failed_terrain_revision = 0; // 新地形修订不得继承旧失败。
        self.failed_preset_revision = 0; // 新目标清除上一图正式外观失败身份。
    }

    /// 当前全部水层已经采用与未保存GLB顶点一致的联合修订。
    pub(crate) fn ready_for(
        &self,
        scene_id: &SceneId,
        water_revision: u64,
        terrain_revision: u64,
    ) -> bool {
        self.applied_scene_id.as_ref() == Some(scene_id)
            && self.applied_water_revision == water_revision
            && self.applied_terrain_revision == terrain_revision
            && self.applied_preset_revision == self.desired_preset_revision
    }

    /// 诊断只属于当前期望版本；旧水位或兄弟画布的进度不能解释本次保存。
    pub(crate) fn waiting_detail_for(
        &self,
        scene_id: &SceneId,
        water_revision: u64,
        terrain_revision: u64,
    ) -> Option<&str> {
        (self.desired_scene_id.as_ref() == Some(scene_id)
            && self.desired_water_revision == water_revision
            && self.desired_terrain_revision == terrain_revision)
            .then_some(self.waiting_detail.as_deref())
            .flatten()
    }

    /// 当前联合修订已经明确失败；调用方应保留诊断而不是反复重提同一任务。
    pub(crate) fn failed_for(
        &self,
        scene_id: &SceneId,
        water_revision: u64,
        terrain_revision: u64,
    ) -> bool {
        self.failed_scene_id.as_ref() == Some(scene_id)
            && self.failed_water_revision == water_revision
            && self.failed_terrain_revision == terrain_revision
            && self.failed_preset_revision == self.desired_preset_revision
    }
}

#[cfg(test)]
mod tests {
    use bevy::tasks::TaskPoolBuilder; // 独立本地池避免修改进程级AsyncCompute单例。

    use super::*; // 直接锁定作者水预览的single-flight状态语义。

    #[test]
    fn retarget_keeps_running_water_preview_task_in_single_flight_slot() {
        let pool = TaskPoolBuilder::new().num_threads(1).build(); // 本地worker避免修改进程级AsyncCompute单例。
        let old_scene = SceneId::parse("MAP-water-old").expect("旧测试场景ID合法");
        let new_scene = SceneId::parse("MAP-water-new").expect("新测试场景ID合法");
        let task_map = old_scene.clone(); // 完成值仍携带旧场景身份，后续正式轮询会拒收。
        let task = pool.spawn(async move {
            (
                WaterPreviewSource { scene_id: task_map, terrain_id: None },
                3,
                5,
                7, // 旧任务携带当时已采用的正式目录修订。
                Ok::<Vec<BuiltWaterSurface>, String>(Vec::new()),
            )
        }); // 即使worker已完成，Task句柄也必须由非阻塞轮询而不是retarget释放。
        let mut state = WaterPreviewState {
            desired_scene_id: Some(old_scene.clone()),
            applied_scene_id: Some(old_scene.clone()),
            failed_scene_id: Some(old_scene),
            task: Some(task),
            ..default()
        }; // 同时覆盖期望、已采用、失败和在途四类旧身份。

        state.retarget(Some(new_scene.clone()), 17, 23); // 切换场景只重定向，不取消后台重任务。

        assert!(state.task.is_some()); // 旧CPU任务自然结束前继续占住唯一single-flight槽。
        assert_eq!(state.desired_scene_id.as_ref(), Some(&new_scene)); // 最新场景成为唯一采用目标。
        assert_eq!(state.desired_water_revision, 17); // 最新水修订被完整记录。
        assert_eq!(state.desired_terrain_revision, 23); // 最新地形修订被完整记录。
        assert!(state.applied_scene_id.is_none()); // 已清理的可见资产不能继续报告就绪。
        assert!(state.failed_scene_id.is_none()); // 新目标不得继承上一场景失败缓存。
    }
}
