//! 场景文档的轻量标签与按图工具选择；不保存窗口、相机或运行时实体。

use crate::resources::SceneEditorState;
use crate::types::EditorTool;
use crate::ui::gltf_browser::{EditorGltfBrowserState, EditorGltfMeshSelection};
use bevy::prelude::*;
use feathrise_scene_content::{SceneId, ScenePortalTarget};

pub(crate) struct SceneDocumentTab {
    pub(crate) scene_id: SceneId,
    pub(crate) name: String,
    pub(crate) dirty: bool,
}

/// 共享工作区仍只有一个场景画布，按钮用稳定场景身份区分具体文档。
#[derive(Component, Clone)]
pub(crate) enum SceneDocumentButton {
    Activate(SceneId),
    Close(SceneId),
}

/// 仅记忆作者明确选择和工具参数；鼠标事务、焦点、模态及一帧请求不能跨场景恢复。
pub(crate) struct SceneDocumentUi {
    tool: EditorTool,
    radius: f32,
    strength: f32,
    spacing: f32,
    sculpt_speed: f32,
    surface_layer: usize,
    surface_preset: Option<String>,
    vegetation_layer: usize,
    vegetation_preset: Option<String>,
    vegetation_page: usize,
    water_layer: usize,
    water_preset: Option<String>,
    river: usize,
    waterfall: usize,
    water_page: usize,
    water_point: Option<usize>,
    portal: Option<String>,
    portal_target: Option<ScenePortalTarget>,
    building_preset: Option<String>,
    pub(crate) placement: Option<String>,
    pub(crate) mesh: Option<EditorGltfMeshSelection>,
    pub(crate) subscene: Option<feathrise_scene_content::SceneInstanceId>,
}

impl SceneDocumentUi {
    pub(crate) fn capture(editor: &SceneEditorState, browser: &EditorGltfBrowserState) -> Self {
        Self {
            tool: editor.tool,
            radius: editor.brush_radius,
            strength: editor.brush_strength,
            spacing: editor.brush_spacing,
            sculpt_speed: editor.terrain_sculpt_speed,
            surface_layer: editor.selected_surface_layer,
            surface_preset: editor.surface_selected_preset_id.clone(),
            vegetation_layer: editor.selected_vegetation_layer,
            vegetation_preset: editor.vegetation_selected_preset_id.clone(),
            vegetation_page: editor.vegetation_layer_page,
            water_layer: editor.selected_water_layer,
            water_preset: editor.water_selected_preset_id.clone(),
            river: editor.selected_water_river,
            waterfall: editor.selected_waterfall,
            water_page: editor.water_feature_page,
            water_point: editor.selected_water_control_point,
            portal: editor.selected_current_portal_id.clone(),
            portal_target: editor.portal_target.clone(),
            building_preset: editor.building_selected_preset_id.clone(),
            placement: browser.selected_placement_id.clone(),
            mesh: browser.selection.clone(),
            subscene: None,
        }
    }

    pub(crate) fn restore(&self, editor: &mut SceneEditorState) {
        editor.tool = self.tool;
        editor.brush_radius = self.radius;
        editor.brush_strength = self.strength;
        editor.brush_spacing = self.spacing;
        editor.terrain_sculpt_speed = self.sculpt_speed;
        editor.selected_surface_layer = self.surface_layer;
        editor.surface_selected_preset_id = self.surface_preset.clone();
        editor.selected_vegetation_layer = self.vegetation_layer;
        editor.vegetation_selected_preset_id = self.vegetation_preset.clone();
        editor.vegetation_layer_page = self.vegetation_page;
        editor.selected_water_layer = self.water_layer;
        editor.water_selected_preset_id = self.water_preset.clone();
        editor.selected_water_river = self.river;
        editor.selected_waterfall = self.waterfall;
        editor.water_feature_page = self.water_page;
        editor.selected_water_control_point = self.water_point;
        editor.selected_current_portal_id = self.portal.clone();
        editor.portal_target = self.portal_target.clone();
        editor.building_selected_preset_id = self.building_preset.clone();
    }
}
