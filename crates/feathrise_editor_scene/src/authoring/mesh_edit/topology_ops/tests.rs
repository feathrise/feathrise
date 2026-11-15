//! 用正式几何校验和闭壳不变量验证五种拓扑结果，额外核对世界米制宽度与作者元数据保留。

use super::super::{selection::MeshSelection, types::MeshElementKind};
use super::*;
use bevy::{math::DVec3, prelude::*};
use feathrise_scene_content::{EditableMeshGeometry, ScenePlacementAssetTarget, ScenePlacementRecord};
use std::collections::{BTreeMap, BTreeSet};

fn record() -> ScenePlacementRecord {
    serde_json::from_value(serde_json::json!({
        "id":"topology-test", "asset_path":"",
        "asset_target":ScenePlacementAssetTarget::DEFAULT_CUBOID,
        "translation":[0,0,0], "yaw_degrees":0,"scale":[1,1,1],
        "editor":{"name":"拓扑事务测试"}
    }))
    .unwrap()
}

fn geometry(record: &ScenePlacementRecord) -> EditableMeshGeometry {
    record.asset_target.editable_geometry().unwrap()
}

fn edge_selection(record: &ScenePlacementRecord, a: usize, b: usize) -> u128 {
    let index = geometry(record)
        .edges()
        .iter()
        .position(|&[start, end]| mesh::edge(start, end) == mesh::edge(a, b))
        .unwrap();
    1_u128 << index
}

fn assert_record_preserved(before: &ScenePlacementRecord, after: &ScenePlacementRecord) {
    let mut restored = after.clone();
    restored.asset_target = before.asset_target.clone();
    assert_eq!(
        &restored, before,
        "拓扑只能替换几何来源，不能改变对象身份、姿态或元数据"
    );
    let before_material = match before.asset_target {
        ScenePlacementAssetTarget::Cuboid {
            color_srgb,
            roughness_milli,
            collision,
            ..
        }
        | ScenePlacementAssetTarget::EditableCuboid {
            color_srgb,
            roughness_milli,
            collision,
            ..
        }
        | ScenePlacementAssetTarget::EditableMesh {
            color_srgb,
            roughness_milli,
            collision,
            ..
        } => (color_srgb, roughness_milli, collision),
        _ => unreachable!(),
    };
    let ScenePlacementAssetTarget::EditableMesh {
        color_srgb,
        roughness_milli,
        collision,
        ..
    } = after.asset_target
    else {
        panic!("实际拓扑变化必须保存一般网格");
    };
    assert_eq!((color_srgb, roughness_milli, collision), before_material);
}

fn assert_closed(record: &ScenePlacementRecord) -> (usize, usize, usize) {
    record.asset_target.validate().unwrap();
    let geometry = geometry(record);
    let mut edges = BTreeMap::<[u32; 2], Vec<[u32; 2]>>::new();
    for face in geometry.faces() {
        let vertices = face.indices();
        for i in 0..vertices.len() {
            let a = vertices[i];
            let b = vertices[(i + 1) % vertices.len()];
            let key = if a < b { [a, b] } else { [b, a] };
            edges.entry(key).or_default().push([a, b]);
        }
    }
    for uses in edges.values() {
        assert_eq!(uses.len(), 2, "每条真实边必须恰好属于两个面");
        assert_eq!(uses[0], [uses[1][1], uses[1][0]], "共享边绕序必须相反");
    }
    assert_eq!(
        geometry.vertices().len() as isize - edges.len() as isize + geometry.faces().len() as isize,
        2
    );
    (
        geometry.vertices().len(),
        edges.len(),
        geometry.faces().len(),
    )
}

fn volume(record: &ScenePlacementRecord) -> f64 {
    let geometry = geometry(record);
    let points = geometry
        .vertices()
        .iter()
        .map(|p| DVec3::from_array(p.map(f64::from)) * 0.0005)
        .collect::<Vec<_>>();
    geometry
        .triangles()
        .iter()
        .map(|&[a, b, c]| points[a].dot(points[b].cross(points[c])) / 6.0)
        .sum::<f64>()
        * record.scale.iter().map(|&v| f64::from(v)).product::<f64>()
}

#[test]
fn inset_creates_a_true_equal_width_ring_and_keeps_cap_index() {
    let before = record();
    let after = inset_record(&before, MeshElementKind::Face, &MeshSelection::from(1 << 3), 0.25).unwrap();
    assert_eq!(assert_closed(&after), (12, 20, 10));
    assert_record_preserved(&before, &after);
    let work = mesh::MeshWork::from_record(&after).unwrap();
    let inner = &work.faces[3];
    assert_eq!(inner.len(), 4);
    for &vertex in inner {
        assert!((work.points[vertex].y - 1.0).abs() < 1e-6);
        assert!((work.points[vertex].x.abs() - 0.75).abs() < 1e-6);
        assert!((work.points[vertex].z.abs() - 0.75).abs() < 1e-6);
    }
    assert!((volume(&after) - volume(&before)).abs() < 1e-6);
    assert!(inset_record(&before, MeshElementKind::Face, &MeshSelection::from(1 << 3), 1.1).is_err());
}

#[test]
fn inset_width_is_world_meters_under_rotated_nonuniform_object_scale() {
    let mut before = record();
    before.translation = [15.0, 7.0, -13.0];
    before.yaw_degrees = 37.0;
    before.scale = [2.0, 3.0, 4.0];
    let old = mesh::MeshWork::from_record(&before).unwrap();
    let after = inset_record(&before, MeshElementKind::Face, &MeshSelection::from(1 << 3), 0.25).unwrap();
    let new = mesh::MeshWork::from_record(&after).unwrap();
    let normal = mesh::planar_normal(&old.faces[3], &old.points).unwrap();
    for (i, &point) in new.faces[3].iter().enumerate() {
        let a = old.points[old.faces[3][i]];
        let b = old.points[old.faces[3][(i + 1) % 4]];
        let inward = normal.cross((b - a).normalize());
        assert!((inward.dot(new.points[point] - a) - 0.25).abs() < 0.003);
    }
    assert_closed(&after);
    assert_record_preserved(&before, &after);
}

#[test]
fn bevel_replaces_the_selected_edge_with_a_real_strip_and_trims_corner_faces() {
    let before = record();
    let after = bevel_record(
        &before,
        MeshElementKind::Edge,
        &MeshSelection::from(edge_selection(&before, 0, 1)),
        0.25,
    )
    .unwrap();
    assert_eq!(assert_closed(&after), (10, 15, 7));
    assert_record_preserved(&before, &after);
    assert!(volume(&after) < volume(&before));
    let mesh = geometry(&after);
    assert_eq!(
        mesh.faces()
            .iter()
            .filter(|face| face.indices().len() == 5)
            .count(),
        2
    );
    assert_eq!(mesh.faces().last().unwrap().indices().len(), 4);
    let old = geometry(&before);
    let new_points = mesh.vertices().iter().copied().collect::<BTreeSet<_>>();
    assert!(!new_points.contains(&old.vertices()[0]));
    assert!(!new_points.contains(&old.vertices()[1]));
    let touching = edge_selection(&before, 0, 1) | edge_selection(&before, 0, 2);
    assert!(bevel_record(&before, MeshElementKind::Edge, &MeshSelection::from(touching), 0.25).is_err());
    assert!(
        bevel_record(
            &before,
            MeshElementKind::Edge,
            &MeshSelection::from(edge_selection(&before, 0, 1)),
            2.1
        )
        .is_err()
    );
}

#[test]
fn loop_cut_propagates_through_all_four_quads_and_slides_consistently() {
    let before = record();
    let selected = edge_selection(&before, 0, 1);
    for factor in [0.25, 0.5, 0.75] {
        let after = loop_cut_record(&before, MeshElementKind::Edge, &MeshSelection::from(selected), factor).unwrap();
        assert_eq!(assert_closed(&after), (12, 20, 10));
        assert_record_preserved(&before, &after);
        assert!((volume(&after) - volume(&before)).abs() < 1e-6);
        let geometry = geometry(&after);
        let created = &geometry.vertices()[8..];
        assert_eq!(created.len(), 4);
        for point in created {
            assert_eq!(point[0], (-2000.0 + 4000.0 * factor).round() as i32);
        }
    }
}

#[test]
fn plane_knife_preserves_both_sides_and_reuses_all_intersections() {
    let before = record();
    let after = knife_record(&before, Vec3::ZERO, Vec3::X).unwrap();
    assert_closed(&after);
    assert_record_preserved(&before, &after);
    assert!((volume(&after) - volume(&before)).abs() < 1e-6);
    let geometry = geometry(&after);
    assert!(geometry.vertices().iter().any(|p| p[0] < 0));
    assert!(geometry.vertices().iter().any(|p| p[0] > 0));
    assert!(geometry.vertices().iter().filter(|p| p[0] == 0).count() >= 4);
    for face in geometry.faces() {
        let sides = face
            .indices()
            .iter()
            .map(|&v| geometry.vertices()[v as usize][0])
            .collect::<Vec<_>>();
        assert!(!sides.iter().any(|&v| v < 0) || !sides.iter().any(|&v| v > 0));
    }
    assert_eq!(
        knife_record(&before, Vec3::X * 3.0, Vec3::X).unwrap(),
        before
    );
    assert!(knife_record(&before, Vec3::ZERO, Vec3::ZERO).is_err());
}

#[test]
fn knife_uses_the_world_plane_under_rotated_scaled_instance_transform() {
    let mut before = record();
    before.translation = [12.0, 4.0, -7.0];
    before.yaw_degrees = 37.0;
    before.scale = [2.0, 3.0, 4.0];
    let normal = Quat::from_rotation_y(before.yaw_degrees.to_radians()) * Vec3::X;
    let after = knife_record(&before, Vec3::from_array(before.translation), normal).unwrap();
    assert_closed(&after);
    assert_record_preserved(&before, &after);
    assert!((volume(&after) - volume(&before)).abs() < 0.01);
    let work = mesh::MeshWork::from_record(&after).unwrap();
    let origin = Vec3::from_array(before.translation).as_dvec3();
    assert!(
        work.points
            .iter()
            .filter(|p| normal.as_dvec3().dot(**p - origin).abs() < 0.002)
            .count()
            >= 4
    );
}

#[test]
fn subdivide_all_faces_creates_shared_midpoints_and_twenty_four_real_quads() {
    let before = record();
    let after = subdivide_record(&before, MeshElementKind::Face, &MeshSelection::from((1 << 6) - 1)).unwrap();
    assert_eq!(assert_closed(&after), (26, 48, 24));
    assert_record_preserved(&before, &after);
    assert!(
        geometry(&after)
            .faces()
            .iter()
            .all(|face| face.indices().len() == 4)
    );
    assert!((volume(&after) - volume(&before)).abs() < 1e-6);
}

fn concave_record() -> ScenePlacementRecord {
    // 复用可见面选择回归的合法L形棱柱：盖面的算术平均恰好落在原凹角顶点，
    // 不能以一个新中心连接整个凹边界，否则会制造重合点或跨越凹口的子面。
    let boundary = [
        [-2000, -2000], [-2000, 2000], [2000, 2000],
        [2000, 0], [0, 0], [0, -2000],
    ];
    let mut vertices = Vec::new();
    for y in [-2000, 2000] {
        vertices.extend(boundary.iter().map(|&[x, z]| [x, y, z]));
    }
    let mut faces = vec![
        feathrise_scene_content::MeshFace::new(&[5, 4, 3, 2, 1, 0]).unwrap(),
        feathrise_scene_content::MeshFace::new(&[6, 7, 8, 9, 10, 11]).unwrap(),
    ];
    for side in 0..6 {
        let next = (side + 1) % 6;
        faces.push(feathrise_scene_content::MeshFace::new(&[side, next, next + 6, side + 6]).unwrap());
    }
    let mut before = record();
    before.asset_target = ScenePlacementAssetTarget::EditableMesh {
        geometry: EditableMeshGeometry::from_parts(&vertices, &faces).unwrap(),
        color_srgb: [21, 34, 55],
        roughness_milli: 321,
        collision: false,
    };
    before
}

#[test]
fn concave_face_subdivision_preserves_the_original_surface_and_shared_closed_shell() {
    let before = concave_record();
    let original = mesh::MeshWork::from_record(&before).unwrap();
    let source = geometry(&before);
    let triangles = source.triangles();
    for selected in [1, 1 << 1, (1 << 8) - 1] {
        let after = subdivide_record(&before, MeshElementKind::Face, &MeshSelection::from(selected))
            .expect("合法凹面和整壳都应能细分，不能因平均中心落在凹角而拒绝");
        assert_closed(&after);
        assert_record_preserved(&before, &after);
        assert!((volume(&after) - volume(&before)).abs() < 1e-6);
        let subdivided = mesh::MeshWork::from_record(&after).unwrap();
        for point in &subdivided.points {
            assert!(triangles.iter().any(|triangle| {
                let [a, b, c] = triangle.map(|index| original.points[index]);
                let normal = (b - a).cross(c - a);
                normal.dot(*point - a).abs() < 1e-7
                    && [(a, b), (b, c), (c, a)].iter().all(|&(start, end)| {
                        normal.dot((end - start).cross(*point - start)) >= -1e-7
                    })
            }), "线性细分不能让新点离开原正式三角表面：{point:?}");
        }
    }
}

#[test]
fn concave_subdivision_does_not_depend_on_instance_scale_or_world_area() {
    let mut before = concave_record();
    let ScenePlacementAssetTarget::EditableMesh { geometry: authored_geometry, .. } = &mut before.asset_target else {
        unreachable!();
    };
    let vertices = authored_geometry.vertices().iter()
        .map(|vertex| vertex.map(|coordinate| coordinate / 100)).collect();
    *authored_geometry = authored_geometry.with_vertices(vertices).unwrap();
    before.asset_target.validate().unwrap();
    let expected = subdivide_record(&before, MeshElementKind::Face, &MeshSelection::from((1 << 8) - 1)).unwrap();
    // 半毫米作者坐标仍有足够细分间距；合法最小实例scale不能把凹角错判为凸角。
    before.scale = [0.001; 3];
    before.yaw_degrees = 37.0;
    before.translation = [10.0, 3.0, -7.0];
    let after = subdivide_record(&before, MeshElementKind::Face, &MeshSelection::from((1 << 8) - 1))
        .expect("凹性由作者拓扑决定，不能依赖世界面积的绝对阈值");
    assert_closed(&after);
    assert_record_preserved(&before, &after);
    assert_eq!(geometry(&after), geometry(&expected));
}

#[test]
fn partial_face_and_edge_subdivision_repair_neighbors_without_t_junctions() {
    let before = record();
    for (kind, selected, counts) in [
        (MeshElementKind::Face, 1 << 3, (13, 20, 9)),
        (MeshElementKind::Edge, edge_selection(&before, 0, 1), (9, 13, 6)),
        (MeshElementKind::Vertex, (1 << 0) | (1 << 1), (9, 13, 6)),
    ] {
        let after = subdivide_record(&before, kind, &MeshSelection::from(selected)).unwrap();
        assert_eq!(assert_closed(&after), counts, "邻面补点不能把内部渲染三角变成作者面");
        assert_record_preserved(&before, &after);
        let result = geometry(&after);
        assert!(result.faces().iter().all(|face| face.indices().len() >= 4));
        assert_eq!(result.triangles().len(), counts.0 * 2 - 4);
        assert!((volume(&after) - volume(&before)).abs() < 1e-6);
    }
}

#[test]
fn subdivision_uses_the_same_grid_for_all_modes_and_uniform_multiple_cuts() {
    let before = record();
    let original = geometry(&before);
    let options = SubdivisionOptions { cuts: 2, ..Default::default() };
    let face_mode = subdivide_record_with_options(&before, MeshElementKind::Face, &MeshSelection::from(0x3f), options).unwrap();
    assert_eq!(assert_closed(&face_mode.record), (56, 108, 54));
    assert_eq!(face_mode.selected.len(), 54);
    assert_record_preserved(&before, &face_mode.record);
    let result = geometry(&face_mode.record);
    assert!(result.faces().iter().all(|face| face.indices().len() == 4));
    for &[a, b] in original.edges() {
        for fraction in [1.0 / 3.0, 2.0 / 3.0] {
            let cut = std::array::from_fn(|axis| {
                let a = f64::from(original.vertices()[a][axis]);
                let b = f64::from(original.vertices()[b][axis]);
                (a + (b - a) * fraction).round() as i32
            });
            assert!(result.vertices().contains(&cut), "两刀应均匀分割成三段");
        }
    }
    for (kind, selected, expected) in [(MeshElementKind::Edge, 0xfff, 108), (MeshElementKind::Vertex, 0xff, 56)] {
        let outcome = subdivide_record_with_options(&before, kind, &MeshSelection::from(selected), options).unwrap();
        assert_eq!(outcome.record, face_mode.record);
        assert_eq!(outcome.selected.len(), expected);
    }
    for cuts in [0, MAX_SUBDIVISION_CUTS + 1] {
        assert!(subdivide_record_with_options(&before, MeshElementKind::Face, &MeshSelection::from(0x3f),
            SubdivisionOptions { cuts, ..options }).is_err(), "切数超出工具范围");
    }
}

#[test]
fn partial_quad_patterns_match_blender_and_flush_selection_from_new_points() {
    let before = record();
    let [a, b, c, d] = [2, 6, 7, 3];
    let ab = edge_selection(&before, a, b);
    let bc = edge_selection(&before, b, c);
    let cd = edge_selection(&before, c, d);
    for (selection, cuts, counts, selected_edges) in [
        (ab, 1, (9, 13, 6), 2),
        (ab | bc, 1, (10, 15, 7), 5),
        (ab | cd, 1, (10, 15, 7), 7),
        (ab | bc | cd, 1, (11, 18, 9), 10),
        (ab | bc | cd, 2, (14, 22, 10), 14),
        (ab | bc | cd, 3, (17, 27, 12), 19),
    ] {
        let outcome = subdivide_record_with_options(&before, MeshElementKind::Edge, &MeshSelection::from(selection),
            SubdivisionOptions { cuts, ..Default::default() }).unwrap();
        assert_eq!(assert_closed(&outcome.record), counts);
        assert_eq!(outcome.selected.len(), selected_edges);
        assert!((volume(&outcome.record) - volume(&before)).abs() < 1e-6);
    }
}

#[test]
fn corner_modes_keep_authored_faces_and_resolve_collinear_quad_diagonals() {
    let before = record();
    let selection = edge_selection(&before, 2, 6) | edge_selection(&before, 6, 7);
    for (corner, added_vertices, expected_top) in [
        (SubdivisionCorner::StraightCut, 0, vec![3, 5]),
        (SubdivisionCorner::Path, 0, vec![3, 3, 4]),
        (SubdivisionCorner::Fan, 0, vec![3, 3, 4]),
        (SubdivisionCorner::InnerVertex, 1, vec![4, 4, 4]),
    ] {
        let outcome = subdivide_record_with_options(&before, MeshElementKind::Edge, &MeshSelection::from(selection),
            SubdivisionOptions { corner, ..Default::default() }).unwrap();
        assert_closed(&outcome.record);
        let result = geometry(&outcome.record);
        let mut top: Vec<_> = result.faces().iter()
            .filter(|face| face.indices().iter().all(|&index| result.vertices()[index as usize][1] == 2000))
            .map(|face| face.indices().len()).collect();
        top.sort_unstable();
        assert_eq!(top, expected_top, "{corner:?}");
        assert_eq!(result.vertices().len(), 10 + added_vertices);
    }
    let outcome = subdivide_record_with_options(&before, MeshElementKind::Edge, &MeshSelection::from(selection),
        SubdivisionOptions { cuts: 2, create_ngons: false, ..Default::default() }).unwrap();
    let result = geometry(&outcome.record);
    let top: Vec<_> = result.faces().iter()
        .filter(|face| face.indices().iter().all(|&index| result.vertices()[index as usize][1] == 2000))
        .map(|face| face.indices().len()).collect();
    assert_eq!(top, vec![4; 5]);
    assert!(result.faces().iter().all(|face| face.indices().len() <= 4));
}

#[test]
fn triangle_subdivision_connects_two_edges_and_generates_a_uniform_triangle_grid() {
    let mut before = record();
    let vertices = [[0, 0, 0], [6_000, 0, 0], [0, 6_000, 0], [0, 0, 6_000]];
    let faces = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]]
        .map(|face| feathrise_scene_content::MeshFace::new(&face).unwrap());
    before.asset_target = before.asset_target.with_editable_geometry(
        EditableMeshGeometry::from_parts(&vertices, &faces).unwrap(),
    ).unwrap();
    let selection = edge_selection(&before, 0, 1) | edge_selection(&before, 1, 2);
    let options = SubdivisionOptions { cuts: 2, create_ngons: false, ..Default::default() };
    let partial = subdivide_record_with_options(&before, MeshElementKind::Edge, &MeshSelection::from(selection), options).unwrap();
    assert_eq!(assert_closed(&partial.record), (8, 16, 10));
    let result = geometry(&partial.record);
    let mut bottom: Vec<_> = result.faces().iter()
        .filter(|face| face.indices().iter().all(|&vertex| result.vertices()[vertex as usize][2] == 0))
        .map(|face| face.indices().len()).collect();
    bottom.sort_unstable();
    assert_eq!(bottom, vec![3, 4, 4]);

    let full = subdivide_record_with_options(&before, MeshElementKind::Face, &MeshSelection::from(1),
        SubdivisionOptions { create_ngons: true, ..options }).unwrap();
    let result = geometry(&full.record);
    assert_eq!(assert_closed(&full.record), (11, 21, 12));
    assert_eq!(full.selected.len(), 9);
    assert_eq!(result.faces().iter().filter(|face| face.indices().len() == 3).count(), 9);
    assert!(result.vertices().contains(&[2_000, 2_000, 0]), "两刀三角网格应添加内部等距点");
    assert!((volume(&full.record) - volume(&before)).abs() < 1e-6);
}

#[test]
fn ngon_subdivision_connects_safe_chords_and_skips_the_concave_gap() {
    let before = concave_record();
    let a = edge_selection(&before, 6, 7);
    let b = edge_selection(&before, 7, 8);
    // [9→10,10→11] 是L形凹角；简单连接两个中点会沿缺口走到原表面外。
    let unsafe_corner = edge_selection(&before, 9, 10) | edge_selection(&before, 10, 11);
    for (selection, expected_top) in [(a | b, vec![3, 7]), (unsafe_corner, vec![8])] {
        let outcome = subdivide_record_with_options(&before, MeshElementKind::Edge, &MeshSelection::from(selection),
            SubdivisionOptions::default()).unwrap();
        assert_closed(&outcome.record);
        let result = geometry(&outcome.record);
        let mut top: Vec<_> = result.faces().iter()
            .filter(|face| face.indices().iter().all(|&index| result.vertices()[index as usize][1] == 2000))
            .map(|face| face.indices().len()).collect();
        top.sort_unstable();
        assert_eq!(top, expected_top);
        assert!((volume(&outcome.record) - volume(&before)).abs() < 1e-6);
    }
    // 三次切割会把六边形扩到24个点；沿原平面拆分，接缝和体积均保持。
    let wide = subdivide_record_with_options(&before, MeshElementKind::Face, &MeshSelection::from(1),
        SubdivisionOptions { cuts: 3, ..Default::default() }).unwrap();
    assert_closed(&wide.record);
    assert!((volume(&wide.record) - volume(&before)).abs() < 1e-6);
}

#[test]
fn subdividing_a_selected_child_face_grid_preserves_neighbor_polygons() {
    let before = record();
    let once = subdivide_record(&before, MeshElementKind::Face, &MeshSelection::from((1 << 6) - 1)).unwrap();
    let twice = subdivide_record(&once, MeshElementKind::Face, &MeshSelection::from(0x0f)).unwrap();
    assert_eq!(assert_closed(&twice), (42, 76, 36));
    let result = geometry(&twice);
    assert_eq!(result.faces().iter().filter(|face| face.indices().len() == 5).count(), 8);
    assert_eq!(result.faces().iter().filter(|face| face.indices().len() == 4).count(), 28);
    assert_eq!(result.triangles().len(), 80, "闭壳渲染与碰撞仍采用完整三角表面");
}

#[test]
fn partial_subdivision_preserves_an_unselected_warped_quads_original_surface() {
    let mut before = record();
    let mesh = geometry(&before);
    let mut vertices = mesh.vertices().to_vec();
    vertices[7][1] += 600;
    let mesh = mesh.with_vertices(vertices).unwrap();
    before.asset_target = ScenePlacementAssetTarget::EditableMesh {
        geometry: mesh,
        color_srgb: [21, 34, 55],
        roughness_milli: 321,
        collision: true,
    };
    before.asset_target.validate().unwrap();
    let after = subdivide_record(
        &before,
        MeshElementKind::Edge,
        &MeshSelection::from(edge_selection(&before, 2, 3)),
    )
    .unwrap();
    assert_eq!(assert_closed(&after), (9, 14, 7));
    assert_record_preserved(&before, &after);
    assert!((volume(&after) - volume(&before)).abs() < 1e-6);
    let old = mesh::MeshWork::from_record(&before).unwrap();
    let new = mesh::MeshWork::from_record(&after).unwrap();
    // 新点仍在原真实边上；补点顶面保留原有折线2→7，其余面内三角只是派生数据。
    assert_eq!(new.points.len(), old.points.len() + 1);
    assert!(new.points[8].distance((old.points[2] + old.points[3]) * 0.5) < 1e-6);
    let real_edges = new.edges.into_iter().map(|[a, b]| mesh::edge(a, b)).collect::<BTreeSet<_>>();
    let mut expected = old.edges.into_iter().map(|[a, b]| mesh::edge(a, b)).collect::<BTreeSet<_>>();
    expected.remove(&[2, 3]);
    expected.extend([[2, 8], [3, 8], [2, 7]]);
    assert_eq!(real_edges, expected);
}

#[test]
fn linear_quad_grid_follows_a_slanted_face_without_repositioning_original_corners() {
    let mut before = record();
    let mesh = geometry(&before);
    let mut points = mesh.vertices().to_vec();
    // 四个角点组成斜平行四边形，边及中心在局部半毫米坐标量化后仍保留原始点和合法闭壳。
    let offset = [1_866, 1_084, -826];
    let u = [-4_489, -2_049, -3_650];
    let v = [1_578, -751, 4_385];
    for (index, point) in points.iter_mut().enumerate() {
        *point = std::array::from_fn(|axis| {
            offset[axis] + (index as i32 & 1) * u[axis] + ((index as i32 >> 2) & 1) * v[axis]
                - if index & 2 == 0 || axis != 1 { 0 } else { 4_000 }
        });
    }
    before.asset_target = before.asset_target.with_editable_geometry(mesh.with_vertices(points).unwrap()).unwrap();
    let original = geometry(&before);
    let face = original.faces()[2].indices();
    let selected = (0..face.len()).fold(0, |bits, index| {
        let [a, b] = mesh::edge(face[index] as usize, face[(index + 1) % face.len()] as usize);
        bits | edge_selection(&before, a, b)
    });
    let after = subdivide_record(&before, MeshElementKind::Edge, &MeshSelection::from(selected)).unwrap();
    assert_closed(&after);
    let result = geometry(&after);
    assert_eq!(result.vertices().len(), 13);
    assert_eq!(&result.vertices()[..8], original.vertices());
    assert!(result.faces().iter().all(|face| face.indices().len() <= 5),
        "全选斜面边应按四边网格细分，补点邻面保留边界");
}

#[test]
fn two_collinear_selected_segments_only_insert_boundary_points() {
    let mut before = record();
    // 底部五边形的其中一边由原四边面中点拆为两段，两次新增切点只进入两侧原多边形。
    let original = geometry(&before);
    let mut vertices = original.vertices().to_vec();
    vertices.push([0, -2_000, -2_000]);
    let mut faces = original.faces().to_vec();
    faces[2] = feathrise_scene_content::MeshFace::new(&[0, 8, 1, 5, 4]).unwrap();
    faces[4] = feathrise_scene_content::MeshFace::new(&[0, 2, 3, 1, 8]).unwrap();
    before.asset_target = before.asset_target.with_editable_geometry(
        EditableMeshGeometry::from_parts(&vertices, &faces).unwrap(),
    ).unwrap();
    let result = subdivide_record_with_options(&before, MeshElementKind::Edge,
        &MeshSelection::from(edge_selection(&before, 0, 8) | edge_selection(&before, 8, 1)),
        SubdivisionOptions::default()).unwrap();
    let geometry = geometry(&result.record);
    assert_eq!(assert_closed(&result.record), (11, 15, 6));
    assert_eq!(geometry.faces().iter().filter(|face| face.indices().len() == 7).count(), 2,
        "共线选边不得接成横跨零面积的内部线");
}

#[test]
fn zero_operations_preserve_parametric_recipe_and_invalid_selection_is_rejected_atomically() {
    let before = record();
    assert_eq!(
        inset_record(&before, MeshElementKind::Face, &MeshSelection::from(0), 0.0).unwrap(),
        before
    );
    assert_eq!(
        bevel_record(&before, MeshElementKind::Edge, &MeshSelection::from(0), 0.0).unwrap(),
        before
    );
    assert_eq!(
        loop_cut_record(&before, MeshElementKind::Edge, &MeshSelection::from(0), 0.0).unwrap(),
        before
    );
    assert_eq!(
        subdivide_record(&before, MeshElementKind::Face, &MeshSelection::from(0)).unwrap(),
        before
    );
    let once = subdivide_record(&before, MeshElementKind::Face, &MeshSelection::from((1 << 6) - 1)).unwrap();
    let snapshot = once.clone();
    assert!(subdivide_record(&once, MeshElementKind::Face, &MeshSelection::from(1 << 24)).is_err());
    assert_eq!(once, snapshot, "过期元素索引不得向来源记录写入部分结果");
    assert!(inset_record(&before, MeshElementKind::Vertex, &MeshSelection::from(1), 0.2).is_err());
    assert!(
        bevel_record(
            &before,
            MeshElementKind::Edge,
            &MeshSelection::from(edge_selection(&before, 0, 1)),
            f32::NAN
        )
        .is_err()
    );
}

#[test]
fn malformed_source_indices_are_rejected_before_topology_indexing() {
    let before = record();
    let original = geometry(&before);
    let mut faces = original.faces().to_vec();
    faces[0] = feathrise_scene_content::MeshFace::new(&[63, 2, 6, 4]).unwrap();
    assert!(EditableMeshGeometry::from_parts(original.vertices(), &faces).is_err());

    let stale = MeshSelection::single(500);
    assert!(inset_record(&before, MeshElementKind::Face, &stale, 0.1).is_err());
    assert!(bevel_record(&before, MeshElementKind::Edge, &stale, 0.1).is_err());
    assert!(loop_cut_record(&before, MeshElementKind::Edge, &stale, 0.5).is_err());
    assert!(subdivide_record(&before, MeshElementKind::Face, &stale).is_err());
}

#[test]
fn repeated_default_subdivision_keeps_selection_past_u16_indices() {
    let mut source = record();
    let mut selection = MeshSelection::all(6);
    for round in 1..=7 {
        let result = subdivide_record_with_options(
            &source, MeshElementKind::Face, &selection, SubdivisionOptions::default(),
        ).unwrap();
        let geometry = geometry(&result.record);
        let faces = 6 * 4_usize.pow(round);
        assert_eq!(geometry.vertex_count(), faces + 2);
        assert_eq!(geometry.face_count(), faces);
        assert_eq!(geometry.edges().len(), faces * 2);
        assert_eq!(result.selected, MeshSelection::all(faces));
        source = result.record;
        selection = result.selected;
    }
    assert!(geometry(&source).faces().last().unwrap().indices().iter().any(|&i| i > u16::MAX as u32));
    assert!((volume(&source) - 8.0).abs() < 1e-8);
}

#[test]
fn maximum_cuts_builds_sixty_one_thousand_quads_with_the_original_bounds() {
    let before = record();
    let result = subdivide_record_with_options(
        &before, MeshElementKind::Face, &MeshSelection::all(6),
        SubdivisionOptions { cuts: MAX_SUBDIVISION_CUTS, ..Default::default() },
    ).unwrap();
    let geometry = geometry(&result.record);
    assert_eq!(geometry.vertex_count(), 61_208);
    assert_eq!(geometry.face_count(), 61_206);
    assert_eq!(geometry.edges().len(), 122_412);
    assert_eq!(geometry.bounds_half_mm(), ([-2_000; 3], [2_000; 3]));
    assert_eq!(result.selected, MeshSelection::all(61_206));
    assert!((volume(&result.record) - 8.0).abs() < 1e-8);
}

#[test]
fn subdivision_reports_half_millimeter_precision_when_cut_points_would_coincide() {
    let mut before = record();
    // 0.5mm 边的中点在作者格式内不能与两个端点都不同。
    let source = geometry(&before);
    let points = source.vertices().iter().map(|point| point.map(|value| i32::from(value > 0))).collect();
    before.asset_target = before.asset_target.with_editable_geometry(source.with_vertices(points).unwrap()).unwrap();
    let frozen = before.clone();
    let result = subdivide_record_with_options(
        &before, MeshElementKind::Face, &MeshSelection::all(6), SubdivisionOptions::default(),
    );
    assert!(result.err().unwrap().contains("半毫米"));
    assert_eq!(before, frozen);
}
