//! 三维视口左侧的竖向模式图标和独立筛选菜单；原位更新样式，避免悬停重建导致闪烁。

use feathrise_editor_ui::InteractionButton;
use feathrise_editor_ui::EditorButtonClick;
use crate::authoring::mesh_edit::{MeshEditAction, MeshEditState, MeshEditTool};
use super::{SelectionFilter, ViewportMode, ViewportModeState};
use crate::components::MapEditorViewport; // 控件只附着永久地图视口。
use crate::resources::{MapEditorNumericScrubMouseGuard, MapEditorState}; // 模态与拖动期间拒绝切模式。
use bevy::prelude::*; // 原生UI节点与交互。
use bevy::ui::{FocusPolicy, RelativeCursorPosition}; // 浮动按钮阻断底层点击。
use crate::ui::theme as colors; // 沿用编辑器当前中性灰主题与弱分隔。
use crate::ui::scene::FONT_PATH;

const COMPACT_BUTTON_WIDTH: f32 = 40.0; // 两字标签在12px字体下保留边距，统一收紧筛选和批量按钮。
const MODE_BUTTON_SIZE: f32 = 30.0; // 参考传统编辑器的窄图标栏，不再用五个横排文字按钮占顶行。
const MODE_RAIL_WIDTH: f32 = MODE_BUTTON_SIZE + 6.0; // 两侧各2px内边距与1px弱边。

fn preferred_controls_width(mode: ViewportMode) -> f32 {
    let content = match mode {
        ViewportMode::Select => 7.0 * COMPACT_BUTTON_WIDTH + 6.0 * 3.0,
        ViewportMode::Edit => 68.0 + 3.0 * 48.0 + 3.0 * 3.0, // 包含体素笔刷类型行。
        _ => 3.0 * COMPACT_BUTTON_WIDTH + 68.0 + 3.0 * 3.0, // 变换模式只为四个对象操作保留宽度。
    };
    // 两侧各4px内边距和1px边框；额外1px吸收非整数UI/DPI倍率下的宽度舍入。
    content + 2.0 * (4.0 + 1.0) + 1.0
}

#[derive(Component, Clone, Copy)]
pub(super) enum ModeButton {
    Mode(ViewportMode),      // 选择、三类变换和编辑互斥入口。
    Filter(SelectionFilter), // 选择模式下的类别过滤。
    Batch(crate::authoring::placement_batch::PlacementBatchAction), // 复制、粘贴及批量结构操作复用同一作者事务。
    Options,
}
impl Default for ModeButton {
    fn default() -> Self { Self::Mode(ViewportMode::Select) }
}

fn mesh_tool(mode: ViewportMode) -> Option<MeshEditTool> {
    match mode {
        ViewportMode::Select => Some(MeshEditTool::Select),
        ViewportMode::Move => Some(MeshEditTool::Move),
        ViewportMode::Rotate => Some(MeshEditTool::Rotate),
        ViewportMode::Scale => Some(MeshEditTool::Scale),
        ViewportMode::Edit => None,
    }
}

#[derive(Component, Clone, Default)]
pub(crate) struct GlobalModeToolbar;
#[derive(Component)]
pub(crate) struct ModeToolsRoot; // 独立于空间笔刷的ControlsRoot宿主，避免重复创建作者输入树。
#[derive(Component)]
pub(super) struct ContextControls;

/// 顶行只保留紧凑菜单入口；五种模式的唯一按钮位于实际三维视口内部。
pub(crate) fn global_mode_toolbar() -> impl Scene {
    use bevy::text::FontSourceTemplate;
    let action = ModeButton::Options;
    let tooltip = crate::ui::tooltips::EditorTooltipTarget(crate::ui::tooltips::TooltipAction::ViewportOptions);
    bsn! { GlobalModeToolbar crate::components::MapEditorSceneContext RelativeCursorPosition::default() Interaction::default() ~{FocusPolicy::Block}
        Node { column_gap: px(2), height: px(26), flex_shrink: 0.0, align_items: AlignItems::Center }
        Children [(InteractionButton ~{action} ~{tooltip}
            Node { height: px(26), padding: UiRect::horizontal(px(6)), align_items: AlignItems::Center,
                justify_content: JustifyContent::Center, flex_shrink: 0.0, border_radius: BorderRadius::all(px(2)) }
            BackgroundColor(colors::PANEL_BG) BorderColor::all(colors::COPPER_BORDER_WEAK)
            Children [(Text("筛选与操作") @feathrise_editor_ui::localization::label_scene("筛选与操作") TextFont { font: FontSourceTemplate::Handle(feathrise_editor_core::editor_font_template(FONT_PATH)), font_size: px(12) }
                TextColor(colors::TEXT_PRIMARY) TextLayout::no_wrap() Pickable::IGNORE)])] }
}

fn spawn_mode_tools(parent: &mut ChildSpawnerCommands, assets: &AssetServer) {
    parent.spawn((
        ModeToolsRoot, Interaction::default(), RelativeCursorPosition::default(), FocusPolicy::Block,
        Node {
            position_type: PositionType::Absolute, left: px(8), top: px(8), width: px(MODE_RAIL_WIDTH),
            flex_direction: FlexDirection::Column, row_gap: px(2), padding: UiRect::all(px(2)),
            border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)), ..default()
        },
        BackgroundColor(colors::PANEL_BG_STRONG), BorderColor::all(colors::COPPER_BORDER_WEAK), ZIndex(25),
    )).with_children(|rail| {
        for (mode, label, icon) in [
            (ViewportMode::Select, "选择", "tool_select"), (ViewportMode::Move, "移动", "tool_move"),
            (ViewportMode::Rotate, "旋转", "tool_rotate"), (ViewportMode::Scale, "缩放", "tool_scale"),
            (ViewportMode::Edit, "编辑", "tool_edit"),
        ] {
            rail.spawn((
                InteractionButton, ModeButton::Mode(mode), Name::new(label), FocusPolicy::Block,
                crate::ui::tooltips::EditorTooltipTarget(crate::ui::tooltips::TooltipAction::ViewportMode(mode)),
                Node {
                    width: px(MODE_BUTTON_SIZE), height: px(MODE_BUTTON_SIZE), flex_shrink: 0.0,
                    border: UiRect::all(px(1)), border_radius: BorderRadius::all(px(2)),
                    align_items: AlignItems::Center, justify_content: JustifyContent::Center, ..default()
                },
                BackgroundColor(colors::SLOT_BG), BorderColor::all(colors::COPPER_BORDER_WEAK),
            )).with_child((
                ImageNode { image: assets.load(format!("icons/editor/{icon}.png")), color: colors::TEXT_PRIMARY, ..default() },
                Node { width: px(20), height: px(20), flex_shrink: 0.0, ..default() },
                FocusPolicy::Pass, Pickable::IGNORE, // 透明轮廓由现有主题着色，图标本身不截获父按钮动作。
            ));
        }
    });
}

#[derive(Component, Clone, Default)]
pub(crate) struct ControlsRoot; // 面板空隙也阻止鼠标穿透，手柄读取同帧 RelativeCursorPosition。

#[derive(Component)]
pub(super) struct FilterRow; // 类型过滤只在选择模式显示。

#[derive(Component)]
pub(super) struct BatchRow; // 编辑笔刷模式不显示只能用于对象选择的结构动作。

pub(super) fn setup(
    mut commands: Commands,   // 视口首次出现时创建五种模式与七类筛选按钮。
    assets: Res<AssetServer>, // 共用项目中文字体。
    viewports: Query<Entity, Added<MapEditorViewport>>, // 保持生命周期随视口自动回收。
    global_toolbar: Query<(), With<GlobalModeToolbar>>,
) {
    let global = !global_toolbar.is_empty();
    for viewport in &viewports {
        let font: Handle<Font> = feathrise_editor_core::editor_font(&assets, crate::ui::scene::FONT_PATH); // 显式中文字体避免空白按钮。
        commands.entity(viewport).with_children(|parent| {
            spawn_mode_tools(parent, &assets); // 只挂在三维视口，文档/预设正文沿用原互斥显隐而不被遮挡。
            super::mesh_edit_ui::spawn(parent, &font);
            let mut controls = parent.spawn((
                    ControlsRoot,                      // 根区域统一占用指针。
                    Interaction::default(),            // 包括行间空隙的悬停检测。
                    RelativeCursorPosition::default(), // 面板内部子按钮仍算整个浮层命中。
                    FocusPolicy::Block,                // 空隙不透传给地图。
                    Node {
                        position_type: PositionType::Absolute, // 覆盖地图，不挤压离屏视口尺寸。
                        left: px(50),                          // 筛选与空间笔刷位于图标栏右侧，不覆盖五种模式按钮。
                        right: px(8),                          // 窄视口按剩余空间收窄，不能把菜单推到右侧属性栏。
                        top: px(42),                           // 网格编辑模式入口常驻首行，展开菜单在下方避免重叠。
                        max_width: px(preferred_controls_width(ViewportMode::Select)), // 受检首选宽度与左右定位共同限制真实可用空间。
                        flex_direction: FlexDirection::Column, // 模式、笔刷类型与选择筛选按行排列。
                        row_gap: px(3),                        // 行间保留清晰间隔。
                        padding: UiRect::all(px(4)),           // 暗墨容器内留少量点击安全间距。
                        border: UiRect::all(px(1)),            // 细铜边与视口背景分离。
                        ..default()
                    },
                    BackgroundColor(colors::PANEL_BG_STRONG), // 深墨底让文字不受地图亮度影响。
                    BorderColor::all(colors::COPPER_BORDER_WEAK), // 菜单外沿与当前工作台弱分隔一致。
                    ZIndex(25),                               // 始终高于离屏画面。
                ));
            if global { controls.insert(ContextControls); }
            controls.with_children(|panel| {
                    crate::ui::terrain_sidebar::spawn_volume_brush_types(panel, &font);
                    panel
                        .spawn((
                            FilterRow,
                            Node {
                                width: percent(100),
                                min_width: px(0),
                                column_gap: px(3), // 七类过滤按可用宽度自动折行。
                                row_gap: px(3),
                                flex_wrap: FlexWrap::Wrap,
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ))
                        .with_children(|row| {
                            for (label, filter) in [
                                ("全部", SelectionFilter::All),
                                ("地表", SelectionFilter::Surface),
                                ("植被", SelectionFilter::Vegetation),
                                ("扩展", SelectionFilter::Domain),
                                ("水体", SelectionFilter::Water), // 与其他作者类型统一筛选。
                                ("资产", SelectionFilter::Placements), // 只拾取单独摆放的GLB实例。
                                ("交互", SelectionFilter::Gameplay), // 传送点与区域边界进入同一候选列表。
                            ] {
                                button(row, &font, label, ModeButton::Filter(filter), COMPACT_BUTTON_WIDTH); // 只筛选选择候选，不隐藏实际地图内容。
                            }
                        });
                    panel.spawn((BatchRow, Node { width: percent(100), min_width: px(0), column_gap: px(3), row_gap: px(3), flex_wrap: FlexWrap::Wrap, flex_shrink: 0.0, ..default() }))
                        .with_children(|row| {
                            use crate::authoring::placement_batch::PlacementBatchAction as Action;
                            for (label, action) in [("复制", Action::Copy), ("粘贴", Action::Paste), ("复制放置", Action::Duplicate), ("删除", Action::Delete)] {
                                button(row, &font, label, ModeButton::Batch(action), if action == Action::Duplicate { 68.0 } else { COMPACT_BUTTON_WIDTH });
                            }
                        }); // 对象操作组随前面的完整折行高度下移，仍由同一浮层拥有鼠标。
                });
        });
    }
}

fn button(
    parent: &mut ChildSpawnerCommands,
    font: &Handle<Font>,
    label: &str,
    action: ModeButton,
    width: f32,
) {
    let mut root = parent
        .spawn((
            InteractionButton,             // 标准按钮生成稳定交互组件。
            action,             // 独立身份不进入全局侧栏工具样式查询。
            FocusPolicy::Block, // 点击不得传给背后地图。
            Node {
                width: px(width),                        // 两种按钮规格固定宽度。
                height: px(26),                          // 紧凑但可明确命中的操作高度。
                border: UiRect::all(px(1)),              // 仅保留弱分隔，选中态由填色区分。
                justify_content: JustifyContent::Center, // 中文文字水平居中。
                align_items: AlignItems::Center,         // 文字垂直居中。
                ..default()
            },
            BackgroundColor(colors::SLOT_BG), // 非选中按钮深墨。
            BorderColor::all(colors::COPPER_BORDER_WEAK), // 默认弱铜边。
        ));
    if let ModeButton::Batch(action) = action { root.insert(crate::authoring::placement_batch::PlacementBatchButton(action)); }
    root.with_child((
            feathrise_editor_ui::localization::editor_label(label), // 完整中文名称，不只用图标猜测模式。
            TextFont {
                font: font.clone().into(), // 所有按钮共用已加载的中文字体。
                font_size: 12.0.into(),    // 缩放后仍可读。
                ..default()
            },
            TextColor(colors::TEXT_PRIMARY), // 米白文字。
            TextLayout::no_wrap(), // 操作短标签保持单行，由父按钮居中放置。
            Node {
                // 使用字体测量的自然宽度；NoWrap不采用段落宽度约束，满宽节点会让字形贴左。
                flex_shrink: 0.0,    // 固定按钮文本不被压缩隐藏。
                ..default()
            },
            FocusPolicy::Pass, // 文字点击归属父按钮。
            Pickable::IGNORE,  // 文字自身不遮蔽按钮交互。
        ));
}

/// 旧Interaction不能在地图页隐藏后继续切模式；不依赖上一帧的悬停或字形布局结果。
pub(crate) fn controls_visible(mut entity: Entity, nodes: &Query<(&Node, Option<&ChildOf>)>) -> bool {
    for _ in 0..32 {
        let Ok((node, parent)) = nodes.get(entity) else { return false; };
        if node.display == Display::None { return false; }
        let Some(parent) = parent else { return true; };
        entity = parent.parent();
    }
    false
}

/// 同一竖栏在编辑模式只发布元素工具意图；全局模式继续由核心锁在 Select，不能变换完整对象。
pub(super) fn mesh_tool_input(
    editor: Res<MapEditorState>, guard: Res<MapEditorNumericScrubMouseGuard>,
    mut state: Option<ResMut<MeshEditState>>,
    buttons: Query<(Entity, &ModeButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    nodes: Query<(&Node, Option<&ChildOf>)>,
    workspace: Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
) {
    let Some(state) = state.as_deref_mut() else { return; };
    if !state.active() || state.moving() || state.boxing() || state.request.is_some()
        || !editor.open || editor.close_requested || guard.blocked()
        || workspace.as_ref().is_some_and(|workspace| workspace.active_tab != crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Map)
    { return; }
    for (entity, button, click) in &buttons {
        if !click.is_clicked() || !controls_visible(entity, &nodes) { continue; }
        if let ModeButton::Mode(mode) = *button && let Some(tool) = mesh_tool(mode) {
            state.request = Some(MeshEditAction::Tool(tool));
            break; // 不覆盖同帧元素按钮或其他已冻结请求；一个释放只选择一个工具。
        }
    }
}

pub(super) fn input(
    mut editor: ResMut<MapEditorState>,          // 五种模式共用原手柄启用与收尾入口。
    guard: Res<MapEditorNumericScrubMouseGuard>, // 数值拖动的释放帧继续让出指针。
    mut mode: ResMut<ViewportModeState>,         // UI只修改模式意图，不直接改笔刷。
    roots: Query<(Entity, &RelativeCursorPosition), Or<(With<ControlsRoot>, With<ModeToolsRoot>, With<GlobalModeToolbar>, With<super::MeshEditToolbarRoot>, With<crate::components::MapEditorOrientationAxisRoot>, With<super::super::shading::ViewShadingRoot>)>>, // 图标栏、方向球、菜单及容器空隙沿用同一鼠标门。
    buttons: Query<(Entity, &ModeButton, &EditorButtonClick), Changed<EditorButtonClick>>, // 只消费本帧交互边沿。
    nodes: Query<(&Node, Option<&ChildOf>)>,
    workspace: Option<Res<crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
    mesh_edit: Option<Res<crate::authoring::mesh_edit::MeshEditState>>,
) {
    let map_workspace = workspace.as_ref().is_none_or(|state| state.active_tab == crate::ui::vegetation_preset_editor::PresetWorkspaceTab::Map);
    let hovered = editor.open && map_workspace && roots.iter().any(|(entity, cursor)|
        cursor.cursor_over() && controls_visible(entity, &nodes)); // 只让真实可见三维视口控件阻断世界射线。
    if mode.pointer_over_controls != hovered {
        mode.pointer_over_controls = hovered;
    } // 稳定帧不触发模式资源更新。
    if !editor.open || guard.blocked() || !map_workspace || mesh_edit.as_ref().is_some_and(|state| state.active()) {
        return;
    } // 禁止关闭或被其他输入拥有者占用时修改模式。
    for (entity, button, interaction) in &buttons {
        if !interaction.is_clicked() || !controls_visible(entity, &nodes) {
            continue;
        } // 悬停只改变样式。
        match *button {
            ModeButton::Mode(value) => {
                crate::viewport::placement_move::set_placement_mode(&mut editor, Some(&mut mode), value);
            }, // 选择关闭手柄，编辑恢复该模块之前记忆的绘制工具。
            ModeButton::Filter(value) => mode.filter = value, // 过滤偏好保留到下次选择。
            ModeButton::Batch(_) => {}, // 批量域在Input阶段已经消费本帧按钮；模式不替它改写草稿。
            ModeButton::Options => mode.context_controls_open = !mode.context_controls_open,
        }
    }
}

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "layout_tests.rs"]
mod layout_tests;

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod gpu_tests;

pub(super) fn style(
    mode: Res<ViewportModeState>, // 使用本帧收口的模式与筛选值。
    domain: Option<Res<crate::game_authoring::map_interaction::MapInteractionState>>,
    mesh_edit: Option<Res<crate::authoring::mesh_edit::MeshEditState>>,
    mut roots: Query<(&mut Node, Has<ContextControls>), (With<ControlsRoot>, Without<GlobalModeToolbar>, Without<FilterRow>, Without<BatchRow>)>,
    mut filters: Query<&mut Node, (With<FilterRow>, Without<BatchRow>)>, // 编辑模式收起无关筛选，保留地图空间。
    mut batches: Query<&mut Node, (With<BatchRow>, Without<FilterRow>)>,
    mut rails: Query<&mut Node, (With<ModeToolsRoot>, Without<ControlsRoot>, Without<FilterRow>, Without<BatchRow>)>,
    mut mode_nodes: Query<(&ModeButton, &Children, &mut Node), (Without<ControlsRoot>, Without<GlobalModeToolbar>, Without<FilterRow>, Without<BatchRow>, Without<ModeToolsRoot>)>,
    mut labels: Query<&mut Text>,
    mut buttons: Query<(
        &ModeButton,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>, // 唯一样式写入者，悬停不重建节点。
) {
    let editing_mesh = mesh_edit.as_ref().is_some_and(|state| state.active());
    for mut rail in &mut rails {
        let display = Display::Flex; // 对象和元素编辑共用竖栏，工具位置不随模式迁移。
        if rail.display != display { rail.display = display; }
    }
    for (button, children, mut node) in &mut mode_nodes {
        let external = matches!(button, ModeButton::Filter(SelectionFilter::Domain));
        let hidden = editing_mesh && matches!(button, ModeButton::Mode(mode) if mesh_tool(*mode).is_none())
            || external && domain.as_ref().is_none_or(|state| !state.enabled);
        if external && let Some(domain) = domain.as_ref() {
            for child in children.iter() {
                if let Ok(mut text) = labels.get_mut(child) { text.set_if_neq(Text::new(domain.label())); }
            }
        }
        let display = if hidden { Display::None } else { Display::Flex };
        if node.display != display { node.display = display; }
    }
    let width = px(preferred_controls_width(mode.mode));
    for (mut root, context) in &mut roots {
        if root.max_width != width { root.max_width = width; } // 仅模式变化时调整首选宽度；左右边距限制窄视口的剩余空间。
        let display = if editing_mesh || context && !mode.context_controls_open { Display::None } else { Display::Flex };
        if root.display != display { root.display = display; }
    }
    for mut node in &mut filters {
        let display = if mode.mode == ViewportMode::Select {
            Display::Flex
        } else {
            Display::None
        }; // 筛选只影响选择，不混入编辑工具。
        if node.display != display {
            node.display = display;
        } // 只在模式切换时更新布局。
    }
    for mut node in &mut batches {
        let display = if mode.mode == ViewportMode::Edit { Display::None } else { Display::Flex };
        if node.display != display { node.display = display; }
    }
    for (button, interaction, mut background, mut border) in &mut buttons {
        let selected = match *button {
            ModeButton::Mode(value) => if editing_mesh {
                mesh_edit.as_ref().is_some_and(|state| mesh_tool(value) == Some(state.tool))
            } else { mode.mode == value }, // 编辑时回显元素工具，不把全局 Select 锁误当作当前工具。
            ModeButton::Filter(value) => mode.filter == value, // 过滤持续高亮。
            ModeButton::Batch(_) => false,
            ModeButton::Options => mode.context_controls_open,
        };
        let color = match (*interaction, selected) {
            (Interaction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (Interaction::Hovered, _) => colors::SELECT_BG_HOVER,
            (Interaction::None, true) => colors::SELECT_BG,
            (Interaction::None, false) => colors::SLOT_BG,
        }; // 模式、筛选与展开入口共用导航填色，不以亮白描边表示普通选中。
        let edge = colors::COPPER_BORDER_WEAK;
        if background.0 != color {
            background.0 = color;
        } // 无变化不触发UI提取更新。
        if *border != BorderColor::all(edge) {
            *border = BorderColor::all(edge);
        } // 同一系统保持稳定边框。
    }
}
