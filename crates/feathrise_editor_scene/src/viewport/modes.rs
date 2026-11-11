//! 视口级选择、三类变换与编辑模式；只改变输入意图，不改变作者层数据或撤销历史。

use crate::SceneEditorSystemSet;
use crate::resources::SceneEditorState; // 实际工具始终由模式门统一收口。
use crate::types::EditorTool; // 复用现有工具与各笔划的切换收尾路径。
use crate::ui::courtyard::{CourtyardUiState, CourtyardViewportMode}; // 院落占用编辑视口时暂停单体工具。
use crate::ui::sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState}; // 按作者模块恢复最近工具。
use bevy::prelude::*; // 模式资源与固定UI系统注册。 // 模式在输入之后、射线之前收口。

mod ui; // 固定左上角模式和类别筛选按钮。
mod mesh_edit_ui; // 网格拓扑编辑与普通作者画笔使用独立模式及意图。
pub(crate) use mesh_edit_ui::MeshEditToolbarRoot;
pub(crate) use ui::ControlsRoot as ViewportControlsRoot; // 手柄按本帧真实控件矩形阻止初次按下穿透。
pub(crate) use ui::{GlobalModeToolbar as ViewportModeMenuRoot, ModeToolsRoot as ViewportModeToolsRoot};
pub(crate) use ui::global_mode_toolbar;
pub(crate) use ui::{MODE_BUTTON_SIZE, MODE_ICON_SIZE, MODE_RAIL_WIDTH, mode_icon_path};
pub(crate) use ui::controls_visible;
#[cfg(any(test, feature = "validation"))]
pub(crate) fn validation_press_select(world: &mut World) -> Result<Entity, String> {
    validation_press_mode(world, ViewportMode::Select)
}
#[cfg(any(test, feature = "validation"))]
pub(crate) fn validation_press_mode(world: &mut World, mode: ViewportMode) -> Result<Entity, String> {
    let entity = world.query::<(Entity, &ui::ModeButton)>().iter(world)
        .find_map(|(entity, button)| matches!(button, ui::ModeButton::Mode(value) if *value == mode).then_some(entity))
        .ok_or("原视口模式按钮尚未建立")?;
    let mut nodes = world.query::<(&Node, Option<&ChildOf>)>();
    if !ui::controls_visible(entity, &nodes.query(world)) {
        return Err("原视口模式按钮不可见".into());
    }
    // 验收仍由原按钮输入与模式门收口；不能只改tool后被Edit记忆在下一帧改回笔刷。
    world.entity_mut(entity).insert(feathrise_editor_ui::EditorButtonClick::triggered());
    Ok(entity)
}
#[cfg(test)]
pub fn install_snapshot_controls(app: &mut App) {
    app.init_resource::<mesh_edit_ui::MeshAdvancedMenuState>()
        .add_systems(Update, (ui::setup, ui::style, mesh_edit_ui::style).chain());
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewportMode {
    #[default]
    Select, // 默认只选择作者层，绝不落笔。
    Move, // 保持已选完整资产并拖动世界轴/XZ平面，与绘制模式独立高亮。
    Rotate, // 绕世界Y轴旋转，建筑保留90°档位。
    Scale, // 普通资产等比缩放，保持已有三轴比例。
    Edit, // 显式进入后恢复当前模块最近工具。
}

impl ViewportMode {
    pub fn transforms_placement(self) -> bool {
        matches!(self, Self::Move | Self::Rotate | Self::Scale)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionFilter {
    #[default]
    All, // 所有可见类别参与同位置候选。
    Surface,    // 只选择地表作者层。
    Vegetation, // 只选择植被作者层。
    Domain,     // 只选择当前已装配应用的作者层。
    Water,      // 静水、河流与瀑布共同进入水体候选。
    Placements, // 单独摆放的GLB实例，按实例ID与刷制植被区分。
    Gameplay,   // 传送点与区域边界共用玩法对象筛选。
}

#[derive(Resource)]
pub struct ViewportModeState {
    pub mode: ViewportMode,          // 全视口唯一操作模式。
    pub filter: SelectionFilter,     // 只限制选择，不修改图层显隐。
    pub pointer_over_controls: bool, // 浮动控件拥有鼠标时禁止穿透场景。
    pub context_controls_open: bool, // 筛选与批量操作显式展开，默认不遮挡场景画面。
    remembered: [EditorTool; 7],            // 六类作者工具各自记忆；场景和网格共用只读槽，不随场景规模增长。
    observed_tab: EditorLeftSidebarTab,     // 手动切换模块时恢复该模块自己的工具。
    was_open: bool,                         // 每次重新打开编辑器都回到安全选择模式。
}

impl Default for ViewportModeState {
    fn default() -> Self {
        Self {
            mode: ViewportMode::Select,   // 初次进入必须安全只读。
            filter: SelectionFilter::All, // 初次点选允许跨类别候选。
            pointer_over_controls: false, // 未生成UI时没有额外指针占用。
            context_controls_open: false,
            remembered: [
                EditorTool::Inspect,
                EditorTool::Raise,
                EditorTool::Inspect,
                EditorTool::Vegetation,
                EditorTool::Water,
                EditorTool::Building,
                EditorTool::PlaceAsset,
            ], // 固定模块提供首次编辑的合理默认工具。
            observed_tab: EditorLeftSidebarTab::Scene, // 与侧栏默认入口一致。
            was_open: false,              // 首次打开同样走安全模式收口。
        }
    }
}

impl ViewportModeState {
    pub fn selection_allowed(&self, _editor: &SceneEditorState) -> bool {
        self.mode == ViewportMode::Select
    } // 【需求设计】先选择再变换；移动、旋转和缩放只操作既有目标，空处或其他物体不能换选。

    pub fn selection_filter(&self, _editor: &SceneEditorState) -> SelectionFilter {
        self.filter
    } // 变换不发起拾取，也不改变已选对象及其原筛选身份。

    pub fn remember_tool(&mut self, tool: EditorTool) {
        let slot = match tool {
            EditorTool::Inspect => return, // 选择模式不覆盖编辑工具记忆。
            EditorTool::Raise
            | EditorTool::VolumeDig
            | EditorTool::VolumeFill
            | EditorTool::VolumeSmooth
            | EditorTool::VolumeFlatten
            | EditorTool::Lower
            | EditorTool::Smooth
            | EditorTool::Flatten
            | EditorTool::Puddles // 地形页记忆独立积水工具。
            | EditorTool::Surface => 1, // 地形与地表共享侧栏。
            EditorTool::Domain(_) => 2,   // 外部作者的子工具由实际消费者会话保留。
            EditorTool::Vegetation => 3,   // 植被参数仍由原会话保留。
            EditorTool::Water | EditorTool::RiverSpline | EditorTool::Waterfall => 4, // 水域内部工具独立记忆。
            EditorTool::Building => 5, // 建筑点放拥有独立于普通资产的预设和校验状态。
            EditorTool::PlaceAsset | EditorTool::Portal | EditorTool::Boundary => 6, // 玩法工具独立记忆。
        };
        if self.remembered[slot] != tool {
            self.remembered[slot] = tool; // 只更新当前请求类别，不覆盖其他模块。
        }
    }
}

fn tab_slot(tab: EditorLeftSidebarTab) -> usize {
    match tab {
        EditorLeftSidebarTab::Scene | EditorLeftSidebarTab::Meshes | EditorLeftSidebarTab::Environment => 0, // 目录、网格和环境都没有绘制工具。
        EditorLeftSidebarTab::Terrain => 1,    // 地形与地表属于同一页。
        EditorLeftSidebarTab::Domain(_) => 2, // 当前固定外部作者的工具记忆槽。
        EditorLeftSidebarTab::Vegetation => 3, // 植被工具记忆槽。
        EditorLeftSidebarTab::Water => 4,      // 水域工具记忆槽。
        EditorLeftSidebarTab::Building => 5,   // 建筑预设点放记忆槽。
        EditorLeftSidebarTab::Gameplay => 6,   // 玩法工具记忆槽。
    }
}

pub fn sync_mode(
    mut mode: ResMut<ViewportModeState>, // 读取按钮请求并保存工具记忆。
    mut sidebar: ResMut<SceneEditorSidebarState>, // 当前模块是恢复目标。
    mut editor: ResMut<SceneEditorState>,  // 收口所有侧栏工具请求。
    mut courtyard: ResMut<CourtyardUiState>, // 院落工具只能在建筑页编辑模式持有视口。
    mesh_edit: Option<Res<crate::authoring::mesh_edit::MeshEditState>>,
) {
    if !editor.open {
        if mode.was_open {
            mode.was_open = false;
        } // 关闭只清会话边沿，不丢失各模块工具记忆。
        return;
    } // 关闭时不干预编辑状态。
    if !mode.was_open {
        mode.mode = ViewportMode::Select; // 每次打开都必须显式进入编辑，避免沿用上次绘制状态。
        mode.context_controls_open = false;
        editor.placement_transform_enabled = false; // 手柄同样不能跨关闭隐式重新武装。
        mode.was_open = true; // 后续稳定打开帧尊重模式按钮。
    }
    if mesh_edit.as_ref().is_some_and(|state| state.active()) {
        // 【需求设计】点边面编辑全程锁定原对象；它不是既有画笔 Edit 模式，任何侧栏工具不得抢走输入。
        mode.mode = ViewportMode::Select;
        mode.context_controls_open = false;
        editor.placement_transform_enabled = false;
        editor.tool = EditorTool::Inspect;
        sidebar.observed_tool = EditorTool::Inspect;
        mode.observed_tab = sidebar.left;
        if courtyard.mode != CourtyardViewportMode::Idle { courtyard.cancel_viewport_mode(); }
        return;
    }
    if courtyard.mode != CourtyardViewportMode::Idle
        && (mode.mode != ViewportMode::Edit || sidebar.left != EditorLeftSidebarTab::Building)
    {
        courtyard.cancel_viewport_mode(); // 作者主动选择或切页后，旧院落落点不得继续响应。
        courtyard.status = "已取消院落视口工具".to_owned(); // 左栏明确反馈取消结果。
    }
    mode.remember_tool(editor.tool); // 先保存本帧点击的工具，再应用全局模式门。
    if mode.mode.transforms_placement()
        && (mode.observed_tab != sidebar.left || editor.tool != EditorTool::Inspect)
    {
        mode.mode = ViewportMode::Edit;
    } // 显式切模块或作者工具时结束变换，回显当前模块编辑模式。
    let transforming = mode.mode.transforms_placement();
    if editor.placement_transform_enabled != transforming {
        editor.placement_transform_enabled = transforming;
    } // 【需求设计】模式互斥，三类手柄共用开关和鼠标槽，不能同时启用或与编辑模式同时高亮。
    let tool = if mode.mode != ViewportMode::Edit
        || courtyard.mode != CourtyardViewportMode::Idle
        || (matches!(sidebar.left, EditorLeftSidebarTab::Meshes | EditorLeftSidebarTab::Environment)
            && !(sidebar.left == EditorLeftSidebarTab::Meshes && crate::authoring::basic_shapes::is_cuboid_selection(&editor)))
    {
        EditorTool::Inspect // 网格仅接受明确武装的内置形状点放；普通旧画笔仍不能泄漏。
    } else if crate::authoring::basic_shapes::is_cuboid_selection(&editor) {
        EditorTool::PlaceAsset // 点放由统一网格面板显式武装，不能因Meshes共享只读记忆槽改回Inspect。
    } else if editor.tool == EditorTool::Inspect || mode.observed_tab != sidebar.left {
        mode.remembered[tab_slot(sidebar.left)] // 显式进入编辑或切模块时恢复该模块工具。
    } else {
        editor.tool // 同模块新工具请求原样保留。
    };
    if editor.tool != tool {
        editor.tool = tool;
    } // 稳定帧不触发工具变化。
    if sidebar.observed_tool != tool {
        sidebar.observed_tool = tool; // 模式门产生的工具变化已处理，下一帧不能把手动切页抢回旧模块。
    }
    if mode.observed_tab != sidebar.left {
        mode.observed_tab = sidebar.left;
    } // 记录当前模块，避免每帧重设工具。
}

pub fn block_overlay_cursor(
    mode: Res<ViewportModeState>, // 当前浮层拥有指针时场景射线必须失效。
    mut editor: ResMut<SceneEditorState>, // 清除本帧射线，所有绘制入口共用此门。
) {
    if mode.pointer_over_controls {
        editor.cursor_world = None; // 选择/放置不读取浮层背后的世界落点。
        editor.cursor_triangle = None; // 建筑联合整平不能穿透浮层冻结背后的三角。
        editor.brush_cursor_world = None; // 刷子不在按钮背后绘制。
        editor.brush_cursor_triangle = None; // 精确三角身份与有效笔刷同步清理。
    }
}

pub fn install(app: &mut App) {
    app.init_resource::<ViewportModeState>() // 独立编辑器启动即有安全默认模式。
        .init_resource::<mesh_edit_ui::MeshAdvancedMenuState>()
        .add_systems(Update, ui::setup.in_set(SceneEditorSystemSet::Tasks)) // 视口节点建立后只生成一次固定控件。
        .add_systems(Update, (mesh_edit_ui::input, ui::mesh_tool_input).chain()
            .after(feathrise_editor_ui::EditorUiSystemSet::NumericInput)
            .before(crate::authoring::mesh_edit::MeshEditInput)) // 网格元素及竖栏工具意图在核心输入前同帧收口。
        .add_systems(
            Update,
            (ui::input, sync_mode)
                .chain()
                .after(SceneEditorSystemSet::Input)
                .before(SceneEditorSystemSet::Tasks),
        ) // 输入模式切换与工具门先于预览任务、命中和任何绘制。
        .add_systems(Update, (ui::style, mesh_edit_ui::style).in_set(SceneEditorSystemSet::Presentation)); // 最终模式回显为持续铜金高亮。
}

#[cfg(test)]
mod tests {
    use super::*; // 真实同步系统验证跨模块记忆与选择模式写入门。

    #[test]
    fn mesh_element_edit_locks_ordinary_transform_and_sculpt_modes_to_inspect() {
        let mut app = App::new();
        app.insert_resource(SceneEditorState { open: true, tool: EditorTool::VolumeFill, placement_transform_enabled: true, ..default() })
            .init_resource::<ViewportModeState>()
            .init_resource::<SceneEditorSidebarState>()
            .init_resource::<CourtyardUiState>()
            .init_resource::<crate::authoring::mesh_edit::MeshEditState>()
            .add_systems(Update, sync_mode);
        app.world_mut().resource_mut::<crate::authoring::mesh_edit::MeshEditState>().activate_for_test();
        app.update();
        for requested in [ViewportMode::Edit, ViewportMode::Move, ViewportMode::Rotate, ViewportMode::Scale] {
            app.world_mut().resource_mut::<ViewportModeState>().mode = requested;
            app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::VolumeFill;
            app.update();
            assert!(app.world().resource::<crate::authoring::mesh_edit::MeshEditState>().active());
            assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Select);
            assert_eq!(app.world().resource::<SceneEditorState>().tool, EditorTool::Inspect);
            assert!(!app.world().resource::<SceneEditorState>().placement_transform_enabled);
        }
    }

    #[test]
    fn explicit_move_has_its_own_mode_and_other_tools_end_it() {
        let mut app = App::new();
        app.insert_resource(SceneEditorState { open: true, ..default() })
            .init_resource::<ViewportModeState>().init_resource::<SceneEditorSidebarState>()
            .init_resource::<CourtyardUiState>().add_systems(Update, sync_mode);
        app.update(); // 首次打开的安全选择门已经完成。
        app.world_mut().resource_scope(|world, mut editor: Mut<SceneEditorState>| {
            let mut mode = world.resource_mut::<ViewportModeState>();
            crate::viewport::placement_move::toggle_placement_move(&mut editor, Some(&mut mode));
        });
        app.update();
        assert!(app.world().resource::<SceneEditorState>().placement_transform_enabled);
        assert_eq!(app.world().resource::<SceneEditorState>().tool, EditorTool::Inspect);
        assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Move);
        app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::PlaceAsset;
        // 正式宿主会在工具变化时定位模块；单系统夹具显式复现同一前置状态。
        app.world_mut().resource_mut::<SceneEditorSidebarState>().left = EditorLeftSidebarTab::Gameplay;
        app.update();
        assert!(!app.world().resource::<SceneEditorState>().placement_transform_enabled);
        assert_eq!(app.world().resource::<SceneEditorState>().tool, EditorTool::PlaceAsset);
        assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Edit);
    }

    #[test]
    fn selection_blocks_requested_tools_and_edit_restores_per_module() {
        let mut app = App::new(); // 仅测试固定会话，不启动渲染。
        app.init_resource::<ViewportModeState>()
            .init_resource::<SceneEditorSidebarState>()
            .init_resource::<SceneEditorState>()
            .init_resource::<CourtyardUiState>() // 模式门同时读取院落视口所有权。
            .add_systems(Update, sync_mode); // 使用真实模式门。
        app.world_mut().resource_mut::<SceneEditorState>().open = true; // 模拟打开编辑器。
        app.world_mut().resource_mut::<SceneEditorSidebarState>().left =
            EditorLeftSidebarTab::Terrain; // 地形页选择平滑工具。
        app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::Smooth; // 侧栏工具不能擅自进入编辑模式。
        app.update(); // 应先记录平滑，再保持只读。
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Inspect
        ); // 选择模式无写入工具。
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit; // 明确进入编辑模式。
        app.update(); // 恢复地形最近工具。
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Smooth
        ); // 保留原平滑偏好。
        app.world_mut().resource_mut::<SceneEditorSidebarState>().left =
            EditorLeftSidebarTab::Vegetation; // 切植被不沿用地形工具。
        app.update(); // 当前模块恢复植被绘制。
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Vegetation
        ); // 植被工具独立。
        app.world_mut().resource_mut::<SceneEditorSidebarState>().left =
            EditorLeftSidebarTab::Terrain; // 再次回到地形。
        app.update(); // 仍应保留平滑。
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Smooth
        ); // 跨模块记忆不会丢失。
        app.world_mut().resource_mut::<SceneEditorSidebarState>().left = EditorLeftSidebarTab::Environment;
        app.update();
        assert_eq!(app.world().resource::<SceneEditorState>().tool, EditorTool::Inspect);
        assert_eq!(app.world().resource::<ViewportModeState>().mode, ViewportMode::Edit);
        // 环境只有参数，不能借“编辑”模式武装上一模块画笔；返回地形仍恢复原平滑工具。
        app.world_mut().resource_mut::<SceneEditorSidebarState>().left = EditorLeftSidebarTab::Terrain;
        app.update();
        assert_eq!(app.world().resource::<SceneEditorState>().tool, EditorTool::Smooth);
        app.world_mut().resource_mut::<SceneEditorState>().open = false; // 模拟结束编辑会话。
        app.update(); // 登记关闭边沿。
        app.world_mut().resource_mut::<SceneEditorState>().open = true; // 下次打开不沿用写入模式。
        app.update(); // 必须回到安全选择模式。
        assert_eq!(
            app.world().resource::<ViewportModeState>().mode,
            ViewportMode::Select
        ); // 每次重开都显式选择模式。
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Inspect
        ); // 不会落下上次工具的一笔。
    }

    #[test]
    fn courtyard_stamp_stays_in_edit_mode_without_activating_single_building() {
        let mut app = App::new(); // 只运行真实模式门以隔离两种视口工具的输入所有权。
        app.init_resource::<ViewportModeState>()
            .init_resource::<SceneEditorSidebarState>()
            .init_resource::<SceneEditorState>()
            .init_resource::<CourtyardUiState>()
            .add_systems(Update, sync_mode); // 不依赖渲染或场景资产即可检查模式收口。
        app.world_mut().resource_mut::<SceneEditorState>().open = true; // 模拟打开 F12。
        app.world_mut().resource_mut::<SceneEditorState>().tool = EditorTool::Building; // 记住建筑页原单栋工具。
        app.world_mut().resource_mut::<SceneEditorSidebarState>().left =
            EditorLeftSidebarTab::Building; // 院落按钮属于建筑页。
        app.update(); // 首帧安全选择模式先保存建筑工具记忆。
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Edit; // 盖印应属于编辑模式。
        app.world_mut().resource_mut::<CourtyardUiState>().mode = CourtyardViewportMode::Stamp; // 武装院落印章。
        app.update(); // 模式门暂停单栋点放但保留编辑模式高亮。
        assert_eq!(
            app.world().resource::<ViewportModeState>().mode,
            ViewportMode::Edit
        ); // 点击盖印后不能回显选择模式。
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Inspect
        ); // 单栋工具不能抢同一落点。
        assert_eq!(
            app.world().resource::<CourtyardUiState>().mode,
            CourtyardViewportMode::Stamp
        ); // 院落印章继续持有视口。
        app.world_mut()
            .resource_mut::<CourtyardUiState>()
            .cancel_viewport_mode(); // 模拟取消印章。
        app.update(); // 恢复建筑页先前的单栋工具。
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Building
        ); // 取消后仍可普通建筑点放。
        app.world_mut().resource_mut::<CourtyardUiState>().mode = CourtyardViewportMode::Stamp; // 再次武装印章。
        app.update(); // 确保重新进入院落输入状态。
        app.world_mut().resource_mut::<ViewportModeState>().mode = ViewportMode::Select; // 作者主动点击全局选择。
        app.update(); // 选择模式必须同时取消未完成的院落输入。
        assert_eq!(
            app.world().resource::<CourtyardUiState>().mode,
            CourtyardViewportMode::Idle
        ); // 后续左键只能选择对象。
        assert_eq!(
            app.world().resource::<SceneEditorState>().tool,
            EditorTool::Inspect
        ); // 全局选择保持只读工具。
    }

    #[test]
    fn controls_do_not_leave_a_world_hit_behind() {
        let mut app = App::new(); // 仅验证浮层对所有共享落点的统一门。
        app.init_resource::<ViewportModeState>()
            .init_resource::<SceneEditorState>()
            .add_systems(Update, block_overlay_cursor); // 使用真实模式浮层阻挡系统。
        app.world_mut()
            .resource_mut::<ViewportModeState>()
            .pointer_over_controls = true; // 鼠标进入模式按钮。
        {
            let mut editor = app.world_mut().resource_mut::<SceneEditorState>(); // 模拟相机刚算出的背后世界命中。
            editor.cursor_world = Some(Vec3::ONE); // 普通选择/放置来源。
            editor.cursor_triangle = Some(0); // 普通建筑整平的真实表层来源。
            editor.brush_cursor_world = Some(Vec3::ONE); // 绘制来源。
            editor.brush_cursor_triangle = Some(0); // 地形精确命中来源。
        }
        app.update(); // 浮层截断本帧场景输入。
        let editor = app.world().resource::<SceneEditorState>(); // 检查三个共享来源均清除。
        assert!(
            editor.cursor_world.is_none()
                && editor.cursor_triangle.is_none()
                && editor.brush_cursor_world.is_none()
                && editor.brush_cursor_triangle.is_none()
        ); // 选择和编辑模式按钮都不能穿透落笔。
    }
}
