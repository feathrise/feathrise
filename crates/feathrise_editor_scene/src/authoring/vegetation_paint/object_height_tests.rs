//! 普通Clipmap坡面通过真实相机和后台光标进入二维R8；两套光标不能把同一高度面分流到三维字段。

use super::*;
use feathrise_scene::{ClipmapGroundHit, TerrainObjectRaySnapshot};
use feathrise_scene_core::residency::SceneSourceFrame;

fn slope_camera_fixture() -> (App, SceneId, TerrainId, Vec3) {
    let mut app = fixture();
    let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
    let map = draft.scene_id.clone();
    let id = draft.terrain_documents.active.as_ref().unwrap().definition.id.clone();
    let ground = feathrise_scene::runtime_test_support::clipmap_height_query_fixture(map.clone(), 0);
    let ClipmapGroundHit::Hit(center) = ground.sample_authoritative_surface(Vec2::new(22., 12.))
        else { panic!("真实坡面必须具有同代作者页"); };
    assert!(center.y > 5., "必须使用实际非零坡面，不能借水平面替代高度查询");
    let adjacent = ground.sample_authoritative_surface(Vec2::new(23., 12.));
    assert!(matches!(adjacent, ClipmapGroundHit::Hit(point) if (point.y-center.y).abs()>0.1));
    {
        let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
        let draft = session.draft.as_mut().unwrap();
        let mut object = (*draft.terrain_documents.active.as_ref().unwrap().definition).clone();
        object.world_min = [0.; 2]; object.world_max = [96., 64.];
        draft.manifest.terrains = Some(Arc::from([object.clone()]));
        let manifest = object.source_manifest(&draft.manifest);
        let active = draft.terrain_documents.active.as_mut().unwrap();
        active.definition = Arc::new(object.clone()); active.manifest = manifest;
        let vegetation = draft.vegetation.as_mut().unwrap();
        for layer in &mut vegetation.layers { layer.resolution = [97, 65]; }
        for layer in &mut vegetation.authoring.as_mut().unwrap().layers { layer.resolution = [97, 65]; }
        for pixels in draft.vegetation_author_densities.values_mut() { *pixels = Arc::new(vec![0; 97 * 65]); }
        draft.heights = None; draft.terrain_mesh = None; draft.terrain_editing_index = None;
    }
    app.world_mut().resource_mut::<SceneEditorViewportState>().center = center;
    (app, map, id, center)
}

fn install_slope_ground(
    app: &mut App,
    id: &TerrainId,
    ground: feathrise_scene::ClipmapGroundSnapshot,
    generation: u64,
    geometry_revision: u8,
    sibling_position: Option<Vec3>,
) {
    let mut session = app.world_mut().resource_mut::<EditorAuthoringSession>();
    let draft = session.draft.as_mut().unwrap();
    let pose = draft.terrain_documents.active.as_ref().unwrap().definition.transform;
    let query = TerrainObjectRaySnapshot::Clipmap(ground, SceneSourceFrame::from_transform(pose.as_transform()));
    let mut sources = vec![(id.clone(), pose, query, [geometry_revision; 32])];
    if let Some(position) = sibling_position {
        let sibling = TerrainId::parse("sibling-clipmap").unwrap();
        let pose = TerrainObjectTransform { translation: position.to_array(), ..default() };
        let solid = avian3d::prelude::Collider::cuboid(4., 1., 4.);
        let query = TerrainObjectRaySnapshot::Mesh(solid, SceneSourceFrame::from_transform(pose.as_transform()));
        sources.push((sibling, pose, query, [8; 32]));
    }
    draft.terrain_ground = Some(TerrainGroundSnapshot::test_queries(draft.scene_id.clone(), generation, sources));
}

#[test]
fn real_clipmap_height_cursor_starts_only_the_two_dimensional_vegetation_brush() {
    let (mut app, map, id, center) = slope_camera_fixture();
    let ground = feathrise_scene::runtime_test_support::clipmap_height_query_fixture(map, 0);
    let visible = ground.ray_surface(center + Vec3::Y * 30., Vec3::NEG_Y, 100.).unwrap().0;
    let ground = feathrise_scene::runtime_test_support::clipmap_with_surface_sheet(ground, visible - Vec3::Y * 0.0005);
    assert!(ground.is_three_dimensional_surface(visible), "半毫米背面须能触发旧点距误判");
    install_slope_ground(&mut app, &id, ground, 1, 1, None);
    until(&mut app, |app| app.world().resource::<SceneEditorState>().brush_cursor_world.is_some()
        && !cursor_running_for_tests(app.world()));
    let cursor = app.world().resource::<TerrainGroundCursor>();
    assert_eq!(cursor.terrain.as_ref(), Some(&id));
    assert!(cursor.volume_ray.is_none(), "普通Clipmap最近高度面不得启动三维字段");
    let legacy = app.world().resource::<VegetationGroundCursor>();
    assert!(legacy.volume_paint_ray().is_none() && !legacy.has_volume_cursor(),
        "旧光标和对象光标必须共同让出三维绘制资格");

    begin(&mut app).send(()).unwrap();
    until(&mut app, |app| app.world().resource::<VegetationBrushStroke>().active
        && !app.world().resource::<VegetationBrushStroke>().before.is_empty());
    let stroke = app.world().resource::<VegetationBrushStroke>();
    assert!(stroke.last_world.is_some_and(|point| point.distance(center) < 0.01));
    assert!(pixels(&app, "grass-a").iter().any(|value| *value > 0));
    assert!(pixels(&app, "grass-b").iter().all(|value| *value == 0));
    assert!(app.world().resource::<TerrainGroundCursor>().volume_ray.is_none());
    assert!(app.world().resource::<VegetationGroundCursor>().volume_paint_ray().is_none());
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<VegetationBrushStroke>().active);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1,
        "真实高度面二维起笔必须只产生一笔非空历史");
}

fn begin_cold_stroke(app: &mut App, map: SceneId, id: &TerrainId) -> feathrise_scene::ClipmapGroundSnapshot {
    let (cold, adopt) = feathrise_scene::runtime_test_support::cold_clipmap_height_query_fixture(map, 0);
    let cold_revision = cold.authoritative_page_revision();
    install_slope_ground(app, id, cold, 2, 1, None);
    begin(app).send(()).unwrap();
    until(app, |app| !cursor_running_for_tests(app.world()));
    assert!(app.world().resource::<TerrainGroundCursor>().paint_hit().is_none());
    assert!(!app.world().resource::<VegetationBrushStroke>().active);
    assert!(pixels(app, "grass-a").iter().all(|value| *value == 0));
    for _ in 0..4 {
        step(app);
        assert!(!cursor_running_for_tests(app.world()), "同一冷快照等待作者页时不能逐帧重新派发");
    }
    let adopted = adopt(); // 采用前须收到原光标落笔任务在真实高度采样中登记的页需求。
    assert!(adopted.authoritative_page_revision() > cold_revision);
    adopted
}

#[test]
fn real_clipmap_cold_press_survives_author_page_and_sibling_adoption() {
    let (mut app, map, id, center) = slope_camera_fixture();
    let adopted = begin_cold_stroke(&mut app, map, &id);
    let epoch = app.world().resource::<TerrainGroundCursor>().ray.unwrap().epoch;
    let sibling = Vec3::new(70., 10., 40.);
    install_slope_ground(&mut app, &id, adopted, 3, 1, Some(sibling));
    until(&mut app, |app| !cursor_running_for_tests(app.world())
        && app.world().resource::<TerrainGroundCursor>().paint_hit().is_some());
    let cursor = app.world().resource::<TerrainGroundCursor>();
    assert_eq!(cursor.ray.unwrap().epoch, epoch, "等待后必须复用这一次实际按压");
    assert_eq!(cursor.paint_hit().unwrap().terrain, id);
    assert!(app.world().resource::<VegetationBrushStroke>().last_world
        .is_some_and(|point| point.distance(center) < 0.01), "正式作者页到达应完成冷起笔");
    assert!(pixels(&app, "grass-a").iter().any(|value| *value > 0));
    assert!(pixels(&app, "grass-b").iter().all(|value| *value == 0));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().release(MouseButton::Left);
    until(&mut app, |app| !app.world().resource::<VegetationBrushStroke>().active);
    assert_eq!(app.world().resource::<EditorAuthoringSession>().undo.len(), 1);
}

#[test]
fn real_clipmap_cold_press_rejects_changed_target_geometry_and_new_occluder() {
    for changes_target in [true, false] {
        let (mut app, map, id, _) = slope_camera_fixture();
        let adopted = begin_cold_stroke(&mut app, map, &id);
        let ray = app.world().resource::<TerrainGroundCursor>().ray.unwrap();
        let sibling = (!changes_target).then(|| {
            let visible = adopted.ray_surface(ray.origin, ray.direction, 1_000_000.).unwrap().0;
            ray.origin.lerp(visible, 0.5)
        });
        install_slope_ground(&mut app, &id, adopted, 3, if changes_target { 2 } else { 1 }, sibling);
        let expected = if changes_target { id.as_str() } else { "sibling-clipmap" };
        let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
        let nearest = draft.terrain_ground.as_ref().unwrap().ray_visible_checked(ray.origin, ray.direction, &Default::default())
            .expect("同代上下两面须可被实际射线验证").expect("本次真实射线须命中当前目标或遮挡体");
        assert_eq!(nearest.0.as_str(), expected);
        step(&mut app);
        until(&mut app, |app| !cursor_running_for_tests(app.world())
            && app.world().resource::<TerrainGroundCursor>().terrain.as_ref().is_some_and(|terrain|terrain.as_str() == expected));
        assert!(!app.world().resource::<VegetationBrushStroke>().active);
        assert!(pixels(&app, "grass-a").iter().all(|value| *value == 0));
        assert!(app.world().resource::<EditorAuthoringSession>().undo.is_empty());
    }
}
