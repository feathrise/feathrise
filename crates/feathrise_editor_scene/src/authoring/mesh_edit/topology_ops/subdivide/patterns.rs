//! 按原面的选边排列生成线性子面，所有接缝使用共享边上的同一顶点。

use super::{EdgePoints, MeshWork, SubdivisionCorner, SubdivisionOptions, check_budget, push_face};
use bevy::math::{DVec2, DVec3};

pub(super) fn quad_grid(
    face: &[usize],
    edge_points: &EdgePoints,
    segments: usize,
    work: &mut MeshWork,
    faces: &mut Vec<Vec<usize>>,
) -> Result<(), String> {
    let cuts = segments - 1;
    check_grid_budget(work, faces, cuts * cuts, segments * segments)?;
    let [a, b, c, d] = [face[0], face[1], face[2], face[3]];
    let top = edge_points.chain(a, b);
    let bottom = edge_points.chain(d, c);
    let left = edge_points.chain(a, d);
    let right = edge_points.chain(b, c);
    let mut grid = vec![top];
    for row in 1..segments {
        let [start, end] = [work.points[left[row]], work.points[right[row]]];
        let mut line = vec![left[row]];
        for column in 1..segments {
            line.push(work.push_point(start.lerp(end, column as f64 / segments as f64))?);
        }
        line.push(right[row]);
        grid.push(line);
    }
    grid.push(bottom);
    let cell = |column: usize, row: usize| vec![grid[row][column], grid[row][column + 1],
        grid[row + 1][column + 1], grid[row + 1][column]];
    if segments == 2 {
        // 四个子面沿原角点顺序登记，保留单刀细分的稳定选区及待确认的旧记录顺序。
        for (corner, (column, row)) in [(0, 0), (1, 0), (1, 1), (0, 1)].into_iter().enumerate() {
            let mut child = cell(column, row);
            child.rotate_left(corner);
            push_face(faces, child)?;
        }
    } else {
        for row in 0..segments {
            for column in 0..segments {
                push_face(faces, cell(column, row))?;
            }
        }
    }
    Ok(())
}

pub(super) fn triangle_grid(
    face: &[usize],
    edge_points: &EdgePoints,
    segments: usize,
    work: &mut MeshWork,
    faces: &mut Vec<Vec<usize>>,
) -> Result<(), String> {
    check_grid_budget(work, faces, (segments - 1) * (segments - 2) / 2, segments * segments)?;
    let [a, b, c] = [face[0], face[1], face[2]];
    let left = edge_points.chain(a, b);
    let right = edge_points.chain(a, c);
    let mut grid = vec![vec![a]];
    for row in 1..segments {
        let [start, end] = [work.points[left[row]], work.points[right[row]]];
        let mut line = vec![left[row]];
        for column in 1..row {
            line.push(work.push_point(start.lerp(end, column as f64 / row as f64))?);
        }
        line.push(right[row]);
        grid.push(line);
    }
    grid.push(edge_points.chain(b, c));
    for row in 0..segments {
        for column in 0..=row {
            push_face(faces, vec![grid[row][column], grid[row + 1][column], grid[row + 1][column + 1]])?;
        }
        for column in 0..row {
            push_face(faces, vec![grid[row][column], grid[row + 1][column + 1], grid[row][column + 1]])?;
        }
    }
    Ok(())
}

fn check_grid_budget(
    work: &MeshWork,
    faces: &[Vec<usize>],
    inner_vertices: usize,
    child_faces: usize,
) -> Result<(), String> {
    check_budget(work.points.len() + inner_vertices, faces.len() + child_faces)
}

/// 角点裁出的各条连线将一个面环分成若干面环，已有多边面只采用内部合法的连线。
pub(super) fn partial_faces(
    face: &[usize],
    sides: &[usize],
    boundary: Vec<usize>,
    edge_points: &EdgePoints,
    options: SubdivisionOptions,
    work: &mut MeshWork,
) -> Result<Vec<Vec<usize>>, String> {
    let mut polygons = vec![boundary];
    match sides {
        [side] if !options.create_ngons && face.len() <= 4 => {
            let side = *side;
            let [a, b] = [face[side], face[(side + 1) % face.len()]];
            let other = face[(side + 2) % face.len()];
            let outer = face[(side + 3) % face.len()];
            let edge = edge_points.ordered(a, b);
            for (index, &point) in edge.iter().enumerate() {
                if face.len() == 3 {
                    connect(&mut polygons, point, other, &work.points, false)?;
                } else {
                    if index < edge.len().div_ceil(2) {
                        connect(&mut polygons, point, outer, &work.points, false)?;
                    }
                    if index >= edge.len() / 2 {
                        connect(&mut polygons, point, other, &work.points, false)?;
                    }
                }
            }
        }
        [first, second] if non_collinear(face, *first, *second, &work.points) => {
            let [first, second] = [*first, *second];
            let right = (first + 1) % face.len();
            let left = (second + 1) % face.len();
            if face.len() == 4 && (right == second || left == first) {
                let start = if right == second { first } else { second };
                let corners = std::array::from_fn(|index| face[(start + index) % 4]);
                quad_corner(&mut polygons, corners, edge_points, options, work)?;
            } else {
                let [a, b] = [face[first], face[right]];
                let [c, d] = [face[second], face[left]];
                let from = edge_points.ordered(a, b);
                let to = edge_points.ordered(d, c);
                for (from, to) in from.into_iter().zip(to) {
                    connect(&mut polygons, from, to, &work.points, face.len() > 4)?;
                }
            }
        }
        [_, _, _] if face.len() == 4 => {
            // 三条相邻边中间一条左右交替连接，再从未切边一侧向中心连接对应的两条对边。
            let first = (0..4).find(|&side| !sides.contains(&side)).expect("四边面还有一条未切边") + 1;
            let [a, b, c, d] = std::array::from_fn(|index| face[(first + index) % 4]);
            let near = edge_points.ordered(a, b);
            let mid = edge_points.ordered(b, c);
            let far = edge_points.ordered(d, c);
            let count = mid.len();
            for (index, middle) in mid.into_iter().enumerate() {
                let opposite = count - 1 - index;
                if index < count.div_ceil(2) {
                    connect(&mut polygons, near[opposite], middle, &work.points, false)?;
                }
                if index >= count / 2 {
                    connect(&mut polygons, middle, far[index], &work.points, false)?;
                }
            }
            for index in 0..=count / 2 {
                connect(&mut polygons, near[index], far[index], &work.points, false)?;
            }
        }
        _ => {}
    }
    Ok(polygons)
}

fn quad_corner(
    polygons: &mut Vec<Vec<usize>>,
    [a, b, c, d]: [usize; 4],
    edge_points: &EdgePoints,
    options: SubdivisionOptions,
    work: &mut MeshWork,
) -> Result<(), String> {
    let first = edge_points.ordered(b, a);
    let second = edge_points.ordered(b, c);
    let pattern = match (options.corner, options.create_ngons) {
        (SubdivisionCorner::StraightCut, false) => SubdivisionCorner::InnerVertex,
        (corner, _) => corner,
    };
    match pattern {
        SubdivisionCorner::InnerVertex => {
            // 每条横向切线引入一个内点；从最近的一条起连成角平分方向，末点接远角。
            let mut previous = None;
            for (first, second) in first.into_iter().zip(second) {
                let middle = work.push_point((work.points[first] + work.points[second]) * 0.5)?;
                split(polygons, first, second, Some(middle), &work.points, false)?;
                if let Some(previous) = previous {
                    connect(polygons, previous, middle, &work.points, false)?;
                }
                previous = Some(middle);
            }
            connect(polygons, previous.expect("角切至少有一刀"), d, &work.points, false)?;
        }
        SubdivisionCorner::Fan => {
            for (first, second) in first.into_iter().zip(second) {
                connect(polygons, first, d, &work.points, false)?;
                connect(polygons, second, d, &work.points, false)?;
            }
        }
        SubdivisionCorner::StraightCut | SubdivisionCorner::Path => {
            for (first, second) in first.into_iter().zip(second) {
                connect(polygons, first, second, &work.points, false)?;
            }
            if pattern == SubdivisionCorner::Path {
                connect(polygons, a, c, &work.points, false)?;
            }
        }
    }
    Ok(())
}

fn non_collinear(face: &[usize], first: usize, second: usize, points: &[DVec3]) -> bool {
    let next = |index| (index + 1) % face.len();
    if next(first) != second && next(second) != first {
        return true;
    }
    let vector = |side| (points[face[next(side)]] - points[face[side]]).normalize_or_zero();
    vector(first).dot(vector(second)).abs() <= 0.99995
}

fn connect(
    polygons: &mut Vec<Vec<usize>>,
    a: usize,
    b: usize,
    points: &[DVec3],
    check_inside: bool,
) -> Result<(), String> {
    split(polygons, a, b, None, points, check_inside)
}

fn split(
    polygons: &mut Vec<Vec<usize>>,
    a: usize,
    b: usize,
    inside_vertex: Option<usize>,
    points: &[DVec3],
    check_inside: bool,
) -> Result<(), String> {
    let Some((index, low, high)) = polygons.iter().enumerate().find_map(|(index, polygon)| {
        let start = polygon.iter().position(|&vertex| vertex == a)?;
        let end = polygon.iter().position(|&vertex| vertex == b)?;
        let (low, high) = (start.min(end), start.max(end));
        (high - low > 1 && high - low < polygon.len() - 1).then_some((index, low, high))
    }) else {
        return Err("细分时无法定位原面上的连接线".into());
    };
    let polygon = &polygons[index];
    if check_inside && !internal_chord(polygon, low, high, points) {
        return Ok(()); // 凹多边形的跨缺口直线会穿出表面，保留当前合法边界。
    }
    let mut first = polygon[low..=high].to_vec();
    let mut second = polygon[high..].to_vec();
    second.extend_from_slice(&polygon[..=low]);
    if let Some(vertex) = inside_vertex {
        first.push(vertex);
        second.push(vertex);
    }
    polygons[index] = first;
    polygons.push(second);
    Ok(())
}

fn internal_chord(polygon: &[usize], low: usize, high: usize, points: &[DVec3]) -> bool {
    let a = points[polygon[low]];
    let (min, max) = polygon.iter().fold((a, a), |(min, max), &vertex| {
        (min.min(points[vertex]), max.max(points[vertex]))
    });
    let span = (max - min).max_element();
    if span <= 0.0 {
        return false;
    }
    let normalized: Vec<_> = polygon.iter().map(|&vertex| (points[vertex] - a) / span).collect();
    let area = (0..polygon.len()).fold(DVec3::ZERO, |sum, side| {
        sum + normalized[side].cross(normalized[(side + 1) % polygon.len()])
    });
    let axis = area.abs().max_element();
    let plane = |p: DVec3| {
        if area.x.abs() == axis { DVec2::new(p.y, p.z) }
        else if area.y.abs() == axis { DVec2::new(p.z, p.x) }
        else { DVec2::new(p.x, p.y) }
    };
    let projected: Vec<_> = normalized.into_iter().map(plane).collect();
    let end = projected[high];
    let middle = end * 0.5;
    let cross = |a: DVec2, b: DVec2| a.perp_dot(b);
    let mut inside = false;
    for side in 0..polygon.len() {
        let next = (side + 1) % polygon.len();
        let [p, q] = [projected[side], projected[next]];
        if (p.y > middle.y) != (q.y > middle.y) && middle.x < p.x + (middle.y - p.y) * (q.x - p.x) / (q.y - p.y) {
            inside = !inside;
        }
        if [side, next].contains(&low) || [side, next].contains(&high) {
            continue;
        }
        let crosses_chord = cross(end, p) * cross(end, q) < -1e-14;
        let crosses_side = cross(q - p, -p) * cross(q - p, end - p) < -1e-14;
        let touches_chord = [p, q].into_iter().any(|p| {
            cross(end, p).abs() <= 1e-8 && p.dot(p - end) <= 0.0
        });
        if (crosses_chord && crosses_side) || touches_chord {
            return false;
        }
    }
    inside
}
