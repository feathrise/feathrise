//! 实际覆盖列表优先于包围盒，跨空白位置落笔沿用分块层。
use super::*;
#[test]
fn flatten_anchor_survives_pending_segments_and_new_strokes_resample_height() {
    let mut stroke = Stroke {
        spatial: None,
        shape: feathrise_map_terrain::volume::VolumeBrushShape::Box,
        half_height: 1.,
        lock_height: false,
        flatten_height: None,
        field: None,
        tool: EditorTool::VolumeFlatten,
        revision: 0,
        before: None,
        index: 0,
        released: false,
        last: None,
        tail: None,
        first_click_done: false,
        seed: None,
        new_region: false,
        history_slot: None,
        started: 0.,
    };
    let mut first = Vec3::new(1., 17.37, 2.);
    lock_flatten_height(&mut stroke, &mut first);
    for y in [4., -12., 28.] {
        let mut later = Vec3::new(8., y, 9.);
        lock_flatten_height(&mut stroke, &mut later);
        assert_eq!(later, Vec3::new(8., 17.37, 9.));
        assert_eq!(stroke.flatten_height, Some(17.37));
    }
    stroke.flatten_height = None; // 新按压建立新Stroke，不能沿用上一笔高度。
    let mut next = Vec3::new(8., 4.5, 9.);
    lock_flatten_height(&mut stroke, &mut next);
    assert_eq!(stroke.flatten_height, Some(4.5));
    stroke.tool = EditorTool::VolumeDig;
    let mut dig = Vec3::new(8., -3., 9.);
    lock_flatten_height(&mut stroke, &mut dig);
    assert_eq!(dig.y, -3., "挖掘必须仍能向内深入");
    stroke.lock_height = true;
    stroke.flatten_height = None;
    lock_flatten_height(&mut stroke, &mut dig);
    let mut later = Vec3::new(12., -8., 15.);
    lock_flatten_height(&mut stroke, &mut later);
    assert_eq!(
        later,
        Vec3::new(12., -3., 15.),
        "固定高度开挖不跟随新洞底下沉"
    );
}

#[test]
fn volume_layer_routing_uses_actual_coverage_and_extends_the_same_author_layer() {
    let bounds = Rect::from_corners(Vec2::splat(-256.), Vec2::splat(256.));
    let sparse: MapPlacementRecord = serde_json::from_value(serde_json::json!({
        "id":"sparse-layer","asset_path":"maps/volume/v.glb","translation":[0,0,0],"yaw_degrees":0,"scale":[1,1,1],
        "terrain_patch":{"sculpted_volume":true,"cutouts":[{"min":[-8,-8],"max":[8,8]},{"min":[72,-8],"max":[88,8]}]}
    })).unwrap();
    let external: MapPlacementRecord = serde_json::from_value(serde_json::json!({
        "id":"external","asset_path":"maps/other.glb","translation":[0,0,0],"yaw_degrees":0,"scale":[1,1,1],
        "terrain_patch":{"cutout":{"min":[32,-8],"max":[48,8]}}
    })).unwrap();
    let index = RecordIndex {
        ready: true,
        records: vec![
            (0, sparse.clone(), sparse.terrain_cutout(bounds).unwrap()),
            (
                1,
                external.clone(),
                external.terrain_cutout(bounds).unwrap(),
            ),
        ],
        ..default()
    };
    assert_eq!(
        choose_layer(&index, Vec2::new(40., 0.), bounds)
            .unwrap()
            .1
            .id,
        "external",
        "断开列的包围盒不能抢走中间外部GLB的作者身份"
    );
    assert_eq!(
        choose_layer(&index, Vec2::new(120., 0.), bounds)
            .unwrap()
            .1
            .id,
        "sparse-layer",
        "无覆盖的新落点继续扩展原层"
    );
    assert_eq!(
        choose_layer(&index, Vec2::new(80., 0.), bounds)
            .unwrap()
            .1
            .id,
        "sparse-layer"
    );
}
