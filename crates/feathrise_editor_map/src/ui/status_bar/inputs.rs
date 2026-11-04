//! 从已有保存、切图与五项正式运行时状态读取阶段，不轮询任务或改变事务。

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use feathrise_scene::{
    DynamicMapRuntimePhase, DynamicMapRuntimeStatus, DynamicVegetationPhase,
    DynamicVegetationStatus, DynamicWaterPhase, DynamicWaterStatus, MapPlacementRuntimePhase,
    MapPlacementRuntimeStatus, MapPortalRuntimePhase, MapPortalRuntimeStatus,
};

use super::types::{RuntimeEntry, RuntimePhase};
use crate::authoring::{EditorAuthoringSession, TerrainExternalChangeState};
use crate::resources::{
    MapBrowserDragState, MapBrowserState, MapCreationTask, MapDeletionTask, MapEditorLayoutState,
    MapEditorSettingsState, MapEditorState,
};

#[derive(SystemParam)]
pub(super) struct StatusInputs<'w> {
    pub(super) editor: Res<'w, MapEditorState>,
    pub(super) session: Option<Res<'w, EditorAuthoringSession>>,
    loading: Option<Res<'w, feathrise_scene_core::state::LoadingProgress>>,
    creation: Option<Res<'w, MapCreationTask>>,
    deletion: Option<Res<'w, MapDeletionTask>>,
    settings: Option<Res<'w, MapEditorSettingsState>>,
    folders: Option<Res<'w, MapBrowserState>>,
    conflict: Option<Res<'w, TerrainExternalChangeState>>,
    asset_menu: Option<Res<'w, crate::ui::asset_context_menu::AssetContextMenuState>>,
    tab_menu: Option<Res<'w, crate::ui::asset_preview::AssetTabMenuState>>,
    map_drag: Option<Res<'w, MapBrowserDragState>>,
    layout: Option<Res<'w, MapEditorLayoutState>>,
    numeric: Option<Res<'w, feathrise_editor_ui::ActiveNumericInputScrub>>,
    dynamic_map: Option<Res<'w, DynamicMapRuntimeStatus>>,
    vegetation: Option<Res<'w, DynamicVegetationStatus>>,
    water: Option<Res<'w, DynamicWaterStatus>>,
    placements: Option<Res<'w, MapPlacementRuntimeStatus>>,
    portals: Option<Res<'w, MapPortalRuntimeStatus>>,
    surface_presets: Option<Res<'w, crate::ui::surface_preset_editor::SurfacePresetEditorState>>,
    water_presets: Option<Res<'w, crate::ui::water_preset_editor::WaterPresetEditorState>>,
    vegetation_presets:
        Option<Res<'w, crate::ui::vegetation_preset_editor::VegetationPresetEditorState>>,
}

impl StatusInputs<'_> {
    pub(super) fn loading_failed(&self) -> bool {
        self.editor.map_transition_in_progress
            && self.loading.as_ref().is_some_and(|progress| progress.stage == feathrise_scene_core::state::LoadingStage::Failed)
    }

    pub(super) fn preset_failures(
        &self,
    ) -> [Option<(
        crate::types::EditorStatusOperation,
        &crate::types::EditorStatusTarget,
        &str,
    )>; 3] {
        use crate::types::EditorStatusOperation;
        [
            self.surface_presets
                .as_ref()
                .and_then(|state| state.save_feedback_failure())
                .map(|(target, detail)| (EditorStatusOperation::SurfacePresetSave, target, detail)),
            self.water_presets
                .as_ref()
                .and_then(|state| state.save_feedback_failure())
                .map(|(target, detail)| (EditorStatusOperation::WaterPresetSave, target, detail)),
            self.vegetation_presets
                .as_ref()
                .and_then(|state| state.save_feedback_failure())
                .map(|(target, detail)| {
                    (EditorStatusOperation::VegetationPresetSave, target, detail)
                }),
        ]
    }

    pub(super) fn suppressed(&self) -> bool {
        !self.editor.open
            || self.editor.close_requested
            || self.editor.new_map_dialog_open
            // 加载期间底栏只读诊断仍可打开、滚动和复制；作者输入继续由透明加载门隔离。
            || self.editor.map_switch_confirmation.is_some()
            || self
                .settings
                .as_ref()
                .is_some_and(|state| state.open || state.owns_mouse_this_frame)
            || self
                .folders
                .as_ref()
                .is_some_and(|state| state.dialog.is_some())
            || self
                .deletion
                .as_ref()
                .is_some_and(|state| state.confirmation.is_some())
            || self
                .conflict
                .as_ref()
                .is_some_and(|state| state.modal_open())
            || self
                .asset_menu
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .tab_menu
                .as_ref()
                .is_some_and(|state| state.owns_mouse())
            || self
                .map_drag
                .as_ref()
                .is_some_and(|state| state.blocks_other_mouse())
            || self
                .layout
                .as_ref()
                .is_some_and(|state| state.drag.is_some() || state.owns_mouse_this_frame)
            || self
                .numeric
                .as_ref()
                .is_some_and(|state| state.entity.is_some())
    }

    pub(super) fn entries(&self) -> [RuntimeEntry<'_>; 5] {
        let belongs = |id: &Option<feathrise_map_content::MapId>| {
            self.editor.selected_map_id.is_some()
                && *id == self.editor.selected_map_id
                && !self.editor.map_transition_in_progress
        };
        let map = self
            .dynamic_map
            .as_ref()
            .filter(|status| belongs(&status.map_id));
        let vegetation = self
            .vegetation
            .as_ref()
            .filter(|status| belongs(&status.map_id));
        let water = self.water.as_ref().filter(|status| belongs(&status.map_id));
        let placements = self
            .placements
            .as_ref()
            .filter(|status| belongs(&status.map_id));
        let portals = self
            .portals
            .as_ref()
            .filter(|status| belongs(&status.map_id));
        [
            RuntimeEntry {
                label: "地图与碰撞",
                phase: map.map(|status| match status.phase {
                    DynamicMapRuntimePhase::Ready => RuntimePhase::Ready,
                    DynamicMapRuntimePhase::Failed => RuntimePhase::Failed,
                    _ => RuntimePhase::Working,
                }),
                detail: map.map_or("", |status| status.detail.as_str()),
            },
            RuntimeEntry {
                label: "植被",
                phase: vegetation.map(|status| match status.phase {
                    DynamicVegetationPhase::Ready => RuntimePhase::Ready,
                    DynamicVegetationPhase::Failed => RuntimePhase::Failed,
                    _ => RuntimePhase::Working,
                }),
                detail: vegetation.map_or("", |status| status.detail.as_str()),
            },
            RuntimeEntry {
                label: "水域",
                phase: water.map(|status| match status.phase {
                    DynamicWaterPhase::Ready => RuntimePhase::Ready,
                    DynamicWaterPhase::Disabled => RuntimePhase::Disabled,
                    DynamicWaterPhase::Failed => RuntimePhase::Failed,
                    _ => RuntimePhase::Working,
                }),
                detail: water.map_or("", |status| status.detail.as_str()),
            },
            RuntimeEntry {
                label: "摆放实例",
                phase: placements.map(|status| match status.phase {
                    MapPlacementRuntimePhase::Ready => RuntimePhase::Ready,
                    MapPlacementRuntimePhase::Failed => RuntimePhase::Failed,
                    _ => RuntimePhase::Working,
                }),
                detail: placements.map_or("", |status| status.detail.as_str()),
            },
            RuntimeEntry {
                label: "传送点",
                phase: portals.map(|status| match status.phase {
                    MapPortalRuntimePhase::Ready => RuntimePhase::Ready,
                    MapPortalRuntimePhase::Failed => RuntimePhase::Failed,
                    _ => RuntimePhase::Working,
                }),
                detail: portals.map_or("", |status| status.detail.as_str()),
            },
        ]
    }

    pub(super) fn task(&self, entries: &[RuntimeEntry<'_>; 5]) -> String {
        let mut task = if self.session.as_ref().is_some_and(|session| session.saving) {
            "保存中".into()
        } else if self.editor.map_transition_in_progress
            || self.editor.pending_map_transition.is_some()
        {
            "准备地图".into()
        } else if self.editor.close_requested {
            "确认关闭".into() // 原地形/碰撞完成门仍在等待，不能因旧Ready或已落盘回执冒充当前就绪。
        } else if self
            .creation
            .as_ref()
            .is_some_and(|state| state.task.is_some())
        {
            self.creation
                .as_ref()
                .and_then(|state| state.feedback_target.as_ref())
                .map_or_else(
                    || "新建地图".into(),
                    |target| format!("新建地图“{}” · 后台处理中", target.label),
                )
        } else if self
            .deletion
            .as_ref()
            .is_some_and(|state| state.task.is_some() || state.pending_start.is_some())
        {
            self.deletion
                .as_ref()
                .and_then(|state| state.feedback_target.as_ref())
                .map_or_else(
                    || "回收地图".into(),
                    |target| format!("删除地图“{}” [{}] · 后台处理中", target.label, target.id),
                )
        } else if self.editor.save_requested {
            "等待保存".into() // 请求不是成功；预检仍在等待时使用明确的处理中阶段。
        } else if let Some(entry) = entries
            .iter()
            .find(|entry| entry.phase == Some(RuntimePhase::Working))
        {
            format!("{}准备中", entry.label)
        } else {
            String::new()
        };
        if let Some(request) = self
            .creation
            .as_ref()
            .and_then(|state| state.pending_after_save.as_ref())
        {
            append_task(
                &mut task,
                &format!("新建地图“{}” · 等待当前地图保存", request.display_name),
            );
        }
        if let Some(plan) = self
            .deletion
            .as_ref()
            .and_then(|state| state.pending_start.as_ref())
        {
            append_task(
                &mut task,
                &format!(
                    "删除地图“{}” [{}] · 等待安全回收校验",
                    plan.request.display_name, plan.request.map_id
                ),
            );
        }
        // 至多三项独立保存同时进行；全部阶段进入详情，不扫描预设标签或目录内容。
        for preset in [
            self.surface_presets
                .as_ref()
                .and_then(|state| state.save_feedback_task()),
            self.water_presets
                .as_ref()
                .and_then(|state| state.save_feedback_task()),
            self.vegetation_presets
                .as_ref()
                .and_then(|state| state.save_feedback_task()),
        ]
        .into_iter()
        .flatten()
        {
            append_task(&mut task, &preset);
        }
        task
    }
}

fn append_task(task: &mut String, next: &str) {
    if !task.is_empty() {
        task.push_str("；");
    }
    task.push_str(next);
}
