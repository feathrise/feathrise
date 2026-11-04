//! 多网格水网延后单击的身份门回归；测试已完成点击的路由，不代替真实Collider交互验收。

use super::object_click::{capture, take};
use crate::{
    authoring::{
        EditorAuthoringSession,
        terrain_ground::{CursorIdentity, TerrainGroundCursor},
    },
    resources::MapEditorState,
    types::{EditorTool, EditorView},
};
use bevy::prelude::*;
use feathrise_map_content::{MapWaterRiver, TerrainId};
use std::time::{Duration, Instant};

fn fixture() -> (MapEditorState, EditorAuthoringSession, TerrainGroundCursor) {
    let mut app = crate::authoring::vegetation_paint::object_tests::fixture();
    let mut editor = app.world_mut().remove_resource::<MapEditorState>().unwrap();
    let mut session = app
        .world_mut()
        .remove_resource::<EditorAuthoringSession>()
        .unwrap();
    editor.tool = EditorTool::RiverSpline;
    let draft = session.draft.as_mut().unwrap();
    let mut water =
        crate::authoring::water_asset_with_layer_for_tests(&draft.map_id, &draft.folder);
    water.layers.clear();
    water.rivers.push(MapWaterRiver {
        id: "river-current".into(),
        label: "当前河流".into(),
        source_layer_id: None,
        target_layer_id: None,
        points: Vec::new(),
        base_color_srgba: [0.1, 0.2, 0.3, 0.7],
        perceptual_roughness: 0.1,
    });
    draft.water = Some(water);
    let terrain = editor.terrain_canvas_target.clone().unwrap();
    let mut ground = TerrainGroundCursor::default();
    ground.test_paint_hit(terrain.clone(), Vec3::ZERO, None);
    ground.identity = Some(CursorIdentity {
        map: draft.map_id.clone(),
        revision: session.revision,
        terrain_revision: session.terrain_revision,
        tool: EditorTool::RiverSpline,
        ground_snapshot: None,
        asset: None,
        asset_target: None,
        portal: None,
        camera: Mat4::IDENTITY,
        size: UVec2::splat(128),
        camera_entity: Entity::PLACEHOLDER,
        projection: Mat4::IDENTITY,
        view: EditorView::Top,
        orthographic: true,
        extent: 16_f32.to_bits(),
        visibility: 0,
        relocation: None,
        translation_snap: None,
    });
    ground.click_terrain = Some(terrain);
    (editor, session, ground)
}

#[test]
fn deferred_object_river_click_consumes_only_a_completed_point_once() {
    let (mut editor, session, ground) = fixture();
    let mut pending = capture(&editor, &session, &ground);
    assert!(pending.is_some());
    // 按下时只有冻结身份；悬停不构成点击，松手后正式完成点仍可被一次消费。
    editor.cursor_world = Some(Vec3::splat(99.));
    assert!(take(&mut pending, &mut editor, &session, Some(&ground)).is_none());
    assert!(pending.is_some());
    editor.ground_click_world = Some(Vec3::new(3., 2., 7.));
    assert_eq!(
        take(&mut pending, &mut editor, &session, Some(&ground)),
        Some(Vec3::new(3., 2., 7.))
    );
    assert!(pending.is_none());
    assert!(editor.ground_click_world.is_none());
}

#[test]
fn changed_map_river_revision_canvas_or_input_authority_rejects_old_river_point() {
    for branch in 0..5 {
        let (mut editor, mut session, mut ground) = fixture();
        let mut pending = capture(&editor, &session, &ground);
        editor.ground_click_world = Some(Vec3::ONE);
        match branch {
            0 => editor.selected_map_id = Some(feathrise_map_content::MapId::parse("OTHER-map").unwrap()),
            1 => {
                session
                    .draft
                    .as_mut()
                    .unwrap()
                    .water
                    .as_mut()
                    .unwrap()
                    .rivers[0]
                    .id = "river-replaced".into()
            }
            2 => session.revision += 1,
            3 => editor.terrain_canvas_target = Some(TerrainId::parse("other-ground").unwrap()),
            _ => ground.cancel_click(),
        }
        assert!(
            take(&mut pending, &mut editor, &session, Some(&ground)).is_none(),
            "分支{branch}不得复活旧点"
        );
        assert!(pending.is_none());
    }
}

#[test]
fn completed_other_terrain_or_removed_draft_cannot_append_to_current_river() {
    let (mut editor, mut session, mut ground) = fixture();
    let mut pending = capture(&editor, &session, &ground);
    editor.ground_click_world = Some(Vec3::ONE);
    ground.click_terrain = Some(TerrainId::parse("overlap-ground").unwrap());
    assert!(take(&mut pending, &mut editor, &session, Some(&ground)).is_none());
    assert!(pending.is_none());
    let mut pending = capture(&editor, &session, &ground);
    session.draft = None;
    assert!(take(&mut pending, &mut editor, &session, Some(&ground)).is_none());
    assert!(pending.is_none());
}

#[test]
fn object_river_three_released_clicks_follow_original_collider_and_history() {
    use crate::{authoring::WaterNetworkEditState, components::MapEditorViewport};
    use bevy::ui::RelativeCursorPosition;
    let (editor, session, _) = fixture();
    let mut app = crate::authoring::vegetation_paint::object_tests::fixture();
    app.insert_resource(editor)
        .insert_resource(session)
        .init_resource::<WaterNetworkEditState>()
        .init_resource::<feathrise_scene::DynamicClipmapGroundSurface>()
        .configure_sets(
            Update,
            (
                crate::MapEditorSystemSet::Cursor,
                crate::MapEditorSystemSet::Presentation,
            )
                .chain(),
        )
        .add_systems(
            Update,
            super::edit_water_network_from_viewport.in_set(crate::MapEditorSystemSet::Presentation),
        );
    for (number, pointer) in [
        Vec2::new(-0.25, 0.),
        Vec2::new(0., 0.25),
        Vec2::new(0.25, 0.),
    ]
    .into_iter()
    .enumerate()
    {
        for mut cursor in app
            .world_mut()
            .query_filtered::<&mut RelativeCursorPosition, With<MapEditorViewport>>()
            .iter_mut(app.world_mut())
        {
            cursor.normalized = Some(pointer);
        }
        until(&mut app, |app| {
            app.world()
                .resource::<MapEditorState>()
                .cursor_world
                .is_some()
        });
        let before = app.world().resource::<EditorAuthoringSession>().undo.len();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        tick(&mut app);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .release(MouseButton::Left);
        tick(&mut app);
        until(&mut app, |app| {
            app.world()
                .resource::<EditorAuthoringSession>()
                .draft
                .as_ref()
                .unwrap()
                .water
                .as_ref()
                .unwrap()
                .rivers[0]
                .points
                .len()
                == number + 1
        });
        let session = app.world().resource::<EditorAuthoringSession>();
        assert_eq!(
            session.undo.len(),
            before + 1,
            "一次真实单击只形成一笔水网历史"
        );
        let height = session
            .draft
            .as_ref()
            .unwrap()
            .water
            .as_ref()
            .unwrap()
            .rivers[0]
            .points[number]
            .position[1];
        assert!(
            (height - 2.05).abs() < 0.0001,
            "控制点高度来自真实两米Collider而非根Empty平面"
        );
    }
    let session = app.world().resource::<EditorAuthoringSession>();
    let points = &session
        .draft
        .as_ref()
        .unwrap()
        .water
        .as_ref()
        .unwrap()
        .rivers[0]
        .points;
    let a = Vec3::from_array(points[0].position);
    let b = Vec3::from_array(points[1].position);
    let c = Vec3::from_array(points[2].position);
    assert!((b - a).xz().perp_dot((c - a).xz()).abs() > 0.1);
}

fn tick(app: &mut App) {
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
}

fn until(app: &mut App, predicate: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate(app) {
        assert!(
            Instant::now() < deadline,
            "原Collider河流单击必须有限完成：{}",
            app.world().resource::<MapEditorState>().status
        );
        tick(app);
        std::thread::sleep(Duration::from_millis(1));
    }
}
