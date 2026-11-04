//! 地图编辑器可撤销命令与局部差分普通类型。

use super::surface_layer_edit::SurfaceAuthoringChange; // 引入地表作者层事务所需类型和纯助手。
use std::collections::{BTreeMap, BTreeSet}; // 作者画布快照与唯一Arc内存身份使用确定性集合。
use std::sync::Arc; // 图层结构撤销不复制整套R8像素。 // 地表层表和两张作者图必须同命令精确恢复。

use feathrise_map_content::{
    CelestialLightingProfile, MAX_COURTYARD_MEMBERS_PER_PRESET, MapEnvironmentDescriptor,
    MapPlacementRecord, MapPortalRecord, MapSurfaceLayer, MapVegetationAuthoring,
    MapVegetationLayer, MapWaterAsset, MapWaterEndpoint, MapWaterLayer, MapWaterRiver,
    MapWaterSplinePoint, MapWaterfall,
}; // 命令只保存可序列化作者值，不借用ECS世界。

/// 当前可撤销作者操作；密度和高度使用局部差分，图层结构使用Arc浅快照。
#[derive(Clone)]
pub enum EditorCommand {
    /// 整条拼接关系及吸附姿态一次恢复；曲线正文由Arc共享，字节额度在后台预计算。
    ClipmapStitchesChanged {before:feathrise_map_content::MapTerrainCollection,after:feathrise_map_content::MapTerrainCollection,heap_bytes:usize},
    /// 对象画布命令仍按地图线性撤销，但必须先恢复明确所属的地形槽位。
    TerrainCanvas { id: feathrise_map_content::TerrainId, command: Box<EditorCommand> },
    /// 根静水独立于网格作者槽；撤销先恢复根水，不能将同名水层误写入当前兄弟网格。
    RootWater(Box<EditorCommand>),
    TerrainObjectsChanged {
        before: feathrise_map_content::MapTerrainCollection,
        after: feathrise_map_content::MapTerrainCollection,
    },
    /// 父地图只拥有引用和整体变换；内部作者数据仍由源地图保存。
    SubMapsChanged {
        before: Arc<[feathrise_map_content::MapInstancePlacement]>,
        after: Arc<[feathrise_map_content::MapInstancePlacement]>,
    },
    /// 只持有有界管理元数据的Arc前后值，批量修改不复制Mesh或整网格水体。
    SceneObjectsChanged(Box<SceneObjectBatchChange>),
    /// 全局太阳/月光基础配置按完整前后快照撤销，颜色三个通道共同保持原子性。
    GlobalCelestialLightingChanged {
        sequence: u64, // 全局作者修订单调命令号，跨地图交接保留原撤销顺序，不写入文件。
        before: CelestialLightingProfile, // 操作前完整六组全局属性。
        after: CelestialLightingProfile, // 操作后已经通过共享校验的完整配置。
    },
    DomainChanged {
        domain: &'static str,
        before: super::domain_documents::DomainValue,
        after: super::domain_documents::DomainValue,
    }, // 整笔前后Arc版本，撤销O(1)。
    PlacementAdded(MapPlacementRecord), // 显式非植被场景实例。
    /// 删除已有摆放时保存原数组位置及预览所有权，撤销必须恢复精确顺序和画面。
    PlacementRemoved {
        index: usize,               // 删除前在placements数组中的稳定位置。
        record: MapPlacementRecord, // 完整记录供撤销原样插回并重建未保存预览。
        had_author_preview: bool,   // 未保存新增项被删除时需要由撤销重新生成WorldAsset根。
    },
    /// 一个已摆放GLB实例的整网格水体语义按完整前后记录原子撤销。
    PlacementChanged {
        record_index: usize, // 命令创建时冻结的数组下标；撤销/重做O(1)直达后仍复核稳定ID。
        before: MapPlacementRecord, // 操作前的变换、资产路径与全部网格水体覆盖。
        after: MapPlacementRecord, // 操作后已经通过公共格式校验的完整记录。
    },
    /// 显式建筑整平把placement与真实GLB顶点差分绑定为一条不可拆分事务。
    BuildingTerrainChanged(Box<BuildingTerrainChange>), // Box避免扩大每条普通历史命令的栈尺寸。
    /// 单栋稀疏平台和建筑共用一条历史，格点Arc可交给后台恢复而不在输入帧复制。
    BuildingSparseTerrainChanged(Box<BuildingSparseTerrainChange>),
    /// 院落落印、整体变换或复制作为一条有界事务恢复全部成员。
    CourtyardBatch(Box<CourtyardBatchChange>), // 大批成员放入Box，避免扩大所有命令枚举的栈尺寸。
    PortalAdded(MapPortalRecord), // 新增点始终追加，撤销时可O(1)移除末项。
    /// 删除已有源点时保存原数组位置及预览所有权，保证撤销恢复精确顺序和画面。
    PortalRemoved {
        index: usize,             // 删除前在当前地图传送数组中的稳定位置。
        record: MapPortalRecord,  // 完整作者记录供撤销原样插回。
        had_author_preview: bool, // 仅未保存新增点需要在撤销时重建作者预览。
    },
    /// 作者传送点名称按稳定ID保存前后值，默认命名状态与文字必须原子撤销。
    PortalLabelChanged {
        portal_id: String,       // 当前地图内稳定ID，名称重名不参与定位。
        before_label: String,    // 修改前保存的作者名称或默认快照。
        before_customized: bool, // 修改前是否仍采用自动“前往目标地图”命名。
        after_label: String,     // 修改后已去除首尾空白的作者名称。
        after_customized: bool,  // 显式应用名称后固定为true。
    },
    TerrainChanged(Vec<HeightChange>), // 局部GLB顶点世界Y差分。
    SparseTerrainChanged(Arc<Vec<super::sparse_terrain::SparseTerrainHeightChange>>), // 完整笔划只读共享，后台撤销不在输入帧复制格点数组。
    VolumeSculptChanged(Box<super::volume_sculpt::VolumeSculptChange>), // 密度前后Arc与自动placement构成一笔原子历史。
    PuddlesChanged {
        // 完整前后快照实现O(1)撤销与重做。
        before: super::puddles::PuddleSnapshot, // 包括首次创建前的空层状态。
        after: super::puddles::PuddleSnapshot,  // 包括完整可保存的独立画布。
    }, // 不混入植被密度或静水覆盖。
    SurfaceChanged {
        weight_changes: Vec<ByteChange>, // RGBA8混合权重局部差分。
        index_changes: Vec<ByteChange>,  // RGBA32_UINT地图局部层索引局部差分。
    },
    /// 第一次真实地表落笔按稳定预设追加一个局部层；只保存新增项，避免复制任意长层表。
    SurfaceLayerAdded {
        index: usize,
        layer: MapSurfaceLayer,
    },
    SurfaceAuthoringChanged(Box<SurfaceAuthoringChange>), // 整层删除的定义、索引和权重Arc前后值原子撤销。
    VegetationChanged {
        layer_id: String,         // 实际被笔刷修改的作者层ID。
        changes: Vec<ByteChange>, // 当前作者R8局部差分。
    },
    VegetationAuthoringChanged(Box<VegetationAuthoringChange>), // 图层增删、分组和参数的浅快照。
    /// 跨网格一笔及绘制组元数据是地图级历史，不能包装成当前TerrainCanvas。
    VegetationPaintChanged(Box<super::vegetation_paint_scope::VegetationPaintChange>),
    #[allow(dead_code)] // 仅既有局部差分原子性回归仍构造；生产水笔划统一写入下方整笔快照。
    WaterStroke {
        layer_id: String,                  // 笔划开始时绑定的独立水层ID。
        mask_changes: Vec<ByteChange>,     // 当前水层R8覆盖的局部差分。
        height_changes: Vec<HeightChange>, // 同笔非擦除操作下挖的GLB顶点世界Y差分。
    },
    WaterStrokeSnapshot(Box<WaterStrokeSnapshot>), // 按下到松开的一笔只保存一个完整前后版本，不把内部块暴露成撤销步骤。
    WaterLevelChanged {
        layer_id: String, // 当前独立水层ID。
        before: f32,      // 操作前静止水位。
        after: f32,       // 操作后静止水位。
    },
    WaterLayersChanged(Box<WaterLayersChange>), // 水层增删与Arc遮罩表必须同次撤销。
    /// 河流、急流和瀑布定义按完整前后水域文档原子撤销。
    WaterDefinitionsChanged {
        before: MapWaterAsset, // 操作前完整静水层与连续水网定义。
        after: MapWaterAsset,  // 操作后已通过编辑期宽松校验的完整定义。
    },
    /// 地图环境按完整前后快照撤销，避免路径、亮度和方向出现部分状态。
    EnvironmentChanged {
        before: MapEnvironmentDescriptor, // 操作前完整环境。
        after: MapEnvironmentDescriptor,  // 操作后完整环境。
    },
    /// 空气墙对象事务只保存受影响的一道墙，不复制全图墙体。
    BoundaryPathChanged {
        index: usize,                                     // 删除后撤销恢复原列表位置。
        before: Option<feathrise_map_content::MapBoundaryPath>, // None表示新建。
        after: Option<feathrise_map_content::MapBoundaryPath>,  // None表示删除。
    },
}

/// 对象管理历史只保存轻量Arc字段差分，预先计费避免每次历史裁剪重扫全组。
#[derive(Clone)]
pub struct SceneObjectBatchChange {
    pub changes: Vec<SceneObjectChange>,
    pub heap_bytes: usize,
}

impl SceneObjectBatchChange {
    pub fn apply(&self, placements: &mut feathrise_map_content::MapPlacementsAsset, undoing: bool) -> Result<(), String> {
        if self.changes.len() > feathrise_map_content::MAX_MAP_PLACEMENTS_PER_MAP { return Err("对象管理历史数量无效".into()); }
        for change in &self.changes {
            let record = placements.placements.get(change.index).filter(|record| record.id == change.id)
                .ok_or("场景对象历史目标已经变化")?;
            let expected = if undoing { &change.after } else { &change.before };
            if &record.editor != expected { return Err("场景对象历史版本已经变化".into()); }
            (if undoing { &change.before } else { &change.after }).validate()?;
        }
        for change in &self.changes {
            placements.placements[change.index].editor = if undoing { change.before.clone() } else { change.after.clone() };
        }
        Ok(())
    }
}

/// 每份摆放的元数据差分，稳定ID与下标共同阻止重排后误改另一对象。
#[derive(Clone)]
pub struct SceneObjectChange {
    pub index: usize,
    pub id: String,
    pub before: Arc<feathrise_map_content::MapPlacementEditorMetadata>,
    pub after: Arc<feathrise_map_content::MapPlacementEditorMetadata>,
}

/// 联合整平中建筑记录的前后版本；新增与原位修改共享同一地形差分。
#[derive(Clone)]
pub enum BuildingPlacementChange {
    Added {
        record_index: usize,        // 首次采用时的尾部数组下标。
        record: MapPlacementRecord, // 撤销删除、重做原样恢复的完整建筑记录。
    },
    Changed {
        record_index: usize,        // 移动建筑的稳定数组槽。
        before: MapPlacementRecord, // 地形与建筑共同操作前的完整记录。
        after: MapPlacementRecord,  // 地形与建筑共同操作后的完整记录。
    },
}

/// 一次建筑新建、移动或复制与局部地形抬升的原子历史载荷。
#[derive(Clone)]
pub struct BuildingTerrainChange {
    pub placement: BuildingPlacementChange, // 建筑侧新增或原位变换。
    pub heights: Vec<HeightChange>,         // 同一操作修改的全部GLB属性顶点Y。
}

#[derive(Clone)]
pub struct BuildingSparseTerrainChange {
    pub placement: BuildingPlacementChange,
    pub heights: Arc<Vec<super::sparse_terrain::SparseTerrainHeightChange>>,
}

/// 一次院落作者操作中追加的成员和原位修改的成员。
#[derive(Clone)]
pub struct CourtyardBatchChange {
    pub added: Vec<MapPlacementRecord>, // 落印或整组复制始终向placements末尾追加。
    pub changed: Vec<(usize, MapPlacementRecord, MapPlacementRecord)>, // 组建和整体移动保留原数组下标、前后完整值。
    pub removed: Vec<(usize, MapPlacementRecord)>, // 批量删除按原下标降序交换移除；撤销反向交换恢复原排列。
    pub original_len: Option<usize>, // 删除历史核对完整长度，禁止在结构已变化时猜测尾部关系。
    heap_bytes: usize, // 创建命令时一次计费，后续历史裁剪不重复扫描最多256个成员。
}

impl CourtyardBatchChange {
    /// 构造固定上限的完整院落命令，并冻结其真实堆容量用于历史预算。
    pub fn new(
        added: Vec<MapPlacementRecord>, // 新成员完整快照。
        changed: Vec<(usize, MapPlacementRecord, MapPlacementRecord)>, // 已有成员的完整前后快照。
    ) -> Result<Self, String> {
        let member_count = added.len().saturating_add(changed.len()); // 单笔操作不能绕过预设的成员上限。
        if member_count == 0 || member_count > MAX_COURTYARD_MEMBERS_PER_PRESET {
            return Err(format!(
                "院落历史命令成员数{member_count}无效，最多{MAX_COURTYARD_MEMBERS_PER_PRESET}项"
            )); // 空命令和过大快照都不能进入作者历史。
        }
        if changed.iter().any(|(_, before, after)| {
            before.id != after.id || placement_water_differs(before, after)
        }) {
            return Err("院落整批操作不能更换已有成员 ID 或网格水体语义".to_owned()); // 组合操作只更改成员变换与归属，水编辑仍走单体证据链。
        }
        let mut heap_bytes = added
            .capacity()
            .saturating_mul(std::mem::size_of::<MapPlacementRecord>())
            .saturating_add(changed.capacity().saturating_mul(std::mem::size_of::<(
                usize,
                MapPlacementRecord,
                MapPlacementRecord,
            )>())); // Vec本身预留的完整记录槽也计入历史占用。
        for record in &added {
            heap_bytes = heap_bytes.saturating_add(placement_record_heap_bytes(record)); // 新成员持有模型、院落和可选水绑定字符串。
        }
        for (_, before, after) in &changed {
            heap_bytes = heap_bytes
                .saturating_add(placement_record_heap_bytes(before))
                .saturating_add(placement_record_heap_bytes(after)); // 前后两份记录都属于同一可撤销事务。
        }
        Ok(Self {
            added,
            changed,
            removed: Vec::new(),
            original_len: None,
            heap_bytes,
        }) // 构造后只移动或克隆，不修改冻结快照。
    }

    /// 删除只捕获选中的有限记录；交换移除避免按每个下标移动完整地图数组。
    pub fn new_removed(mut removed: Vec<(usize, MapPlacementRecord)>, original_len: usize) -> Result<Self, String> {
        if removed.is_empty() || removed.len() > MAX_COURTYARD_MEMBERS_PER_PRESET {
            return Err("批量删除数量无效".into());
        }
        removed.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        if removed.iter().any(|(index, _)| *index >= original_len)
            || removed.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err("批量删除下标无效或重复".into());
        }
        let mut ids = std::collections::HashSet::with_capacity(removed.len());
        if removed.iter().any(|(_, record)| record.id.is_empty() || !ids.insert(record.id.as_str())) {
            return Err("批量删除实例身份无效或重复".into());
        }
        let heap_bytes = removed.capacity() * std::mem::size_of::<(usize, MapPlacementRecord)>()
            + removed.iter().map(|(_, record)| placement_record_heap_bytes(record)).sum::<usize>();
        Ok(Self { added: Vec::new(), changed: Vec::new(), removed, original_len: Some(original_len), heap_bytes })
    }
}

impl EditorCommand {
    pub fn unscoped(&self) -> &Self {
        match self { Self::TerrainCanvas { command, .. } | Self::RootWater(command) => command.unscoped(), _ => self }
    }
    pub fn is_water_canvas(&self) -> bool {
        matches!(self.unscoped(), Self::WaterStroke { .. } | Self::WaterStrokeSnapshot(_)
            | Self::WaterLevelChanged { .. } | Self::WaterLayersChanged(_) | Self::WaterDefinitionsChanged { .. })
    }
    pub fn is_terrain_canvas(&self) -> bool {
        matches!(self.unscoped(), Self::TerrainChanged(_) | Self::SparseTerrainChanged(_) | Self::BuildingSparseTerrainChanged(_)
            | Self::SurfaceChanged { .. } | Self::SurfaceLayerAdded { .. }
            | Self::SurfaceAuthoringChanged(_) | Self::PuddlesChanged { .. } | Self::VegetationChanged { .. }
            | Self::VegetationAuthoringChanged(_) | Self::WaterStroke { .. } | Self::WaterStrokeSnapshot(_)
            | Self::WaterLevelChanged { .. } | Self::WaterLayersChanged(_) | Self::WaterDefinitionsChanged { .. })
            || matches!(self.unscoped(), Self::BuildingTerrainChanged(change) if !change.heights.is_empty())
    }
    pub const fn global_history_id(&self) -> Option<u64> {
        match self {
            Self::GlobalCelestialLightingChanged { sequence, .. } => Some(*sequence),
            _ => None,
        }
    }
    /// 返回命令是否只修改唯一全局世界环境文件而不要求地图保存预检。
    pub const fn is_global_environment(&self) -> bool {
        matches!(self, Self::GlobalCelestialLightingChanged { .. }) // 新增全局属性时必须继续收口到该命令族。
    }

    /// 稀疏高度先独立发布块清单，后续联合保存据此区分是否仍有其他地图文件责任。
    pub fn is_sparse_terrain(&self) -> bool {
        matches!(self.unscoped(), Self::SparseTerrainChanged(_))
    }
}

/// 两份完整摆放记录是否会改变GLB实例树或整网格水绑定诊断行。
pub fn placement_structure_differs(
    before: &MapPlacementRecord,
    after: &MapPlacementRecord,
) -> bool {
    before.id != after.id
        || before.asset_path != after.asset_path
        || before.asset_target != after.asset_target
        || before.building_preset_id != after.building_preset_id // 建筑语义变化要求右侧树及派生空间索引换代。
        || before.courtyard != after.courtyard // 院落归属变化要求成员选择与结构身份换代。
        || before.water_surfaces.len() != after.water_surfaces.len()
        || before
            .water_surfaces
            .iter()
            .zip(&after.water_surfaces)
            .any(|(before, after)| {
                before.node_path != after.node_path
                    || before.mesh_name != after.mesh_name
                    || before.mesh_index != after.mesh_index
                    || before.kind != after.kind
            }) // 流速、水面高度、摆放变换及地形跟随模式不会改变浏览树，故意不进入结构修订。
}

/// 两份完整摆放记录是否真实改变了整网格水体数据。
pub fn placement_water_differs(
    before: &MapPlacementRecord, // 操作前完整记录。
    after: &MapPlacementRecord,  // 操作后完整记录。
) -> bool {
    before.water_surfaces != after.water_surfaces // 平移、旋转、缩放和高度模式不应冻结网格水交接或占用水历史预算。
}

/// 植被作者图层结构及R8 Arc表的一次可撤销前后值。
#[derive(Clone)]
pub struct VegetationAuthoringChange {
    pub before_outputs: Vec<MapVegetationLayer>, // 首次使用预设前的地图正式输出集合。
    pub after_outputs: Vec<MapVegetationLayer>,  // 首次使用预设后或普通操作的不变输出集合。
    pub before: MapVegetationAuthoring,          // 操作前组、层顺序和参数。
    pub after: MapVegetationAuthoring,           // 操作后组、层顺序和参数。
    pub before_densities: BTreeMap<String, Arc<Vec<u8>>>, // 操作前画布表浅快照。
    pub after_densities: BTreeMap<String, Arc<Vec<u8>>>, // 操作后画布表浅快照。
}

/// 水域层描述及R8 Arc表的一次可撤销前后值。
#[derive(Clone)]
pub struct WaterLayersChange {
    pub before: MapWaterAsset, // 操作前全部水层描述和独立水位。
    pub after: MapWaterAsset,  // 操作后完整受检水层描述。
    pub before_masks: BTreeMap<String, Arc<Vec<u8>>>, // 浅快照保留被删层供撤销。
    pub after_masks: BTreeMap<String, Arc<Vec<u8>>>, // 复制层可继续共享只读像素。
}

/// 静水整笔前后版本；复合河床与遮罩只做常数时间原子换指针。
#[derive(Clone)]
pub struct WaterStrokeSnapshot {
    pub layer_id: String,          // 按下时绑定的稳定水层。
    pub before_mask: Arc<Vec<u8>>, // 本笔首次写入前的完整R8。
    pub after_mask: Arc<Vec<u8>>,  // 全部待补段完成后的完整R8。
    pub terrain: Option<(
        Arc<feathrise_map_terrain::TerrainMeshInput>,
        Arc<feathrise_map_terrain::TerrainMeshInput>,
    )>, // 纯擦除为None，实际下挖则成对恢复GLB。
}

/// 一次笔划内单个高度样本的前后值。
#[derive(Clone, Debug)]
pub struct HeightChange {
    pub index: usize, // GLB原顶点顺序下标。
    pub before: f32,  // 笔划首次触及时的世界Y。
    pub after: f32,   // 笔划完成后的世界Y。
}

/// R8或RGBA8作者源中一个字节的局部可撤销差分。
#[derive(Clone)]
pub struct ByteChange {
    pub index: usize, // 作者源连续字节下标。
    pub before: u8,   // 操作前值。
    pub after: u8,    // 操作后值。
}

const MAX_RETAINED_WATER_UNDO_SAMPLES: usize = 262_144 * 4; // 最多保留四笔满预算R8/GLB顶点局部差分。
const MAX_RETAINED_WATER_MASK_SNAPSHOT_BYTES: usize = 160 * 1024 * 1024; // 八张4096² R8结构快照及一代写侧仍有明确硬上限。
const MAX_RETAINED_WATER_ASSET_BYTES: usize = 128 * 1024 * 1024; // 完整水网前后快照合计堆内存硬上限。
const MAX_RETAINED_WATER_COMMANDS: usize = 256; // 小型水位/参数命令也不得无界增长。

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct WaterHistoryUsage {
    commands: usize,    // 所有会改变水表现的作者命令总数，包括整网格赋水。
    samples: usize,     // WaterStroke两类局部差分样本数。
    mask_bytes: usize,  // WaterLayersChanged中按Arc指针去重后的Vec容量。
    asset_bytes: usize, // 完整MapWaterAsset及其字符串/Vec堆容量估算。
}

fn endpoint_heap_bytes(endpoint: &MapWaterEndpoint) -> usize {
    match endpoint {
        MapWaterEndpoint::StillWater { layer_id } => layer_id.capacity(),
        MapWaterEndpoint::RiverStart { river_id } | MapWaterEndpoint::RiverEnd { river_id } => {
            river_id.capacity()
        }
    }
}

fn water_asset_heap_bytes(asset: &MapWaterAsset) -> usize {
    let mut bytes = asset.map_id.as_str().len(); // MapId加载/克隆后的容量至少覆盖当前UTF-8长度。
    bytes = bytes.saturating_add(asset.layers.capacity() * std::mem::size_of::<MapWaterLayer>());
    for layer in &asset.layers {
        bytes = bytes
            .saturating_add(layer.id.capacity())
            .saturating_add(layer.label.capacity())
            .saturating_add(layer.mask_source_path.capacity())
            .saturating_add(layer.mask_texture_path.capacity());
    }
    bytes = bytes.saturating_add(asset.rivers.capacity() * std::mem::size_of::<MapWaterRiver>());
    for river in &asset.rivers {
        bytes = bytes
            .saturating_add(river.id.capacity())
            .saturating_add(river.label.capacity())
            .saturating_add(river.source_layer_id.as_ref().map_or(0, String::capacity))
            .saturating_add(river.target_layer_id.as_ref().map_or(0, String::capacity))
            .saturating_add(river.points.capacity() * std::mem::size_of::<MapWaterSplinePoint>());
        for point in &river.points {
            bytes = bytes.saturating_add(point.id.capacity());
        }
    }
    bytes = bytes.saturating_add(asset.waterfalls.capacity() * std::mem::size_of::<MapWaterfall>());
    for waterfall in &asset.waterfalls {
        bytes = bytes
            .saturating_add(waterfall.id.capacity())
            .saturating_add(waterfall.label.capacity())
            .saturating_add(waterfall.source.as_ref().map_or(0, endpoint_heap_bytes))
            .saturating_add(waterfall.target.as_ref().map_or(0, endpoint_heap_bytes))
            .saturating_add(
                waterfall.points.capacity() * std::mem::size_of::<MapWaterSplinePoint>(),
            );
        for point in &waterfall.points {
            bytes = bytes.saturating_add(point.id.capacity());
        }
    }
    bytes
}

/// 估算一个显式摆放记录及其整网格水体稳定引用所持有的堆内存。
pub fn placement_record_heap_bytes(record: &MapPlacementRecord) -> usize {
    let mut bytes = record.editor.name.capacity() + record.editor.group.capacity() + record
        .id
        .capacity()
        .saturating_add(record.asset_path.capacity())
        .saturating_add(
            record
                .building_preset_id
                .as_ref()
                .map_or(0, String::capacity),
        ) // 建筑稳定身份属于完整撤销快照持有的堆字符串。
        .saturating_add(record.courtyard.as_ref().map_or(0, |courtyard| {
            courtyard
                .instance_id
                .capacity()
                .saturating_add(courtyard.preset_id.capacity())
                .saturating_add(courtyard.member_id.capacity())
        })) // 院落副本、预设和成员身份都由完整撤销快照持有。
        .saturating_add(
            record.water_surfaces.capacity()
                * std::mem::size_of::<feathrise_map_content::MapPlacementWaterSurface>(),
        );
    for surface in &record.water_surfaces {
        bytes = bytes
            .saturating_add(surface.mesh_name.capacity())
            .saturating_add(surface.node_path.capacity() * std::mem::size_of::<String>());
        for segment in &surface.node_path {
            bytes = bytes.saturating_add(segment.capacity());
        }
    }
    bytes // 小型完整记录也进入统一水作者历史字节门，连续参数操作不能无界增长。
}

/// 命令是否持有需要服从统一水作者内存预算的记录、遮罩或差分快照。
pub fn command_uses_bounded_water_history(command: &EditorCommand) -> bool {
    match command.unscoped() {
        EditorCommand::ClipmapStitchesChanged {..}=>true,
        EditorCommand::PlacementAdded(record) | EditorCommand::PlacementRemoved { record, .. } => {
            !record.water_surfaces.is_empty() // 复制或删除带水资产同样持有完整节点路径和网格名。
        }
        EditorCommand::PlacementChanged { before, after, .. } => {
            placement_water_differs(before, after) // 资产纯变换仍是普通小型作者命令。
        }
        EditorCommand::BuildingTerrainChanged(change) => match &change.placement {
            BuildingPlacementChange::Added { record, .. } => !record.water_surfaces.is_empty(),
            BuildingPlacementChange::Changed { before, after, .. } => {
                placement_water_differs(before, after)
            }
        }, // 联合命令的建筑记录仍服从同一整网格水历史预算。
        EditorCommand::CourtyardBatch(_) | EditorCommand::BuildingSparseTerrainChanged(_) | EditorCommand::SceneObjectsChanged(_) => true, // 整组管理也复用统一历史内存预算。
        EditorCommand::VegetationPaintChanged(_) // 多网格完整R8共享同一有界历史预算。
        | EditorCommand::PuddlesChanged { .. } // 独立积水快照共用历史内存预算，不复用作者数据。
        | EditorCommand::WaterStroke { .. }
        | EditorCommand::WaterStrokeSnapshot(_)
        | EditorCommand::WaterLevelChanged { .. }
        | EditorCommand::WaterLayersChanged(_)
        | EditorCommand::WaterDefinitionsChanged { .. } => true, // 其余四类始终属于水作者历史。
        _ => false, // 摆放增删、传送、地形和环境不占用水命令额度。
    }
}

fn water_history_usage(commands: &[EditorCommand]) -> WaterHistoryUsage {
    let mut usage = WaterHistoryUsage::default();
    let mut seen_masks = BTreeSet::new();
    let mut seen_terrain = BTreeSet::new(); // 连续整笔共享的前后GLB版本只计一次。
    for command in commands {
        let command = command.unscoped(); // 包装只增加对象身份，不能使像素快照逃过统一历史预算。
        if command_uses_bounded_water_history(command) {
            usage.commands = usage.commands.saturating_add(1);
        }
        match command {
            EditorCommand::ClipmapStitchesChanged {heap_bytes,..}=>{usage.asset_bytes=usage.asset_bytes.saturating_add(*heap_bytes);}
            EditorCommand::VegetationPaintChanged(change) => {
                usage.asset_bytes=usage.asset_bytes.saturating_add((change.before_groups.len()+change.after_groups.len())
                    .saturating_mul(std::mem::size_of::<feathrise_map_content::MapVegetationPaintGroup>())); // 组元数据同样承担历史预算，纯勾选不能无限积累。
                for buffer in change.buffers() {
                    if seen_masks.insert(Arc::as_ptr(buffer) as usize) { usage.mask_bytes=usage.mask_bytes.saturating_add(buffer.capacity()); }
                }
            }
            EditorCommand::PuddlesChanged { before, after } => {
                // 只按Arc容量计费，不逐像素扫描。
                for mask in [before.mask.as_ref(), after.mask.as_ref()]
                    .into_iter()
                    .flatten()
                {
                    // 空层不持有画布。
                    if seen_masks.insert(Arc::as_ptr(mask) as usize) {
                        // 连续历史共享版本只计一次。
                        usage.mask_bytes = usage.mask_bytes.saturating_add(mask.capacity()); // 防止积水历史无界增长。
                    } // 完成去重累计。
                } // 完成两个端点。
            } // 结束积水快照计费。
            EditorCommand::WaterStrokeSnapshot(change) => {
                for mask in [&change.before_mask, &change.after_mask] {
                    // 同一完整R8版本按所有权身份去重。
                    if seen_masks.insert(Arc::as_ptr(mask) as usize) {
                        // 与水层结构快照共享同一像素预算。
                        usage.mask_bytes = usage.mask_bytes.saturating_add(mask.capacity()); // 容量包括Vec预留空间。
                    }
                }
                if let Some((before, after)) = &change.terrain {
                    // 纯擦除没有网格历史负担。
                    for mesh in [before, after] {
                        // 至多两个版本，不扫描任何顶点。
                        if seen_terrain.insert(Arc::as_ptr(mesh) as usize) {
                            // 历史相邻版本只计一次。
                            usage.asset_bytes = usage
                                .asset_bytes
                                .saturating_add(mesh.positions.capacity() * 12)
                                .saturating_add(mesh.indices.capacity() * 4); // 完整GLB位置和拓扑均进入字节预算。
                        }
                    }
                }
            }
            EditorCommand::PlacementAdded(record)
            | EditorCommand::PlacementRemoved { record, .. }
                if !record.water_surfaces.is_empty() =>
            {
                usage.asset_bytes = usage
                    .asset_bytes
                    .saturating_add(placement_record_heap_bytes(record)); // 结构操作只持有一份完整含水记录。
            }
            EditorCommand::PlacementChanged { before, after, .. }
                if placement_water_differs(before, after) =>
            {
                usage.asset_bytes = usage
                    .asset_bytes
                    .saturating_add(placement_record_heap_bytes(before))
                    .saturating_add(placement_record_heap_bytes(after));
            }
            EditorCommand::BuildingTerrainChanged(change) => match &change.placement {
                BuildingPlacementChange::Added { record, .. }
                    if !record.water_surfaces.is_empty() =>
                {
                    usage.asset_bytes = usage
                        .asset_bytes
                        .saturating_add(placement_record_heap_bytes(record));
                }
                BuildingPlacementChange::Changed { before, after, .. }
                    if placement_water_differs(before, after) =>
                {
                    usage.asset_bytes = usage
                        .asset_bytes
                        .saturating_add(placement_record_heap_bytes(before))
                        .saturating_add(placement_record_heap_bytes(after));
                }
                _ => {}
            },
            EditorCommand::CourtyardBatch(change) => {
                usage.asset_bytes = usage.asset_bytes.saturating_add(change.heap_bytes); // 预计算成员字节让每次裁剪只按命令数线性工作。
            }
            EditorCommand::SceneObjectsChanged(change) => {
                usage.asset_bytes = usage.asset_bytes.saturating_add(change.heap_bytes);
            }
            EditorCommand::BuildingSparseTerrainChanged(change) => {
                usage.asset_bytes = usage.asset_bytes.saturating_add(
                    change.heights.capacity()
                        * std::mem::size_of::<super::sparse_terrain::SparseTerrainHeightChange>(),
                );
                usage.asset_bytes = usage.asset_bytes.saturating_add(match &change.placement {
                    BuildingPlacementChange::Added { record, .. } => {
                        placement_record_heap_bytes(record)
                    }
                    BuildingPlacementChange::Changed { before, after, .. } => {
                        placement_record_heap_bytes(before)
                            .saturating_add(placement_record_heap_bytes(after))
                    }
                }); // Arc差分以真实容量计费，统一前缀裁剪保留完整联合事务。
            }
            EditorCommand::WaterStroke {
                mask_changes,
                height_changes,
                ..
            } => {
                usage.samples = usage
                    .samples
                    .saturating_add(mask_changes.len())
                    .saturating_add(height_changes.len());
            }
            EditorCommand::WaterLayersChanged(change) => {
                usage.asset_bytes = usage
                    .asset_bytes
                    .saturating_add(water_asset_heap_bytes(&change.before))
                    .saturating_add(water_asset_heap_bytes(&change.after));
                for mask in change
                    .before_masks
                    .values()
                    .chain(change.after_masks.values())
                {
                    let identity = Arc::as_ptr(mask) as usize;
                    if seen_masks.insert(identity) {
                        usage.mask_bytes = usage.mask_bytes.saturating_add(mask.capacity());
                    }
                }
            }
            EditorCommand::WaterDefinitionsChanged { before, after } => {
                usage.asset_bytes = usage
                    .asset_bytes
                    .saturating_add(water_asset_heap_bytes(before))
                    .saturating_add(water_asset_heap_bytes(after));
            }
            EditorCommand::WaterLevelChanged { layer_id, .. } => {
                usage.asset_bytes = usage.asset_bytes.saturating_add(layer_id.capacity());
            }
            _ => {}
        }
    }
    usage
}

fn trim_water_history_to_budget(undo: &mut Vec<EditorCommand>) {
    trim_water_history_with_mask_budget(undo, MAX_RETAINED_WATER_MASK_SNAPSHOT_BYTES); // 正式容量保持原配置。
}

fn trim_water_history_with_mask_budget(undo: &mut Vec<EditorCommand>, mask_budget: usize) {
    loop {
        let usage = water_history_usage(undo);
        if usage.samples <= MAX_RETAINED_WATER_UNDO_SAMPLES
            && usage.mask_bytes <= mask_budget // 小测试预算复用同一生产裁剪路径。
            && usage.asset_bytes <= MAX_RETAINED_WATER_ASSET_BYTES
            && usage.commands <= MAX_RETAINED_WATER_COMMANDS
        {
            return;
        }
        let Some(oldest_water_index) = undo.iter().position(command_uses_bounded_water_history)
        else {
            return; // 统计只来自上述显式水载荷命令；防御未来修改造成无法收敛的循环。
        };
        if oldest_water_index + 1 == undo.len() {
            // 水层、水网、绑定和笔划再大也保留最新完整事务，缓存预算不能变成作者容量门。
            undo.drain(..oldest_water_index); // 超预算时只保留最新一笔，峰值受两代作者源大小约束。
            return; // 其余旧历史已释放，不能继续裁当前唯一事务。
        }
        undo.drain(..=oldest_water_index); // 烘焙最旧水结果时同步删除其前方全部命令，保持线性撤销基线。
    }
}

/// 会话入口与纯测试复用此Vec级裁剪；生产调用须由会话同步调整画布归属游标。
pub fn push_bounded_water_history(
    undo: &mut Vec<EditorCommand>,
    redo: &mut Vec<EditorCommand>,
    command: EditorCommand,
) {
    redo.clear(); // 新作者操作首先终止旧重做分支，释放其中可能保留的整图Arc。
    undo.push(command);
    trim_water_history_to_budget(undo);
}

/// Vec级重做保留后续redo链；生产调用须通过会话入口同步更新归属游标。
pub fn push_redone_bounded_water_history(
    undo: &mut Vec<EditorCommand>,
    command: EditorCommand,
) {
    undo.push(command);
    trim_water_history_to_budget(undo);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_building_joint_history_counts_real_capacity_and_evicts_complete_prefix() {
        let record: MapPlacementRecord = serde_json::from_str(r#"{"id":"house", "asset_path":"gltf/house.glb", "translation":[0,0,0], "yaw_degrees":0, "scale":[1,1,1]}"#).unwrap();
        let mut heights = Vec::with_capacity(5);
        heights.push(super::super::sparse_terrain::SparseTerrainHeightChange {
            x: 1,
            z: 1,
            before: 0,
            after: 1,
        });
        let bytes = heights.capacity()
            * std::mem::size_of::<super::super::sparse_terrain::SparseTerrainHeightChange>();
        let command =
            EditorCommand::BuildingSparseTerrainChanged(Box::new(BuildingSparseTerrainChange {
                placement: BuildingPlacementChange::Added {
                    record_index: 0,
                    record,
                },
                heights: Arc::new(heights),
            }));
        assert!(!command.is_sparse_terrain()); // 保存高度后还有建筑JSON责任。
        let usage = water_history_usage(std::slice::from_ref(&command));
        assert_eq!(usage.commands, 1);
        assert!(usage.asset_bytes > bytes); // 差分按容量、建筑按完整记录共同计费。
        let mut undo = vec![EditorCommand::TerrainChanged(Vec::new())];
        let mut redo = Vec::new();
        for _ in 0..=MAX_RETAINED_WATER_COMMANDS {
            push_bounded_water_history(&mut undo, &mut redo, command.clone());
        }
        assert_eq!(undo.len(), MAX_RETAINED_WATER_COMMANDS);
        assert!(
            undo.iter()
                .all(|item| matches!(item, EditorCommand::BuildingSparseTerrainChanged(_)))
        ); // 丢最旧完整前缀，联合命令不可拆半条。
    }

    #[test]
    fn oversized_latest_stroke_retains_both_versions_and_evicts_older_prefix() {
        let before = Arc::new(vec![0; 8]); // 小预算即可验证完整版本保留，无需分配大画布。
        let after = Arc::new(vec![1; 8]); // 独立最终版本。
        let mut undo = vec![
            EditorCommand::TerrainChanged(Vec::new()), // 旧线性前缀不能独立残留。
            water_stroke(1),                           // 超预算时回收整条旧水命令。
            EditorCommand::WaterStrokeSnapshot(Box::new(WaterStrokeSnapshot {
                layer_id: "lake".to_owned(), // 最新完整笔划。
                before_mask: before.clone(), // 撤销最初状态。
                after_mask: after.clone(),   // 重做最终状态。
                terrain: None,               // 擦除不含地形。
            })),
        ];
        trim_water_history_with_mask_budget(&mut undo, 1); // 最新一笔自身也超过预算。
        assert_eq!(undo.len(), 1); // 仅保留最新完整事务。
        let EditorCommand::WaterStrokeSnapshot(change) = &undo[0] else {
            panic!("最新整笔历史必须保留")
        };
        assert!(Arc::ptr_eq(&change.before_mask, &before)); // 不能裁掉操作前版本。
        assert!(Arc::ptr_eq(&change.after_mask, &after)); // 不能裁掉重做版本。
    }

    fn water_stroke(sample_count: usize) -> EditorCommand {
        EditorCommand::WaterStroke {
            layer_id: "lake".to_owned(),
            mask_changes: (0..sample_count)
                .map(|index| ByteChange {
                    index,
                    before: 0,
                    after: 1,
                })
                .collect(),
            height_changes: Vec::new(),
        }
    }

    #[test]
    fn over_budget_water_eviction_drains_the_whole_linear_prefix() {
        let mut undo = vec![
            EditorCommand::TerrainChanged(Vec::new()),
            water_stroke(2),
            EditorCommand::SurfaceChanged {
                weight_changes: Vec::new(),
                index_changes: Vec::new(),
            },
            water_stroke(2),
        ];

        // 用测试内的小预算复现：不能只remove索引1而留下依赖它之前基线的地形命令。
        while water_history_usage(&undo).samples > 2 {
            let oldest = undo
                .iter()
                .position(|command| matches!(command, EditorCommand::WaterStroke { .. }))
                .expect("超预算来自水笔划");
            undo.drain(..=oldest);
        }

        assert_eq!(undo.len(), 2);
        assert!(matches!(undo[0], EditorCommand::SurfaceChanged { .. }));
        assert!(matches!(undo[1], EditorCommand::WaterStroke { .. }));
    }

    #[test]
    fn layer_snapshots_count_each_shared_arc_only_once() {
        let shared = Arc::new(vec![0_u8; 7]);
        let mut before_masks = BTreeMap::new();
        before_masks.insert("lake".to_owned(), shared.clone());
        let mut after_masks = before_masks.clone();
        after_masks.insert("pond".to_owned(), shared);
        let command = EditorCommand::WaterLayersChanged(Box::new(WaterLayersChange {
            before: MapWaterAsset::default(),
            after: MapWaterAsset::default(),
            before_masks,
            after_masks,
        }));

        assert_eq!(water_history_usage(&[command]).mask_bytes, 7);
    }

    #[test]
    fn oversized_latest_water_layer_keeps_its_complete_undo_snapshot() {
        let before = Arc::new(vec![0; 8]);
        let after = Arc::new(vec![255; 8]);
        let mut undo = vec![
            water_stroke(1),
            EditorCommand::WaterLayersChanged(Box::new(WaterLayersChange {
                before: MapWaterAsset::default(),
                after: MapWaterAsset::default(),
                before_masks: BTreeMap::from([("lake".into(), before.clone())]),
                after_masks: BTreeMap::from([("lake".into(), after.clone())]),
            })),
        ];
        trim_water_history_with_mask_budget(&mut undo, 1);
        assert_eq!(undo.len(), 1);
        let EditorCommand::WaterLayersChanged(change) = &undo[0] else {
            panic!("完整最新水层历史必须保留")
        };
        assert!(Arc::ptr_eq(&change.before_masks["lake"], &before));
        assert!(Arc::ptr_eq(&change.after_masks["lake"], &after));
    }

    #[test]
    fn water_level_commands_share_the_same_count_bound_and_prefix_rule() {
        let mut undo = vec![EditorCommand::TerrainChanged(Vec::new())];
        let mut redo = Vec::new();
        for index in 0..=MAX_RETAINED_WATER_COMMANDS {
            push_bounded_water_history(
                &mut undo,
                &mut redo,
                EditorCommand::WaterLevelChanged {
                    layer_id: format!("lake-{index}"),
                    before: index as f32,
                    after: index as f32 + 1.0,
                },
            );
        }

        assert_eq!(
            water_history_usage(&undo).commands,
            MAX_RETAINED_WATER_COMMANDS
        );
        assert_eq!(undo.len(), MAX_RETAINED_WATER_COMMANDS); // 超限时地形前缀与最旧水位命令一起烘焙。
        assert!(matches!(
            undo.first(),
            Some(EditorCommand::WaterLevelChanged { .. })
        ));
    }

    #[test]
    fn complete_water_definition_snapshots_contribute_heap_bytes() {
        let map_id = feathrise_map_content::MapId::parse("MAP-history").expect("测试ID合法");
        let before =
            crate::authoring::water_asset_with_layer_for_tests(&map_id, "maps/MAP-history");
        let mut after = before.clone();
        after.layers[0].label = "包含独立堆字符串的修改后湖泊".to_owned();
        let usage =
            water_history_usage(&[EditorCommand::WaterDefinitionsChanged { before, after }]);

        assert_eq!(usage.commands, 1);
        assert!(usage.asset_bytes > 0);
    }

    #[test]
    fn placement_water_changes_share_the_bounded_water_history() {
        let mut before = MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: "river-placement".to_owned(),
            asset_path: "gltf/river.glb".to_owned(),
            asset_target: Default::default(), // 水绑定只适用于完整Scene0。
            building_preset_id: None,         // 普通含水夹具不声明建筑身份。
            courtyard: None,                  // 独立摆放记录不属于任何院落副本。
            translation: [0.0, 0.0, 0.0],
            height_mode: feathrise_map_content::MapPlacementHeightMode::FixedHeight, // 水历史测试不引入地形派生变化。
            terrain_offset: 0.0, // 固定高度不携带离地偏移。
            yaw_degrees: 0.0,
            scale: [1.0, 1.0, 1.0],
            water_surfaces: Vec::new(),
        };
        let mut after = before.clone();
        after
            .water_surfaces
            .push(feathrise_map_content::MapPlacementWaterSurface {
                mesh_index: 0,
                node_path: vec!["河面对象".to_owned()],
                mesh_name: "河面网格".to_owned(),
                kind: feathrise_map_content::MapPlacementWaterKind::Horizontal,
                flow_speed: 2.0,
                height_offset: 0.0,
            });
        before.id.shrink_to_fit(); // 让测试不依赖String额外预留容量。
        let usage = water_history_usage(&[EditorCommand::PlacementChanged {
            record_index: 0,
            before,
            after,
        }]);
        assert_eq!(usage.commands, 1);
        assert!(usage.asset_bytes > 0); // 路径、网格名和水体数组都必须进入统一字节预算。
    }

    #[test]
    fn pure_placement_transform_does_not_consume_water_history_budget() {
        let before = MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: "ordinary-placement".to_owned(), // 普通资产没有任何整网格水绑定。
            asset_path: "gltf/house.glb".to_owned(), // 来源路径仍属于完整撤销快照。
            asset_target: Default::default(),    // 纯变换历史使用完整Scene0基线。
            building_preset_id: None,            // 普通房屋路径不自动推断为建筑预设。
            courtyard: None,                     // 独立摆放记录不属于任何院落副本。
            translation: [0.0, 0.0, 0.0],        // 变换起点位于世界原点。
            height_mode: feathrise_map_content::MapPlacementHeightMode::FixedHeight, // 纯变换测试使用确定世界Y。
            terrain_offset: 0.0,        // 固定高度不携带离地偏移。
            yaw_degrees: 0.0,           // 起点没有旋转。
            scale: [1.0, 1.0, 1.0],     // 起点使用单位缩放。
            water_surfaces: Vec::new(), // 关键前提：纯显式场景资产。
        };
        let mut after = before.clone(); // 只改变位置，其他记录内容保持一致。
        after.translation = [2.0, 3.0, 4.0]; // 平移仍应进入普通小型作者历史。
        let command = EditorCommand::PlacementChanged {
            record_index: 0, // 测试唯一记录槽。
            before,          // 完整变换起点。
            after,           // 完整变换终点。
        };

        assert!(!command_uses_bounded_water_history(&command)); // 纯变换不能冻结水交接或挤掉水历史。
        assert_eq!(
            water_history_usage(&[command]),
            WaterHistoryUsage::default()
        ); // 四项水预算都必须保持零。
    }

    #[test]
    fn placement_structure_commands_with_water_use_the_same_budget() {
        let mut record = MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: "water-placement-copy".to_owned(), // 模拟复制或删除的完整稳定资产。
            asset_path: "gltf/river.glb".to_owned(), // 来源路径由结构历史持有。
            asset_target: Default::default(),      // 含水复制只能使用Scene0层级。
            building_preset_id: None,              // 含水结构夹具仍是普通显式摆放。
            courtyard: None,                       // 独立摆放记录不属于任何院落副本。
            translation: [0.0, 0.0, 0.0],          // 合法默认位置。
            height_mode: feathrise_map_content::MapPlacementHeightMode::FixedHeight, // 结构预算测试不依赖R16。
            terrain_offset: 0.0,        // 固定高度不携带离地偏移。
            yaw_degrees: 0.0,           // 合法默认角度。
            scale: [1.0, 1.0, 1.0],     // 合法默认缩放。
            water_surfaces: Vec::new(), // 下方加入一项正式水语义。
        };
        record
            .water_surfaces
            .push(feathrise_map_content::MapPlacementWaterSurface {
                mesh_index: 0,                                            // 只作诊断的导出索引。
                node_path: vec!["河面对象".to_owned()],                   // 稳定Blender节点路径。
                mesh_name: "河面网格".to_owned(),                         // 稳定网格名称。
                kind: feathrise_map_content::MapPlacementWaterKind::Horizontal, // 合法水平水语义。
                flow_speed: 2.0,                                          // 有限流速。
                height_offset: 0.0,                                       // 无额外世界Y偏移。
            });
        let usage = water_history_usage(&[
            EditorCommand::PlacementAdded(record.clone()), // 复制完整含水记录。
            EditorCommand::PlacementRemoved {
                index: 0,                  // 删除前原数组位置。
                record,                    // 删除同样持有完整含水记录。
                had_author_preview: false, // 预览来源不改变内存分类。
            },
        ]);

        assert_eq!(usage.commands, 2); // 复制和删除都必须进入统一水命令计数。
        assert!(usage.asset_bytes > 0); // 两份完整节点路径快照必须被字节门覆盖。
    }

    #[test]
    fn courtyard_batch_counts_all_member_snapshots_as_one_bounded_command() {
        let member: MapPlacementRecord = serde_json::from_str(
            r#"{
            "id":"courtyard-house", "asset_path":"gltf/house.glb",
            "translation":[0,0,0], "yaw_degrees":0, "scale":[1,1,1],
            "courtyard":{"instance_id":"courtyard-1","preset_id":"small-court",
                "member_id":"main-house","origin":[0,0,0],"yaw_degrees":0}
        }"#,
        )
        .expect("院落成员测试记录合法"); // 真实JSON同时覆盖可选院落身份。
        let change =
            CourtyardBatchChange::new(vec![member], Vec::new()).expect("单成员落印可进入历史"); // 构造时冻结容量计费。
        let command = EditorCommand::CourtyardBatch(Box::new(change)); // 整批只占一次Ctrl+Z。
        assert!(command_uses_bounded_water_history(&command)); // 院落批量快照受现有历史硬门约束。
        let usage = water_history_usage(&[command]); // 历史裁剪不逐成员重复扫描字符串。
        assert_eq!(usage.commands, 1); // 单次印章只有一条可撤销事务。
        assert!(usage.asset_bytes > std::mem::size_of::<MapPlacementRecord>()); // Vec槽与成员字符串均计入字节预算。
    }
}
