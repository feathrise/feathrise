//! 玩法工具准备先经过全局模式门；页面只观察稳定选择和原作者状态，不制造额外事务。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::EditorButtonClick;
use super::{components::*, resources::*, types::*};
use crate::authoring::{BoundaryEditState, EditorAuthoringSession, TerrainExternalChangeState};
use crate::resources::{
    SceneBrowserDragState, SceneBrowserState, SceneDeletionTask, SceneEditorNumericScrubMouseGuard,
    SceneEditorState,
};
use crate::ui::sidebars::{EditorLeftSidebarTab, EditorNavigatorTab, SceneEditorSidebarState};
use crate::ui::text_input::EditorTextInputGuard;
use crate::ui::theme as colors;
use crate::viewport::content_selection::{LayerCandidateState, LayerTarget};
use crate::viewport::modes::{ViewportMode, ViewportModeState};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use feathrise_scene_content::SceneId;

#[derive(SystemParam)]
pub(super) struct GameplaySidebarInputGate<'w> {
    numeric: Res<'w, SceneEditorNumericScrubMouseGuard>,
    numeric_input: Option<Res<'w, feathrise_editor_ui::ActiveNumericInput>>,
    text: Res<'w, EditorTextInputGuard>,
    browser: Option<Res<'w, SceneBrowserState>>,
    drag: Option<Res<'w, SceneBrowserDragState>>,
    deletion: Option<Res<'w, SceneDeletionTask>>,
    conflict: Option<Res<'w, TerrainExternalChangeState>>,
}

impl GameplaySidebarInputGate<'_> {
    fn blocked(&self) -> bool {
        self.numeric.blocked()
            || self
                .numeric_input
                .as_ref()
                .is_some_and(|state| state.entity.is_some())
            || self.text.block_shortcuts
            || self
                .browser
                .as_ref()
                .is_some_and(|state| state.dialog.is_some())
            || self
                .drag
                .as_ref()
                .is_some_and(|state| state.blocks_other_mouse())
            || self.deletion.as_ref().is_some_and(|state| state.busy())
            || self
                .conflict
                .as_ref()
                .is_some_and(|state| state.modal_open())
    }
}

pub(super) fn handle_tabs(
    tabs: Query<(&EditorButtonClick, &GameplaySidebarTab), Changed<EditorButtonClick>>,
    browse: Query<(&EditorButtonClick, &GameplaySidebarBrowseButton), Changed<EditorButtonClick>>,
    gate: GameplaySidebarInputGate,
    mut sidebar: ResMut<SceneEditorSidebarState>,
    session: Res<EditorAuthoringSession>,
    boundary: Res<BoundaryEditState>,
    mut editor: ResMut<SceneEditorState>,
    mut state: ResMut<GameplaySidebarState>,
) {
    if !editor.open
        || editor.close_requested
        || editor.new_scene_dialog_open
        || editor.scene_transition_in_progress
        || editor.scene_switch_confirmation.is_some()
        || !sidebar.tool_page_visible(EditorLeftSidebarTab::Gameplay)
        || session.saving
        || boundary.dragging()
        || gate.blocked()
    {
        return; // 保持系统运行来消费阻塞期间的Changed边沿，松键或关闭模态后不重放旧点击。
    }
    if let Some(page) = GameplaySidebarPage::for_tool(editor.tool) {
        if state.page != page {
            state.page = page; // 在Select把工具收成Inspect之前保存真实准备请求。
        }
    }
    for (interaction, tab) in &tabs {
        if interaction.is_clicked()
            && let Some(page) = GameplaySidebarPage::for_action(tab.0)
        {
            if state.page != page {
                state.page = page;
            }
            if editor.tool != page.tool() {
                editor.tool = page.tool();
            } // 同页点击也明确准备工具；只准备、不清选择、不创建对象，也不强行进入Edit。
        }
    }
    for (interaction, button) in &browse {
        if interaction.is_clicked() && state.page == GameplaySidebarPage::Asset {
            match button.0 {
                GameplaySidebarBrowse::Assets => sidebar.navigator = EditorNavigatorTab::Assets,
                GameplaySidebarBrowse::Scene => sidebar.reveal_scene_properties(),
            }
        }
    }
}

pub(super) fn sync_pages(
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    session: Res<EditorAuthoringSession>,
    selection: Option<Res<LayerCandidateState>>,
    mut state: ResMut<GameplaySidebarState>,
    mut observed_selection: Local<Option<(Option<SceneId>, Option<LayerTarget>, u64)>>,
    mut panels: Query<(&GameplaySidebarPanel, &mut Node)>,
    mut tabs: Query<(
        &UiInteraction,
        &GameplaySidebarTab,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut browse: Query<
        (&UiInteraction, &mut BackgroundColor),
        (
            With<GameplaySidebarBrowseButton>,
            Without<GameplaySidebarTab>,
        ),
    >,
) {
    let identity = (
        session.draft.as_ref().map(|draft| draft.scene_id.clone()),
        selection
            .as_ref()
            .and_then(|selection| selection.selected.clone()),
        selection
            .as_ref()
            .map_or(0, |selection| selection.selection_revision),
    ); // 只跟踪一个稳定ID；不扫描全图候选或墙体控制点。
    let selected_page = if observed_selection.as_ref() != Some(&identity) {
        *observed_selection = Some(identity.clone());
        match identity.1 {
            Some(LayerTarget::Portal(_)) => Some(GameplaySidebarPage::Portal),
            Some(LayerTarget::Boundary(_)) => Some(GameplaySidebarPage::Boundary),
            Some(LayerTarget::Placement(_)) => Some(GameplaySidebarPage::Asset),
            _ => None,
        }
    } else {
        None // 旧选择不会每帧抢回作者手动切换的工具页。
    };
    if editor.open && sidebar.tool_page_visible(EditorLeftSidebarTab::Gameplay) {
        if let Some(page) = selected_page.or_else(|| GameplaySidebarPage::for_tool(editor.tool)) {
            if state.page != page {
                state.page = page; // 视口在Select中选择玩法对象，也展示对应属性而不落笔。
            }
        }
    }
    for (panel, mut node) in &mut panels {
        let display = if panel.0 == state.page {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (interaction, tab, mut background, mut border) in &mut tabs {
        let selected = GameplaySidebarPage::for_action(tab.0) == Some(state.page);
        let fill = match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (UiInteraction::None, true) => colors::SELECT_BG,
            (UiInteraction::None, false) => colors::BUTTON_DEV_SMALL_NORMAL,
        };
        let outline = BorderColor::all(if selected {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
        if background.0 != fill {
            background.0 = fill;
        }
        if *border != outline {
            *border = outline;
        }
    }
    for (interaction, mut background) in &mut browse {
        let fill = match interaction {
            UiInteraction::Pressed => colors::SELECT_BG_PRESSED,
            UiInteraction::Hovered => colors::SELECT_BG_HOVER,
            UiInteraction::None => colors::BUTTON_DEV_SMALL_NORMAL,
        };
        if background.0 != fill {
            background.0 = fill;
        }
    }
}

pub(super) fn sync_feedback(
    editor: Res<SceneEditorState>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    sidebar: Res<SceneEditorSidebarState>,
    state: Res<GameplaySidebarState>,
    session: Res<EditorAuthoringSession>,
    mode: Res<ViewportModeState>,
    boundary: Res<BoundaryEditState>,
    mut texts: Query<(&GameplaySidebarFeedbackText, &mut Text, &mut TextColor)>,
) {
    if !editor.open || !sidebar.tool_page_visible(EditorLeftSidebarTab::Gameplay) {
        return;
    }
    let editing = mode.mode == ViewportMode::Edit && editor.tool == state.page.tool();
    let locale = locale.as_deref().copied().unwrap_or_default();
    let mode_text = format!(
        "{} · {}",
        locale.editor_label(state.page.label()),
        locale.text(if editing {
            "interaction.state.editing"
        } else if mode.mode == ViewportMode::Select {
            "interaction.state.selecting"
        } else {
            "interaction.state.unavailable"
        }),
    );
    let (availability, tint) = if editor.scene_transition_in_progress {
        (
            "正在切换场景，作者操作暂不可用".to_owned(),
            colors::TEXT_ACCENT,
        )
    } else if session.saving {
        (
            "正在保存场景，等待原事务完成".to_owned(),
            colors::TEXT_ACCENT,
        )
    } else if boundary.dragging() {
        (
            "控制点拖动中；请松开后切换工具".to_owned(),
            colors::TEXT_ACCENT,
        )
    } else if let Some(draft) = session
        .draft
        .as_ref()
        .filter(|draft| editor.selected_scene_id.as_ref() == Some(&draft.scene_id))
    {
        (
            format!("当前场景 · {}", draft.manifest.display_name),
            colors::TEXT_VALUE,
        )
    } else {
        (
            "等待当前场景作者数据；请先进入场景".to_owned(),
            colors::TEXT_ACCENT,
        )
    }; // 仅观察真实草稿与保存/拖动状态，不把隐藏视口或旧预览称为就绪。
    let asset = editor.selected_asset.as_ref().map_or_else(
        || "待摆放资产 · 尚未选择".to_owned(),
        |path| format!("待摆放资产 · {path}"),
    );
    for (marker, mut text, mut color) in &mut texts {
        let (value, tint) = match marker.0 {
            GameplaySidebarFeedbackKind::Mode => (mode_text.as_str(), colors::TEXT_ACCENT),
            GameplaySidebarFeedbackKind::Availability => (availability.as_str(), tint),
            GameplaySidebarFeedbackKind::Message => (editor.status.as_str(), colors::TEXT_MUTED),
            GameplaySidebarFeedbackKind::Asset => (asset.as_str(), colors::TEXT_VALUE),
        };
        let value = locale.editor_notice(value);
        if text.as_str() != value.as_ref() {
            **text = value.into_owned();
        }
        if color.0 != tint {
            color.0 = tint;
        }
    }
}
