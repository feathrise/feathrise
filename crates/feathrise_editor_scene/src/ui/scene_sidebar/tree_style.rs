//! 根行与虚拟行池共用交互优先级，选中、悬停和有效落点不互相掩盖。

use feathrise_editor_ui::{InteractionButton, UiInteraction};
use crate::ui::theme as colors;
use crate::ui::tooltips::{EditorTooltipTarget, TooltipAction};
use bevy::prelude::*;

const TREE_BRANCH_SLOT_WIDTH: f32 = 12.0;
const TREE_BRANCH_MARGIN_LEFT: f32 = -4.0;
const TREE_ROW_INSET: f32 = 5.0;
pub(crate) const TREE_DISCLOSURE_WIDTH: f32 = 18.0;
pub(crate) const TREE_DISCLOSURE_ICON_SIZE: f32 = 14.0;
pub(crate) const TREE_INDENT: f32 = 14.0;
pub(crate) const TREE_CONTENT_INSET: f32 = TREE_INDENT;
pub(crate) const TREE_ROOT_BORDER: f32 = 1.0;
pub(crate) const TREE_ICON_SIZE: f32 = 16.0;
pub(crate) const TREE_BRANCH_GAP: f32 = 2.0;
pub(crate) const TREE_CAPTION_GAP: f32 = 5.0;

pub(crate) fn tree_row_left(depth: usize) -> Val {
    px(TREE_ROW_INSET + depth as f32 * TREE_INDENT)
}

/// 同级类型图标同列；每行只占一个窄箭头槽，层级变化只增加缩进。
pub(crate) fn tree_row_icon_left(depth: usize, width: f32) -> Val {
    px(TREE_ROW_INSET + depth as f32 * TREE_INDENT + TREE_BRANCH_SLOT_WIDTH + TREE_BRANCH_GAP + (TREE_ICON_SIZE - width) * 0.5)
}

/// 根行的边框计入左边距，正文从根行的下一档开始。
pub(crate) fn tree_root_row_left() -> Val {
    px(TREE_ROW_INSET - TREE_ROOT_BORDER)
}

pub(crate) fn tree_parent_guide_x(depth: usize) -> f32 {
    TREE_ROW_INSET + depth.saturating_sub(1) as f32 * TREE_INDENT + TREE_BRANCH_MARGIN_LEFT + TREE_DISCLOSURE_WIDTH * 0.5
}

fn tree_branch_node(width: f32) -> Node {
    Node {
        width: px(width), height: px(21), flex_shrink: 0.0,
        // 展开控件可以覆盖类型图标列，抵消新增宽度后，图标和名称仍在原位。
        margin: UiRect {
            left: px(TREE_BRANCH_MARGIN_LEFT),
            right: px(TREE_BRANCH_SLOT_WIDTH - width - TREE_BRANCH_MARGIN_LEFT),
            ..UiRect::ZERO
        },
        padding: UiRect::left(px((TREE_DISCLOSURE_WIDTH - TREE_DISCLOSURE_ICON_SIZE) * 0.5)),
        align_items: AlignItems::Center,
        border_radius: BorderRadius::all(px(3)), ..default()
    }
}

/// 没有类型图标的显示入口使用单独的箭头槽。
pub(crate) fn tree_branch_button() -> impl Scene {
    tree_branch_control(TREE_DISCLOSURE_WIDTH)
}

/// 对象分支的展开区延伸到名称起点，覆盖箭头和类型图标。
pub(crate) fn tree_object_branch_button() -> impl Scene {
    tree_branch_control(TREE_DISCLOSURE_WIDTH + TREE_ICON_SIZE + TREE_CAPTION_GAP)
}

fn tree_branch_control(width: f32) -> impl Scene {
    bsn! { InteractionButton ~{tree_branch_node(width)} BackgroundColor(Color::NONE)
        Children [ImageNode::default()
            Node { width: px(TREE_DISCLOSURE_ICON_SIZE), height: px(TREE_DISCLOSURE_ICON_SIZE), flex_shrink: 0.0 } Pickable::IGNORE] }
}

pub(crate) fn tree_section_header() -> Node {
    Node {
        width: percent(100), min_width: px(0), height: px(24),
        // 分类复用普通容器的箭头槽，名称、同级图标和右侧操作按固定列排列。
        padding: UiRect { left: tree_row_left(0), right: px(4), ..UiRect::ZERO },
        border_radius: BorderRadius::all(px(2)), align_items: AlignItems::Center,
        column_gap: px(TREE_BRANCH_GAP), flex_shrink: 0.0, ..default()
    }
}

pub(crate) fn load_tree_branch_images(server: &AssetServer) -> [Handle<Image>; 2] {
    ["tree_expand", "tree_collapse"].map(|name| server.load(format!("icons/editor/{name}.png")))
}

pub(crate) fn sync_tree_branch_icon(
    collapsed: bool, available: bool, images: &[Handle<Image>; 2], image: &mut Mut<'_, ImageNode>,
) {
    let handle = &images[usize::from(!collapsed)];
    if &image.image != handle { image.image = handle.clone(); }
    let color = if available { colors::TEXT_PRIMARY } else { colors::TEXT_MUTED };
    if image.color != color { image.color = color; }
}

pub(crate) fn tree_branch_fill(available: bool, interaction: UiInteraction) -> BackgroundColor {
    BackgroundColor(match (available, interaction) {
        (true, UiInteraction::Pressed) => colors::SELECT_BG_PRESSED,
        (true, UiInteraction::Hovered) => colors::SELECT_BG_HOVER,
        _ => Color::NONE,
    })
}

pub(crate) fn tree_row_colors(
    selected: bool,
    drop_target: bool,
    interaction: UiInteraction,
) -> (Color, BorderColor) {
    let background = if drop_target {
        colors::SELECT_BG_HOVER
    } else {
        match interaction {
            UiInteraction::Pressed => colors::SELECT_BG_PRESSED,
            UiInteraction::Hovered => colors::SELECT_BG_HOVER,
            UiInteraction::None if selected => colors::SELECT_BG,
            UiInteraction::None => colors::PANEL_BG,
        }
    };
    let border = BorderColor::all(
        if drop_target || selected || interaction != UiInteraction::None {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        },
    );
    (background, border)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeRowToggle {
    Participation,
    Visibility,
    Lock,
}

impl TreeRowToggle {
    pub(crate) const ALL: [Self; 3] = [Self::Participation, Self::Visibility, Self::Lock];
}

/// 场景树各对象共用固定按钮尺寸、提示和图标，由各列表绑定自己的作者目标。
pub(crate) fn tree_toggle_button(toggle: TreeRowToggle) -> impl Scene {
    let tooltip = match toggle {
        TreeRowToggle::Participation => TooltipAction::SceneTreeParticipation,
        TreeRowToggle::Visibility => TooltipAction::SceneTreeVisibility,
        TreeRowToggle::Lock => TooltipAction::SceneTreeLock,
    };
    bsn! { InteractionButton EditorTooltipTarget(tooltip)
        Node { width: px(18), height: px(21), flex_shrink: 0.0, border: px(0),
            border_radius: BorderRadius::all(px(3)), align_items: AlignItems::Center, justify_content: JustifyContent::Center }
        BackgroundColor(colors::BUTTON_DEV_SMALL_NORMAL) BorderColor::all(colors::COPPER_BORDER_WEAK) }
}

pub(crate) fn tree_toggle_icon() -> impl Scene {
    bsn! { ImageNode::default()
        Node { width: px(14), height: px(14), flex_shrink: 0.0 } Pickable::IGNORE }
}

pub(crate) fn load_tree_toggle_images(server: &AssetServer) -> [Handle<Image>; 6] {
    ["enabled", "disabled", "visible", "hidden", "locked", "unlocked"]
        .map(|name| server.load(format!("icons/editor/mesh_{name}.png")))
}

pub(crate) fn tree_toggle_colors(
    available: bool, active: bool, interaction: UiInteraction,
) -> (BackgroundColor, BorderColor) {
    let fill = match (available, interaction, active) {
        (true, UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
        (true, UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
        (_, _, true) => colors::SELECT_BG,
        _ => colors::BUTTON_DEV_SMALL_NORMAL,
    };
    let border = if active { colors::COPPER_BORDER } else { colors::COPPER_BORDER_WEAK };
    (BackgroundColor(fill), BorderColor::all(border))
}

pub(crate) fn sync_tree_toggle_icon(
    toggle: TreeRowToggle, active: bool, images: &[Handle<Image>; 6], image: &mut Mut<'_, ImageNode>,
) {
    let pair = match toggle {
        TreeRowToggle::Participation => 0,
        TreeRowToggle::Visibility => 2,
        TreeRowToggle::Lock => 4,
    };
    let handle = &images[pair + usize::from(!active)];
    if &image.image != handle { image.image = handle.clone(); }
    let color = if !active && toggle != TreeRowToggle::Lock { colors::TEXT_MUTED } else { colors::TEXT_PRIMARY };
    if image.color != color { image.color = color; }
}
