//! 根行与虚拟行池共用交互优先级，选中、悬停和有效落点不互相掩盖。

use feathrise_editor_ui::{InteractionButton, UiInteraction};
use crate::ui::theme as colors;
use crate::ui::tooltips::{EditorTooltipTarget, TooltipAction};
use bevy::prelude::*;

const TREE_BRANCH_WIDTH: f32 = 18.0;
pub(crate) const TREE_BRANCH_GAP: f32 = 2.0;
pub(crate) const TREE_CAPTION_GAP: f32 = 5.0;

pub(crate) fn tree_row_left(depth: usize) -> Val {
    tree_row_icon_left(depth, TREE_BRANCH_WIDTH)
}

/// 网格图标与同深度箭头共用列中心。
pub(crate) fn tree_row_icon_left(depth: usize, width: f32) -> Val {
    px(5.0 + depth as f32 * TREE_BRANCH_WIDTH + (TREE_BRANCH_WIDTH - width) * 0.5)
}

fn tree_branch_node() -> Node {
    Node {
        width: px(TREE_BRANCH_WIDTH), height: px(21), flex_shrink: 0.0,
        align_items: AlignItems::Center, justify_content: JustifyContent::Center,
        border_radius: BorderRadius::all(px(3)), ..default()
    }
}

/// 场景树各分支共享同一箭头列与命中面积，视觉图标不截走按钮点击。
pub(crate) fn tree_branch_button() -> impl Scene {
    bsn! { InteractionButton ~{tree_branch_node()} BackgroundColor(Color::NONE)
        Children [@tree_toggle_icon()] }
}

/// 叶子和纯标题占用同一箭头列，缩进只取决于树深度。
pub(crate) fn tree_branch_slot() -> impl Scene {
    bsn! { ~{tree_branch_node()} Pickable::IGNORE }
}

pub(crate) fn tree_section_header() -> Node {
    Node {
        width: percent(100), min_width: px(0),
        padding: UiRect { left: tree_row_left(0), top: px(4), ..UiRect::ZERO },
        border: UiRect::bottom(px(1)), align_items: AlignItems::Center,
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
