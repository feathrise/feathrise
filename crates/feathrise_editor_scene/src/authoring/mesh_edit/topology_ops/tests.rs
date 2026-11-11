//! 用正式几何校验和闭壳不变量验证五种拓扑结果，额外核对世界米制宽度与作者元数据保留。

use super::super::types::MeshElementKind;
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
        .position(|&edge| edge == mesh::edge(a, b))
        .unwrap();
    1_u128 << index
}

fn assert_record_preserved(before: &ScenePlacementRecord, after: &ScenePlacementRecord) {
    let mut restored = after.clone();
    restored.asset_target = before.asset_target;
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
    let mut edges = BTreeMap::<[u8; 2], Vec<[u8; 2]>>::new();
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
    let after = inset_record(&before, MeshElementKind::Face, 1 << 3, 0.25).unwrap();
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
    assert!(inset_record(&before, MeshElementKind::Face, 1 << 3, 1.1).is_err());
}

#[test]
fn inset_width_is_world_meters_under_rotated_nonuniform_object_scale() {
    let mut before = record();
    before.translation = [15.0, 7.0, -13.0];
    before.yaw_degrees = 37.0;
    before.scale = [2.0, 3.0, 4.0];
    let old = mesh::MeshWork::from_record(&before).unwrap();
    let after = inset_record(&before, MeshElementKind::Face, 1 << 3, 0.25).unwrap();
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
        edge_selection(&before, 0, 1),
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
    assert!(bevel_record(&before, MeshElementKind::Edge, touching, 0.25).is_err());
    assert!(
        bevel_record(
            &before,
            MeshElementKind::Edge,
            edge_selection(&before, 0, 1),
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
        let after = loop_cut_record(&before, MeshElementKind::Edge, selected, factor).unwrap();
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
            .map(|&v| geometry.vertices()[usize::from(v)][0])
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
    let after = subdivide_record(&before, MeshElementKind::Face, (1 << 6) - 1).unwrap();
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
    let triangles = geometry(&before).triangles();
    for selected in [1, 1 << 1, (1 << 8) - 1] {
        let after = subdivide_record(&before, MeshElementKind::Face, selected)
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
    for vertex in authored_geometry.vertices_half_mm.iter_mut().take(authored_geometry.vertex_count as usize) {
        *vertex = vertex.map(|coordinate| coordinate / 100);
    }
    before.asset_target.validate().unwrap();
    let expected = subdivide_record(&before, MeshElementKind::Face, (1 << 8) - 1).unwrap();
    // 半毫米作者坐标仍有足够细分间距；合法最小实例scale不能把凹角错判为凸角。
    before.scale = [0.001; 3];
    before.yaw_degrees = 37.0;
    before.translation = [10.0, 3.0, -7.0];
    let after = subdivide_record(&before, MeshElementKind::Face, (1 << 8) - 1)
        .expect("凹性由作者拓扑决定，不能依赖世界面积的绝对阈值");
    assert_closed(&after);
    assert_record_preserved(&before, &after);
    assert_eq!(geometry(&after), geometry(&expected));
}

#[test]
fn partial_face_and_edge_subdivision_repair_neighbors_without_t_junctions() {
    let before = record();
    for (kind, selected) in [
        (MeshElementKind::Face, 1 << 3),
        (MeshElementKind::Edge, edge_selection(&before, 0, 1)),
        (MeshElementKind::Vertex, (1 << 0) | (1 << 1)),
    ] {
        let after = subdivide_record(&before, kind, selected).unwrap();
        assert_closed(&after);
        assert_record_preserved(&before, &after);
        assert!(geometry(&after).vertices().len() > 8);
        assert!((volume(&after) - volume(&before)).abs() < 1e-6);
    }
}

#[test]
fn partial_subdivision_preserves_an_unselected_warped_quads_original_surface() {
    let mut before = record();
    let mut mesh = geometry(&before);
    mesh.vertices_half_mm[7][1] += 600;
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
        edge_selection(&before, 2, 3),
    )
    .unwrap();
    assert_closed(&after);
    assert_record_preserved(&before, &after);
    assert!((volume(&after) - volume(&before)).abs() < 1e-6);
    let old = mesh::MeshWork::from_record(&before).unwrap();
    let new = mesh::MeshWork::from_record(&after).unwrap();
    // 新点仍在原真实边上；原顶面0→2折线两边的表面由三角片原位保留。
    assert_eq!(new.points.len(), old.points.len() + 1);
    assert!(new.points[8].distance((old.points[2] + old.points[3]) * 0.5) < 1e-6);
}

#[test]
fn zero_operations_preserve_parametric_recipe_and_over_capacity_is_rejected_atomically() {
    let before = record();
    assert_eq!(
        inset_record(&before, MeshElementKind::Face, 0, 0.0).unwrap(),
        before
    );
    assert_eq!(
        bevel_record(&before, MeshElementKind::Edge, 0, 0.0).unwrap(),
        before
    );
    assert_eq!(
        loop_cut_record(&before, MeshElementKind::Edge, 0, 0.0).unwrap(),
        before
    );
    assert_eq!(
        subdivide_record(&before, MeshElementKind::Face, 0).unwrap(),
        before
    );
    let once = subdivide_record(&before, MeshElementKind::Face, (1 << 6) - 1).unwrap();
    let snapshot = once.clone();
    assert!(subdivide_record(&once, MeshElementKind::Face, (1 << 24) - 1).is_err());
    assert_eq!(once, snapshot, "超容量错误不得向来源记录写入部分结果");
    assert!(inset_record(&before, MeshElementKind::Vertex, 1, 0.2).is_err());
    assert!(
        bevel_record(
            &before,
            MeshElementKind::Edge,
            edge_selection(&before, 0, 1),
            f32::NAN
        )
        .is_err()
    );
}

#[test]
fn malformed_source_indices_are_rejected_before_topology_indexing() {
    let mut before = record();
    let mut broken = geometry(&before);
    broken.faces[0].vertices[0] = 63;
    before.asset_target = ScenePlacementAssetTarget::EditableMesh {
        geometry: broken,
        color_srgb: [10, 20, 30],
        roughness_milli: 700,
        collision: true,
    };
    assert!(inset_record(&before, MeshElementKind::Face, 1, 0.1).is_err());
    assert!(bevel_record(&before, MeshElementKind::Edge, 1, 0.1).is_err());
    assert!(loop_cut_record(&before, MeshElementKind::Edge, 1, 0.5).is_err());
    assert!(knife_record(&before, Vec3::ZERO, Vec3::X).is_err());
    assert!(subdivide_record(&before, MeshElementKind::Face, 1).is_err());
}
