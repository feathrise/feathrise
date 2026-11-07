//! 编辑器固定两侧区域的拖动、临时专注、窄窗适配及首选布局记忆。

use bevy::prelude::*;
use feathrise_editor_ui::EditorButtonClick;
use bevy::window::PrimaryWindow;
use feathrise_editor_core::EditorSettings;

use super::theme as colors;
use crate::components::{
    MapEditorLayoutButton, MapEditorLayoutButtonText, MapEditorPane, MapEditorPaneSplitter,
    MapEditorSceneContext,
};
use crate::constants::{
    EDITOR_CONTENT_MAX_WIDTH, EDITOR_CONTENT_MIN_WIDTH, EDITOR_INSPECTOR_MAX_WIDTH,
    EDITOR_INSPECTOR_MIN_WIDTH, EDITOR_SPLITTER_WIDTH,
    EDITOR_WORKSPACE_MIN_WIDTH,
};
use crate::resources::{MapEditorLayoutState, MapEditorNumericScrubMouseGuard, MapEditorState};
use crate::types::{EditorLayoutAction, EditorPane, EditorPaneDrag, EditorPaneLayout};
use crate::ui::sidebars::{
    EditorRightSidebarTab, EditorSidebarTab, MapEditorSidebarTabButton, MapEditorSidebarTabText,
};
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};

/// 正式配置在PreStartup加载；Startup恢复一次，首帧输入不会被默认布局覆盖。
pub(crate) fn restore_editor_layout_preferences(
    settings: Res<EditorSettings>,
    mut state: ResMut<MapEditorLayoutState>,
) {
    state.apply_preferences(settings.editor_layout);
}

/// 松手后才发布宽度，显隐按钮立即发布；四个值未变时不触发原设置保存链。
pub(crate) fn remember_editor_layout_preferences(
    state: Res<MapEditorLayoutState>,
    mut settings: ResMut<EditorSettings>,
    access: Option<Res<crate::workbench::WorkbenchSettingsAccess>>,
) {
    // 无工程宿主交接期间保留已保存来源，不让旧布局或迟到拖动写入下一份作者偏好。
    if access.as_ref().is_some_and(|access| !access.editable) || state.drag.is_some() { return; }
    let preferences = state.preferences();
    if settings.editor_layout != preferences {
        settings.editor_layout = preferences;
    }
}

/// 窄窗口先临时收起内容浏览器，再收起检查器；放大窗口自动恢复原首选布局。
pub(crate) fn pane_layout(state: &MapEditorLayoutState, width: f32) -> EditorPaneLayout {
    let available = width.max(0.0) - EDITOR_WORKSPACE_MIN_WIDTH;
    let mut content = state.content_visible;
    let mut inspector = state.inspector_visible;
    if content
        && inspector
        && available
            < EDITOR_CONTENT_MIN_WIDTH + EDITOR_INSPECTOR_MIN_WIDTH + EDITOR_SPLITTER_WIDTH * 2.0
    {
        content = false;
    }
    if inspector && available < EDITOR_INSPECTOR_MIN_WIDTH + EDITOR_SPLITTER_WIDTH {
        inspector = false;
    }
    if content && available < EDITOR_CONTENT_MIN_WIDTH + EDITOR_SPLITTER_WIDTH {
        content = false;
    }
    let splitters = (u8::from(content) + u8::from(inspector)) as f32 * EDITOR_SPLITTER_WIDTH;
    let mut content_width = if content {
        state.content_width.clamp(EDITOR_CONTENT_MIN_WIDTH, EDITOR_CONTENT_MAX_WIDTH)
    } else {
        0.0
    };
    let mut inspector_width = if inspector {
        state
            .inspector_width
            .clamp(EDITOR_INSPECTOR_MIN_WIDTH, EDITOR_INSPECTOR_MAX_WIDTH)
    } else {
        0.0
    };
    let excess = (content_width + inspector_width + splitters - available).max(0.0);
    let reduce_content = if content {
        excess.min(content_width - EDITOR_CONTENT_MIN_WIDTH)
    } else {
        0.0
    };
    content_width -= reduce_content;
    if inspector {
        inspector_width -=
            (excess - reduce_content).min(inspector_width - EDITOR_INSPECTOR_MIN_WIDTH);
    }
    EditorPaneLayout {
        content: content.then_some(content_width),
        inspector: inspector.then_some(inspector_width),
    }
}

/// 拖动只处理两个固定分隔条；统一数值鼠标门负责和弦与 release 帧防穿透。
pub(crate) fn handle_editor_layout_input(
    editor: Res<MapEditorState>,
    guard: Res<MapEditorNumericScrubMouseGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    ui_scale: Res<UiScale>,
    windows: Query<&Window, With<PrimaryWindow>>,
    splitters: Query<(&MapEditorPaneSplitter, &Interaction)>,
    buttons: Query<(&MapEditorLayoutButton, &EditorButtonClick), Changed<EditorButtonClick>>,
    mut state: ResMut<MapEditorLayoutState>,
    access: Option<Res<crate::workbench::WorkbenchSettingsAccess>>,
) {
    if state.owns_mouse_this_frame {
        state.owns_mouse_this_frame = false;
    }
    if access.as_ref().is_some_and(|access| !access.editable) {
        state.drag = None;
        return;
    }
    if !editor.open || editor.new_map_dialog_open || editor.close_requested {
        if state.drag.is_some() {
            state.drag = None;
        }
        return;
    }
    if state.drag.is_none() {
        if guard.blocked() {
            return;
        }
        for (button, interaction) in &buttons {
            if interaction.is_clicked() {
                state.toggle(button.0);
                return;
            }
        }
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let scale = ui_scale.0.max(0.1);
    let Some(cursor) = window.cursor_position().map(|cursor| cursor.x / scale) else {
        state.drag = None;
        return;
    };
    if let Some(drag) = state.drag {
        state.owns_mouse_this_frame = true;
        if !mouse.pressed(MouseButton::Left) {
            state.drag = None;
            return;
        }
        let delta = cursor - drag.origin_x;
        if !drag.moved {
            if delta == 0.0 { return; }
            state.drag = Some(EditorPaneDrag { moved: true, ..drag });
        }
        match drag.pane {
            EditorPane::Content => {
                state.content_width =
                    (drag.origin_width + delta).clamp(EDITOR_CONTENT_MIN_WIDTH, EDITOR_CONTENT_MAX_WIDTH)
            }
            EditorPane::Inspector => {
                state.inspector_width =
                    (drag.origin_width - delta).clamp(EDITOR_INSPECTOR_MIN_WIDTH, EDITOR_INSPECTOR_MAX_WIDTH)
            }
        }
        return;
    }
    if mouse.just_pressed(MouseButton::Left) {
        let layout = pane_layout(&state, window.width() / scale);
        for (splitter, interaction) in &splitters {
            if *interaction == Interaction::None {
                continue;
            }
            let origin_width = match splitter.0 {
                EditorPane::Content => layout.content,
                EditorPane::Inspector => layout.inspector,
            };
            if let Some(origin_width) = origin_width {
                state.drag = Some(EditorPaneDrag {
                    pane: splitter.0,
                    origin_x: cursor,
                    origin_width,
                    moved: false,
                });
                state.owns_mouse_this_frame = true;
                break;
            }
        }
    }
}

/// 常量规模的节点原位回显；窗口收缩不改首选宽度，也不构造或编译任何预设。
pub(crate) fn sync_editor_layout(
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    ui_scale: Res<UiScale>,
    windows: Query<&Window, With<PrimaryWindow>>,
    workspace: Res<VegetationPresetEditorState>,
    mut state: ResMut<MapEditorLayoutState>,
    mut panes: Query<
        (&MapEditorPane, &mut Node),
        (
            Without<MapEditorPaneSplitter>,
            Without<MapEditorSceneContext>,
        ),
    >,
    mut splitters: Query<
        (
            &MapEditorPaneSplitter,
            &mut Node,
            &Interaction,
            &mut BackgroundColor,
        ),
        (
            Without<MapEditorPane>,
            Without<MapEditorSceneContext>,
            Without<MapEditorLayoutButton>,
        ),
    >,
    mut scene_context: Query<
        (&mut Node, Has<crate::components::MapEditorSceneContextHint>),
        (
            With<MapEditorSceneContext>,
            Without<MapEditorPane>,
            Without<MapEditorPaneSplitter>,
        ),
    >,
    mut buttons: Query<
        (
            &MapEditorLayoutButton,
            &Interaction,
            &mut BackgroundColor,
            &mut BorderColor,
            Option<&mut feathrise_editor_ui::ButtonColors>,
        ),
        Without<MapEditorPaneSplitter>,
    >,
    mut labels: Query<
        (
            &mut Text,
            Option<&MapEditorLayoutButtonText>,
            Option<&MapEditorSidebarTabText>,
            Option<&ChildOf>,
        ),
        Or<(
            With<MapEditorLayoutButtonText>,
            With<MapEditorSidebarTabText>,
        )>,
    >,
    sidebar_buttons: Query<&MapEditorSidebarTabButton>,
) {
    let width = windows
        .single()
        .map_or(1920.0, |window| window.width() / ui_scale.0.max(0.1));
    let layout = pane_layout(&state, width);
    if state.content_fits != layout.content.is_some() {
        state.content_fits = layout.content.is_some();
    }
    if state.inspector_fits != layout.inspector.is_some() {
        state.inspector_fits = layout.inspector.is_some();
    }
    let author_preview = matches!(workspace.active_tab, PresetWorkspaceTab::Domain(_, id) if id != 0)
        || matches!(workspace.active_tab, PresetWorkspaceTab::Water(id) if id != 0);
    for (pane, mut node) in &mut panes {
        let size = match pane.0 {
            EditorPane::Content => layout.content,
            EditorPane::Inspector => layout.inspector,
        };
        let display = if size.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        if let Some(size) = size {
            if node.width != Val::Px(size) {
                node.width = Val::Px(size);
            }
        }
    }
    for (splitter, mut node, interaction, mut background) in &mut splitters {
        let visible = match splitter.0 {
            EditorPane::Content => layout.content.is_some(),
            EditorPane::Inspector => layout.inspector.is_some(),
        };
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let color = if *interaction == Interaction::None {
            colors::PANEL_BG_STRONG
        } else {
            colors::COPPER_BORDER
        };
        if background.0 != color {
            background.0 = color;
        }
    }
    for (mut node, hint) in &mut scene_context {
        let display = if author_preview == hint {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (button, interaction, mut background, mut border, palette) in &mut buttons {
        let active = match button.0 {
            EditorLayoutAction::Content => layout.content.is_some(),
            EditorLayoutAction::Inspector => layout.inspector.is_some(),
            EditorLayoutAction::Focus => state.focus_restore.is_some(),
        };
        // 图标入口也会被通用UI补齐三态色；由实际布局状态同步其底色，避免悬停结束恢复成旧选中态。
        if let Some(mut palette) = palette {
            let next = feathrise_editor_ui::ButtonColors::new(
                if active { colors::SELECT_BG } else { colors::BUTTON_DEV_SMALL_NORMAL },
                colors::BUTTON_DEV_SMALL_HOVERED,
                colors::SELECT_BG_PRESSED,
            );
            if *palette != next { *palette = next; }
        }
        let color = match (*interaction, active) {
            (Interaction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (Interaction::Hovered, _) => colors::BUTTON_DEV_SMALL_HOVERED,
            (Interaction::None, true) => colors::SELECT_BG,
            _ => colors::BUTTON_DEV_SMALL_NORMAL,
        };
        if background.0 != color {
            background.0 = color;
        }
        let color = BorderColor::all(if active {
            colors::COPPER_BORDER
        } else {
            colors::COPPER_BORDER_WEAK
        });
        if *border != color {
            *border = color;
        }
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    for (mut text, layout_button, sidebar_tab, parent) in &mut labels {
        let label = match layout_button.map(|button| button.0) {
            Some(EditorLayoutAction::Content) => Some("导航"),
            Some(EditorLayoutAction::Inspector) => Some("工作区"),
            Some(EditorLayoutAction::Focus) => Some(if state.focus_restore.is_some() {
                "展开两侧"
            } else {
                "专注编辑"
            }),
            None if sidebar_tab.is_some()
                && parent
                    .and_then(|parent| sidebar_buttons.get(parent.parent()).ok())
                    .is_some_and(|tab| {
                        tab.0 == EditorSidebarTab::Right(EditorRightSidebarTab::Scene)
                    }) =>
            {
                Some(if author_preview { "预览" } else { "属性" })
            }
            _ => None,
        };
        if let Some(label) = label {
            let label = if label == "属性" {
                locale.text("sidebar.inspector")
            } else {
                locale.editor_label(label)
            };
            if text.as_str() != label {
                **text = label.to_owned();
            }
        }
    }
}

#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
