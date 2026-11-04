//! F12 院落组合的左栏入口、显式多选和视口输入所有权。
//!
//! 【功能设计】UI 只发布带稳定摆放 ID 的请求；作者事务在独立模块验证并写入地图。

mod scene; // 静态左栏卡片与视口框选轮廓。
mod systems; // 按钮、输入、分帧投影和文字回显。

use bevy::prelude::*; // 资源、消息与系统阶段。
use feathrise_map_content::MapId; // 选集绑定地图身份，切图不串用旧记录。

use crate::MapEditorSystemSet; // 与 F12 原有输入和回显顺序一致。

pub(crate) use scene::{
    CourtyardModeText, CourtyardNameInput, CourtyardSelectionRegion, CourtyardStatusText, courtyard_panel, selection_region,
}; // 顶部工具页复用唯一名称与反馈，不建立第二套输入。
pub(crate) use systems::viewport_input; // 插入既有 Cursor 链，在普通 Inspect 拾取前取得输入。

/// 原有单体落点系统只在院落没有武装时继续接收左键。
pub(crate) fn mode_idle(state: Res<CourtyardUiState>) -> bool {
    state.mode == CourtyardViewportMode::Idle // 状态资源是原拾取和作者落点的唯一共享门。
}

/// 院落视口工具的唯一输入语义。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CourtyardViewportMode {
    #[default]
    Idle, // 普通查看与建筑点放继续使用原入口。
    BoxSelect, // 下一次左键拖拽只选择摆放记录。
    Stamp,     // 下一次有效落点盖印选定组合。
    MoveGroup, // 下一次落点移动所选成员所属副本。
    CopyGroup, // 下一次落点复制所选成员所属副本。
}

/// 左栏保存与旋转只发送意图，不能直接改动作者草稿。
#[derive(Message, Clone, Debug)]
pub(crate) enum CourtyardUiRequest {
    SaveSelection {
        name: String,               // 已确认的显示名，作者层仍需完整校验。
        placement_ids: Vec<String>, // 固定上限的稳定记录 ID。
    },
    RotateSelection {
        placement_ids: Vec<String>, // 作者层据成员归属确认完整院落副本。
        quarter_turns: i32,         // 正数向右，负数向左，避免浮点按钮误差。
    },
}

/// 框选处理中的完整身份与固定预算扫描进度。
#[derive(Debug)]
pub(super) struct CourtyardSelectionScan {
    pub(super) map_id: MapId,               // 不允许跨图采用结果。
    pub(super) revision: u64,               // 摆放作者修改后本轮扫描必须重来。
    pub(super) camera_transform: Transform, // 视角变化使跨帧投影失效。
    pub(super) render_size: UVec2,          // 视口缩放使矩形像素基准失效。
    pub(super) min: Vec2,                   // 左上物理像素。
    pub(super) max: Vec2,                   // 右下物理像素。
    pub(super) cursor: usize,               // 下一条待判断记录。
    pub(super) ids: Vec<String>,            // 只积累上限以内的稳定身份。
}

/// 一个 F12 会话内的院落名称、选集与当前视口模式。
#[derive(Resource, Debug)]
pub(crate) struct CourtyardUiState {
    pub(crate) selected_placement_ids: Vec<String>, // 至多 256 个真实地图摆放 ID。
    pub(crate) selected_preset_id: Option<String>,  // 全局目录规范 ID，热更后仍可解析。
    pub(crate) mode: CourtyardViewportMode,         // 让原拾取与作者落点使用同一输入所有权。
    pub(crate) name: String,                        // 作者输入法确认后的院落显示名。
    pub(crate) status: String,                      // 左栏局部结果与失败原因。
    pub(crate) map_id: Option<MapId>,               // 选集与扫描的地图身份。
    pub(crate) quarter_turns: i32,                  // 当前印章的九十度朝向。
    pub(super) drag_start: Option<Vec2>,            // 归一化视口坐标，拖拽中不扫描记录。
    pub(super) drag_current: Option<Vec2>,          // 松手前可视框右下角。
    pub(super) scan: Option<CourtyardSelectionScan>, // 最多一轮分帧扫描。
}

impl Default for CourtyardUiState {
    fn default() -> Self {
        Self {
            selected_placement_ids: Vec::new(), // 初次没有跨图选择。
            selected_preset_id: None,           // 等待全局目录加载后作者显式选择。
            mode: CourtyardViewportMode::Idle,  // 首次打开不抢占普通视口输入。
            name: "新院落".to_owned(),          // 可以直接保存，也允许中文编辑。
            status: String::new(), // 首次操作教程归入院落预设的?帮助，反馈只记录实际操作。
            map_id: None,                       // 尚未绑定任何草稿地图。
            quarter_turns: 0,                   // 新印章沿预设原方向。
            drag_start: None,                   // 没有按下边沿。
            drag_current: None,                 // 不显示选择框。
            scan: None,                         // 无后台或分帧结果待采用。
        }
    }
}

impl CourtyardUiState {
    /// 全局选择模式或切离建筑页时取消院落输入，避免旧盖印继续消费视口点击。
    pub(crate) fn cancel_viewport_mode(&mut self) {
        self.mode = CourtyardViewportMode::Idle; // 归还视口输入所有权。
        self.drag_start = None; // 未结束的框选不能在切模式后继续绘制。
        self.drag_current = None; // 清除旧选框终点与画面。
        self.scan = None; // 未完成的分帧扫描不能迟到发布选择。
    }
}

/// 安装独立院落 UI，作者事务可消费同一消息与资源。
pub(crate) fn install(app: &mut App) {
    app.init_resource::<CourtyardUiState>() // 会话状态与地图作者草稿独立。
        .add_message::<CourtyardUiRequest>() // 保存和整体旋转只发结构化请求。
        .add_systems(
            Update,
            (
                systems::sync_map_identity,
                systems::read_name,
                systems::handle_buttons,
            )
                .chain()
                .in_set(MapEditorSystemSet::Input), // 同帧先取得最新中文名称再处理按钮。
        )
        .add_systems(
            Update,
            (
                systems::setup_region_pointer_passthrough,
                systems::advance_selection_scan,
            )
                .in_set(MapEditorSystemSet::Tasks), // UI 轮廓不截获指针，扫描每帧固定预算。
        )
        .add_systems(
            Update,
            (
                systems::sync_panel_text,
                systems::sync_button_styles,
                systems::sync_region,
            )
                .in_set(MapEditorSystemSet::Presentation), // 回显使用本帧最终选择和模式。
        );
}
