//! 固定目录菜单挂在编辑器根下，不占左栏高度；边缘按真实UI倍率向工作区内收。

use super::{components::*, resources::MapDirectoryMenuState};
use crate::components::MapEditorRoot;
use crate::types::EditorAction;
use crate::ui::theme as colors;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

pub(super) fn setup(mut commands: Commands, roots: Query<Entity, Added<MapEditorRoot>>) {
    for root in &roots {
        commands.spawn_scene(menu()).insert(ChildOf(root));
    }
}

fn menu() -> impl Scene {
    bsn! {
        DirectoryMenuBackdrop Interaction::default() ~{FocusPolicy::Block}
        GlobalZIndex(427)
        Node { display: Display::None, position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: percent(100), height: percent(100) }
        Children [(
            DirectoryMenuPopup ~{FocusPolicy::Block}
            Node { position_type: PositionType::Absolute, width: px(160), min_width: px(0),
                flex_direction: FlexDirection::Column, row_gap: px(1),
                padding: px(4), border: px(1), border_radius: BorderRadius::all(px(3)),
                overflow: Overflow::scroll_y() }
            BackgroundColor(colors::PANEL_BG_STRONG) BorderColor::all(colors::COPPER_BORDER_WEAK)
            Children [
                (@super::super::scene::action("进入地图", EditorAction::EnterSelectedMap)),
                (@super::super::scene::action("编辑", EditorAction::MapFolderRename)),
                (@super::super::scene::action("删除", EditorAction::DeleteMap)),
                (@super::super::scene::action("新地图", EditorAction::NewMap)),
                (@super::super::scene::action("新目录", EditorAction::MapFolderCreate)),
            ]
        )]
    }
}

pub(super) fn sync(
    mut state: ResMut<MapDirectoryMenuState>,
    editor: Res<crate::resources::MapEditorState>,
    browser: Res<crate::resources::MapBrowserState>,
    index: Res<crate::resources::MapBrowserIndexState>,
    registry: Option<Res<feathrise_map_content::MapContentRegistry>>,
    sidebar: Option<Res<crate::ui::sidebars::MapEditorSidebarState>>,
    layout: Option<Res<crate::resources::MapEditorLayoutState>>,
    roots: Query<&ComputedNode, With<MapEditorRoot>>,
    mut backdrops: Query<&mut Node, (With<DirectoryMenuBackdrop>, Without<DirectoryMenuPopup>)>,
    mut popups: Query<&mut Node, (With<DirectoryMenuPopup>, Without<DirectoryMenuBackdrop>)>,
) {
    // Tasks可在输入之后采用新索引；表现阶段也复验，旧菜单不能多闪一帧或暴露重绑动作。
    if state.target.as_ref().is_some_and(|target| {
        !editor.open
            || editor.close_requested
            || editor.new_map_dialog_open
            || browser.dialog.is_some()
            || editor.map_transition_in_progress
            || editor.pending_map_transition.is_some()
            || editor.map_switch_confirmation.is_some()
            || sidebar
                .as_ref()
                .is_some_and(|sidebar| !sidebar.map_browser_visible())
            || layout
                .as_ref()
                .is_some_and(|layout| !layout.content_shown())
            || !super::systems::target_current(
                target,
                &browser,
                &index,
                &editor,
                registry.as_ref().map_or(0, |registry| registry.revision),
            )
    }) {
        state.target = None;
    }
    let shown = state.target.is_some();
    for mut node in &mut backdrops {
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    if !shown {
        return;
    }
    let Ok(root) = roots.single() else { return };
    let rows = [
        EditorAction::EnterSelectedMap,
        EditorAction::MapFolderRename,
        EditorAction::DeleteMap,
        EditorAction::NewMap,
        EditorAction::MapFolderCreate,
    ]
    .into_iter()
    .filter(|action| state.action_visible(*action))
    .count();
    let rect = crate::ui::asset_context_menu::helpers::sized_menu_rect(
        state.anchor,
        root.size() * root.inverse_scale_factor(),
        Vec2::new(
            160.0,
            10.0 + rows as f32 * 26.0 + rows.saturating_sub(1) as f32,
        ),
    );
    for mut node in &mut popups {
        let next = (
            px(rect.min.x),
            px(rect.min.y),
            px(rect.width()),
            px(rect.height()),
        );
        if (node.left, node.top, node.width, node.height) != next {
            (node.left, node.top, node.width, node.height) = next;
        }
    }
}
