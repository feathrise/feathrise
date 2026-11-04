//! 当前地图作者草稿、撤销历史和有界后台任务资源。

use bevy::prelude::*; // Entity、Handle、Mesh、Resource与Task。
use bevy::tasks::Task; // 加载与保存各自最多一个后台任务。
use feathrise_map_content::{
    MapBoundariesAsset, MapId, MapManifestAsset, MapPlacementsAsset, MapPortalsAsset,
    MapSurfaceAsset, MapVegetationAsset, MapWaterAsset, WorldEnvironmentAsset,
};
use feathrise_map_terrain::{TerrainEditingIndex, TerrainMeshInput}; // GLB顶点与原索引成为动态地形唯一作者源。
use std::collections::{BTreeMap, BTreeSet, VecDeque}; // 预览实体、脏层和笔刷首次值使用确定性容器。
use std::sync::Arc; // 河水整图后台预览共享R8画布，避免每次任务启动复制完整遮罩。 // 草稿复用正式热加载格式。

use crate::types::WaterLevelInputTarget; // 水位左拖事务冻结地图、层与期望作者修订。

pub use super::commands::{ByteChange, EditorCommand, HeightChange}; // 保持现有作者子模块的集中类型入口。
use super::surface_layer_edit::PendingSurfaceBrushSample; // 引入地表作者层事务所需类型和纯助手。
use super::vegetation_compile::{CompiledDensityImage, CompiledDensityStats}; // 最终密度、统计与冲突覆盖同修订原子采用。

mod previews; // 各类有界后台预览、地形文件交接和GPU采用状态。
mod save_evidence; // 保存前整网格水证据登记、冻结候选与有界轮询资源独立维护。
mod identity; // 新对象身份独立于可重置的地图修订和显示序号，重开不复用持久ID。

pub(crate) use previews::{
    PendingWaterPreviewBatch, SurfacePreviewCandidate, SurfacePreviewImage, SurfacePreviewState,
    TerrainLiveApplyState, TerrainPreviewBuild, TerrainPreviewIdentity, TerrainPreviewState, VegetationMeshLayerPreview,
    VegetationPreviewBuild, VegetationPreviewInstall, VegetationPreviewState, WaterPreviewState,
    WaterPreviewRuntimeTarget, WaterPreviewSource,
}; // 保持`authoring::resources::*`既有路径和可见性不变。
pub(crate) use save_evidence::{
    PlacementWaterCachedReadiness, PlacementWaterCandidateObservation,
    PlacementWaterEvidenceCandidate, PlacementWaterEvidenceRegistry, PlacementWaterRecordEvidence,
    PlacementWaterRetiredEvidenceGeneration, PlacementWaterSavePreflightCache,
    PlacementWaterStatusEvent,
}; // 保存功能域继续通过集中资源入口取得类型，主文件保持低于500行。

/// 保存事务返回地图身份、作者修订和结构化结果；失败可要求主线程重载已回滚正式路径。
type DraftSaveTaskResult = (
    u64,
    u64,
    Result<super::save::DraftSaveSuccess, super::save::DraftSaveError>,
);

/// 全局世界环境单文件保存返回票据、联合作者修订、全局修订、完整快照与事务结果。
type GlobalEnvironmentSaveTaskResult = (
    u64,
    u64,
    u64,
    WorldEnvironmentAsset,
    Result<(), super::save::DraftSaveError>,
);

/// 大R8共享画布在后台物化独立写侧，完成值绑定地图、层、修订和原Arc身份。
type WaterMaskMaterializeResult = (MapId, String, u64, Arc<Vec<u8>>, Vec<u8>);

/// 一张已完整载入、可编辑和可保存的地图草稿。
#[derive(Clone)]
pub struct EditorMapDraft {
    pub map_id: MapId,
    /// 相对assets根的地图私有目录。
    pub folder: String,
    /// 目录实际声明的水域JSON路径；允许旧地图沿用私有目录内的自定义文件名。
    pub water_asset_path: String,
    pub manifest: MapManifestAsset,
    /// 多地形的不可变空间索引由后台构建；保存只写作者清单，不序列化派生查询状态。
    pub terrain_ground: Option<Arc<super::terrain_ground::TerrainGroundSnapshot>>,
    pub(crate) terrain_documents: super::terrain_documents::TerrainDocumentStore,
    pub placements: MapPlacementsAsset,
    /// 当前摆放文档的整网格水绑定总数；加载任务计算，赋水/撤销按单记录差值维护。
    pub placement_water_surface_count: usize,
    pub portals: MapPortalsAsset,
    pub domain: Option<super::domain_documents::MapDomainDocument>, // 普通地图不定义外部作者正文。
    pub preserved_asset_paths: Arc<[String]>, // 未安装适配时仍保护原目录登记的文件。
    /// GLB世界空间顶点与原三角索引；F12只修改位置Y并保持拓扑不变。
    pub terrain_mesh: Option<Arc<TerrainMeshInput>>,
    /// 与当前GLB拓扑修订绑定的X/Z空间桶和真实邻接表。
    pub terrain_editing_index: Option<Arc<TerrainEditingIndex>>,
    /// 打开草稿时terrain.glb的SHA-256；保存开始与正式替换前都重新核对。
    pub terrain_source_sha256: Option<[u8; 32]>,
    /// 用户明确选择“使用草稿覆盖”时冻结的当前外部GLB摘要；只授权覆盖这一个版本。
    pub terrain_overwrite_authorized_sha256: Option<[u8; 32]>,
    /// 从GLB派生的16位规则高度；仅供水域、植被和边界查询，不再保存到磁盘。
    pub heights: Option<Arc<Vec<u16>>>,
    /// 动态地图可编辑地表描述；旧地图未声明时为None。
    pub surface: Option<MapSurfaceAsset>,
    pub puddle_mask: Option<Arc<Vec<u8>>>, // 独立潜力R8；背景任务和整笔历史浅共享完整版本。
    pub puddles_dirty: bool, // raw源与GPU KTX2配对保存，撤销到空层也需更新清单。
    /// 四个局部材质槽的 RGBA8 归一化权重作者源。
    pub surface_weights: Option<Arc<Vec<u8>>>, // 删除历史、预览和保存浅共享；整图写侧只在后台物化。
    /// 与权重同分辨率的 RGBA32_UINT 局部地图层索引源，每像素四个小端 `u32`。
    pub surface_indices: Option<Arc<Vec<u8>>>, // 与权重共同冻结和采用，输入帧不得为删层深拷贝整图。
    /// 动态地图可编辑植被层描述；旧地图未声明时为None。
    pub vegetation: Option<MapVegetationAsset>,
    /// 每个正式运行时层ID到编译完成的R8密度；实际LOD与保存只读取此表。
    pub vegetation_densities: BTreeMap<String, Arc<Vec<u8>>>,
    /// 当前完整编译修订中至少含一个非零像素的正式层ID；预览系统据此避免主线程整图扫描。
    pub vegetation_nonzero_layers: BTreeSet<String>,
    /// 每个正式运行时层的常数大小密度统计；只随完整后台编译结果原子替换。
    pub(crate) vegetation_density_stats: BTreeMap<String, CompiledDensityStats>,
    /// 每个作者层ID到独立R8画布；笔刷只修改此表，正式运行时从不直接读取。
    pub vegetation_author_densities: BTreeMap<String, Arc<Vec<u8>>>,
    /// 每个作者组的重叠诊断R8；只供F12冲突预览，不写入运行时资产。
    pub(crate) vegetation_conflicts: BTreeMap<String, CompiledDensityImage>,
    /// 动态地图可编辑水域描述；旧动态地图打开时会在内存中补零对象水域文件。
    pub water: Option<MapWaterAsset>,
    /// 每层稳定ID到R8水域覆盖；Arc让single-flight预览任务只复制句柄。
    pub water_masks: BTreeMap<String, Arc<Vec<u8>>>,
    /// 独立空气墙对象文档；未创建墙的动态地图使用空列表。
    pub boundaries: Option<MapBoundariesAsset>,
    /// 顶点高度变化时保存任务重写唯一 terrain.glb。
    pub terrain_dirty: bool,
    /// 地表权重或层参数发生变化。
    pub surface_dirty: bool,
    /// 需要重新派生R8 KTX2的植被层ID。
    pub vegetation_dirty_layers: BTreeSet<String>,
    /// 需要写入无Mip R8作者源的作者层ID。
    pub vegetation_author_dirty_layers: BTreeSet<String>,
    /// 需要重新派生R8 KTX2的水域层ID。
    pub water_dirty_layers: BTreeSet<String>,
    /// 地图环境草稿已经修改；保存前由同一内存值实时接管离屏与玩法相机。
    pub environment_dirty: bool,
    /// 旧动态地图首次补水域时，保存事务最后为全局目录登记water_path。
    pub water_catalog_registration_required: bool,
    /// 旧Clipmap首次登记水域时的原文件摘要；`missing`表示打开时不存在，防止覆盖稍后出现的作者文件。
    pub water_registration_source_hash: Option<String>,
    /// 旧Clipmap首次植被登记责任及打开时私有JSON摘要；missing表示当时不存在，防止覆盖后来写入的内容。
    pub vegetation_registration_source_hash: Option<String>,
    /// 首次创建空气墙文档时，保存事务最后为全局目录登记boundary_path。
    pub boundary_catalog_registration_required: bool,
    /// 区域控制点、闭合状态或墙体规格发生变化。
    pub boundaries_dirty: bool,
}

/// 当前地图编辑会话及未保存预览实体。
#[derive(Resource, Default)]
pub struct EditorAuthoringSession {
    pub vegetation_cross_busy: bool, // 冻结笔划及完整发布期间，姿态等作者事务不得插入历史顺序。
    /// 玩法选择记忆只绑定地图和稳定路径ID；不保存、不进入撤销、不随控制点松手清空。
    pub selected_boundary_path: Option<(MapId, String)>,
    pub draft: Option<EditorMapDraft>,
    /// 每次作者修改或完整来源采用时递增；后台保存只采用身份仍匹配的结果。
    pub revision: u64,
    /// 仅当前修订的保存完整成功才递增，供“保存并切图”识别正式回执，禁止解析状态文字或把清脏当成功。
    pub successful_save_sequence: u64,
    /// 打开旧图时的兼容补齐修订，可随无编辑的自动来源采用延续；作者编辑或开始保存后失效。
    pub loaded_migration_revision: Option<u64>,
    /// 只在摆放增删、来源或整网格水绑定身份/类别变化及草稿换代时递增。
    /// GLB浏览器依赖此确定性身份跳过流速、高度、变换等非树结构修改，禁止用内存地址猜测换代。
    pub placement_structure_revision: u64,
    /// 仅贴地等派生任务实际采用姿态时递增；使旧世界锚点和异步事务失效，不记作者历史或脏位。
    pub placement_derived_pose_revision: u64,
    /// 名称、分组和保护状态独立换代，不重建建筑BVH或真实模型。
    pub scene_objects_revision: u64,
    /// 只在传送点增删、目标修改或草稿换代时递增，双列表稳定帧不扫描完整传送图。
    pub portal_revision: u64,
    pub undo: Vec<EditorCommand>,
    pub redo: Vec<EditorCommand>,
    pub placement_previews: BTreeMap<String, Entity>,
    pub(crate) placement_deletions: super::placement_deletion::PlacementDeletionIntents,
    pub portal_previews: BTreeMap<String, Entity>,
    pub next_serial: u64,
    pub(crate) author_ids: identity::AuthorIdSource,
    pub saving: bool,
    /// 水体预设删除独占引用写侧，持续至文件事务及目录采用收口；不冒充地图保存或冻结视图。
    pub(crate) water_preset_delete_pending: bool,
    /// 只在高度变化时递增，避免摆放模型触发无关全地形重建。
    pub terrain_revision: u64,
    /// 新图稀疏高度块尚未写回清单；它不能借用旧GLB的terrain_dirty保存分支。
    pub sparse_terrain_dirty: bool,
    /// 只在地表变化时递增，驱动后台综合色预览。
    pub surface_revision: u64,
    /// 只在植被层或密度变化时递增，驱动实际模型抽样预览。
    pub vegetation_revision: u64,
    /// 图层结构、画布交换和来源重载独立换代；连续密度刷印允许采用同源的完整中间预览。
    pub vegetation_structure_revision: u64,
    /// 最近一次完整编译并原子采用到正式层的植被作者修订。
    pub vegetation_compiled_revision: u64,
    /// 当前正式密度属于哪一代图层结构；换画布后不能用旧输出构建新网格预览。
    pub vegetation_compiled_structure_revision: u64,
    /// 只在水域遮罩或水位变化时递增，驱动正式GPU水面后台预览。
    pub water_revision: u64,
    /// 只在地图环境变化或换图时递增，离屏与玩法相机据此拒绝迟到天空资产。
    pub environment_revision: u64,
    /// 只在边界操作完成时递增，拖动期间不逐帧重建Avian静态碰撞。
    pub boundary_revision: u64,
}

impl EditorAuthoringSession {
    /// 保存建立新历史基线时，同帧复位画布归属游标；后续新笔不能继承旧栈下标。
    pub fn clear_history(
        &mut self,
        retirement: Option<&mut super::AuthoringRetirement>,
    ) {
        if let Some(draft) = self.draft.as_mut() {
            draft.terrain_documents.scoped_through = 0;
        }
        if let Some(retirement) = retirement {
            retirement.push(std::mem::take(&mut self.undo));
            retirement.push(std::mem::take(&mut self.redo));
        } else {
            self.undo.clear();
            self.redo.clear();
        } // 独立作者测试未安装退役资源时保留原清理方式。
    }

    /// 水域历史可裁剪完整旧前缀；按删除数量移动归属游标，不重新扫描既有记录。
    pub fn push_bounded_water_history(&mut self, command: EditorCommand) {
        let previous_len = self.undo.len();
        super::commands::push_bounded_water_history(&mut self.undo, &mut self.redo, command);
        self.adjust_history_scope_after_append(previous_len);
    }

    /// 重做保留后续重做链，同时仍按同一水域预算调整线性历史前缀。
    pub fn push_redone_bounded_water_history(&mut self, command: EditorCommand) {
        let previous_len = self.undo.len();
        super::commands::push_redone_bounded_water_history(&mut self.undo, command);
        self.adjust_history_scope_after_append(previous_len);
    }

    /// 弹出撤销记录立即缩短已核对前缀，保护同帧从撤销状态起笔的新历史分支。
    pub fn pop_undo(&mut self) -> Option<EditorCommand> {
        let command = self.undo.pop();
        if let Some(draft) = self.draft.as_mut() {
            draft.terrain_documents.scoped_through =
                draft.terrain_documents.scoped_through.min(self.undo.len());
        }
        command
    }

    fn adjust_history_scope_after_append(&mut self, previous_len: usize) {
        let removed_prefix = previous_len
            .saturating_add(1)
            .saturating_sub(self.undo.len());
        if let Some(draft) = self.draft.as_mut() {
            draft.terrain_documents.scoped_through = draft
                .terrain_documents
                .scoped_through
                .min(previous_len)
                .saturating_sub(removed_prefix);
        } // 新命令仍等待既有64条/帧归属门；裁剪不能把它误认为已核对。
    }
    /// 自动补齐仍有显式保存责任，但星号和离开保护只提示作者编辑；按完整地图与修订身份判断。
    pub fn is_unedited_load_migration_for(&self, map_id: &MapId) -> bool {
        self.loaded_migration_revision == Some(self.revision)
            && !self.saving
            && !self.sparse_terrain_dirty
            && self.draft.as_ref().is_some_and(|draft| {
                &draft.map_id == map_id
                    && !draft.domain_dirty() && !draft.stored_terrain_changes()
                    && !draft.terrain_dirty
                    && !draft.surface_dirty
                    && !draft.puddles_dirty
                    && !draft.environment_dirty
                    && !draft.boundaries_dirty
            }) // 身份、修订及真实编辑脏位共同证明，不能仅因没有撤销记录就放行。
    }

    /// 加载补齐仍可显式保存，但未编辑的兼容草稿可由下次打开重建，不应阻止浏览地图。
    /// 这里只查询状态：关闭等待和切图轮询都会调用，不能在谓词内逐帧写日志。
    pub fn needs_save_before_map_switch(
        &self,
        editor: &crate::resources::MapEditorState,
        global: &GlobalEnvironmentAuthoringState,
    ) -> bool {
        if !editor.dirty {
            return false;
        }
        let only_loaded_migration = editor
            .selected_map_id
            .as_ref()
            .is_some_and(|map_id| self.is_unedited_load_migration_for(map_id));
        global.dirty || !only_loaded_migration
    }

    /// 返回当前会话是否含有不能由“仅保存全局环境”处理的地图作者修改。
    pub fn has_map_changes(&self) -> bool {
        self.has_map_data_changes()
            || self
                .undo
                .iter()
                .chain(&self.redo)
                .any(|command| !command.is_global_environment())
    }

    /// 文档暂停只读取固定脏位；完整历史是否含地图命令在后台交接中判定。
    pub fn has_map_data_changes(&self) -> bool {
        let draft_dirty = self.draft.as_ref().is_some_and(|draft| {
            draft.domain_dirty() || draft.stored_terrain_changes()
                || draft.terrain_dirty
                || draft.surface_dirty
                || draft.puddles_dirty // 积水编辑同样进入统一关闭与切图保存门。
                || !draft.vegetation_dirty_layers.is_empty()
                || !draft.vegetation_author_dirty_layers.is_empty()
                || !draft.water_dirty_layers.is_empty()
                || draft.environment_dirty
                || draft.vegetation_registration_source_hash.is_some() // 首次植被登记同样必须保留未保存责任。
                || draft.water_catalog_registration_required
                || draft.boundary_catalog_registration_required
                || draft.boundaries_dirty
        }); // 首载迁移和运行时交接失败即使没有撤销命令，也由专用脏位保留地图责任。
        self.sparse_terrain_dirty || draft_dirty
    }

    /// 稀疏高度已经独立落盘后，判断是否还需要继续旧的联合地图保存事务。
    pub fn has_non_sparse_map_changes(&self) -> bool {
        let draft_dirty = self.draft.as_ref().is_some_and(|draft| {
            draft.domain_dirty() || draft.stored_terrain_changes()
                || draft.terrain_dirty
                || draft.surface_dirty
                || draft.puddles_dirty
                || !draft.vegetation_dirty_layers.is_empty()
                || !draft.vegetation_author_dirty_layers.is_empty()
                || !draft.water_dirty_layers.is_empty()
                || draft.environment_dirty
                || draft.vegetation_registration_source_hash.is_some() // 首次植被登记同样必须保留未保存责任。
                || draft.water_catalog_registration_required
                || draft.boundary_catalog_registration_required
                || draft.boundaries_dirty
        });
        draft_dirty
            || self
                .undo
                .iter()
                .chain(&self.redo)
                .any(|command| !command.is_global_environment() && !command.is_sparse_terrain())
    }
}

/// F12 对唯一全局世界环境文件的作者草稿与保存基线。
#[derive(Resource, Clone, Debug)]
pub struct GlobalEnvironmentAuthoringState {
    /// 当前实时预览和下一次保存使用的完整有效配置。
    pub draft: WorldEnvironmentAsset,
    /// 最近一次成功加载或保存的磁盘基线，用于判断拖回起点和测试。
    pub saved: WorldEnvironmentAsset,
    /// 用户是否已经修改全局草稿且尚未成功落盘。
    pub dirty: bool,
    /// 每次完整全局操作递增；单文件和联合任务都按该身份拒收过期结果。
    pub revision: u64,
    /// 最近一次观察到的运行时全局资产修订。
    pub source_revision: u64,
    /// 文件缺失、加载或校验失败的可见诊断；编辑仍从安全默认值继续。
    pub load_error: Option<String>,
    /// 首次同步完成前草稿仍使用编译期安全默认值。
    pub initialized: bool,
}

impl Default for GlobalEnvironmentAuthoringState {
    fn default() -> Self {
        let asset = WorldEnvironmentAsset::default(); // 编辑器即使独立启动也有完整可保存配置。
        Self {
            draft: asset.clone(), // 初始预览不等待磁盘 IO。
            saved: asset,         // 首次资产事件到达后再替换真实基线。
            dirty: false,         // 默认值尚未被作者修改。
            revision: 0,          // 首次操作才推进作者身份。
            source_revision: 0,   // 尚未观察运行时资产注册表。
            load_error: None,     // 文件未决不等于失败。
            initialized: false,   // 同步系统会采用全局注册表当前有效值与错误。
        }
    }
}

impl GlobalEnvironmentAuthoringState {
    /// 提交一次完整全局快照变化并进入统一关闭/保存门。
    pub fn mark_changed(
        &mut self,
        session: &mut EditorAuthoringSession,
        editor: &mut crate::resources::MapEditorState,
    ) {
        self.dirty = true; // 全局文件与地图文件保存责任保持独立。
        self.revision = self.revision.wrapping_add(1); // 迟到单文件或联合任务失去采用资格。
        session.revision = session.revision.wrapping_add(1); // 联合事务使用统一作者身份冻结两份草稿。
        editor.dirty = true; // 顶部保存和关闭确认继续使用唯一可见脏状态。
    }
}

/// 一次水位左拖的作者事务；途中可实时刷新，松开只压入一条前后值命令。
#[derive(Resource, Default)]
pub struct WaterLevelScrubState {
    pub target: Option<WaterLevelInputTarget>, // 按第一次真实变化冻结的地图、稳定层ID和起始修订。
    pub before: f32,                           // 拖动前作者水位，用于唯一撤销命令。
    pub after: f32,                            // 最近一次已应用到草稿的水位。
    pub expected_revision: u64,                // 只接受由本事务连续推进后的全局作者修订。
    pub dirty_before: bool,                    // 拖回原值时恢复进入事务前的保存门状态。
    pub aborted: bool,                         // 冲突后保留取消态到松手，防止同次拖动重绑。
}

impl WaterLevelScrubState {
    pub fn clear(&mut self) {
        *self = Self::default(); // 释放稳定身份，下一次拖动必须从当前草稿重新绑定。
    }
}

impl EditorAuthoringSession {
    pub fn mark_changed(&mut self, editor: &mut crate::resources::MapEditorState) {
        self.revision = self.revision.wrapping_add(1);
        editor.dirty = true;
    }

    pub fn mark_scene_objects_changed(&mut self, editor: &mut crate::resources::MapEditorState) {
        self.scene_objects_revision = self.scene_objects_revision.wrapping_add(1);
        self.mark_changed(editor);
    }

    /// 完成一次会改变GLB实例树或水绑定诊断行的作者动作，并进入统一保存门。
    pub fn mark_placement_structure_changed(
        &mut self,
        editor: &mut crate::resources::MapEditorState,
    ) {
        self.placement_structure_revision = self.placement_structure_revision.wrapping_add(1); // 仅结构动作换代，连续数值拖动不触发最多8192实例扫描。
        self.mark_changed(editor); // placements JSON仍复用全局修订与dirty语义。
    }

    /// 完成一次传送图结构或目标修改，并只换代传送双列表相关缓存。
    pub fn mark_portals_changed(
        &mut self,                                     // 当前作者会话持有传送专用修订。
        editor: &mut crate::resources::MapEditorState, // 全局dirty和保存门仍由编辑器状态统一发布。
    ) {
        self.portal_revision = self.portal_revision.wrapping_add(1); // 当前/目标列表只在真实传送变化时重读可见行。
        self.mark_changed(editor); // 传送JSON仍属于统一地图保存事务。
    }

    pub fn mark_terrain_changed(&mut self, editor: &mut crate::resources::MapEditorState) {
        self.terrain_revision = self.terrain_revision.wrapping_add(1);
        self.mark_changed(editor);
        if let Some(draft) = self.draft.as_mut() {
            draft.terrain_dirty = true;
        }
    }

    /// 新图高度沿用地形派生修订，但只标记局部块保存责任，绝不进入旧GLB整图写入。
    pub fn mark_sparse_terrain_changed(
        &mut self,
        editor: &mut crate::resources::MapEditorState,
    ) {
        self.terrain_revision = self.terrain_revision.wrapping_add(1); // Clipmap页与近场碰撞使用同一作者代次拒收迟到结果。
        self.sparse_terrain_dirty = true; // F12保存门先提交局部块和清单。
        self.mark_changed(editor); // 顶栏、关闭确认和跨图门继续共用唯一dirty信号。
    }

    /// 建筑与地形联合事务只推进一次全局作者修订，同时精确换代两个受影响领域。
    pub fn mark_building_sparse_terrain_changed(
        &mut self,
        editor: &mut crate::resources::MapEditorState,
        structure_changed: bool,
        terrain_changed: bool,
    ) {
        if structure_changed {
            self.placement_structure_revision = self.placement_structure_revision.wrapping_add(1);
        }
        if terrain_changed {
            self.terrain_revision = self.terrain_revision.wrapping_add(1);
            self.sparse_terrain_dirty = true; // 仅局部块/清单保存，绝不触发GLB或Blender派生。
        }
        self.mark_changed(editor);
    }

    /// 建筑与GLB地形联合事务只推进一次全局作者修订。
    pub fn mark_building_terrain_changed(
        &mut self,
        editor: &mut crate::resources::MapEditorState,
        placement_structure_changed: bool,
        terrain_changed: bool,
    ) {
        if placement_structure_changed {
            self.placement_structure_revision = self.placement_structure_revision.wrapping_add(1); // 新建或复制使数组和场景树换代。
        }
        if terrain_changed {
            self.terrain_revision = self.terrain_revision.wrapping_add(1); // Mesh、高度、碰撞和水岸共同换代。
        }
        self.mark_changed(editor); // 两个领域属于同一次Ctrl+Z事务，全局revision严格只增加一次。
        if terrain_changed && let Some(draft) = self.draft.as_mut() {
            draft.terrain_dirty = true; // 保存必须把联合结果中的GLB顶点一并发布。
        }
    }

    pub fn mark_surface_changed(&mut self, editor: &mut crate::resources::MapEditorState) {
        self.surface_revision = self.surface_revision.wrapping_add(1);
        self.mark_changed(editor);
        if let Some(draft) = self.draft.as_mut() {
            draft.surface_dirty = true;
        }
    }

    pub fn mark_vegetation_changed(
        &mut self,
        editor: &mut crate::resources::MapEditorState,
        layer_id: &str,
    ) {
        self.vegetation_revision = self.vegetation_revision.wrapping_add(1);
        self.mark_changed(editor);
        if let Some(draft) = self.draft.as_mut() {
            draft
                .vegetation_author_dirty_layers
                .insert(layer_id.to_owned()); // 传入身份始终是作者层，不再冒充正式运行时层。
        }
    }

    /// 图层增删、组关系或编译参数变化使全部当前作者源和正式输出进入同一新修订。
    pub fn mark_vegetation_authoring_changed(
        &mut self,
        editor: &mut crate::resources::MapEditorState,
    ) {
        self.vegetation_structure_revision = self.vegetation_structure_revision.wrapping_add(1);
        self.vegetation_revision = self.vegetation_revision.wrapping_add(1); // 结构换代仍拒绝旧编译；纯刷印可连续显示完整结果。
        self.mark_changed(editor); // 图层结构属于地图作者JSON，必须进入统一保存门。
        if let Some(draft) = self.draft.as_mut()
            && let Some(authoring) = draft
                .vegetation
                .as_ref()
                .and_then(|vegetation| vegetation.authoring.as_ref())
        {
            draft
                .vegetation_author_dirty_layers
                .extend(authoring.layers.iter().map(|layer| layer.id.clone())); // Arc浅快照恢复后，当前存在的全部作者源都可安全原子写回。
        }
    }

    /// 河水复合笔划用一次全局作者换代共同发布R8覆盖与可选GLB顶点下挖。
    pub fn mark_water_stroke_changed(
        &mut self,
        editor: &mut crate::resources::MapEditorState,
        layer_id: &str,
        terrain_changed: bool,
    ) {
        self.water_revision = self.water_revision.wrapping_add(1); // 每个真实刷印都使旧水面候选失效。
        if terrain_changed {
            self.terrain_revision = self.terrain_revision.wrapping_add(1); // 只有GLB顶点真实变化才重建地形、碰撞和联合岸线。
        }
        self.mark_changed(editor); // 水与地形属于同一次作者操作，全局revision严格只增加一次。
        if let Some(draft) = self.draft.as_mut() {
            draft.water_dirty_layers.insert(layer_id.to_owned()); // R8作者源与KTX2等待同次保存。
            if terrain_changed {
                draft.terrain_dirty = true; // GLB顶点和水域R8等待同次保存。
            }
        }
    }

    /// 水位等JSON参数变化不要求重写R8/KTX2，但必须重建预览并保存水域文件。
    pub fn mark_water_definition_changed(
        &mut self,
        editor: &mut crate::resources::MapEditorState,
    ) {
        self.water_revision = self.water_revision.wrapping_add(1);
        self.mark_changed(editor);
        if let Some(draft) = self.draft.as_mut() {
            draft.terrain_documents.active_dirty = true; // 只有JSON变化也必须进入活动/停放画布的保存门。
        }
    }

    /// 水层增删或复制会同时改变JSON与遮罩表，当前全部层均需重新派生正式KTX2。
    pub fn mark_water_layers_changed(
        &mut self,
        editor: &mut crate::resources::MapEditorState,
    ) {
        self.water_revision = self.water_revision.wrapping_add(1); // 迟到旧层预览按修订拒收。
        self.mark_changed(editor); // 水域JSON与作者源进入同一次保存事务。
        if let Some(draft) = self.draft.as_mut() {
            draft.terrain_documents.active_dirty = true; // 删除最后一层时没有剩余R8，仍要保存空层表。
        }
        if let Some(draft) = self.draft.as_mut()
            && let Some(water) = draft.water.as_ref()
        {
            draft
                .water_dirty_layers
                .extend(water.layers.iter().map(|layer| layer.id.clone()));
        }
    }

    /// 环境JSON变化不重建地形或密度，只换代统一天空盒请求并标记整份清单待保存。
    pub fn mark_environment_changed(
        &mut self,
        editor: &mut crate::resources::MapEditorState,
    ) {
        self.environment_revision = self.environment_revision.wrapping_add(1); // 旧Image事件不再拥有当前作者身份。
        self.mark_changed(editor); // 环境仍属于地图作者文档，复用统一保存与关闭提示。
        if let Some(draft) = self.draft.as_mut() {
            draft.environment_dirty = true; // 保存前保持玩法相机内存覆盖，关闭F12查看实景时也不会退回旧天空。
        }
    }

    /// 区域边界变化只换代小型静态碰撞集合，不触发地形、纹理或植被任务。
    pub fn mark_boundaries_changed(
        &mut self,
        editor: &mut crate::resources::MapEditorState,
    ) {
        self.boundary_revision = self.boundary_revision.wrapping_add(1); // 每个完成操作只产生一个碰撞修订。
        self.mark_changed(editor); // 边界JSON属于地图作者文档，进入统一保存与关闭提示。
        if let Some(draft) = self.draft.as_mut() {
            draft.boundaries_dirty = true; // 保存成功前由F12内存覆盖继续拥有运行时权威。
        }
    }
}

/// 地图草稿加载任务槽。
#[derive(Resource, Default)]
pub struct DraftLoadTask {
    pub ticket: u64,
    pub desired_map_id: Option<MapId>,
    pub task: Option<Task<(u64, Result<EditorMapDraft, String>)>>,
}

/// 地图草稿保存任务槽。
#[derive(Debug)]
pub struct WaterSaveHandoffSnapshot {
    pub map_id: MapId,               // IO启动时当前玩法地图的稳定身份。
    pub water_revision: u64,         // 成功发布门只接受这次冻结的作者水修订。
    pub root_water_revision: u64, // 根水停放期间不随私有笔划换代，正式根门冻结其自身修订。
    pub terrain_revision: u64,       // 水面构建同时依赖的GLB地形作者修订。
    pub baseline_generation: u64,    // 后续正式水域必须严格越过的运行时代次。
    pub manifest_asset_path: String, // 水网世界范围与高度参数也必须完成同批交接。
    pub water_asset_path: String,    // 目录实际声明的自定义water JSON路径。
    /// 本批真正改写的(layer id, raw R8路径, KTX2路径)；正式采用和回滚必须同时验证两份遮罩。
    pub dirty_layers: Vec<(String, String, String)>,
    pub dirty_height_path: Option<String>, // 仅供静态/专用兼容地图记录真正改写的raw R16。
    pub catalog_registration_required: bool, // 首次水体登记失败时正式状态仍应保持无水。
    /// 显式地图冻结本批脏画布；完成路径再按后台真实水改动回执筛选，不能借当前选择代替整个保存批次。
    pub terrains: Vec<TerrainWaterSaveHandoffSnapshot>,
}

/// 独立画布的水域交接身份；一个旧活动实例和一个在途候选分别保留自己的代次下限。
#[derive(Clone, Debug)]
pub struct TerrainWaterSaveHandoffSnapshot {
    pub terrain_id: feathrise_map_content::TerrainId,
    pub water_asset_path: String,
    pub baselines: Vec<(Entity, u64)>,
    pub dirty_layers: Vec<(String, String, String)>,
    pub catalog_registration_required: bool,
    pub retry_required: bool,
    /// 本批清单仍启用该对象时才等待运行时采用；禁用或删除目标只承担完整正式文件责任。
    pub runtime_required: bool,
}

/// 一个保存批次对精确作者预览的回收证据；Entity地址不能代表同地址上的后来作者版本。
#[derive(Debug)]
pub struct PlacementPreviewHandoffSnapshot {
    pub id: String,
    pub root: Entity,
    instance_version: Option<feathrise_scene::AuthoringVersion>,
    transform: Option<Transform>,
    parent: Option<Entity>,
    water_version: Option<feathrise_scene::AuthoringVersion>,
    pub cleanup_owned: bool, // 只有核验成功并撤销作者资格的根才可以进入后续DFS。
}

impl PlacementPreviewHandoffSnapshot {
    pub fn from_components(map_id: &MapId, id: &str, root: Entity,
        instance: Option<&feathrise_scene::MapPlacementInstance>, transform: Option<&Transform>,
        parent: Option<&ChildOf>, water: Option<&feathrise_scene::DynamicWaterMeshOverrides>) -> Option<Self> {
        if instance.is_some_and(|instance| instance.map_id() != map_id || instance.id() != id) {
            return None; // 陈旧session地址不能认领另一地图或同批相邻记录，即使它们共享相同gate。
        }
        Some(Self { id: id.to_owned(), root, instance_version: instance.map(feathrise_scene::MapPlacementInstance::authoring_version),
            transform: transform.copied(), parent: parent.map(ChildOf::parent),
            water_version: water.map(feathrise_scene::DynamicWaterMeshOverrides::authoring_version), cleanup_owned: false })
    }

    /// 真实世界捕获与保存Query共用同一轻量证据；不复制水数组、作者记录或后代树。
    #[cfg(test)]
    pub fn capture(world: &World, map_id: &MapId, id: &str, root: Entity) -> Option<Self> {
        world.get::<crate::components::EditorPlacedAsset>(root)?;
        Self::from_components(map_id, id, root, world.get(root), world.get(root), world.get(root), world.get(root))
    }

    pub fn matches_world(&self, world: &World, map_id: &MapId) -> bool {
        world.get::<crate::components::EditorPlacedAsset>(self.root).is_some()
            && world.get::<feathrise_scene::MapPlacementInstance>(self.root)
                .is_none_or(|instance| instance.map_id() == map_id && instance.id() == self.id)
            && self.instance_version == world.get::<feathrise_scene::MapPlacementInstance>(self.root)
                .map(feathrise_scene::MapPlacementInstance::authoring_version)
            && self.transform.as_ref() == world.get::<Transform>(self.root)
            && self.parent == world.get::<ChildOf>(self.root).map(ChildOf::parent)
            && self.water_version == world.get::<feathrise_scene::DynamicWaterMeshOverrides>(self.root)
                .map(feathrise_scene::DynamicWaterMeshOverrides::authoring_version)
    } // 局部Transform覆盖尚未提交的手柄预览，父身份覆盖整组临时重挂，水票据覆盖失败后的原位修复。
}

/// IO启动时冻结的正式摆放交接基线；只有实际发布当前玩法地图时才消费。
#[derive(Debug)]
pub struct PlacementSaveHandoffSnapshot {
    pub map_id: MapId, // 保存期间换图时旧摆放结果不能阻塞新的作者地图。
    pub baseline_revision: u64, // 新Ready必须严格越过的已采用修订。
    pub baseline_attempt_revision: u64, // 新Failed必须严格越过的正式尝试修订。
    pub retry_failed_runtime: bool, // 上一交接失败时允许不修改资产内容而显式重建。
    /// 保存启动时仍属于本批草稿的精确预览实体；正式严格新Ready前持续保留。
    pub editor_previews: Vec<PlacementPreviewHandoffSnapshot>,
    /// 只冻结删除意图的弱票据；旧保存确认不能清除后来同ID的新删除。
    pub deleted: Vec<super::placement_deletion::PlacementDeletionSnapshot>,
}

#[derive(Resource, Default)]
pub struct DraftSaveTask {
    pub ticket: u64,
    pub task: Option<Task<DraftSaveTaskResult>>,
    /// 保存IO启动前冻结的正式水域完整身份；成功或失败回滚都只能使用这份不可变快照建门。
    pub water_handoff_snapshot: Option<WaterSaveHandoffSnapshot>,
    /// 保存IO启动前冻结的正式摆放修订；内容无变化时完成路径会直接丢弃它。
    pub placement_handoff_snapshot: Option<PlacementSaveHandoffSnapshot>,
    /// 保存前Scene0水体证明的分帧作者索引；只在修订变化时重新扫描完整摆放数组。
    pub placement_water_preflight: PlacementWaterSavePreflightCache,
    /// 联合事务启动时冻结的全局配置与修订；成功后才清除全局脏状态。
    pub global_environment_snapshot: Option<(u64, WorldEnvironmentAsset)>,
}

/// 只有全局配置脏时使用的单文件原子保存槽；不进入任何地图预检。
#[derive(Resource, Default)]
pub struct GlobalEnvironmentSaveTask {
    pub ticket: u64, // 每次启动递增，迟到任务不能清理当前脏状态。
    pub task: Option<Task<GlobalEnvironmentSaveTaskResult>>, // 同时最多一个小型 IO 事务。
}

/// 水笔划至多一个后台整图复制；Update只轮询并原子替换完整Vec。
#[derive(Resource, Default)]
pub struct WaterMaskMaterializeTask {
    pub task: Option<Task<WaterMaskMaterializeResult>>,
    pub terrain_task: Option<Task<(MapId, u64, Arc<TerrainMeshInput>, TerrainMeshInput)>>, // 历史长期共享GLB时只允许一个后台写侧副本。
}

/// 传送点预览共享几何与材质，避免每次点击创建资产。
#[derive(Resource)]
pub struct EditorPreviewAssets {
    pub portal_base_mesh: Handle<Mesh>, // 未保存预览保留正式传送范围底盘，衬托中央方向箭头。
    pub portal_arrow_head_mesh: Handle<Mesh>, // 三角箭头头部明确表达出口方向。
    pub portal_arrow_shaft_mesh: Handle<Mesh>, // 矩形杆身与头部共同构成完整方向标记。
    pub portal_edge_beam_mesh: Handle<Mesh>, // 未保存传送点共享十四米高且顶部渐隐的固定半径圆柱壁。
    pub portal_edge_ring_mesh: Handle<Mesh>, // 地面边缘环明确表达传送范围。
    pub portal_base_material: Handle<StandardMaterial>, // 深色半透明底盘让箭头在复杂地表上保持可读。
    pub portal_arrow_material: Handle<StandardMaterial>, // 橙金自发光材质恢复中央方向箭头。
    pub portal_edge_beam_material: Handle<StandardMaterial>, // 低透明叠加材质形成边缘直光幕。
    pub portal_edge_ring_material: Handle<StandardMaterial>, // 环面在草根间提供高亮边界。
    pub portal_label_font: Handle<Font>, // 作者预览与正式传送点共享可完整烘焙中文名称的字体。
                                                // 地表预览由SurfacePreviewState持有正式PBR材质，不再分配纯色替代材质。
}

/// 当前左键地形笔划；只保存触及GLB顶点的首次世界Y，不复制完整网格。
#[derive(Resource, Default)]
pub struct TerrainBrushStroke {
    pub active: bool,
    pub before: BTreeMap<usize, f32>, // 顶点下标到笔划首次世界Y。
    pub flatten_target: Option<f32>,  // 压平目标直接使用世界米制高度。
}

/// 地表笔划分别记录权重与局部层索引的首次字节，撤销时两张作者图必须原子恢复。
#[derive(Resource, Default)]
pub struct SurfaceBrushStroke {
    pub active: bool,
    pub pending_ground: Option<(
        MapId,
        crate::viewport::vegetation_ground::GroundCursorTarget,
        u64,
        u64,
    )>, // 缺页起笔冻结地图、层/预设、作者高度和地表修订，仅本次按住可延迟开始。
    pub ground_target: Option<crate::viewport::vegetation_ground::GroundCursorTarget>, // 活动Clipmap笔划不跨作者目标。
    pub ground_content: Option<[u64; 7]>, // 已受理样本绑定高度内容，视角换代可继续，内容换代取消旧段。
    pub before_weights: BTreeMap<usize, u8>,
    pub before_indices: BTreeMap<usize, u8>,
    pub last_world: Option<Vec3>, // 连续采样连接相邻鼠标帧，快速拖动也不会留下断线。
    pub waiting_amount: Option<(bool, f32)>, // 同笔真实查询在途累计曝光；擦除语义变化或明确断点不转嫁旧时间。
    pub map_id: Option<MapId>,    // 待补段不能跨图或草稿换代后误写。
    pub layer_id: Option<String>, // 同预设多个合法层仍按按下时稳定作者ID绘制。
    pub layer_index: usize,       // 当前笔划绑定的地图局部索引。
    pub expected_revision: u64,   // 只允许本笔连续推进后的完整作者修订。
    pub expected_surface_revision: u64, // 同图地表换代后禁止用新像素重新构造旧笔撤销。
    pub pending_sample: Option<PendingSurfaceBrushSample>, // 共享等待时保留完整原落点，松键仍会补画。
    pub queued_samples: VecDeque<PendingSurfaceBrushSample>, // 只保存有界标量折线段，不持有作者画布或高度快照。
    pub tail_disconnected: bool, // 队列背压或明确断点后，下个受理点不能跨被跳过的输入拉弦。
    pub release_requested: bool, // 松键、切工具和关闭请求都等待冻结段完成后形成历史。
}

/// 植被R8笔划与开始时的稳定层身份绑定，切层不会污染撤销记录。
#[derive(Resource, Default)]
pub struct VegetationBrushStroke {
    pub pending_ground: Option<(MapId, String)>, // 缺页按下保持意图，仅本次按住可延迟开始。
    pub volume_handoff: Option<(MapId, String)>, // 同次按住从三维面回到外部地表，绑定原地图/作者层后续画。
    pub ground_map: Option<MapId>,               // 防止跨图同名层生成错误撤销。
    pub active: bool,
    pub layer_id: Option<String>,
    pub before: BTreeMap<usize, u8>,
    pub last_world: Option<Vec3>, // 当前笔划上一世界点，只在按住左键期间有效。
    /// 同一次按住的真实射线在途时累计流量；等待不是松笔，不能让采样延迟降低涂抹强度。
    pub waiting_exposure: f32,
    /// 本帧从上一点到当前点的完整刷段，两个即时GPU消费者据此同步中间插值刷印。
    pub segment_start: Option<Vec3>,
}

/// R8或GLB地形网格暂时被后台任务共享时冻结的唯一待补水笔刷段。
#[derive(Clone, Copy)]
pub struct PendingWaterBrushSample {
    pub from: Vec3,           // 资源共享前最后已采用落点。
    pub to: Vec3,             // 本次必须补做的实际鼠标落点。
    pub radius: f32,          // 采样帧冻结的可见硬半径。
    pub erase: bool,          // Shift语义不能随等待期间键盘状态变化。
    pub amount: f32,          // 单击立即强度或连续帧时间积分完成值。
    pub cursor: usize,        // 大图分帧扫描的候选样本游标；仅完整完成后才能推进to。
    pub mask_changed: bool,   // 各局部块合并后只在完整段末发布一次R8修订。
    pub height_changed: bool, // GLB顶点变化同样跨块累积，避免每块触发后台重建。
}

/// 河水复合笔划与稳定层、水位绑定；R8覆盖和GLB顶点河床共享一次撤销边界。
#[derive(Resource, Default)]
pub struct WaterBrushStroke {
    pub active: bool,
    pub pending_ground: Option<(MapId, String)>, // Clipmap按下后等待真实三角命中，松手或换层立即取消。
    pub ground_map: Option<MapId>, // 当前笔划是否绑定Clipmap地表，换图时整笔失效。
    pub map_id: Option<MapId>, // pending补画必须严格回到按下时草稿，禁止同名layer跨图串写。
    pub layer_id: Option<String>,
    pub water_level: Option<f32>, // 笔划中途即使UI状态变化，河床仍使用按下时的静止水位。
    pub mask_before: BTreeMap<usize, u8>, // 当前R8笔划首次触及值。
    pub height_before: BTreeMap<usize, f32>, // 同笔下挖的GLB顶点首次世界Y；Shift擦除时保持为空。
    pub last_world: Option<Vec3>,
    pub segment_start: Option<Vec3>, // 本帧已受理刷段起点，供Clipmap地表覆盖局部即时刷新。
    pub pending_sample: Option<PendingWaterBrushSample>, // 资源共享期间冻结一段，快速松键也必须补做。
    pub queued_sample: Option<PendingWaterBrushSample>, // pending期间合并最新尾段，拖动后松键也不丢B→C。
    pub release_requested: bool, // pending采用后立即收口，不能把click-release静默丢成空笔。
    pub snapshot_mask: Option<Arc<Vec<u8>>>, // 整笔按下前R8版本，不随内部检查点推进。
    pub snapshot_terrain: Option<Arc<TerrainMeshInput>>, // 首次下挖前GLB版本；纯擦除不保留网格。
    pub snapshot_changed: bool, // 内部块变化跨检查点累计，松开才生成一个历史点。
    pub snapshot_height_changed: bool, // 只有真实下挖的笔划才保留前后GLB快照。
}

/// 区域边界控制点拖动状态；完整撤销快照只在按下时保存一次。
#[derive(Resource, Default)]
pub struct BoundaryEditState {
    /// 当前拖动所属地图，换图时立即使旧索引失效。
    pub map_id: Option<MapId>,
    /// 当前拖动路径编号。
    pub path_index: Option<usize>,
    /// 当前拖动控制点编号。
    pub point_index: Option<usize>,
    /// 拖动开始前仅所选墙的快照，用于一次撤销或非法位置回退。
    pub before: Option<feathrise_map_content::MapBoundaryPath>,
    pub selected_point: Option<(String, usize)>, // 松手后保留所选控制点。
    pub expected_revision: u64,                  // 拖动拒绝并发作者修改。
    pub clipmap_content: Option<[u64; 7]>, // 精确拖点冻结基础高度内容，临时缺页只暂停，确认换代取消。
    pub terrain_revision: u64, // 起拖时的作者高度修订；未采用新高度不能继续移动旧控制点。
    pub preview_point: Option<[f32; 2]>, // 已受理中间点只供辅助显示，松手校验成功才原子写入草稿。
    /// 本次拖动是否真实改变过控制点。
    pub moved: bool,
}

impl BoundaryEditState {
    /// 结束当前拖动并释放全部临时身份。
    pub fn clear_drag(&mut self) {
        self.path_index = None; // 不再引用路径数组。
        self.point_index = None; // 不再引用控制点数组。
        self.before = None; // 释放撤销前快照。
        self.clipmap_content = None; // 新拖动不能沿用旧高度来源。
        self.preview_point = None; // 取消不会遗留无历史的作者点修改。
        self.moved = false; // 下一次拖动重新记录真实变化。
    }

    /// 当前是否正在拖动一个区域边界控制点。
    pub fn dragging(&self) -> bool {
        self.before.is_some() && self.path_index.is_some() && self.point_index.is_some() // 只有冻结快照才是活动拖动。
    }
}

/// 河流或瀑布控制点拖动事务；按下冻结完整水域，松手只登记一条撤销命令。
#[derive(Resource, Default)]
pub struct WaterNetworkEditState {
    /// 当前拖动所属地图，换图后旧事务立即失效。
    pub map_id: Option<MapId>,
    /// 当前拖动河流或瀑布的稳定ID。
    pub feature_id: Option<String>,
    /// 当前拖动控制点的稳定ID。
    pub point_id: Option<String>,
    /// true表示当前特征属于瀑布集合，false表示属于河流集合。
    pub waterfall: bool,
    /// 拖动开始前完整小型水域文档，用于单条撤销或非法事务回退。
    pub before: Option<MapWaterAsset>,
    /// 只接受本事务逐帧推进产生的作者修订，拒绝并发按钮修改。
    pub expected_revision: u64,
    /// 进入拖动前保存门状态，拖回原位时据此恢复。
    pub dirty_before: bool,
    /// 本次拖动是否真实改变过控制点XZ。
    pub moved: bool,
}

impl WaterNetworkEditState {
    /// 结束当前拖动并释放全部稳定身份与完整快照。
    pub fn clear_drag(&mut self) {
        *self = Self::default(); // 下一次按下必须从当前地图和修订重新冻结事务。
    }

    /// 当前是否拥有完整的水网控制点拖动身份。
    pub fn dragging(&self) -> bool {
        self.map_id.is_some() && self.feature_id.is_some() && self.point_id.is_some()
    }
}

/// F12未保存边界覆盖最近一次发布身份，稳定帧不重复写运行时资源。
#[derive(Resource, Default)]
pub struct BoundaryLiveOverrideState {
    /// 最近发布或清除的地图身份。
    pub map_id: Option<MapId>,
    /// 最近发布的作者边界修订。
    pub revision: u64,
    /// true表示正式动态地图当前由F12内存草稿接管。
    pub active: bool,
}
