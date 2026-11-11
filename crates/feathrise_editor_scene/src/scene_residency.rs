//! 作者派生场景的驻留边界；草稿/历史仍属于文档，预览实体和GPU资格跟随完整场景。

use crate::{authoring::*, resources::*, viewport::*};
use bevy::prelude::*;
use feathrise_scene_core::residency::SceneResidencyAppExt;

/// 玩法传送期间作者文档仍可能是上一张图，禁止其清理或覆盖已经暂停的派生场景。
pub(crate) fn document_active(
    session: Option<Res<EditorAuthoringSession>>,
    scenes: Option<Res<feathrise_scene::ResidentScenes>>,
    activity: Option<Res<feathrise_scene_core::residency::SceneResidencyActivity>>,
) -> bool {
    if activity.is_some_and(|state| state.switching) {
        return false;
    }
    let Some(map) = session
        .as_ref()
        .and_then(|session| session.draft.as_ref().map(|draft| &draft.scene_id))
    else {
        return true;
    };
    scenes.is_none_or(|scenes| !scenes.contains(map) || scenes.active_is(map))
}

pub(crate) fn install(app: &mut App) {
    app.register_optional_resident_resource::<SceneEditorViewportState>()
        .init_resident_resource::<feathrise_scene_content::SceneAuthorPreview>()
        .init_resident_resource::<feathrise_atmosphere::WeatherAuthoringPreview>()
        .init_resident_resource::<feathrise_scene_content::DynamicCelestialLightingPreview>()
        .init_resident_resource::<PlacementTerrainFollowState>()
        .init_resident_resource::<TerrainPreviewState>()
        .init_resident_resource::<TerrainLiveApplyState>()
        .init_resident_resource::<TerrainExternalChangeState>()
        .init_resident_resource::<SurfacePreviewState>()
        .init_resident_resource::<VegetationPreviewState>()
        .init_resident_resource::<WaterPreviewState>()
        .init_resident_resource::<VegetationDensityOverlayState>()
        .init_resident_resource::<LiveBladeDensityState>()
        .init_resident_resource::<LiveClipmapWaterState>()
        .init_resident_resource::<BoundaryLiveOverrideState>()
        .init_resident_resource::<PortalPreviewHandoff>()
        .init_resident_resource::<WaterFileHandoff>()
        .init_resident_resource::<PlacementWaterEvidenceRegistry>()
        .init_resident_resource::<SurfaceSelectionOverlay>()
        .init_resident_resource::<SurfaceStrokeTrail>()
        .init_resident_resource::<SelectedWaterViewportState>()
        .init_resident_resource::<SelectedPortalViewportState>()
        .init_resident_resource::<SelectedPlacementViewportState>()
        .init_resident_resource::<VegetationGroundCursor>()
        .init_resident_resource::<EditorBladeViewHandoff>()
        .init_resident_resource::<crate::ui::gltf_browser::EditorGltfBrowserState>()
        .init_resident_resource::<crate::ui::placement_water::PlacementInstanceIndex>()
        .init_resident_resource::<crate::ui::placement_water::PlacementWaterRuntimeSelectionCache>()
        .init_resident_resource::<crate::ui::placement_water::PlacementWaterSelectionCache>();
    // 只登记真正属于场景的根；公共UI、资产缩略图及独立预设预览保持原生命周期。
    feathrise_scene::register_resident_scene_root::<crate::components::SceneEditorCamera>(app);
    feathrise_scene::register_resident_scene_root::<crate::components::EditorPlacedAsset>(app);
    feathrise_scene::register_resident_scene_root::<crate::components::EditorPortalPreview>(app);
    feathrise_scene::register_resident_scene_root::<crate::components::EditorTerrainPreview>(app);
    feathrise_scene::register_resident_scene_root::<crate::components::EditorVegetationPreview>(app);
    feathrise_scene::register_resident_scene_root::<crate::components::EditorVegetationDensityOverlay>(
        app,
    );
    feathrise_scene::register_resident_scene_root::<crate::components::EditorWaterPreview>(app);
    feathrise_scene::register_resident_scene_root::<crate::components::EditorBuildingGhost>(app);
}
