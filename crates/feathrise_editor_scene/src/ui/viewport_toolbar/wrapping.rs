//! 右端按钮只切换原工具栏的排版，保留原按钮、数值输入和共享滚轮入口。

use super::components::{ViewportToolbarExpandButton, ViewportToolbarScroll};
use crate::resources::SceneEditorNumericScrubMouseGuard;
use bevy::{prelude::*, ui::UiSystems};
use feathrise_editor_ui::EditorButtonClick;

fn toggle(
    clicks: Query<
        (&ChildOf, &Children, &EditorButtonClick),
        (
            With<ViewportToolbarExpandButton>,
            Changed<EditorButtonClick>,
        ),
    >,
    parents: Query<&Children>,
    mut rows: Query<(&mut Node, &mut ScrollPosition, &Children), With<ViewportToolbarScroll>>,
    mut groups: Query<&mut Node, Without<ViewportToolbarScroll>>,
    mut labels: Query<&mut Text>,
    guard: Option<Res<SceneEditorNumericScrubMouseGuard>>,
) {
    if guard.is_some_and(|guard| guard.blocked()) {
        return;
    }
    for (parent, caption, click) in &clicks {
        if !click.is_clicked() {
            continue;
        }
        let Ok(children) = parents.get(parent.parent()) else {
            continue;
        };
        let Some(row) = children.iter().find(|&entity| rows.get(entity).is_ok()) else {
            continue;
        };
        let Ok((mut node, mut scroll, sections)) = rows.get_mut(row) else {
            continue;
        };
        let expanded = node.flex_wrap == FlexWrap::NoWrap;
        let wrap = if expanded {
            FlexWrap::Wrap
        } else {
            FlexWrap::NoWrap
        };
        let gap = if expanded { px(4) } else { px(0) };
        node.flex_wrap = wrap;
        node.row_gap = gap;
        node.height = if expanded { Val::Auto } else { px(32) };
        scroll.x = 0.0; // 用户显式展开或收起都回到左侧第一个控件。
        // 文字和行布局同帧切换，展开后立即提供向上的收起提示。
        for child in caption.iter() {
            if let Ok(mut text) = labels.get_mut(child) {
                text.0 = if expanded { "▲" } else { "▼" }.into();
            }
        }
        for section in sections.iter() {
            if let Ok(mut group) = groups.get_mut(section) {
                // 先按组换行，组本身太宽时继续在按钮和完整数值槽之间换行。
                group.flex_wrap = wrap;
                group.max_width = if expanded { percent(100) } else { Val::Auto };
                group.height = if expanded { Val::Auto } else { px(26) };
                group.row_gap = gap;
            }
        }
    }
}

fn present(
    parents: Query<(&ComputedNode, &Children)>,
    rows: Query<(&Node, &ComputedNode), With<ViewportToolbarScroll>>,
    mut buttons: Query<
        (&ChildOf, &mut Node),
        (
            With<ViewportToolbarExpandButton>,
            Without<ViewportToolbarScroll>,
        ),
    >,
) {
    for (parent, mut button) in &mut buttons {
        let Ok((shell, siblings)) = parents.get(parent.parent()) else {
            continue;
        };
        let Some((row, measured)) = siblings.iter().find_map(|entity| rows.get(entity).ok()) else {
            continue;
        };
        let expanded = row.flex_wrap == FlexWrap::Wrap;
        // 用预留箭头前的总宽比较，避免按钮出现后靠自己的占宽维持显示。
        let overflow = measured.content_size().x > shell.size().x + 1.0;
        let display = if !shell.is_empty() && (expanded || overflow) {
            Display::Flex
        } else {
            Display::None
        };
        if button.display != display {
            button.display = display;
        }
    }
}

pub(crate) fn install(app: &mut App) {
    app.add_systems(
        Update,
        toggle
            .after(feathrise_editor_ui::update_editor_button_clicks)
            .in_set(crate::SceneEditorSystemSet::Input),
    )
    .add_systems(PostUpdate, present.after(UiSystems::Layout));
}
