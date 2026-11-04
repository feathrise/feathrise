//! 导航栏可见性和计数；会话失效只取消自己的输入，不抢其他编辑器字段焦点。

use super::super::{resources::DocumentState, types::ViewMode};
use super::{
    components::*,
    index,
    resources::DocumentNavigation,
    types::{NavigationMode, SearchOptions},
};
use bevy::{input_focus::InputFocus, prelude::*, text::EditableText, ui::InteractionDisabled};

pub(super) fn sync(
    mut commands: Commands,
    documents: Res<DocumentState>,
    mut state: ResMut<DocumentNavigation>,
    mut focus: ResMut<InputFocus>,
    mut fields: Query<(Entity, &NavigationField, &mut EditableText)>,
    mut nodes: Query<
        (
            &mut Node,
            Has<NavigationFindBar>,
            Has<NavigationGoBar>,
            Has<NavigationReplaceBar>,
            Has<NavigationSearchHint>,
        ),
        Or<(
            With<NavigationTools>,
            With<NavigationFindBar>,
            With<NavigationGoBar>,
            With<NavigationReplaceBar>,
            With<NavigationSearchHint>,
        )>,
    >,
    mut labels: Query<
        (&mut Text, Has<NavigationCount>, Has<NavigationSearchHint>),
        Or<(
            With<NavigationCount>,
            With<NavigationLineHint>,
            With<NavigationSearchHint>,
        )>,
    >,
    mut buttons: Query<(
        Entity,
        &NavigationAction,
        &mut BackgroundColor,
        &mut BorderColor,
        Has<InteractionDisabled>,
    )>,
) {
    let current = documents.current();
    let same_document = current.is_some_and(|doc| {
        state
            .document
            .as_ref()
            .is_some_and(|key| key.path == doc.path && key.generation == doc.generation)
    });
    let editing = documents.visible && current.is_some_and(|doc| doc.mode == ViewMode::Edit);
    if !same_document || !editing {
        state.mode = NavigationMode::Closed;
        state.pending = None;
        state.cancel_replace();
        state.reveal = None;
        state.selected = None;
        if !same_document {
            state.document = current.map(|doc| doc.source_key());
            state.query.clear();
            state.replacement.clear();
            state.options = SearchOptions::default();
            state.in_selection = false;
            state.scope = None;
            state.line.clear();
            state.hint.clear();
            state.ticket = state.ticket.wrapping_add(1);
            state.index = None;
        }
        for (entity, _, mut text) in &mut fields {
            if focus.get() == Some(entity) {
                focus.clear();
            }
            if !same_document || text.is_composing() {
                text.clear();
            }
        }
    }
    for (mut node, find, go, replace, hint) in &mut nodes {
        let visible = editing
            && if find {
                state.mode.searching()
            } else if go {
                state.mode == NavigationMode::Go
            } else if replace {
                state.mode == NavigationMode::Replace
            } else if hint {
                false // 搜索错误与进度由底栏读取，不在工具栏插入重复文字高度。
            } else {
                true
            };
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let index = current.and_then(|doc| {
        state
            .index
            .as_ref()
            .filter(|result| index::current(&result.key, doc, &state))
    });
    // 工具栏计数只显示已完成的真实匹配；输入说明进?，失败与等待进底栏。
    let count = if state.query.is_empty() || (state.in_selection && state.scope.is_none()) {
        String::new()
    } else if let Some(index) = index {
        if index.error.is_some() {
            String::new()
        } else if index.matches.is_empty() {
            "0 / 0".into()
        } else {
            format!(
                "{} / {}",
                state.selected.map_or(0, |n| n + 1),
                index.matches.len()
            )
        }
    } else {
        String::new()
    };
    let search_hint = if !editing || !state.mode.searching() {
        String::new()
    } else if !state.hint.is_empty() {
        state.hint.clone()
    } else if state.query.is_empty() {
        String::new()
    } else if state.in_selection && state.scope.is_none() {
        "选区已失效".into()
    } else if let Some(index) = index {
        index.error.clone().unwrap_or_default()
    } else {
        "查找中…".into()
    };
    let line_hint = if !editing || state.mode != NavigationMode::Go {
        String::new()
    } else if !state.hint.is_empty() {
        state.hint.clone()
    } else if index.is_none() {
        "正在准备行索引…".into()
    } else {
        String::new()
    };
    for (mut text, is_count, is_search_hint) in &mut labels {
        let value = if is_count {
            &count
        } else if is_search_hint {
            &search_hint
        } else {
            &line_hint
        };
        if text.as_str() != value {
            **text = value.clone();
        }
    }
    let can_replace = editing
        && state.mode == NavigationMode::Replace
        && state.replace.is_none()
        && state.ready.is_none()
        && documents.pending_load.is_none()
        && (documents.io.is_none() || documents.saving.is_some())
        && index.is_some_and(|index| index.error.is_none() && !index.matches.is_empty());
    for (entity, action, mut background, mut border, disabled) in &mut buttons {
        let enabled = !matches!(
            action,
            NavigationAction::ReplaceCurrent | NavigationAction::ReplaceAll
        ) || can_replace;
        if enabled == disabled {
            if enabled {
                commands.entity(entity).remove::<InteractionDisabled>();
            } else {
                commands.entity(entity).insert(InteractionDisabled);
            }
        }
        let active = match action {
            NavigationAction::CaseSensitive => state.options.case_sensitive,
            NavigationAction::WholeWord => state.options.whole_word,
            NavigationAction::InSelection => state.in_selection,
            _ => false,
        };
        background.set_if_neq(BackgroundColor(if active {
            crate::ui::theme::BUTTON_DEV_SMALL_PRESSED
        } else {
            crate::ui::theme::SLOT_BG
        }));
        border.set_if_neq(BorderColor::all(if active {
            crate::ui::theme::INPUT_ACTIVE_BORDER
        } else {
            crate::ui::theme::COPPER_BORDER_WEAK
        }));
    }
}
