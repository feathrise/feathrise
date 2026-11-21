//! 按钮与快捷键共用的对象取景入口；全图由前序content_framing读取真实内容。
//!
//! 【功能设计】所选对象复用已有双击定位的完整范围及换代门，不扫描场景实体或Mesh顶点。
//! 全图只投影当前场景AABB的八角；保留方向、投影和作者草稿，切换场景/模态中的迟到请求立即丢弃。

use bevy::prelude::*;

use super::{
    LayerCandidateState, LayerTarget, SelectedPlacementViewportState, SelectedPortalViewportState,
    SelectedWaterViewportState,
    camera_math::{framing_extent, scene_scene_frame},
};
use crate::authoring::{EditorAuthoringSession, WaterNetworkEditState};
use crate::resources::{SceneEditorNumericScrubMouseGuard, SceneEditorState, SceneEditorViewportState};
use crate::types::{EditorTool, EditorViewportFraming, WaterAuthoringFeatureKind};
use crate::ui::gltf_browser::EditorGltfBrowserState;
use crate::ui::vegetation_preset_editor::{PresetWorkspaceTab, VegetationPresetEditorState};

pub(super) fn framing_available(
    editor: &SceneEditorState,
    guard: &SceneEditorNumericScrubMouseGuard,
    water_edit: &WaterNetworkEditState,
    workspace: &VegetationPresetEditorState,
) -> bool {
    editor.open && !editor.close_requested && !editor.new_scene_dialog_open
        && !editor.scene_transition_in_progress && editor.pending_scene_transition.is_none()
        && editor.scene_switch_confirmation.is_none() && !guard.blocked()
        && !water_edit.dragging() && workspace.active_tab == PresetWorkspaceTab::Scene
}

pub(crate) fn apply_viewport_framing(
    mut editor: ResMut<SceneEditorState>,
    session: Res<EditorAuthoringSession>,
    viewport: Option<ResMut<SceneEditorViewportState>>,
    guard: Res<SceneEditorNumericScrubMouseGuard>,
    water_edit: Res<WaterNetworkEditState>,
    workspace: Res<VegetationPresetEditorState>,
    browser: Res<EditorGltfBrowserState>,
    candidates: Res<LayerCandidateState>,
    mut placement: ResMut<SelectedPlacementViewportState>,
    mut water: ResMut<SelectedWaterViewportState>,
    mut portal: ResMut<SelectedPortalViewportState>,
    registry: Option<Res<feathrise_scene_content::SceneContentRegistry>>,
    manifests: Option<Res<Assets<feathrise_scene_content::SceneManifestAsset>>>,
    transform: Option<Res<super::subscenes::SubsceneTransformState>>,
    terrain: (Option<Res<crate::ui::terrain_objects::TerrainObjectsEditorState>>,
        Option<Res<feathrise_scene::TerrainObjectTransformPreview>>),
    mut group_focus: Option<ResMut<super::group_focus::GroupFocusState>>,
) {
    if editor.viewport_framing_requested.is_none() {
        return;
    }
    let request = editor
        .viewport_framing_requested
        .take()
        .expect("已确认存在一次取景请求");
    if let Some(state) = group_focus.as_mut() { state.cancel(); }
    if !framing_available(&editor, &guard, &water_edit, &workspace) {
        return; // 不把一次按键缓存到其他场景、中央标签或模态结束后补执行。
    }
    let Some(draft) = session.draft.as_ref() else {
        return;
    };
    let Some(mut viewport) = viewport else {
        return;
    };
    if viewport.framed_scene_id.as_ref() != Some(&draft.scene_id)
        || editor.selected_scene_id.as_ref() != Some(&draft.scene_id)
    {
        return; // 必须由已采用的真实场景提供身份，目录高亮不能伪装成当前场景。
    }
    placement.cancel_focus();
    water.cancel_focus();
    portal.cancel_focus(); // 新导航意图覆盖未完成的旧双击，Home不会随后又被旧定位抢回。
    if request == EditorViewportFraming::Scene { return; } // 不能回退到旧清单矩形。
    if request == EditorViewportFraming::TreePlacement {
        // 显式树命令已发布摆放身份，不能沿用当前水域等工具记住的旧索引。
        let requested = matches!(candidates.selected.as_ref(), Some(LayerTarget::Placement(id))
            if browser.selected_placement_id.as_ref() == Some(id))
            && placement.request_selected_focus(&editor, &browser, &session, &candidates);
        editor.status = if requested { "正在聚焦所选摆放，等待当前完整范围" } else { "所选摆放已变化，无法聚焦" }.into();
        return;
    }
    if let (Some(group), Some(mut state)) = (browser.selected_group.as_deref(), group_focus) {
        state.request(group, &draft.scene_id, &session, &viewport, editor.orthographic);
        editor.status = format!("正在按可见成员计算组「{group}」的范围");
        return;
    }
    let (terrains, terrain_preview) = terrain;
    if let Some(LayerTarget::TerrainObject(id)) = candidates.selected.as_ref()
        && let Some(object)=terrains.as_ref().and_then(|state|crate::ui::terrain_objects::selected(&session,state)).filter(|object|object.id.as_str()==id) {
        let fallback_min=object.voxel.as_ref().map_or(Vec3::new(object.world_min[0],object.terrain.base_height,object.world_min[1]),|v|Vec3::from_array(v.min));
        let fallback_max=object.voxel.as_ref().map_or(Vec3::new(object.world_max[0],object.terrain.base_height+object.terrain.height_scale,object.world_max[1]),|v|Vec3::from_array(v.max));
        let (min,max)=draft.terrain_ground.as_ref().filter(|ground|ground.revision==session.terrain_revision)
            .and_then(|ground|ground.local_bounds(&object.id)).unwrap_or((fallback_min,fallback_max));
        let pose=terrain_preview.as_ref().and_then(|preview|preview.for_object(&draft.scene_id,&object.id)).unwrap_or(object.transform).as_transform();
        let matrix=pose.to_matrix();let center=matrix.transform_point3((min+max)*0.5);let half=(max-min)*0.5;
        let world_half=matrix.transform_vector3(Vec3::X).abs()*half.x+matrix.transform_vector3(Vec3::Y).abs()*half.y+matrix.transform_vector3(Vec3::Z).abs()*half.z;
        viewport.center=center;viewport.extent=framing_extent(world_half,viewport.yaw,viewport.pitch,viewport.render_size,editor.orthographic);
        editor.status=format!("已聚焦地形 {}",object.display_name);return;
    }
    if let Some(LayerTarget::Subscene(id)) = candidates.selected.as_ref() {
        let references = transform.as_ref().and_then(|state| state.preview(&session)).unwrap_or(&draft.manifest.subscenes);
        let record = references.iter().find(|record| record.id.as_str() == id);
        let manifest = record.and_then(|record| registry.as_ref()?.handles(&record.source_scene_id))
            .and_then(|handles| manifests.as_ref()?.get(&handles.manifest));
        if let (Some(record), Some(manifest), Some(bounds)) = (record, manifest, manifest.and_then(|manifest| manifest.world_bounds())) {
            let (center, half) = scene_scene_frame(bounds.center(), bounds.size(), manifest.terrain.base_height, manifest.terrain.height_scale);
            let rotation = record.transform.as_transform().rotation;
            let world_half = (rotation * Vec3::X).abs() * half.x + Vec3::Y * half.y + (rotation * Vec3::Z).abs() * half.z;
            viewport.center = record.transform.to_world(center);
            viewport.extent = framing_extent(world_half, viewport.yaw, viewport.pitch, viewport.render_size, editor.orthographic);
            editor.status = format!("已聚焦子场景 {}", record.label);
        } else { editor.status = "子场景源清单尚未就绪，请稍后聚焦".into(); }
        return;
    }
    // 贴地换代可清空候选列表，正式视口选择仍决定取景类别，不能回退到旧资产。
    let has_viewport_selection = editor.tool == EditorTool::Inspect && candidates.has_viewport_selection();
    let requested = if has_viewport_selection {
        match candidates.selected.as_ref() {
            Some(LayerTarget::Placement(_)) => {
                placement.request_selected_focus(&editor, &browser, &session, &candidates)
            }
            Some(LayerTarget::Water(kind, id)) => {
                water.request_focus(&draft.scene_id, *kind, id);
                true
            }
            Some(LayerTarget::Portal(id)) => {
                portal.request_focus(&draft.scene_id, id);
                true
            }
            _ => false, // 作者密度层不冒充某一株实例；不回退到另一类别的旧场景树选择。
        }
    } else {
        match editor.tool {
            EditorTool::Inspect | EditorTool::PlaceAsset => {
                placement.request_selected_focus(&editor, &browser, &session, &candidates)
            }
            EditorTool::Portal => editor
                .selected_current_portal_id
                .as_deref()
                .is_some_and(|id| {
                    portal.request_focus(&draft.scene_id, id);
                    true
                }),
            EditorTool::Water | EditorTool::RiverSpline | EditorTool::Waterfall => {
                let selected = draft
                    .water
                    .as_ref()
                    .and_then(|definition| match editor.tool {
                        EditorTool::Water => definition
                            .layers
                            .get(editor.selected_water_layer)
                            .map(|item| (WaterAuthoringFeatureKind::StillWater, item.id.as_str())),
                        EditorTool::RiverSpline => definition
                            .rivers
                            .get(editor.selected_water_river)
                            .map(|item| (WaterAuthoringFeatureKind::River, item.id.as_str())),
                        EditorTool::Waterfall => definition
                            .waterfalls
                            .get(editor.selected_waterfall)
                            .map(|item| (WaterAuthoringFeatureKind::Waterfall, item.id.as_str())),
                        _ => None,
                    });
                selected.is_some_and(|(kind, id)| {
                    water.request_focus(&draft.scene_id, kind, id);
                    true
                })
            }
            _ => false,
        }
    };
    editor.status = if requested {
        "正在聚焦所选，等待当前完整范围"
    } else {
        "请先选择资产、水体或传送点，再聚焦所选"
    }
    .into();
}

#[cfg(test)]
#[path = "framing_tests.rs"]
mod tests;
