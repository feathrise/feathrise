//! 以不可变草稿执行点选、框选和画笔。每批返回完整结果与对应预览网格。

use super::{
    asset::Document,
    influences::VertexWeights,
    posed::PosedGeometry,
    spatial::{Occlusion, Points},
};
use crate::ui::asset_preview::rigging::files::active;
use crate::viewport::placement_move::MoveCamera;
use bevy::prelude::*;
use std::{
    collections::BTreeMap,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tool {
    #[default]
    Point,
    Box,
    Bone,
    Set,
    Add,
    Subtract,
    Smooth,
}

impl Tool {
    pub fn paint(self) -> bool {
        matches!(self, Self::Add | Self::Subtract | Self::Smooth)
    }
}

#[derive(Clone)]
pub(super) struct Brush {
    pub tool: Tool,
    pub joint: u16,
    pub radius: f32,
    pub strength: f32,
    pub visible_only: bool,
    pub mirror: bool,
    pub restrict_selection: bool,
    pub locks: Arc<[bool]>,
}

#[derive(Clone, Copy)]
pub(super) struct Selection {
    pub view: MoveCamera,
    pub start: Vec2,
    pub end: Vec2,
    pub point: bool,
    pub extend: bool,
    pub visible: bool,
}

pub(super) enum Operation {
    Refresh,
    Select(Selection, Option<PosedGeometry>),
    Paint {
        view: MoveCamera,
        dabs: Vec<Vec2>,
        brush: Brush,
    },
    Set {
        joint: u16,
        amount: f32,
        locks: Arc<[bool]>,
    },
    History(Arc<[VertexWeights]>),
}

pub(super) struct Prepared {
    pub surface: usize,
    pub weights: Arc<[VertexWeights]>,
    pub selection: Arc<[u32]>,
    pub shaded: Option<Mesh>,
    pub heat: Mesh,
    pub changed: usize,
}

fn visible(
    occlusion: &Occlusion,
    extent: f32,
    view: MoveCamera,
    screen: Vec2,
    world: Vec3,
) -> bool {
    view.ray(screen)
        .is_some_and(|ray| occlusion.visible(ray, world, (extent * 0.0002).max(0.00003)))
}

fn select(
    points: &Points,
    occlusion: Option<&Occlusion>,
    extent: f32,
    query: Selection,
) -> Vec<u32> {
    let Selection {
        view,
        start,
        end,
        point,
        ..
    } = query;
    let pixels = view.size * view.inverse_scale;
    let margin = if point {
        Vec2::splat(10.0) / pixels
    } else {
        Vec2::ZERO
    };
    let min = start.min(end) - margin;
    let max = start.max(end) + margin;
    let mut matches = Vec::new();
    let mut nearest: Option<(u32, f32, f32)> = None;
    points.projected(view, min, max, |index, screen, world| {
        if occlusion.is_some_and(|occlusion| !visible(occlusion, extent, view, screen, world)) {
            return;
        }
        if !point {
            matches.push(index);
            return;
        }
        let distance = ((screen - end) * pixels).length_squared();
        let depth = view
            .ray(screen)
            .map_or(f32::INFINITY, |ray| world.distance_squared(ray.origin));
        if distance <= 100.0
            && nearest.is_none_or(|(_, old_distance, old_depth)| {
                distance < old_distance || distance == old_distance && depth < old_depth
            })
        {
            nearest = Some((index, distance, depth));
        }
    });
    if let Some((index, _, _)) = nearest {
        matches.push(index);
    }
    matches
}

fn coverage(
    doc: &Document,
    surface: usize,
    view: MoveCamera,
    dabs: &[Vec2],
    brush: &Brush,
    selection: &[u32],
) -> BTreeMap<(u32, u16), f32> {
    let pixels = view.size * view.inverse_scale;
    let margin = Vec2::splat(brush.radius) / pixels;
    let min = dabs.iter().copied().reduce(Vec2::min).unwrap_or_default() - margin;
    let max = dabs.iter().copied().reduce(Vec2::max).unwrap_or_default() + margin;
    let surface = &doc.surfaces[surface];
    let mut hits = BTreeMap::<(u32, u16), f32>::new();
    let active_joint = &surface.skin.joints[brush.joint as usize];
    let opposite = active_joint
        .opposite
        .filter(|&other| surface.skin.joints[other as usize].opposite == Some(brush.joint));
    let mirror = surface.skin.mirror.filter(|_| brush.mirror);
    let tolerance = (doc.extent * 0.006).max(0.0001);
    surface
        .points
        .projected(view, min, max, |index, screen, world| {
            if brush.restrict_selection && selection.binary_search(&index).is_err() {
                return;
            }
            let distance = dabs
                .iter()
                .map(|dab| ((screen - *dab) * pixels).length_squared())
                .min_by(f32::total_cmp)
                .unwrap_or(f32::INFINITY)
                .sqrt();
            if distance >= brush.radius
                || brush.visible_only && !visible(&doc.occlusion, doc.extent, view, screen, world)
            {
                return;
            }
            let u = 1.0 - distance / brush.radius;
            let amount = brush.strength * u * u * (3.0 - 2.0 * u);
            let mut insert = |vertex, joint| {
                hits.entry((vertex, joint))
                    .and_modify(|old| *old = old.max(amount))
                    .or_insert(amount);
            };
            let Some(plane) = mirror else {
                insert(index, brush.joint);
                return;
            };
            let joint_side = (active_joint.rest - plane.center).dot(plane.axis);
            let vertex_side = (world - plane.center).dot(plane.axis);
            // 宽笔刷越过中线时按真实侧别选择骨骼，避免左右两种影响落到同一个点。
            let (direct, reflected) = match opposite {
                Some(other) if joint_side * vertex_side < 0.0 => (other, brush.joint),
                Some(other) => (brush.joint, other),
                None => (brush.joint, brush.joint),
            };
            insert(index, direct);
            if opposite.is_none() && joint_side.abs() > tolerance * 3.0 {
                return;
            }
            surface.points.reflected(index, plane, tolerance, |other| {
                if !brush.restrict_selection || selection.binary_search(&other).is_ok() {
                    insert(other, reflected);
                }
            });
        });
    hits
}

fn heat_colors(weights: &[VertexWeights], joint: u16, selection: &[u32]) -> Vec<[f32; 4]> {
    let colors = [
        Vec3::new(0.025, 0.050, 0.120),
        Vec3::new(0.015, 0.29, 0.9),
        Vec3::new(0.025, 0.78, 0.33),
        Vec3::new(1.0, 0.67, 0.025),
        Vec3::new(1.0, 0.05, 0.015),
    ];
    let mut selected = selection.iter().copied().peekable();
    weights
        .iter()
        .enumerate()
        .map(|(index, vertex)| {
            let value = vertex.weight(joint).clamp(0.0, 1.0) * 4.0;
            let base = (value as usize).min(3);
            let mut rgb = colors[base].lerp(colors[base + 1], value - base as f32);
            if selected.peek() == Some(&(index as u32)) {
                selected.next();
                rgb = rgb.lerp(Vec3::new(1.0, 0.74, 0.9), 0.55);
            }
            rgb.extend(1.0).to_array()
        })
        .collect()
}

pub(super) fn prepare(
    doc: &Document,
    surface: usize,
    before: Arc<[VertexWeights]>,
    mut selection: Arc<[u32]>,
    joint: u16,
    operation: Operation,
    cancel: &AtomicBool,
) -> Result<Prepared, String> {
    active(cancel)?;
    let geometry = doc.surfaces.get(surface).ok_or("选中的图元已失效")?;
    if joint as usize >= geometry.skin.joints.len() || before.len() != geometry.original.len() {
        return Err("权重草稿与当前蒙皮不匹配".into());
    }
    let mut weights = before.clone();
    let mut changed = 0;
    match operation {
        Operation::Refresh => {}
        Operation::Select(query, pose) => {
            let mut selected = if let Some(pose) = pose {
                let (points, occlusion) = pose.deform(doc, surface, query.visible, cancel)?;
                select(&points, occlusion.as_ref(), doc.extent, query)
            } else {
                select(
                    &geometry.points,
                    query.visible.then_some(&doc.occlusion),
                    doc.extent,
                    query,
                )
            };
            if query.extend {
                selected.extend_from_slice(&selection);
            }
            selected.sort_unstable();
            selected.dedup();
            selection = selected.into();
        }
        Operation::Set {
            joint,
            amount,
            locks,
        } => {
            let mut draft = before.to_vec();
            for group in selection.chunks(2048) {
                active(cancel)?;
                for &index in group {
                    let row = &mut draft[index as usize];
                    let next = row.edit(joint, amount, &locks);
                    changed += usize::from(next != *row);
                    *row = next;
                }
            }
            if changed > 0 {
                weights = draft.into();
            }
        }
        Operation::Paint { view, dabs, brush } => {
            let hits = coverage(doc, surface, view, &dabs, &brush, &selection);
            if !hits.is_empty() {
                let mut draft = before.to_vec();
                for (position, ((index, joint), amount)) in hits.into_iter().enumerate() {
                    if position % 1024 == 0 {
                        active(cancel)?;
                    }
                    let row = &mut draft[index as usize];
                    let old = row.weight(joint);
                    let goal = match brush.tool {
                        Tool::Add => old + amount,
                        Tool::Subtract => old - amount,
                        Tool::Smooth => {
                            let neighbors = geometry.points.neighbors(index);
                            if neighbors.is_empty() {
                                continue;
                            }
                            let mean = neighbors
                                .iter()
                                .map(|&neighbor| before[neighbor as usize].weight(joint))
                                .sum::<f32>()
                                / neighbors.len() as f32;
                            old + (mean - old) * amount
                        }
                        _ => continue,
                    };
                    let next = row.edit(joint, goal, &brush.locks);
                    changed += usize::from(next != *row);
                    *row = next;
                }
                if changed > 0 {
                    weights = draft.into();
                }
            }
        }
        Operation::History(previous) => {
            changed = before
                .iter()
                .zip(previous.iter())
                .filter(|(a, b)| a != b)
                .count();
            weights = previous;
        }
    }
    active(cancel)?;
    let heat = geometry.mesh(&weights, Some(heat_colors(&weights, joint, &selection)));
    let shaded = (!Arc::ptr_eq(&before, &weights) && !Arc::ptr_eq(&geometry.original, &weights))
        .then(|| geometry.mesh(&weights, None));
    Ok(Prepared {
        surface,
        weights,
        selection,
        shaded,
        heat,
        changed,
    })
}
