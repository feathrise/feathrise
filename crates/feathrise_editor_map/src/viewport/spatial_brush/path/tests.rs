//! 路径节点编辑、冻结请求与换图拒收；复用真实作者会话和输入资源。
use super::{systems, types::*};
use crate::{
    authoring::EditorAuthoringSession,
    resources::MapEditorState,
    viewport::spatial_brush::{resources::SpatialBrushState, types::ApplyMode},
};
use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use feathrise_map_terrain::volume::VolumePathParameters;
use std::sync::Arc;

fn bounds() -> Rect {
    Rect::from_corners(Vec2::splat(-256.), Vec2::splat(256.))
}

#[test]
fn editor_path_add_keeps_the_brush_free_and_editing_a_node_preserves_the_old_draft() {
    let nodes: Arc<[[f32; 3]]> = Arc::from([[2., -1., 2.]]);
    let added = systems::build(
        nodes.clone(),
        None,
        Edit::Add(Vec3::new(8., -1., 5.)),
        VolumePathParameters::default(),
        bounds(),
        Mat4::IDENTITY,
    );
    assert_eq!(nodes.len(), 1);
    assert_eq!(added.nodes.len(), 2);
    assert_eq!(
        added.selected, None,
        "加入后应能移动刷心放置下一点，不改写刚加入的节点"
    );
    assert!(added.plan.as_ref().unwrap().is_some());
    let changed = systems::build(
        added.nodes.clone(),
        Some(0),
        Edit::Replace(0, Vec3::new(3., -2., 4.)),
        VolumePathParameters::default(),
        bounds(),
        Mat4::IDENTITY,
    );
    assert_eq!(added.nodes[0], [2., -1., 2.]);
    assert_eq!(changed.nodes[0], [3., -2., 4.]);
    assert_eq!(changed.selected, Some(0));
    let deleted = systems::build(
        changed.nodes,
        Some(1),
        Edit::Delete(1),
        VolumePathParameters::default(),
        bounds(),
        Mat4::IDENTITY,
    );
    assert_eq!(deleted.nodes.len(), 1);
    assert!(deleted.plan.unwrap().is_none());
}

#[test]
fn editor_path_request_freezes_geometry_and_rejects_unprepared_coordinates_and_source_changes() {
    let (mut app, _) = super::super::tests::app();
    let built = systems::build(
        Arc::from([[2., -1., 2.], [8., -2., 5.]]),
        Some(0),
        Edit::Refresh,
        VolumePathParameters {
            curved: true,
            width: 3.,
            height: 2.,
            ..Default::default()
        },
        bounds(),
        Mat4::IDENTITY,
    );
    app.world_mut()
        .resource_scope(|world, mut state: Mut<SpatialBrushState>| {
            state.sync_scope(
                world.resource::<MapEditorState>(),
                world.resource::<EditorAuthoringSession>(),
            );
            state.enabled = true;
            state.mode = ApplyMode::Path;
            state.path.nodes = built.nodes;
            state.path.plan = built.plan.unwrap();
            state.path.preview = built.preview;
            state.path.parameters = state.path.plan.as_ref().unwrap().parameters();
            state.path.selected = Some(0);
            state.center = Some(Vec3::new(2., -1., 2.));
            state.path.applied_revision = state.path.revision;
        });
    let request = systems::build_request(
        app.world().resource::<SpatialBrushState>(),
        app.world().resource::<MapEditorState>(),
        app.world().resource::<EditorAuthoringSession>(),
    )
    .unwrap();
    assert_eq!(request.path.as_ref().unwrap().parameters().width, 3.);
    app.world_mut().resource_mut::<SpatialBrushState>().center = Some(Vec3::new(4., -1., 2.));
    assert!(
        systems::build_request(
            app.world().resource::<SpatialBrushState>(),
            app.world().resource::<MapEditorState>(),
            app.world().resource::<EditorAuthoringSession>()
        )
        .is_err(),
        "坐标已改但新预览未完成，不能执行旧路径"
    );
    assert_eq!(request.path.as_ref().unwrap().parameters().width, 3.);
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .terrain_revision += 1;
    assert!(!request.matches(
        app.world().resource::<MapEditorState>(),
        app.world().resource::<EditorAuthoringSession>()
    ));
}

#[test]
fn editor_path_scope_change_retains_running_credit_and_discards_the_old_result() {
    let (mut app, _) = super::super::tests::app();
    let original = Arc::from([[2., -1., 2.], [8., -1., 8.]]);
    let built = systems::build(
        original,
        None,
        Edit::Refresh,
        VolumePathParameters::default(),
        bounds(),
        Mat4::IDENTITY,
    );
    app.world_mut()
        .resource_scope(|world, mut state: Mut<SpatialBrushState>| {
            state.sync_scope(
                world.resource::<MapEditorState>(),
                world.resource::<EditorAuthoringSession>(),
            );
            state.path.working_revision = state.path.revision;
            state.path.task =
                Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move { built }));
            state.path.reset();
            assert!(
                state.path.task.is_some(),
                "不能丢弃在途句柄并提前归还工作信用"
            );
        });
    for _ in 0..1000 {
        app.world_mut().run_system_once(systems::drive).unwrap();
        if !app.world().resource::<SpatialBrushState>().path.busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let state = app.world().resource::<SpatialBrushState>();
    assert!(state.path.nodes.is_empty());
    assert!(state.path.plan.is_none());
    assert!(!state.path.busy());
}

#[test]
fn editor_path_late_preview_cannot_overwrite_a_new_coordinate_on_the_same_frame() {
    let (mut app, _) = super::super::tests::app();
    let nodes: Arc<[[f32; 3]]> = Arc::from([[2., -1., 2.], [8., -1., 8.]]);
    let built = systems::build(
        nodes.clone(),
        Some(0),
        Edit::Replace(0, Vec3::new(3., -1., 2.)),
        VolumePathParameters::default(),
        bounds(),
        Mat4::IDENTITY,
    );
    app.world_mut()
        .resource_scope(|world, mut state: Mut<SpatialBrushState>| {
            state.sync_scope(
                world.resource::<MapEditorState>(),
                world.resource::<EditorAuthoringSession>(),
            );
            state.enabled = true;
            state.mode = ApplyMode::Path;
            state.path.nodes = nodes;
            state.path.selected = Some(0);
            state.path.observed_center = Some(Vec3::new(2., -1., 2.));
            state.path.working_revision = state.path.revision;
            state.path.task =
                Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move { built }));
            state.center = Some(Vec3::new(20., -2., 7.));
        });
    for _ in 0..1000 {
        app.world_mut().run_system_once(systems::drive).unwrap();
        if !app.world().resource::<SpatialBrushState>().path.busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let state = app.world().resource::<SpatialBrushState>();
    assert_eq!(state.path.nodes[0], [20., -2., 7.]);
    assert_eq!(state.center, Some(Vec3::new(20., -2., 7.)));
    assert!(state.path.ready(state.center));
}

#[test]
fn editor_path_visible_nodes_are_selectable_without_editing_the_terrain() {
    use crate::resources::MapEditorViewportState;
    use bevy::ui::RelativeCursorPosition;
    let (mut app, viewport) = super::super::tests::app();
    let nodes: Arc<[[f32; 3]]> = Arc::from([[2., -1., 2.], [12., -1., 2.]]);
    let camera = crate::viewport::placement_move::MoveCamera::new(
        app.world().resource::<MapEditorState>(),
        app.world().resource::<MapEditorViewportState>(),
        1.,
    )
    .unwrap();
    let cursor = camera.project(Vec3::from_array(nodes[1])).unwrap() + Vec2::splat(0.5);
    app.world_mut()
        .resource_scope(|world, mut state: Mut<SpatialBrushState>| {
            state.sync_scope(
                world.resource::<MapEditorState>(),
                world.resource::<EditorAuthoringSession>(),
            );
            state.enabled = true;
            state.mode = ApplyMode::Path;
            state.path.nodes = nodes;
            state.path.selected = Some(0);
            state.center = Some(Vec3::new(2., -1., 2.));
            state.path.observed_center = state.center;
        });
    app.world_mut()
        .get_mut::<RelativeCursorPosition>(viewport)
        .unwrap()
        .normalized = Some(cursor);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    let revision = app.world().resource::<EditorAuthoringSession>().revision;
    app.update();
    let state = app.world().resource::<SpatialBrushState>();
    assert_eq!(state.path.selected, Some(1));
    assert_eq!(state.center, Some(Vec3::new(12., -1., 2.)));
    assert!(state.owns_mouse());
    assert_eq!(
        app.world().resource::<EditorAuthoringSession>().revision,
        revision
    );
}


fn object_app() -> (App, Mat4) {
    let (mut app, _) = super::super::tests::app();
    let transform = feathrise_map_content::TerrainObjectTransform {
        translation: [100., 2., -50.], yaw_degrees: 30., scale: [0.5, 2., 1.5],
        ..Default::default()
    };
    let id = feathrise_map_content::TerrainId::parse("path-object").unwrap();
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let object = feathrise_map_content::MapTerrainObject {clipmap_stitches: Default::default(),
            mesh_lod: None, id: id.clone(), display_name: "路径测试网格".into(), enabled: true,
            transform, world_min: [-32.; 2], world_max: [32.; 2],
            terrain: draft.manifest.terrain.clone(), voxel: None, surface_path: None,
            vegetation_path: None, water_path: None, puddles: None,
        };
        draft.manifest.world_min = [-1.; 2];
        draft.manifest.world_max = [1.; 2]; // 容器遗留范围不能裁剪独立网格上的路径。
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        draft.terrain_documents.active = Some(crate::authoring::terrain_documents::TerrainDocumentIdentity {
            manifest: object.source_manifest(&draft.manifest),
            folder: object.private_folder(&draft.map_id), definition: Arc::new(object),
        });
    }
    app.world_mut().resource_mut::<MapEditorState>().terrain_canvas_target = Some(id);
    app.world_mut().resource_scope(|world, mut state: Mut<SpatialBrushState>| {
        state.sync_scope(world.resource::<MapEditorState>(), world.resource::<EditorAuthoringSession>());
        state.enabled = true;
        state.mode = ApplyMode::Path;
        state.path.library_attempted = true; // 本测试不访问磁盘路径库。
    });
    (app, transform.as_transform().to_matrix())
}

#[test]
fn editor_path_uses_selected_object_bounds_and_revalidates_after_movement() {
    let (mut app, transform) = object_app();
    {
        let mut state = app.world_mut().resource_mut::<SpatialBrushState>();
        state.path.nodes = Arc::from([Vec3::ZERO, Vec3::new(4., 0., 0.)]
            .map(|p| transform.transform_point3(p).to_array()));
        state.path.queue(Edit::Refresh);
    }
    for iteration in 0..2 {
        for _ in 0..1000 {
            app.world_mut().run_system_once(systems::drive).unwrap();
            if app.world().resource::<SpatialBrushState>().path.ready(None) { break; }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let request = systems::build_request(
            app.world().resource::<SpatialBrushState>(), app.world().resource::<MapEditorState>(),
            app.world().resource::<EditorAuthoringSession>(),
        ).unwrap();
        assert!(request.brush.from[0] > 90., "预览与冻结请求仍使用世界坐标");
        if iteration == 0 {
            {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                let active = session.draft.as_mut().unwrap().terrain_documents.active.as_mut().unwrap();
                Arc::make_mut(&mut active.definition).transform.translation[0] += 1.;
                session.revision += 1;
            }
            assert!(systems::build_request(
                app.world().resource::<SpatialBrushState>(), app.world().resource::<MapEditorState>(),
                app.world().resource::<EditorAuthoringSession>(),
            ).is_err(), "网格移动后不能执行旧来源下的预览");
        }
    }
}

#[test]
fn editor_path_library_completion_uses_map_owner_not_terrain_private_folder() {
    let (mut app, _) = object_app();
    let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
    let map = draft.map_id.0.clone();
    let folder = draft.folder.clone();
    assert_ne!(folder, draft.terrain_folder());
    {
        let mut state = app.world_mut().resource_mut::<SpatialBrushState>();
        state.path.io_scope = Some((map.clone(), folder));
        state.path.io = Some(bevy::tasks::IoTaskPool::get().spawn(async move {
            IoResult { library: Ok(Arc::new(crate::authoring::SculptPathLibrary::empty(map))), saved: false }
        }));
    }
    for _ in 0..1000 {
        app.world_mut().run_system_once(systems::drive).unwrap();
        if app.world().resource::<SpatialBrushState>().path.io.is_none() { break; }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(app.world().resource::<SpatialBrushState>().path.library.is_some(),
        "同地图独立网格的路径库结果必须正常采用");
}
