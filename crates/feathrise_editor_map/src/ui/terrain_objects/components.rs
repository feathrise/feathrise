//! 网格页、场景列表和数值目标的明确身份；固定行池不限制作者对象数量。
use bevy::prelude::*;

#[derive(Component, Clone, Copy, Default)]
pub(super) struct MeshPageRoot;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct MeshSceneListRoot;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct ListViewport;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct ListContent;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct Summary;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct ListSummary;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct CreationSummary;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct CreationFeedback;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct SourceSummary;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct LodSummary;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct NativeLodSummary(pub bool);
/// 两组各有独立详细编辑树；用途随实体固定，展开另一组不能迁走本组输入。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct NativeLodEditor(pub bool);
#[derive(Component, Clone, Copy, Default)]
pub(super) struct RepresentationSummary;
/// 完整高度的LOD占位容器，沿用右侧属性栏的纵向滚动，不建立第二层滚动或分页。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct LodList(pub bool);
/// 固定可见行池每行常驻倍率与范围两数值根，直接编辑而不随作者条目数量增殖。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct LodRow(pub usize, pub bool);
#[derive(Component, Clone, Copy, Default)]
pub(super) struct LodRowText(pub usize, pub LodColumn, pub bool);
#[derive(Clone, Copy, Default)]
pub(super) enum LodColumn {
    #[default]
    Layer,
    Multiplier,
    Range,
}
/// 只有上次实际呈现的完整层身份可响应点击，滚动换绑不能把旧Pressed改派到新层。
#[derive(Component, Clone, Default)]
pub(super) struct LodRowBinding(pub Option<super::types::LodRowTarget>);
/// 增删动作同样冻结上次呈现的地图、作者修订、网格、用途与选层，旧点击不能借新状态改派。
#[derive(Component, Clone, Default)]
pub(super) struct LodActionBinding(pub Option<super::types::LodRowTarget>);
/// 数值根关联其真实行实体；首次接管必须冻结该行上次呈现的完整身份。
#[derive(Component, Clone, Copy)]
pub(super) struct LodFieldRow(pub Entity);
/// Clipmap末显示层在原范围单元格显示只读边界，不在表格下另加说明行。
#[derive(Component, Clone, Copy, Default)]
pub(super) struct LodValueFallback(pub Field);
#[derive(Component, Clone, Copy, Default)]
pub(super) struct PanelSection(pub Section);
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Section {
    #[default]
    Creation,
    CreationCuboid,
    Selection,
    Basic,
    Type,
    NativeLods,
    PolicyLods,
    DisplayOverride,
    CollisionOverride,
    DisplayLods,
    CollisionLods,
    Source,
    Stitch,
}
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) enum MeshPropertySection {
    #[default]
    Basic,
    Type,
    DisplayLods,
    CollisionLods,
    Source,
    Stitch,
}
#[derive(Component, Clone, Copy, Default)]
pub(super) struct Row(pub usize);
#[derive(Component, Clone, Copy, Default)]
pub(super) struct RowText(pub usize);
#[derive(Component, Clone, Copy, Default)]
pub(super) struct RowKind(pub usize);
/// 虚拟行按最后呈现的地图、修订及稳定对象冻结点击目标，滚动换绑不能误操作邻行。
#[derive(Component, Clone, Default)]
pub(super) struct RowAction {
    pub slot: usize,
    pub target: Option<super::types::RowTarget>,
}
#[derive(Component, Clone, Copy, Default)]
pub(super) struct RowIcon {
    pub slot: usize,
    pub action: Action,
}
#[derive(Component, Clone, Copy, Default)]
pub(super) struct Slot(pub Field);
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    /// 创建规格始终只影响下一次添加，已有选择不接管这些控件。
    Creation(usize),
    CuboidCreation(usize),
    /// 0～7为所选对象字段；LOD使用独立用途字段，不能借所选对象下标获得写权限。
    Selected(usize),
    /// 三轴旋转独立于旧0～7字段与8起的LOD槽；轴0/1/2分别为X/Y/Z。
    Rotation(usize),
    /// 用途固定在数字根上；8起每两个值对应本组虚拟行槽，真实层由LodRowBinding冻结。
    Lod(bool, usize),
    /// 源高度域以本地米计量，体素实高以世界米计量，均不借用基本页的无量纲缩放槽。
    AuthorHeightDomain,
    VoxelHeight,
    /// 通用网格策略使用独立字段，不与后端原生LOD行号重叠。
    Policy(usize),
}
impl Default for Field {
    fn default() -> Self {
        Self::Creation(0)
    }
}
impl Field {
    pub(super) fn index(self) -> usize {
        match self {
            Self::Creation(index) | Self::CuboidCreation(index) | Self::Selected(index) | Self::Rotation(index) | Self::Policy(index) | Self::Lod(_, index) => index,
            Self::AuthorHeightDomain | Self::VoxelHeight => 4,
        }
    }
    pub(super) fn is_creation(self) -> bool {
        matches!(self, Self::Creation(_) | Self::CuboidCreation(_))
    }
}
#[derive(Component, Default)]
pub(super) struct NumericFieldBinding {
    pub current: Option<super::types::NumericTarget>,
    /// 首次聚焦/拖动冻结原目标；换选择、行绑定或页面后不把旧提交改发给新目标。
    pub editing: Option<super::types::NumericTarget>,
    pub was_active: bool,
    /// 取消、拒收和已提交都关闭本笔授权；下一次真实聚焦/拖动边沿才可重新获得权限。
    pub invalidated: bool,
}
#[derive(Component, Clone, Copy, Default)]
pub(super) struct NameInput;
#[derive(Component, Clone, Copy, Default)]
pub(super) struct ActionButton(pub Action);
#[derive(Component, Clone, Copy, Default)]
pub(super) struct ActionLabel(pub Action);
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub(super) enum Action {
    StitchNearest,
    ConfirmStitch,
    CancelStitch,
    Unstitch,
    #[default]
    Glb,
    ImportGlb,
    Clipmap,
    Voxel,
    Cuboid,
    PlacementViewport,
    PlacementCoordinates,
    StartCreation,
    CancelPlacement,
    CuboidCollision,
    Remove,
    DisplayLods,
    CollisionLods,
    SelectLod(usize, bool),
    AddLod(bool),
    RemoveLod(bool),
    Enabled,
    Visible,
    Lock,
    Rename,
    ToggleCreation,
    BasicProperties,
    TypeProperties,
    SourceProperties,
    StitchProperties,
    DisplayPolicy,
    CollisionPolicy,
}
impl Action {
    pub(super) fn creation_action(self) -> bool {
        matches!(self, Self::ToggleCreation | Self::Glb | Self::Clipmap | Self::Voxel | Self::ImportGlb
            | Self::Cuboid | Self::PlacementViewport | Self::PlacementCoordinates | Self::StartCreation
            | Self::CancelPlacement | Self::CuboidCollision)
    }
    /// 行内开关与整网格删除跟随场景列表，不依赖下方当前属性页。
    pub(super) fn scene_list_action(self) -> bool {
        matches!(self, Self::Enabled | Self::Visible | Self::Lock)
    }
}
