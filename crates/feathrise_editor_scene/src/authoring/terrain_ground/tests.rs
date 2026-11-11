//! 多地形拾取与旧GLB笔刷共存：退出查询不覆盖旧作者源已发布的命中。
use super::*;
use bevy::{ecs::system::RunSystemOnce, prelude::*, ui::RelativeCursorPosition};
use crate::{authoring::{EditorAuthoringSession, test_heightfield_draft}, resources::SceneEditorState};
use std::{sync::Arc, time::Duration};

#[test]
fn adopted_terrain_bounds_keep_extreme_heights_when_reframed_or_transformed() {
    let map = feathrise_scene_content::SceneId::parse("MAP-float-height-bounds").unwrap();
    let id = feathrise_scene_content::TerrainId::parse("terrain-float").unwrap();
    let pose = feathrise_scene_content::TerrainObjectTransform {
        translation: [80.0, -15.0, 40.0], yaw_degrees: 37.0, scale: [2.0, 3.0, 0.5],
        ..Default::default()
    };
    let collider = avian3d::prelude::Collider::cuboid(20.0, 2000.25, 30.0);
    let ground = TerrainGroundSnapshot::test_mesh(map, id.clone(), pose, collider);
    let (min, max) = ground.local_bounds(&id).unwrap();
    assert!(min.y <= -1000.125 && max.y >= 1000.125);
    let next = feathrise_scene_content::TerrainObjectTransform {
        translation: [120.0, 8.0, -50.0], yaw_degrees: -65.0, scale: [0.5, 1.5, 2.0],
        ..Default::default()
    };
    // 选择高亮和聚焦只转换八个缓存角点；新姿态不能把实际地形范围钳回旧作者量程。
    let matrix = next.as_transform().to_matrix();
    let low = matrix.transform_point3(Vec3::new(0.0, min.y, 0.0));
    let high = matrix.transform_point3(Vec3::new(0.0, max.y, 0.0));
    assert!(low.y <= 8.0 - 1000.125 * 1.5 && high.y >= 8.0 + 1000.125 * 1.5);
    assert!(ground.local_bounds(&feathrise_scene_content::TerrainId::parse("absent").unwrap()).is_none());
}

#[test]
fn legacy_heightfield_hit_survives_object_cursor_and_reaches_sculpt_brush() {
    let draft = test_heightfield_draft(Arc::new(vec![2; 121]));
    let mesh = draft.terrain_mesh.as_ref().unwrap();
    let hit = draft.terrain_editing_index.as_ref().unwrap().ray_intersection(
        &mesh.positions, &mesh.indices, [5.0, 20.0, 5.0], [0.0, -1.0, 0.0],
    ).unwrap(); // 使用真实作者三角命中，验证多对象系统不会截断后续雕刻。
    let point = Some(Vec3::from_array(hit.position));
    let mut app = App::new();
    let mut time = Time::<Real>::default();
    time.advance_by(Duration::from_millis(100));
    app.insert_resource(time)
        .insert_resource(SceneEditorState {
            open: true, selected_scene_id: Some(draft.scene_id.clone()),
            tool: crate::types::EditorTool::Raise,
            cursor_world: point, cursor_triangle: Some(hit.triangle),
            brush_cursor_world: point, brush_cursor_triangle: Some(hit.triangle),
            ..default()
        })
        .insert_resource(EditorAuthoringSession { draft: Some(draft), ..default() })
        .init_resource::<TerrainGroundCursor>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<crate::authoring::resources::TerrainBrushStroke>()
        .init_resource::<crate::authoring::resources::WaterMaskMaterializeTask>();
    app.world_mut().spawn((crate::components::SceneEditorViewport, RelativeCursorPosition {
        cursor_over: true, normalized: Some(Vec2::ZERO),
    }));
    app.world_mut().resource_mut::<ButtonInput<MouseButton>>().press(MouseButton::Left);
    app.world_mut().run_system_once(systems::cursor).unwrap();
    let editor = app.world().resource::<SceneEditorState>();
    assert_eq!(editor.cursor_world, point);
    assert_eq!(editor.brush_cursor_triangle, Some(hit.triangle));
    app.world_mut().run_system_once(crate::authoring::terrain::apply_terrain_brush).unwrap();
    let draft = app.world().resource::<EditorAuthoringSession>().draft.as_ref().unwrap();
    assert!(draft.terrain_mesh.as_ref().unwrap().positions[60][1] > 2.0);

    // 切到显式空容器后，本系统重新拥有落点；缺少真实表面时必须清空，不能保留旧GLB证据。
    app.world_mut().resource_mut::<EditorAuthoringSession>().draft.as_mut().unwrap()
        .manifest.terrains = Some(Arc::from([]));
    app.world_mut().run_system_once(systems::cursor).unwrap();
    let editor = app.world().resource::<SceneEditorState>();
    assert!(editor.cursor_world.is_none());
    assert!(editor.brush_cursor_world.is_none());
    assert!(editor.brush_cursor_triangle.is_none());
}
