//! 玩法工具准备先经过全局模式门；页面只观察稳定选择和原作者状态，不制造额外事务。

use super::{components::*, resources::*, types::*};
use crate::authoring::{BoundaryEditState, EditorAuthoringSession, TerrainExternalChangeState};
use crate::resources::{
    SceneBrowserDragState, SceneBrowserState, SceneDeletionTask, SceneEditorNumericScrubMouseGuard,
    SceneEditorState,
};
use crate::ui::sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState};
use crate::ui::text_input::EditorTextInputGuard;
use crate::ui::theme as colors;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};
use crate::viewport::content_selection::{LayerCandidateState, LayerTarget};
use crate::viewport::modes::{ViewportMode, ViewportModeState};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use feathrise_editor_ui::EditorButtonClick;
use feathrise_editor_ui::UiInteraction;
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
    pub(super) fn blocked(&self) -> bool {
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
    gate: GameplaySidebarInputGate,
    sidebar: Res<SceneEditorSidebarState>,
    session: Res<EditorAuthoringSession>,
    boundary: Res<BoundaryEditState>,
    mut editor: ResMut<SceneEditorState>,
    mut state: ResMut<GameplaySidebarState>,
    mut request: ResMut<super::project_rules::RuleOpenRequest>,
    mut mode: ResMut<ViewportModeState>,
    workspace: Option<Res<VegetationPresetEditorState>>,
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
    if let Some(page) =
        GameplaySidebarPage::for_tool(editor.tool).filter(|_| state.page.section().is_none())
    {
        if state.page != page {
            state.page = page; // 在Select把工具收成Inspect之前保存真实准备请求。
        }
    }
    let next = tabs
        .iter()
        .find(|(click, _)| click.is_clicked())
        .map(|(_, tab)| tab.0);
    if let Some(page) = next {
        state.page = page;
        request.0 = page.section().is_some();
        // 工具只切换右栏；空间工具仅在当前可见的场景视口准备编辑。
        let in_scene = workspace
            .as_ref()
            .is_none_or(|work| work.active_tab == PresetWorkspaceTab::Scene);
        if let Some(tool) = page.tool() {
            mode.remember_tool(tool);
            if in_scene {
                editor.tool = tool;
                return;
            }
        }
        // 表单和隐藏的视口都不接收落点；手动回场景时仍能准备刚选的工具。
        mode.mode = ViewportMode::Select;
        editor.tool = crate::types::EditorTool::Inspect;
    }
}

pub(super) fn show(mut node: Mut<Node>, mut visibility: Mut<Visibility>, visible: bool) {
    let (display, inherited) = if visible {
        (Display::Flex, Visibility::Inherited)
    } else {
        (Display::None, Visibility::Hidden)
    };
    if node.display != display {
        node.display = display;
    }
    if *visibility != inherited {
        *visibility = inherited;
    }
}

pub(super) fn sync_pages(
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    session: Res<EditorAuthoringSession>,
    selection: Option<Res<LayerCandidateState>>,
    mut state: ResMut<GameplaySidebarState>,
    mut observed_selection: Local<Option<(Option<SceneId>, Option<LayerTarget>, u64)>>,
    mut panels: Query<
        (&GameplaySidebarPanel, &mut Node, &mut Visibility),
        (
            Without<ViewportFeedback>,
            Without<InteractionRulesHost>,
        ),
    >,
    mut sections: Query<
        (
            Has<InteractionRulesHost>,
            &mut Node,
            &mut Visibility,
        ),
        (
            Or<(
                With<ViewportFeedback>,
                With<InteractionRulesHost>,
            )>,
            Without<GameplaySidebarPanel>,
        ),
    >,
    mut scrolls: Query<&mut ScrollPosition, With<GameplaySidebarScroll>>,
    mut last_page: Local<Option<GameplaySidebarPage>>,
    mut tabs: Query<(
        &UiInteraction,
        &GameplaySidebarTab,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
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
            _ => None,
        }
    } else {
        None // 旧选择不会每帧抢回作者手动切换的工具页。
    };
    if editor.open
        && sidebar.tool_page_visible(EditorLeftSidebarTab::Gameplay)
        && state.page.section().is_none()
    {
        if let Some(page) = selected_page.or_else(|| GameplaySidebarPage::for_tool(editor.tool)) {
            if state.page != page {
                state.page = page; // 视口在Select中选择玩法对象，也展示对应属性而不落笔。
            }
        }
    }
    if *last_page != Some(state.page) {
        for mut scroll in &mut scrolls {
            if scroll.0 != Vec2::ZERO {
                scroll.0 = Vec2::ZERO;
            }
        }
        *last_page = Some(state.page);
    }
    for (panel, node, visibility) in &mut panels {
        show(node, visibility, panel.0 == state.page);
    }
    for (rules, node, visibility) in &mut sections {
        show(
            node,
            visibility,
            if rules {
                state.page.section().is_some()
            } else {
                state.page.tool().is_some()
            },
        );
    }
    for (interaction, tab, mut background, mut border) in &mut tabs {
        let selected = tab.0 == state.page;
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
    let editing = mode.mode == ViewportMode::Edit && Some(editor.tool) == state.page.tool();
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
    for (marker, mut text, mut color) in &mut texts {
        let (value, tint) = match marker.0 {
            GameplaySidebarFeedbackKind::Mode => (mode_text.as_str(), colors::TEXT_ACCENT),
            GameplaySidebarFeedbackKind::Availability => (availability.as_str(), tint),
            GameplaySidebarFeedbackKind::Message => (editor.status.as_str(), colors::TEXT_MUTED),
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
