//! 模型 LOD 检查器。沿场景树选择定位一个实际模型，预览与原始对照只属于当前视口。

use bevy::prelude::*;
use feathrise_editor_ui::localization::EditorLocale;
use feathrise_scene::{MeshLodInspection, MeshLodInspectionRequest, ScenePlacementInstance};
use feathrise_scene_content::SceneId;

use super::{PlacementTransformAction as Action, PlacementTransformInputState, PlacementTransformTarget, selected_target};
use crate::{authoring::{EditorAuthoringSession, mesh_edit::MeshEditState}, resources::SceneEditorState,
    types::EditorTool, ui::{gltf_browser::EditorGltfBrowserState, placement_water::PlacementInstanceIndex}};

#[derive(Component, Clone, Copy, Default)]
pub(crate) struct LodPreviewSummary;

pub(super) struct PlacementLodPreview {
    scene: SceneId,
    placement: String,
    triangles: bool,
    original: bool,
}

impl PlacementLodPreview {
    pub(super) fn matches(&self, target: &PlacementTransformTarget) -> bool {
        self.scene == target.scene_id && self.placement == target.placement_id
    }

    pub(super) fn selected(&self, action: Action) -> bool {
        match action {
            Action::ToggleLodPreview => true,
            Action::ToggleLodTriangles => self.triangles,
            Action::CompareLodOriginal => self.original,
            _ => false,
        }
    }
}

pub(super) fn handle(action: Action, target: &PlacementTransformTarget, state: &mut Option<PlacementLodPreview>) {
    if action == Action::ToggleLodPreview {
        *state = (!state.as_ref().is_some_and(|state| state.matches(target))).then(|| PlacementLodPreview {
            scene: target.scene_id.clone(),
            placement: target.placement_id.clone(),
            triangles: true, // 第一次打开就看见正在绘制的三角边，关闭三角线即可公平对照帧率。
            original: false,
        });
    } else if let Some(preview) = state.as_mut().filter(|state| state.matches(target)) {
        match action {
            Action::ToggleLodTriangles => preview.triangles = !preview.triangles,
            Action::CompareLodOriginal => preview.original = !preview.original,
            _ => {}
        }
    }
}

/// 使用已有摆放索引处理正式根和新作者预览的交接。异步替换根时不向旧对象继续授权。
pub(crate) fn sync_lod_preview_request(
    editor: Res<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    browser: Res<EditorGltfBrowserState>,
    mesh_edit: Option<Res<MeshEditState>>,
    selected: Option<Res<crate::viewport::multi_select::PlacementSelectionSet>>,
    view: (Option<Res<feathrise_editor_core::ProjectLaunchSettings>>, Option<Res<crate::viewport::modes::ViewportModeState>>),
    index: Res<PlacementInstanceIndex>,
    roots: Query<(&ScenePlacementInstance, Option<&Visibility>)>,
    mut transform_state: ResMut<PlacementTransformInputState>,
    mut inspection: ResMut<MeshLodInspection>,
    locale: Option<Res<EditorLocale>>,
    mut texts: Query<(&mut Text, &mut Node), With<LodPreviewSummary>>,
) {
    let context = editor.open && editor.tool == EditorTool::Inspect && !editor.close_requested
        && !editor.scene_transition_in_progress && editor.pending_scene_transition.is_none()
        && !editor.new_scene_dialog_open && !transform_state.relocation_armed()
        && view.0.as_ref().is_none_or(|launch| !launch.preview)
        && view.1.as_ref().is_none_or(|view| view.mode == crate::viewport::modes::ViewportMode::Select)
        && selected.as_ref().is_none_or(|set| set.members.len() <= 1)
        && mesh_edit.as_ref().is_none_or(|state| !state.owns_mouse());
    // 允许修改同一个模型的误差预算。换选择、编辑工具或场景时结束预览。
    let target = context.then(|| selected_target(&browser, &session)).flatten();
    if transform_state.lod_preview_available != context {
        transform_state.lod_preview_available = context;
    }
    if transform_state.lod_preview.as_ref().is_some_and(|preview|
        target.as_ref().is_none_or(|target| !preview.matches(target)))
    {
        transform_state.lod_preview = None;
    }
    let preview = transform_state.lod_preview.as_ref();
    let request = preview.and_then(|preview| {
        let active = |entity| roots.get(entity).is_ok_and(|(instance, visibility)| {
            instance.is_active() && !instance.is_terrain_patch()
                && instance.scene_id() == &preview.scene && instance.id() == preview.placement
                && visibility != Some(&Visibility::Hidden)
        });
        let root = session.placement_previews.get(&preview.placement).copied().filter(|&entity| active(entity))
            .or_else(|| index.roots_for(&preview.scene, &preview.placement).iter().copied().find(|&entity| active(entity)))?;
        Some(MeshLodInspectionRequest { root, original: preview.original, triangles: preview.triangles })
    });
    if inspection.request != request {
        if inspection.request.map(|request| request.root) != request.map(|request| request.root) {
            inspection.sample = None;
        }
        inspection.request = request;
    }
    let display = if preview.is_some() { Display::Flex } else { Display::None };
    let english = locale.as_deref() == Some(&EditorLocale::English);
    for (mut text, mut node) in &mut texts {
        if node.display != display {
            node.display = display;
        }
        if preview.is_none() {
            continue;
        }
        let summary = match inspection.sample {
            Some(sample) => {
                let lods = sample.levels.iter().enumerate().filter(|(_, active)| **active)
                    .map(|(level, _)| format!("LOD{level}")).collect::<Vec<_>>().join(" / ");
                let mode = if sample.pending {
                    if english { "Preparing LOD · " } else { "正在准备 LOD · " }
                } else { "" };
                let triangles = if english { "triangles" } else { "三角面" };
                format!("{mode}{lods} · {} / {} {triangles}", sample.displayed_triangles, sample.original_triangles)
            }
            None => if english { "Waiting for mesh statistics" } else { "等待模型网格统计" }.into(),
        };
        if text.as_str() != summary {
            **text = summary;
        }
    }
}
