//! 人形蒙皮权重。使用同部件焊接后的表面距离传播骨段影响，输出按原顶点顺序排列。

use super::{Geometry, Joint, VertexSkin};
use bevy::math::{IVec3, Vec3};
use std::{
    cmp::Ordering as Rank,
    collections::{BinaryHeap, HashMap},
    sync::atomic::{AtomicBool, Ordering},
};

const MAX_VERTICES: usize = 500_000;
const MAX_TRIANGLES: usize = 1_000_000;
const MAX_JOINTS: usize = 128;
const INFLUENCES: usize = 4;
const NO_VERTEX: u32 = u32::MAX;
const NO_JOINT: u16 = u16::MAX;

pub(super) fn solve(
    geometry: &Geometry,
    joints: &[Joint],
    cancel: &AtomicBool,
) -> Result<(Vec<VertexSkin>, Vec<String>), String> {
    check_cancel(cancel)?;
    let (minimum, height) = validate(geometry, joints, cancel)?;
    let epsilon = height * 0.00001;
    let welded = weld(geometry, minimum, epsilon, cancel)?;
    let graph = Graph::new(&welded, &geometry.triangles, cancel)?;
    let bones = Bones::new(joints, height);
    let labels = surface_labels(&welded.points, &graph, &bones, cancel)?;
    drop(graph);

    let mut fallback = vec![false; welded.points.len()];
    let mut unique_skins = Vec::with_capacity(welded.points.len());
    for (index, (&point, labels)) in welded.points.iter().zip(labels).enumerate() {
        checkpoint(index, cancel)?;
        let skin = if labels[0].joint == NO_JOINT {
            // 孤岛或过远的附件没有表面种子。回退有明确覆盖统计，仍保持左右约束。
            fallback[index] = true;
            bones.fallback(point)
        } else {
            bones.skin(point, &labels)
        };
        unique_skins.push(skin);
    }

    let mut unseeded = 0;
    let mut result = Vec::with_capacity(geometry.vertices.len());
    for (index, welded) in welded.remap.into_iter().enumerate() {
        checkpoint(index, cancel)?;
        let welded = welded as usize;
        unseeded += usize::from(fallback[welded]);
        let skin = &unique_skins[welded];
        result.push(VertexSkin {
            joints: skin.joints,
            weights: skin.weights,
        });
    }
    let warnings = (unseeded > 0)
        .then(|| format!("{unseeded} 个顶点缺少可达的表面种子，已使用骨段距离计算权重"))
        .into_iter()
        .collect();
    Ok((result, warnings))
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        return Err("绑骨任务已取消".into());
    }
    Ok(())
}

fn checkpoint(index: usize, cancel: &AtomicBool) -> Result<(), String> {
    if index % 1024 == 0 {
        check_cancel(cancel)?;
    }
    Ok(())
}

fn validate(
    geometry: &Geometry,
    joints: &[Joint],
    cancel: &AtomicBool,
) -> Result<(Vec3, f32), String> {
    if !(1..=MAX_VERTICES).contains(&geometry.vertices.len())
        || geometry.triangles.len() > MAX_TRIANGLES
        || !(1..=MAX_JOINTS).contains(&joints.len())
    {
        return Err("原生蒙皮最多支持 50 万顶点、100 万三角形和 128 个关节".into());
    }
    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    for (index, &point) in geometry.vertices.iter().enumerate() {
        checkpoint(index, cancel)?;
        if !point.is_finite() {
            return Err("模型包含无效的顶点坐标".into());
        }
        minimum = minimum.min(point);
        maximum = maximum.max(point);
    }
    let size = maximum - minimum;
    let height = size.y;
    if !size.is_finite() || !(1e-6..=1e8).contains(&height) || size.max_element() > height * 100.0 {
        return Err("模型范围无法用于人形权重计算，请检查站立姿态和单位".into());
    }
    for joint in joints {
        let relative = joint.position - minimum;
        if !relative.is_finite()
            || relative.abs().max_element() > height * 100.0
            || !joint.rotation.is_finite()
        {
            return Err("骨架包含无效的绑定姿态".into());
        }
        // 关节列表可以是任意顺序，父链必须有界并最终结束。
        let mut parent = joint.parent;
        for _ in 0..joints.len() {
            let Some(index) = parent else {
                break;
            };
            parent = joints.get(index).ok_or("骨架父级越界")?.parent;
        }
        if parent.is_some() {
            return Err("骨架父级形成循环".into());
        }
    }
    Ok((minimum, height))
}

struct Welded {
    points: Vec<Vec3>,
    remap: Vec<u32>,
    parts: Vec<usize>,
}

fn weld(
    geometry: &Geometry,
    minimum: Vec3,
    epsilon: f32,
    cancel: &AtomicBool,
) -> Result<Welded, String> {
    let len = geometry.vertices.len();
    let mut points: Vec<Vec3> = Vec::with_capacity(len);
    let mut remap = Vec::with_capacity(len);
    let mut parts = Vec::with_capacity(len);
    let mut next = Vec::with_capacity(len);
    for (part_index, part) in geometry.parts.iter().enumerate() {
        if part.start != remap.len() || part.start >= part.end || part.end > len {
            return Err("网格部件的顶点范围无效".into());
        }
        // UV 与材质切缝产生的位置副本应得到相同结果。部件之间保持隔离，
        // 避免贴身衣物、重叠配饰或相邻但独立的网格形成虚假的表面捷径。
        let mut bins = HashMap::<IVec3, u32>::with_capacity(part.len().min(32768));
        for index in part.clone() {
            checkpoint(index, cancel)?;
            let point = geometry.vertices[index];
            let grid = (point - minimum) / (epsilon * 2.0);
            let base = grid.floor();
            let cell = base.as_ivec3();
            let fraction = grid - base;
            let near = IVec3::new(
                if fraction.x < 0.5 { -1 } else { 1 },
                if fraction.y < 0.5 { -1 } else { 1 },
                if fraction.z < 0.5 { -1 } else { 1 },
            );
            // 格边为焊接容差的两倍，每轴只需查当前格与相邻的一格。
            let mut matched = NO_VERTEX;
            for x in [0, near.x] {
                for y in [0, near.y] {
                    for z in [0, near.z] {
                        let key = cell + IVec3::new(x, y, z);
                        let mut cursor = bins.get(&key).copied().unwrap_or(NO_VERTEX);
                        while cursor != NO_VERTEX {
                            if (points[cursor as usize] - point).abs().max_element() <= epsilon {
                                matched = matched.min(cursor);
                            }
                            cursor = next[cursor as usize];
                        }
                    }
                }
            }
            if matched == NO_VERTEX {
                matched = points.len() as u32;
                points.push(point);
                parts.push(part_index);
                next.push(bins.insert(cell, matched).unwrap_or(NO_VERTEX));
            }
            remap.push(matched);
        }
    }
    if remap.len() != len {
        return Err("网格部件没有覆盖全部顶点".into());
    }
    Ok(Welded {
        points,
        remap,
        parts,
    })
}

#[derive(Clone, Copy, Default)]
struct Edge {
    to: u32,
    length: f32,
}

struct Graph {
    offsets: Vec<usize>,
    edges: Vec<Edge>,
}

impl Graph {
    fn new(welded: &Welded, triangles: &[[u32; 3]], cancel: &AtomicBool) -> Result<Self, String> {
        // 用紧凑边表和 CSR 邻接。最多 300 万候选边和 600 万定向边。
        let mut keys = Vec::with_capacity(triangles.len() * 3);
        for (index, &[a, b, c]) in triangles.iter().enumerate() {
            checkpoint(index, cancel)?;
            let map = |vertex: u32| welded.remap.get(vertex as usize).copied();
            let (Some(a), Some(b), Some(c)) = (map(a), map(b), map(c)) else {
                return Err("蒙皮三角形的顶点索引越界".into());
            };
            let part = welded.parts[a as usize];
            if welded.parts[b as usize] != part || welded.parts[c as usize] != part {
                return Err("蒙皮三角形跨越两个网格部件".into());
            }
            if a == b || b == c || c == a {
                continue;
            }
            for (start, end) in [(a, b), (b, c), (c, a)] {
                let low = u64::from(start.min(end));
                let high = u64::from(start.max(end));
                keys.push((low << 32) | high);
            }
        }
        keys.sort_unstable();
        check_cancel(cancel)?;
        keys.dedup();
        let mut offsets = vec![0; welded.points.len() + 1];
        for (index, &key) in keys.iter().enumerate() {
            checkpoint(index, cancel)?;
            let (a, b) = edge_indices(key);
            offsets[a + 1] += 1;
            offsets[b + 1] += 1;
        }
        for index in 1..offsets.len() {
            offsets[index] += offsets[index - 1];
        }
        let mut cursor = offsets[..welded.points.len()].to_vec();
        let mut edges = vec![Edge::default(); keys.len() * 2];
        for (index, key) in keys.into_iter().enumerate() {
            checkpoint(index, cancel)?;
            let (a, b) = edge_indices(key);
            let length = welded.points[a].distance(welded.points[b]);
            edges[cursor[a]] = Edge {
                to: b as u32,
                length,
            };
            edges[cursor[b]] = Edge {
                to: a as u32,
                length,
            };
            cursor[a] += 1;
            cursor[b] += 1;
        }
        Ok(Self { offsets, edges })
    }

    fn neighbors(&self, vertex: usize) -> &[Edge] {
        &self.edges[self.offsets[vertex]..self.offsets[vertex + 1]]
    }
}

fn edge_indices(key: u64) -> (usize, usize) {
    ((key >> 32) as usize, key as u32 as usize)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Center,
    Left,
    Right,
}

impl Side {
    fn matches(self, other: Self) -> bool {
        self == Self::Center || other == Self::Center || self == other
    }
}

struct Midplane {
    center: Vec3,
    left: Vec3,
    tolerance: f32,
}

impl Midplane {
    fn new(joints: &[Joint], height: f32) -> Option<Self> {
        let position = |name| {
            joints
                .iter()
                .find(|joint| bone_name(&joint.name) == name)
                .map(|joint| joint.position)
        };
        let hips = position("LeftUpLeg").zip(position("RightUpLeg"));
        let (left, right) = position("LeftArm").zip(position("RightArm")).or(hips)?;
        // glTF 为 Y-up。肩高的微小偏差不能令左右分界在脚边倾斜。
        let lateral = left - right;
        let left_axis = Vec3::new(lateral.x, 0.0, lateral.z).try_normalize()?;
        let half_width = hips.map_or(lateral.dot(left_axis), |(a, b)| {
            (a - b).dot(left_axis).abs()
        }) * 0.18;
        Some(Self {
            center: position("Hips").unwrap_or((left + right) * 0.5),
            left: left_axis,
            tolerance: half_width.clamp(height * 0.0075, height * 0.025),
        })
    }

    fn across(&self, point: Vec3) -> f32 {
        (point - self.center).dot(self.left) / self.tolerance
    }

    fn side(&self, point: Vec3) -> Side {
        let across = self.across(point);
        if across >= 1.0 {
            Side::Left
        } else if across <= -1.0 {
            Side::Right
        } else {
            Side::Center
        }
    }
}

fn bone_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

struct Segment {
    joint: u16,
    start: Vec3,
    axis: Vec3,
    length: f32,
    support: f32,
    blend: f32,
    side: Side,
    finger: bool,
}

impl Segment {
    fn sample(&self, point: Vec3) -> (f32, f32) {
        let delta = point - self.start;
        let along = delta.dot(self.axis).clamp(0.0, self.length);
        let distance = (delta - self.axis * along).length_squared();
        let t = along / self.length.max(f32::MIN_POSITIVE);
        (distance, t)
    }
}

struct Bones {
    segments: Vec<Segment>,
    by_joint: [Option<usize>; MAX_JOINTS],
    midplane: Option<Midplane>,
    widest_blend: f32,
    rigid: u16,
}

impl Bones {
    fn new(joints: &[Joint], height: f32) -> Self {
        let mut segments = Vec::new();
        let mut by_joint = [None; MAX_JOINTS];
        for (index, joint) in joints.iter().enumerate() {
            let name = bone_name(&joint.name);
            let preferred = match name {
                "Hips" => Some("Spine"),
                "Spine2" => Some("Neck"),
                "LeftHand" => Some("LeftHandMiddle1"),
                "RightHand" => Some("RightHandMiddle1"),
                _ => None,
            };
            let children = joints.iter().filter(|child| child.parent == Some(index));
            let end = preferred
                .and_then(|name| {
                    children
                        .clone()
                        .find(|child| bone_name(&child.name) == name)
                })
                .or_else(|| {
                    children.max_by(|a, b| {
                        a.position
                            .distance_squared(joint.position)
                            .total_cmp(&b.position.distance_squared(joint.position))
                    })
                })
                .map(|child| child.position)
                .or_else(|| {
                    // 最末端的标记骨只定义长度；头、掌、脚趾根缺少标记时才由父骨外推。
                    matches!(
                        name,
                        "Head" | "LeftHand" | "RightHand" | "LeftToeBase" | "RightToeBase"
                    )
                    .then(|| {
                        joint.parent.map(|parent| {
                            joint.position + (joint.position - joints[parent].position) * 0.55
                        })
                    })
                    .flatten()
                });
            let Some(end) = end else {
                continue;
            };
            let axis = end - joint.position;
            let length = axis.length();
            if !length.is_finite() || length < height * 0.0001 {
                continue;
            }
            let finger =
                name.contains("Hand") && name.as_bytes().last().is_some_and(u8::is_ascii_digit);
            let torso = matches!(
                name,
                "Hips" | "Spine" | "Spine1" | "Spine2" | "Neck" | "Head"
            );
            let (support, blend) = if finger {
                // 指骨只能由贴近该段的网格提供种子，避免连指或饰物被远处细骨抢占。
                (
                    (length * 0.75).max(height * 0.008),
                    (length * 0.55).max(height * 0.008),
                )
            } else if torso {
                (
                    height * 0.18,
                    (length * 0.6).clamp(height * 0.04, height * 0.075),
                )
            } else {
                (
                    (length * 0.65).max(height * 0.035),
                    (length * 0.5).clamp(height * 0.022, height * 0.075),
                )
            };
            let side = if name.starts_with("Left") {
                Side::Left
            } else if name.starts_with("Right") {
                Side::Right
            } else {
                Side::Center
            };
            by_joint[index] = Some(segments.len());
            segments.push(Segment {
                joint: index as u16,
                start: joint.position,
                axis: axis / length,
                length,
                support,
                blend,
                side,
                finger,
            });
        }
        let rigid = joints
            .iter()
            .position(|joint| joint.parent.is_none())
            .unwrap_or(0) as u16;
        let widest_blend = segments
            .iter()
            .map(|segment| segment.blend)
            .fold(height * 0.025, f32::max);
        Self {
            segments,
            by_joint,
            midplane: Midplane::new(joints, height),
            widest_blend,
            rigid,
        }
    }

    fn side(&self, point: Vec3) -> Side {
        self.midplane
            .as_ref()
            .map_or(Side::Center, |plane| plane.side(point))
    }

    fn segment(&self, joint: u16) -> &Segment {
        &self.segments[self.by_joint[joint as usize].expect("权重标签必有骨段")]
    }

    fn seed(&self, point: Vec3, side: Side) -> Option<u16> {
        let mut nearest = None;
        for segment in self.segments.iter().filter(|bone| bone.side.matches(side)) {
            let (distance, t) = segment.sample(point);
            if segment.finger && distance > segment.support * segment.support {
                continue;
            }
            if nearest.is_none_or(|(_, closest, _)| distance < closest) {
                nearest = Some((segment, distance, t));
            }
        }
        let (segment, distance, t) = nearest?;
        // 只锚定骨段中部，关节两侧留空给沿表面的混合过渡。
        ((0.18..=0.82).contains(&t) && distance <= segment.support * segment.support)
            .then_some(segment.joint)
    }

    fn fallback(&self, point: Vec3) -> VertexSkin {
        let side = self.side(point);
        let mut labels = [Label::EMPTY; INFLUENCES];
        for segment in self.segments.iter().filter(|bone| bone.side.matches(side)) {
            let (distance, _) = segment.sample(point);
            if !segment.finger || distance <= segment.support * segment.support {
                insert_label(
                    &mut labels,
                    Label {
                        joint: segment.joint,
                        distance: distance.sqrt(),
                    },
                );
            }
        }
        self.skin(point, &labels)
    }

    fn skin(&self, point: Vec3, labels: &[Label; INFLUENCES]) -> VertexSkin {
        let nearest = labels[0];
        if nearest.joint == NO_JOINT {
            return VertexSkin {
                joints: [self.rigid, 0, 0, 0],
                weights: [1.0, 0.0, 0.0, 0.0],
            };
        }
        let base_width = self.segment(nearest.joint).blend;
        let across = self
            .midplane
            .as_ref()
            .map_or(0.0, |plane| plane.across(point));
        let mut values = [(0, 0.0_f32); INFLUENCES];
        for (slot, &label) in values.iter_mut().zip(labels) {
            if label.joint == NO_JOINT {
                break;
            }
            let bone = self.segment(label.joint);
            let width = base_width.max(bone.blend);
            let ramp = (1.0 - (label.distance - nearest.distance) / width).max(0.0);
            // 中缝允许两侧共同影响，但反侧权重在阻断前已降为零，避免衣摆出现硬边。
            let side = match bone.side {
                Side::Center => 1.0,
                Side::Left => (1.0 + across).clamp(0.0, 1.0),
                Side::Right => (1.0 - across).clamp(0.0, 1.0),
            };
            *slot = (label.joint, ramp * ramp * side);
        }
        values.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let total = values.iter().map(|value| value.1).sum::<f32>();
        let mut skin = VertexSkin {
            joints: [0; INFLUENCES],
            weights: [0.0; INFLUENCES],
        };
        for (index, (joint, weight)) in values.into_iter().enumerate() {
            if weight > 0.0 {
                skin.joints[index] = joint;
                skin.weights[index] = weight / total;
            }
        }
        skin
    }
}

#[derive(Clone, Copy, PartialEq)]
struct Label {
    joint: u16,
    distance: f32,
}

impl Label {
    const EMPTY: Self = Self {
        joint: NO_JOINT,
        distance: f32::INFINITY,
    };

    fn rank(self, other: Self) -> Rank {
        self.distance
            .total_cmp(&other.distance)
            .then_with(|| self.joint.cmp(&other.joint))
    }
}

fn insert_label(labels: &mut [Label; INFLUENCES], candidate: Label) -> bool {
    let mut slot = labels
        .iter()
        .position(|label| label.joint == candidate.joint)
        .unwrap_or(INFLUENCES - 1);
    if candidate.rank(labels[slot]) != Rank::Less {
        return false;
    }
    labels[slot] = candidate;
    while slot > 0 && candidate.rank(labels[slot - 1]) == Rank::Less {
        labels.swap(slot, slot - 1);
        slot -= 1;
    }
    true
}

#[derive(Clone, Copy, PartialEq)]
struct Frontier {
    vertex: u32,
    label: Label,
}

impl Eq for Frontier {}

impl Ord for Frontier {
    fn cmp(&self, other: &Self) -> Rank {
        other
            .label
            .rank(self.label)
            .then_with(|| other.vertex.cmp(&self.vertex))
    }
}

impl PartialOrd for Frontier {
    fn partial_cmp(&self, other: &Self) -> Option<Rank> {
        Some(self.cmp(other))
    }
}

fn surface_labels(
    points: &[Vec3],
    graph: &Graph,
    bones: &Bones,
    cancel: &AtomicBool,
) -> Result<Vec<[Label; INFLUENCES]>, String> {
    let sides: Vec<_> = points.iter().map(|point| bones.side(*point)).collect();
    let mut labels = vec![[Label::EMPTY; INFLUENCES]; points.len()];
    // 每点至多四个当前标签，队列到两倍上限时清掉已被更近标签替换的旧记录。
    let limit = points.len() * INFLUENCES * 2;
    let mut pending = BinaryHeap::with_capacity(limit);
    for (index, (&point, &side)) in points.iter().zip(&sides).enumerate() {
        checkpoint(index, cancel)?;
        if !graph.neighbors(index).is_empty()
            && let Some(joint) = bones.seed(point, side)
        {
            let label = Label {
                joint,
                distance: 0.0,
            };
            labels[index][0] = label;
            pending.push(Frontier {
                vertex: index as u32,
                label,
            });
        }
    }
    let mut steps = 0;
    while let Some(Frontier { vertex, label }) = pending.pop() {
        checkpoint(steps, cancel)?;
        steps += 1;
        if !labels[vertex as usize].contains(&label) {
            continue;
        }
        let side = bones.segment(label.joint).side;
        for edge in graph.neighbors(vertex as usize) {
            checkpoint(steps, cancel)?;
            steps += 1;
            let to = edge.to as usize;
            let distance = label.distance + edge.length;
            if !side.matches(sides[to]) || distance > labels[to][0].distance + bones.widest_blend {
                continue;
            }
            let next = Label {
                joint: label.joint,
                distance,
            };
            if insert_label(&mut labels[to], next) {
                if pending.len() == limit {
                    pending.retain(|entry| labels[entry.vertex as usize].contains(&entry.label));
                    check_cancel(cancel)?;
                }
                pending.push(Frontier {
                    vertex: edge.to,
                    label: next,
                });
            }
        }
    }
    Ok(labels)
}

#[cfg(test)]
#[path = "rigging_native_weights_tests.rs"]
mod tests;
