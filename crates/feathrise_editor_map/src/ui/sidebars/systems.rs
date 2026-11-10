//! 侧栏页签输入、工具一次性跟随、页面显隐、帮助折叠与上下文属性同步。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::EditorButtonClick;
use crate::ui::theme as colors; // 活动页签与悬停沿用统一铜金反馈。

use crate::authoring::EditorAuthoringSession; // 上下文显隐读取当前草稿中真实对象数量。
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorState}; // 页签不得穿透数值拖动，工具变化来自独立编辑器状态。
use crate::types::EditorTool; // 固定工具到左页的产品映射。
use crate::ui::gltf_browser::EditorGltfBrowserState; // 任意有效或失效网格选择都应保留属性清除入口。

use super::*; // 页签、帮助和上下文marker均由侧栏模块私有共享。

/// 返回编辑工具应定位的顶部模块；Inspect仅保留枚举回退，选择模式不触发自动切页。
const fn left_tab_for_tool(tool: EditorTool) -> EditorLeftSidebarTab {
    match tool {
        EditorTool::Inspect
        | EditorTool::Raise
        | EditorTool::VolumeDig
        | EditorTool::VolumeFill
        | EditorTool::VolumeSmooth
        | EditorTool::VolumeFlatten
        | EditorTool::Lower
        | EditorTool::Smooth
        | EditorTool::Flatten
        | EditorTool::Puddles // 积水与地表共用地形页公共笔刷参数。
        | EditorTool::Surface => EditorLeftSidebarTab::Terrain,
        EditorTool::Vegetation => EditorLeftSidebarTab::Vegetation,
        EditorTool::Water | EditorTool::RiverSpline | EditorTool::Waterfall => {
            EditorLeftSidebarTab::Water
        }
        EditorTool::Domain(id) => EditorLeftSidebarTab::Domain(id),
        EditorTool::Building => EditorLeftSidebarTab::Building,
        EditorTool::PlaceAsset | EditorTool::Portal | EditorTool::Boundary => {
            EditorLeftSidebarTab::Gameplay
        }
    }
}

/// 点击页签；帮助由独立服务提前消费，稳定工具不会参与本系统。
pub(crate) fn handle_editor_sidebar_input(
    controls: Query<(&EditorButtonClick, &MapEditorSidebarTabButton), Changed<EditorButtonClick>>,
    numeric_scrub_mouse: Res<MapEditorNumericScrubMouseGuard>, // 数值和弦release帧不得补触发页签。
    mut workspace: Option<ResMut<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
    mut editor: ResMut<MapEditorState>,
    mut state: ResMut<MapEditorSidebarState>,
) {
    if numeric_scrub_mouse.blocked() {
        return; // Changed tick仍由查询推进，本次物理点击不会在解锁后重放。
    }
    if editor.new_map_dialog_open {
        return; // 新建/资料弹框独占本帧点击，底层页签不得在关闭后补触发切模块。
    }
    for (interaction, tab_button) in &controls {
        if !interaction.is_clicked() {
            continue;
        }
        match tab_button.0 {
            EditorSidebarTab::Navigator(tab) => state.navigator = tab,
            EditorSidebarTab::Left(tab) => {
                state.left = tab;
                state.right = tab.work_page(); // 顶部模块打开独立工具栏目，不占用对象属性。
                if let EditorLeftSidebarTab::Domain(id) = tab {
                    editor.tool = EditorTool::Domain(id);
                    if let Some(workspace) = workspace.as_mut() {
                        workspace.active_tab =
                            crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Map;
                        workspace.touch_ui();
                    }
                }
            }
            EditorSidebarTab::Right(EditorRightSidebarTab::Tools) => state.right = state.left.work_page(),
            EditorSidebarTab::Right(EditorRightSidebarTab::Scene) => state.reveal_properties(),
            EditorSidebarTab::Right(tab) => state.right = tab,
        }
    }
}

/// 工具只在值真正变化的一帧定位相关页面，避免稳定工具反复抢回手动页签。
pub(crate) fn follow_editor_tool_with_sidebar(
    editor: Res<MapEditorState>,
    mut state: ResMut<MapEditorSidebarState>,
) {
    if !editor.open || editor.tool == state.observed_tool {
        return; // 用户手动换页后，稳定工具不再覆盖选择。
    }
    if crate::authoring::basic_shapes::is_cuboid_selection(&editor) {
        state.observed_tool = editor.tool;
        state.left = EditorLeftSidebarTab::Meshes;
        state.right = EditorRightSidebarTab::Tools;
        return; // 内置来源归统一网格面板，不能被普通GLB摆放自动抢到玩法/资产页。
    }
    state.observed_tool = editor.tool; // 先登记本次真实变化，后续稳定帧直接退出。
    if editor.new_map_dialog_open {
        return; // 模态期间工具变化不得把左栏从地图模块抢到地形等工具页。
    }
    if editor.tool != EditorTool::Inspect {
        state.left = left_tab_for_tool(editor.tool); // 模块记忆独立于左侧目录/场景导航。
        state.right = EditorRightSidebarTab::Tools;
    }
    if editor.tool == EditorTool::PlaceAsset {
        state.navigator = EditorNavigatorTab::Assets;
        state.reveal_asset_properties(); // 来源列表位于左栏，右侧仅保留所选文件属性和预览。
    }
}

/// 新建/资料弹框打开时冻结当前左栏页，关闭后恢复；关闭同帧禁止视口世界点选补触发。
pub(crate) fn freeze_sidebar_tab_for_new_map_dialog(
    mut editor: ResMut<MapEditorState>,
    mut state: ResMut<MapEditorSidebarState>,
    mut was_open: Local<bool>,
) {
    editor.suppress_world_pick_this_frame = false; // 每帧Input开头清掉上一帧的一次性门。
    if editor.new_map_dialog_open && !*was_open {
        state.freeze_left_for_new_map_dialog();
    } else if !editor.new_map_dialog_open && *was_open {
        state.restore_left_after_new_map_dialog();
        // 取消/保存按钮的同一次左键不得在Cursor阶段被当成视口点选并跳到玩法对象。
        editor.suppress_world_pick_this_frame = true;
    }
    *was_open = editor.new_map_dialog_open;
}

/// 页签、页面和折叠帮助的视觉与Display回显；全部实体始终保留。
pub(crate) fn sync_editor_sidebar_presentation(
    state: Res<MapEditorSidebarState>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    domain: Option<Res<crate::game_authoring::map_interaction::MapInteractionState>>,
    tool_labels: Option<Res<crate::game_authoring::DomainToolLabels>>,
    mut controls: Query<
        (
            &UiInteraction,
            Option<&MapEditorSidebarTabButton>,
            Option<&MapEditorSidebarHelpButton>,
            &Children,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Or<(
            With<MapEditorSidebarTabButton>,
            With<MapEditorSidebarHelpButton>,
        )>,
    >,
    mut texts: Query<
        (
            &mut Text,
            &mut TextColor,
            Option<&MapEditorSidebarTabText>,
            Option<&MapEditorSidebarHelpButtonText>,
        ),
        Or<(
            With<MapEditorSidebarTabText>,
            With<MapEditorSidebarHelpButtonText>,
        )>,
    >,
    mut layouts: ParamSet<(
        Query<(&MapEditorSidebarPage, &mut Node)>,
        Query<(&MapEditorSidebarHelpBody, &mut Node), Without<crate::ui::context_help::ContextHelpMounted>>,
        Query<&mut Node, With<crate::game_authoring::DomainToolsHost>>,
    )>, // 页面与帮助正文都写Node，ParamSet避免静态marker误组合时产生别名。
) {
    let locale = locale.as_deref().copied().unwrap_or_default();
    for mut node in &mut layouts.p2() {
        let active = matches!(state.left, EditorLeftSidebarTab::Domain(_));
        let display = if active { Display::Flex } else { Display::None };
        if node.display != display { node.display = display; }
        if node.flex_grow != 1.0 { node.flex_grow = 1.0; }
    }
    for (page, mut node) in &mut layouts.p0() {
        let display = if state.page_visible(page.0) {
            Display::Flex // 活动页恢复原有ScrollPosition并参与布局。
        } else {
            Display::None // 非活动页完全退出布局但实体与滚动值保留。
        };
        if node.display != display {
            node.display = display; // 稳定活动页不重复触发布局Changed。
        }
    }

    for (_, mut node) in &mut layouts.p1() {
        if node.display != Display::None {
            node.display = Display::None; // 未挂入浮层的原正文永远不参与面板布局。
        }
    }

    for (interaction, tab_button, help_button, children, mut background, mut border) in
        &mut controls
    {
        let tab_active = tab_button.is_some_and(|button| state.tab_active(button.0));
        let help_active = help_button.is_some_and(|button| state.help_expanded(button.0));
        let selected = tab_active || help_active;
        let background_color = if tab_button.is_some() {
            sidebar_tab_background(*interaction, selected)
        } else {
            // 页内帮助按钮继续沿用普通按钮底色。
            match (*interaction, selected) {
                (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
                (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
                (UiInteraction::None, true) => colors::SELECT_BG,
                (UiInteraction::None, false) => colors::BUTTON_DEV_SMALL_NORMAL,
            }
        };
        if background.0 != background_color {
            background.0 = background_color; // 只在交互或活动态真正变化时写颜色。
        }
        let border_color = BorderColor::all(if selected {
            colors::COPPER_BORDER // 活动页或已展开说明使用更清晰铜边。
        } else {
            colors::COPPER_BORDER_WEAK
        });
        if *border != border_color {
            *border = border_color; // 稳定页签不重复标记边框Changed。
        }
        for child in children.iter() {
            let Ok((mut text, mut text_color, tab_text, help_text)) = texts.get_mut(child) else {
                continue;
            };
            if tab_text.is_some() {
                if tab_button.is_some_and(|button| button.0 == EditorSidebarTab::Right(EditorRightSidebarTab::Tools)) {
                    let label = if let EditorLeftSidebarTab::Domain(id) = state.left {
                        tool_labels.as_ref().and_then(|labels| labels.0.get(id)).map(String::as_str)
                            .unwrap_or_else(|| domain.as_ref().filter(|domain| domain.domain() == id).map_or("扩展", |domain| domain.label()))
                    } else if state.left == EditorLeftSidebarTab::Meshes {
                        locale.text("sidebar.mesh")
                    } else {
                        locale.editor_label(state.left.label())
                    };
                    if text.as_str() != label {
                        **text = label.to_owned(); // 动态栏目名称只跟随创建与工具模块，不跟随地图选中对象。
                    }
                }
                let color = sidebar_tab_text_color(tab_active);
                if text_color.0 != color {
                    text_color.0 = color; // 活动文字色稳定时保持只读。
                }
            }
            if help_text.is_some()
                && let Some(help_button) = help_button
            {
                let expanded = state.help_expanded(help_button.0);
                let label = "?";
                let color = if expanded { colors::TEXT_ACCENT } else { colors::TEXT_HINT };
                if text.as_str() != label {
                    **text = label.to_owned(); // 只在展开箭头改变时更新字符串。
                }
                if text_color.0 != color {
                    text_color.0 = color; // 折叠说明保持提示色，展开后使用强调色。
                }
            }
        }
    }
}

/// 当前选择对应哪些属性分组；只用O(1)索引与Vec长度计算。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SidebarContext {
    water_count: usize,
    still_water_selected: bool,
    water_network_selected: bool,
    vegetation_count: usize,
    vegetation_selected: bool,
    placement_selected: bool,
}

/// 从作者草稿和会话选择计算上下文，不读取或写入磁盘。
fn sidebar_context(
    editor: &MapEditorState,
    session: &EditorAuthoringSession,
    browser: &EditorGltfBrowserState,
) -> SidebarContext {
    let water = session
        .draft
        .as_ref()
        .and_then(|draft| draft.water.as_ref());
    let still_count = water.map_or(0, |water| water.layers.len());
    let river_count = water.map_or(0, |water| water.rivers.len());
    let waterfall_count = water.map_or(0, |water| water.waterfalls.len());
    let vegetation_count = session
        .draft
        .as_ref()
        .and_then(|draft| draft.vegetation.as_ref())
        .and_then(|vegetation| vegetation.authoring.as_ref())
        .map_or(0, |authoring| authoring.layers.len());
    sidebar_context_from_counts(
        editor,
        [still_count, river_count, waterfall_count],
        vegetation_count,
        browser.selection.is_some(),
    )
}

/// 用已经O(1)取得的计数计算显隐，便于完整覆盖零项和三类水域选择。
fn sidebar_context_from_counts(
    editor: &MapEditorState,
    water_counts: [usize; 3], // 依次为静水、河流和瀑布数量。
    vegetation_count: usize,  // 当前作者层真实数量。
    placement_selected: bool, // 有效或失效场景树网格选择都为true。
) -> SidebarContext {
    let [still_count, river_count, waterfall_count] = water_counts;
    SidebarContext {
        water_count: still_count + river_count + waterfall_count,
        still_water_selected: editor.tool == EditorTool::Water
            && editor.selected_water_layer < still_count,
        water_network_selected: (editor.tool == EditorTool::RiverSpline
            && editor.selected_water_river < river_count)
            || (editor.tool == EditorTool::Waterfall
                && editor.selected_waterfall < waterfall_count),
        vegetation_count,
        vegetation_selected: vegetation_count > 0
            && editor.selected_vegetation_layer < vegetation_count,
        placement_selected, // 失效引用也保留清除与诊断入口。
    }
}

/// 使水域、植被和场景网格属性严格跟随当前真实选择。
pub(crate) fn sync_editor_sidebar_context(
    selection: Option<Res<crate::viewport::LayerCandidateState>>,
    editor: Res<MapEditorState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    mut previous: Local<Option<SidebarContext>>, // 视口光标高频变化不应重复写静态上下文Node。
    mut contexts: ParamSet<(
        Query<&mut Node, With<MapEditorWaterListActions>>,
        Query<&mut Node, With<MapEditorStillWaterProperties>>,
        Query<&mut Node, With<MapEditorWaterNetworkProperties>>,
        Query<&mut Node, With<MapEditorWaterSelectionDetails>>,
        Query<&mut Node, With<MapEditorVegetationListActions>>,
        Query<&mut Node, With<MapEditorVegetationSelectionProperties>>,
        Query<&mut Node, With<MapEditorPlacementSelectionProperties>>,
    )>, // ParamSet明确这些静态容器即使误挂多个marker也按顺序独占Node写访问。
) {
    let mut context = sidebar_context(&editor, &session, &browser);
    // 选择模式下工具已恢复 Inspect，属性类别仍来自正式水域身份。
    if let Some(crate::viewport::LayerTarget::Water(kind, _)) = selection.as_ref().and_then(|state| state.selected.as_ref()) {
        context.still_water_selected = *kind == crate::types::WaterAuthoringFeatureKind::StillWater;
        context.water_network_selected = !context.still_water_selected;
    }
    if *previous == Some(context) {
        return; // 只有对象数量、选择类别或场景树选择真正变化时更新七个容器。
    }
    *previous = Some(context); // 首帧None确保BSN默认Display得到一次正式收口。
    for mut node in &mut contexts.p0() {
        node.display = display_if(context.water_count > 0); // 有对象才保留分页与删除所选。
    }
    for mut node in &mut contexts.p1() {
        node.display = display_if(context.still_water_selected); // 静水只出现复制和水位。
    }
    for mut node in &mut contexts.p2() {
        node.display = display_if(context.water_network_selected); // 河流/瀑布只出现水网属性。
    }
    for mut node in &mut contexts.p3() {
        node.display = display_if(context.still_water_selected || context.water_network_selected); // 零对象或无效选择时状态与说明也不占空间。
    }
    for mut node in &mut contexts.p4() {
        node.display = display_if(context.vegetation_count > 0); // 空作者层不显示分页删除行。
    }
    for mut node in &mut contexts.p5() {
        node.display = display_if(context.vegetation_selected); // 无有效选择不显示专业属性。
    }
    for mut node in &mut contexts.p6() {
        node.display = display_if(context.placement_selected); // 没有网格选择时整个属性区收起。
    }
}

/// 布尔上下文统一转换为完全参与或退出布局的Display值。
const fn display_if(visible: bool) -> Display {
    if visible {
        Display::Flex
    } else {
        Display::None
    }
}

#[cfg(test)]
mod tests {
    use super::*; // 验证工具映射和默认页签不被初始查看工具覆盖。

    #[test]
    fn tool_mapping_matches_authoring_pages() {
        let cases = [
            // 每个工具枚举都必须具有唯一稳定的左栏自动定位目标。
            (EditorTool::Inspect, EditorLeftSidebarTab::Terrain), // 兼容枚举回退；全局选择模式实际不调用此映射切页。
            (EditorTool::Raise, EditorLeftSidebarTab::Terrain),   // 抬高属于地形塑形流程。
            (EditorTool::Lower, EditorLeftSidebarTab::Terrain),   // 降低属于地形塑形流程。
            (EditorTool::Smooth, EditorLeftSidebarTab::Terrain),  // 平滑属于地形塑形流程。
            (EditorTool::Flatten, EditorLeftSidebarTab::Terrain), // 压平属于地形塑形流程。
            (EditorTool::Surface, EditorLeftSidebarTab::Terrain), // 地表笔刷位于地形页。
            (EditorTool::Vegetation, EditorLeftSidebarTab::Vegetation), // 植被工具定位植被页。
            (EditorTool::Water, EditorLeftSidebarTab::Water),     // 静水工具定位水域页。
            (EditorTool::RiverSpline, EditorLeftSidebarTab::Water), // 河流样条定位水域页。
            (EditorTool::Waterfall, EditorLeftSidebarTab::Water), // 瀑布工具定位水域页。
            (EditorTool::Domain("test.domain"), EditorLeftSidebarTab::Domain("test.domain")), // 地图角色定位角色页。
            (EditorTool::PlaceAsset, EditorLeftSidebarTab::Gameplay), // 资产摆放定位玩法页。
            (EditorTool::Portal, EditorLeftSidebarTab::Gameplay), // 传送点定位玩法页。
            (EditorTool::Boundary, EditorLeftSidebarTab::Gameplay), // 区域边界定位玩法页。
        ];
        for (tool, expected) in cases {
            // 表驱动检查避免新增枚举只覆盖一条代表路径。
            assert_eq!(left_tab_for_tool(tool), expected); // 自动定位规则必须完整匹配产品分组。
        }
    }

    #[test]
    fn default_observed_tool_keeps_initial_map_page() {
        let state = MapEditorSidebarState::default();
        assert_eq!(state.observed_tool, EditorTool::Inspect);
        assert_eq!(state.left, EditorLeftSidebarTab::Meshes);
    }

    #[test]
    fn stable_tool_does_not_steal_a_manual_tab_and_place_asset_opens_both_targets() {
        let mut app = App::new();
        app.init_resource::<MapEditorSidebarState>()
            .init_resource::<MapEditorState>()
            .add_systems(Update, follow_editor_tool_with_sidebar);
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.open = true;
            editor.tool = EditorTool::Raise; // 第一次真实变化应定位地形页。
        }
        app.update();
        assert_eq!(
            app.world().resource::<MapEditorSidebarState>().left,
            EditorLeftSidebarTab::Terrain,
        );

        app.world_mut().resource_mut::<MapEditorSidebarState>().navigator = EditorNavigatorTab::Scene;
        app.world_mut().resource_mut::<MapEditorSidebarState>().right = EditorRightSidebarTab::Environment;
        app.update();
        assert_eq!(
            app.world().resource::<MapEditorSidebarState>().left,
            EditorLeftSidebarTab::Terrain,
        ); // 切导航与环境不丢失当前模块，稳定工具也不会抢回工具页。
        assert_eq!(app.world().resource::<MapEditorSidebarState>().navigator, EditorNavigatorTab::Scene);
        assert_eq!(app.world().resource::<MapEditorSidebarState>().right, EditorRightSidebarTab::Environment);

        app.world_mut().resource_mut::<MapEditorState>().tool = EditorTool::PlaceAsset;
        app.update();
        let state = app.world().resource::<MapEditorSidebarState>();
        assert_eq!(state.left, EditorLeftSidebarTab::Gameplay);
        assert_eq!(state.right, EditorRightSidebarTab::Assets);
    }

    #[test]
    fn cancel_new_map_dialog_restores_map_tab_instead_of_jumping_to_terrain() {
        let mut app = App::new();
        app.init_resource::<MapEditorSidebarState>()
            .init_resource::<MapEditorState>()
            .add_systems(
                Update,
                (
                    follow_editor_tool_with_sidebar,
                    freeze_sidebar_tab_for_new_map_dialog,
                )
                    .chain(),
            );
        {
            let mut editor = app.world_mut().resource_mut::<MapEditorState>();
            editor.open = true;
            editor.tool = EditorTool::Inspect;
            editor.new_map_dialog_open = true; // 打开新建弹框时应冻结当前地图模块。
        }
        app.update();
        assert_eq!(
            app.world().resource::<MapEditorSidebarState>().left,
            EditorLeftSidebarTab::Meshes,
        );
        // 模态期间即使工具变成抬高，也不得把左栏抢到地形。
        app.world_mut().resource_mut::<MapEditorState>().tool = EditorTool::Raise;
        app.update();
        assert_eq!(
            app.world().resource::<MapEditorSidebarState>().left,
            EditorLeftSidebarTab::Meshes,
        );
        // 取消/保存关闭弹框后必须回到打开前的地图模块，且同帧禁止世界点选。
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .new_map_dialog_open = false;
        app.update();
        assert_eq!(
            app.world().resource::<MapEditorSidebarState>().left,
            EditorLeftSidebarTab::Meshes,
        );
        assert!(
            app.world()
                .resource::<MapEditorState>()
                .suppress_world_pick_this_frame
        );
        app.update(); // 下一帧解除一次性门。
        assert!(
            !app.world()
                .resource::<MapEditorState>()
                .suppress_world_pick_this_frame
        );
    }

    #[test]
    fn display_helper_fully_removes_hidden_context() {
        assert_eq!(display_if(false), Display::None);
        assert_eq!(display_if(true), Display::Flex);
    }

    #[test]
    fn context_distinguishes_zero_still_river_waterfall_and_selection_validity() {
        let mut editor = MapEditorState::default();
        let empty = sidebar_context_from_counts(&editor, [0, 0, 0], 0, false);
        assert_eq!(empty.water_count, 0);
        assert!(!empty.still_water_selected && !empty.water_network_selected);
        assert!(!empty.vegetation_selected && !empty.placement_selected);

        editor.tool = EditorTool::Water;
        assert!(sidebar_context_from_counts(&editor, [1, 1, 1], 1, true).still_water_selected);
        editor.tool = EditorTool::RiverSpline;
        assert!(sidebar_context_from_counts(&editor, [1, 1, 1], 1, true).water_network_selected);
        editor.tool = EditorTool::Waterfall;
        let waterfall = sidebar_context_from_counts(&editor, [1, 1, 1], 1, true);
        assert!(waterfall.water_network_selected);
        assert!(waterfall.vegetation_selected); // 有效第零层允许显示植被选择属性。
        assert!(waterfall.placement_selected); // 失效绑定也由调用方以已有选择传入true。

        editor.selected_waterfall = 1;
        assert!(!sidebar_context_from_counts(&editor, [1, 1, 1], 0, false).water_network_selected); // 越界索引不伪造瀑布属性上下文。
    }

    #[test]
    fn tab_click_switches_visibility_and_preserves_each_page_scroll_position() {
        let mut app = App::new();
        app.init_resource::<MapEditorSidebarState>()
            .init_resource::<MapEditorNumericScrubMouseGuard>()
            .init_resource::<MapEditorState>()
            .add_systems(
                Update,
                (
                    handle_editor_sidebar_input,
                    sync_editor_sidebar_presentation,
                )
                    .chain(),
            );
        let map_page = app
            .world_mut()
            .spawn((
                MapEditorSidebarPage(EditorSidebarTab::Navigator(EditorNavigatorTab::Maps)),
                Node::default(),
                ScrollPosition(Vec2::new(0.0, 12.0)),
            ))
            .id();
        let terrain_page = app
            .world_mut()
            .spawn((
                MapEditorSidebarPage(EditorSidebarTab::Left(EditorLeftSidebarTab::Terrain)),
                Node::default(),
                ScrollPosition(Vec2::new(0.0, 34.0)),
            ))
            .id();
        let terrain_tab = app
            .world_mut()
            .spawn((
                UiInteraction::None,
                MapEditorSidebarTabButton(EditorSidebarTab::Left(EditorLeftSidebarTab::Terrain)),
                BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                BorderColor::all(colors::COPPER_BORDER_WEAK),
            ))
            .with_child((
                Text::new("地形"),
                TextColor(colors::TEXT_PRIMARY),
                MapEditorSidebarTabText,
            ))
            .id();
        let water_help_body = app
            .world_mut()
            .spawn((
                MapEditorSidebarHelpBody(EditorSidebarHelp::Water),
                Node::default(),
            ))
            .id();
        let water_help_button = app
            .world_mut()
            .spawn((
                UiInteraction::None,
                MapEditorSidebarHelpButton(EditorSidebarHelp::Water),
                BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL),
                BorderColor::all(colors::COPPER_BORDER_WEAK),
            ))
            .with_child((
                Text::new("操作说明 ▸"),
                TextColor(colors::TEXT_HINT),
                MapEditorSidebarHelpButtonText,
            ))
            .id();

        app.update(); // 首帧应用地图默认页与全部帮助折叠状态。
        assert_eq!(
            app.world().get::<Node>(map_page).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(terrain_page).unwrap().display,
            Display::None
        );
        assert_eq!(
            app.world().get::<Node>(water_help_body).unwrap().display,
            Display::None
        );
        let terrain_tab_text = app // 读取生产层级中页签的唯一文字子实体。
            .world()
            .get::<Children>(terrain_tab)
            .and_then(|children| children.first().copied())
            .unwrap();
        assert_eq!(
            app.world().get::<BackgroundColor>(terrain_tab).unwrap().0,
            colors::PANEL_BG_STRONG,
        ); // 非活动一级页签保持深墨分类底色。
        assert_eq!(
            *app.world().get::<BorderColor>(terrain_tab).unwrap(),
            BorderColor::all(colors::COPPER_BORDER_WEAK),
        ); // 非活动一级页签使用弱铜边。
        assert_eq!(
            app.world().get::<TextColor>(terrain_tab_text).unwrap().0,
            colors::TEXT_TAB_INACTIVE,
        ); // 非活动页签文字不得与普通二级工具同亮度。

        *app.world_mut().get_mut::<UiInteraction>(terrain_tab).unwrap() = UiInteraction::Hovered;
        app.update(); // 悬停只改变一级页签反馈，不提前切换页面。
        assert_eq!(
            app.world().get::<BackgroundColor>(terrain_tab).unwrap().0,
            colors::SELECT_BG_HOVER,
        ); // 一级页签悬停使用铜墨选择色。
        assert_eq!(
            app.world().resource::<MapEditorSidebarState>().left,
            EditorLeftSidebarTab::Meshes,
        ); // Hovered不能改变默认地图页。

        app.world_mut().entity_mut(terrain_tab).insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.update(); // 模拟用户手动点击地形页签。
        assert_eq!(
            app.world().resource::<MapEditorSidebarState>().left,
            EditorLeftSidebarTab::Terrain,
        );
        assert_eq!(
            app.world().get::<Node>(map_page).unwrap().display,
            Display::Flex
        );
        assert_eq!(
            app.world().get::<Node>(terrain_page).unwrap().display,
            Display::Flex
        );
        assert_eq!(app.world().get::<ScrollPosition>(map_page).unwrap().y, 12.0); // 隐藏页保留自己的旧位置。
        assert_eq!(
            app.world().get::<ScrollPosition>(terrain_page).unwrap().y,
            34.0
        ); // 新活动页恢复自己的旧位置。

        *app.world_mut().get_mut::<UiInteraction>(terrain_tab).unwrap() = UiInteraction::None;
        app.update(); // 松开后验证稳定活动态，而非瞬时Pressed颜色。
        assert_eq!(
            app.world().get::<BackgroundColor>(terrain_tab).unwrap().0,
            colors::SELECT_BG,
        ); // 活动一级页签保留稳定选择底色。
        assert_eq!(
            *app.world().get::<BorderColor>(terrain_tab).unwrap(),
            BorderColor::all(colors::COPPER_BORDER),
        ); // 28px按钮的2px底边由强铜色形成活动下划线。
        assert_eq!(
            app.world().get::<TextColor>(terrain_tab_text).unwrap().0,
            colors::TEXT_TAB_ACTIVE,
        ); // 活动页签使用强调文字色。

        app.world_mut().entity_mut(water_help_button)
            .insert((UiInteraction::Pressed, feathrise_editor_ui::EditorButtonClick::triggered()));
        app.update(); // 侧栏不再就地展开帮助；独立根浮层服务处理问号，不影响正文高度。
        assert_eq!(
            app.world().get::<Node>(water_help_body).unwrap().display,
            Display::None
        );
        assert!(!
            app.world()
                .resource::<MapEditorSidebarState>()
                .help_expanded(EditorSidebarHelp::Water)
        );
    }
}
