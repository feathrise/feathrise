//! 三价凸折边的单段倒角：截短相邻面、修补端面并生成独立倒角面，保持共享边闭壳。

use super::super::types::MeshElementKind;
use super::mesh::{EPSILON, MeshWork, chosen, edge, planar_normal, positive_amount};
use feathrise_map_content::MapPlacementRecord;
use std::collections::BTreeSet;

pub(in super::super) fn bevel_record(
    before: &MapPlacementRecord,
    kind: MeshElementKind,
    selected: u128,
    width_meters: f32,
) -> Result<MapPlacementRecord, String> {
    let width = positive_amount(width_meters, "倒角宽度")?;
    if width == 0.0 {
        return Ok(before.clone());
    }
    if kind != MeshElementKind::Edge {
        return Err("倒角需要在边模式选择凸折边".into());
    }
    let mut work = MeshWork::from_record(before)?;
    let selected = chosen(selected, work.edges.len())?;
    if selected.is_empty() {
        return Err("请先选择需要倒角的边".into());
    }
    let edges = selected
        .into_iter()
        .map(|index| work.edges[index])
        .collect::<Vec<_>>();
    let mut endpoints = BTreeSet::new();
    for &[a, b] in &edges {
        if !endpoints.insert(a) || !endpoints.insert(b) {
            return Err("单段倒角暂不合并共端点边，请分步选择互不接触的边".into());
        }
    }
    for [a, b] in edges {
        bevel_edge(&mut work, a, b, width)?;
    }
    work.finish(before)
}

fn bevel_edge(work: &mut MeshWork, a: usize, b: usize, width: f64) -> Result<(), String> {
    let mut adjacent = Vec::new();
    for (index, face) in work.faces.iter().enumerate() {
        for i in 0..face.len() {
            if edge(face[i], face[(i + 1) % face.len()]) == edge(a, b) {
                adjacent.push((index, i));
            }
        }
    }
    if adjacent.len() != 2 {
        return Err("倒角边必须恰好连接两个面".into());
    }
    if work.faces[adjacent[0].0][adjacent[0].1] != a {
        adjacent.swap(0, 1);
    }
    let [(first, first_edge), (second, second_edge)] = [adjacent[0], adjacent[1]];
    if work.faces[first][first_edge] != a || work.faces[second][second_edge] != b {
        return Err("倒角边的两个面绕序不一致".into());
    }
    let f0 = &work.faces[first];
    let f1 = &work.faces[second];
    let near = [
        [
            f0[(first_edge + f0.len() - 1) % f0.len()],
            f1[(second_edge + 2) % f1.len()],
        ],
        [
            f0[(first_edge + 2) % f0.len()],
            f1[(second_edge + f1.len() - 1) % f1.len()],
        ],
    ];
    for vertex in [a, b] {
        let incident = work
            .faces
            .iter()
            .filter(|face| face.contains(&vertex))
            .collect::<Vec<_>>();
        let neighbors = incident
            .iter()
            .flat_map(|face| {
                let i = face.iter().position(|&v| v == vertex).unwrap();
                [
                    face[(i + face.len() - 1) % face.len()],
                    face[(i + 1) % face.len()],
                ]
            })
            .collect::<BTreeSet<_>>();
        if incident.len() != 3 || neighbors.len() != 3 {
            return Err("单段倒角目前需要边两端各连接三条边，请先处理复杂角点".into());
        }
    }
    let normal0 = planar_normal(f0, &work.points)?;
    let normal1 = planar_normal(f1, &work.points)?;
    if normal0.dot(work.points[near[0][1]] - work.points[a]) >= -EPSILON
        || normal1.dot(work.points[near[0][0]] - work.points[a]) >= -EPSILON
    {
        return Err("单段倒角目前支持凸折边，不能倒角共面或内凹折边".into());
    }
    let direction = (work.points[b] - work.points[a])
        .try_normalize()
        .ok_or("倒角边长度为零")?;
    let mut cut = [[0; 2]; 2];
    for (endpoint, vertex) in [a, b].into_iter().enumerate() {
        for side in 0..2 {
            let toward = work.points[near[endpoint][side]] - work.points[vertex];
            let distance = (toward - direction * toward.dot(direction)).length();
            if distance <= EPSILON || width >= distance * (1.0 - 1e-6) {
                return Err("倒角宽度越过邻边，请减小宽度".into());
            }
            // 以邻面中垂直于原边的距离计宽度，非直角端部也不会把宽度偷换成边长比例。
            cut[endpoint][side] =
                work.push_point(work.points[vertex] + toward * (width / distance))?;
        }
    }
    for (index, face) in work.faces.iter_mut().enumerate() {
        let old = face.clone();
        let mut result = Vec::with_capacity(old.len() + 2);
        for (position, &vertex) in old.iter().enumerate() {
            let Some(endpoint) = [a, b].iter().position(|&end| end == vertex) else {
                result.push(vertex);
                continue;
            };
            if index == first {
                result.push(cut[endpoint][0]);
            } else if index == second {
                result.push(cut[endpoint][1]);
            } else {
                let previous = old[(position + old.len() - 1) % old.len()];
                let next = old[(position + 1) % old.len()];
                let previous_side = near[endpoint]
                    .iter()
                    .position(|&v| v == previous)
                    .ok_or("倒角端面邻接不完整")?;
                let next_side = near[endpoint]
                    .iter()
                    .position(|&v| v == next)
                    .ok_or("倒角端面邻接不完整")?;
                result.extend([cut[endpoint][previous_side], cut[endpoint][next_side]]);
            }
        }
        *face = result;
    }
    // 与邻面共享边使用相反绕序；原端点不再被引用，由统一完成步骤压紧顶点表。
    work.faces
        .push(vec![cut[1][0], cut[0][0], cut[0][1], cut[1][1]]);
    Ok(())
}
