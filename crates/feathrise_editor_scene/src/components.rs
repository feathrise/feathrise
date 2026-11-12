//! F12场景编辑器UI、相机、动态行和预览实体组件。

use bevy::prelude::*; // Component、Entity和UI交互类型。
use feathrise_scene_content::ScenePortalTarget; // 目标列表行冻结场景与可选传送点稳定引用。

use crate::monitor_resources::{EditorSceneMonitorGroup, EditorSceneMonitorPage}; // F4两级页签和正文共享唯一分页身份。
use crate::types::{
    EditorAction, EditorLayoutAction, EditorPane, SceneBrowserItem, WaterAuthoringFeatureKind,
}; // 静态动作与编辑器布局使用各自稳定身份。

/// 全屏编辑器根节点。
#[derive(Component, Clone, Copy, Default)]
#[require(feathrise_editor_ui::EditorThemeRoot)] // 所有F12子页继承主题所有权；游戏HUD不参与。
pub(crate) struct SceneEditorRoot;

/// 窗口UI承载者独立于各场景三维相机，切换场景暂停不能改变F12布局的DPI和物理尺寸。
#[derive(Component)]
pub(crate) struct SceneEditorUiCamera;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorPane(pub(crate) EditorPane);

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorPaneSplitter(pub(crate) EditorPane);

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorLayoutButton(pub(crate) EditorLayoutAction);

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorLayoutButtonText(pub(crate) EditorLayoutAction);

/// 场景树与场景对象属性整体退出预设编辑布局，唯一作者预览仍留在右栏。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneContext;

/// 独立预设正在编辑时，左侧场景导航显示返回场景提示，避免留下空白栏。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneContextHint;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSettingsButton(pub(crate) crate::types::EditorSettingsAction);

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSettingsPanel;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSettingsScaleText;

/// 设置框内的快捷键页与固定行原位回显，不创建第二个模态或随场景数量增删节点。
#[derive(Component, Clone, Default)]
pub(crate) struct SceneEditorShortcutSettingsPanel;
#[derive(Component, Clone, Default)]
pub(crate) struct SceneEditorShortcutSettingsBody;
#[derive(Component, Clone, Default)]
pub(crate) struct SceneEditorShortcutBindingText(pub(crate) feathrise_editor_core::EditorShortcutAction);
#[derive(Component, Clone, Default)]
pub(crate) struct SceneEditorShortcutNoticeText;

/// 编辑器标题栏当前场景文本。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorCurrentSceneText;

/// 编辑器底部状态文本。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorStatusText;

/// F4编辑器场景数据监控面板根节点。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorMonitorPanel;

/// F4 顶层功能组按钮。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SceneEditorMonitorGroupButton {
    pub(crate) group: EditorSceneMonitorGroup,
}

/// F4 二级叶子分页按钮。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SceneEditorMonitorTabButton {
    pub(crate) page: EditorSceneMonitorPage,
}

/// F4 二级页签行。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorMonitorSubTabRow;

/// F4 单个分页的正文文字；首版只有树木页会被低频刷新。
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SceneEditorMonitorPageText {
    pub(crate) page: EditorSceneMonitorPage,
}

/// 中央离屏三维视口图像节点。
#[derive(Component, Clone, Copy, Default)]
pub struct SceneEditorViewport;

/// 右侧场景页的地形平滑/平直着色按钮；不修改网格或作者数据。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct TerrainShadingButton(pub(crate) bool);

/// 右侧场景页独立的真实三角网格显示按钮；可与两种着色模式同时启用。
#[derive(Component, Clone, Copy, Default)] // 固定静态按钮只需零大小交互身份。
pub(crate) struct TerrainGridButton; // 点击仅修改本进程TerrainGridPreview，不进入场景保存或撤销。

/// 场景显示栏复用全局物理线条开关，不改变实际碰撞或场景作者数据。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct TerrainCollisionButton;

/// 中央视口左上角当前静水体身份徽标；绝对定位且完全穿透鼠标。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSelectedWaterBadge;

/// 当前静水体徽标内的增量文字节点。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSelectedWaterBadgeText;

/// 中央视口左上角当前传送点身份徽标；仅在传送工具存在有效当前源点时显示。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSelectedPortalBadge;

/// 当前传送点徽标内的增量文字节点。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSelectedPortalBadgeText;

/// 复用正式静水网格的编辑器专属半透明选择覆盖层。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSelectedWaterHighlight;

/// 中央视口左上角当前摆放网格身份徽标；位于静水徽标下方并完整穿透鼠标。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSelectedPlacementBadge;

/// 当前摆放网格徽标内的增量文字节点。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSelectedPlacementBadgeText;

/// 复用当前活动Scene0 primitive网格的编辑器专属半透明选择覆盖层。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSelectedPlacementHighlight;

/// 中央视口工具条中的三类通用笔刷滑杆。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SceneEditorBrushSliderKind {
    /// 世界米制笔刷半径；界面使用对数映射兼顾小范围精修和大场景铺陈。
    #[default]
    Radius,
    /// 角色点位最小间距，复用公共数值控件。
    Spacing,
    /// 单次笔刷作用强度；界面显示为百分比。
    Strength,
    /// 地形抬高与降低的世界高度变化速度；界面显示为米每秒。
    TerrainSpeed,
}

/// 可接收按下与拖动输入的笔刷滑杆轨道。
#[derive(Component, Clone, Copy, Debug, Default)]
pub(crate) struct SceneEditorBrushSlider(pub(crate) SceneEditorBrushSliderKind);

/// 编辑器独立三维相机。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorCamera;

/// 中央视口右上角的固定屏幕空间XYZ方向轴根节点。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorOrientationAxisRoot;

/// XYZ方向轴从中心指向正方向端点的彩色线段。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct SceneEditorOrientationAxisLine {
    /// 该线段代表的单位世界轴，只允许使用`Vec3::X/Y/Z`。
    pub(crate) world_axis: Vec3,
}

/// XYZ方向轴的正向实心端点或负向空心端点。
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct SceneEditorOrientationAxisEndpoint {
    /// 该端点代表的单位世界轴，只允许使用`Vec3::X/Y/Z`。
    pub(crate) world_axis: Vec3,
    /// `true`表示带字母的正方向，`false`表示无文字的负方向。
    pub(crate) positive: bool,
}

/// 左侧场景列表动态内容容器。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneList;

/// 场景页中显示当前展开项数与完整树项目数的短摘要。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneListTitle;

/// 场景树顶部固定显示的虚拟根目录；同时是把场景移回根目录的永久拖拽落点。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneRootRow;

/// 场景树独立滚动视口；树内容不再使用页码切换。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneTreeScroll;

/// 左侧每个常驻页正文的独立滚动marker；页签条不挂载该组件。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorLeftPanelScroll;

/// 右侧资产列表动态内容容器。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorAssetList;

/// 右侧每个常驻页正文的独立滚动marker；页签条不挂载该组件。
#[derive(Component, Clone, Copy, Default)]
pub struct SceneEditorRightPanelScroll;

/// 右侧当前资产目录文本。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorAssetFolderText;

/// 资产页中仅显示当前目录条目总数与页码的短摘要。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorAssetListTitle;

/// 右栏动态天空说明、当前环境 IBL 路径、作者参数与异步加载阶段回显。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorEnvironmentText;

/// 当前新传送点目标的可见文本。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorPortalTargetText;

/// 玩法页只在传送点工具激活时显示的双列表面板根。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorPortalSelectionPanel;

/// 当前已进入场景的传送点列表标题。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorCurrentPortalListTitle;

/// 当前场景全部传送点行的纵向容器。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorCurrentPortalList;

/// 当前场景传送点操作行；存在源点时保留删除入口。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorCurrentPortalActions;

/// 当前场景所选传送点的可点击名称输入框。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorPortalNameInput;

/// 传送点名称输入框内的已提交文字、IME预编辑和光标回显。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorPortalNameInputText;

/// 当前场景传送点行；点击后只检查已有源点，不改变新点目标。
#[derive(Component)]
pub(crate) struct SceneEditorCurrentPortalRow {
    pub(crate) slot: usize,               // 全量列表扁平索引与作者数组顺序一致。
    pub(crate) portal_id: Option<String>, // Presentation只绑定当前草稿中仍有效的稳定ID。
}

/// 当前场景传送点行的子文本实体。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorCurrentPortalRowText;

/// 场景树当前高亮目标场景的可用落点列表标题。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorTargetPortalListTitle;

/// 目标场景全部落点行的纵向容器；首项是清单安全默认出生点。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorTargetPortalList;

/// 目标场景落点行；`portal_id == None`明确代表清单默认出生点。
#[derive(Component)]
pub(crate) struct SceneEditorTargetPortalRow {
    pub(crate) slot: usize, // 全量列表扁平索引，零固定对应默认出生点。
    pub(crate) target: Option<ScenePortalTarget>, // 空槽不持有旧场景或旧传送点身份。
}

/// 目标场景落点行的子文本实体。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorTargetPortalRowText;

/// 地形页当前地表输出层与地表画笔状态回显。
#[derive(Component, Default, Clone)]
pub(crate) struct SceneEditorSurfaceText;

/// 植被页当前正式输出、作者层、分组和预览状态回显。
#[derive(Component, Default, Clone)]
pub(crate) struct SceneEditorVegetationStatusText;

/// 植被页开发者预设目录的总数、同步进度和加载错误摘要。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorVegetationPresetListTitle;

/// 植被页三列连续预设列表容器；完整高度交由左栏现有滚动视口访问。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorVegetationPresetList;

/// 连续预设列表中的一个真实目录按钮；索引直接对应当前已采用目录顺序。
#[derive(Component, Clone, Copy)]
pub(crate) struct SceneEditorVegetationPresetRow {
    pub(crate) index: usize, // 不再通过页码换算，任意目录位置都能直接选择。
}

/// 连续预设按钮的唯一子文本实体。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorVegetationPresetRowText;

/// 植被页作者层总数与当前页码的短摘要。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorVegetationLayerListTitle;

/// 左栏植被作者图层固定行池容器。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorVegetationLayerList;

/// 植被作者图层固定行；当前页刷新时原地替换作者层索引。
#[derive(Component)]
pub(crate) struct SceneEditorVegetationLayerRow {
    pub(crate) slot: usize,                // 固定行池槽位。
    pub(crate) layer_index: Option<usize>, // 当前页映射的作者层索引。
}

/// 植被作者图层行的子文本实体。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorVegetationLayerRowText;

/// 左栏当前区域边界点数、闭合状态、厚度和墙高回显。
#[derive(Component, Default, Clone)]
pub(crate) struct SceneEditorBoundaryText;

/// 左栏水网面板当前河流、瀑布、连接和控制点参数回显。
#[derive(Component, Default, Clone)]
pub(crate) struct SceneEditorWaterNetworkText;

/// 左栏静水、河流与瀑布共用的固定水域对象行池容器。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorWaterFeatureList;

/// 水域对象列表当前总数和页码的紧凑标题文本。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorWaterFeatureListTitle;

/// 水域对象固定行；点击时必须用类别和稳定ID重新解析当前数组位置。
#[derive(Component)]
pub(crate) struct SceneEditorWaterFeatureRow {
    pub(crate) slot: usize,                     // 固定行池槽位，稳定帧不增删实体。
    pub(crate) kind: WaterAuthoringFeatureKind, // 区分三个彼此独立的作者数组。
    pub(crate) feature_id: Option<String>, // 当前页绑定的稳定ID，禁止只靠易变数组下标提交选择。
    pub(crate) terrain: Option<feathrise_scene_content::TerrainId>, // None是场景根水；私有水必须先恢复其作者槽再操作。
}

/// 水域对象行的子文本实体。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorWaterFeatureRowText;

/// 左栏当前水层的公共精确米制数值输入根。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorWaterLevelInput;

/// BSN左栏中供原生公共数值输入挂载的固定槽位。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorWaterLevelInputSlot;

/// 新建场景名称输入弹框。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct NewSceneDialog;

/// 新建与编辑场景模态中按可见字段自然收缩的实体面板。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct NewSceneDialogPanel;

/// 新建与编辑场景模态的可滚动正文；标题、状态与底部操作栏不在此容器内。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct NewSceneDialogBodyScroll;

/// 新建场景名称可见文本。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct NewSceneNameText;

/// 新建和编辑共用的可选多行场景备注官方输入实体。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct NewSceneNotesText;

/// 新建与编辑共用弹框的动态标题、只读提示和提交按钮文字。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct NewSceneDialogTitle;
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct NewSceneDialogHint;
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct NewSceneConfirmText;

/// 新建弹框内的校验、自动保存和后台创建状态；避免失败只出现在屏幕底栏。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct NewSceneStatusText;

/// 可恢复场景删除的全屏模态确认层。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneDeletionDialog;

/// 删除确认框中冻结的场景显示名称、稳定ID与回收说明文本。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneDeletionDialogText;

/// 未保存切换场景的全屏模态根；隐藏时必须归还命中所有权。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneSwitchDialog;

/// 切换场景确认正文同时显示冻结目标、修改范围和保存失败原因。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneSwitchDialogText;

/// 目标名与失败原因在矮窗口内独立滚动，三种操作始终留在面板底部。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneSwitchDialogBody;

/// Blender与F12同时修改terrain.glb时的全屏模态选择层。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct TerrainConflictDialog;

/// 地形冲突框中冻结的场景和外部版本摘要文本。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct TerrainConflictDialogText;

/// 虚拟场景文件夹创建、重命名与删除共用的全屏模态层。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneFolderDialog;

/// 文件夹弹框当前模式标题。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneFolderDialogTitle;

/// 文件夹弹框名称输入区域；删除确认时原地隐藏。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneFolderDialogNameGroup;

/// 文件夹弹框已提交文字、IME预编辑文字和光标回显。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneFolderDialogNameText;

/// 文件夹弹框校验、保存与确认说明文本。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneFolderDialogStatusText;

/// 场景拖拽时跟随系统鼠标、且不参与命中的名称浮层。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneBrowserDragPreview;

/// 场景拖拽浮层中的冻结场景名称。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneBrowserDragPreviewText;

/// BSN静态按钮动作。
#[derive(Component, Clone, Copy)]
pub(crate) struct SceneEditorActionButton(pub(crate) EditorAction);

/// 按钮保留通用动作分派，但由所属工具页独占视觉；公共样式系统不反向依赖具体工具域。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorCustomActionStyle;

impl Default for SceneEditorActionButton {
    fn default() -> Self {
        Self(EditorAction::Close) // BSN模板先构造默认组件，再覆盖括号内的实际动作。
    }
}

/// 场景目录动态行。
#[derive(Component)]
pub(crate) struct SceneEditorSceneRow {
    /// 固定行池槽位。
    pub(crate) slot: usize,
    /// 当前行对应虚拟文件夹或场景；展开状态或滚动位置变化会原地替换。
    pub(crate) item: Option<SceneBrowserItem>,
}

/// 场景行的子文本实体。
#[derive(Component)]
pub(crate) struct SceneEditorSceneRowText;

/// 行名在独立裁切单元中显示，右侧“当前”徽标不能被长名称挤走或覆盖。
#[derive(Component)]
pub(crate) struct SceneEditorSceneRowNameCell;
/// 固定树行共享线框图标，路径只在实体首次挂载时交给AssetServer加载。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorTreeGlyph(pub &'static str);
#[derive(Component, Clone, Copy)]
pub(crate) struct SceneEditorAssetRowGlyph;
#[derive(Component)]
pub(crate) struct SceneEditorGltfBrowserRowGlyph;
#[derive(Component)]
pub(crate) struct SceneEditorSceneRowCurrent;
/// 位图图标承担目录类别和展开状态，不借用字体里的符号或文字前缀。
#[derive(Component, Clone, Copy, Default)]
pub(crate) enum SceneEditorSceneRowGlyph {
    #[default]
    Entry,
    Disclosure,
    Root,
}

/// 资产目录动态行。
#[derive(Component)]
pub(crate) struct SceneEditorAssetRow {
    /// 固定行池槽位。
    pub(crate) slot: usize,
    /// 全目录绝对索引；连续滚动时只换绑可见窗口。
    pub(crate) entry_index: Option<usize>,
}

/// 虚拟资产行的旁路身份；滚动换绑后不能接受旧文件的按下/释放。
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AssetRowBinding {
    pub(crate) revision: u64,
    pub(crate) viewport_revision: u64,
    pub(crate) index: Option<usize>,
}

/// 资产行的子文本实体。
#[derive(Component)]
pub(crate) struct SceneEditorAssetRowText;

/// 编辑器直接摆放、尚未发布为正式LOD配置的场景实例根。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct EditorPlacedAsset;

/// 建筑点放工具唯一的半透明模型幽灵根；不携带正式摆放身份或碰撞。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct EditorBuildingGhost;

/// 已完成F12专层隔离并使用共享幽灵材质的建筑模型网格。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct EditorBuildingGhostPrimitive;

/// 编辑器传送点位置预览；正式保存后由JSON热加载生成玩法实体。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct EditorPortalPreview;

/// 编辑器生成的可撤销地形预览网格。
#[derive(Component)]
pub(crate) struct EditorTerrainPreview;

/// 标记正式动态地形当前由哪一份未保存作者修订实时接管。
#[derive(Component, Clone, Debug, PartialEq, Eq)] // 修订身份用于识别GLB热重载是否覆盖了当前草稿。
pub(crate) struct EditorLiveTerrainSource {
    pub(crate) scene_id: feathrise_scene_content::SceneId, // 草稿所属稳定场景身份，切换场景后绝不复用。
    pub(crate) revision: u64, // 与EditorAuthoringSession::terrain_revision严格一致。
}

/// 当前选中植被层的有限实际模型抽样预览。
#[derive(Component)]
pub(crate) struct EditorVegetationPreview;

/// 当前植被密度层的半透明所见即所得覆盖网格。
#[derive(Component)]
pub(crate) struct EditorVegetationDensityOverlay;

/// 当前整批静水、河流、急流和瀑布生成的正式GPU水网预览。
#[derive(Component)]
pub(crate) struct EditorWaterPreview;

/// 保存一次指针按下时的视口实体，防止拖出后把其他按钮误当笔刷。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct EditorViewportPointerCapture;

pub use feathrise_editor_animation_model::media::SceneEditorOwnedEntity;
/// 无场景且未选中预设标签时的中央提示，不承担任何场景渲染或拾取职责。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorEmptyWorkspace;

/// 编辑器自己的场景加载页与透明鼠标门，均属于既有工作台外壳。
#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneLoadingPanel;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneLoadingInputBlocker;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneLoadingTitle;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneLoadingDetail;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneLoadingPercent;

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct SceneEditorSceneLoadingFill;

/// 恢复动作属于加载事务，不经过被加载遮罩隔离的普通作者按钮。
#[derive(Component, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum SceneLoadingRecoveryButton { Retry, #[default] Cancel }

pub(crate) use feathrise_editor_core::{EditorTextInput, SceneEditorPresetTabScroll, SceneEditorPresetWorkspaceScroll}; // 保留原组件路径和唯一 ECS 类型身份。
