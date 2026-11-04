// 地表与密度笔刷的纯算法回归测试。
use super::*;

#[test]
fn transformed_surface_and_vegetation_pixels_keep_the_world_circle_on_a_source_canvas() {
    use crate::authoring::terrain_documents::{PaintMetricSpace, TerrainDocumentIdentity};
    use std::sync::Arc;
    let mut draft = crate::authoring::test_heightfield_draft(Arc::new(vec![0; 121]));
    let mut object = draft.manifest.terrain_objects_for_edit()[0].clone();
    object.transform = feathrise_map_content::TerrainObjectTransform {
        translation: [20.0, 6.0, -30.0],
        yaw_degrees: 37.0,
        scale: [2.0, 3.0, 0.5],
        ..Default::default()
    };
    let matrix = object.transform.as_transform().to_matrix();
    draft.terrain_documents.active = Some(TerrainDocumentIdentity {
        definition: Arc::new(object),
        manifest: draft.manifest.clone(),
        folder: draft.folder.clone(),
    });
    let metric = PaintMetricSpace::from_draft(&draft);
    let center = Vec3::new(5.0, 0.0, 5.0);
    let mut density = vec![0; 121];
    let mut indices = vec![0; 121 * 16];
    let mut weights = [255, 0, 0, 0].repeat(121);
    let mut density_before = BTreeMap::new();
    let mut index_before = BTreeMap::new();
    let mut weight_before = BTreeMap::new();
    let min = metric.bounds(draft.terrain_manifest().world_min);
    let max = metric.bounds(draft.terrain_manifest().world_max);
    assert!(paint_density_neighborhood_with_flow(
        &mut density,
        [11, 11],
        min,
        max,
        metric.point(center),
        2.0,
        false,
        1.0,
        0.0,
        &mut density_before
    ));
    assert!(paint_surface_neighborhood(
        &mut indices,
        &mut weights,
        [11, 11],
        min,
        max,
        metric.point(center),
        2.0,
        1,
        2,
        false,
        1.0,
        &mut index_before,
        &mut weight_before
    ));
    for z in 0..11 {
        for x in 0..11 {
            let distance = matrix
                .transform_point3(Vec3::new(x as f32, 0.0, z as f32))
                .distance(matrix.transform_point3(center));
            let expected = ((1.0 - distance / 2.0).clamp(0.0, 1.0) * 255.0).round() as u8;
            let texel = z * 11 + x;
            assert!(
                density[texel].abs_diff(expected) <= 1,
                "世界圆刷密度 ({x},{z})"
            );
            let painted = (0..4)
                .find_map(|slot| {
                    let start = texel * 16 + slot * 4;
                    (u32::from_le_bytes(indices[start..start + 4].try_into().unwrap()) == 1)
                        .then_some(weights[texel * 4 + slot])
                })
                .unwrap_or(0);
            assert!(painted.abs_diff(expected) <= 1, "世界圆刷地表 ({x},{z})");
        }
    }
}

#[test]
fn ordinary_and_volume_grass_brushes_write_equal_density_bytes() {
    use feathrise_map_terrain::volume::sparse::paged::{
        PagedVolume, VolumeFieldBrush, VolumeFieldChannel, load_field_window, load_surface_fields,
        prepare_field_paint, write_field_paint,
    };
    let root = std::env::temp_dir().join(format!(
        "bevy-grass-density-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let mut volume = PagedVolume::new(0.25).unwrap();
    let mut density = vec![0; 81];
    let mut before = BTreeMap::new();
    for (starting, erase, delta) in [
        (true, false, 1.0 / 60.),
        (false, false, 1.0 / 30.),
        (false, false, 1.0 / 120.),
        (true, true, 1.0 / 60.),
        (false, true, 0.5),
    ] {
        let (amount, flow_exposure) = vegetation_paint_exposure(0.6, delta, starting);
        paint_density_neighborhood_with_flow(
            &mut density,
            [9, 9],
            [-1., -1.],
            [1., 1.],
            Vec3::ZERO,
            0.9,
            erase,
            amount,
            flow_exposure,
            &mut before,
        );
        let brush = VolumeFieldBrush {
            channel: VolumeFieldChannel::Vegetation,
            id: "grass".into(),
            from: [0.; 3],
            to: [0.; 3],
            normal: [0., 1., 0.],
            radius: 0.9,
            strength: amount,
            flow_exposure,
            erase,
            source_to_world: bevy::math::Mat3::IDENTITY,
        };
        let input = load_field_window(&root, &volume, &brush).unwrap();
        let result = prepare_field_paint(input, &volume, &brush, |p, d, max| {
            let t = -p.y / d.y;
            (t >= 0. && t <= max).then_some((p + d * t, Vec3::Y))
        })
        .unwrap();
        volume = write_field_paint(&root, &volume, result).unwrap();
        let fields = load_surface_fields(&root, &volume, &[[0.; 3]]).unwrap();
        for z in 0..9 {
            for x in 0..9 {
                let actual = fields
                    .samples
                    .get(&[x - 4, 0, z - 4])
                    .and_then(|s| s.vegetation.iter().find(|(id, _)| id == "grass"))
                    .map_or(0, |(_, p)| *p);
                assert_eq!(
                    actual,
                    density[(z * 9 + x) as usize],
                    "相同点按/按住/擦除强度必须写相同R8密度：({x},{z})"
                );
            }
        }
    }
    assert_eq!(
        vegetation_paint_exposure(0.6, 0., true),
        (0.6, 0.0),
        "首次点按必须立即留下刷印"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn indexed_surface_paint_keeps_four_weights_exact() {
    let (indices, weights) = paint_indexed_weights([0, 0, 0, 0], [255, 0, 0, 0], 3, 8, false, 0.5);
    assert_eq!(weights.iter().map(|value| *value as u16).sum::<u16>(), 255);
    assert!(
        indices
            .into_iter()
            .zip(weights)
            .any(|(id, weight)| id == 3 && weight > 0)
    );
}

#[test]
fn fifth_material_stably_replaces_the_smallest_local_contributor() {
    let (indices, weights) =
        paint_indexed_weights([8, 2, 5, 1], [100, 80, 60, 15], 7, 10, false, 0.5);
    assert_eq!(weights.iter().map(|value| *value as u16).sum::<u16>(), 255);
    assert!(indices.contains(&7));
    assert!(!indices.contains(&1));
}

#[test]
fn erasing_surface_redistributes_weight_to_existing_layers() {
    let (indices, weights) = paint_indexed_weights([3, 0, 0, 0], [200, 55, 0, 0], 3, 4, true, 0.5);
    let selected = indices
        .iter()
        .zip(weights)
        .find_map(|(id, weight)| (*id == 3).then_some(weight))
        .unwrap_or_default();
    let fallback = indices
        .iter()
        .zip(weights)
        .find_map(|(id, weight)| (*id == 0).then_some(weight))
        .unwrap_or_default();
    assert!(selected < 200);
    assert!(fallback > 55);
}
#[test]
fn fast_drag_interpolates_dabs_but_keeps_per_frame_work_bounded() {
    assert_eq!(interpolated_dab_count(Vec3::ZERO, Vec3::ZERO, 10.0), 1);
    assert!(interpolated_dab_count(Vec3::ZERO, Vec3::X * 20.0, 10.0) > 1);
    let extreme = interpolated_dab_count(Vec3::ZERO, Vec3::X * 100_000.0, 2.0);
    assert_eq!(extreme, MAX_INTERPOLATED_DABS_PER_FRAME);
}

#[test]
fn a_five_centimeter_brush_paints_a_continuous_line_instead_of_separated_spots() {
    let resolution = [1001, 3];
    let mut indices = vec![0; 1001 * 3 * 16];
    let mut weights = vec![0; 1001 * 3 * 4];
    let mut before_indices = BTreeMap::new();
    let mut before_weights = BTreeMap::new();
    let from = Vec3::new(0.1, 0.0, 0.0);
    let to = Vec3::new(0.9, 0.0, 0.0);
    let radius = 0.05;
    let count = brush_path_dab_count(from, to, radius);
    assert!(count > MAX_INTERPOLATED_DABS_PER_FRAME as usize);
    assert_eq!(interpolated_dab_count(from, to, radius), MAX_INTERPOLATED_DABS_PER_FRAME);
    for dab in 1..=count {
        paint_surface_neighborhood(&mut indices, &mut weights, resolution, [0.0, -0.1], [1.0, 0.1],
            from.lerp(to, dab as f32 / count as f32), radius, 1, 2, false, 0.5,
            &mut before_indices, &mut before_weights);
    }
    for x in 100..=900 {
        let texel = 1001 + x;
        assert!(weights[texel * 4..texel * 4 + 4].iter().any(|weight| *weight > 0),
            "5厘米刷子的已行走范围第{x}列不能因0.25米补点下限断开");
    }
}

#[test]
fn circle_rectangle_overlap_accepts_edges_and_rejects_outside_corners() {
    let world_min = [0.0, 0.0]; // 测试地图左上角。
    let world_max = [10.0, 10.0]; // 测试地图右下角。
    assert!(brush_overlaps_world_bounds(
        Vec3::new(-2.0, 99.0, 5.0),
        2.0,
        world_min,
        world_max,
    )); // 圆心在左边界外且圆周相切时仍属于合法边缘笔刷。
    assert!(brush_overlaps_world_bounds(
        Vec3::new(-1.0, 0.0, -1.0),
        1.5,
        world_min,
        world_max,
    )); // 角部使用欧氏最近距离，不能按扩张矩形误判。
    assert!(!brush_overlaps_world_bounds(
        Vec3::new(-1.0, 0.0, -1.0),
        1.4,
        world_min,
        world_max,
    )); // 半径小于到最近角距离时必须完全拒绝。
}

#[test]
fn outside_density_brush_clips_to_edge_and_preserves_subcell_center() {
    let resolution = [11, 11]; // 十米地图形成一米纹素间距。
    let mut density = vec![0; 11 * 11]; // 空白R8可直接观察本次修改范围。
    let mut before = BTreeMap::new(); // 收集边缘笔划首次值。
    assert!(paint_density_neighborhood(
        &mut density,
        resolution,
        [0.0, 0.0],
        [10.0, 10.0],
        Vec3::new(-0.5, 0.0, 5.0),
        1.0,
        false,
        1.0,
        &mut before,
    )); // 圆心越过左边界半米后，圆内的边缘纹素仍可绘制。
    assert!(density[5 * 11] > 0); // 最近的左边界纹素获得真实半米距离对应的柔边强度。
    assert_eq!(density[5 * 11 + 1], 0); // 距真实圆心一点五米的第二列严格保持空白。

    let mut subcell_density = vec![0; 11 * 11]; // 第二张R8用于验证圆心不再吸附到整数格。
    let mut subcell_before = BTreeMap::new(); // 收集亚格笔划差分。
    assert!(paint_density_neighborhood(
        &mut subcell_density,
        resolution,
        [0.0, 0.0],
        [10.0, 10.0],
        Vec3::new(0.49, 0.0, 5.0),
        1.0,
        false,
        1.0,
        &mut subcell_before,
    )); // 圆心位于两列之间时必须保留真实小数位置。
    assert!(subcell_density[5 * 11 + 1] > 0); // 距圆心0.51米的第二列不能被旧式四舍五入误当成圆周零强度。
}

#[test]
fn outside_surface_brush_updates_only_overlapping_rgba_edge() {
    let resolution = [3, 3]; // 两米地图形成一米地表纹素间距。
    let mut weights = [255, 0, 0, 0].repeat(3 * 3); // 初始全部权重属于地图局部第零层。
    let mut indices = vec![0; 3 * 3 * 16]; // 四个RGBA32_UINT槽初始都引用第零层。
    let mut before_weights = BTreeMap::new(); // 收集RGBA8权重首次值。
    let mut before_indices = BTreeMap::new(); // 收集RGBA32_UINT索引首次值。
    assert!(paint_surface_neighborhood(
        &mut indices,
        &mut weights,
        resolution,
        [0.0, 0.0],
        [2.0, 2.0],
        Vec3::new(-0.5, 0.0, 1.0),
        1.0,
        1,
        2,
        false,
        1.0,
        &mut before_indices,
        &mut before_weights,
    )); // 越界圆心仍把地图内相交部分绘入第二个地图局部层。
    let edge_indices = (0..4)
        .map(|slot| {
            let start = 3 * 16 + slot * 4;
            u32::from_le_bytes(indices[start..start + 4].try_into().unwrap())
        })
        .collect::<Vec<_>>();
    assert!(edge_indices.contains(&1)); // 左边界中点纹素登记了新材质的局部索引。
    assert!(
        weights[(4 * 4)..(4 * 4 + 4)]
            .iter()
            .copied()
            .eq([255, 0, 0, 0])
    ); // 圆外相邻纹素保持第零层全权重。
}

#[test]
fn coarse_density_grid_never_expands_the_visible_brush_radius() {
    let resolution = [3, 3]; // 十六米地图形成八米纹素间距，专门暴露旧式“至少一格宽”扩笔。
    let mut density = vec![0; 3 * 3]; // 空白R8便于逐样本核对真实作用范围。
    let mut before = BTreeMap::new(); // 只应记录可见圆内的左边界样本。
    assert!(paint_density_neighborhood(
        &mut density,
        resolution,
        [0.0, 0.0],
        [16.0, 16.0],
        Vec3::new(1.0, 0.0, 8.0),
        2.0,
        false,
        1.0,
        &mut before,
    )); // 半径两米的圆只覆盖距圆心一米的左边界样本。
    assert!(density[3] > 0); // 世界(0,8)位于可见圆内，必须留下刷印。
    assert_eq!(density[4], 0); // 世界(8,8)距圆心七米，不能再被网格间距暗中扩大的半径修改。
    assert_eq!(before.keys().copied().collect::<Vec<_>>(), vec![3]); // 撤销范围也必须与显示圆完全一致。
}

#[test]
fn fully_outside_density_brush_is_a_strict_noop() {
    let mut density = vec![0; 11 * 11]; // 空白R8用于验证完全无交集路径。
    let mut before = BTreeMap::new(); // 无修改时撤销差分必须保持为空。
    assert!(!paint_density_neighborhood(
        &mut density,
        [11, 11],
        [0.0, 0.0],
        [10.0, 10.0],
        Vec3::new(-2.1, 0.0, 5.0),
        1.0,
        false,
        1.0,
        &mut before,
    )); // 圆与地图间仍有空隙时不得访问或修改任何纹素。
    assert!(density.iter().all(|value| *value == 0)); // 作者源逐字节保持原样。
    assert!(before.is_empty()); // 空笔划不产生撤销记录。
}
#[test] // 清空后重新绘制和擦除不得在裸地区自动恢复第一层。
fn empty_surface_brush_uses_selected_layer_and_erase_keeps_blank() {
    assert_eq!(
        super::paint_indexed_weights([0; 4], [0; 4], 1, 2, true, 1.0),
        ([0; 4], [0; 4])
    ); // 空白擦除无副作用。
    assert_eq!(
        super::paint_indexed_weights([0; 4], [0; 4], 1, 2, false, 0.0),
        ([0; 4], [0; 4])
    ); // 笔刷边缘零强度保持裸地。
    assert_eq!(
        super::paint_indexed_weights([0; 4], [0; 4], 1, 2, false, 0.5),
        ([1, 0, 0, 0], [255, 0, 0, 0])
    ); // 首次有效贡献来自所选层。
} // 空画布笔刷回归结束。
