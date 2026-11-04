//! 基础形状工作平面的隔离与几何边界，不把普通资产未命中解释为默认地面。

use super::{placement::plane_hit, *};
use bevy::prelude::*;

#[test]
fn explicit_plane_uses_forward_finite_intersection_and_rejects_parallel_or_backward_rays() {
    let point = plane_hit(Ray3d::new(Vec3::new(3.0, 5.0, 7.0), Dir3::NEG_Y)).unwrap();
    assert_eq!(point, Vec3::new(3.0, 0.0, 7.0));
    assert!(plane_hit(Ray3d::new(Vec3::Y, Dir3::Y)).is_none());
    assert!(plane_hit(Ray3d::new(Vec3::Y, Dir3::X)).is_none());
}

#[test]
fn builtin_selection_is_a_scoped_placement_intent() {
    let mut editor = crate::resources::MapEditorState::default();
    editor.selected_asset = Some(CUBOID_SELECTION.into());
    assert!(!is_cuboid_selection(&editor));
    editor.tool = crate::types::EditorTool::PlaceAsset;
    assert!(is_cuboid_selection(&editor));
    editor.selected_asset = Some("gltf/立方体.glb".into());
    assert!(!is_cuboid_selection(&editor));
}

#[test]
fn builtin_target_does_not_inherit_stale_gltf_preview_selection() {
    let preview = crate::ui::asset_preview::AssetPreviewState::default();
    assert_eq!(
        preview.placement_target_for(CUBOID_SELECTION).unwrap(),
        feathrise_map_content::MapPlacementAssetTarget::DEFAULT_CUBOID
    );
    assert_eq!(
        preview.placement_target_for("gltf/cube.glb").unwrap(),
        feathrise_map_content::MapPlacementAssetTarget::Scene0
    );
    assert!(!work_plane_is_available(
        &crate::authoring::EditorAuthoringSession::default()
    ));
}

#[test]
fn empty_map_plane_result_cannot_cross_author_revision_or_legacy_terrain_mode() {
    let mut draft = crate::authoring::test_heightfield_draft(vec![0; 121].into());
    draft.manifest.terrains = Some(Vec::new().into());
    let map = draft.map_id.clone();
    let mut session = crate::authoring::EditorAuthoringSession {
        draft: Some(draft),
        revision: 7,
        ..default()
    };
    let state = BasicShapeState {
        work_plane: Some((map, 7, Vec3::new(3.0, 0.0, 4.0))),
        ..default()
    };
    assert_eq!(
        state.work_plane_point(&session),
        Some(Vec3::new(3.0, 0.0, 4.0))
    );
    session.revision += 1;
    assert!(state.work_plane_point(&session).is_none());
    session.revision = 7;
    session.draft.as_mut().unwrap().manifest.terrains = None;
    assert!(
        state.work_plane_point(&session).is_none(),
        "真实地形和旧图未命中不能回退工作平面"
    );
}
