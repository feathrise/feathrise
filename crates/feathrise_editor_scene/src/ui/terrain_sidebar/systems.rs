//! 工具页请求先经过原视口模式门；反馈只读固定身份和原任务状态，不创建作者任务。

use feathrise_editor_ui::UiInteraction;
use feathrise_editor_ui::EditorButtonClick;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;
use feathrise_scene_content::{SceneTerrainKind, SurfacePresetRegistry};
use feathrise_scene::ClipmapGroundHit;

use super::{components::*, resources::*};
use crate::authoring::{
    EditorAuthoringSession, PuddleAuthoringState, SurfaceBrushStroke, SurfaceLayerEditTask,
    TerrainExternalChangeState, TerrainPreviewState,
};
use crate::components::SceneEditorViewport;
use crate::resources::{
    SceneBrowserDragState, SceneBrowserState, SceneDeletionTask, SceneEditorNumericScrubMouseGuard,
    SceneEditorState,
};
use crate::types::{EditorTool, EditorView};
use crate::ui::sidebars::{EditorLeftSidebarTab, SceneEditorSidebarState};
use crate::ui::text_input::EditorTextInputGuard;
use crate::ui::theme as colors;
use crate::viewport::modes::{ViewportMode, ViewportModeState};
use crate::viewport::vegetation_ground::VegetationGroundCursor;

#[derive(SystemParam)]
pub(super) struct TerrainSidebarInputGate<'w> {
    numeric: Res<'w, SceneEditorNumericScrubMouseGuard>,
    text: Res<'w, EditorTextInputGuard>,
    browser: Option<Res<'w, SceneBrowserState>>,
    drag: Option<Res<'w, SceneBrowserDragState>>,
    deletion: Option<Res<'w, SceneDeletionTask>>,
    conflict: Option<Res<'w, TerrainExternalChangeState>>,
}

impl TerrainSidebarInputGate<'_> {
    pub(super) fn modal_blocked(&self) -> bool {
        self.browser.as_ref().is_some_and(|s| s.dialog.is_some())
            || self.drag.as_ref().is_some_and(|s| s.blocks_other_mouse())
            || self.deletion.as_ref().is_some_and(|s| s.busy())
            || self.conflict.as_ref().is_some_and(|s| s.modal_open())
    }

    pub(super) fn blocked(&self) -> bool {
        self.numeric.blocked()
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
    tabs: Query<(&EditorButtonClick, &TerrainSidebarTab), Changed<EditorButtonClick>>,
    gate: TerrainSidebarInputGate,
    sidebar: Res<SceneEditorSidebarState>,
    mut editor: ResMut<SceneEditorState>,
    mut state: ResMut<TerrainSidebarState>,
) {
    if !editor.open
        || editor.close_requested
        || editor.new_scene_dialog_open
        || !sidebar.tool_page_visible(EditorLeftSidebarTab::Terrain)
        || gate.blocked()
    {
        return; // 仍执行Changed查询推进tick，模态/拖动期间的点击不会在解锁后重放。
    }
    // 捕获选择预设、点击雕刻按钮的请求，再由原sync_mode决定是否真的进入写入工具。
    if TerrainSidebarPage::for_tool(editor.tool).is_some() && state.requested_tool() != editor.tool
    {
        state.observe_tool(editor.tool);
    }
    for (interaction, tab) in &tabs {
        if !interaction.is_clicked() || tab.0 == state.page {
            continue;
        }
        state.page = tab.0;
        crate::ui::actions::request_terrain_tool(state.requested_action(), &mut editor);
        // 不强行打开编辑模式，不重置画布、选择或参数。切工具的原收笔路径继续收口已受理样本。
    }
}

pub(super) fn sync_pages(
    editor: Res<SceneEditorState>,
    sidebar: Res<SceneEditorSidebarState>,
    mut state: ResMut<TerrainSidebarState>,
    mut panels: Query<(&TerrainSidebarPanel, &mut Node)>,
    mut controls: Query<
        (
            &UiInteraction,
            Option<&TerrainSidebarTab>,
            Option<&TerrainSidebarTool>,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Or<(With<TerrainSidebarTab>, With<TerrainSidebarTool>)>,
    >,
) {
    if editor.open
        && !editor.new_scene_dialog_open
        && sidebar.tool_page_visible(EditorLeftSidebarTab::Terrain)
        && TerrainSidebarPage::for_tool(editor.tool).is_some()
        && state.requested_tool() != editor.tool
    {
        state.observe_tool(editor.tool); // 全局“编辑”或场景点选恢复真实工具时显示对应工具页。
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
    // 仅固定页签和雕刻按钮；不把半径变化变成全局预设目录着色扫描。
    for (interaction, tab, tool, mut background, mut border) in &mut controls {
        let selected = tab.is_some_and(|tab| tab.0 == state.page)
            || tool.is_some_and(|tool| tool.0 == state.sculpt_tool);
        let fill = match (*interaction, selected) {
            (UiInteraction::Pressed, _) => colors::SELECT_BG_PRESSED,
            (UiInteraction::Hovered, _) => colors::SELECT_BG_HOVER,
            (UiInteraction::None, true) => colors::SELECT_BG,
            (UiInteraction::None, false) => colors::BUTTON_DEV_SMALL_NORMAL,
        };
        // 雕刻与三维工具沿用导航选中填色；普通选中不借用输入焦点的亮边。
        let outline = BorderColor::all(colors::COPPER_BORDER_WEAK);
        if background.0 != fill {
            background.0 = fill;
        }
        if *border != outline {
            *border = outline;
        }
    }
}

/// 独立网格按作者源能力开放塑形；兼容主网格继续由原工具门保持历史操作行为。
pub(super) fn sync_tools(
    mut commands: Commands,
    session: Res<EditorAuthoringSession>,
    selection: Option<Res<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
    tools: Query<(Entity, &TerrainSidebarTool, Has<bevy::ui::InteractionDisabled>)>,
) {
    let Some(draft) = session.draft.as_ref() else { return; };
    let independent = draft.manifest.terrains.is_some();
    let object = selection.as_deref().and_then(|state| crate::ui::terrain_objects::selected(&session, state));
    let writable = object.is_some_and(|object| object.enabled && selection.as_deref().is_some_and(|state| {
        !state.locked.contains(&object.id) && !state.hidden.contains(&object.id)
            && !state.preparing() && !state.documents_loading && !state.transforming
    }));
    for (entity, tool, disabled) in &tools {
        let supported = !independent || (writable && object.is_some_and(|object| {
            if tool.0.is_volume_sculpt() {
                object.terrain.kind.supports_volume_sculpt()
            } else {
                object.terrain.kind.supports_height_sculpt()
            }
        }));
        if supported && disabled { commands.entity(entity).remove::<bevy::ui::InteractionDisabled>(); }
        if !supported && !disabled { commands.entity(entity).insert(bevy::ui::InteractionDisabled); }
    }
}

#[derive(SystemParam)]
pub(super) struct TerrainSidebarFeedback<'w> {
    volume: Option<Res<'w, crate::authoring::volume_sculpt::VolumeAuthoringState>>,
    session: Res<'w, EditorAuthoringSession>,
    registry: Option<Res<'w, SurfacePresetRegistry>>,
    puddles: Option<Res<'w, PuddleAuthoringState>>,
    surface_task: Option<Res<'w, SurfaceLayerEditTask>>,
    surface_stroke: Option<Res<'w, SurfaceBrushStroke>>,
    terrain_preview: Option<Res<'w, TerrainPreviewState>>,
    ground: Option<Res<'w, VegetationGroundCursor>>,
    selection: Option<Res<'w, crate::ui::terrain_objects::TerrainObjectsEditorState>>,
}

impl TerrainSidebarFeedback<'_> {
    fn availability(
        &self,
        page: TerrainSidebarPage,
        editor: &SceneEditorState,
        over_viewport: bool,
    ) -> (String, Color) {
        let waiting = |message: &str| (message.to_owned(), colors::TEXT_ACCENT);
        let Some(draft) = self
            .session
            .draft
            .as_ref()
            .filter(|draft| Some(&draft.scene_id) == editor.selected_scene_id.as_ref())
        else {
            return waiting("等待当前场景作者数据；全局预设仍可管理");
        };
        if self.session.saving {
            return waiting("正在保存当前场景；等待完整作者事务完成");
        }
        let selected = self.selection.as_deref().and_then(|state| crate::ui::terrain_objects::selected(&self.session, state));
        if draft.manifest.terrains.is_some() {
            let Some(object) = selected else {
                return waiting("先在左侧场景树选择网格；空场景可用顶部“网格”创建");
            };
            let state = self.selection.as_deref().expect("已选择网格必然有选择资源");
            if !object.enabled || state.locked.contains(&object.id) || state.hidden.contains(&object.id) {
                return waiting("所选网格未参与场景、已隐藏或锁定；恢复后才能塑形");
            }
            if state.documents_loading || state.preparing() || state.transforming {
                return waiting("所选网格作者数据或变换正在准备；保留上一完整结果");
            }
            if page == TerrainSidebarPage::Sculpt && object.terrain.kind == SceneTerrainKind::Gltf {
                return ("原始 GLB 保留资产几何；当前塑形工具适用于高度网格、Clipmap 和体素".into(), colors::TEXT_MUTED);
            }
        }
        let kind = selected.map_or(draft.terrain_manifest().terrain.kind, |object| object.terrain.kind);
        if editor.tool.is_volume_sculpt() {
            if !kind.supports_volume_sculpt() {
                return ("三维雕刻适用于Clipmap和体素网格".into(), colors::DANGER_TEXT);
            }
            if self.volume.as_ref().is_some_and(|v| v.busy()) {
                return waiting("三维笔划计算/联合碰撞正在进行；松手后可保存或撤销");
            }
        }
        if page == TerrainSidebarPage::Surface {
            if let Some(error) = self
                .registry
                .as_ref()
                .and_then(|registry| registry.last_error.as_deref())
            {
                return (format!("地表目录加载失败：{error}"), colors::DANGER_TEXT);
            }
            if self.registry.is_none() {
                return waiting("等待全局地表预设目录");
            }
            if self
                .surface_task
                .as_ref()
                .is_some_and(|task| task.is_deleting())
            {
                return waiting("正在删除场景层；保留上一完整地表，完成后可撤销");
            }
            if self
                .surface_task
                .as_ref()
                .is_some_and(|task| task.is_pending())
            {
                return waiting("正在准备地表作者画布；等待完整结果");
            }
            if self
                .surface_stroke
                .as_ref()
                .is_some_and(|stroke| stroke.active)
            {
                return waiting("地表笔划正在进行或收口；已受理样本按原事务完成");
            }
            if draft
                .surface
                .as_ref()
                .and_then(|surface| {
                    crate::ui::surface_layers::selected_index(
                        surface,
                        editor,
                        self.registry.as_deref(),
                    )
                })
                .is_none()
            {
                return waiting("请先选择预设，并显式新建或选中当前场景层");
            }
        }
        if page == TerrainSidebarPage::Puddles {
            if let Some(error) = self
                .puddles
                .as_ref()
                .and_then(|puddles| puddles.preview_error_for(draft))
            {
                return (format!("积水预览失败：{error}"), colors::DANGER_TEXT);
            }
            if self.puddles.as_ref().is_some_and(|puddles| puddles.busy()) {
                return waiting("积水笔划正在进行或收口；等待完整作者结果");
            }
            if self
                .puddles
                .as_ref()
                .is_some_and(|puddles| puddles.preview_pending_for(draft))
            {
                return waiting("积水预览正在准备；保留上一完整画面");
            }
        }
        if page == TerrainSidebarPage::Sculpt
            && self
                .terrain_preview
                .as_ref()
                .is_some_and(|preview| preview.task.is_some())
        {
            return waiting("地形画面与碰撞正在准备；保留上一完整结果");
        }
        if editor.tool == EditorTool::Inspect {
            return (
                "当前仅选择；点击视口“编辑”后才允许落笔".to_owned(),
                colors::TEXT_MUTED,
            );
        }
        if matches!(editor.view, EditorView::Front | EditorView::Side)
            && !editor.tool.is_volume_sculpt()
        {
            return waiting("前视/侧视不接受新笔划；请切换透视或顶视");
        }
        if kind == SceneTerrainKind::SparseClipmap
            && over_viewport
            && editor.brush_cursor_world.is_none()
        {
            let status = self
                .ground
                .as_ref()
                .and_then(|ground| ground.completed_status());
            return match status {
                Some(ClipmapGroundHit::Failed) => (
                    "精确地形查询失败；未写入作者数据".to_owned(),
                    colors::DANGER_TEXT,
                ),
                Some(ClipmapGroundHit::Miss) => waiting("未命中可编辑地形；未写入作者数据"),
                _ => waiting("等待精确地形落点或作者高度页；不会使用零平面落笔"),
            };
        }
        (
            "场景已加载；仅在有效视口落点上绘制".to_owned(),
            colors::TEXT_VALUE,
        )
    }
}

pub(super) fn sync_feedback(
    editor: Res<SceneEditorState>,
    locale: Option<Res<feathrise_editor_ui::localization::EditorLocale>>,
    sidebar: Res<SceneEditorSidebarState>,
    mode: Res<ViewportModeState>,
    state: Res<TerrainSidebarState>,
    feedback: TerrainSidebarFeedback,
    viewports: Query<&RelativeCursorPosition, With<SceneEditorViewport>>,
    mut texts: Query<
        (
            &mut Text,
            &mut TextColor,
            Has<TerrainSidebarModeText>,
            Has<TerrainSidebarHintText>,
            Has<TerrainSidebarAvailabilityText>,
            Has<TerrainSidebarMessageText>,
            Has<TerrainSidebarPuddleTargetText>,
            Has<TerrainSidebarMeshContext>,
        ),
        Or<(
            With<TerrainSidebarModeText>,
            With<TerrainSidebarHintText>,
            With<TerrainSidebarAvailabilityText>,
            With<TerrainSidebarMessageText>,
            With<TerrainSidebarPuddleTargetText>,
            With<TerrainSidebarMeshContext>,
        )>,
    >,
) {
    if !editor.open || !sidebar.tool_page_visible(EditorLeftSidebarTab::Terrain) {
        return;
    }
    let locale = locale.as_deref().copied().unwrap_or_default();
    let label = match state.requested_tool() {
        EditorTool::Raise => "抬高",
        EditorTool::VolumeDig => "三维挖掘",
        EditorTool::VolumeFill => "三维填充",
        EditorTool::VolumeSmooth => "三维平滑",
        EditorTool::VolumeFlatten => "三维整平",
        EditorTool::Lower => "降低",
        EditorTool::Smooth => "平滑",
        EditorTool::Flatten => "压平",
        EditorTool::Surface => "地表绘制",
        _ => "积水潜力",
    };
    let mode_key = if mode.mode == ViewportMode::Select || editor.tool == EditorTool::Inspect {
        "terrain.mode.select"
    } else {
        "terrain.mode.edit"
    };
    let mode_value = locale.format(mode_key, &[("tool", locale.editor_label(label).to_owned())]);
    let hint = match state.page {
        TerrainSidebarPage::Sculpt => {
            "先选网格和工具，再点视口左侧“编辑”后左键雕刻；平滑降低局部高差，压平以起笔高度为目标，两者按雕刻速度接近目标并停止。半径、强度和雕刻速度在视口工具条调整。"
        }
        TerrainSidebarPage::Surface => {
            "选择预设 → 新建层 → 编辑绘制；Shift擦除。删除场景层不删除全局预设。"
        }
        TerrainSidebarPage::Puddles => {
            "左键绘制，Shift擦除；零值禁止积水。切换工具查看随天气变化的实际水洼。"
        }
    };
    let over_viewport =
        !mode.pointer_over_controls && viewports.iter().any(|cursor| cursor.cursor_over());
    let (availability, availability_color) =
        feedback.availability(state.page, &editor, over_viewport);
    let mesh_context = feedback.session.draft.as_ref().map_or_else(
        || "当前网格：等待场景作者数据".to_owned(),
        |draft| {
            if draft.manifest.terrains.is_some() {
                feedback.selection.as_deref().and_then(|selection| crate::ui::terrain_objects::selected(&feedback.session, selection))
                    .map_or_else(|| "当前网格：尚未选择 · 从左侧场景树选择，或用顶部“网格”创建".to_owned(), |object| {
                        let capability = match object.terrain.kind {
                            SceneTerrainKind::Gltf => "资产几何 · 无源塑形",
                            SceneTerrainKind::Heightfield => "高度雕刻",
                            SceneTerrainKind::SparseClipmap => "高度与三维雕刻",
                            SceneTerrainKind::Voxel => "三维雕刻",
                            SceneTerrainKind::Empty => "作者源未就绪",
                        };
                        locale.format("terrain.mesh.selected", &[
                            ("name", object.display_name.clone()),
                            ("capability", locale.editor_label(capability).to_owned()),
                        ])
                    })
            } else { "当前网格：兼容主网格 · 工具沿用原作者源".to_owned() }
        },
    );
    let puddle_target = feedback
        .session
        .draft
        .as_ref()
        .filter(|draft| Some(&draft.scene_id) == editor.selected_scene_id.as_ref())
        .map_or_else(
            || "等待当前场景作者数据".to_owned(),
            |draft| {
                draft.manifest.puddles.as_ref().map_or_else(
                    || "当前场景：尚未创建积水画布；首次有效笔划创建".to_owned(),
                    |layer| {
                        format!(
                            "当前场景：独立 R8 潜力画布 · {} × {}",
                            layer.resolution[0], layer.resolution[1]
                        )
                    },
                )
            },
        );
    let message = if editor.status.is_empty() {
        String::new()
    } else {
        locale.format("terrain.recent", &[("action", locale.editor_notice(&editor.status).into_owned())])
    };
    for (mut text, mut color, is_mode, is_hint, is_availability, is_message, is_puddle, is_mesh) in
        &mut texts
    {
        let (value, tint) = if is_mesh {
            (mesh_context.as_str(), colors::TEXT_MUTED)
        } else if is_mode {
            (mode_value.as_str(), colors::TEXT_VALUE)
        } else if is_hint {
            (hint, colors::TEXT_MUTED)
        } else if is_availability {
            (availability.as_str(), availability_color)
        } else if is_message {
            (message.as_str(), colors::TEXT_MUTED)
        } else if is_puddle {
            (puddle_target.as_str(), colors::TEXT_VALUE)
        } else {
            continue;
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
