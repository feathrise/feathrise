//! 完整文档的原生输入绑定与工具条回显；连续正文构造由独立有界系统负责。

use super::{components::*, resources::DocumentState, types::*};
use crate::ui::theme as colors;
use bevy::{
    input_focus::InputFocus,
    prelude::*,
};

pub(super) fn sync(
    state: Res<DocumentState>,
    replacement: Option<Res<super::search::AssetReplaceState>>,
    preview_state: Option<Res<super::super::types::AssetPreviewState>>,
    mut nodes: Query<
        (
            &mut Node,
            Has<DocumentPanel>,
            Has<DocumentSource>,
            Has<DocumentEditArea>,
            Has<DocumentPreviewScroll>,
            Has<DocumentSidebarLaunch>,
            Has<DocumentExternalNotice>,
        ),
        (
            Or<(
                With<DocumentPanel>,
                With<DocumentSource>,
                With<DocumentEditArea>,
                With<DocumentPreviewScroll>,
                With<DocumentSidebarLaunch>,
                With<DocumentExternalNotice>,
            )>,
            Without<DocumentAction>,
        ),
    >,
    mut texts: Query<
        (
            &mut Text,
            Has<DocumentStatus>,
            Has<DocumentReloadLabel>,
            Has<DocumentExpandLabel>,
            Has<DocumentPathText>,
        ),
        Or<(
            With<DocumentStatus>,
            With<DocumentReloadLabel>,
            With<DocumentExpandLabel>,
            With<DocumentPathText>,
        )>,
    >,
    mut buttons: Query<(
        &DocumentAction,
        &Interaction,
        &mut Node,
        &mut BackgroundColor,
        &mut BorderColor,
    ), Without<feathrise_editor_ui::workspace_tabs::WorkspaceTabButton>>,
    changed_buttons: Query<(), (With<DocumentAction>, Changed<Interaction>, Without<feathrise_editor_ui::workspace_tabs::WorkspaceTabButton>)>,
) {
    if !state.is_changed()
        && replacement.as_ref().is_none_or(|state| !state.is_changed())
        && changed_buttons.is_empty()
        && preview_state
            .as_ref()
            .is_none_or(|preview| !preview.is_changed())
    {
        return;
    }
    let doc = state.current();
    let editing = doc.is_some_and(|doc| doc.mode == ViewMode::Edit);
    for (mut node, panel, source, edit_area, preview, launch, external) in &mut nodes {
        let show = if panel {
            state.active.is_some() && replacement.as_ref().is_none_or(|state| !state.preview_visible())
        } else if source || edit_area {
            editing
        } else if launch {
            state.expanded.is_some()
                && preview_state.as_ref().is_some_and(|preview| {
                    matches!(
                        preview.content,
                        super::super::types::AssetPreviewContent::Document
                    )
                })
        } else if external {
            doc.is_some_and(|doc| doc.external.warning)
        } else {
            preview && !editing
        };
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    let other_drafts = state
        .documents
        .iter()
        .filter(|item| item.dirty() && state.active.as_deref() != Some(&item.path))
        .count();
    let status = match doc {
        Some(doc) => format!(
            "{} · {}{}{}{}",
            doc.format.label(),
            if doc.dirty() { "未保存 · " } else { "" },
            doc.status,
            if editing {
                "\nCtrl+Z撤销 · Ctrl+Y重做 · Ctrl+S保存 · Ctrl+Shift+S保存全部草稿"
            } else {
                ""
            },
            if !editing && doc.preview_clipped {
                "\n预览内容较多，已达显示上限；源码和保存仍保留完整文档"
            } else {
                ""
            }
        ),
        None if !state.error.is_empty() => state.error.clone(),
        None => "正在后台读取文本文件…".into(),
    };
    let status = if other_drafts > 0 {
        format!("{status}\n另有{other_drafts}份草稿，返回对应文档可继续编辑")
    } else {
        status
    };
    let reload = if doc.is_some_and(|doc| doc.confirm_reload) {
        "确认丢弃"
    } else {
        "重新载入"
    };
    for (mut text, is_status, is_reload, is_expand, _) in &mut texts {
        let value = if is_status {
            status.as_str()
        } else if is_reload {
            reload
        } else if is_expand {
            if state.central_active {
                "返回侧栏"
            } else {
                "放大"
            }
        } else {
            super::source::display_path(state.active.as_deref().unwrap_or_default())
        };
        if text.as_str() != value {
            **text = value.into();
        }
    }
    for (action, interaction, mut node, mut background, mut border) in &mut buttons {
        if matches!(action, DocumentAction::Preview | DocumentAction::Edit | DocumentAction::Save) {
            // 中央工具条保持紧凑；原位归还侧栏时恢复三项等分，不影响其他文档入口。
            let (width, basis, grow, shrink) = if state.in_center() {
                (px(72), px(72), 0.0, 0.0)
            } else {
                (Val::Auto, px(0), 1.0, 1.0)
            };
            if (node.width, node.flex_basis, node.flex_grow, node.flex_shrink)
                != (width, basis, grow, shrink)
            {
                node.width = width;
                node.flex_basis = basis;
                node.flex_grow = grow;
                node.flex_shrink = shrink;
            }
        }
        let active = matches!(action, DocumentAction::Edit) && editing
            || matches!(action, DocumentAction::Preview) && !editing
            || (matches!(action, DocumentAction::ActivateTab(id)
                if state.central_active && state.active_tab_id() == Some(*id)));
        let color = if active {
            colors::SELECT_BG
        } else if *interaction == Interaction::Hovered {
            colors::SELECT_BG_HOVER
        } else if *interaction == Interaction::Pressed {
            colors::SELECT_BG_PRESSED
        } else {
            colors::SLOT_BG
        };
        background.set_if_neq(BackgroundColor(color));
        border.set_if_neq(BorderColor::all(if active {
            colors::TEXT_ACCENT
        } else {
            colors::COPPER_BORDER_WEAK
        }));
    }
}

/// 排版后、原生跟随前冻结静止编辑器的视区，允许滚轮浏览光标之外的代码。
pub(super) fn capture_source_scroll(
    focus: Option<Res<InputFocus>>,
    mut sources: Query<(
        Entity,
        &bevy::text::EditableTextGeneration,
        &bevy::ui::widget::TextScroll,
        &mut DocumentScrollState,
    ), With<DocumentSource>>,
) {
    for (entity, generation, scroll, mut state) in &mut sources {
        let focused = focus.as_ref().is_some_and(|focus| focus.get() == Some(entity));
        // Bevy排版每帧借用EditableText为可变组件，Changed不足以证明输入发生了变化。
        // Parley真实代次覆盖内容、光标、选区与布局变更；获得焦点交给原生跟随，失焦保留视区。
        state.preserved = state.restore.take().or_else(||
            (state.generation == Some(*generation) && (state.focused == focused || !focused)).then_some(scroll.0));
        state.generation = Some(*generation);
        state.focused = focused;
    }
}

/// 静止帧恢复手动视区，真实编辑帧保留原生结果；在IME定位前完成，避免候选框读到旧偏移。
pub(super) fn restore_source_scroll(
    mut sources: Query<(
        &mut DocumentScrollState,
        &ComputedNode,
        &bevy::text::TextLayoutInfo,
        &mut bevy::ui::widget::TextScroll,
    ), With<DocumentSource>>,
) {
    for (mut state, node, layout, mut scroll) in &mut sources {
        let view = node.content_box().size();
        let (offset, extent) = if let Some(offset) = state.preserved.take() {
            (offset, layout.size)
        } else if state.focused && view.cmpgt(Vec2::ZERO).all()
            && let Some((_, cursor)) = layout.cursor
        {
            // 原生跟随按行框滚动，中文光标可能超出行框几像素；仅真实输入/重排帧补齐光标边缘。
            let minimum = (cursor.max - view).max(Vec2::ZERO);
            let offset = scroll.0.max(minimum).min(cursor.min.max(Vec2::ZERO));
            (offset, layout.size.max(cursor.max))
        } else {
            continue;
        };
        let maximum = (extent - view).max(Vec2::ZERO);
        scroll.set_if_neq(bevy::ui::widget::TextScroll(offset.clamp(Vec2::ZERO, maximum)));
    }
}

#[cfg(test)]
mod workspace_tab_style_tests {
    use super::*;
    use bevy::ecs::world::CommandQueue;
    use feathrise_editor_ui::workspace_tabs::{WorkspaceTabButton, spawn_preset_tab};

    #[test]
    fn business_sync_preserves_workspace_tab_fill_and_danger_close() {
        let mut app = App::new();
        app.init_resource::<DocumentState>().add_systems(Update, sync);
        let mut queue = CommandQueue::default();
        Commands::new(&mut queue, app.world()).spawn(Node::default()).with_children(|root| {
            for (id, active, confirm) in [(1, true, false), (2, false, false), (3, true, true)] {
                spawn_preset_tab(root, &Handle::default(), "地图说明.md*",
                    DocumentAction::ActivateTab(id), DocumentAction::CloseTab(id), active, confirm);
            }
        });
        queue.apply(app.world_mut());
        let expected: Vec<_> = app.world_mut()
            .query_filtered::<(Entity, &BackgroundColor, &BorderColor), With<WorkspaceTabButton>>()
            .iter(app.world()).map(|(entity, fill, border)| (entity, *fill, *border)).collect();
        assert_eq!(expected.len(), 6);
        let sentinel = BackgroundColor(Color::srgb(1.0, 0.0, 1.0));
        let ordinary = app.world_mut().spawn((Button, Node::default(), DocumentAction::Save,
            sentinel, BorderColor::all(Color::NONE))).id();
        // 文档真实同步曾把普通关闭键改成SLOT_BG；状态换代和交互快路都必须保持共享底色。
        for (index, interaction) in [Interaction::None, Interaction::Hovered, Interaction::Pressed, Interaction::None].into_iter().enumerate() {
            if index == 0 || index == 3 {
                app.world_mut().resource_mut::<DocumentState>().error.push(' ');
            }
            app.world_mut().entity_mut(ordinary).insert((interaction, sentinel));
            for &(entity, _, _) in &expected {
                app.world_mut().entity_mut(entity).insert(interaction);
            }
            app.update();
            assert_ne!(app.world().get::<BackgroundColor>(ordinary), Some(&sentinel));
            for &(entity, fill, border) in &expected {
                assert_eq!(app.world().get::<BackgroundColor>(entity), Some(&fill));
                assert_eq!(app.world().get::<BorderColor>(entity), Some(&border));
                assert_eq!(app.world().get::<Node>(entity).unwrap().border, UiRect::default());
            }
        }
        assert!(expected.iter().any(|(_, fill, _)| fill.0 == colors::DANGER_BG));
    }
}
