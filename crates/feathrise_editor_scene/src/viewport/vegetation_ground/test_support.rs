//! 给实际外部消费者的点击所有权回归构造三个固定阶段；不暴露查询槽或增加生产入口。

use super::*;

pub fn seeded_click(
    map: SceneId,
    target: GroundCursorTarget,
    revision: u64,
    stage: usize,
) -> VegetationGroundCursor {
    assert!(stage < 3);
    let context = GroundCursorContext {
        scene_id: map,
        target: Some(target),
        placement_snap: None,
        terrain_revision: 11,
        missing_pages: [UVec4::ZERO; 16],
        surface_revision: [1; 3],
        camera_transform: [0; 16],
        view: EditorView::Top,
        orthographic: true,
        extent: 1,
        render_size: UVec2::ONE,
        input_epoch: 1,
        held: true,
        modifiers: 0,
        authoring_revision: Some(revision),
    };
    let mut cursor = VegetationGroundCursor::default();
    cursor.queue_author_click(context, Ray3d::new(Vec3::new(3., 10., 0.), Dir3::NEG_Y));
    let completion = GroundCompletion {
        terrain_mesh: false,
        request: cursor.pending_author_click.clone().unwrap(),
        result: ClipmapGroundHit::Hit(Vec3::new(3., 12., 0.)),
        point: Some(Vec3::new(3., 12., 0.)),
    };
    match stage {
        1 => cursor.pending_author_surface = Some(completion),
        2 => {
            cursor.pending_author_click = None;
            cursor.completed_author_click = Some(completion);
        }
        _ => {}
    }
    cursor
}

pub fn click_retained(cursor: &VegetationGroundCursor, target: &GroundCursorTarget, stage: usize) -> bool {
    let request = match stage {
        0 => cursor.pending_author_click.as_ref(),
        1 => cursor.pending_author_surface.as_ref().map(|hit| &hit.request),
        2 => cursor.completed_author_click.as_ref().map(|hit| &hit.request),
        _ => panic!("未知的点击夹具阶段"),
    };
    request.is_some_and(|request| request.context.target.as_ref() == Some(target))
}
