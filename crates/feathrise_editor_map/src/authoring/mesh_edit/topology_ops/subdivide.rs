//! 共享边中点与面中心细分；邻面同步接入新边点，避免只细分所选面留下T形裂缝。

use super::super::types::MeshElementKind;
use super::mesh::{EPSILON, MeshWork, chosen, edge, face_normal, triangulate};
use bevy::math::DVec3;
use feathrise_map_content::MapPlacementRecord;
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn subdivide_record(
    before: &MapPlacementRecord,
    kind: MeshElementKind,
    selected: u128,
) -> Result<MapPlacementRecord, String> {
    if selected == 0 {
        return Ok(before.clone());
    }
    let mut work = MeshWork::from_record(before)?;
    let mut selected_faces = BTreeMap::new();
    let edges = match kind {
        MeshElementKind::Face => {
            let geometry = before.asset_target.editable_geometry()
                .ok_or("当前来源不支持拓扑编辑")?;
            // 凹性属于作者表面；以本地半毫米整数检测，避免合法小scale或远原点压掉世界转角面积。
            let points = geometry.vertices().iter()
                .map(|point| DVec3::from_array(point.map(f64::from))).collect::<Vec<_>>();
            let triangles = geometry.triangles_with_faces();
            for index in chosen(selected, work.faces.len())? {
                let face = &work.faces[index];
                let normal = face_normal(face, &points)?;
                let concave = (0..face.len()).any(|i| {
                    let previous = points[face[(i + face.len() - 1) % face.len()]];
                    let point = points[face[i]];
                    let next = points[face[(i + 1) % face.len()]];
                    normal.dot((point - previous).cross(next - point)) < -EPSILON
                });
                // 凹面的平均中心可能落在面外或与原凹角重合；沿正式表面先分片，
                // 再共享真实边和内部对角线的中点。凸面仍保留原来的面中心四边细分。
                let parts = if concave {
                    triangles.iter().filter(|&&(face, _)| face == index)
                        .map(|&(_, triangle)| triangle.to_vec()).collect::<Vec<_>>()
                } else {
                    vec![face.clone()]
                };
                selected_faces.insert(index, parts);
            }
            selected_faces
                .values().flatten()
                .flat_map(|face| {
                    (0..face.len())
                        .map(|i| edge(face[i], face[(i + 1) % face.len()]))
                        .collect::<Vec<_>>()
                })
                .collect::<BTreeSet<_>>()
        }
        MeshElementKind::Edge => chosen(selected, work.edges.len())?
            .into_iter()
            .map(|index| work.edges[index])
            .collect(),
        MeshElementKind::Vertex => {
            let vertices = chosen(selected, work.points.len())?
                .into_iter()
                .collect::<BTreeSet<_>>();
            work.edges
                .iter()
                .copied()
                .filter(|[a, b]| vertices.contains(a) && vertices.contains(b))
                .collect()
        }
    };
    if edges.is_empty() {
        return Err("细分需要完整选边，或选择需要细分的面".into());
    }
    let mut midpoints = BTreeMap::new();
    for [a, b] in edges {
        work.midpoint(a, b, &mut midpoints)?;
    }
    let old_faces = work.faces.clone();
    let mut result = Vec::new();
    for (index, face) in old_faces.into_iter().enumerate() {
        if let Some(parts) = selected_faces.get(&index) {
            for face in parts {
                let center = face
                    .iter()
                    .fold(DVec3::ZERO, |sum, &vertex| sum + work.points[vertex])
                    / face.len() as f64;
                let center = work.push_point(center)?;
                for i in 0..face.len() {
                    let previous = face[(i + face.len() - 1) % face.len()];
                    let vertex = face[i];
                    let next = face[(i + 1) % face.len()];
                    result.push(vec![
                        vertex,
                        midpoints[&edge(vertex, next)],
                        center,
                        midpoints[&edge(previous, vertex)],
                    ]);
                }
            }
        } else {
            if !(0..face.len())
                .any(|i| midpoints.contains_key(&edge(face[i], face[(i + 1) % face.len()])))
            {
                result.push(face);
            } else {
                // 先按正式表面拆三角，再把真实边中点插回三角边界；合法非平面四边面
                // 不能被当成平面多边面拒绝，也不能因修补邻面而改掉原来的0→2折面。
                for triangle in triangulate(&face, &work.points)? {
                    let mut boundary = Vec::with_capacity(6);
                    for i in 0..3 {
                        boundary.push(triangle[i]);
                        if let Some(&midpoint) =
                            midpoints.get(&edge(triangle[i], triangle[(i + 1) % 3]))
                        {
                            boundary.push(midpoint);
                        }
                    }
                    result.extend(
                        triangulate(&boundary, &work.points)?
                            .into_iter()
                            .map(|triangle| triangle.to_vec()),
                    );
                }
            }
        }
    }
    work.faces = result;
    work.finish(before)
}
