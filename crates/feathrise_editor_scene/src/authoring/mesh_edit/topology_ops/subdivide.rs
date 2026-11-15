//! 线性细分先在原拓扑上标记选边，再生成共用边点、自有面片与结果选区。

use super::super::{selection::MeshSelection, types::MeshElementKind};
use super::mesh::{MeshWork, chosen, edge, face_normal, triangulate};
use bevy::math::DVec3;
use feathrise_scene_content::{
    MAX_EDITABLE_MESH_BYTES, MAX_EDITABLE_MESH_FACE_VERTICES, MAX_EDITABLE_MESH_FACES,
    ScenePlacementRecord, editable_mesh_required_bytes,
};
use std::collections::{BTreeSet, HashMap};

mod patterns;

/// 统一操作参数可表达的范围；可用刀数最终由当前对象的真实顶点和面容量决定。
pub(crate) const MAX_SUBDIVISION_CUTS: u8 = 100;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SubdivisionCorner {
    InnerVertex,
    Path,
    #[default]
    StraightCut,
    Fan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SubdivisionOptions {
    pub cuts: u8,
    pub corner: SubdivisionCorner,
    pub create_ngons: bool,
}

impl Default for SubdivisionOptions {
    fn default() -> Self {
        Self { cuts: 1, corner: SubdivisionCorner::StraightCut, create_ngons: true }
    }
}

pub(in super::super) struct SubdivisionResult {
    pub record: ScenePlacementRecord,
    pub selected: MeshSelection,
}

#[cfg(test)]
pub(in super::super) fn subdivide_record(
    before: &ScenePlacementRecord,
    kind: MeshElementKind,
    selected: &MeshSelection,
) -> Result<ScenePlacementRecord, String> {
    Ok(subdivide_record_with_options(before, kind, selected, SubdivisionOptions::default())?.record)
}

pub(in super::super) fn subdivide_record_with_options(
    before: &ScenePlacementRecord,
    kind: MeshElementKind,
    selected: &MeshSelection,
    options: SubdivisionOptions,
) -> Result<SubdivisionResult, String> {
    if !(1..=MAX_SUBDIVISION_CUTS).contains(&options.cuts) {
        return Err(format!("细分切割次数必须为1～{MAX_SUBDIVISION_CUTS}"));
    }
    if selected.is_empty() {
        return Ok(SubdivisionResult { record: before.clone(), selected: selected.clone() });
    }
    // 线性插值不依赖观察或实例姿态；在作者局部半毫米空间计算，最后一次量化。
    let mut work = MeshWork::from_local_record(before)?;
    let (edges, mut selected_vertices) = selected_edges(&work, kind, selected)?;
    if edges.is_empty() {
        return Err("细分需要完整选边，或选择需要细分的面".into());
    }
    let cuts = usize::from(options.cuts);
    let segments = cuts + 1;
    let original_vertices = work.points.len();
    check_budget(original_vertices + edges.len() * cuts, work.faces.len())?;

    // 只有原边参与拓扑模式判定；新建对角线不反向扩张相邻面选区。
    let mut edge_points = EdgePoints(HashMap::with_capacity(edges.len()));
    for &[a, b] in &edges {
        edge_points.add(&mut work, a, b, segments)?;
    }
    let old_faces = std::mem::take(&mut work.faces);
    let mut faces = Vec::with_capacity(old_faces.len());
    for face in old_faces {
        let cuts_on_sides: Vec<_> = (0..face.len())
            .filter(|&side| edge_points.has(face[side], face[(side + 1) % face.len()]))
            .collect();
        if cuts_on_sides.is_empty() {
            push_face(&mut faces, face)?;
            continue;
        }
        if face.len() == 4 && !regular_quad(&face, &work.points) {
            // 现有四边面的固定0→2剖分承载真实折面；在其三角片内生成网格，避免双线性插值移走表面。
            let triangles = [[face[0], face[1], face[2]], [face[0], face[2], face[3]]];
            if cuts_on_sides.len() == 4 {
                edge_points.add(&mut work, face[0], face[2], segments)?;
                for tri in triangles {
                    patterns::triangle_grid(&tri, &edge_points, segments, &mut work, &mut faces)?;
                }
            } else {
                for tri in triangles {
                    append_quantized(&mut faces, edge_points.boundary(&tri), &work)?;
                }
            }
            continue;
        }

        match (face.len(), cuts_on_sides.as_slice()) {
            (3, [_, _, _]) => {
                patterns::triangle_grid(&face, &edge_points, segments, &mut work, &mut faces)?;
            }
            (4, [_, _, _, _]) => {
                patterns::quad_grid(&face, &edge_points, segments, &mut work, &mut faces)?;
            }
            _ => {
                let boundary = edge_points.boundary(&face);
                let children = patterns::partial_faces(&face, &cuts_on_sides, boundary, &edge_points, options, &mut work)?;
                for child in children {
                    append_quantized(&mut faces, child, &work)?;
                }
            }
        }
    }
    work.faces = faces;
    let record = work.finish_local(before)?;
    let result = record.asset_target.editable_geometry().expect("细分产生完整作者网格");
    // 细分始终保留原顶点顺序。原选点和新建点一起刷新同类型的边/面选区。
    for index in original_vertices..result.vertex_count() {
        selected_vertices.insert(index);
    }
    let selected = match kind {
        MeshElementKind::Vertex => selected_vertices,
        MeshElementKind::Edge => result.edges().iter().enumerate().filter_map(|(index, &[a, b])| {
            (selected_vertices.contains(a) && selected_vertices.contains(b)).then_some(index)
        }).collect(),
        MeshElementKind::Face => result.faces().iter().enumerate().filter_map(|(index, face)| {
            face.indices().iter().all(|&vertex| selected_vertices.contains(vertex as usize)).then_some(index)
        }).collect(),
    };
    Ok(SubdivisionResult { record, selected })
}

fn selected_edges(
    work: &MeshWork,
    kind: MeshElementKind,
    selected: &MeshSelection,
) -> Result<(BTreeSet<[usize; 2]>, MeshSelection), String> {
    let mut edges = BTreeSet::new();
    let mut vertices = MeshSelection::default();
    match kind {
        MeshElementKind::Face => {
            let indices = chosen(selected, work.faces.len())?;
            if indices.len() == work.faces.len() {
                edges.extend(work.edges.iter().copied());
                return Ok((edges, MeshSelection::all(work.points.len())));
            }
            for index in indices {
                let face = &work.faces[index];
                for (side, &vertex) in face.iter().enumerate() {
                    vertices.insert(vertex);
                    edges.insert(edge(vertex, face[(side + 1) % face.len()]));
                }
            }
        }
        MeshElementKind::Edge => {
            for index in chosen(selected, work.edges.len())? {
                let [a, b] = work.edges[index];
                vertices.insert(a);
                vertices.insert(b);
                edges.insert(edge(a, b));
            }
        }
        MeshElementKind::Vertex => {
            vertices = chosen(selected, work.points.len())?.into_iter().collect();
            edges.extend(work.edges.iter().copied().filter(|&[a, b]| vertices.contains(a) && vertices.contains(b)));
        }
    }
    Ok((edges, vertices))
}

struct EdgePoints(HashMap<[usize; 2], Vec<usize>>);

impl EdgePoints {
    fn has(&self, a: usize, b: usize) -> bool {
        self.0.contains_key(&edge(a, b))
    }

    fn add(&mut self, work: &mut MeshWork, a: usize, b: usize, segments: usize) -> Result<(), String> {
        let [a, b] = edge(a, b);
        if self.has(a, b) {
            return Ok(());
        }
        let [start, end] = [work.points[a], work.points[b]];
        let mut points = Vec::with_capacity(segments - 1);
        let mut previous = start.round();
        for step in 1..segments {
            let point = start.lerp(end, step as f64 / segments as f64);
            let quantized = point.round();
            // 半毫米格式无法容纳重合的连续切点，拒绝候选同时保留原网格。
            if quantized == previous || quantized == end.round() {
                return Err("选边已接近半毫米存档精度，无法插入这些切点".into());
            }
            points.push(work.push_point(point)?);
            previous = quantized;
        }
        self.0.insert([a, b], points);
        Ok(())
    }

    fn ordered(&self, a: usize, b: usize) -> Vec<usize> {
        let Some(points) = self.0.get(&edge(a, b)) else { return Vec::new(); };
        if a < b { points.clone() } else { points.iter().rev().copied().collect() }
    }

    fn chain(&self, a: usize, b: usize) -> Vec<usize> {
        let mut chain = vec![a];
        chain.extend(self.ordered(a, b));
        chain.push(b);
        chain
    }

    fn boundary(&self, face: &[usize]) -> Vec<usize> {
        let mut boundary = Vec::with_capacity(face.len());
        for (index, &vertex) in face.iter().enumerate() {
            boundary.push(vertex);
            boundary.extend(self.ordered(vertex, face[(index + 1) % face.len()]));
        }
        boundary
    }
}

fn regular_quad(face: &[usize], points: &[DVec3]) -> bool {
    let Ok(normal) = face_normal(face, points) else { return false; };
    let origin = points[face[0]];
    // 使用本地半毫米点判断真实折面或凹角，实例缩放和世界位置不参与判定。
    (0..4).all(|index| {
        let [a, b, c] = [points[face[index]], points[face[(index + 1) % 4]], points[face[(index + 2) % 4]]];
        normal.dot(a - origin).abs() <= 1e-6 && normal.dot((b - a).cross(c - b)) > 0.0
    })
}

fn push_face(faces: &mut Vec<Vec<usize>>, face: Vec<usize>) -> Result<(), String> {
    if faces.len() >= MAX_EDITABLE_MESH_FACES {
        return Err("细分结果超过作者网格内存预算，请减少切割次数或选择范围".into());
    }
    faces.push(face);
    Ok(())
}

fn check_budget(vertices: usize, faces: usize) -> Result<(), String> {
    if editable_mesh_required_bytes(vertices, faces).is_none_or(|bytes| bytes > MAX_EDITABLE_MESH_BYTES) {
        return Err("细分结果超过作者网格内存预算，请减少切割次数或选择范围".into());
    }
    Ok(())
}

fn append_quantized(
    faces: &mut Vec<Vec<usize>>,
    mut face: Vec<usize>,
    work: &MeshWork,
) -> Result<(), String> {
    if face.len() == 4 {
        // 四边面保存使用固定0→2剖分。含共线边点的四边面旋转一次，保留同一真实边界与绕序。
        let good_diagonal = |offset: usize| {
            let [a, b, c, d] = std::array::from_fn(|index| work.points[face[(index + offset) % 4]].round());
            (b - a).cross(c - a).length_squared() >= 0.5 && (c - a).cross(d - a).length_squared() >= 0.5
        };
        if !good_diagonal(0) && good_diagonal(1) {
            face.rotate_left(1);
        }
    } else if face.len() > 4 {
        let origin = work.points[face[0]].round();
        let normal = (0..face.len()).fold(DVec3::ZERO, |sum, side| {
            sum + (work.points[face[side]].round() - origin)
                .cross(work.points[face[(side + 1) % face.len()]].round() - origin)
        }).try_normalize().ok_or("细分后平面面片退化")?;
        // 只拆超过单面格式边界或量化后不共面的面片，保留一般邻接 N 边面。
        if face.len() > MAX_EDITABLE_MESH_FACE_VERTICES
            || face.iter().any(|&vertex| normal.dot(work.points[vertex].round() - origin).abs() > 0.75) {
            for triangle in triangulate(&face, &work.points)? {
                push_face(faces, triangle.to_vec())?;
            }
            return Ok(());
        }
    }
    push_face(faces, face)
}
