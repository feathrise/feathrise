//! 编辑器会话、视口、资产目录和后台作者任务资源。

use std::collections::{BTreeMap, BTreeSet}; // 地图视角和目录展开均按稳定ID保存，只属于当前编辑器会话。
use std::sync::Arc; // 完整树在后台可见投影任务之间O(1)共享，避免主线程深复制。

use bevy::prelude::*; // Handle、Image、Resource、Task和数学类型。
use bevy::tasks::Task; // 创建地图与资产扫描均使用有界后台任务。
use feathrise_map_content::{MapId, MapPortalTarget}; // 当前编辑目标与作者层选择。
use feathrise_scene_core::state::{ScenePhase, LoadingProgress, SceneStartupGate}; // 地图可用性以正式场景及加载完成证据为准。

pub use crate::asset_catalog::EditorAssetCatalog; // 资产浏览状态由专属领域维护，保留既有资源导入路径。

/// 加载恢复只有一个请求；完成文档交接后回到空世界，再允许同图冷重试。
#[derive(Resource, Default)]
pub(crate) struct MapLoadingRecovery {
    pub(crate) request: Option<MapLoadingRecoveryRequest>,
    pub(crate) epoch: u64, // 恢复完成后重新计时，即使重试仍是同一MapId。
}

pub(crate) struct MapLoadingRecoveryRequest {
    pub(crate) map_id: MapId,
    pub(crate) retry: bool,
    pub(crate) returning: bool,
}

/// 设置窗口属于UI会话；只有显式修改显示、操作或快捷键偏好才触发持久化。
#[derive(Resource, Default)]
pub(crate) struct MapEditorSettingsState {
    pub(crate) open: bool,
    /// 同一设置框的顶部两页互斥，切页不叠加模态，关闭帧仍阻止点透。
    pub(crate) shortcuts: bool,
    pub(crate) listening: Option<feathrise_editor_core::EditorShortcutAction>,
    pub(crate) shortcut_notice: String,
    pub(crate) shortcut_notice_kind: crate::types::EditorShortcutNoticeKind,
    pub(crate) owns_mouse_this_frame: bool,
}
use crate::constants::{
    EDITOR_CONTENT_WIDTH, EDITOR_INSPECTOR_WIDTH,
}; // 弹框每次打开恢复受检尺寸与两条地形路线共用的作者目标格距。
use crate::types::{
    CreatedMap, DeletedMap, EditorLayoutAction, EditorPaneDrag, EditorTool, EditorView,
    EnvironmentEditAction, MapBrowserDragPress, MapBrowserDropTarget, MapBrowserEntry,
    MapBrowserIndexBuild, MapBrowserIndexSignature, MapBrowserItem, MapCreationRequest,
    MapDeletionPlan, MapDeletionRequest, MapEditTarget, MapEditorViewSnapshot, MapFolderAsset,
    MapFolderDialogState, MapFolderDocument, MapFolderFileSnapshot, MapFolderWriteFailure,
    MapSwitchConfirmation, NewMapInputField, VegetationPreviewMode,
    WaterLevelInputTarget, WaterLevelSetRequest, WaterNetworkAction,
};

/// F12布局运行状态；只将首选宽度和显式显隐同步到编辑器偏好。
#[derive(Resource, Clone, Debug)]
pub struct MapEditorLayoutState {
    pub(crate) content_width: f32,
    pub(crate) inspector_width: f32,
    pub(crate) content_visible: bool,
    pub(crate) inspector_visible: bool,
    pub(crate) content_fits: bool, // 窄窗口临时隐藏导航时，旧场景树交互同样失效。
    pub(crate) inspector_fits: bool,
    pub(crate) focus_restore: Option<(bool, bool)>, // 只保留专注期间的持久化偏好；退出专注一律展开两侧。
    pub(crate) drag: Option<EditorPaneDrag>,
    pub(crate) owns_mouse_this_frame: bool,
}

impl Default for MapEditorLayoutState {
    fn default() -> Self {
        Self {
            content_width: EDITOR_CONTENT_WIDTH,
            inspector_width: EDITOR_INSPECTOR_WIDTH,
            content_visible: true,
            inspector_visible: true,
            content_fits: true,
            inspector_fits: true,
            focus_restore: None,
            drag: None,
            owns_mouse_this_frame: false,
        }
    }
}

impl MapEditorLayoutState {
    /// 专注模式保存进入前的显隐，窄窗是否容纳检查器不参与持久化。
    pub(crate) fn preferences(&self) -> feathrise_editor_core::EditorLayoutPreferences {
        let (content_visible, inspector_visible) = self.focus_restore
            .unwrap_or((self.content_visible, self.inspector_visible));
        feathrise_editor_core::EditorLayoutPreferences {
            content_width: self.content_width,
            inspector_width: self.inspector_width,
            content_visible,
            inspector_visible,
        }.sanitized()
    }

    /// 启动恢复和默认重置都退出临时专注/拖动，不重建文档或触碰作者数据。
    pub(crate) fn apply_preferences(&mut self, preferences: feathrise_editor_core::EditorLayoutPreferences) {
        let preferences = preferences.sanitized();
        self.content_width = preferences.content_width;
        self.inspector_width = preferences.inspector_width;
        self.content_visible = preferences.content_visible;
        self.inspector_visible = preferences.inspector_visible;
        self.focus_restore = None;
        self.drag = None;
    }

    pub fn inspector_shown(&self) -> bool {
        self.inspector_visible && self.inspector_fits
    }

    pub fn content_shown(&self) -> bool {
        self.content_visible && self.content_fits
    }

    /// 专注开关固定关闭／展开两侧；单侧按钮仍从当前显隐独立切换。
    pub(crate) fn toggle(&mut self, action: EditorLayoutAction) {
        if self.focus_restore.take().is_some() {
            if action == EditorLayoutAction::Focus {
                self.content_visible = true;
                self.inspector_visible = true;
                self.drag = None;
                self.owns_mouse_this_frame = true;
                return;
            }
            // 显式打开一侧即退出临时专注；保持另一侧当前关闭，后续偏好保存作者的新选择。
        } else if action == EditorLayoutAction::Focus {
            self.focus_restore = Some((self.content_visible, self.inspector_visible));
            self.content_visible = false;
            self.inspector_visible = false;
        }
        match action {
            EditorLayoutAction::Content => self.content_visible = !self.content_visible,
            EditorLayoutAction::Inspector => self.inspector_visible = !self.inspector_visible,
            EditorLayoutAction::Focus => {}
        }
        self.drag = None;
        self.owns_mouse_this_frame = true; // 布局变动的同次点击不能落到刚移过来的地图画布。
    }
}

/// 无缓存地读取真实世界会话；目录选中项和编辑器启动标记都不能充当进图证据。
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct MapEditorWorldSession<'w> {
    hosted: Option<Res<'w, crate::workbench::WorkbenchModel>>, // 独立UI只读游戏进程就绪快照，不伪造本地GameScene或World。
    scene: Option<Res<'w, State<ScenePhase>>>,
    active_dynamic: Option<Res<'w, feathrise_map_content::ActiveDynamicMap>>,
    startup_gate: Option<Res<'w, SceneStartupGate>>,
    loading: Option<Res<'w, LoadingProgress>>,
    residency: Option<Res<'w, feathrise_scene_core::residency::SceneResidencyActivity>>,
}

impl MapEditorWorldSession<'_> {
    /// 稳定空世界或正常菜单允许收起F12；加载和驻留交接中的世界仍需等待。
    pub(crate) fn idle_menu(&self) -> bool {
        if let Some(hosted) = &self.hosted {
            return hosted.visible && hosted.active_map.is_none() && !hosted.busy;
        }
        self.scene
            .as_ref()
            .is_some_and(|scene| *scene.get() == ScenePhase::Idle)
            && self.loading.is_none()
            && self.residency.as_ref().is_none_or(|state| !state.switching)
    }

    pub(crate) fn ready(&self) -> bool {
        if let Some(hosted) = &self.hosted {
            return hosted.visible && hosted.active_map.is_some();
        }
        if self.residency.as_ref().is_some_and(|state| state.switching) {
            return false;
        }
        let Some(scene) = self.scene.as_ref().map(|scene| *scene.get()) else {
            return false;
        };
        if !ScenePhase::active_phases().contains(&scene)
            || self.loading.is_some()
            || (scene == ScenePhase::Active
                && self
                    .active_dynamic
                    .as_ref()
                    .is_none_or(|active| active.map_id.is_none()))
        {
            return false;
        }
        // 与正式 scene_startup_ready 保持相同完成门：进了玩法枚举但仍在加载，不算已进入世界。
        self.startup_gate
            .as_ref()
            .is_none_or(|gate| gate.target != scene || gate.ready)
    }
}

/// 新地图创建事务的带票据结果。
type MapCreationTaskResult = (u64, Result<CreatedMap, String>);

/// 地图删除事务的带票据结果。
type MapDeletionTaskResult = (u64, Result<DeletedMap, String>);

/// 虚拟文件夹原子写入的带票据结果。
pub(crate) type MapFolderWriteTaskResult = (
    u64, // 迟到任务只有匹配当前single-flight票据时才可采用。
    u64, // 乐观文档修订也必须匹配，防止异常状态换代后采用迟到结果。
    Result<MapFolderFileSnapshot, MapFolderWriteFailure>, // 成功返回新基线，失败区分并发与普通IO。
);

/// 地图浏览索引后台排序的完整身份与结果。
pub(crate) type MapBrowserIndexTaskResult = (
    MapBrowserIndexSignature,  // 后台任务启动时冻结的目录与归类修订。
    Arc<Vec<MapBrowserEntry>>, // 文件夹优先、地图保持catalog顺序的完整深度优先树。
);

/// 地图树展开集合后台投影的完整身份与结果。
pub(crate) type MapBrowserProjectionTaskResult = (
    MapBrowserIndexSignature, // 投影必须属于当前完整树双修订。
    u64,                      // 展开集合修订用于拒收快速连续点击产生的迟到结果。
    Arc<Vec<MapBrowserEntry>>, // 可见投影也以Arc供Shift连选O(1)共享。
);

/// F12左栏虚拟地图文件夹的当前权威内存状态。
#[derive(Resource, Debug)]
pub struct MapBrowserState {
    /// 最近一次有效文件或乐观操作形成的完整归类文档。
    pub(crate) document: MapFolderDocument,
    /// 最近一次有效磁盘文件的精确字节；`None`表示文件缺失时的合法空分类。
    pub(crate) source_bytes: Option<Vec<u8>>,
    /// 首次有效资产或明确加载失败是否已经收口；只用于弥补启动帧早到事件。
    pub(crate) initial_load_resolved: bool,
    /// 每次文档或回滚变化时供完整树索引拒收迟到结果的归类修订。
    pub(crate) document_revision: u64,
    /// 当前会话已经展开的文件夹稳定ID；根目录固定展开且不写入本集合。
    pub(crate) expanded_folder_ids: BTreeSet<String>,
    /// 展开集合每次真实变化后递增，后台可见投影按此拒收迟到结果。
    pub(crate) expansion_revision: u64,
    /// 当前高亮的根目录、文件夹或地图；高亮与实际已进入地图保持独立。
    pub(crate) selected_item: Option<MapBrowserItem>,
    /// Ctrl/Shift选集与单个焦点分开；地图进入、资料和删除仍要求单选。
    pub(crate) selection: crate::types::MapBrowserSelection,
    pub(crate) selection_pending: bool,
    /// 创建、重命名和删除空文件夹共用的模态输入状态。
    pub(crate) dialog: Option<MapFolderDialogState>,
    /// 损坏文件或并发变化等待重新加载期间的写入禁用原因。
    pub(crate) write_blocked: Option<String>,
    /// 最近一次非致命写入或热更新诊断。
    pub(crate) last_error: Option<String>,
}

impl MapBrowserState {
    /// 应用作者只观察模态门，文件夹表单的内容与所有权仍归编辑器。
    pub fn modal_open(&self) -> bool { self.dialog.is_some() }
}

impl Default for MapBrowserState {
    fn default() -> Self {
        Self {
            document: MapFolderDocument::default(), // 首帧安全显示全部地图在根目录。
            source_bytes: None,                     // AssetServer成功或明确缺失后再建立写入基线。
            initial_load_resolved: false, // 首次Update还需主动采用已在Startup完成的有效资产。
            document_revision: 0,         // 首个资产事件会推进到非零修订。
            expanded_folder_ids: BTreeSet::new(), // 初始只显示根目录直属项目，目录由作者逐项展开。
            expansion_revision: 0,        // 首个展开动作推进投影身份。
            selected_item: None,          // 不猜测任意地图作为结构操作目标。
            selection: default(),
            selection_pending: false,
            dialog: None,                 // 初始没有模态文件夹操作。
            write_blocked: Some("正在加载地图归类文件".to_owned()), // 防止首载完成前覆盖已有文件。
            last_error: None,             // 正常启动没有诊断。
        }
    }
}

/// 保持编辑器归类Asset强句柄并接收文件热更新。
#[derive(Resource)]
pub(crate) struct MapFolderAssetHandle {
    /// 唯一`editor.map_folders.json`强句柄。
    pub(crate) handle: Handle<MapFolderAsset>,
}

/// 乐观结构修改的回滚快照与唯一后台写入槽。
#[derive(Resource, Default)]
pub(crate) struct MapFolderPersistence {
    /// 每次真实文件写入递增，迟到结果不得覆盖较新状态。
    pub(crate) ticket: u64,
    /// 同一时间最多一个归类文件写入任务。
    pub(crate) task: Option<Task<MapFolderWriteTaskResult>>,
    // 地图名称与备注共用既有single-flight写入槽，加载和保存继续据此互斥。
    pub(crate) rename_task: Option<Task<crate::map_browser::rename::MapEditResult>>,
    /// 与单项保存共用互斥门；批量校验和写入串行占用一个后台槽。
    pub(crate) batch: Option<crate::types::MapFolderBatchMove>,
    /// 写入失败时恢复的完整归类文档。
    pub(crate) rollback_document: Option<MapFolderDocument>,
    /// 写入失败时恢复的高亮行。
    pub(crate) rollback_selected_item: Option<Option<MapBrowserItem>>,
    /// 写入失败时恢复的会话展开集合。
    pub(crate) rollback_expanded_folder_ids: Option<BTreeSet<String>>,
    /// 写入失败时恢复的磁盘字节基线。
    pub(crate) rollback_source_bytes: Option<Option<Vec<u8>>>,
    /// 写入失败时恢复的旧地图按钮目标。
    pub(crate) rollback_map_selection_id: Option<Option<MapId>>,
}

impl MapFolderPersistence {
    /// 当前是否已有结构写入占用唯一文件槽。
    pub(crate) fn busy(&self) -> bool {
        self.task.is_some() || self.rename_task.is_some() || self.batch.is_some() // 所有结构写入共享互斥门。
    }
}

/// 分帧目录快照与后台排序的地图浏览索引状态。
#[derive(Resource, Default)]
pub(crate) struct MapBrowserIndexState {
    /// 当前希望构建的完整输入身份。
    pub(crate) desired_signature: Option<MapBrowserIndexSignature>,
    /// 当前分帧推进的目录快照。
    pub(crate) build: Option<MapBrowserIndexBuild>,
    /// 同一时间最多一个不可协作取消的后台完整树整理任务。
    pub(crate) task: Option<Task<MapBrowserIndexTaskResult>>,
    /// 已完整整理树对应的双修订身份。
    pub(crate) applied_signature: Option<MapBrowserIndexSignature>,
    /// 完整深度优先树以Arc供后台展开投影O(1)共享。
    pub(crate) full_entries: Arc<Vec<MapBrowserEntry>>,
    /// 当前希望采用的“完整树 + 展开集合”身份。
    pub(crate) desired_projection: Option<(MapBrowserIndexSignature, u64)>,
    /// 同一时间最多一个展开过滤任务，快速连点只换代而不并发堆积。
    pub(crate) projection_task: Option<Task<MapBrowserProjectionTaskResult>>,
    /// 当前公开可见行对应的“完整树 + 展开集合”身份。
    pub(crate) applied_projection: Option<(MapBrowserIndexSignature, u64)>,
    /// 当前展开路径下的完整可见行；滚动虚拟行池只读取视口附近片段。
    pub(crate) entries: Arc<Vec<MapBrowserEntry>>,
    /// 与当前公开可见行同一次原子采用的完整树项目总数。
    pub(crate) total_entries: usize,
    /// 构建或排序尚未完成时为`true`，摘要明确显示整理状态。
    pub(crate) rebuilding: bool,
}

/// 地图与文件夹左键阈值拖拽、落点和延迟鼠标释放状态。
#[derive(Resource, Debug, Default)]
pub struct MapBrowserDragState {
    /// 尚未超过六物理像素阈值的按下候选。
    pub(crate) press: Option<MapBrowserDragPress>,
    /// 仅多选拖拽持有浅共享快照；单项继续使用原press身份。
    pub(crate) selection: Option<crate::types::MapBrowserSelection>,
    pub(crate) catalog_revision: Option<u64>,
    /// 普通按下已选组时先保留整组，未拖动的松开才回到单选。
    pub(crate) collapse_on_release: bool,
    /// Ctrl/Shift选择文件夹不附带展开操作。
    pub(crate) suppress_folder_toggle: bool,
    /// 超过阈值后正在显示浮层并独占编辑器鼠标。
    pub(crate) active: bool,
    /// 当前光标下可接受松开的可见文件夹或固定根目录目标。
    pub(crate) drop_target: Option<MapBrowserDropTarget>,
    /// 当前无效文件夹落点的原因，浮层明确提示且不高亮为可放置。
    pub(crate) drop_error: Option<String>,
    /// 拖拽及其release帧是否仍阻止其他作者鼠标入口。
    pub blocked: bool,
}

impl MapBrowserDragState {
    /// 当前帧是否必须让其他地图编辑器鼠标系统完全让位。
    pub fn blocks_other_mouse(&self) -> bool {
        self.blocked // 延迟清理保证松键边沿不会补触发视口、按钮或数值框。
    }
}

/// 公共数值拖动与资产预览拖动对其余地图编辑器鼠标入口的延迟释放门。
///
/// 共享 UI 控件只跟踪当前正在拖动的数字实体；地图编辑器还要覆盖和弦场景：主拖动键先松开时，
/// 另一枚鼠标键可能仍处于按下状态。此资源把所有权延长到三枚作者鼠标键全部松开后的下一静默帧，
/// 避免同一组物理按键补触发按钮、笔刷、控制点或视口导航。
/// 资产预览复用此门：画布内开始的拖动即使越过地图视口，也只能继续调整预览。
#[derive(Resource, Debug, Default)]
pub struct MapEditorNumericScrubMouseGuard {
    blocked: bool, // `true`表示本帧除已持有数值/预览拖动者外的作者鼠标入口都必须丢弃输入。
    pub(crate) floating_panel_hovered: bool, // 悬停挡住滚轮；只有在浮窗内按下才保留拖动所有权。
}

impl MapEditorNumericScrubMouseGuard {
    /// 共享 UI 控件拖动态与本帧鼠标边沿共同推进唯一所有权状态机。
    pub(crate) fn advance(
        &mut self,
        scrub_active: bool,        // 当前是否仍有公共数值框持有左右拖动身份。
        any_button_pressed: bool,  // 左、右、中任一作者鼠标键是否仍按住。
        any_button_released: bool, // 本帧是否刚松开左、右、中任一键；release帧仍不得穿透。
    ) {
        if scrub_active {
            self.blocked = true; // 只要拖动实体存在，就立即取得地图编辑器级鼠标所有权。
            return;
        }
        if !self.blocked {
            return; // 从未开始数值拖动时，普通鼠标按键不能反向激活该门。
        }
        self.blocked = any_button_pressed || any_button_released; // 全松后的下一静默帧才归还鼠标。
    }

    /// 当前帧是否必须丢弃全部非数值地图编辑器鼠标输入。
    pub fn blocked(&self) -> bool {
        self.blocked || self.floating_panel_hovered // 鼠标离开浮窗时，单纯悬停不会扣住下一次外部点击。
    }
}

/// 当前传送点名称的内联中文输入会话；稳定ID冻结后才允许提交到作者草稿。
#[derive(Clone, Debug, Default)]
pub(crate) struct PortalNameEditState {
    /// 是否正在接收键盘和中文IME输入。
    pub(crate) active: bool,
    /// 开始编辑时冻结的当前地图传送点稳定ID。
    pub(crate) portal_id: Option<String>,
    /// 已提交的正式名称文本。
    pub(crate) value: String,
    // 未提交候选只存在于官方EditableText，不复制到业务状态。
}

impl PortalNameEditState {
    /// 结束编辑并清除所有冻结身份，防止切图或换选择后误改另一点。
    pub(crate) fn clear(&mut self) {
        self.active = false; // 后续键盘事件不再进入传送名称。
        self.portal_id = None; // 冻结稳定ID不能跨选择复用。
        self.value.clear(); // 下次聚焦从当时作者记录重新读取名称。
    }
}

/// 按地图缓存本进程调试选择；关闭编辑器仍把当前地图模式留给玩法视角。
#[derive(Resource, Default)]
pub(crate) struct TerrainShadingSession {
    pub(crate) map_id: Option<MapId>,
    pub(crate) editor_open: bool,
    pub(crate) preview_expanded: bool, // 兼容旧内部名：仅表示地图显示预览展开，不授权网格对象选择。
    pub(crate) flat_maps: BTreeSet<MapId>, // 本进程仅记录主动选过平直的地图，重启自然回到平滑。
    pub(crate) grid_maps: BTreeSet<MapId>, // 本进程仅记录开启三角网格的地图，不写settings、清单或存档。
}

/// F12编辑器当前会话状态。
#[derive(Resource, Debug)]
pub struct MapEditorState {
    /// 是否显示全屏编辑器。
    pub open: bool,
    /// 空工作台已被用户关闭；保留无菜单窗口模式，但启动标记不能再强制打开F12。
    pub(crate) empty_workspace_closed: bool,
    /// 用户已请求关闭；未保存地形的同修订碰撞确认完成后才真正解除F12暂停。
    pub close_requested: bool,
    /// 新建地图名称输入弹框是否显示。
    pub new_map_dialog_open: bool,
    /// 弹框关闭同帧禁止视口世界点选；取消/保存的左键不得再选中传送/边界/摆放并跳到玩法页。
    pub suppress_world_pick_this_frame: bool,
    /// 有目标时复用新建弹框，只允许修改名称和备注；None时仍为新建流程。
    pub(crate) new_map_edit_target: Option<MapEditTarget>,
    /// 当前输入的地图显示名称。
    pub(crate) new_map_name: String,
    /// 可选地图备注；新建请求与编辑事务在确认帧各自冻结此值。
    pub(crate) new_map_notes: String,
    /// 新建地图弹框当前键盘焦点，保证中文名称与数值输入互不污染。
    pub(crate) new_map_input_field: NewMapInputField,
    /// 中文输入法尚未提交的预编辑文本，只用于回显，不写入最终名称。
    /// 当前已真正进入并加载作者草稿的地图；不得被目录行单击改写。
    pub selected_map_id: Option<MapId>,
    /// 地图目录中的高亮选择；仅供“进入/删除”按钮取目标，不触发加载。
    pub map_list_selection_id: Option<MapId>,
    /// 点击“进入地图”后等待正式目录与清单就绪的稳定目标。
    pub pending_map_transition: Option<MapId>,
    /// 未保存切图的冻结目标与一次性选择；目录高亮不能重定向这份请求。
    pub(crate) map_switch_confirmation: Option<MapSwitchConfirmation>,
    /// 当前资产浏览器选中的资源路径。
    pub selected_asset: Option<String>,
    /// 左栏当前选中的全局建筑预设规范 ID；建筑摆放不依赖普通资产选择。
    pub(crate) building_selected_preset_id: Option<String>,
    /// 当前工具。
    pub tool: EditorTool,
    /// 视口移动/旋转/缩放模式同步的公共手柄开关，供原输入门暂停笔刷及新资产点放。
    pub(crate) placement_transform_enabled: bool,
    /// 当前方向快捷键或自由轨道状态。
    pub view: EditorView,
    /// 正交投影独立于方向；默认透视，旋转或对准方向均不自动切换投影。
    pub orthographic: bool,
    /// 世界米制笔刷半径。
    pub brush_radius: f32,
    /// 扩展散布工具的最小间距，复用公共数值控件。
    pub brush_spacing: f32,
    /// 扩展工具在Input阶段发布笔刷事实，公共拾取和光标不读取领域模型。
    /// 外部作者只发布圆刷事实；半径、强度与光标显示仍复用普通地图状态。
    pub domain_brush_active: bool,
    pub domain_brush_erase: bool,
    /// 笔刷不透明度或单次强度。
    pub(crate) brush_strength: f32,
    /// 地形抬高与降低共用的世界高度变化速度，单位为米每秒。
    pub(crate) terrain_sculpt_speed: f32,
    /// 三维几何刷的会话参数，不影响地表/植被的独立球形表面笔触。
    pub(crate) volume_brush_shape: feathrise_map_terrain::volume::VolumeBrushShape,
    pub(crate) volume_brush_half_height: f32,
    pub(crate) volume_brush_lock_height: bool,
    /// 视口光标当前严格地图命中；摆放、传送、边界与取水位不得读取外扩笔刷坐标。
    pub cursor_world: Option<Vec3>,
    /// 后台完成的冻结单击仅发布一帧，松键不会丢失落点，也不会重复创建对象。
    pub ground_click_world: Option<Vec3>,
    pub(crate) ground_click_is_lower_layer: bool,
    /// 视口光标当前实际命中的地形三角序号；建筑联合整平据此冻结洞穴中的真实表层。
    pub(crate) cursor_triangle: Option<u32>,
    /// 圆形作者笔刷当前世界坐标；允许圆心位于地图外，但只在笔刷圆仍覆盖地图时存在。
    pub brush_cursor_world: Option<Vec3>,
    pub(crate) brush_terrain_id: Option<feathrise_map_content::TerrainId>,
    pub(crate) terrain_canvas_target: Option<feathrise_map_content::TerrainId>,
    /// 对象会话写保护由列表输入增量镜像，公共画笔不再依赖某个侧栏是否可见。
    pub(crate) terrain_locked: std::collections::BTreeSet<feathrise_map_content::TerrainId>,
    pub(crate) terrain_hidden: std::sync::Arc<std::collections::BTreeSet<feathrise_map_content::TerrainId>>,
    pub(crate) terrain_volume_field_active: bool,
    /// 地形笔刷当前实际命中的源三角序号；用于避免同一X/Z的洞顶与洞底被一笔同时修改。
    pub(crate) brush_cursor_triangle: Option<u32>,
    /// 最新用户可见状态。
    pub status: String,
    /// 固定七槽避免同帧不同操作的回执覆盖；这些元数据不参与作者事务。
    pub(crate) status_results:
        [Option<crate::types::EditorStatusResult>; crate::types::EditorStatusOperation::COUNT],
    pub(crate) status_sequence: u64,
    /// 地图作者文档是否有尚未保存的变更。
    pub dirty: bool,
    /// 切图期间保持工作台打开；完整就绪前锁定地图操作并显示编辑器进度。
    pub map_transition_in_progress: bool,
    /// 保存按钮写入的一帧请求，由作者系统消费。
    pub save_requested: bool,
    /// 撤销按钮写入的一帧请求。
    pub(crate) undo_requested: bool,
    /// 重做按钮写入的一帧请求。
    pub(crate) redo_requested: bool,
    /// 按钮和快捷键只登记一次导航意图；Cursor阶段复用完整对象范围或地图边界。
    pub(crate) viewport_framing_requested: Option<crate::types::EditorViewportFraming>,
    /// 公共数值输入获得焦点时冻结的地图、作者修订与稳定水层身份。
    pub(crate) water_level_input_target: Option<WaterLevelInputTarget>,
    /// 输入框提交的一次精确水位请求，由作者域按完整冻结身份消费。
    pub(crate) water_level_set_requested: Option<WaterLevelSetRequest>,
    /// “取地形水位”冻结地图、稳定层和作者修订；Clipmap等待真实三角命中后再消费。
    pub(crate) water_level_from_cursor_requested: Option<WaterLevelInputTarget>,
    /// 天空盒按钮的一帧请求；作者系统消费后统一校验、登记撤销并刷新离屏预览。
    pub(crate) environment_action_requested: Option<EnvironmentEditAction>,
    /// “重画边界”按钮的一帧请求。
    pub(crate) boundary_redraw_requested: bool,
    /// “删除末点”按钮的一帧请求。
    pub(crate) boundary_remove_last_requested: bool,
    /// “闭合边界”按钮的一帧请求。
    pub(crate) boundary_close_requested: bool,
    /// 新放置传送点当前引用的目标。
    pub(crate) portal_target: Option<MapPortalTarget>,
    /// 当前地图传送点列表选中的稳定ID；只用于作者检查现有源点。
    pub(crate) selected_current_portal_id: Option<String>,
    /// 当前所选传送点的可编辑作者名称输入会话。
    pub(crate) portal_name_edit: PortalNameEditState,
    /// 当前地表层编号；超出草稿范围时显示系统会安全钳制。
    pub(crate) selected_surface_layer: usize,
    /// 左栏当前选择的全局 PBR 地表预设稳定 ID；首次绘制前会按需加入地图局部层表。
    pub(crate) surface_selected_preset_id: Option<String>,
    /// 当前植被层编号。
    pub(crate) selected_vegetation_layer: usize,
    /// “新空白层”使用的开发者预设稳定 ID；正式输出只在首次使用时延迟创建。
    pub(crate) vegetation_selected_preset_id: Option<String>,
    /// 作者植被图层固定行池当前页。
    pub(crate) vegetation_layer_page: usize,
    /// 当前植被覆盖图显示作者源、最终编译结果或组内冲突。
    pub(crate) vegetation_preview_mode: VegetationPreviewMode,
    /// 当前湖泊/池塘静水体编号；零层时保持为零但不代表已有对象。
    pub(crate) selected_water_layer: usize,
    /// 左栏当前选择的全局水体外观预设；新建或显式应用时写入静水层稳定引用。
    pub(crate) water_selected_preset_id: Option<String>,
    /// 当前河流样条编号；结构变化后由作者域钳制到仍存在的项目。
    pub(crate) selected_water_river: usize,
    /// 当前瀑布编号；结构变化后由作者域钳制到仍存在的项目。
    pub(crate) selected_waterfall: usize,
    /// 静水、河流和瀑布统一对象列表的当前页；仅为F12会话UI状态。
    pub(crate) water_feature_page: usize,
    /// 当前河流或瀑布控制点编号；路径切换时按稳定规则重新选择。
    pub(crate) selected_water_control_point: Option<usize>,
    /// 水网面板的一帧请求；作者域消费后必须清空，避免稳定帧重复修改。
    pub(crate) water_network_action_requested: Option<WaterNetworkAction>,
}

impl Default for MapEditorState {
    fn default() -> Self {
        Self {
            open: false,
            empty_workspace_closed: false,
            close_requested: false,
            new_map_dialog_open: false,
            suppress_world_pick_this_frame: false,
            new_map_edit_target: None, // 初次弹框总是新建模式。
            new_map_name: String::new(),
            new_map_notes: String::new(), // 旧地图不带备注仍可直接编辑。
            new_map_input_field: NewMapInputField::Name,
            selected_map_id: None,
            map_list_selection_id: None,
            pending_map_transition: None,
            map_switch_confirmation: None,
            selected_asset: None,
            building_selected_preset_id: None,
            tool: EditorTool::Inspect,
            placement_transform_enabled: false,
            view: EditorView::Perspective,
            orthographic: false,
            brush_radius: 24.0,
            brush_spacing: 3.0,
            domain_brush_active: false,
            domain_brush_erase: false,
            brush_strength: 0.35,
            terrain_sculpt_speed: 24.0, // 保持原抬高/降低工具写死的默认速度，升级后手感不突变。
            volume_brush_shape: default(),
            volume_brush_half_height: 2.0,
            volume_brush_lock_height: false,
            cursor_world: None,
            ground_click_world: None,
            ground_click_is_lower_layer: false,
            cursor_triangle: None,
            brush_cursor_world: None,
            brush_terrain_id: None,
            terrain_canvas_target: None,
            terrain_locked: Default::default(),
            terrain_hidden: Default::default(),
            terrain_volume_field_active: false,
            brush_cursor_triangle: None,
            status: "地图编辑器已打开".to_owned(),
            status_results: Default::default(),
            status_sequence: 0,
            dirty: false,
            map_transition_in_progress: false,
            save_requested: false,
            undo_requested: false,
            redo_requested: false,
            viewport_framing_requested: None,
            water_level_input_target: None,
            water_level_set_requested: None,
            water_level_from_cursor_requested: None,
            environment_action_requested: None,
            boundary_redraw_requested: false,
            boundary_remove_last_requested: false,
            boundary_close_requested: false,
            portal_target: None,
            selected_current_portal_id: None,
            portal_name_edit: PortalNameEditState::default(), // 未选择源点时不持有名称输入身份。
            selected_surface_layer: 0,
            surface_selected_preset_id: None,
            selected_vegetation_layer: 0,
            vegetation_selected_preset_id: None,
            vegetation_layer_page: 0,
            vegetation_preview_mode: VegetationPreviewMode::Source,
            selected_water_layer: 0,
            water_selected_preset_id: None,
            selected_water_river: 0,
            selected_waterfall: 0,
            water_feature_page: 0,
            selected_water_control_point: None,
            water_network_action_requested: None,
        }
    }
}

impl MapEditorState {
    pub fn map_switch_confirmation_open(&self) -> bool { self.map_switch_confirmation.is_some() }
    /// 在原业务分支已经确认结果后附加只读回执；不改变保存票据、脏位或事务。
    pub fn record_status_result(&mut self, kind: crate::types::EditorStatusKind) {
        let id = self.selected_map_id.as_ref().map_or("", |id| id.as_str());
        let target = crate::types::EditorStatusTarget::new(id, id);
        self.record_operation_result(crate::types::EditorStatusOperation::MapSave, &target, kind);
    }

    /// 附加真实分支的对象化反馈；全局预设和目录操作不绑定当时浏览的地图。
    pub fn record_operation_result(
        &mut self,
        operation: crate::types::EditorStatusOperation,
        target: &crate::types::EditorStatusTarget,
        kind: crate::types::EditorStatusKind,
    ) {
        let status_text = Arc::from(self.status.as_str());
        self.record_operation_feedback(operation, target, kind, status_text);
    }

    /// 采用仍在等待时也能报告权威目录错误，但不改原业务提示或释放原采用门。
    pub(crate) fn record_operation_feedback(
        &mut self,
        operation: crate::types::EditorStatusOperation,
        target: &crate::types::EditorStatusTarget,
        kind: crate::types::EditorStatusKind,
        status_text: Arc<str>,
    ) {
        self.status_sequence = self.status_sequence.wrapping_add(1);
        let detail = if operation == crate::types::EditorStatusOperation::MapSave {
            status_text.clone() // 原地图保存诊断保留原文及已有地图身份，不重复添加标题。
        } else if operation == crate::types::EditorStatusOperation::NativePlugin {
            // 底栏摘要取详情首行；先显示真实结果，避免插件失败只剩工具名称。
            // 完整文档身份仍保留在详情，供多文档及后台保存区分来源。
            Arc::from(format!(
                "{status_text}\n\n{}“{}” [{}]",
                operation.label(),
                target.label,
                target.id
            ))
        } else if target.id.is_empty() {
            Arc::from(format!("{}\n{status_text}", operation.label()))
        } else if target.id == target.label {
            Arc::from(format!(
                "{}“{}”\n{status_text}",
                operation.label(),
                target.label
            ))
        } else {
            Arc::from(format!(
                "{}“{}” [{}]\n{}",
                operation.label(),
                target.label,
                target.id,
                status_text
            ))
        };
        self.status_results[operation.slot()] = Some(crate::types::EditorStatusResult {
            sequence: self.status_sequence, // 同因新尝试仍有新回执；不同槽可在同帧被消费。
            operation,
            target: target.clone(),
            map_id: self.selected_map_id.clone(),
            kind,
            status_text,
            detail,
        });
    }

    /// 沿用原关闭取消行为，仅为确定的失败补充可保留的表现回执。
    pub(crate) fn cancel_close_with_error(&mut self, status: impl Into<String>) {
        self.cancel_close_request(status);
        self.record_status_result(crate::types::EditorStatusKind::Error);
    }

    pub(crate) fn record_global_status_result(&mut self, kind: crate::types::EditorStatusKind) {
        let target = crate::types::EditorStatusTarget::new("world_environment", "全局世界环境");
        self.record_operation_result(
            crate::types::EditorStatusOperation::GlobalEnvironmentSave,
            &target,
            kind,
        );
    }

    /// 原全局保存取消关闭行为不变，只把反馈与单张地图保存区分开。
    pub(crate) fn cancel_global_close_with_error(&mut self, status: impl Into<String>) {
        self.cancel_close_request(status);
        self.record_global_status_result(crate::types::EditorStatusKind::Error);
    }

    /// 登记一次可等待地形画面与碰撞联合采用的关闭请求。
    pub(crate) fn request_close(&mut self) {
        self.close_requested = true; // UI继续保持打开和物理暂停，直到Presentation阶段确认安全。
        self.status = "正在确认未保存地形画面与物理碰撞；完成后自动关闭编辑器".to_owned(); // 直接说明等待原因。
    }

    /// 取消尚未完成的关闭请求，恢复当前作者会话和世界笔刷输入。
    pub(crate) fn cancel_close_request(&mut self, status: impl Into<String>) {
        self.close_requested = false; // 下一光标发布阶段会在有效笔刷命中处恢复Gizmo并隐藏系统鼠标。
        self.status = status.into(); // 明确说明关闭被取消，避免作者误以为F12输入失效。
    }

    /// F12在已打开编辑器中切换“请求安全关闭/取消等待关闭”。
    pub(crate) fn toggle_close_request(&mut self) {
        if self.close_requested {
            self.cancel_close_request("已取消等待关闭；地图编辑器继续保持打开".to_owned()); // 第二次F12必须成为可恢复出口。
        } else {
            self.request_close(); // 第一次F12继续服从画面、材质与碰撞原子关闭门。
        }
    }

    /// 显式进入按钮登记一次可等待资产热加载的正式切图请求。
    pub(crate) fn queue_map_transition(&mut self, map_id: MapId, status: impl Into<String>) {
        self.map_list_selection_id = Some(map_id.clone()); // 请求期间保持用户明确选中的目录项。
        self.pending_map_transition = Some(map_id); // 驱动系统完整验证清单后才改写当前作者地图。
        self.status = status.into(); // 底栏明确说明当前正在等待自动进入，而不是要求用户再次点击。
    }

    /// 用户关闭F12时同时关闭新建弹框并取消尚未采用的切图请求。
    pub(crate) fn cancel_pending_map_transition(&mut self) {
        self.new_map_dialog_open = false; // 不让新建弹框残留到下次打开编辑器。
        self.new_map_edit_target = None; // 关闭F12不保留旧地图的编辑目标。
        self.pending_map_transition = None; // 迟到目录热加载结果不得在关闭后突然切换地图。
        self.map_switch_confirmation = None; // 放弃授权不能跨编辑器关闭或下次打开复用。
    }
}

/// 按地图记忆上次离开时的相机；加载阶段销毁RTT时仍保留，不写入地图或settings。
#[derive(Resource, Debug, Default)]
pub(crate) struct MapEditorViewMemory {
    pub(crate) maps: BTreeMap<MapId, MapEditorViewSnapshot>,
}

/// 独立离屏相机与世界覆盖范围。
#[derive(Resource, Debug)]
pub struct MapEditorViewportState {
    /// 保持离屏纹理强引用；图像节点和相机目标之外由本资源明确拥有其生命周期。
    pub(crate) _image: Handle<Image>,
    /// 编辑器相机实体。
    pub camera: Entity,
    /// 轨道或正交视图中心的世界坐标，平移同时支持水平与垂直方向。
    pub center: Vec3,
    /// 当前地图三维包围盒中心；正交相机沿实际观察方向锚定裁剪深度。
    pub(crate) scene_center: Vec3,
    /// 当前地图三维包围盒半尺寸；投影到观察轴确定正交深度，不让水平范围抬高顶视相机。
    pub(crate) scene_half_extents: Vec3,
    /// 统一使用透视轨道距离；正交覆盖由同一FOV换算，切投影不改缩放。
    pub extent: f32,
    /// 两种投影共用的水平轨道角。
    pub yaw: f32,
    /// 两种投影共用的俯仰角。
    pub pitch: f32,
    /// 已采用取景的地图ID；缓存源视角必须使用此身份，不能误用已指向目标的目录选择。
    pub(crate) framed_map_id: Option<MapId>,
    /// RTT物理像素尺寸，供相机射线与导航速度换算。
    pub render_size: UVec2,
}

/// 中央视口内容选择的纯会话状态；植被只保存正式输出身份，不保存任一株实例实体。
#[derive(Resource, Debug, Default)]
pub(crate) struct EditorViewportSelectionState {
    /// “查看”工具中当前通过视口选中的植被所属地图；切图后不得复用同名输出层。
    pub(crate) selected_vegetation_map_id: Option<MapId>,
    /// 当前通过视口选中的正式植被输出 ID；整层密度覆盖按此身份决定是否显示。
    pub(crate) selected_vegetation_output_id: Option<String>,
    /// 上次植被候选点击在离屏视口中的物理像素位置；六像素内才允许循环重叠层。
    pub(crate) last_vegetation_click_position: Option<Vec2>,
    /// 上次植被候选点击使用的真实时间；玩法暂停期间仍保持正常重复点击节奏。
    pub(crate) last_vegetation_click_seconds: Option<f64>,
    /// 上次落点按正式层顺序得到的有界候选 ID；只有完整相同才延续循环序列。
    pub(crate) last_vegetation_candidates: Vec<String>,
    /// 上次已经采用的候选下标；再次点击或 Alt 点击从下一项继续。
    pub(crate) last_vegetation_candidate_index: usize,
}

impl EditorViewportSelectionState {
    /// 清除整层高亮身份和重叠候选序列；场景对象或空白处点击都会调用。
    pub(crate) fn clear_vegetation_selection(&mut self) {
        self.selected_vegetation_map_id = None; // 下一帧“查看”工具立即隐藏旧密度覆盖。
        self.selected_vegetation_output_id = None; // 不让旧正式层身份影响新的作者层选择。
        self.last_vegetation_click_position = None; // 下一次植被点击从候选首项开始。
        self.last_vegetation_click_seconds = None; // 旧时间窗不能跨选择类别继续。
        self.last_vegetation_candidates.clear(); // 正式层 ID 数量受作者层硬门约束，原位复用容量。
        self.last_vegetation_candidate_index = 0; // 空序列固定回到安全首下标。
    }
}

/// 新建地图后台任务槽。
#[derive(Resource, Default)]
pub(crate) struct MapCreationTask {
    /// 每次确认递增，迟到结果必须匹配票据。
    pub(crate) ticket: u64,
    /// 同一时间最多创建一张地图。
    pub(crate) task: Option<Task<MapCreationTaskResult>>,
    /// 当前地图有未保存修改时冻结请求；保存完整成功后再启动创建，绝不丢弃草稿。
    pub(crate) pending_after_save: Option<MapCreationRequest>,
    /// 只用于失败回执与阶段文字，不能由后台期间继续输入的名称替换。
    pub(crate) feedback_target: Option<crate::types::EditorStatusTarget>,
}

impl MapCreationTask {
    /// 取消尚未启动的“保存后创建”；已经进入IO线程的创建事务仍按原有原子发布规则完成。
    pub(crate) fn cancel_pending_after_save(&mut self) -> bool {
        self.pending_after_save.take().is_some()
    }
}

/// 可恢复地图删除的确认、切图等待与单一后台任务槽。
#[derive(Resource, Default)]
pub(crate) struct MapDeletionTask {
    /// 每次真正启动文件事务时递增，迟到结果必须匹配当前票据。
    pub(crate) ticket: u64,
    /// 已通过保护规则的二次确认目标；存在时显示模态确认框。
    pub(crate) confirmation: Option<MapDeletionRequest>,
    /// 已确认并等待指定安全玩法地图身份的回收启动计划。
    pub(crate) pending_start: Option<MapDeletionPlan>,
    /// 同一时间最多移动一张地图并改写一次目录。
    pub(crate) task: Option<Task<MapDeletionTaskResult>>,
    pub(crate) references: Option<MapDeletionReferences>,
    /// 原事务移动请求前冻结的反馈对象，不参与安全回退或删除资格。
    pub(crate) feedback_target: Option<crate::types::EditorStatusTarget>,
}

/// 每帧只冻结两份已打开文档的引用Arc；全部入向检查在既有删除IO席位内执行。
#[derive(Default)]
pub(crate) struct MapDeletionReferences {
    pub(crate) cursor: Option<MapId>,
    pub(crate) revision: u64,
    pub(crate) current: Option<MapId>,
    pub(crate) entries: BTreeMap<MapId, Arc<[feathrise_map_content::MapInstancePlacement]>>,
}

impl MapDeletionTask {
    /// 返回是否已有确认、切图等待或文件事务，防止多个删除请求覆盖彼此。
    pub(crate) fn busy(&self) -> bool {
        self.confirmation.is_some() // 等待作者二次确认时保持目标不变。
            || self.pending_start.is_some() // 安全场景等待期间仍保留冻结目标。
            || self.task.is_some() // 文件事务开始后必须完整结束再接受下一请求。
    }

    /// 关闭F12时只取消尚未确认的弹框；已经确认的原子事务继续完成。
    pub(crate) fn cancel_confirmation(&mut self) {
        self.confirmation = None; // 未确认请求没有文件副作用，可直接丢弃。
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 资源状态机测试不启动窗口或渲染插件。

    #[test] // F12和关闭按钮都只能登记请求，不能先解除玩法暂停。
    fn close_request_keeps_editor_open_until_the_joint_gate_finishes() {
        let mut editor = MapEditorState {
            // 模拟已经打开的F12会话。
            open: true,  // UI与统一物理暂停当前有效。
            ..default()  // 其他作者字段使用正式默认值。
        };
        editor.request_close(); // 用户再次按F12或点击关闭。
        assert!(editor.open); // Presentation联合门尚未确认前必须继续显示并保持暂停。
        assert!(editor.close_requested); // 后续系统能够消费稳定关闭意图。
    }

    #[test] // 延迟关闭等待期间再次按F12必须恢复编辑，不能成为永久单向状态。
    fn second_f12_cancels_pending_close_and_restores_the_editor_session() {
        let mut editor = MapEditorState {
            open: true,  // F12界面仍因原子交接门保持可见。
            ..default()  // 当前工具、视图与笔刷命中无需改写。
        };
        editor.toggle_close_request(); // 第一次F12登记安全关闭。
        assert!(editor.close_requested); // 后台交接完成前保持等待态。
        editor.toggle_close_request(); // 第二次F12明确取消等待。
        assert!(editor.open); // 取消只恢复编辑，不应先关闭再重开。
        assert!(!editor.close_requested); // 笔刷系统下一阶段重新获得作者输入资格。
        assert!(editor.status.contains("取消等待关闭")); // 状态栏提供明确恢复反馈。
    }

    #[test] // 目录操作必须把“当前地图”和“准备进入的地图”保持为两个身份。
    fn queue_transition_does_not_publish_the_target_as_the_current_map_early() {
        let current = MapId::parse("HN-01").expect("测试当前MapId合法");
        let target = MapId::parse("MAP-target").expect("测试目标MapId合法");
        let mut editor = MapEditorState {
            selected_map_id: Some(current.clone()), // 当前作者草稿仍属于已经进入的HN-01。
            map_list_selection_id: Some(current),   // 打开目录时默认高亮当前图。
            ..default()
        };

        editor.queue_map_transition(target.clone(), "等待显式进入"); // 只有清单验证完成后才能发布当前身份。

        assert_eq!(
            editor.selected_map_id.as_ref().map(MapId::as_str),
            Some("HN-01")
        ); // 登记请求本身不能让标题、草稿或删除门提前冒充目标图。
        assert_eq!(editor.map_list_selection_id.as_ref(), Some(&target)); // 目录高亮继续显示冻结的按钮目标。
        assert_eq!(editor.pending_map_transition.as_ref(), Some(&target)); // 正式切图驱动取得唯一稳定请求。
    }
}
/// 地图、文件夹与传送名称的唯一已初始化静态文本会话。
#[derive(Resource, Default)] // 首次建立焦点前不得读取空控件覆盖业务初值。
pub(crate) struct EditorStaticTextInputState {
    // 固定一个字段身份，不随地图规模增长。
    pub(crate) previous: Option<(Entity, String)>, // 实体及冻结业务身份决定何时重置官方编辑器。
} // 控件初始化与业务同步通过这一身份衔接。

pub(crate) use feathrise_editor_core::EditorTextInputGuard; // 全部作者库共享同一候选输入门。
