//! 真实后台拾取、独立画布载入与属性显隐共同回归，防止首次选择被画布修订清掉。
#[path = "layered_canvas_tests.rs"]
mod layered_canvas_tests;
use super::super::layers::*;
use super::super::{apply_layer_candidate, poll_layer_candidates};
use crate::{
    MapEditorSystemSet,
    authoring::{
        BoundaryEditState, EditorAuthoringSession,
        terrain_documents, terrain_ground::TerrainGroundSnapshot, test_heightfield_draft,
    },
    resources::{EditorViewportSelectionState, MapEditorState, TerrainShadingSession},
    ui::{
        gltf_browser::EditorGltfBrowserState,
        placement_transform::PlacementTransformInputState,
        scene_inspector::{InspectorSection, sync_scene_inspector},
        sidebars::MapEditorSidebarState,
        terrain_objects::TerrainObjectsEditorState,
    },
    viewport::{
        SelectedPlacementViewportState, SelectedPortalViewportState, SelectedWaterViewportState,
        modes::ViewportModeState,
    },
};
use crate::game_authoring::map_interaction::MapInteractionState;
use bevy::prelude::*;
use feathrise_map_content::{MapTerrainDescriptor, MapTerrainKind, TerrainId};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn app() -> (App, Entity) {
    let mut draft = test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut first = draft.manifest.terrain_objects_for_edit()[0].clone();
    first.id = TerrainId::parse("terrain-a").unwrap();
    first.terrain.kind = MapTerrainKind::SparseClipmap;
    first.surface_path = None;
    first.vegetation_path = None;
    first.water_path = None;
    let mut second = first.clone();
    second.id = TerrainId::parse("terrain-b").unwrap();
    second.transform.translation[0] = 30.0;
    draft.manifest.terrain = MapTerrainDescriptor::empty();
    draft.manifest.terrains = Some(Arc::from([first, second]));
    let map = draft.map_id.clone();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(MapEditorState {
            open: true,
            selected_map_id: Some(map.clone()),
            ..default()
        })
        .insert_resource(EditorAuthoringSession {
            draft: Some(draft),
            revision: 7,
            ..default()
        })
        .init_resource::<MapInteractionState>()
        .init_resource::<ViewportModeState>()
        .init_resource::<MapEditorSidebarState>()
        .init_resource::<EditorViewportSelectionState>()
        .init_resource::<EditorGltfBrowserState>()
        .init_resource::<SelectedPlacementViewportState>()
        .init_resource::<SelectedWaterViewportState>()
        .init_resource::<SelectedPortalViewportState>()
        .init_resource::<BoundaryEditState>()
        .init_resource::<LayerCandidateState>()
        .init_resource::<TerrainObjectsEditorState>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<TerrainShadingSession>()
        .init_resource::<PlacementTransformInputState>()
        .configure_sets(
            Update,
            (MapEditorSystemSet::Tasks, MapEditorSystemSet::Presentation).chain(),
        )
        .add_systems(
            Update,
            (poll_layer_candidates, apply_layer_candidate)
                .chain()
                .in_set(MapEditorSystemSet::Tasks),
        )
        .add_systems(
            Update,
            sync_scene_inspector.in_set(MapEditorSystemSet::Presentation),
        );
    app.world_mut()
        .resource_mut::<TerrainObjectsEditorState>()
        .map = Some(map);
    terrain_documents::install(&mut app);
    let panel = app
        .world_mut()
        .spawn((
            InspectorSection::Terrain,
            Node {
                display: Display::None,
                ..default()
            },
        ))
        .id();
    app.update();
    (app, panel)
}

fn click(app: &mut App, index: usize) -> LayerTarget {
    let session = app.world().resource::<EditorAuthoringSession>();
    let draft = session.draft.as_ref().unwrap();
    let objects = draft.manifest.terrains.clone().unwrap();
    let object = &objects[index];
    let target = LayerTarget::TerrainObject(object.id.as_str().to_owned());
    let ground = TerrainGroundSnapshot::test_mesh(
        draft.map_id.clone(),
        object.id.clone(),
        object.transform,
        avian3d::prelude::Collider::cuboid(20.0, 2.0, 20.0),
    );
    let key = Identity {
        map: draft.map_id.clone(),
        revision: session.revision,
        placement_derived_pose_revision: session.placement_derived_pose_revision,
        domain_hash: draft.adopted_domain_hash().to_owned(),
        generation: 0,
        ticket: 0,
        filter: super::SelectionFilter::All,
    };
    let ray = Ray3d::new(
        Vec3::new(5.0 + object.transform.translation[0], 20.0, 5.0),
        Dir3::NEG_Y,
    );
    app.world_mut()
        .resource_mut::<EditorAuthoringSession>()
        .draft
        .as_mut()
        .unwrap()
        .terrain_ground = Some(ground.clone());
    let mut candidates = app.world_mut().resource_mut::<LayerCandidateState>();
    candidates.request(
        key,
        ray,
        None,
        Vec2::splat(index as f32 * 100.0),
        Vec2::splat(1000.0),
        1.0,
        None,
    );
    let request = candidates.request.as_mut().unwrap();
    request.terrain_ground = Some(ground);
    request.terrain_objects = objects;
    target
}

fn finish(app: &mut App, target: &LayerTarget, panel: Entity) {
    let started = Instant::now();
    let mut selected = false;
    loop {
        app.update();
        let current = app
            .world()
            .resource::<LayerCandidateState>()
            .selected
            .as_ref();
        selected |= current == Some(target);
        if selected {
            assert_eq!(current, Some(target), "画布载入不应清除已确认网格");
            assert_eq!(
                app.world().get::<Node>(panel).unwrap().display,
                Display::Flex,
                "属性栏不应闪回无选择"
            );
        }
        let session = app.world().resource::<EditorAuthoringSession>();
        if selected
            && !app
                .world()
                .resource::<TerrainObjectsEditorState>()
                .documents_loading
            && matches!(target, LayerTarget::TerrainObject(id) if session.draft.as_ref().unwrap()
                .terrain_documents.active.as_ref().is_some_and(|active| active.definition.id.as_str() == id))
        {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "后台选择与画布载入没有完成"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    // 再经过稳定帧，让轮询实际核对载入后的新修订，不能只验收刚显示属性的一瞬间。
    for _ in 0..6 {
        app.update();
        assert_eq!(
            app.world()
                .resource::<LayerCandidateState>()
                .selected
                .as_ref(),
            Some(target)
        );
        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::Flex
        );
    }
}

#[test]
fn first_terrain_click_keeps_properties_through_fresh_and_cached_canvas_switches() {
    let (mut app, panel) = app();
    for (click_index, index) in [0, 1, 0].into_iter().enumerate() {
        let revision = app.world().resource::<EditorAuthoringSession>().revision;
        let target = click(&mut app, index); // 每次只有一个真实后台射线请求，没有补点第二次。
        finish(&mut app, &target, panel);
        assert_eq!(
            app.world()
                .resource::<LayerCandidateState>()
                .selection_revision,
            click_index as u64 + 1,
            "后台载入不能伪造第二次选择确认"
        );
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.revision, revision + 1);
        assert!(session.undo.is_empty() && session.redo.is_empty());
        assert!(!app.world().resource::<MapEditorState>().dirty);
    }
}

#[test]
fn continuous_sculpt_revisions_keep_confirmed_mesh_and_properties_without_reselection() {
    let (mut app, panel) = app();
    let target = click(&mut app, 0);
    finish(&mut app, &target, panel);
    let confirmations = app.world().resource::<LayerCandidateState>().selection_revision;
    app.world_mut().resource_mut::<ViewportModeState>().mode = crate::viewport::modes::ViewportMode::Edit;
    app.world_mut().resource_mut::<MapEditorState>().tool = crate::types::EditorTool::Raise;
    for _ in 0..12 {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        session.revision += 1; session.terrain_revision += 1;
        drop(session);
        app.update();
        assert_eq!(app.world().resource::<LayerCandidateState>().selected.as_ref(), Some(&target));
        assert_eq!(app.world().resource::<LayerCandidateState>().selection_revision, confirmations);
        assert_eq!(app.world().get::<Node>(panel).unwrap().display, Display::Flex);
        assert_eq!(app.world().resource::<TerrainObjectsEditorState>().selected_index, Some(0));
    }
    app.world_mut().resource_mut::<ViewportModeState>().mode = crate::viewport::modes::ViewportMode::Select;
    app.update();
    assert_eq!(app.world().resource::<LayerCandidateState>().selected.as_ref(), Some(&target));
    assert_eq!(app.world().get::<Node>(panel).unwrap().display, Display::Flex);
}

#[test]
fn canvas_completion_does_not_reselect_terrain_after_scene_tree_takes_ownership() {
    let (mut app, _) = app();
    click(&mut app, 0);
    let started = Instant::now();
    while !app
        .world()
        .resource::<TerrainObjectsEditorState>()
        .documents_loading
    {
        app.update();
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(1));
    }
    let target = LayerTarget::Placement("scene-tree-object".into());
    {
        let mut candidates = app.world_mut().resource_mut::<LayerCandidateState>();
        candidates.clear_for_scene_tree(); // 明确的树选择淘汰旧点击，加载旧画布不能再抢回网格属性。
        candidates.selected = Some(target.clone());
    }
    while app
        .world()
        .resource::<TerrainObjectsEditorState>()
        .documents_loading
    {
        app.update();
        assert_eq!(
            app.world()
                .resource::<LayerCandidateState>()
                .selected
                .as_ref(),
            Some(&target)
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().revision, 8);
}

#[test]
fn late_empty_pick_cannot_clear_selection_after_canvas_switch() {
    let (mut app, panel) = app();
    let first = click(&mut app, 0);
    finish(&mut app, &first, panel); // 完整候选就绪才确认首次选择，不依赖载入期间曾公布“只有网格”的半套结果。
    let started = Instant::now();
    let (release, receiver) = std::sync::mpsc::channel();
    {
        let mut candidates = app.world_mut().resource_mut::<LayerCandidateState>();
        let key = candidates.identity.clone().unwrap();
        candidates.request(
            key,
            Ray3d::new(Vec3::Y, Dir3::NEG_Y),
            None,
            Vec2::ZERO,
            Vec2::splat(1000.0),
            1.0,
            None,
        );
        let request = candidates.request.take().unwrap();
        let key = request.identity.clone();
        candidates.task = Some(bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
            let _ = receiver.recv(); // 仅测试屏障延迟旧修订空命中；正式系统仍非阻塞轮询同一在途槽。
            (key, request, Vec::new())
        }));
    }
    // 旧空查询已经运行后再明确选择另一个网格；正式树选择必须永久淘汰旧票据。
    let second = TerrainId::parse("terrain-b").unwrap();
    let target = LayerTarget::TerrainObject(second.as_str().to_owned());
    app.world_mut().resource_mut::<LayerCandidateState>().confirm_terrain(&second, 1);
    {
        let mut terrains = app.world_mut().resource_mut::<TerrainObjectsEditorState>();
        terrains.selected = Some(second);
        terrains.selected_index = Some(1);
    }
    app.update(); // 与生产树选择一样，由既有画布系统开始载入第二个网格。
    assert!(app.world().resource::<TerrainObjectsEditorState>().documents_loading);
    while app
        .world()
        .resource::<TerrainObjectsEditorState>()
        .documents_loading
    {
        app.update();
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        app.world().resource::<LayerCandidateState>().task.is_some(),
        "取消采用资格不能提前释放运行中的任务槽"
    );
    release.send(()).unwrap();
    while app.world().resource::<LayerCandidateState>().task.is_some() {
        app.update();
        assert_eq!(
            app.world()
                .resource::<LayerCandidateState>()
                .selected
                .as_ref(),
            Some(&target)
        );
        assert_eq!(
            app.world().get::<Node>(panel).unwrap().display,
            Display::Flex
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!app.world().resource::<LayerCandidateState>().clear_objects);
}
