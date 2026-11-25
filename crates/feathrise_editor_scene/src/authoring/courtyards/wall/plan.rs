//! 用建筑实际连接点表达连续墙段。一条直线只保存墙、门、墙三个连续区段。
use bevy::prelude::*;
use feathrise_scene_content::{
    BuildingPresetDescriptor, BuildingWorldQuad, ScenePlacementHeightMode, ScenePlacementRecord,
};
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct Module {
    pub preset: Arc<BuildingPresetDescriptor>,
    incoming: Vec3,
    yaw: f32,
    length: f32,
    pub kind: String,
}

impl Module {
    pub fn resolve(preset: &BuildingPresetDescriptor, kind: Option<&str>) -> Option<Self> {
        for a in &preset.connections {
            if kind.is_some_and(|kind| kind != a.kind) {
                continue;
            }
            for b in &preset.connections {
                if a.kind != b.kind
                    || (a.outward_yaw_degrees - b.outward_yaw_degrees).rem_euclid(360.0) != 180.0
                {
                    continue;
                }
                let axis = Quat::from_rotation_y(b.outward_yaw_degrees.to_radians()) * Vec3::Z;
                let delta = Vec3::new(
                    b.position[0] - a.position[0],
                    0.0,
                    b.position[1] - a.position[1],
                );
                let length = delta.dot(axis);
                if length > 0.0 && (delta - length * axis).length() <= length * 1e-5 {
                    return Some(Self {
                        preset: Arc::new(preset.clone()),
                        kind: a.kind.clone(),
                        length,
                        yaw: b.outward_yaw_degrees,
                        incoming: Vec3::new(a.position[0], 0., a.position[1]),
                    });
                }
            }
        }
        None
    }
}

#[derive(Clone)]
struct Run {
    module: Module,
    start: Vec3,
    step: Vec3,
    yaw: f32,
    count: usize,
    door: bool,
}
impl Run {
    fn root(&self, index: usize) -> Vec3 {
        self.start + self.step * index as f32
            - Quat::from_rotation_y(self.yaw.to_radians()) * self.module.incoming
    }
    fn footprint(&self, index: usize) -> BuildingWorldQuad {
        self.module
            .preset
            .world_footprint(self.root(index).to_array(), self.yaw, [1.; 3])
    }
}

#[derive(Default)]
pub(super) struct Plan {
    runs: Vec<Run>,
    legs: Vec<(Vec3, Vec3)>,
    pub count: usize,
}

impl Plan {
    pub fn build(
        points: &[Vec3],
        wall: &Module,
        gate: Option<(&Module, usize, f32)>,
    ) -> Result<Self, String> {
        let mut plan = Self::default();
        let Some(first) = points.first().copied() else {
            return Ok(plan);
        };
        for (leg, &point) in points.iter().skip(1).enumerate() {
            let (mut start, dir) = plan.next_start(first, point)?;
            let distance = (point - start).dot(dir);
            let door = gate.filter(|(_, index, _)| *index == leg);
            let door_length = door.map_or(0., |(module, ..)| module.length);
            if door.is_some() && distance + wall.length * 0.5 < door_length {
                return Err("这段围墙放不下所选门楼".into());
            }
            let cells = ((distance - door_length) / wall.length).round().max(0.) as usize;
            let length = cells as f32 * wall.length + door_length;
            if !length.is_finite() || length <= 0.0 {
                return Err("请把下一点放在至少半个墙段以外".into());
            }
            let before = door.map_or(cells, |(_, _, offset)| {
                ((offset - door_length * 0.5) / wall.length)
                    .round()
                    .clamp(0., cells as f32) as usize
            });
            let heading = dir.x.atan2(dir.z).to_degrees();
            let leg_start = start;
            let slope = (point.y - start.y) / length;
            let mut push = |module: &Module, count: usize, is_door: bool| -> Result<(), String> {
                if count == 0 {
                    return Ok(());
                }
                plan.count = plan
                    .count
                    .checked_add(count)
                    .ok_or("围墙长度超出可表示范围")?;
                let step = (dir + Vec3::Y * slope) * module.length;
                plan.runs.push(Run {
                    module: module.clone(),
                    start,
                    step,
                    count,
                    door: is_door,
                    yaw: (heading - module.yaw).rem_euclid(360.),
                });
                start += step * count as f32;
                Ok(())
            };
            push(wall, before, false)?;
            if let Some((module, ..)) = door {
                push(module, 1, true)?;
            }
            push(wall, cells - before, false)?;
            plan.legs.push((leg_start, start));
        }
        Ok(plan)
    }

    pub fn end(&self, first: Vec3) -> Vec3 {
        self.legs.last().map_or(first, |(_, end)| *end)
    }

    fn next_start(&self, first: Vec3, point: Vec3) -> Result<(Vec3, Vec3), &'static str> {
        let mut start = self.end(first);
        let delta = point - start;
        let dir = if delta.x.abs() >= delta.z.abs() {
            Vec3::X * delta.x.signum()
        } else {
            Vec3::Z * delta.z.signum()
        };
        if let Some(last) = self.runs.last() {
            let alignment = last.step.dot(dir);
            if alignment < 0.0 {
                return Err("墙段不能沿原路折返，请撤回上一点");
            }
            if alignment == 0.0 {
                // 导线与正式转角共用上一段实体侧面，两个模块在侧边相接。
                let edge = last
                    .footprint(last.count - 1)
                    .corners
                    .into_iter()
                    .map(|p| (p - start.xz()).dot(dir.xz()))
                    .fold(0.0, f32::max);
                start += dir * edge;
            }
        }
        Ok((start, dir))
    }

    pub fn guide(&self, first: Vec3, point: Vec3, wall: &Module) -> Option<(Vec3, Vec3)> {
        let (start, dir) = self.next_start(first, point).ok()?;
        let cells = ((point - start).dot(dir) / wall.length).round();
        (cells > 0.0).then(|| {
            (
                start,
                start + dir * (cells * wall.length) + Vec3::Y * (point.y - start.y),
            )
        })
    }

    pub fn nearest(&self, point: Vec3) -> Option<(usize, f32)> {
        self.legs
            .iter()
            .enumerate()
            .map(|(i, (a, b))| {
                let axis = (b.xz() - a.xz()).normalize();
                let offset = (point.xz() - a.xz())
                    .dot(axis)
                    .clamp(0.0, b.xz().distance(a.xz()));
                (
                    i,
                    offset,
                    (a.xz() + axis * offset).distance_squared(point.xz()),
                )
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(i, offset, _)| (i, offset))
    }

    pub fn draw(&self, gizmos: &mut Gizmos<crate::viewport::SceneEditorBrushGizmos>) {
        // 连续相同模块只画一条占地带，预览成本由点选的拐角数决定，与围墙长度无关。
        for run in &self.runs {
            let color = if run.door {
                Color::srgb(1., 0.7, 0.15)
            } else {
                Color::srgb(0.2, 0.9, 0.75)
            };
            let quad = run.footprint(0);
            let travel = run.step * (run.count - 1) as f32;
            let y = run.start.y + 0.10;
            let start = quad.corners.map(|v| Vec3::new(v.x, y, v.y));
            let end = start.map(|v| v + travel);
            for i in 0..4 {
                let j = (i + 1) % 4;
                gizmos.line(start[i], start[j], color);
                if run.count > 1 {
                    gizmos.line(end[i], end[j], color);
                    gizmos.line(start[i], end[i], color);
                }
            }
        }
    }

    pub fn candidates(
        &self,
        prefix: &str,
    ) -> Result<Vec<(ScenePlacementRecord, Option<Arc<BuildingPresetDescriptor>>)>, String> {
        let mut records = Vec::new();
        records
            .try_reserve_exact(self.count)
            .map_err(|_| "无法为当前围墙分配摆放记录".to_owned())?;
        for run in &self.runs {
            let preset = run.module.preset.as_ref();
            for index in 0..run.count {
                let record = ScenePlacementRecord {
                    id: format!("{prefix}-{}", records.len() + 1),
                    asset_path: preset.asset_path.clone(),
                    asset_target: preset.asset_target.clone(),
                    building_preset_id: Some(preset.id.clone()),
                    translation: run.root(index).to_array(),
                    yaw_degrees: run.yaw,
                    scale: [1.; 3],
                    height_mode: ScenePlacementHeightMode::FixedHeight,
                    terrain_offset: 0.0,
                    courtyard: None,
                    water_surfaces: Vec::new(),
                    editor: default(),
                    mesh_lod: None,
                    terrain_binding: None,
                    terrain_patch: None,
                };
                records.push((record, Some(run.module.preset.clone())));
            }
        }
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_scene_content::{BuildingConnectionPointDescriptor, BuildingFootprintRect};

    fn module(id: &str, half: f32) -> Module {
        let mut p = BuildingPresetDescriptor::from_asset(
            id.into(),
            id.into(),
            "models/piece.glb".into(),
            default(),
        );
        p.footprint = BuildingFootprintRect {
            center: [0.0; 2],
            half_extents: [half, 0.3],
        };
        for (x, yaw) in [(-half, 270.), (half, 90.)] {
            p.connections.push(BuildingConnectionPointDescriptor {
                id: format!("{x}"),
                kind: "test-wall".into(),
                position: [x, 0.],
                outward_yaw_degrees: yaw,
            });
        }
        Module::resolve(&p, None).unwrap()
    }

    #[test]
    fn wall_route_gate_and_corner_share_edges_without_overlapping() {
        let wall = module("wall", 1.5);
        let gate = module("gate", 3.);
        let points = [Vec3::ZERO, Vec3::X * 18., Vec3::new(18., 0., 15.)];
        let plan = Plan::build(&points, &wall, Some((&gate, 0, 9.))).unwrap();
        assert_eq!(plan.count, 10);
        let values = plan.candidates("test").unwrap();
        assert_eq!(
            values
                .iter()
                .filter(|(r, _)| r.building_preset_id.as_deref() == Some("gate"))
                .count(),
            1
        );
        let footprints: Vec<_> = values
            .iter()
            .map(|(r, p)| {
                p.as_ref()
                    .unwrap()
                    .world_footprint(r.translation, r.yaw_degrees, r.scale)
            })
            .collect();
        assert!(
            footprints
                .iter()
                .enumerate()
                .all(|(i, a)| footprints[..i].iter().all(|b| !a.overlaps(*b)))
        );
        let previous = footprints[4].aabb();
        let corner = footprints[5].aabb();
        assert!((previous.max.y - corner.min.y).abs() < 1e-4);
        assert_eq!(plan.nearest(Vec3::new(17.8, 0., 8.)).unwrap().0, 1);
    }

    #[test]
    fn wall_route_long_negative_run_stays_compressed_and_can_be_shortened() {
        let wall = module("short", 1.5);
        let start = Vec3::new(0., 3., 0.);
        let end = Vec3::new(-900., 6., 0.);
        let plan = Plan::build(&[start, end], &wall, None).unwrap();
        assert_eq!((plan.count, plan.runs.len()), (300, 1));
        let values = plan.candidates("long").unwrap();
        assert_eq!(values[0].0.yaw_degrees, 180.);
        assert!(values[299].0.translation[1] > values[0].0.translation[1]);
        assert!(Plan::build(&[start, end, start], &wall, None).is_err());
        assert!(Module::resolve(&wall.preset, Some("unrelated")).is_none());
    }
}
