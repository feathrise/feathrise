//! 建筑预设编辑器的强类型草稿、动作、后台任务和 UI 标记。

use bevy::prelude::*; // Component、Resource 与 Entity。
use bevy::tasks::Task; // 全局目录只允许一个后台写任务。
use feathrise_map_content::BuildingPresetDescriptor; // 草稿直接复用正式公共格式。

use crate::ui::vegetation_preset_editor::PresetWorkspaceTab; // 全部预设共享中央标签身份。

/// 新建或编辑模式；编辑始终冻结原稳定 ID。
#[derive(Clone, Debug)]
pub(super) enum BuildingDraftMode {
    Create,                       // 新资产尚未存在于磁盘目录。
    Edit { original_id: String }, // 只允许替换同一规范身份。
}

/// 一个中央标签持有的完整建筑语义草稿。
#[derive(Clone, Debug)]
pub(super) struct BuildingDraft {
    pub(super) mode: BuildingDraftMode, // 决定保存采用追加或原位替换。
    pub(super) descriptor: BuildingPresetDescriptor, // 当前可编辑值。
    pub(super) baseline: BuildingPresetDescriptor, // 最近一次热采用值。
    pub(super) section: BuildingPresetSection, // 每张文档独立记住分区，不属于正式预设数据。
    pub(super) selected_connection: usize, // 只生成当前连接点的字段，列表选择不改连接点身份。
}

impl BuildingDraft {
    /// 从正式描述建立独立草稿。
    pub(super) fn new(mode: BuildingDraftMode, descriptor: BuildingPresetDescriptor) -> Self {
        Self {
            baseline: descriptor.clone(), // 脏判断不依赖显示字符串格式。
            descriptor,                   // 当前标签拥有完整编辑副本。
            mode,                         // 保存时仍能定位原规范项。
            section: BuildingPresetSection::Basics,
            selected_connection: 0,
        }
    }

    /// 返回当前值是否偏离最近热采用基线。
    pub(super) fn dirty(&self) -> bool {
        self.descriptor != self.baseline // 公共数据结构提供完整强类型比较。
    }
}

/// 建筑作者文档的四个顶部分区；切页仍先走原输入提交和IME确认门。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BuildingPresetSection {
    #[default]
    Basics,
    Footprint,
    Entrance,
    Connections,
}

/// 工作台按钮的局部表现缓存；不扫描目录或复制草稿，稳定帧直接走Changed查询。
#[derive(Default)]
pub(super) struct BuildingWorkbenchStyleCache {
    pub(super) initialized: bool,
    pub(super) ui_revision: u64,
    pub(super) workspace_revision: u64,
    pub(super) active_field: Option<BuildingPresetField>,
}

/// 一个可关闭的建筑预设标签。
#[derive(Clone, Debug)]
pub(super) struct BuildingTab {
    pub(super) id: u64,                  // 本次 F12 会话内稳定，不写磁盘。
    pub(super) draft: BuildingDraft,     // 标签独占草稿。
    pub(super) close_confirmation: bool, // 脏标签需要第二次关闭确认。
}

/// 中央表单可精确输入的文字或数值字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuildingPresetField {
    Label,                 // 中文显示名称。
    ConnectionId(usize),   // 连接点在预设内的稳定技术ID。
    ConnectionKind(usize), // 完全相同才兼容的显式类型。
    ConnectionX(usize),    // 地基平面局部X位置。
    ConnectionZ(usize),    // 地基平面局部Z位置。
    ConnectionYaw(usize),  // 局部朝外九十度档位。
    FootprintCenterX,      // 占地局部中心 X。
    FootprintCenterZ,      // 占地局部中心 Z。
    FootprintHalfX,        // 占地半宽。
    FootprintHalfZ,        // 占地半深。
    EntranceX,             // 门槛局部 X。
    EntranceY,             // 门槛局部 Y。
    EntranceZ,             // 门槛局部 Z。
    EntranceYaw,           // 门口相对局部 +Z 的朝外角。
    EntranceHalfWidth,     // 门前净空横向半尺寸。
    EntranceHalfDepth,     // 门前净空纵向半尺寸。
    FoundationBaseY,       // 模型地基基准 Y。
    FoundationMaxDelta,    // 五点地形最大允许坡差。
    VegetationCenterX,     // 植被净空中心 X。
    VegetationCenterZ,     // 植被净空中心 Z。
    VegetationHalfX,       // 植被净空半宽。
    VegetationHalfZ,       // 植被净空半深。
}

impl BuildingPresetField {
    /// 返回字段是否交给官方可编辑文字组件；数值仍使用有界ASCII缓冲。
    pub(super) const fn accepts_text(self) -> bool {
        matches!(
            self,
            Self::Label | Self::ConnectionId(_) | Self::ConnectionKind(_)
        ) // 其余字段使用有限浮点语法。
    }

    /// 只有面向作者的中文显示名称启用输入法；技术ID保持ASCII。
    pub(super) const fn accepts_ime(self) -> bool {
        matches!(self, Self::Label) // 连接点ID和类型使用跨地图稳定技术字符集。
    }
}

/// 建筑目录、标签、字段和摆放控制的结构化动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuildingPresetAction {
    SelectPreset(usize),          // 三列目录绝对索引。
    CreateFromSelectedAsset,      // 从右侧当前 GLB/GLTF 建立草稿。
    EditSelected,                 // 打开当前规范项草稿。
    DeleteSelected,               // 两次点击后执行引用保护删除。
    ActivateTab(u64),             // 切换中央活动标签。
    CloseTab(u64),                // 关闭或确认丢弃标签。
    Focus(BuildingPresetField),   // 聚焦一个表单字段。
    SelectSection(BuildingPresetSection), // 顶部导航只改变当前文档的会话视图。
    SelectConnection(usize), // 选择一个仍存在的连接点，不改其ID或数据。
    CycleCategory,                // 循环作者浏览分类。
    CycleInterior,                // 循环室内可达声明。
    PlaceSelected,                // 切换到专用建筑摆放工具。
    RotateLeft,                   // 摆放朝向左转九十度。
    RotateRight,                  // 摆放朝向右转九十度。
    CycleGrid,                    // 循环关闭、0.5、1、2、4 米吸附。
    ToggleConnectionSnap,         // 显式开启或关闭模块连接点吸附。
    ToggleTerrainLeveling,        // 显式开关建筑与真实地形联合整平事务。
    AddConnectionPoint,           // 向当前预设草稿追加一个有界默认连接点。
    RemoveConnectionPoint(usize), // 删除当前草稿中的指定连接点。
    Save,                         // 原子保存当前标签。
    Reset,                        // 恢复最近热采用基线。
    Cancel,                       // 关闭当前标签。
}

/// 所有建筑按钮携带自己的领域动作，不扩大全局地图动作枚举。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct BuildingPresetButton(pub(crate) BuildingPresetAction);

impl Default for BuildingPresetButton {
    fn default() -> Self {
        Self(BuildingPresetAction::PlaceSelected) // BSN 模板默认值不会承担业务分派。
    }
}

/// 三列动态目录挂载点。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct BuildingPresetList;

/// 目录总数、同步进度或错误摘要。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct BuildingPresetListTitle;

/// 一个动态预设按钮绑定的绝对目录索引。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct BuildingPresetRow {
    pub(crate) index: usize, // 热更完成前容器隐藏，避免旧索引点击新项目。
}

/// 当前选择、类别和模型来源摘要。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct BuildingPresetSummaryText;

/// 目录任务与具体错误状态。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct BuildingPresetStatusText;

/// 统一工具反馈区的当前模式与旋转摘要；参数值直接回显在原动作按钮上。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct BuildingPlacementControlText;

/// 文件事务成功后的轻量完成值。
#[derive(Debug)]
pub(super) struct BuildingCatalogWriteResult {
    pub(super) baseline_revision: u64, // 发布前运行时目录修订。
    pub(super) preset_id: String,      // 目标规范身份。
    pub(super) deleted: bool,          // 采用后应存在还是消失。
    pub(super) tab_id: Option<u64>,    // 保存草稿需要回写基线。
    pub(super) message: String,        // 用户可见成功摘要。
}

/// 文件已替换、等待 AssetServer 完整采用的状态。
#[derive(Debug)]
pub(super) struct BuildingCatalogAdoption {
    pub(super) baseline_revision: u64, // 必须观察到更高目录修订。
    pub(super) preset_id: String,      // 核对最终存在性。
    pub(super) deleted: bool,          // 删除与保存使用相反存在条件。
    pub(super) tab_id: Option<u64>,    // 保存后更新对应标签。
    pub(super) message: String,        // 完整采用后才显示成功。
}

/// 建筑预设编辑器的唯一作者会话。
#[derive(Resource)]
pub(crate) struct BuildingPresetEditorState {
    pub(super) tabs: Vec<BuildingTab>, // 打开顺序就是共享标签顺序。
    pub(super) next_tab_id: u64,       // 零号保留给未初始化。
    pub(super) ui_revision: u64,       // 动态中央子树只在显式变化时重建。
    pub(super) delete_confirmation_id: Option<String>, // 两次删除必须指向同一规范项。
    pub(super) active_field: Option<BuildingPresetField>, // 当前唯一字符输入字段。
    pub(super) input_buffer: String,   // 已确认文字或 ASCII 数值。
    pub(super) write_task: Option<Task<Result<BuildingCatalogWriteResult, String>>>, // 单在途 IO。
    pub(super) adoption: Option<BuildingCatalogAdoption>, // 文件成功后继续等待热采用。
    pub(super) status: String,         // 左栏和底栏共享明确反馈。
}

impl Default for BuildingPresetEditorState {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),             // 首次打开不伪造预设。
            next_tab_id: 1,               // 第一张真实草稿使用非零 ID。
            ui_revision: 1,               // 保证中央工作区首次同步。
            delete_confirmation_id: None, // 尚无危险动作。
            active_field: None,           // 尚无键盘输入归属。
            input_buffer: String::new(),  // 缓冲随焦点建立。
            write_task: None,             // 尚无文件任务。
            adoption: None,               // 尚无待热采用结果。
            status: String::new(), // 建筑预设教程集中到分组?帮助，初始状态不重复通知。
        }
    }
}

impl BuildingPresetEditorState {
    /// 只读原文件提交门；未保存草稿本身仍允许按磁盘版本试玩。
    pub(crate) fn saved_play_pending(&self) -> bool {
        self.write_task.is_some() || self.adoption.is_some()
    }

    /// 新建但尚未落盘的标签也属于作者现场，不能以初始值等于基线为由静默丢失。
    pub(crate) fn exit_blocker(&self) -> Option<&'static str> {
        if self.busy() { return Some("建筑预设写入或热采用尚未完成"); }
        if self.active_field.is_some() { return Some("建筑预设字段尚未完成，请先Enter提交或Esc取消"); }
        self.tabs.iter().any(|tab| matches!(tab.draft.mode, BuildingDraftMode::Create) || tab.draft.dirty())
            .then_some("存在未保存的建筑预设，请先保存或使用原标签关闭确认丢弃")
    }

    /// 中央文档还原前只检查打开标签，不读取或修改预设内容。
    pub(crate) fn has_tab(&self, id: u64) -> bool {
        self.tabs.iter().any(|tab| tab.id == id)
    }

    /// 快捷键与 IME 门查询当前是否存在建筑文字输入。
    pub(crate) fn text_input_active(&self) -> bool {
        self.active_field.is_some() // 数值字符同样阻止底层快捷键。
    }

    /// 目录写入或热采用期间拒绝覆盖另一个请求。
    pub(super) fn busy(&self) -> bool {
        self.write_task.is_some() || self.adoption.is_some() // 文件事务保持 single-flight。
    }

    /// 返回共享工作区当前活动的建筑草稿。
    pub(super) fn active_draft(&self, active: PresetWorkspaceTab) -> Option<&BuildingDraft> {
        let PresetWorkspaceTab::Building(id) = active else {
            return None; // 其他内容域不能借用建筑字段。
        };
        self.tabs
            .iter()
            .find(|tab| tab.id == id)
            .map(|tab| &tab.draft)
    }

    /// 返回共享工作区当前活动建筑草稿的可变引用。
    pub(super) fn active_draft_mut(
        &mut self,
        active: PresetWorkspaceTab,
    ) -> Option<&mut BuildingDraft> {
        let PresetWorkspaceTab::Building(id) = active else {
            return None; // 标签已经切换时旧输入失败关闭。
        };
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == id)
            .map(|tab| &mut tab.draft)
    }

    /// 按会话标签 ID 定位保存后的草稿。
    pub(super) fn draft_mut(&mut self, tab_id: u64) -> Option<&mut BuildingDraft> {
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == tab_id)
            .map(|tab| &mut tab.draft)
    }

    /// 打开草稿；同一正式预设只保留一个编辑标签。
    pub(super) fn open_draft(&mut self, draft: BuildingDraft) -> u64 {
        if let BuildingDraftMode::Edit { original_id } = &draft.mode
            && let Some(id) = self
                .tabs
                .iter()
                .find(|tab| matches!(&tab.draft.mode, BuildingDraftMode::Edit { original_id: current } if current == original_id))
                .map(|tab| tab.id)
        {
            return id; // 调用方只需把已有标签设为活动。
        }
        let id = self.next_tab_id; // 冻结本标签会话身份。
        self.next_tab_id = self.next_tab_id.wrapping_add(1).max(1); // 回绕时跳过零。
        self.tabs.push(BuildingTab {
            id,                        // 标签和动作共享同一 ID。
            draft,                     // 草稿所有权移入编辑器会话。
            close_confirmation: false, // 新打开不显示危险确认。
        });
        self.touch_ui(); // 标签条需要追加新项目。
        id // 调用方切换共享活动页。
    }

    /// 推进动态工作区显式修订。
    pub(super) fn touch_ui(&mut self) {
        self.ui_revision = self.ui_revision.wrapping_add(1).max(1); // 零值只属于未构造缓存。
    }

    /// 让建筑视口校验链把具体进度或失败原因回显到左栏状态卡。
    pub(crate) fn report_status(&mut self, value: impl Into<String>) {
        self.status = value.into(); // 目录与摆放共享同一作者可见反馈位置。
    }
}

/// 当前建筑点放的网格和朝向；关闭 F12 后仍可保留但不写项目文件。
#[derive(Resource, Clone, Debug, PartialEq)]
pub(crate) struct BuildingPlacementControls {
    pub(crate) yaw_degrees: f32, // 只允许九十度步进并规范化到零至三百六十。
    pub(crate) grid_step_meters: f32, // 零表示关闭吸附。
    pub(crate) connection_snap: bool, // 默认关闭；开启后必须找到兼容地面连接点。
    pub(crate) level_terrain: bool, // 默认关闭，禁止建筑落点暗中修改地形。
}

impl Default for BuildingPlacementControls {
    fn default() -> Self {
        Self {
            yaw_degrees: 0.0,       // 新建筑默认使用预设局部正门方向。
            grid_step_meters: 1.0,  // 首期默认一米城镇规划网格。
            connection_snap: false, // 普通建筑点放不能因新增连接数据而改变。
            level_terrain: false,   // 作者必须逐次显式开启单栋整平。
        }
    }
}

impl BuildingPlacementControls {
    /// 未常开统一吸附时保留建筑自己的网格优先级；Ctrl不会把两米建筑格改成普通一米格。
    pub(crate) fn snap_policy(&self, settings: feathrise_editor_core::EditorSnapSettings, control: bool, origin: [f32; 3]) -> feathrise_editor_core::EditorTranslationSnap {
        if !settings.enabled && self.grid_step_meters > 0.0 {
            feathrise_editor_core::EditorSnapSettings { enabled: true, mode: feathrise_editor_core::EditorSnapMode::WorldGrid, translation_meters: self.grid_step_meters, ..settings }.translation_snap(false, origin)
        } else { settings.translation_snap(control, origin) }
    }
    /// 把光标 X/Z 吸附到当前世界网格。
    pub(crate) fn snapped_xz(&self, world: Vec3) -> Vec2 {
        if self.grid_step_meters <= 0.0 {
            return Vec2::new(world.x, world.z); // 关闭网格时保留真实地形命中。
        }
        Vec2::new(
            (world.x / self.grid_step_meters).round() * self.grid_step_meters, // X 独立吸附。
            (world.z / self.grid_step_meters).round() * self.grid_step_meters, // Z 独立吸附。
        )
    }
}
