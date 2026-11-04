//! 真实取景分派与完整范围回归：四方向/两投影/窄视口、资产/传送点及迟到请求隔离。

use super::*;
use crate::types::EditorView;
use crate::viewport::camera_math::{
    PERSPECTIVE_FOV_RADIANS, orbit_angles_from_view, orthographic_projection,
    orthographic_transform, perspective_transform,
};
use crate::viewport::placement_selection::{
    PlacementMeshResolution, SelectedPlacementMeshIdentity,
};
use bevy::camera::primitives::Aabb;
use bevy::camera::{CameraProjection, PerspectiveProjection};
use feathrise_map_content::{MapId, MapManifestAsset, MapPortalRecord, MapPortalTarget};
use feathrise_scene::MapPlacementRuntimeStatus;
use std::sync::Arc;

fn app() -> App {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.world_min = [-300.0, -100.0];
    draft.manifest.world_max = [300.0, 100.0];
    draft.manifest.terrain.base_height = -20.0;
    draft.manifest.terrain.height_scale = 40.0;
    let map = draft.map_id.clone();
    let mut app = App::new();
    app.insert_resource(MapEditorState {
        open: true,
        selected_map_id: Some(map.clone()),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        revision: 7,
        ..default()
    })
    .insert_resource(MapEditorViewportState {
        _image: Handle::default(),
        camera: Entity::PLACEHOLDER,
        center: Vec3::new(5_000.0, -800.0, 4_000.0),
        scene_center: Vec3::ONE,
        scene_half_extents: Vec3::ONE,
        extent: 50.0,
        yaw: 0.8,
        pitch: 0.4,
        framed_map_id: Some(map),
        render_size: UVec2::new(800, 600),
    })
    .init_resource::<MapEditorNumericScrubMouseGuard>()
    .init_resource::<WaterNetworkEditState>()
    .init_resource::<VegetationPresetEditorState>()
    .init_resource::<EditorGltfBrowserState>()
    .init_resource::<LayerCandidateState>()
    .init_resource::<SelectedPlacementViewportState>()
    .init_resource::<SelectedWaterViewportState>()
    .init_resource::<SelectedPortalViewportState>()
    .add_systems(Update, (crate::viewport::content_framing::frame_content, apply_viewport_framing).chain());
    app
}

fn map(app: &App) -> MapId {
    app.world()
        .resource::<MapEditorState>()
        .selected_map_id
        .clone()
        .unwrap()
}

fn request(app: &mut App, request: EditorViewportFraming) {
    app.world_mut()
        .resource_mut::<MapEditorState>()
        .viewport_framing_requested = Some(request);
    app.update();
}

#[test]
fn whole_map_fits_real_projection_from_all_directions_and_aspect_ratios() {
    for half_extents in [
        Vec3::new(300.0, 20.0, 100.0),
        Vec3::new(4096.0, 256.0, 4096.0),
    ] {
        for view in [
            EditorView::Perspective,
            EditorView::Top,
            EditorView::Front,
            EditorView::Side,
        ] {
            for orthographic in [false, true] {
                for size in [
                    UVec2::new(1920, 1080),
                    UVec2::new(320, 800),
                    UVec2::new(1200, 280),
                ] {
                    let mut app = app();
                    let entity = app.world_mut().spawn((Mesh3d::default(), GlobalTransform::IDENTITY,
                        Aabb::from_min_max(-half_extents, half_extents))).id();
                    let token = feathrise_scene_core::residency::ResidentMapToken(1);
                    let mut index = feathrise_scene::ResidentEntityIndex::default();
                    index.insert(entity, token);
                    app.insert_resource(index).insert_resource(feathrise_scene_core::residency::SceneResidencyActivity { active: Some(token), ..default() });
                    {
                        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                        let manifest = &mut session.draft.as_mut().unwrap().manifest;
                        manifest.world_min = [-half_extents.x, -half_extents.z];
                        manifest.world_max = [half_extents.x, half_extents.z];
                        manifest.terrain.base_height = -half_extents.y;
                        manifest.terrain.height_scale = half_extents.y * 2.0;
                    }
                    let (yaw, pitch) = orbit_angles_from_view(view);
                    {
                        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
                        editor.view = view;
                        editor.orthographic = orthographic;
                    }
                    {
                        let mut viewport = app.world_mut().resource_mut::<MapEditorViewportState>();
                        viewport.yaw = yaw;
                        viewport.pitch = pitch;
                        viewport.render_size = size;
                    }
                    request(&mut app, EditorViewportFraming::Map);
                    let viewport = app.world().resource::<MapEditorViewportState>();
                    assert_eq!(viewport.center, Vec3::ZERO);
                    assert_eq!(viewport.scene_half_extents, half_extents);
                    assert_eq!((viewport.yaw, viewport.pitch), (yaw, pitch));
                    let camera = if orthographic {
                        orthographic_transform(
                            viewport.center,
                            viewport.scene_center,
                            viewport.scene_half_extents,
                            yaw,
                            pitch,
                        )
                    } else {
                        perspective_transform(viewport.center, viewport.extent, yaw, pitch)
                    };
                    let clip_from_view = if orthographic {
                        let mut projection = orthographic_projection(
                            crate::viewport::camera_math::focus_plane_height(viewport.extent),
                            yaw,
                            pitch,
                            viewport.scene_half_extents,
                        );
                        projection.update(size.x as f32, size.y as f32);
                        projection.get_clip_from_view()
                    } else {
                        PerspectiveProjection {
                            fov: PERSPECTIVE_FOV_RADIANS,
                            aspect_ratio: size.x as f32 / size.y as f32,
                            far: (viewport.extent * 12.0).max(10_000.0),
                            ..default()
                        }
                        .get_clip_from_view()
                    };
                    let view_from_world = camera.compute_affine().inverse();
                    for x in [-half_extents.x, half_extents.x] {
                        for y in [-half_extents.y, half_extents.y] {
                            for z in [-half_extents.z, half_extents.z] {
                                let view_point =
                                    view_from_world.transform_point3(Vec3::new(x, y, z));
                                let clip = clip_from_view * view_point.extend(1.0);
                                let ndc = clip.truncate() / clip.w;
                                assert!(
                                    clip.w > 0.0
                                        && ndc.x.abs() < 1.0
                                        && ndc.y.abs() < 1.0
                                        && (0.0..=1.0).contains(&ndc.z),
                                    "{view:?} 正交={orthographic} 尺寸={size:?} {ndc:?}"
                                );
                            }
                        }
                    }
                    let editor = app.world().resource::<MapEditorState>();
                    assert_eq!((editor.view, editor.orthographic), (view, orthographic));
                    assert!(!editor.dirty);
                    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 7);
                    assert!(
                        app.world()
                            .resource::<EditorAuthoringSession>()
                            .undo
                            .is_empty()
                    );
                }
            }
        }
    }
}

#[test]
fn whole_map_cancels_a_waiting_old_object_focus() {
    let mut app = app();
    let map_id = map(&app);
    app.world_mut()
        .resource_mut::<SelectedPlacementViewportState>()
        .focus_request = Some(SelectedPlacementMeshIdentity {
        map_id,
        placement_id: "旧对象".into(),
        whole_instance: true,
        node_path: vec![],
        mesh_name: String::new(),
    });
    request(&mut app, EditorViewportFraming::Map);
    assert!(
        app.world()
            .resource::<SelectedPlacementViewportState>()
            .focus_request
            .is_none()
    );
    assert_eq!(
        app.world().resource::<MapEditorViewportState>().center,
        Vec3::ZERO
    );
}

#[test]
fn asset_focus_uses_complete_cached_bounds_and_formal_selection_during_hover() {
    for orthographic in [false, true] {
        let mut app = app();
        let map_id = map(&app);
        app.init_resource::<Assets<StandardMaterial>>()
            .init_resource::<MapPlacementRuntimeStatus>()
            .add_systems(
                Update,
                crate::viewport::sync_selected_placement_viewport.after(apply_viewport_framing),
            );
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .orthographic = orthographic;
        app.world_mut()
            .resource_mut::<EditorGltfBrowserState>()
            .selected_placement_id = Some("树".into());
        app.world_mut().resource_mut::<LayerCandidateState>().hover =
            Some(LayerTarget::Surface("地表".into()));
        let root = app.world_mut().spawn_empty().id();
        let source = app
            .world_mut()
            .spawn((
                Mesh3d::default(),
                Transform::default(),
                GlobalTransform::from_translation(Vec3::new(100.0, 15.0, -20.0)),
                InheritedVisibility::VISIBLE,
                ChildOf(root),
                Aabb::from_min_max(Vec3::new(-2.0, -1.0, -3.0), Vec3::new(2.0, 1.0, 3.0)),
            ))
            .id();
        {
            let mut placement = app
                .world_mut()
                .resource_mut::<SelectedPlacementViewportState>();
            placement.resolved_identity = Some(SelectedPlacementMeshIdentity {
                map_id,
                placement_id: "树".into(),
                whole_instance: true,
                node_path: vec![],
                mesh_name: String::new(),
            });
            placement.resolution = PlacementMeshResolution::Ready;
            placement.source_entities.push(source);
        }
        request(&mut app, EditorViewportFraming::Selected);
        let viewport = app.world().resource::<MapEditorViewportState>();
        assert_eq!(viewport.center, Vec3::new(100.0, 15.0, -20.0));
        assert_eq!((viewport.yaw, viewport.pitch), (0.8, 0.4));
        assert!(viewport.extent < 50.0);
        assert_eq!(
            app.world().resource::<MapEditorState>().orthographic,
            orthographic
        );
        assert!(!app.world().resource::<MapEditorState>().dirty);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 7);
        assert!(
            app.world()
                .resource::<SelectedPlacementViewportState>()
                .focus_request
                .is_none()
        );
    }
}

#[test]
fn portal_focus_reuses_authoritative_position_without_changing_view_or_draft() {
    let mut app = app();
    let map_id = map(&app);
    app.init_resource::<Assets<MapManifestAsset>>().add_systems(
        Update,
        crate::viewport::sync_selected_portal_viewport.after(apply_viewport_framing),
    );
    {
        let mut editor = app.world_mut().resource_mut::<MapEditorState>();
        editor.tool = EditorTool::Portal;
        editor.selected_current_portal_id = Some("传送点".into());
        editor.view = EditorView::Front;
        editor.orthographic = true;
    }
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .portals
        .portals
        .push(MapPortalRecord {
            terrain_binding: None,
            arrival_terrain_binding: None,
            id: "传送点".into(),
            label: "测试传送点".into(),
            label_customized: true,
            position: [10.0, 4.0, -15.0],
            arrival: [20.0, 4.0, -15.0],
            target: MapPortalTarget::default_spawn(map_id),
            color_srgba: [1.0; 4],
            arrow_degrees: 0.0,
        });
    request(&mut app, EditorViewportFraming::Selected);
    assert_eq!(
        app.world().resource::<MapEditorViewportState>().center,
        Vec3::new(10.0, 4.0 + feathrise_portal::PORTAL_EDGE_BEAM_HEIGHT * 0.5, -15.0)
    );
    assert_eq!(
        app.world().resource::<MapEditorState>().view,
        EditorView::Front
    );
    assert!(app.world().resource::<MapEditorState>().orthographic);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 7);
    assert!(!app.world().resource::<MapEditorState>().dirty);
}

#[test]
fn blocked_cross_map_and_other_workspace_requests_are_discarded_without_replay() {
    for case in 0..6 {
        let mut app = app();
        match case {
            0 => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .map_transition_in_progress = true
            }
            1 => {
                app.world_mut()
                    .resource_mut::<MapEditorState>()
                    .new_map_dialog_open = true
            }
            2 => app
                .world_mut()
                .resource_mut::<MapEditorNumericScrubMouseGuard>()
                .advance(true, false, false),
            3 => {
                app.world_mut()
                    .resource_mut::<VegetationPresetEditorState>()
                    .active_tab = PresetWorkspaceTab::Document
            }
            4 => {
                app.world_mut()
                    .resource_mut::<MapEditorViewportState>()
                    .framed_map_id = Some(MapId("另一张图".into()))
            }
            _ => app.world_mut().resource_mut::<MapEditorState>().open = false,
        }
        let before = app.world().resource::<MapEditorViewportState>().center;
        request(&mut app, EditorViewportFraming::Map);
        assert_eq!(
            app.world().resource::<MapEditorViewportState>().center,
            before
        );
        assert!(
            app.world()
                .resource::<MapEditorState>()
                .viewport_framing_requested
                .is_none()
        );
        app.world_mut().resource_mut::<MapEditorState>().open = true;
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .map_transition_in_progress = false;
        app.world_mut()
            .resource_mut::<MapEditorState>()
            .new_map_dialog_open = false;
        app.world_mut()
            .resource_mut::<VegetationPresetEditorState>()
            .active_tab = PresetWorkspaceTab::Map;
        app.update();
        assert_eq!(
            app.world().resource::<MapEditorViewportState>().center,
            before
        );
    }
}
