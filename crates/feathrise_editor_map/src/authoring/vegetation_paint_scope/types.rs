//! 跨网格笔划的冻结输入、纯数据完成值和地图级历史快照。
use super::super::{
    terrain_documents::{TerrainCanvas, TerrainDocumentIdentity},
    terrain_ground::{CursorIdentity, TerrainGroundSnapshot},
    vegetation_compile::{CompiledDensityImage, CompiledDensityStats},
};
use bevy::prelude::*;
use feathrise_map_content::{MapId, MapVegetationAsset, MapVegetationPaintGroup, TerrainId};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum PaintScope {
    #[default]
    Current,
    Cross,
}

/// 只持有植被域，历史应用不能覆盖后来移动的姿态、地表或雕刻结果。
#[derive(Clone, Default)]
pub(crate) struct VegetationValues {
    pub definition: Option<MapVegetationAsset>,
    pub authors: BTreeMap<String, Arc<Vec<u8>>>,
    pub outputs: BTreeMap<String, Arc<Vec<u8>>>,
    pub nonzero: BTreeSet<String>,
    pub stats: BTreeMap<String, CompiledDensityStats>,
    pub conflicts: BTreeMap<String, CompiledDensityImage>,
}

#[derive(Clone)]
/// 外部作者只携带该历史值；内容和创建仍由普通编辑器自己的受检事务掌握。
pub struct VegetationPaintChange {
    pub(crate) before_groups: Arc<[MapVegetationPaintGroup]>,
    pub(crate) after_groups: Arc<[MapVegetationPaintGroup]>,
    pub(crate) canvases: BTreeMap<TerrainId, (VegetationValues, VegetationValues)>,
}
impl VegetationPaintChange {
    /// 完整R8历史共用现有预算，按Arc身份计费，不在帧路径扫描像素。
    pub(crate) fn buffers(&self) -> impl Iterator<Item = &Arc<Vec<u8>>> {
        self.canvases
            .values()
            .flat_map(|(a, b)| [a, b])
            .flat_map(|v| {
                v.authors
                    .values()
                    .chain(v.outputs.values())
                    .chain(v.conflicts.values().map(|c| &c.pixels))
            })
    }
}

pub(super) struct Target {
    pub identity: TerrainDocumentIdentity,
    pub cached: Option<Arc<TerrainCanvas>>,
    pub saved: bool,
}
#[derive(Clone)]
pub(super) struct Dab {
    pub from: Vec3,
    pub to: Vec3,
    pub radius: f32,
    pub amount: f32,
    pub exposure: f32,
    pub erase: bool,
    pub camera: Mat4,
    pub orthographic: bool,
}
#[derive(Clone)]
pub(super) struct Transaction {
    pub map: MapId,
    pub group: MapVegetationPaintGroup,
    pub groups_before: Arc<[MapVegetationPaintGroup]>,
    pub documents: BTreeMap<TerrainId, Arc<TerrainCanvas>>,
    pub before: BTreeMap<TerrainId, VegetationValues>,
    pub changed: BTreeSet<TerrainId>,
    pub ground: Arc<TerrainGroundSnapshot>,
    pub hidden: Arc<BTreeSet<TerrainId>>,
}
pub(super) struct Press {
    pub identity: CursorIdentity,
    pub authority: u64,
    pub group: String,
    pub radius: f32,
    pub strength: f32,
    pub erase: bool,
    pub groups: Arc<[MapVegetationPaintGroup]>,
    pub objects: Arc<[feathrise_map_content::MapTerrainObject]>,
    pub hidden: Arc<BTreeSet<TerrainId>>,
    pub locked: BTreeSet<TerrainId>,
}
pub(super) struct PreparedCanvas {
    pub id: TerrainId,
    pub definition: MapVegetationAsset,
    pub images: VecDeque<(String, Image)>,
    pub handles: BTreeMap<String, Handle<Image>>,
    pub sources: BTreeMap<String, Arc<[u8]>>,
    pub nonzero: BTreeSet<String>,
}
pub(super) struct Batch {
    pub transaction: Transaction,
    pub prepared: Vec<PreparedCanvas>,
    pub waiting: VecDeque<Dab>, // 同一作者源的高度页未就绪时保留整批刷段，不能漏掉跨缝半笔。
    pub remaining: VecDeque<Dab>, // 已处理固定前缀之后的原轨迹，采用后继续，不稀释成长间隔圆印。
}
pub(super) enum SurfaceEligibility {
    Paint,
    Skip,
    Waiting,
    Failed,
}
