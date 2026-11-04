//! 四边面环切与贯穿平面切割：共享交点，增加真实子面，始终保留切面两侧的原闭壳表面。

use super::super::types::MeshElementKind;
use super::mesh::{EPSILON, MeshWork, chosen, edge, triangulate};
use bevy::prelude::Vec3;
use feathrise_map_content::MapPlacementRecord;
use std::collections::{BTreeMap, VecDeque};

pub(in super::super) fn loop_cut_record(
    before: &MapPlacementRecord,
    kind: MeshElementKind,
    selected: u128,
    factor: f32,
) -> Result<MapPlacementRecord, String> {
    if !factor.is_finite() || !(0.0..=1.0).contains(&factor) {
        return Err("环切位置必须在0～1之间".into());
    }
    if factor == 0.0 || factor == 1.0 {
        return Ok(before.clone());
    }
    if kind != MeshElementKind::Edge {
        return Err("环切需要在边模式选择一条种子边".into());
    }
    let mut work = MeshWork::from_record(before)?;
    let selected = chosen(selected, work.edges.len())?;
    if selected.len() != 1 {
        return Err("环切每次需要一条种子边；切线沿相对四边面连续传播".into());
    }
    let seed = work.edges[selected[0]];
    let mut cuts = BTreeMap::from([(edge(seed[0], seed[1]), f64::from(factor))]);
    let mut queue = VecDeque::from([edge(seed[0], seed[1])]);
    while let Some(key) = queue.pop_front() {
        let mut incidents = 0;
        for face in &work.faces {
            let Some(position) =
                (0..face.len()).find(|&i| edge(face[i], face[(i + 1) % face.len()]) == key)
            else {
                continue;
            };
            incidents += 1;
            if face.len() != 4 {
                return Err("环切路径遇到非四边面，请改用切割或先整理拓扑".into());
            }
            let local = if face[position] == key[0] {
                cuts[&key]
            } else {
                1.0 - cuts[&key]
            };
            let opposite = (position + 2) % 4;
            let other = edge(face[opposite], face[(opposite + 1) % 4]);
            let other_factor = if face[opposite] == other[0] {
                1.0 - local
            } else {
                local
            };
            if let Some(old) = cuts.get(&other) {
                if (old - other_factor).abs() > 1e-6 {
                    return Err("环带扭转导致切割位置不一致，请使用中点环切".into());
                }
            } else {
                cuts.insert(other, other_factor);
                queue.push_back(other);
            }
        }
        if incidents != 2 {
            return Err("环切需要闭合的双面共享边".into());
        }
    }
    let mut cut_vertices = BTreeMap::new();
    for (&[a, b], &amount) in &cuts {
        let vertex = work.push_point(work.points[a].lerp(work.points[b], amount))?;
        cut_vertices.insert(edge(a, b), vertex);
    }
    let old_faces = work.faces.clone();
    let mut result = Vec::with_capacity(old_faces.len() * 2);
    for face in old_faces {
        let hits = (0..face.len())
            .filter_map(|i| {
                cut_vertices
                    .get(&edge(face[i], face[(i + 1) % face.len()]))
                    .map(|&v| (i, v))
            })
            .collect::<Vec<_>>();
        if hits.is_empty() {
            result.push(face);
            continue;
        }
        if hits.len() != 2 || (hits[0].0 + 2) % 4 != hits[1].0 {
            return Err("环切路径在同一面交叉，不能产生合法子面".into());
        }
        let [(first, a), (second, b)] = [hits[0], hits[1]];
        result.push(vec![a, face[(first + 1) % 4], face[second], b]);
        result.push(vec![b, face[(second + 1) % 4], face[first], a]);
    }
    work.faces = result;
    work.finish(before)
}

pub(in super::super) fn knife_record(
    before: &MapPlacementRecord,
    plane_origin_world: Vec3,
    plane_normal_world: Vec3,
) -> Result<MapPlacementRecord, String> {
    if !plane_origin_world.is_finite() || !plane_normal_world.is_finite() {
        return Err("切割平面必须使用有限坐标".into());
    }
    let origin = plane_origin_world.as_dvec3();
    let normal = plane_normal_world
        .as_dvec3()
        .try_normalize()
        .ok_or("请拖出有效切割方向")?;
    let mut work = MeshWork::from_record(before)?;
    let distances = work
        .points
        .iter()
        .map(|point| normal.dot(*point - origin))
        .collect::<Vec<_>>();
    if !distances.iter().any(|distance| *distance > EPSILON)
        || !distances.iter().any(|distance| *distance < -EPSILON)
    {
        return Ok(before.clone());
    }
    let old_faces = work.faces.clone();
    let mut intersections = BTreeMap::new();
    let mut result = Vec::new();
    let mut changed = false;
    for face in old_faces {
        if !face.iter().any(|&v| distances[v] > EPSILON)
            || !face.iter().any(|&v| distances[v] < -EPSILON)
        {
            result.push(face);
            continue;
        }
        changed = true;
        // 切割凹面或非平面四边面时先沿正式表面拆三角；不会跨面补一条错误直线或删除半边。
        for triangle in triangulate(&face, &work.points)? {
            if triangle
                .iter()
                .all(|&vertex| distances[vertex].abs() <= EPSILON)
            {
                result.push(triangle.to_vec());
                continue; // 切平面上的原三角只保留一次，不能在正负两侧各复制一份共面壳。
            }
            let mut sides = [Vec::new(), Vec::new()];
            for side in 0..2 {
                let sign = if side == 0 { 1.0 } else { -1.0 };
                for i in 0..3 {
                    let a = triangle[i];
                    let b = triangle[(i + 1) % 3];
                    let da = distances[a] * sign;
                    let db = distances[b] * sign;
                    if da >= -EPSILON {
                        sides[side].push(a);
                    }
                    if (da > EPSILON && db < -EPSILON) || (da < -EPSILON && db > EPSILON) {
                        let key = edge(a, b);
                        let vertex = if let Some(&vertex) = intersections.get(&key) {
                            vertex
                        } else {
                            let ratio = distances[a] / (distances[a] - distances[b]);
                            let vertex =
                                work.push_point(work.points[a].lerp(work.points[b], ratio))?;
                            intersections.insert(key, vertex);
                            vertex
                        };
                        sides[side].push(vertex);
                    }
                }
                sides[side].dedup();
                if sides[side].len() >= 3 {
                    result.push(std::mem::take(&mut sides[side]));
                }
            }
        }
    }
    if !changed {
        return Ok(before.clone());
    }
    work.faces = result;
    work.finish(before)
}
