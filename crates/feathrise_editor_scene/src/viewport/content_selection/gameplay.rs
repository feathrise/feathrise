//! 玩法对象选择只读作者快照；柱体命中传送点，旋转墙段命中边界，不把区域内部当作墙。

use super::layers::{LayerCandidate, LayerTarget}; // 与其他类型共用稳定候选。
use crate::authoring::EditorSceneDraft; // 未保存作者源仍是唯一权威。
use bevy::prelude::*; // 纯几何计算无需ECS访问。
use feathrise_scene_content::{SceneBoundariesAsset, SceneBoundaryCapture}; // 作者规模不再等于单帧复制预算。
use feathrise_portal::{PORTAL_EDGE_BEAM_HEIGHT, PORTAL_VISUAL_RADIUS}; // 与正式视觉柱体尺寸一致。

#[derive(Clone, Default)]
pub(super) struct Snapshot {
    portals: Vec<(usize, String, String, Vec3)>, // 每批仅捕获选择所需字段。
    boundaries: Option<SceneBoundariesAsset>,      // 固定点数上限的小型文档。
    next: usize,                                 // 新票据从零开始，草稿修订变化整体丢弃。
    boundary_capture: SceneBoundaryCapture,        // 同一选择票据分帧捕获，不在点击帧复制整张墙表。
}

impl Snapshot {
    pub(super) fn visible_candidates(
        &self,
        ray: Ray3d,
        terrain: Option<Vec3>,
    ) -> Vec<LayerCandidate> {
        let limit = terrain.map(|world| world.distance(ray.origin) + 0.02); // 地形容差与其他真实对象一致。
        self.candidates(ray)
            .into_iter()
            .filter(|candidate| limit.is_none_or(|distance| candidate.distance <= distance))
            .collect() // 山后玩法对象不穿透命中。
    }
    pub(super) fn capture_batch(&mut self, draft: &EditorSceneDraft) -> bool {
        if self.boundaries.is_some() {
            return true;
        } // 完成后重复探测不再次take并清空已发布输入。
        let end = self
            .next
            .saturating_add(256)
            .min(draft.portals.portals.len()); // 不在点击帧克隆整张传送表。
        for index in self.next..end {
            let portal = &draft.portals.portals[index]; // 每帧最多256个小记录。
            self.portals.push((
                index,
                portal.id.clone(),
                portal.label.chars().take(128).collect(),
                Vec3::from_array(portal.position),
            )); // 列表显示名称有界，稳定ID不截断。
        }
        self.next = end; // 仅完整捕获后才允许后台几何查询。
        if end != draft.portals.portals.len() {
            return false;
        }
        if let Some(source) = draft.boundaries.as_ref() {
            if !self.boundary_capture.step(source, 4096, 64) {
                return false;
            }
            self.boundaries = self.boundary_capture.take();
        }
        true
    }

    pub(super) fn candidates(&self, ray: Ray3d) -> Vec<LayerCandidate> {
        let mut result = Vec::new(); // 后台计算全部候选，不人为截断命中数。
        for (index, id, label, position) in &self.portals {
            if let Some(distance) = portal_distance(ray, *position) {
                result.push(LayerCandidate {
                    author_index: Some(*index),
                    target: LayerTarget::Portal(id.clone()),
                    label: format!("传送点 · {label}"),
                    rank: 0,
                    distance,
                }); // 只选择，不执行传送。
            }
        }
        if let Some(boundaries) = self.boundaries.as_ref() {
            for (index, path) in boundaries.paths.iter().enumerate() {
                if !path.visible {
                    continue;
                } // 隐藏作者辅助线也不拦截视口选择；碰撞启用独立。
                let count = path.points.len(); // 开放路径不补末首段。
                let segments = count.saturating_sub(1) + usize::from(path.closed && count >= 3);
                let distance = (0..segments)
                    .filter_map(|i| {
                        boundary_distance(
                            ray,
                            Vec2::from_array(path.points[i]),
                            Vec2::from_array(path.points[(i + 1) % count]),
                            path.thickness,
                            path.bottom_y,
                            path.top_y,
                        )
                    })
                    .min_by(f32::total_cmp); // 同一路径只保留最近墙段。
                if let Some(distance) = distance {
                    result.push(LayerCandidate {
                        author_index: Some(index),
                        target: LayerTarget::Boundary(path.id.clone()),
                        label: format!("空气墙 · {}", path.label),
                        rank: 0,
                        distance,
                    }); // 内部区域不参与命中。
                }
            }
        }
        result
    }
}

fn portal_distance(ray: Ray3d, position: Vec3) -> Option<f32> {
    let origin = ray.origin - position; // 射线变换到正式传送柱体局部坐标。
    if !origin.is_finite() {
        return None;
    }
    let direction = *ray.direction; // 已归一化射线保留世界距离。
    let a = direction.x * direction.x + direction.z * direction.z;
    let b = origin.x * direction.x + origin.z * direction.z;
    let c = origin.x * origin.x + origin.z * origin.z - PORTAL_VISUAL_RADIUS * PORTAL_VISUAL_RADIUS;
    let (mut near, mut far) = (0.0_f32, f32::INFINITY); // 射线在柱体内时从零起算。
    if a < 1e-8 {
        if c > 0.0 {
            return None;
        } // 竖直射线在圆盘之外不会命中。
    } else {
        let discriminant = b * b - a * c;
        if discriminant < 0.0 {
            return None;
        }
        near = near.max((-b - discriminant.sqrt()) / a);
        far = far.min((-b + discriminant.sqrt()) / a);
    }
    if direction.y.abs() < 1e-8 {
        if origin.y < 0.0 || origin.y > PORTAL_EDGE_BEAM_HEIGHT {
            return None;
        }
    } else {
        let bottom = -origin.y / direction.y;
        let top = (PORTAL_EDGE_BEAM_HEIGHT - origin.y) / direction.y;
        near = near.max(bottom.min(top));
        far = far.min(bottom.max(top));
    }
    (far >= near && near.is_finite()).then_some(near) // 严格圆柱而非方包围盒四角。
}

fn boundary_distance(
    ray: Ray3d,
    start: Vec2,
    end: Vec2,
    thickness: f32,
    bottom: f32,
    top: f32,
) -> Option<f32> {
    let delta = end - start;
    let length = delta.length();
    if !length.is_finite()
        || length <= 0.0
        || !thickness.is_finite()
        || thickness <= 0.0
        || !bottom.is_finite()
        || !top.is_finite()
        || top <= bottom
    {
        return None;
    }
    let tangent = delta / length; // 按正式墙厚构造旋转局部盒。
    let normal = Vec2::new(-tangent.y, tangent.x);
    let center = (start + end) * 0.5;
    let xz = Vec2::new(ray.origin.x, ray.origin.z) - center;
    let direction_xz = Vec2::new(ray.direction.x, ray.direction.z);
    let origin = Vec3::new(
        xz.dot(tangent),
        ray.origin.y - (bottom + (top - bottom) * 0.5),
        xz.dot(normal),
    );
    let direction = Vec3::new(
        direction_xz.dot(tangent),
        ray.direction.y,
        direction_xz.dot(normal),
    );
    let half = Vec3::new(
        (length + thickness) * 0.5,
        (top - bottom) * 0.5,
        thickness * 0.5,
    ); // 两端各伸半厚，与正式碰撞封缝一致。
    let (mut near, mut far) = (0.0_f32, f32::INFINITY);
    for axis in 0..3 {
        if direction[axis].abs() < 1e-8 {
            if origin[axis].abs() > half[axis] {
                return None;
            }
        } else {
            let a = (-half[axis] - origin[axis]) / direction[axis];
            let b = (half[axis] - origin[axis]) / direction[axis];
            near = near.max(a.min(b));
            far = far.min(a.max(b));
        }
    }
    (far >= near && near.is_finite()).then_some(near)
}

#[cfg(test)]
mod tests {
    use super::*; // 无窗口回归真实形状和墙高。

    #[test]
    fn large_wall_snapshot_waits_for_complete_capture() {
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        let mut document = SceneBoundariesAsset::rectangular_default(
            &draft.scene_id,
            [-10_000.0; 2],
            [10_000.0; 2],
            -10.0,
            100.0,
        )
        .unwrap();
        document.paths[0].points = (0..9000).map(|index| [index as f32, 0.0]).collect();
        draft.boundaries = Some(document);
        let mut snapshot = Snapshot::default();
        assert!(!snapshot.capture_batch(&draft));
        assert!(snapshot.boundaries.is_none());
        assert!(!snapshot.capture_batch(&draft));
        assert!(snapshot.capture_batch(&draft));
        assert_eq!(
            snapshot.boundaries.as_ref().unwrap().paths[0].points.len(),
            9000
        ); // 预算不截断真实选择输入。
    }
    #[test]
    fn portal_rejects_box_corners_and_hits_vertical_disk() {
        let radius = PORTAL_VISUAL_RADIUS;
        assert!(
            portal_distance(
                Ray3d::new(Vec3::new(radius * 0.9, 50.0, radius * 0.9), Dir3::NEG_Y),
                Vec3::ZERO
            )
            .is_none()
        );
        assert!(portal_distance(Ray3d::new(Vec3::Y * 50.0, Dir3::NEG_Y), Vec3::ZERO).is_some());
    }
    #[test]
    fn boundary_hits_wall_not_interior_and_respects_vertical_range() {
        let hit = |p| {
            boundary_distance(
                Ray3d::new(p, Dir3::NEG_Y),
                Vec2::ZERO,
                Vec2::new(10.0, 10.0),
                2.0,
                0.0,
                20.0,
            )
        };
        assert!(hit(Vec3::new(5.0, 50.0, 5.0)).is_some());
        assert!(hit(Vec3::new(2.0, 50.0, 8.0)).is_none());
        assert!(hit(Vec3::new(5.0, -1.0, 5.0)).is_none());
    }

    #[test]
    fn overlapping_portals_remain_separate_and_closed_boundary_is_one_candidate() {
        let map = feathrise_scene_content::SceneId::parse("MAP-gameplay-pick").unwrap(); // 独立作者ID不按同位置合并。
        let snapshot = Snapshot {
            portals: vec![
                (0, "a".into(), "甲".into(), Vec3::ZERO),
                (1, "b".into(), "乙".into(), Vec3::ZERO),
            ],
            boundaries: Some(SceneBoundariesAsset {
                schema_version: feathrise_scene_content::SCENE_SCHEMA_VERSION,
                scene_id: map,
                paths: vec![feathrise_scene_content::SceneBoundaryPath {
                    role: feathrise_scene_content::SceneBoundaryRole::Outer, // 明确外框。
                    enabled: true,                                         // 碰撞默认启用。
                    visible: true,                                         // 辅助线默认显示。
                    id: "wall".into(),
                    label: "外墙".into(),
                    points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]],
                    closed: true,
                    bottom_y: 0.0,
                    top_y: 20.0,
                    thickness: 2.0,
                }],
            }),
            next: 2,
            boundary_capture: default(),
        };
        let hits = snapshot.candidates(Ray3d::new(Vec3::Y * 50.0, Dir3::NEG_Y)); // 墙角命中两段也只能得到一条路径。
        assert_eq!(hits.len(), 3);
        assert_eq!(
            hits.iter()
                .filter(|hit| matches!(hit.target, LayerTarget::Portal(_)))
                .count(),
            2
        );
        assert_eq!(
            hits.iter()
                .filter(|hit| matches!(hit.target, LayerTarget::Boundary(_)))
                .count(),
            1
        );
        assert!(
            snapshot
                .candidates(Ray3d::new(Vec3::new(5.0, 50.0, 5.0), Dir3::NEG_Y))
                .is_empty()
        ); // 封闭区域中央不是墙。
        assert!(
            snapshot
                .visible_candidates(
                    Ray3d::new(Vec3::Y * 50.0, Dir3::NEG_Y),
                    Some(Vec3::Y * 30.0)
                )
                .is_empty()
        ); // 更近地形挡住柱体与墙。
    }

    #[test]
    fn portal_snapshot_is_bounded_and_publishes_all_records_after_completion() {
        let mut draft = crate::authoring::test_heightfield_draft(std::sync::Arc::new(vec![0; 121]));
        draft.portals.portals = (0..600)
            .map(|index| feathrise_scene_content::ScenePortalRecord {
                terrain_binding: None,
                arrival_terrain_binding: None,
                id: format!("portal-{index}"),
                label: "传送点".into(),
                label_customized: true,
                position: [0.0; 3],
                arrival: [0.0; 3],
                target: feathrise_scene_content::ScenePortalTarget {
                    scene_id: draft.scene_id.clone(),
                    portal_id: None,
                },
                color_srgba: [1.0; 4],
                arrow_degrees: 0.0,
            })
            .collect();
        let mut snapshot = Snapshot::default();
        assert!(!snapshot.capture_batch(&draft));
        assert_eq!(snapshot.portals.len(), 256);
        assert!(!snapshot.capture_batch(&draft));
        assert_eq!(snapshot.portals.len(), 512);
        assert!(snapshot.capture_batch(&draft));
        assert_eq!(
            snapshot
                .candidates(Ray3d::new(Vec3::Y * 50.0, Dir3::NEG_Y))
                .len(),
            600
        ); // 完整发布，没有把预算当数据上限。
    }
}
