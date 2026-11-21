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
use feathrise_scene_content::{SceneId, SceneManifestAsset, ScenePortalRecord, ScenePortalTarget};
use feathrise_scene::ScenePlacementRuntimeStatus;
use std::sync::Arc;

fn app() -> App {
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    draft.manifest.world_min = [-300.0, -100.0];
    draft.manifest.world_max = [300.0, 100.0];
    draft.manifest.terrain.base_height = -20.0;
    draft.manifest.terrain.height_scale = 40.0;
    let map = draft.scene_id.clone();
    let mut app = App::new();
    app.insert_resource(SceneEditorState {
        open: true,
        selected_scene_id: Some(map.clone()),
        ..default()
    })
    .insert_resource(EditorAuthoringSession {
        draft: Some(draft),
        revision: 7,
        ..default()
    })
    .insert_resource(SceneEditorViewportState {
        _image: Handle::default(),
        camera: Entity::PLACEHOLDER,
        center: Vec3::new(5_000.0, -800.0, 4_000.0),
        scene_center: Vec3::ONE,
        scene_half_extents: Vec3::ONE,
        extent: 50.0,
        yaw: 0.8,
        pitch: 0.4,
        framed_scene_id: Some(map),
        render_size: UVec2::new(800, 600),
    })
    .init_resource::<SceneEditorNumericScrubMouseGuard>()
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

fn map(app: &App) -> SceneId {
    app.world()
        .resource::<SceneEditorState>()
        .selected_scene_id
        .clone()
        .unwrap()
}

fn request(app: &mut App, request: EditorViewportFraming) {
    app.world_mut()
        .resource_mut::<SceneEditorState>()
        .viewport_framing_requested = Some(request);
    app.update();
}

#[test]
fn whole_scene_fits_real_projection_from_all_directions_and_aspect_ratios() {
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
                    let token = feathrise_scene_core::residency::ResidentSceneToken(1);
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
                        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
                        editor.view = view;
                        editor.orthographic = orthographic;
                    }
                    {
                        let mut viewport = app.world_mut().resource_mut::<SceneEditorViewportState>();
                        viewport.yaw = yaw;
                        viewport.pitch = pitch;
                        viewport.render_size = size;
                    }
                    request(&mut app, EditorViewportFraming::Scene);
                    let viewport = app.world().resource::<SceneEditorViewportState>();
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
                    let editor = app.world().resource::<SceneEditorState>();
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
fn whole_scene_cancels_a_waiting_old_object_focus() {
    let mut app = app();
    let scene_id = map(&app);
    app.world_mut()
        .resource_mut::<SelectedPlacementViewportState>()
        .focus_request = Some(SelectedPlacementMeshIdentity {
        scene_id,
        placement_id: "旧对象".into(),
        whole_instance: true,
        node_path: vec![],
        mesh_name: String::new(),
    });
    request(&mut app, EditorViewportFraming::Scene);
    assert!(
        app.world()
            .resource::<SelectedPlacementViewportState>()
            .focus_request
            .is_none()
    );
    assert_eq!(
        app.world().resource::<SceneEditorViewportState>().center,
        Vec3::ZERO
    );
}

#[test]
fn asset_focus_uses_complete_cached_bounds_and_formal_selection_during_hover() {
    for (orthographic, tool, command) in [
        (false, EditorTool::Inspect, EditorViewportFraming::Selected),
        (true, EditorTool::Inspect, EditorViewportFraming::Selected),
        (false, EditorTool::Water, EditorViewportFraming::TreePlacement),
        (false, EditorTool::Vegetation, EditorViewportFraming::TreePlacement),
    ] {
        let mut app = app();
        let scene_id = map(&app);
        app.init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<ScenePlacementRuntimeStatus>()
            .add_systems(
                Update,
                crate::viewport::sync_selected_placement_viewport.after(apply_viewport_framing),
            );
        {
            let mut state = app.world_mut().resource_mut::<SceneEditorState>();
            state.orthographic = orthographic;
            state.tool = tool;
        }
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
                scene_id: scene_id.clone(),
                placement_id: "树".into(),
                whole_instance: true,
                node_path: vec![],
                mesh_name: String::new(),
            });
            placement.resolution = PlacementMeshResolution::Ready;
            placement.source_entities.push(source);
        }
        if command == EditorViewportFraming::TreePlacement {
            app.world_mut().resource_mut::<LayerCandidateState>().selected =
                Some(LayerTarget::Placement("树".into()));
        }
        if tool == EditorTool::Water {
            let mut water = crate::authoring::default_water_asset(&scene_id);
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            let draft = session.draft.as_mut().unwrap();
            water.layers.push(crate::authoring::default_water_layer(&draft.folder));
            draft.water = Some(water);
            let before = app.world().resource::<SceneEditorViewportState>().center;
            request(&mut app, EditorViewportFraming::Selected);
            assert_eq!(app.world().resource::<SceneEditorViewportState>().center, before);
            assert_eq!(app.world().resource::<SceneEditorState>().status, "正在聚焦所选，等待当前完整范围",
                "普通 F 仍按水域工具提交其自身的范围请求");
        }
        request(&mut app, command);
        let viewport = app.world().resource::<SceneEditorViewportState>();
        assert_eq!(viewport.center, Vec3::new(100.0, 15.0, -20.0));
        assert_eq!((viewport.yaw, viewport.pitch), (0.8, 0.4));
        assert!(viewport.extent < 50.0);
        assert_eq!(
            app.world().resource::<SceneEditorState>().orthographic,
            orthographic
        );
        assert_eq!(app.world().resource::<SceneEditorState>().tool, tool);
        assert!(!app.world().resource::<SceneEditorState>().dirty);
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
fn group_focus_waits_for_complete_visible_bounds_and_cancels_after_camera_navigation() {
    use crate::ui::placement_water::{PlacementInstanceIndex, register_placement_instance_root};
    use crate::viewport::group_focus::{GroupFocusState, frame_group_selection};
    use feathrise_scene::ScenePlacementInstance;
    use feathrise_scene_content::ScenePlacementAssetTarget;

    let mut app = app();
    let scene_id = map(&app);
    app.init_resource::<GroupFocusState>()
        .init_resource::<PlacementInstanceIndex>()
        .add_observer(register_placement_instance_root)
        .add_systems(Update, frame_group_selection.after(apply_viewport_framing));
    let mut add_root = |id: &str, group: &str, x, hidden, locked, ready| {
        let mut record = crate::viewport::placement_move::interaction_tests::record();
        record.id = id.into();
        record.asset_target = ScenePlacementAssetTarget::Primitive {
            mesh: 0, primitive: 0, material: None, collision: false,
        };
        let metadata = Arc::make_mut(&mut record.editor);
        metadata.group = group.into();
        metadata.hidden = hidden;
        metadata.locked = locked;
        let world = app.world_mut();
        world.resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().placements.placements.push(record.clone());
        let mut root = world.spawn((ScenePlacementInstance::new(&scene_id, &record),
            Mesh3d::default(), GlobalTransform::from_translation(Vec3::new(x, 0.0, 0.0)), InheritedVisibility::VISIBLE));
        if ready { root.insert(Aabb::from_min_max(-Vec3::ONE, Vec3::ONE)); }
        root.id()
    };
    let first = add_root("left", "村口", -12.0, false, false, true);
    let second = add_root("right", "村口", 20.0, false, true, false);
    add_root("hidden", "村口", 500.0, true, false, true);
    add_root("other", "城里", 600.0, false, false, true);
    for _ in 0..300 { app.world_mut().spawn(ChildOf(first)); }
    app.world_mut().resource_mut::<EditorGltfBrowserState>().selected_group = Some("村口".into());
    let before = app.world().resource::<SceneEditorViewportState>().center;
    request(&mut app, EditorViewportFraming::Selected);
    for _ in 0..5 { app.update(); }
    assert_eq!(app.world().resource::<SceneEditorViewportState>().center, before, "缺右侧模型范围时不领用部分结果");

    let bounds = Aabb::from_min_max(-Vec3::ONE, Vec3::ONE);
    app.world_mut().entity_mut(second).insert(bounds);
    app.update();
    let view = app.world().resource::<SceneEditorViewportState>();
    assert_eq!(view.center, Vec3::new(4.0, 0.0, 0.0)); // 两个可见同组成员都贡献，锁定不限制取景。
    assert_eq!(view.extent, framing_extent(Vec3::new(17.0, 1.0, 1.0), view.yaw, view.pitch, view.render_size, false));
    assert!(!app.world().resource::<SceneEditorState>().dirty);

    app.world_mut().entity_mut(second).remove::<Aabb>();
    request(&mut app, EditorViewportFraming::Selected);
    let manual = Vec3::new(100.0, 5.0, 200.0);
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = manual;
    app.world_mut().entity_mut(second).insert(bounds);
    for _ in 0..5 { app.update(); }
    assert_eq!(app.world().resource::<SceneEditorViewportState>().center, manual, "手动导航覆盖等待中的旧请求");
}

#[test]
fn portal_focus_reuses_authoritative_position_without_changing_view_or_draft() {
    let mut app = app();
    let scene_id = map(&app);
    app.init_resource::<Assets<SceneManifestAsset>>().add_systems(
        Update,
        crate::viewport::sync_selected_portal_viewport.after(apply_viewport_framing),
    );
    {
        let mut editor = app.world_mut().resource_mut::<SceneEditorState>();
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
        .push(ScenePortalRecord {
            terrain_binding: None,
            arrival_terrain_binding: None,
            id: "传送点".into(),
            label: "测试传送点".into(),
            label_customized: true,
            position: [10.0, 4.0, -15.0],
            arrival: [20.0, 4.0, -15.0],
            target: ScenePortalTarget::default_spawn(scene_id),
            color_srgba: [1.0; 4],
            arrow_degrees: 0.0,
        });
    request(&mut app, EditorViewportFraming::Selected);
    assert_eq!(
        app.world().resource::<SceneEditorViewportState>().center,
        Vec3::new(10.0, 4.0 + feathrise_portal::PORTAL_EDGE_BEAM_HEIGHT * 0.5, -15.0)
    );
    assert_eq!(
        app.world().resource::<SceneEditorState>().view,
        EditorView::Front
    );
    assert!(app.world().resource::<SceneEditorState>().orthographic);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 7);
    assert!(!app.world().resource::<SceneEditorState>().dirty);
}

#[test]
fn blocked_cross_scene_and_other_workspace_requests_are_discarded_without_replay() {
    for case in 0..6 {
        let mut app = app();
        match case {
            0 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .scene_transition_in_progress = true
            }
            1 => {
                app.world_mut()
                    .resource_mut::<SceneEditorState>()
                    .new_scene_dialog_open = true
            }
            2 => app
                .world_mut()
                .resource_mut::<SceneEditorNumericScrubMouseGuard>()
                .advance(true, false, false),
            3 => {
                app.world_mut()
                    .resource_mut::<VegetationPresetEditorState>()
                    .active_tab = PresetWorkspaceTab::Document
            }
            4 => {
                app.world_mut()
                    .resource_mut::<SceneEditorViewportState>()
                    .framed_scene_id = Some(SceneId("另一张图".into()))
            }
            _ => app.world_mut().resource_mut::<SceneEditorState>().open = false,
        }
        let before = app.world().resource::<SceneEditorViewportState>().center;
        request(&mut app, EditorViewportFraming::Scene);
        assert_eq!(
            app.world().resource::<SceneEditorViewportState>().center,
            before
        );
        assert!(
            app.world()
                .resource::<SceneEditorState>()
                .viewport_framing_requested
                .is_none()
        );
        app.world_mut().resource_mut::<SceneEditorState>().open = true;
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .scene_transition_in_progress = false;
        app.world_mut()
            .resource_mut::<SceneEditorState>()
            .new_scene_dialog_open = false;
        app.world_mut()
            .resource_mut::<VegetationPresetEditorState>()
            .active_tab = PresetWorkspaceTab::Scene;
        app.update();
        assert_eq!(
            app.world().resource::<SceneEditorViewportState>().center,
            before
        );
    }
}
