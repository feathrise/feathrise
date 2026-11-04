//! F12编辑器工具、视图、资产、虚拟地图文件夹和异步阶段普通类型。

use std::collections::BTreeMap; // 虚拟地图归属以稳定键顺序写入JSON，便于版本审阅。

use bevy::prelude::{Asset, TypePath, Vec3}; // 编辑器归类使用私有热更新Asset，观察快照只保留世界坐标。
use feathrise_map_content::{MapAssetKind, MapId, MapSkyMode, MapTerrainKind}; // 编辑器与运行时共享地图ID、天空模式、地形类型和资产用途。
use serde::{Deserialize, Serialize}; // 虚拟文件夹只写独立编辑器JSON，不改正式地图目录格式。

use crate::constants::MAP_FOLDER_SCHEMA_VERSION; // 空归类文件始终使用当前编辑器格式版本。

pub(crate) use feathrise_editor_core::{EditorStatusKind, EditorStatusOperation, EditorStatusTarget};

/// 每类操作只记录最近一次真实回执，不进入作者历史、配置或存档。
#[derive(Clone, Debug)]
pub(crate) struct EditorStatusResult {
    pub(crate) sequence: u64,
    pub(crate) operation: EditorStatusOperation,
    pub(crate) target: EditorStatusTarget,
    pub(crate) map_id: Option<MapId>,
    pub(crate) kind: EditorStatusKind,
    pub(crate) status_text: std::sync::Arc<str>,
    pub(crate) detail: std::sync::Arc<str>,
}

/// 设置动作只修改编辑器及玩法偏好，不进入作者事务。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EditorSettingsAction {
    #[default]
    Open,
    OpenShortcuts,
    Close,
    RebindShortcut(feathrise_editor_core::EditorShortcutAction),
    UnbindShortcut(feathrise_editor_core::EditorShortcutAction),
    ResetShortcuts,
    ResetSnapping,
    DecreaseScale,
    IncreaseScale,
    SetAutoScale,
    SetScale(u16),
    SetColorTheme(feathrise_editor_core::EditorColorTheme),
    ToggleTerrainBackfaces,
    ResetLayout,
}

/// 快捷键反馈按录入结果着色，不解析可能变更的中文提示，也不写入偏好文件。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EditorShortcutNoticeKind {
    #[default]
    Idle,
    Listening,
    Success,
    Warning,
}

/// 导航请求只改变现有编辑相机，不能制造作者脏位或撤销记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorViewportFraming {
    Selected,
    Map,
    RefreshDepth, // 恢复旧视角时仅更新实际内容的裁剪范围，保留用户镜头。
}

/// 固定两侧区域的布局身份；不引入任意停靠或新的作者数据。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EditorPane {
    #[default]
    Content,
    Inspector,
}

/// 顶栏布局操作与地图、预设保存动作相互独立。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EditorLayoutAction {
    #[default]
    Content,
    Inspector,
    Focus,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct EditorPaneDrag {
    pub(crate) pane: EditorPane,
    pub(crate) origin_x: f32,
    pub(crate) origin_width: f32,
    /// 未真正移动时保留首选宽度，避免单击把窄窗临时压缩写回偏好。
    pub(crate) moved: bool,
}

/// 实际可见宽度由窗口与 UI 缩放共同决定，不覆盖作者记忆的首选宽度。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EditorPaneLayout {
    pub(crate) content: Option<f32>,
    pub(crate) inspector: Option<f32>,
}

/// F12编辑器专用的虚拟文件夹记录；不会映射为真实磁盘目录。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MapFolderRecord {
    /// 重命名后仍保持不变的稳定文件夹身份。
    pub(crate) id: String,
    /// 左栏显示的作者可读名称。
    pub(crate) display_name: String,
    /// 父文件夹稳定ID；`None`表示直接位于虚拟根目录。
    pub(crate) parent_id: Option<String>,
}

/// `assets/maps/editor.map_folders.json`的完整编辑器归类文档。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MapFolderDocument {
    /// 不兼容格式升级必须显式提高版本并提供迁移。
    pub(crate) schema_version: u32,
    /// 全部虚拟文件夹；数组顺序不承担显示排序或层级身份。
    pub(crate) folders: Vec<MapFolderRecord>,
    /// 地图稳定ID到虚拟文件夹稳定ID的归属；缺少键表示根目录。
    pub(crate) map_folders: BTreeMap<String, String>,
}

impl Default for MapFolderDocument {
    fn default() -> Self {
        Self {
            schema_version: MAP_FOLDER_SCHEMA_VERSION, // 缺失文件按当前版本空归类处理。
            folders: Vec::new(),                       // 初始不自动创建任何分类。
            map_folders: BTreeMap::new(),              // 现有地图默认全部留在虚拟根目录。
        }
    }
}

/// AssetServer成功加载并严格校验后的编辑器归类资产。
#[derive(Asset, Clone, Debug, TypePath)]
pub(crate) struct MapFolderAsset {
    /// 可直接原子采用的完整归类文档。
    pub(crate) document: MapFolderDocument,
    /// 加载时的逐字节文件内容，用于后续写入前检测外部并发修改。
    pub(crate) source_bytes: Vec<u8>,
}

/// 左栏地图浏览器一行当前绑定的业务对象。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum MapBrowserItem {
    /// 固定根目录行；只用于会话选择和地图、文件夹拖回根目录，不写入归类文件。
    Root,
    /// 虚拟文件夹行；点击直接展开或收起其树分支。
    Folder(String),
    /// 正式目录地图行；显式进入仍复用原切图链。
    Map(MapId),
}

/// 地图目录多选只属于编辑器会话；浅共享给拖拽与后台连选，不复制地图或子树。
#[derive(Clone, Debug, Default)]
pub(crate) struct MapBrowserSelection {
    pub(crate) items: std::sync::Arc<std::collections::BTreeSet<MapBrowserItem>>,
    /// 与兼容单选字段绑定；创建、删除等旧入口改选后，多选自动失效。
    pub(crate) focus: Option<MapBrowserItem>,
    pub(crate) anchor: Option<MapBrowserItem>,
    pub(crate) maps: usize,
    pub(crate) folders: usize,
    pub(crate) revision: u64,
}

/// Shift按当前完整可见投影连选；唯一计算槽结束前不创建第二份范围任务。
#[derive(Default)]
pub(crate) struct MapBrowserSelectionWork {
    pub(crate) task: Option<bevy::tasks::Task<MapBrowserSelection>>,
    pub(crate) projection: Option<(MapBrowserIndexSignature, u64)>,
    pub(crate) start_physical: Option<bevy::math::Vec2>,
    pub(crate) cancelled: bool,
}

/// 批量整理先在后台准备完整结果，回到主线程核对身份后才进入原受检文件写入器。
pub(crate) enum MapFolderBatchPhase {
    Preparing(bevy::tasks::Task<Result<Option<MapFolderFileSnapshot>, String>>),
    Saving(bevy::tasks::Task<Result<MapFolderFileSnapshot, MapFolderWriteFailure>>),
}

pub(crate) struct MapFolderBatchMove {
    pub(crate) ticket: u64,
    pub(crate) source: MapBrowserIndexSignature,
    pub(crate) target: MapBrowserDropTarget,
    pub(crate) expected_bytes: Option<std::sync::Arc<Vec<u8>>>,
    /// 与源字节一起冻结作者根，后台任务不在完成帧重新猜测写入路径。
    pub(crate) asset_root: std::path::PathBuf,
    pub(crate) cancelled: bool,
    pub(crate) phase: MapFolderBatchPhase,
}

/// 后台整理完成后供滚动虚拟行池显示的一条轻量树记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MapBrowserEntry {
    /// 文件夹或地图的稳定身份。
    pub(crate) item: MapBrowserItem,
    /// 当前快照中的作者可读名称。
    pub(crate) display_name: String,
    /// 地图发布标记；文件夹始终使用`false`。
    pub(crate) published: bool,
    /// 相对固定根目录行的树深度；根目录直属项目从一开始。
    pub(crate) depth: usize,
    /// 该项目所在的直接虚拟文件夹；`None`表示根目录直属项目。
    pub(crate) parent_folder_id: Option<String>,
}

/// 地图浏览索引的完整输入身份；迟到后台结果必须逐字段匹配。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MapBrowserIndexSignature {
    /// 归类文档每次原子采用或乐观修改后递增的修订。
    pub(crate) folder_revision: u64,
    /// 公共地图目录每次完整采用后递增的修订。
    pub(crate) catalog_revision: u64,
}

/// 分帧收集目录输入时持有的小型构建草稿。
#[derive(Clone, Debug)]
pub(crate) struct MapBrowserIndexBuild {
    /// 本草稿对应的完整输入身份。
    pub(crate) signature: MapBrowserIndexSignature,
    /// 下一条待检查虚拟文件夹记录下标。
    pub(crate) folder_cursor: usize,
    /// 下一条待检查正式地图目录记录下标。
    pub(crate) map_cursor: usize,
    /// 已按受检归类文件顺序冻结的全部虚拟文件夹记录。
    pub(crate) folders: Vec<MapFolderRecord>,
    /// 已按catalog作者顺序冻结的全部地图行及其直接父目录。
    pub(crate) maps: Vec<MapBrowserEntry>,
}

/// 地图浏览器文件夹弹框当前执行的结构操作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MapFolderDialogMode {
    /// 在当前虚拟目录新建子文件夹。
    Create,
    /// 重命名当前高亮文件夹而不改变稳定ID。
    Rename,
    /// 二次确认删除当前高亮的空文件夹。
    Delete,
}

/// 文件夹模态框冻结的目标与字符输入状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MapFolderDialogState {
    /// 创建、重命名或删除三种互斥模式。
    pub(crate) mode: MapFolderDialogMode,
    /// 创建时冻结父目录ID，重命名和删除时冻结目标ID；根目录父级为`None`。
    pub(crate) target_folder_id: Option<String>,
    /// 已提交的Unicode文件夹名称或删除确认回显名称。
    pub(crate) name: String,
    // 未提交候选仅由官方EditableText维护。
}

/// 拖拽松开时可采用的虚拟目录目标。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MapBrowserDropTarget {
    /// 拖入树中当前可见的任意虚拟文件夹。
    Folder(String),
    /// 拖到固定根目录行，清除地图归属或文件夹父级并移回虚拟根目录。
    Root,
}

/// 一次地图或文件夹行左键按下冻结的拖拽起点。
#[derive(Clone, Debug)]
pub(crate) struct MapBrowserDragPress {
    /// 冻结地图或文件夹稳定身份；固定根目录不能作为拖动源。
    pub(crate) item: MapBrowserItem,
    /// 外部归类换代后丢弃旧手势，不能把同名或已移动对象当成原目标。
    pub(crate) document_revision: u64,
    /// 浮层显示点击时冻结的中文名称。
    pub(crate) display_name: String,
    /// 按下位置换算后的物理像素坐标。
    pub(crate) start_physical: bevy::math::Vec2,
}

/// 拖拽悬停展开只保留一个短身份和真实时间，不缓存目录或建立后台队列。
pub(crate) struct MapBrowserHoverExpand {
    pub(crate) source: MapBrowserItem,
    pub(crate) document_revision: u64,
    pub(crate) folder_id: String,
    pub(crate) since: f64,
}

/// 文件事务成功后返回给主线程的完整归类文件快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MapFolderFileSnapshot {
    /// 已通过全部结构约束的文档。
    pub(crate) document: MapFolderDocument,
    /// 已原子发布的精确UTF-8字节。
    pub(crate) source_bytes: Vec<u8>,
}

/// 归类文件写入失败类别；并发变化与普通IO失败采用不同恢复策略。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MapFolderWriteFailure {
    /// 目标字节不再等于任务启动时的原始快照，禁止覆盖外部修改。
    ConcurrentChange(String),
    /// 序列化、验证或原子写入失败，内存回滚后仍可重试。
    Io(String),
}

impl std::fmt::Display for MapFolderWriteFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConcurrentChange(message) | Self::Io(message) => formatter.write_str(message), // 用户状态栏显示具体失败原因。
        }
    }
}

/// 编辑器当前笔刷或放置工具。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorTool {
    /// 应用明确装配的工具身份；普通编辑器不登记原游戏领域。
    Domain(&'static str),
    /// 只平移、旋转、缩放视图，不修改地图。
    #[default]
    Inspect,
    /// 在地形上抬高高度。
    Raise,
    /// 在地形上降低高度。
    Lower,
    /// 对邻域高度执行可撤销平滑。
    Smooth,
    /// 把笔刷范围压到首次按下处的高度。
    Flatten,
    /// 局部体积原型：改变岩土/空气密度，自动生成洞口和真实三维表面。
    VolumeDig,
    VolumeFill,
    VolumeSmooth,
    /// 在一次按压中锁定首次落点高度，削高补低形成洞底水平面。
    VolumeFlatten,
    /// 编辑独立R8湖泊/池塘覆盖；水面由正式GPU波浪材质实时预览。
    Water,
    /// 逐点创建和编辑具有连续高程的河流样条；不得复用静水R8笔刷输入。
    RiverSpline,
    /// 逐点创建和编辑连接高低端点的瀑布水幕；不得复用静水R8笔刷输入。
    Waterfall,
    /// 放置资产浏览器选中的GLB或GLTF实例。
    PlaceAsset,
    /// 从全局建筑预设放置带占地、正门和地基语义的建筑实例。
    Building,
    /// 放置或选择传送点并编辑目标引用。
    Portal,
    /// 编辑地表RGBA权重。
    Surface,
    Puddles, // 只编辑地面积水潜力的独立工具。
    /// 编辑草、麦、树等植被分布。
    Vegetation,
    /// 绘制、闭合和拖动动态地图区域空气墙控制点。
    Boundary,
}

impl EditorTool {
    pub const fn is_volume_sculpt(self) -> bool {
        matches!(
            self,
            Self::VolumeDig | Self::VolumeFill | Self::VolumeSmooth | Self::VolumeFlatten
        )
    }
}

/// 独立编辑视口的方向快捷键；投影由正交开关独立决定。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EditorView {
    /// 自由轨道视角；保留既有公开枚举名，允许透视或正交投影。
    #[default]
    Perspective,
    /// 沿世界负Y观察X/Z平面的顶视方向。
    Top,
    /// 沿世界负Z观察X/Y的前视方向。
    Front,
    /// 沿世界负X观察Z/Y的侧视方向。
    Side,
}

/// 单张地图离开时的观察状态；只记会话视角，不包含世界实体、纹理或作者草稿。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MapEditorViewSnapshot {
    pub(crate) view: EditorView,
    pub(crate) orthographic: bool,
    pub(crate) center: Vec3,
    pub(crate) extent: f32,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
}

/// F12植被覆盖图显示作者源、最终运行时结果或组内冲突。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum VegetationPreviewMode {
    /// 当前作者R8当帧回显，适合精确绘制笔刷痕迹。
    #[default]
    Source,
    /// 当前作者层所指profile的最终编译R8，适合检查覆盖与共享结果。
    Compiled,
    /// 当前所属组内至少两张作者图重叠的位置，适合排查规则输入。
    Conflict,
}

impl VegetationPreviewMode {
    /// 按固定顺序循环三个互补诊断视图。
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Source => Self::Compiled,   // 从原始刷印检查最终结果。
            Self::Compiled => Self::Conflict, // 从最终结果定位重叠输入。
            Self::Conflict => Self::Source,   // 回到可即时局部更新的绘制视图。
        }
    }

    /// 返回左栏状态使用的中文短标签。
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Source => "作者源",
            Self::Compiled => "最终结果",
            Self::Conflict => "组内冲突",
        }
    }
}

/// 新建地图弹框当前接收键盘输入的字段。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NewMapInputField {
    /// 中文显示名称字段默认获得焦点并启用IME。
    #[default]
    Name,
    /// 可选多行备注，由官方文本组件处理Unicode与换行。
    Notes,
}

/// 新建地图可选择的地形作者路线；只影响本次创建，不迁移或改写现有地图。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NewMapTerrainMode {
    /// 新图只创建容器；地形由后续对象操作独立添加。
    #[default]
    Empty,
    /// 原始完整 GLB 高度场路线，保留成熟的植被、水域和 Blender 工作流。
    LegacyHeightfield,
    /// 稀疏高度块由固定拓扑 Clipmap 显示；当前作为默认实验路线。
    SparseClipmap,
}

impl NewMapTerrainMode {
    /// 转为地图清单持久化的权威作者源类型。
    pub(crate) const fn terrain_kind(self) -> MapTerrainKind {
        match self {
            Self::Empty => MapTerrainKind::Empty,
            Self::LegacyHeightfield => MapTerrainKind::Heightfield,
            Self::SparseClipmap => MapTerrainKind::SparseClipmap,
        }
    }

    /// 创建过程和完成状态使用同一短标签，避免把两条路线混称为“高度图”。
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Empty => "空白地图",
            Self::LegacyHeightfield => "原始 GLB（稳定）",
            Self::SparseClipmap => "Geometry Clipmap（实验）",
        }
    }
}

/// 水位输入聚焦时冻结的完整作者目标，防止切图、重载或同名层串写。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WaterLevelInputTarget {
    /// 输入开始时草稿所属地图；新地图通常都会使用`lake`静水体ID。
    pub(crate) map_id: MapId,
    /// 地图内稳定层ID，不依赖提交时的当前选择序号。
    pub(crate) layer_id: String,
    /// 输入开始时作者修订；任何并发作者修改都会使旧输入明确失效。
    pub(crate) authoring_revision: u64,
}

/// 输入框提交给作者域的一次精确水位修改请求。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WaterLevelSetRequest {
    /// 完整目标身份必须在作者应用时再次核对。
    pub(crate) target: WaterLevelInputTarget,
    /// 已通过有限性和编辑器业务范围校验的世界Y米数。
    pub(crate) water_level: f32,
}

impl NewMapInputField {
    pub(crate) fn next(self) -> Self {
        match self { Self::Name => Self::Notes, Self::Notes => Self::Name }
    }
}

/// 地图资料编辑只冻结名称和备注基线，网格与LOD由对象作者事务独立管理。
#[derive(Clone, Debug)]
pub(crate) struct MapEditTarget {
    pub(crate) id: MapId, // 名称和备注写入原MapId，不受后续目录选择影响。
    pub(crate) original_name: String, // 弹框打开时目录与清单一致的并发基线。
    pub(crate) original_notes: String, // 外部备注修改不能被旧表单覆盖。
}

/// F12左栏水域对象列表中的作者对象类别；只属于当前编辑会话，不写入地图文件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaterAuthoringFeatureKind {
    /// 拥有独立R8覆盖与世界Y水位的水平静水体。
    StillWater,
    /// 按上游到下游保存控制点的河流或急流样条。
    River,
    /// 显式连接上下游端点的瀑布水幕。
    Waterfall,
}

/// 未保存切图只持有固定大小的会话身份，不复制地图草稿或撤销历史。
#[derive(Clone, Debug)]
pub(crate) struct MapSwitchConfirmation {
    pub(crate) source_map_id: Option<MapId>,
    pub(crate) source_revision: u64,
    pub(crate) global_revision: u64,
    pub(crate) target_map_id: MapId,
    pub(crate) target_display_name: String,
    /// 保存拒绝/失败诊断留在弹框内，不被其他后台任务的底栏状态覆盖。
    pub(crate) diagnostic: Option<String>,
    pub(crate) stage: MapSwitchStage,
}

/// 保存仍沿用现有事务；放弃授权只能在目标清单验证后的最终采用点消费一次。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MapSwitchStage {
    Choosing,
    Saving { success_sequence_before: u64 },
    DiscardApproved,
}

/// UI按钮的稳定动作，不通过实体名称分派逻辑。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorAction {
    Close,
    NewMap,
    /// 在当前树选择所指目录中打开新建文件夹弹框。
    MapFolderCreate,
    /// 为当前高亮文件夹打开重命名弹框。
    MapFolderRename,
    /// 确认文件夹创建、重命名或空文件夹删除。
    ConfirmMapFolderDialog,
    /// 取消尚未写盘的文件夹弹框操作。
    CancelMapFolderDialog,
    /// 点击文件夹名称输入槽后启用中文IME。
    FocusMapFolderName,
    /// 显式进入地图目录中当前选中的地图。
    EnterSelectedMap,
    /// 当前冻结地图成功保存后自动继续切图。
    SaveAndSwitchMap,
    /// 授权最终采用目标时放弃本次冻结修订的修改。
    DiscardAndSwitchMap,
    /// 取消切图；已经开始的保存事务仍正常收口。
    CancelMapSwitch,
    /// 为地图目录中当前选中的编辑器地图打开删除方式确认框。
    DeleteMap,
    /// 确认把受保护规则校验通过的地图移入项目回收区。
    ConfirmDeleteMap,
    /// 明确确认永久删除；与回收共用身份、引用和卸载保护。
    ConfirmDeleteMapPermanently,
    /// 取消尚未开始的地图删除确认。
    CancelDeleteMap,
    /// 放弃未保存F12地形顶点并采用当前Blender GLB完整拓扑。
    AdoptBlenderTerrain,
    /// 只对当前检测到的外部GLB版本授权一次F12覆盖保存。
    OverwriteBlenderTerrain,
    /// 创建空地图，或保存既有地图资料与兼容主网格设置。
    ConfirmNewMap,
    CancelNewMap,
    FocusNewMapName,
    RefreshAssets,
    AssetFolderUp,
    AssetPreviousPage,
    AssetNextPage,
    /// 把当前地图可见背景切换为原生大气与动态云。
    SkyModeDynamic,
    /// 把当前地图可见背景切换为已设置的主 Cube。
    SkyModeSkybox,
    SetSelectedSkybox,
    ClearSkybox,
    Save,
    Undo,
    Redo,
    ViewPerspective,
    ViewTop,
    ViewFront,
    ViewSide,
    /// 独立切换投影，保留当前方向、观察中心和缩放。
    ToggleOrthographic,
    /// 复用当前明确选择的完整范围，保持方向与投影。
    FocusSelected,
    /// 按当前观察轴与视口宽高比容纳完整地图。
    FrameMap,
    ToolRaise,
    ToolLower,
    ToolSmooth,
    ToolFlatten,
    ToolVolumeDig,
    ToolVolumeFill,
    ToolVolumeSmooth,
    ToolVolumeFlatten,
    RiverAdd,
    WaterfallAdd,
    WaterPointPrevious,
    WaterPointDelete,
    WaterSegmentKindToggle,
    WaterSourceNext,
    WaterTargetNext,
    WaterAddLayer,
    WaterDuplicateLayer,
    /// 把水域对象列表切到上一页，并直选该页第一项。
    WaterListPreviousPage,
    /// 把水域对象列表切到下一页，并直选该页第一项。
    WaterListNextPage,
    /// 删除水域对象列表当前高亮的静水体、河流或瀑布。
    WaterDeleteSelected,
    WaterLevelFromCursor,
    ToolPlaceAsset,
    ToolPortal,
    /// 删除当前地图列表中明确高亮的源传送点。
    PortalDeleteSelected,
    /// 聚焦当前所选传送点的中文名称输入框。
    PortalNameFocus,
    /// 把名称输入框内容提交到当前所选传送点。
    PortalNameApply,
    ToolSurface,
    ToolPuddles, // 地形页进入积水作者层的显式动作。
    VegetationPreviewNext,
    /// 把植被作者层列表切到前一页并直选该页第一层。
    VegetationListPreviousPage,
    /// 把植被作者层列表切到后一页并直选该页第一层。
    VegetationListNextPage,
    VegetationAddLayer,
    VegetationDuplicateLayer,
    /// 删除植被作者层列表当前高亮层。
    VegetationDeleteSelected,
    VegetationMoveUp,
    VegetationMoveDown,
    VegetationToggleEnabled,
    VegetationToggleLocked,
    VegetationNewGroup,
    VegetationNextGroup,
    VegetationCycleGroupMode,
    VegetationCycleMergeMode,
    ToolBoundary,
    BoundaryRedraw,
    BoundaryRemoveLastPoint,
    BoundaryClose,
}

/// 水网面板提交给作者域的一帧动作；选择与修改都在同一有序入口消费。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaterNetworkAction {
    /// 新建空河流并等待作者在视口逐点绘制。
    RiverAdd,
    /// 删除当前河流及失效的瀑布端点引用。
    RiverDelete,
    /// 新建瀑布并等待至少两个控制点。
    WaterfallAdd,
    /// 删除当前瀑布。
    WaterfallDelete,
    /// 选择当前路径的上一个控制点。
    PointPrevious,
    /// 删除当前控制点。
    PointDelete,
    /// 在普通河流段与急流段之间切换。
    ToggleSegmentKind,
    /// 循环当前路径的上游连接并吸附首点高度。
    SourceNext,
    /// 循环当前路径的下游连接并吸附末点高度。
    TargetNext,
}

/// 右栏离散环境按钮写入的一帧作者动作；连续数值统一由公共专业字段处理。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnvironmentEditAction {
    /// 显式选择地图可见天空来源；天空盒模式仍需已有主 KTX2 路径。
    SetSkyMode(MapSkyMode),
    /// 把资产浏览器当前选中的 KTX2 主路径登记为环境 IBL 派生身份。
    SetSelectedSkybox,
    /// 清除地图私有天空资源，并自动切回动态天空及相机原环境贴图。
    ClearSkybox,
}

/// 资产目录中的一个文件或子目录。
#[derive(Clone, Debug)]
pub struct EditorAssetEntry {
    /// 相对assets根的正斜杠路径。
    pub asset_path: String,
    /// UI显示文件名。
    pub display_name: String,
    /// 目录单独标记，点击时进入而不是选作画笔。
    pub is_directory: bool,
    /// 文件可被地图编辑器消费的用途。
    pub kind: MapAssetKind,
}

/// 新地图后台任务完成值。
#[derive(Debug)]
pub struct CreatedMap {
    /// 新建稳定地图ID。
    pub map_id: MapId,
    /// UI显示名称。
    pub display_name: String,
    /// 已写入清单的世界X轴宽度，供完成状态明确回显。
    pub width_meters: f32,
    /// 已写入清单的世界Z轴长度，供完成状态明确回显。
    pub length_meters: f32,
    /// 本次创建实际写入清单的地形路线，供完成反馈准确描述。
    pub(crate) terrain_mode: NewMapTerrainMode,
    /// 创建弹框沿用的格点回显；新格式不写入对应整图GLB。
    pub terrain_mesh_resolution: [u32; 2],
    /// 新地图自动归入确认时虚拟文件夹后，已发布的编辑器归类文件快照。
    pub(crate) folder_snapshot: Option<MapFolderFileSnapshot>,
}

/// 已通过UI校验、可在当前地图保存完成后继续执行的新建请求快照。
///
/// 名称和尺寸必须在作者点击确认时冻结，不能让后台保存期间继续输入的字符改变原请求。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MapCreationRequest {
    /// 已去除首尾空白的地图显示名称。
    pub(crate) display_name: String,
    /// 点击创建时冻结的可选作者备注。
    pub(crate) notes: String,
    /// 世界X轴东西宽度，单位为米。
    pub(crate) width_meters: f32,
    /// 世界Z轴南北长度，单位为米。
    pub(crate) length_meters: f32,
    /// 点击确认时冻结的地形作者路线；后台保存等待期间不得随 UI 改变。
    pub(crate) terrain_mode: NewMapTerrainMode,
    /// Clipmap新图的物理路线与地形方案一起冻结，旧GLB不读取。
    pub(crate) collision_lods: Vec<feathrise_map_content::MapCollisionLod>, // 冻结新模式全部层级。
    /// Clipmap新图必须保存显式显示LOD默认值；旧GLB固定为None。
    pub(crate) display_lods: Option<Vec<feathrise_map_content::MapTerrainDisplayLod>>, // 后台再次执行量化与双预算预检。
    /// 未触碰时保留旧方案32个当前Mip格的兼容值。
    /// 地形每格目标边长，单位为米；两条路线分别执行自己的容量校验。
    pub(crate) terrain_cell_meters: f32,
    /// 点击确认时冻结的当前虚拟文件夹；`None`表示根目录。
    pub(crate) folder_id: Option<String>,
    /// 点击确认时归类文件的精确有效字节；后台发布前用于外部并发检测。
    pub(crate) folder_source_bytes: Option<Vec<u8>>,
}

/// 已通过UI和目录校验、可在离开目标地图后执行的删除请求快照。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MapDeletionRequest {
    /// 只允许编辑器创建的`MAP-*`稳定身份。
    pub(crate) map_id: MapId,
    /// 确认框与最终状态使用的作者显示名称。
    pub(crate) display_name: String,
    /// 必须严格等于`maps/<MapId>`的私有目录，禁止宽路径或自定义目录进入移动事务。
    pub(crate) folder: String,
}

/// 删除方式随确认计划冻结，不能在等待卸载或后台执行时改变。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MapDeletionMode {
    Recycle,
    Permanent,
}

impl MapDeletionMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Recycle => "放入回收站",
            Self::Permanent => "直接删除",
        }
    }
}

/// 用户确认后等待安全场景身份的删除启动计划。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MapDeletionPlan {
    /// 二次确认时冻结的精确目录请求。
    pub(crate) request: MapDeletionRequest,
    /// 两个确认按钮分别授权可恢复回收或永久删除。
    pub(crate) mode: MapDeletionMode,
    /// Some要求指定玩法地图稳定运行；None只允许已退出世界、且不在加载或驻留交接中的空工作台。
    pub(crate) safe_active_map_id: Option<MapId>,
}

/// 地图私有目录与全局目录入口均完成更新后的后台结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeletedMap {
    /// 已从正式目录移除的地图身份。
    pub(crate) map_id: MapId,
    /// 状态栏回显的原作者显示名称。
    pub(crate) display_name: String,
    /// 可恢复回收的相对路径；永久删除或仅清理记录时为`None`。
    pub(crate) trash_folder: Option<String>,
    /// 永久删除成功与原目录缺失必须分别回显，不能混淆为清理记录。
    pub(crate) permanently_deleted: bool,
    /// 目录入口已移除但永久清理失败时保留残余路径，不能把半删目录重新登记。
    pub(crate) cleanup_warning: Option<String>,
    /// 删除前的私有目录，用于刷新可能仍浏览该路径的资产面板。
    pub(crate) source_folder: String,
    /// 删除归属键后已发布的编辑器归类文件快照；根目录地图为`None`。
    pub(crate) folder_snapshot: Option<MapFolderFileSnapshot>,
}
