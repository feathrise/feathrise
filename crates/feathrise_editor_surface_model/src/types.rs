//! PBR 地表预设的草稿、动作、任务结果与固定回显缓存，不定义 ECS 组件或资源。

use feathrise_map_content::SurfacePresetDescriptor;

#[derive(Default)]
pub struct SurfaceListCache {
    pub initialized: bool,
    pub revision: u64,
}

#[derive(Default)]
pub struct SurfaceStyleCache {
    pub state_revision: u64,
    pub workspace_revision: u64,
    pub selected_id: Option<String>,
}

#[derive(Clone, Debug)]
pub enum SurfaceDraftMode {
    Create,
    Edit { original_id: String },
}

#[derive(Clone, Debug)]
pub struct SurfaceDraft {
    pub mode: SurfaceDraftMode,
    pub descriptor: SurfacePresetDescriptor,
    pub baseline: SurfacePresetDescriptor,
}

impl SurfaceDraft {
    pub fn new(mode: SurfaceDraftMode, descriptor: SurfacePresetDescriptor) -> Self {
        Self {
            baseline: descriptor.clone(),
            descriptor,
            mode,
        }
    }

    pub fn dirty(&self) -> bool {
        self.descriptor != self.baseline
    }
}

#[derive(Clone, Debug)]
pub struct SurfaceTab {
    pub id: u64,
    pub draft: SurfaceDraft,
    pub section: SurfaceSection, // 分区属于文档会话，不参与脏标记、目录或地图保存。
    pub close_confirmation: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SurfaceSection {
    #[default]
    Textures,
    Material,
    Mapping,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfacePresetField {
    Label,
    BaseColor(usize),
    TileMeters,
    NormalStrength,
    HeightBlendStrength,
    Roughness,
    Metallic,
    CraterResponse, // 与视觉PBR参数分离的技能地形形变响应。
}

impl SurfacePresetField {
    pub const fn accepts_text(self) -> bool {
        matches!(self, Self::Label)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceTextureField {
    BaseColor,
    Normal,
    ArmHeight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfacePresetAction {
    SelectPreset(usize),
    Create,
    EditSelected,
    DeleteSelected,
    ActivateTab(u64),
    CloseTab(u64),
    Section(SurfaceSection),
    Focus(SurfacePresetField),
    ApplySelectedAsset(SurfaceTextureField),
    ClearTexture(SurfaceTextureField),
    ToggleNormalY,
    CycleProjection,
    Save,
    Cancel,
}

#[derive(Debug)]
pub struct SurfaceCatalogWriteResult {
    pub baseline_revision: u64,
    pub preset_id: String,
    pub deleted: bool,
    pub tab_id: Option<u64>,
    pub message: String,
}

#[derive(Debug)]
pub struct SurfaceCatalogAdoption {
    pub baseline_revision: u64,
    pub preset_id: String,
    pub deleted: bool,
    pub tab_id: Option<u64>,
    pub message: String,
}
