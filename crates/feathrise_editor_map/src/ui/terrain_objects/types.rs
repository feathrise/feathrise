//! 对象后台事务的输入、完整结果与采用身份；资源和系统分别负责会话状态与执行流程。

use bevy::tasks::Task;
use feathrise_map_content::{
    MapId, MapTerrainCollection, MapTerrainKind, MapTerrainObject, TerrainId,
    TerrainObjectTransform,
};
use std::sync::Arc;

/// 创建面板的来源与放置方式只影响下一次添加，不借用当前所选对象的类型。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CreationSource {
    #[default]
    Cuboid,
    ImportedGlb,
    Heightfield,
    Clipmap,
    Voxel,
}
impl CreationSource {
    pub(super) fn kind(self) -> Option<MapTerrainKind> {
        Some(match self {
            Self::Cuboid => return None,
            Self::ImportedGlb => MapTerrainKind::Gltf,
            Self::Heightfield => MapTerrainKind::Heightfield,
            Self::Clipmap => MapTerrainKind::SparseClipmap,
            Self::Voxel => MapTerrainKind::Voxel,
        })
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CreationPlacementMode {
    #[default]
    Viewport,
    Coordinates,
}

/// 固定行池的可操作目标；不把当前选择或重排后的行号当成上次显示的对象。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RowTarget {
    pub map: MapId,
    pub revision: u64,
    pub terrain: TerrainId,
    pub index: usize,
}

/// 连续LOD虚拟行冻结已呈现用途与真实下标，不按新滚动窗口重新解释点击。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LodRowTarget {
    pub map: MapId,
    pub revision: u64,
    pub terrain: TerrainId,
    pub index: usize,
    pub display: bool,
}

/// 数字根实体保持稳定，但一次编辑的完整目标不能随列表选择和LOD用途重新解释。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct NumericTarget {
    pub purpose: super::components::Field,
    pub map: MapId,
    pub revision: u64,
    pub terrain: Option<TerrainId>,
    pub index: usize,
    /// 来自Field::Lod与已呈现行的同一固定用途，不能从最后聚焦组推断。
    pub display_lods: bool,
    /// 来源或放置用途切换后旧输入失效，即使地图和作者修订尚未变化。
    pub creation_scope: Option<(CreationSource, CreationPlacementMode)>,
}

pub(crate) enum Mutation {
    StitchNearest(TerrainId, std::collections::BTreeSet<TerrainId>),
    Unstitch(TerrainId),
    Enabled(TerrainId, bool),
    Rename(TerrainId, String),
    LodPolicy(TerrainId, Option<feathrise_map_content::MeshLodPolicy>),
    Brush(
        TerrainId,
        crate::authoring::terrain_editing::ObjectBrushInput,
    ),
    /// 独立体素的材质、种植和积水沿用同一来源版本与整笔历史，不开第二个IO槽。
    Field(
        TerrainId,
        crate::authoring::terrain_editing::NativeFieldInput,
    ),
    Transform(TerrainId, TerrainObjectTransform),
    /// 只调整可雕刻高度域；冻结当前作者网格，后台不得以磁盘源覆盖未保存顶点。
    HeightDomain(TerrainId, f32, Arc<feathrise_map_terrain::TerrainMeshInput>),
    ImportGlb(String),
    Add(MapTerrainKind),
    Remove(TerrainId),
    Value(TerrainId, usize, f32, bool),
    AddLod(TerrainId, bool),
    /// 删除提交时冻结的真实层下标，不把列表选中项默认为末层。
    RemoveLod(TerrainId, bool, usize),
}

pub(super) struct Prepared {
    pub stitch_bytes:usize,
    /// 相等性比较只在worker进行，主线程不扫描整个对象列表。
    pub changed: bool,
    pub objects: Arc<[MapTerrainObject]>,
    pub selected: Option<TerrainId>,
    pub selected_index: Option<usize>,
}

pub(super) enum LodNavigation {
    Added { display: bool },
    Removed { display: bool, index: usize },
}

pub(super) struct Pending {
    pub preview_stitch:bool,
    pub stitching:bool,
    pub stroke: Option<u64>,
    /// 会话锁定可使在途结果失效；名称修改不重建几何、碰撞与作者画布。
    pub target: Option<TerrainId>,
    pub affects_terrain: bool,
    /// 完整事务成功后才调整选层；失败保留作者当前视图。
    pub lod_navigation: Option<LodNavigation>,
    pub map: MapId,
    pub revision: u64,
    pub before: MapTerrainCollection,
    pub task: Task<Result<Prepared, String>>,
}

pub(super) struct StitchPreview {pub pending:Pending,pub prepared:Prepared}
