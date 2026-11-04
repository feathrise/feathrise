//! 中央多标签工作区的字段路径、按钮动作与 ECS 标记。

use bevy::prelude::*;
pub(crate) use feathrise_editor_core::{PresetWorkspaceDynamicRoot, PresetWorkspaceTab}; // 共享标签身份由编辑器契约库唯一拥有。

/// 活动预设标签内部的编辑分区。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PresetEditorSection {
    /// 稳定身份摘要、名称、画布、散布、缩放和可选自由备注。
    #[default]
    Basics,
    /// Blade 当前所选候选窗口的完整参数。
    BladeLod,
    /// Blade 外观向量、包围、纹理与超远预算。
    BladeAppearance,
    /// Blade 屏幕空间 LOD 和常驻范围。
    BladeView,
    /// Blade 世界风或水流参数。
    BladeMotion,
    /// Mesh 当前距离 LOD 的完整范围与结构动作。
    MeshLod,
    /// Mesh 当前部件的资产、变换、材质、风和派生网格。
    MeshPart,
    /// Mesh 贴地与近场碰撞参数。
    MeshCollision,
}

/// Blade 单档作者字段；运行时级别只由档位结构事务维护，不提供手工编辑。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BladeLodField {
    Name,
    MeshPath,
    GridSide,
    Spacing,
    InnerRadius,
    OuterRadius,
    InnerFade,
    OuterFade,
    Seed,
    SnapCells,
}

/// Blade 包围规格字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BladeBoundsField {
    MaxHeight,
    HorizontalSweep,
    ReferenceHeight,
    RootPenetration,
}

/// Blade 八组四维外观参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BladeAppearanceGroup {
    Geometry,
    WidthFlexibility,
    Bending,
    AtlasUv,
    RootColor,
    TipColor,
    CoolTint,
    WarmTint,
}

/// Blade 视图 LOD 参数组；`component` 由字段路径选择组内分量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BladeViewGroup {
    PixelThresholds(usize), // 相邻细档的真实下标，覆盖全部七段并支持减少档数。
    CameraFadeDistances,
    ReferenceGrassHeight,
    FinestGridSpacing,
    ResidencyRadii,
    ResidencyMargin,
    ResidencyTransitionWidths,
}

/// 水流运动的可编辑标量；世界风没有额外目录标量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BladeMotionField {
    DirectionX,
    DirectionZ,
    Speed,
    TipStrength,
    SpatialScale,
}

/// Mesh 单档距离字段；稳定级别属于内部身份，沿用原值及新档分配规则。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MeshLodField {
    StartBegin,
    StartFull,
    EndBegin,
    EndHidden,
}

/// Mesh 单部件的直接字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MeshPartField {
    PartId,
    MeshPath,
    MaterialPath,
    TranslationX,
    TranslationY,
    TranslationZ,
    YawDegrees,
    ScaleX,
    ScaleY,
    ScaleZ,
    DerivationBudget,
    AlphaCutoff,
    WindRootHeight,
    WindTipHeight,
    WindStrength,
    WindFrequency,
}

/// 当前 Mesh 碰撞策略可见的数值字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CollisionField {
    CylinderRadius,
    CylinderHeight,
    CylinderCenterY,
    CuboidSizeX,
    CuboidSizeY,
    CuboidSizeZ,
    CuboidCenterX,
    CuboidCenterY,
    CuboidCenterZ,
    EnterDistance,
    ExitDistance,
}

/// 中央详细表单当前可接收键盘输入的强类型字段路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PresetField {
    Label,
    Notes, // 备注是自由多行文本，不再把仅追踪用途的作者来源展示成模型配置。
    ResolutionX,
    ResolutionY,
    Spacing,
    ScaleMin,
    ScaleMax,
    Seed,
    BladeBaseColorPath,
    BladeDensityGain,
    BladeUltraSweep,
    BladeBounds(BladeBoundsField),
    BladeLod {
        lod: usize,
        field: BladeLodField,
    },
    BladeAppearance {
        group: BladeAppearanceGroup,
        component: usize,
    },
    BladeView {
        group: BladeViewGroup,
        component: usize,
    },
    BladeMotion(BladeMotionField),
    MeshGroundEmbed,
    MeshLod {
        lod: usize,
        field: MeshLodField,
    },
    MeshPart {
        lod: usize,
        part: usize,
        field: MeshPartField,
    },
    Collision(CollisionField),
}

impl Default for PresetField {
    fn default() -> Self {
        Self::Label // 仅供组件默认构造；真实字段始终由工作区显式写入。
    }
}

impl PresetField {
    /// 路径、名称和备注允许中文 IME，其余字段只接受 ASCII 数字语法。
    pub(crate) const fn accepts_text(self) -> bool {
        matches!(
            self,
            Self::Label
                | Self::Notes // 备注和名称同样使用项目中文字体与IME通道。
                | Self::BladeBaseColorPath
                | Self::BladeLod {
                    field: BladeLodField::Name | BladeLodField::MeshPath,
                    ..
                }
                | Self::MeshPart {
                    field: MeshPartField::MeshPath | MeshPartField::MaterialPath,
                    ..
                }
        )
    }
}

/// 预设管理和中央工作区的结构化按钮动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PresetEditorAction {
    /// 选择左栏连续目录中的真实预设；绝对索引只用于当前已采用目录快照。
    SelectCatalogPreset(usize),
    CreateBlade,
    CreateMesh,
    EditSelected,
    DeleteSelected,
    ActivateMapTab,
    ActivatePresetTab(u64),
    ClosePresetTab(u64),
    SelectSection(PresetEditorSection),
    SelectLod(usize),
    SelectPart(usize),
    Focus(PresetField),
    AssetReference(AssetReferenceAction), // 路径行的定位、赋值与带票据子资产选择由专用输入系统处理。
    ApplySelectedAsset,
    AppendMeshLod,
    DuplicateMeshLod,
    RemoveMeshLod,
    AppendBladeLod,
    DuplicateBladeLod,
    RemoveBladeLod,
    DuplicateMeshPart,
    RemoveMeshPart,
    ToggleMotion,
    CycleCollision,
    CycleUltraBudget,
    CycleBillboard,
    CycleMeshDerivation,
    CycleAlphaMode,
    ToggleDoubleSided,
    ToggleShadow,
    ToggleWind,
    CycleWindPart,
    ToggleCollisionScale,
    Save,
    Cancel,
}

/// 单字段资产控件动作；每个子资产按钮携带不可复用的请求票据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssetReferenceAction {
    Locate(PresetField),                  // 只在右侧定位，不提交输入、不改草稿。
    UseSelected(PresetField),             // 明确把右侧资产赋给这一行。
    Choose { ticket: u64, index: usize }, // 选择已完成枚举中的真实子资产。
    Previous { ticket: u64 },             // 子资产列表固定可见行，更多项目仍可访问。
    Next { ticket: u64 },                 // 不对GLB网格或材质总数设置产品上限。
    Cancel { ticket: u64 },               // 放弃当前请求，原字段完整保留。
}

/// 详细页所属草稿身份；路径按钮从自己的祖先读取，不猜当前全局焦点。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct PresetReferenceOwner(pub(crate) u64);

impl Default for PresetEditorAction {
    fn default() -> Self {
        Self::ActivateMapTab // 模板默认值不会触发破坏性动作。
    }
}

/// 所有预设工作区按钮携带的局部动作。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PresetEditorButton(pub(crate) PresetEditorAction);

/// 左栏当前选择、稳定 ID、后端和 Asset 摘要文本。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PresetEditorSummaryText;

/// 左栏校验、加载、引用保护与保存结果文本。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PresetEditorStatusText;

/// 一个动态详细字段的文字。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PresetEditorFieldText(pub(crate) PresetField);

/// 一个动态详细字段的聚焦边框节点。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PresetEditorFieldButton(pub(crate) PresetField);

pub(crate) use feathrise_editor_core::PresetNotesScroll;

/// 中央永久标签条中动态内容的挂载点。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PresetWorkspaceTabHost;

/// 地图、文件与预设共用的顶部固定标签行，不随活动正文显隐。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PresetWorkspaceTabBar;

/// 永久地图视口页；仅活动标签为地图时参与布局。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PresetWorkspaceMapPage;

/// 预设详细编辑页；仅活动标签为预设时参与布局。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct PresetWorkspaceEditorPage;

/// 活动预设详细正文的动态挂载点。
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct PresetWorkspaceEditorHost;
