//! 三维地形手柄必须使用世界坐标，真实输入和层级传播验证源局部记录、完整仿射和单次历史。

use super::*;
use crate::viewport::multi_select::{PlacementSelectionSet, SelectedMember};
use feathrise_scene_content::{ScenePlacementTerrainPatch, SceneTerrainDescriptor, SceneTerrainKind,
    SceneTerrainObject, TerrainId, TerrainObjectTransform};

fn source(frame: TerrainObjectTransform) -> SceneTerrainObject {
    SceneTerrainObject {
        clipmap_stitches: default(), mesh_lod: None,
        id: TerrainId::parse("move-patch-source").unwrap(), display_name: "手柄来源".into(), enabled: true,
        transform: frame, world_min: [0.0; 2], world_max: [64.0; 2],
        terrain: SceneTerrainDescriptor { kind: SceneTerrainKind::SparseClipmap,
            resolution: [65; 2], ..SceneTerrainDescriptor::empty() },
        voxel: None, surface_path: None, vegetation_path: None, water_path: None, puddles: None,
    }
}

fn local(record: &ScenePlacementRecord) -> Transform {
    Transform { translation: record.translation.into(), rotation: Quat::from_rotation_y(record.yaw_degrees.to_radians()),
        scale: record.scale.into() }
}

fn transformed_group(frame: TerrainObjectTransform) -> (App, [Entity; 2], Entity, [Entity; 2]) {
    let (mut app, roots, node, ordinary_parent) = multi_select::group_app();
    // 正式根在提交或取消后可恢复作者预览，夹具须注册该预览实际请求的资产类型。
    app.init_asset::<bevy::world_serialization::WorldAsset>();
    let terrain = source(frame);
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    let parent = feathrise_scene::runtime_test_support::install_active_terrain_instances(app.world_mut(), &map, &[terrain.clone()])[0];
    let items = {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.terrains = Some(vec![terrain.clone()].into());
        let first = &mut draft.placements.placements[0];
        first.terrain_patch = Some(ScenePlacementTerrainPatch { terrain_id: Some(terrain.id), ..default() });
        first.water_surfaces.clear();
        draft.placement_water_surface_count = 1;
        draft.placements.placements.clone()
    };
    app.world_mut().entity_mut(roots[0]).insert((ChildOf(parent), local(&items[0]),
        feathrise_scene::ScenePlacementInstance::new(&map, &items[0])));
    app.world_mut().resource_scope(|world, mut selection: Mut<PlacementSelectionSet>| {
        selection.publish(items.iter().enumerate().map(|(index, item)| SelectedMember {
            id: item.id.clone(), index,
            position: if index == 0 { frame.as_transform().transform_point(item.translation.into()) } else { item.translation.into() },
        }).collect(), world.resource::<EditorAuthoringSession>());
    });
    let center = app.world().resource::<PlacementSelectionSet>().center;
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = center;
    app.update();
    (app, roots, node, [parent, ordinary_parent])
}

fn assert_world(app: &App, root: Entity, expected: Mat4) {
    let actual = app.world().get::<GlobalTransform>(root).unwrap().to_matrix();
    // 比较八角能发现世界中心正确而非等比源父带来的剪切被错误丢弃。
    for x in [-1.0, 1.0] { for y in [-1.0, 1.0] { for z in [-1.0, 1.0] {
        let point = Vec3::new(x, y, z);
        assert!(actual.transform_point3(point).abs_diff_eq(expected.transform_point3(point), 0.004),
            "世界几何不一致：{actual:?} != {expected:?}");
    } } }
}

fn single_patch(frame: TerrainObjectTransform, min: f32, cutout: bool) -> (App, Entity, Entity, Entity) {
    let (mut app, root, node) = app();
    // 单体同步同样经过真实作者预览入口，不能依靠尚未触发加载的路径绕过资产初始化。
    app.init_asset::<bevy::world_serialization::WorldAsset>();
    app.add_plugins(bevy::transform::TransformPlugin);
    let mut terrain = source(frame);
    terrain.world_min = [min; 2]; terrain.world_max = [min + 64.0; 2];
    let map = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().scene_id.clone();
    let parent = feathrise_scene::runtime_test_support::install_active_terrain_instances(app.world_mut(), &map, &[terrain.clone()])[0];
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        draft.manifest.terrains = Some(vec![terrain.clone()].into());
        let item = &mut draft.placements.placements[0];
        item.terrain_patch = Some(ScenePlacementTerrainPatch { terrain_id: Some(terrain.id),
            cutout: cutout.then_some(feathrise_scene_content::SceneTerrainCutout { min: [-1.0; 2], max: [1.0; 2] }), ..default() });
        item.translation = [min + 5.0, 4.0, min + 5.0];
        if cutout { item.yaw_degrees = 90.0; }
        item.water_surfaces.clear();
        draft.placement_water_surface_count = 0;
    }
    let item = draft_record(&app).clone();
    // 作者预览映射必须指向身份完整的根，否则同步入口会按陈旧地址安全重建，不能继续断言旧实体。
    app.world_mut().entity_mut(root).insert((ChildOf(parent), local(&item),
        feathrise_scene::ScenePlacementInstance::new(&map, &item)));
    let origin = frame.as_transform().transform_point(item.translation.into());
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = origin;
    app.update();
    (app, root, node, parent)
}

fn single_point(app: &App, origin: Vec3, delta: f32) -> Vec2 {
    let camera = camera(app);
    camera.project(origin + Vec3::X * (camera.handle_length(origin).unwrap() * 0.65 + delta)).unwrap()
}

/// 模拟作者来源移除或实际运行代际变化；返回取消后仍存在的真实父级世界框架。
fn change_source(app: &mut App, parent: Entity, reason: usize, frame: TerrainObjectTransform) -> Mat4 {
    match reason {
        0 => {
            app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap().manifest.terrains = Some(Vec::new().into());
        }
        1 => {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap())[0].enabled = false;
        }
        2 => {
            let next = TerrainObjectTransform { translation: [13.0, -2.0, 11.0], yaw_degrees: -25.0,
                pitch_degrees: 12.0, scale: [1.5, 2.5, 3.5], ..default() };
            let object = {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                let object = &mut Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap())[0];
                object.transform = next; object.clone()
            };
            app.world_mut().entity_mut(parent).insert((next.as_transform(),
                feathrise_scene_core::residency::SceneSourceFrame::from_transform(next.as_transform()),
                feathrise_scene::TerrainObjectInstance { definition: Arc::new(object) }));
            return next.as_transform().to_matrix();
        }
        3 => app.world_mut().get_mut::<feathrise_scene::SubsceneInstance>(parent).unwrap().generation += 1,
        4 => {
            let next = TerrainObjectTransform { translation: [13.0, -2.0, 11.0], ..frame };
            app.world_mut().entity_mut(parent).insert((next.as_transform(),
                feathrise_scene_core::residency::SceneSourceFrame::from_transform(next.as_transform())));
            return next.as_transform().to_matrix();
        }
        5 => {
            let batch = app.world().get::<feathrise_scene::SubsceneInstance>(parent).unwrap().batch;
            app.world_mut().entity_mut(batch).remove::<feathrise_scene::SubsceneActive>();
        }
        _ => panic!("未知来源失效原因"),
    }
    frame.as_transform().to_matrix()
}

#[test]
fn patch_single_source_removal_pose_and_runtime_generation_cancel_without_history_or_stale_preview() {
    for reason in 0..6 {
        let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 90.0,
            scale: [2.0, 3.0, 4.0], ..default() };
        let (mut app, root, node, parent) = single_patch(frame, 0.0, false);
        let before = draft_record(&app).clone();
        let origin = frame.as_transform().transform_point(before.translation.into());
        let start = single_point(&app, origin, 0.0);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, start);
        let cursor = single_point(&app, origin, 1.0);
        step(&mut app, node, cursor);
        assert!(app.world().resource::<PlacementMoveState>().drag.as_ref().is_some_and(|drag| drag.preview_applied));
        let current_frame = change_source(&mut app, parent, reason, frame);
        step(&mut app, node, cursor);
        assert!(app.world().resource::<PlacementMoveState>().drag.is_none(), "来源变化原因{reason}应取消");
        assert_eq!(draft_record(&app), &before);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        assert!(!app.world().resource::<SceneEditorState>().dirty);
        if matches!(reason, 2 | 3) {
            // 新作者框架已完整采用或只换代时，当前来源仍可用，恢复当前真实父与完整世界几何。
            assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent);
            assert_world(&app, root, current_frame * local(&before).to_matrix());
        } else {
            // 缺失、禁用、旧实际框架和退役批次不能继续显示；尚存自有根回滚局部姿态后隐藏脱父。
            assert_eq!(app.world().get::<Visibility>(root), Some(&Visibility::Hidden));
            assert!(app.world().get::<ChildOf>(root).is_none());
            assert_world(&app, root, local(&before).to_matrix());
        }
        assert!(app.world().resource::<PlacementMoveState>().owns_mouse());
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        step(&mut app, node, cursor);
        step(&mut app, node, cursor);
        assert!(!app.world().resource::<PlacementMoveState>().owns_mouse());
    }
}

#[test]
fn patch_single_refuses_new_author_pose_before_actual_source_frame_is_adopted() {
    let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 90.0,
        scale: [2.0, 3.0, 4.0], ..default() };
    let (mut app, root, node, _) = single_patch(frame, 0.0, false);
    let before = draft_record(&app).clone();
    let matrix = app.world().get::<GlobalTransform>(root).unwrap().to_matrix();
    let next = TerrainObjectTransform { translation: [30.0, 3.0, 7.0], ..frame };
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        Arc::make_mut(session.draft.as_mut().unwrap().manifest.terrains.as_mut().unwrap())[0].transform = next;
    }
    let origin = next.as_transform().transform_point(before.translation.into());
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = origin;
    let cursor = single_point(&app, origin, 0.0);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut app, node, cursor);
    assert!(app.world().resource::<PlacementMoveState>().drag.is_none());
    assert!(app.world().resource::<SceneEditorState>().status.contains("姿态已变化"));
    assert_eq!(draft_record(&app), &before);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert_world(&app, root, matrix);
}

#[test]
fn patch_single_world_move_preserves_source_local_record_and_parent_for_saved_and_unsaved_roots() {
    for formal in [false, true] {
        let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 90.0,
            scale: [2.0, 3.0, 4.0], ..default() };
        let (mut app, root, node, parent) = single_patch(frame, 0.0, false);
        let before = draft_record(&app).clone();
        if formal {
            app.add_observer(crate::ui::placement_water::register_placement_instance_root);
            let map = {
                let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
                session.placement_previews.clear(); session.draft.as_ref().unwrap().scene_id.clone()
            };
            // observer在夹具根生成后才安装，重新触发Add才能登记本次模拟的正式根。
            app.world_mut().entity_mut(root).remove::<feathrise_scene::ScenePlacementInstance>();
            app.world_mut().entity_mut(root).insert(feathrise_scene::ScenePlacementInstance::new(&map, &before));
        }
        let matrix = app.world().get::<GlobalTransform>(root).unwrap().to_matrix();
        let origin = matrix.w_axis.truncate();
        let start = single_point(&app, origin, 0.0);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, start);
        assert!(app.world().resource::<PlacementMoveState>().drag.is_some());
        let cursor = single_point(&app, origin, 1.0);
        step(&mut app, node, cursor);
        let preview = *app.world().resource::<EditorAuthoringSession>().placement_previews.get(&before.id).unwrap();
        assert_eq!(app.world().get::<ChildOf>(preview).unwrap().parent(), parent);
        assert_world(&app, preview, Mat4::from_translation(Vec3::X) * matrix);
        assert_eq!(draft_record(&app), &before);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        step(&mut app, node, cursor);
        let after = draft_record(&app);
        assert!((after.translation[2] - before.translation[2] - 0.25).abs() < 0.002);
        assert!(after.terrain_binding.is_none());
        assert_eq!(after.terrain_patch, before.terrain_patch);
        assert_eq!(after.scale, before.scale);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.undo.len(), 1);
        assert!(matches!(&session.undo[0], EditorCommand::PlacementChanged { before: old, after: new, .. } if old == &before && new == after));
        assert_world(&app, preview, Mat4::from_translation(Vec3::X) * matrix);
    }
}

#[test]
fn patch_single_cutout_uses_source_bounds_for_move_and_uniform_scale() {
    for scaling in [false, true] {
        let frame = TerrainObjectTransform { translation: [-1000.0, 0.0, -1000.0], ..default() };
        let (mut app, root, node, parent) = single_patch(frame, 1000.0, true);
        let before = draft_record(&app).clone();
        let matrix = app.world().get::<GlobalTransform>(root).unwrap().to_matrix();
        let origin = matrix.w_axis.truncate();
        if scaling { app.world_mut().resource_mut::<ViewportModeState>().mode = crate::viewport::modes::ViewportMode::Scale; }
        let camera = camera(&app);
        let start = if scaling { camera.project(origin).unwrap() } else { single_point(&app, origin, 0.0) };
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, start);
        assert!(app.world().resource::<PlacementMoveState>().drag.is_some());
        let cursor = if scaling { start + Vec2::X * 120.0 / camera.size / camera.inverse_scale }
            else { single_point(&app, origin, 1.0) };
        step(&mut app, node, cursor);
        assert!(app.world().resource::<PlacementMoveState>().drag.as_ref().unwrap().valid,
            "{}", app.world().resource::<SceneEditorState>().status);
        let change = if scaling { Mat4::from_translation(origin) * Mat4::from_scale(Vec3::splat(2.0)) * Mat4::from_translation(-origin) }
            else { Mat4::from_translation(Vec3::X) };
        assert_world(&app, root, change * matrix);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        step(&mut app, node, cursor);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
        let after = draft_record(&app);
        if scaling { assert!(Vec3::from_array(after.scale).abs_diff_eq(Vec3::from_array(before.scale) * 2.0, 0.002)); }
        else { assert!((after.translation[0] - before.translation[0] - 1.0).abs() < 0.002); }
        assert!(after.terrain_binding.is_none());
        assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent);
        assert_world(&app, root, change * matrix);
    }
}

#[test]
fn patch_single_representable_world_y_rotation_and_tilted_uniform_scale_commit_exact_geometry() {
    for scaling in [false, true] {
        let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 35.0,
            pitch_degrees: if scaling { 20.0 } else { 0.0 },
            scale: if scaling { [2.0, 3.0, 4.0] } else { [2.0; 3] }, ..default() };
        let (mut app, root, node, parent) = single_patch(frame, 0.0, false);
        let matrix = app.world().get::<GlobalTransform>(root).unwrap().to_matrix();
        let origin = matrix.w_axis.truncate();
        plane_tests::set_camera(&mut app, EditorView::Top, false);
        app.world_mut().resource_mut::<SceneEditorViewportState>().center = origin;
        app.world_mut().resource_mut::<ViewportModeState>().mode = if scaling { crate::viewport::modes::ViewportMode::Scale }
            else { crate::viewport::modes::ViewportMode::Rotate };
        let camera = camera(&app);
        let radius = camera.handle_length(origin).unwrap() * super::super::constants::ROTATION_RADIUS_FRACTION;
        let start = camera.project(origin + if scaling { Vec3::ZERO } else { Vec3::X * radius }).unwrap();
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, start);
        assert!(app.world().resource::<PlacementMoveState>().drag.is_some());
        let cursor = if scaling { start + Vec2::X * 120.0 / camera.size / camera.inverse_scale }
            else { camera.project(origin + super::super::rotation::yaw_direction(20.0) * radius).unwrap() };
        step(&mut app, node, cursor);
        let change = Mat4::from_translation(origin) * if scaling { Mat4::from_scale(Vec3::splat(2.0)) }
            else { Mat4::from_rotation_y(20.0_f32.to_radians()) } * Mat4::from_translation(-origin);
        assert!(app.world().resource::<PlacementMoveState>().drag.as_ref().unwrap().valid,
            "{}", app.world().resource::<SceneEditorState>().status);
        assert_world(&app, root, change * matrix);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        step(&mut app, node, cursor);
        assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
        assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent);
        assert_world(&app, root, change * matrix);
    }
}

#[test]
fn tilted_patch_single_world_y_rotation_is_rejected_without_author_history() {
    let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 35.0,
        pitch_degrees: 20.0, scale: [2.0, 3.0, 4.0], ..default() };
    let (mut app, root, node, parent) = single_patch(frame, 0.0, false);
    let before = draft_record(&app).clone();
    app.world_mut().entity_mut(root).insert((ChildOf(parent), local(&before)));
    plane_tests::set_camera(&mut app, EditorView::Top, false);
    let origin = frame.as_transform().transform_point(before.translation.into());
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = origin;
    app.world_mut().resource_mut::<ViewportModeState>().mode = crate::viewport::modes::ViewportMode::Rotate;
    app.update();
    let matrix = app.world().get::<GlobalTransform>(root).unwrap().to_matrix();
    let camera = camera(&app);
    let radius = camera.handle_length(origin).unwrap() * super::super::constants::ROTATION_RADIUS_FRACTION;
    let cursor = |angle| camera.project(origin + super::super::rotation::yaw_direction(angle) * radius).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(&mut app, node, cursor(0.0));
    assert_eq!(app.world().resource::<PlacementMoveState>().drag.as_ref().map(|drag| drag.handle), Some(MoveHandle::RotationY));
    step(&mut app, node, cursor(20.0));
    assert!(!app.world().resource::<PlacementMoveState>().drag.as_ref().unwrap().valid);
    assert_world(&app, root, matrix);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    step(&mut app, node, cursor(20.0));
    assert_eq!(draft_record(&app), &before);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent);
    assert_world(&app, root, matrix);
}

#[test]
fn patch_group_world_move_preserves_nonidentity_source_parent_and_one_history() {
    let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 90.0,
        scale: [2.0, 3.0, 4.0], ..default() };
    let (mut app, roots, node, parents) = transformed_group(frame);
    let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
    let matrices = roots.map(|root| app.world().get::<GlobalTransform>(root).unwrap().to_matrix());
    multi_select::group_begin(&mut app, node, MoveAxis::X);
    let cursor = multi_select::group_point(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    for (root, matrix) in roots.into_iter().zip(matrices) { assert_world(&app, root, Mat4::from_translation(Vec3::X) * matrix); }
    assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements, before);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    multi_select::finish(&mut app, node, cursor);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.undo.len(), 1);
    assert!(matches!(&session.undo[0], EditorCommand::CourtyardBatch(_)));
    let after = &session.draft.as_ref().unwrap().placements.placements;
    assert!((after[0].translation[2] - before[0].translation[2] - 0.25).abs() < 0.002);
    assert!((after[1].translation[0] - before[1].translation[0] - 1.0).abs() < 0.002);
    assert!(after[0].terrain_binding.is_none());
    for ((root, matrix), parent) in roots.into_iter().zip(matrices).zip(parents) {
        assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent);
        assert_world(&app, root, Mat4::from_translation(Vec3::X) * matrix);
    }
}

#[test]
fn patch_group_saved_patch_staging_parent_is_restored_after_commit_and_escape() {
    for cancel in [false, true] {
        let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 90.0,
            scale: [2.0, 3.0, 4.0], ..default() };
        let (mut app, roots, node, parents) = transformed_group(frame);
        let before_records = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
        let before = before_records[0].clone();
        let revision = app.world().resource::<EditorAuthoringSession>().revision;
        let staging = app.world_mut().spawn((Transform::IDENTITY, Visibility::Inherited, ChildOf(parents[0]))).id();
        app.add_observer(crate::ui::placement_water::register_placement_instance_root)
            .add_observer(crate::ui::placement_water::unregister_placement_instance_root);
        let map = {
            let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
            session.placement_previews.remove(&before.id); session.draft.as_ref().unwrap().scene_id.clone()
        };
        // 正式补丁保持归原staging父级拥有；Add真实登记后，Preparing必须另取独立作者预览。
        app.world_mut().entity_mut(roots[0]).remove::<feathrise_scene::ScenePlacementInstance>();
        app.world_mut().entity_mut(roots[0]).insert((ChildOf(staging), feathrise_scene::ScenePlacementInstance::new(&map, &before)));
        app.update();
        let matrices = roots.map(|root| app.world().get::<GlobalTransform>(root).unwrap().to_matrix());
        let formal_transform = *app.world().get::<Transform>(roots[0]).unwrap();
        let start = multi_select::group_point(&app, MoveAxis::X, 0.0);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
        step(&mut app, node, start);
        // 缺失预览先补生，再由确切Add继续准备；不能把固定两帧等同于接管正式根。
        for _ in 0..8 {
            step(&mut app, node, start);
            if app.world().resource::<PlacementMoveState>().group.as_ref()
                .is_some_and(|group| group.phase == super::super::group_types::GroupPhase::Dragging) { break; }
        }
        let preview = app.world().resource::<EditorAuthoringSession>().placement_previews[&before.id];
        assert_ne!(preview, roots[0], "已保存补丁只能通过独立作者根预览整组变换");
        assert!(app.world().get::<crate::components::EditorPlacedAsset>(preview).is_some());
        let previews = [preview, roots[1]];
        let movement = app.world().resource::<PlacementMoveState>();
        let drag = movement.group.as_ref().expect("独立作者预览准备后应保留整组手势");
        assert!(drag.phase == super::super::group_types::GroupPhase::Dragging,
            "{}", app.world().resource::<SceneEditorState>().status);
        assert!(drag.roots.iter().all(|root| previews.contains(&root.entity)));
        let helpers = group_helpers(&app);
        let cursor = multi_select::group_point(&app, MoveAxis::X, 1.0);
        step(&mut app, node, cursor);
        assert_eq!(app.world().get::<Transform>(roots[0]), Some(&formal_transform));
        assert_eq!(app.world().get::<ChildOf>(roots[0]).unwrap().parent(), staging);
        assert_world(&app, roots[0], matrices[0]);
        for (root, matrix) in previews.into_iter().zip(matrices) { assert_world(&app, root, Mat4::from_translation(Vec3::X) * matrix); }
        assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements, before_records);
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
        if cancel {
            app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
            step(&mut app, node, cursor);
        }
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        multi_select::finish(&mut app, node, cursor);
        assert_eq!(app.world().get::<Transform>(roots[0]), Some(&formal_transform));
        assert_eq!(app.world().get::<ChildOf>(roots[0]).unwrap().parent(), staging);
        assert_world(&app, roots[0], matrices[0]);
        assert!(app.world().get::<feathrise_scene::ScenePlacementInstance>(roots[0]).unwrap()
            .matches_record(&map, &before, &before.water_surfaces));
        assert_eq!(app.world().get::<Visibility>(roots[0]), Some(&Visibility::Hidden));
        assert_eq!(app.world().get::<ChildOf>(staging).unwrap().parent(), parents[0]);
        assert_eq!(app.world().get::<Transform>(staging), Some(&Transform::IDENTITY));
        assert_eq!(app.world().get::<ChildOf>(preview).unwrap().parent(), parents[0],
            "独立作者补丁归还真实地形源父，不能依附可退休的正式staging容器");
        assert_eq!(app.world().get::<ChildOf>(roots[1]).unwrap().parent(), parents[1]);
        let change = if cancel { Mat4::IDENTITY } else { Mat4::from_translation(Vec3::X) };
        for (root, matrix) in previews.into_iter().zip(matrices) { assert_world(&app, root, change * matrix); }
        let session = app.world().resource::<EditorAuthoringSession>();
        let after = &session.draft.as_ref().unwrap().placements.placements;
        assert_eq!(session.placement_previews[&before.id], preview);
        assert_eq!(session.undo.len(), usize::from(!cancel));
        assert!(session.redo.is_empty());
        assert_eq!(session.revision, revision + u64::from(!cancel));
        assert_eq!(app.world().resource::<SceneEditorState>().dirty, !cancel);
        if cancel {
            assert_eq!(after, &before_records);
        } else {
            assert!(matches!(&session.undo[0], EditorCommand::CourtyardBatch(_)));
            assert!((after[0].translation[2] - before.translation[2] - 0.25).abs() < 0.002);
            assert!((after[1].translation[0] - before_records[1].translation[0] - 1.0).abs() < 0.002);
        }
        for (root, record) in previews.into_iter().zip(after) {
            assert!(app.world().get::<feathrise_scene::ScenePlacementInstance>(root).unwrap()
                .matches_record(&map, record, &record.water_surfaces));
        }
        assert_helpers_cleaned(&app, &helpers);
    }
}

fn group_helpers(app: &App) -> Vec<Entity> {
    let movement = app.world().resource::<PlacementMoveState>();
    let drag = movement.group.as_ref().unwrap();
    let mut pending = drag.pivots.iter().map(|pivot| pivot.cleanup).collect::<Vec<_>>();
    let mut helpers = Vec::new();
    while let Some(entity) = pending.pop() {
        if drag.roots.iter().any(|root| root.entity == entity) { continue; }
        helpers.push(entity);
        if let Some(children) = app.world().get::<Children>(entity) { pending.extend(children.iter()); }
    }
    helpers
}

fn assert_helpers_cleaned(app: &App, helpers: &[Entity]) {
    for entity in helpers { assert!(app.world().get_entity(*entity).is_err(), "临时节点{entity:?}未清理"); }
}

#[test]
fn patch_group_source_removal_pose_and_generation_cancel_before_worker_result_is_adopted() {
    for waiting in [false, true] {
        for reason in 0..6 {
            let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 90.0,
                scale: [2.0, 3.0, 4.0], ..default() };
            let (mut app, roots, node, parents) = transformed_group(frame);
            let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
            let ordinary_matrix = app.world().get::<GlobalTransform>(roots[1]).unwrap().to_matrix();
            multi_select::group_begin(&mut app, node, MoveAxis::X);
            let helpers = group_helpers(&app);
            let cursor = multi_select::group_point(&app, MoveAxis::X, 1.0);
            step(&mut app, node, cursor);
            if waiting {
                app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
                step(&mut app, node, cursor);
                assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
                    .is_some_and(|group| group.phase == super::super::group_types::GroupPhase::Waiting));
            }
            let current_frame = change_source(&mut app, parents[0], reason, frame);
            step(&mut app, node, cursor);
            app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
            multi_select::finish(&mut app, node, cursor);
            let session = app.world().resource::<EditorAuthoringSession>();
            assert_eq!(session.draft.as_ref().unwrap().placements.placements, before, "reason={reason}, waiting={waiting}");
            assert!(session.undo.is_empty());
            assert!(!app.world().resource::<SceneEditorState>().dirty);
            assert_eq!(app.world().get::<ChildOf>(roots[0]).unwrap().parent(), parents[0]);
            assert_eq!(app.world().get::<ChildOf>(roots[1]).unwrap().parent(), parents[1]);
            assert_world(&app, roots[0], current_frame * local(&before[0]).to_matrix());
            assert_world(&app, roots[1], ordinary_matrix);
            assert_helpers_cleaned(&app, &helpers);
        }
    }
}

#[test]
fn patch_group_cleanup_preserves_root_reparented_by_new_runtime_owner() {
    let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 90.0,
        scale: [2.0, 3.0, 4.0], ..default() };
    let (mut app, roots, node, parents) = transformed_group(frame);
    let ordinary_matrix = app.world().get::<GlobalTransform>(roots[1]).unwrap().to_matrix();
    multi_select::group_begin(&mut app, node, MoveAxis::X);
    let helpers = group_helpers(&app);
    let cursor = multi_select::group_point(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    let external_frame = Transform::from_xyz(20.0, -3.0, 11.0).with_rotation(Quat::from_rotation_z(0.3));
    let external_parent = app.world_mut().spawn((external_frame, Visibility::Inherited)).id();
    let external_local = Transform::from_xyz(2.0, 1.0, 4.0).with_scale(Vec3::new(2.0, 1.0, 3.0));
    app.world_mut().entity_mut(roots[0]).insert((ChildOf(external_parent), external_local));
    change_source(&mut app, parents[0], 3, frame);
    step(&mut app, node, cursor);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    multi_select::finish(&mut app, node, cursor);
    assert_eq!(app.world().get::<ChildOf>(roots[0]).unwrap().parent(), external_parent);
    assert_eq!(*app.world().get::<Transform>(roots[0]).unwrap(), external_local);
    assert_world(&app, roots[0], external_frame.to_matrix() * external_local.to_matrix());
    assert_world(&app, roots[1], ordinary_matrix);
    assert_helpers_cleaned(&app, &helpers);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
}

fn start_group_transform(app: &mut App, node: Entity, scaling: bool) -> (Vec3, MoveCamera, Vec2) {
    let origin = app.world().resource::<PlacementSelectionSet>().center;
    plane_tests::set_camera(app, EditorView::Top, false);
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = origin;
    app.world_mut().resource_mut::<ViewportModeState>().mode = if scaling { crate::viewport::modes::ViewportMode::Scale }
        else { crate::viewport::modes::ViewportMode::Rotate };
    let camera = camera(app);
    let radius = camera.handle_length(origin).unwrap() * super::super::constants::ROTATION_RADIUS_FRACTION;
    let start = camera.project(origin + if scaling { Vec3::ZERO } else { Vec3::X * radius }).unwrap();
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    step(app, node, start); step(app, node, start);
    assert!(app.world().resource::<PlacementMoveState>().group.as_ref()
        .is_some_and(|group| group.phase == super::super::group_types::GroupPhase::Dragging));
    (origin, camera, start)
}

#[test]
fn patch_group_representable_rotation_and_tilted_uniform_scale_commit_exact_affine_geometry() {
    for scaling in [false, true] {
        let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 35.0,
            pitch_degrees: if scaling { 20.0 } else { 0.0 }, roll_degrees: if scaling { 12.0 } else { 0.0 },
            scale: if scaling { [2.0, 3.0, 4.0] } else { [2.0; 3] }, ..default() };
        let (mut app, roots, node, parents) = transformed_group(frame);
        let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
        let matrices = roots.map(|root| app.world().get::<GlobalTransform>(root).unwrap().to_matrix());
        let (origin, camera, start) = start_group_transform(&mut app, node, scaling);
        let helpers = group_helpers(&app);
        let radius = camera.handle_length(origin).unwrap() * super::super::constants::ROTATION_RADIUS_FRACTION;
        let cursor = if scaling { start + Vec2::X * 120.0 / camera.size / camera.inverse_scale }
            else { camera.project(origin + super::super::rotation::yaw_direction(20.0) * radius).unwrap() };
        step(&mut app, node, cursor);
        let change = Mat4::from_translation(origin) * if scaling { Mat4::from_scale(Vec3::splat(2.0)) }
            else { Mat4::from_rotation_y(20.0_f32.to_radians()) } * Mat4::from_translation(-origin);
        for (root, matrix) in roots.into_iter().zip(matrices) { assert_world(&app, root, change * matrix); }
        assert_eq!(app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements, before);
        app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
        multi_select::finish(&mut app, node, cursor);
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(session.undo.len(), 1);
        assert!(matches!(&session.undo[0], EditorCommand::CourtyardBatch(_)));
        for ((root, matrix), parent) in roots.into_iter().zip(matrices).zip(parents) {
            assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent);
            assert_world(&app, root, change * matrix);
        }
        assert_helpers_cleaned(&app, &helpers);
    }
}

#[test]
fn patch_group_unrepresentable_world_rotation_restores_every_member_and_original_parent() {
    let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 35.0,
        pitch_degrees: 20.0, roll_degrees: 12.0, scale: [2.0, 3.0, 4.0], ..default() };
    let (mut app, roots, node, parents) = transformed_group(frame);
    let before = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap().placements.placements.clone();
    let matrices = roots.map(|root| app.world().get::<GlobalTransform>(root).unwrap().to_matrix());
    let (origin, camera, _) = start_group_transform(&mut app, node, false);
    let helpers = group_helpers(&app);
    let radius = camera.handle_length(origin).unwrap() * super::super::constants::ROTATION_RADIUS_FRACTION;
    let cursor = camera.project(origin + super::super::rotation::yaw_direction(20.0) * radius).unwrap();
    step(&mut app, node, cursor);
    let change = Mat4::from_translation(origin) * Mat4::from_rotation_y(20.0_f32.to_radians()) * Mat4::from_translation(-origin);
    for (root, matrix) in roots.into_iter().zip(matrices) { assert_world(&app, root, change * matrix); }
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    multi_select::finish(&mut app, node, cursor);
    let session = app.world().resource::<EditorAuthoringSession>();
    assert_eq!(session.draft.as_ref().unwrap().placements.placements, before);
    assert!(session.undo.is_empty());
    assert!(!app.world().resource::<SceneEditorState>().dirty);
    for ((root, matrix), parent) in roots.into_iter().zip(matrices).zip(parents) {
        assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent);
        assert_world(&app, root, matrix);
    }
    assert_helpers_cleaned(&app, &helpers);
}

#[test]
fn tilted_patch_group_preview_preserves_shear_and_escape_restores_full_world_geometry() {
    let frame = TerrainObjectTransform { translation: [10.0, 3.0, 7.0], yaw_degrees: 35.0,
        pitch_degrees: 20.0, roll_degrees: 12.0, scale: [2.0, 3.0, 4.0], ..default() };
    let (mut app, roots, node, parents) = transformed_group(frame);
    let matrices = roots.map(|root| app.world().get::<GlobalTransform>(root).unwrap().to_matrix());
    multi_select::group_begin(&mut app, node, MoveAxis::X);
    let cursor = multi_select::group_point(&app, MoveAxis::X, 1.0);
    step(&mut app, node, cursor);
    for (root, matrix) in roots.into_iter().zip(matrices) { assert_world(&app, root, Mat4::from_translation(Vec3::X) * matrix); }
    app.world_mut().resource_mut::<ButtonInput<KeyCode>>().press(KeyCode::Escape);
    step(&mut app, node, cursor);
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    multi_select::finish(&mut app, node, cursor);
    assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    for ((root, matrix), parent) in roots.into_iter().zip(matrices).zip(parents) {
        assert_eq!(app.world().get::<ChildOf>(root).unwrap().parent(), parent);
        assert_world(&app, root, matrix);
    }
}
