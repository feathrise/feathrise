//! 固定目录菜单挂在编辑器根下，不占左栏高度；边缘按真实UI倍率向工作区内收。

use feathrise_editor_ui::UiInteraction;
use super::{components::*, resources::SceneDirectoryMenuState};
use crate::components::SceneEditorRoot;
use crate::types::EditorAction;
use crate::ui::theme as colors;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

pub(super) fn setup(mut commands: Commands, roots: Query<Entity, Added<SceneEditorRoot>>) {
    for root in &roots {
        commands.spawn_scene(menu()).insert(ChildOf(root));
    }
}

fn menu() -> impl Scene {
    bsn! {
        DirectoryMenuBackdrop UiInteraction::default() ~{FocusPolicy::Block}
        GlobalZIndex(427)
        Node { display: Display::None, position_type: PositionType::Absolute,
            left: px(0), top: px(0), width: percent(100), height: percent(100) }
        Children [
            DirectoryMenuPopup ~{FocusPolicy::Block}
            Node { position_type: PositionType::Absolute, width: px(160), min_width: px(0),
                flex_direction: FlexDirection::Column, row_gap: px(1),
                padding: px(4), border: px(1), border_radius: BorderRadius::all(px(3)),
                overflow: Overflow::scroll_y() }
            BackgroundColor(colors::PANEL_BG_STRONG) BorderColor::all(colors::COPPER_BORDER_WEAK)
            Children [
                @super::super::scene::action("进入场景", EditorAction::EnterSelectedScene)--
                @super::super::scene::action("编辑", EditorAction::SceneFolderRename)--
                @super::super::scene::action("删除", EditorAction::DeleteScene)--
                @super::super::scene::action("新场景", EditorAction::NewScene)--
                @super::super::scene::action("新目录", EditorAction::SceneFolderCreate)
            ]
        ]
    }
}

pub(super) fn sync(
    mut state: ResMut<SceneDirectoryMenuState>,
    editor: Res<crate::resources::SceneEditorState>,
    browser: Res<crate::resources::SceneBrowserState>,
    index: Res<crate::resources::SceneBrowserIndexState>,
    registry: Option<Res<feathrise_scene_content::SceneContentRegistry>>,
    sidebar: Option<Res<crate::ui::sidebars::SceneEditorSidebarState>>,
    layout: Option<Res<crate::resources::SceneEditorLayoutState>>,
    roots: Query<&ComputedNode, With<SceneEditorRoot>>,
    mut backdrops: Query<&mut Node, (With<DirectoryMenuBackdrop>, Without<DirectoryMenuPopup>)>,
    mut popups: Query<&mut Node, (With<DirectoryMenuPopup>, Without<DirectoryMenuBackdrop>)>,
) {
    // Tasks可在输入之后采用新索引；表现阶段也复验，旧菜单不能多闪一帧或暴露重绑动作。
    if state.target.as_ref().is_some_and(|target| {
        !editor.open
            || editor.close_requested
            || editor.new_scene_dialog_open
            || browser.dialog.is_some()
            || editor.scene_transition_in_progress
            || editor.pending_scene_transition.is_some()
            || editor.scene_switch_confirmation.is_some()
            || sidebar
                .as_ref()
                .is_some_and(|sidebar| !sidebar.scene_browser_visible())
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
        EditorAction::EnterSelectedScene,
        EditorAction::SceneFolderRename,
        EditorAction::DeleteScene,
        EditorAction::NewScene,
        EditorAction::SceneFolderCreate,
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
