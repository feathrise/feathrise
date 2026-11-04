//! 工具请求先经过全局选择/编辑门；只读原水域目标与联合预览回执，禁止另建作者状态。

use feathrise_editor_ui::EditorButtonClick;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use feathrise_map_content::WaterPresetRegistry;

use super::types::WaterSidebarFeedbackKind;
use super::{components::*, resources::*};
use crate::authoring::{
    EditorAuthoringSession, TerrainExternalChangeState, WaterBrushStroke, WaterNetworkEditState,
    WaterPreviewState,
};
use crate::resources::{
    MapBrowserDragState, MapBrowserState, MapDeletionTask, MapEditorNumericScrubMouseGuard,
    MapEditorState,
};
use crate::ui::sidebars::{EditorLeftSidebarTab, MapEditorSidebarState};
use crate::ui::text_input::EditorTextInputGuard;
use crate::ui::theme as colors;
use crate::viewport::modes::{ViewportMode, ViewportModeState};

#[derive(SystemParam)]
pub(super) struct WaterSidebarInputGate<'w> {
    numeric: Res<'w, MapEditorNumericScrubMouseGuard>,
    numeric_input: Option<Res<'w, feathrise_editor_ui::ActiveNumericInput>>,
    text: Res<'w, EditorTextInputGuard>,
    browser: Option<Res<'w, MapBrowserState>>,
    drag: Option<Res<'w, MapBrowserDragState>>,
    deletion: Option<Res<'w, MapDeletionTask>>,
    conflict: Option<Res<'w, TerrainExternalChangeState>>,
}

impl WaterSidebarInputGate<'_> {
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
    tabs: Query<(&EditorButtonClick, &WaterSidebarTab), Changed<EditorButtonClick>>,
    gate: WaterSidebarInputGate,
    sidebar: Res<MapEditorSidebarState>,
    session: Res<EditorAuthoringSession>,
    stroke: Res<WaterBrushStroke>,
    network: Res<WaterNetworkEditState>,
    mut editor: ResMut<MapEditorState>,
    mut state: ResMut<WaterSidebarState>,
) {
    if !editor.open
        || editor.close_requested
        || editor.new_map_dialog_open
        || editor.map_transition_in_progress
        || !sidebar.tool_page_visible(EditorLeftSidebarTab::Water)
        || session.saving
        || stroke.active
        || network.dragging()
        || gate.blocked()
    {
        return; // 系统保持运行以消费Changed边沿，解锁后不重放模态/拖动期间的旧点击。
    }
    // 统一列表仍按类别+稳定ID选择；在模式门把请求收成Inspect之前记住它的类别。
    if let Some(page) = WaterSidebarPage::for_tool(editor.tool) {
        if state.page != page {
            state.page = page;
        }
    }
    for (interaction, tab) in &tabs {
        if interaction.is_clicked() && state.page != tab.0 {
            state.page = tab.0;
            editor.tool = tab.0.tool(); // 仅发布准备工具，不创建对象、不改索引，也不强行进入编辑模式。
        }
    }
}

pub(super) fn sync_pages(
    editor: Res<MapEditorState>,
    sidebar: Res<MapEditorSidebarState>,
    mut state: ResMut<WaterSidebarState>,
    mut panels: Query<(&WaterSidebarPanel, &mut Node)>,
    mut tabs: Query<(
        &Interaction,
        &WaterSidebarTab,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    if editor.open && sidebar.tool_page_visible(EditorLeftSidebarTab::Water) {
        if let Some(page) = WaterSidebarPage::for_tool(editor.tool) {
            if state.page != page {
                state.page = page; // 外部视口选择和创建沿用原工具，工具页跟随真实请求。
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
        let selected = tab.0 == state.page;
        let fill = match (*interaction, selected) {
            (Interaction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (Interaction::Hovered, _) => colors::SELECT_BG_HOVER,
            (Interaction::None, true) => colors::SELECT_BG,
            (Interaction::None, false) => colors::BUTTON_DEV_SMALL_NORMAL,
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

#[derive(SystemParam)]
pub(super) struct WaterSidebarFeedback<'w> {
    session: Res<'w, EditorAuthoringSession>,
    registry: Option<Res<'w, WaterPresetRegistry>>,
    preview: Option<Res<'w, WaterPreviewState>>,
    stroke: Res<'w, WaterBrushStroke>,
    network: Res<'w, WaterNetworkEditState>,
}

impl WaterSidebarFeedback<'_> {
    fn availability(&self, page: WaterSidebarPage, editor: &MapEditorState) -> (String, Color) {
        let waiting = |message: &str| (message.to_owned(), colors::TEXT_ACCENT);
        if page == WaterSidebarPage::Still {
            if let Some(error) = self
                .registry
                .as_ref()
                .and_then(|registry| registry.last_error.as_deref())
            {
                return (
                    format!("水体预设目录加载失败：{error}"),
                    colors::DANGER_TEXT,
                );
            }
        }
        let Some(draft) = self
            .session
            .draft
            .as_ref()
            .filter(|draft| Some(&draft.map_id) == editor.selected_map_id.as_ref())
        else {
            return waiting("等待当前地图作者数据；全局静水预设仍可管理");
        };
        if self.session.saving {
            return waiting("正在保存当前地图；等待完整水域事务完成");
        }
        if self.stroke.active {
            return waiting("静水笔划正在进行或收尾；松开后再切换对象或工具页");
        }
        if self.network.dragging() {
            return waiting("正在拖动水网控制点；松开后形成一条完整撤销记录");
        }
        if page != WaterSidebarPage::Still && !draft.terrain_manifest().terrain.kind.supports_spline_water() {
            return waiting("当前地形不支持河流、急流与瀑布编辑；可查看已有对象");
        }
        let Some(water) = draft.water.as_ref() else {
            return waiting("当前地图尚无水域作者数据；使用上方入口显式创建");
        };
        let has_target = match page {
            WaterSidebarPage::Still => water.layers.get(editor.selected_water_layer).is_some(),
            WaterSidebarPage::River => water.rivers.get(editor.selected_water_river).is_some(),
            WaterSidebarPage::Waterfall => {
                water.waterfalls.get(editor.selected_waterfall).is_some()
            }
        };
        if !has_target {
            return waiting("当前类别尚无有效目标；从统一列表选择，或显式新建");
        }
        if let Some(preview) = self.preview.as_ref() {
            if preview.failed_for(
                &draft.map_id,
                self.session.water_revision,
                self.session.terrain_revision,
            ) {
                return (
                    "当前水面预览失败；保留上一完整结果，具体原因见最近操作".into(),
                    colors::DANGER_TEXT,
                );
            }
            if preview.ready_for(
                &draft.map_id,
                self.session.water_revision,
                self.session.terrain_revision,
            ) {
                return (
                    "水面预览已采用当前修订；空覆盖或干涸水体可能没有可见水面".into(),
                    colors::TEXT_VALUE,
                );
            }
        }
        waiting("水面预览等待当前完整联合修订；不将旧画面视为本次完成")
    }
}

pub(super) fn sync_feedback(
    editor: Res<MapEditorState>,
    sidebar: Res<MapEditorSidebarState>,
    mode: Res<ViewportModeState>,
    state: Res<WaterSidebarState>,
    feedback: WaterSidebarFeedback,
    mut texts: Query<(&WaterSidebarFeedbackText, &mut Text, &mut TextColor)>,
) {
    if !editor.open || !sidebar.tool_page_visible(EditorLeftSidebarTab::Water) {
        return;
    }
    let page = state.page;
    let editing = mode.mode == ViewportMode::Edit && editor.tool == page.tool();
    let mode_text = format!(
        "{} · {}{}",
        if editing { "编辑" } else { "选择" },
        if editing { "" } else { "预备" },
        page.label()
    );
    let (availability, tint) = feedback.availability(page, &editor);
    let target = feedback
        .session
        .draft
        .as_ref()
        .filter(|draft| Some(&draft.map_id) == editor.selected_map_id.as_ref())
        .and_then(|draft| draft.water.as_ref())
        .and_then(|water| match page {
            WaterSidebarPage::Still => water.layers.get(editor.selected_water_layer).map(|layer| {
                format!(
                    "{} [{}] · 世界Y {:.3}m",
                    layer.label, layer.id, layer.water_level
                )
            }),
            WaterSidebarPage::River => water.rivers.get(editor.selected_water_river).map(|river| {
                format!(
                    "{} [{}] · {}个控制点",
                    river.label,
                    river.id,
                    river.points.len()
                )
            }),
            WaterSidebarPage::Waterfall => {
                water
                    .waterfalls
                    .get(editor.selected_waterfall)
                    .map(|waterfall| {
                        format!(
                            "{} [{}] · {}个控制点",
                            waterfall.label,
                            waterfall.id,
                            waterfall.points.len()
                        )
                    })
            }
        })
        .map_or_else(
            || "当前工具目标：尚未选择或创建".to_owned(),
            |label| {
                format!(
                    "{}：{label}",
                    if editing {
                        "当前工具目标"
                    } else {
                        "预备目标"
                    }
                )
            },
        );
    for (marker, mut text, mut color) in &mut texts {
        let (value, color_value) = match marker.0 {
            WaterSidebarFeedbackKind::Mode => (mode_text.as_str(), colors::TEXT_ACCENT),
            WaterSidebarFeedbackKind::Availability => (availability.as_str(), tint),
            WaterSidebarFeedbackKind::Message => (editor.status.as_str(), colors::TEXT_MUTED),
            WaterSidebarFeedbackKind::Target => (target.as_str(), colors::TEXT_VALUE),
        };
        if text.as_str() != value {
            **text = value.to_owned();
        }
        if color.0 != color_value {
            color.0 = color_value;
        }
    }
}
