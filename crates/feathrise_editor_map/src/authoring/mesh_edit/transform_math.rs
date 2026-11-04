//! 点边面选择集的移动、旋转、缩放手柄与世界变换逆算；固定预算几何同时服务绘制与拾取。

use super::types::{MeshEditTool, MoveConstraint};
use crate::viewport::placement_move::{MoveAxis, MoveCamera};
use bevy::{
    math::{DQuat, DVec3},
    prelude::*,
};
use feathrise_map_content::MapPlacementRecord;

pub(super) const RING_SEGMENTS: usize = 64; // 四个圆环最多256段，不跟随地图规模或模型面数增长。
const PICK_PIXELS: f32 = 8.0;

pub(crate) struct MoveArrow {
    pub constraint: MoveConstraint,
    pub world: [Vec3; 4], // 起点、轴末端及两条箭翼端点；world和screen同时通过退化门。
    pub screen: [Vec2; 4],
}

pub(crate) struct RotationRing {
    pub constraint: MoveConstraint,
    pub world: [Vec3; RING_SEGMENTS + 1],
    pub screen: [Vec2; RING_SEGMENTS + 1],
    edge_on: bool,
}

pub(super) struct ScaleHandle {
    pub constraint: MoveConstraint,
    pub world: [Vec3; 6],
    pub screen: [Vec2; 6],
}

/// 三轴末端使用方帽，与移动箭头区分；中心方块控制整体倍率，绘制与拾取共享投影。
pub(super) fn scale_handles(camera: MoveCamera, origin: Vec3) -> [Option<ScaleHandle>; 3] {
    let length = camera.handle_length(origin);
    MoveAxis::ALL.map(|axis| {
        let length = length?;
        let (start, end) = camera.axis_segment(origin, axis, length)?;
        let tip = origin + axis.direction() * length;
        let corners = camera.screen_square(tip, 4.0)?;
        let world = [origin, tip, corners[0], corners[1], corners[2], corners[3]];
        let mut screen = [start, end, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO];
        for i in 2..6 {
            screen[i] = camera.project(world[i])? * camera.size * camera.inverse_scale;
        }
        Some(ScaleHandle {
            constraint: constraint(axis),
            world,
            screen,
        })
    })
}

/// 沿用对象缩放的指数拖动手感；单轴只取屏幕上的轴向分量，起点在中心也能立即操作。
pub(super) fn scale_factor(
    camera: MoveCamera,
    start: Vec2,
    current: Vec2,
    origin: Vec3,
    constraint: MoveConstraint,
    snapping: feathrise_editor_core::EditorSnapSettings,
    control: bool,
) -> Result<f32, String> {
    let delta = (current - start) * camera.size * camera.inverse_scale;
    let delta = if constraint == MoveConstraint::View {
        delta
    } else {
        let axis = scale_handles(camera, origin)
            .into_iter()
            .flatten()
            .find(|handle| handle.constraint == constraint)
            .ok_or("当前视角与缩放轴平行，请调整视角")?;
        Vec2::new(
            delta.dot((axis.screen[1] - axis.screen[0]).normalize()),
            0.0,
        )
    };
    crate::viewport::placement_move::group_scale_factor(
        delta,
        if snapping.active(control) {
            snapping.scale_factor
        } else {
            0.0
        },
        0.0001,
        10000.0,
    )
    .ok_or_else(|| "缩放倍率不是有限正数".into())
}

fn constraint(axis: MoveAxis) -> MoveConstraint {
    match axis {
        MoveAxis::X => MoveConstraint::X,
        MoveAxis::Y => MoveConstraint::Y,
        MoveAxis::Z => MoveConstraint::Z,
    }
}

/// 沿用完整对象手柄的80逻辑像素和端视轴拒绝门，但元素中心使用视图平面而非XZ平面。
pub(crate) fn move_arrows(camera: MoveCamera, origin: Vec3) -> [Option<MoveArrow>; 3] {
    let length = camera.handle_length(origin);
    MoveAxis::ALL.map(|axis| {
        let length = length?;
        let (start, end) = camera.axis_segment(origin, axis, length)?;
        let direction = axis.direction();
        let tip = origin + direction * length;
        let view = *camera.ray(camera.project(tip)?)?.direction;
        let side = direction.cross(view).try_normalize()?;
        let world = [
            origin,
            tip,
            tip - direction * length * 0.16 + side * length * 0.07,
            tip - direction * length * 0.16 - side * length * 0.07,
        ];
        let logical = camera.size * camera.inverse_scale;
        let mut screen = [start, end, Vec2::ZERO, Vec2::ZERO];
        screen[2] = camera.project(world[2])? * logical;
        screen[3] = camera.project(world[3])? * logical;
        Some(MoveArrow {
            constraint: constraint(axis),
            world,
            screen,
        })
    })
}

fn rotation_axis(camera: MoveCamera, constraint: MoveConstraint) -> Option<Vec3> {
    constraint
        .axis()
        .or_else(|| camera.ray(Vec2::ZERO).map(|ray| *ray.direction))
}

/// XYZ环使用相同半径，视图环稍大；侧视收成细线时仍绘制和拾取，拖动改用屏幕切向。
pub(crate) fn rotation_rings(camera: MoveCamera, origin: Vec3) -> [Option<RotationRing>; 4] {
    let radius = camera.handle_length(origin).map(|length| length * 0.9);
    [
        MoveConstraint::X,
        MoveConstraint::Y,
        MoveConstraint::Z,
        MoveConstraint::View,
    ]
    .map(|constraint| {
        let radius = radius?
            * if constraint == MoveConstraint::View {
                1.2
            } else {
                1.0
            };
        let axis = rotation_axis(camera, constraint)?;
        let seed = if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        let u = (seed - axis * axis.dot(seed)).try_normalize()?;
        let v = axis.cross(u).try_normalize()?;
        let logical = camera.size * camera.inverse_scale;
        let mut world = [Vec3::ZERO; RING_SEGMENTS + 1];
        let mut screen = [Vec2::ZERO; RING_SEGMENTS + 1];
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for index in 0..=RING_SEGMENTS {
            let angle = index as f32 / RING_SEGMENTS as f32 * std::f32::consts::TAU;
            world[index] = origin + (u * angle.cos() + v * angle.sin()) * radius;
            screen[index] = camera.project(world[index])? * logical;
            min = min.min(screen[index]);
            max = max.max(screen[index]);
        }
        // 面积/最长轴识别侧视，包括斜对角细线；只切换拖动算法，不能让整条轴突然消失。
        let center = camera.project(origin)? * logical;
        let twice_area: f32 = screen
            .windows(2)
            .map(|segment| (segment[0] - center).perp_dot(segment[1] - center))
            .sum();
        let major = (max - min).max_element();
        if major < 16.0 {
            return None;
        }
        Some(RotationRing {
            constraint,
            world,
            screen,
            edge_on: twice_area.abs() / major < 12.0,
        })
    })
}

fn segment_distance(point: Vec2, a: Vec2, b: Vec2) -> f32 {
    let edge = b - a;
    if edge.length_squared() < 1e-6 {
        return f32::INFINITY;
    }
    point.distance(a + edge * ((point - a).dot(edge) / edge.length_squared()).clamp(0.0, 1.0))
}

pub(crate) fn pick_transform_handle(
    tool: MeshEditTool,
    camera: MoveCamera,
    origin: Vec3,
    cursor: Vec2,
) -> Option<MoveConstraint> {
    if !cursor.is_finite() {
        return None;
    }
    let logical = camera.size * camera.inverse_scale;
    let point = cursor * logical;
    let mut nearest = None;
    match tool {
        MeshEditTool::Select => None,
        MeshEditTool::Move => {
            let center = camera.project(origin)? * logical;
            if (point - center).abs().max_element() <= 7.0 {
                return Some(MoveConstraint::View);
            }
            for arrow in move_arrows(camera, origin).into_iter().flatten() {
                let [a, b, c, d] = arrow.screen;
                let start = a.lerp(b, (12.0 / a.distance(b)).min(1.0));
                let distance = segment_distance(point, start, b)
                    .min(segment_distance(point, b, c))
                    .min(segment_distance(point, b, d));
                if distance <= PICK_PIXELS && nearest.is_none_or(|(_, best)| distance < best) {
                    nearest = Some((arrow.constraint, distance));
                }
            }
            nearest.map(|(constraint, _)| constraint)
        }
        MeshEditTool::Rotate => {
            // 侧视圆环穿过轴心时给骨节/中心选择留空，避免细线抢走中心点击。
            if point.distance(camera.project(origin)? * logical) < 12.0 {
                return None;
            }
            for ring in rotation_rings(camera, origin).into_iter().flatten() {
                let distance = ring
                    .screen
                    .windows(2)
                    .map(|segment| segment_distance(point, segment[0], segment[1]))
                    .fold(f32::INFINITY, f32::min);
                if distance <= PICK_PIXELS && nearest.is_none_or(|(_, best)| distance < best) {
                    nearest = Some((ring.constraint, distance));
                }
            }
            nearest.map(|(constraint, _)| constraint)
        }
        MeshEditTool::Scale => {
            let center = camera.project(origin)? * logical;
            if (point - center).abs().max_element() <= 8.0 {
                return Some(MoveConstraint::View);
            }
            for handle in scale_handles(camera, origin).into_iter().flatten() {
                let [a, b, c, d, e, f] = handle.screen;
                let start = a.lerp(b, (12.0 / a.distance(b)).min(1.0));
                let distance = segment_distance(point, start, b)
                    .min(segment_distance(point, c, d))
                    .min(segment_distance(point, d, e))
                    .min(segment_distance(point, e, f))
                    .min(segment_distance(point, f, c));
                if distance <= PICK_PIXELS && nearest.is_none_or(|(_, best)| distance < best) {
                    nearest = Some((handle.constraint, distance));
                }
            }
            nearest.map(|(constraint, _)| constraint)
        }
    }
}

/// 正常视角在冻结的世界平面求有向角；侧视改用屏幕切向，避免求交退化。
pub(crate) fn rotation_delta(
    camera: MoveCamera,
    start: Vec2,
    current: Vec2,
    origin: Vec3,
    constraint: MoveConstraint,
) -> Result<(Vec3, f32), String> {
    let ring = rotation_rings(camera, origin)
        .into_iter()
        .flatten()
        .find(|ring| ring.constraint == constraint)
        .ok_or("旋转环越过裁剪面，请调整视角后重试")?;
    let axis = rotation_axis(camera, constraint).ok_or("无法计算旋转轴")?;
    if !start.is_finite() || !current.is_finite() {
        return Err("旋转指针不在有效视口内".into());
    }
    let center = camera.project(origin).ok_or("旋转中心不在有效视口内")?;
    let logical = camera.size * camera.inverse_scale;
    if ((start - center) * logical).length() < 8.0 {
        return Err("指针过于接近旋转轴心".into());
    }
    if ring.edge_on {
        // 射线与旋转平面近平行时求交会失效或跳角。冻结相机前侧的切向，
        // 按72逻辑像素一弧度连续拖动；即使环完全重合为直线，也能操作该轴。
        let view = camera.ray(center).ok_or("无法计算旋转视线")?;
        let tangent = axis
            .cross(-*view.direction)
            .try_normalize()
            .ok_or("无法计算旋转切向")?;
        let radius = camera.handle_length(origin).ok_or("无法计算旋转半径")? * 0.9;
        let tip = camera
            .project(origin + tangent * radius)
            .ok_or("旋转切向越过裁剪面")?;
        let direction = ((tip - center) * logical)
            .try_normalize()
            .ok_or("无法计算旋转切向")?;
        let radians = ((current - start) * logical).dot(direction) / 72.0;
        return if radians.is_finite() {
            Ok((axis, radians))
        } else {
            Err("旋转角度不是有限数值".into())
        };
    }
    let point = |cursor| -> Result<Vec3, String> {
        let ray = camera.ray(cursor).ok_or("旋转指针不在有效视口内")?;
        let denominator = ray.direction.dot(axis);
        if denominator.abs() < 0.02 {
            return Err("旋转平面接近端视，请调整视角后重试".into());
        }
        let distance = (origin - ray.origin).dot(axis) / denominator;
        if !distance.is_finite() || distance < 0.0 {
            return Err("旋转平面位于相机后方".into());
        }
        (ray.get_point(distance) - origin)
            .try_normalize()
            .ok_or_else(|| "请从旋转环开始拖动，避免旋转轴心".into())
    };
    if ((current - center) * logical).length() < 8.0 {
        return Err("指针过于接近旋转轴心".into());
    }
    let a = point(start)?;
    let b = point(current)?;
    let radians = axis.dot(a.cross(b)).atan2(a.dot(b));
    if !radians.is_finite() {
        return Err("旋转角度不是有限数值".into());
    }
    Ok((axis, radians))
}

/// 选择点先在世界空间围绕共同枢轴转动，再逆实例yaw与非均匀scale写回本地共享点。
pub(super) fn rotated_record(
    before: &MapPlacementRecord,
    mask: u64,
    pivot: Vec3,
    axis: Vec3,
    radians: f32,
) -> Result<MapPlacementRecord, String> {
    if !pivot.is_finite() || !axis.is_finite() || !radians.is_finite() {
        return Err("旋转参数不是有限数值".into());
    }
    let Some(axis) = axis.try_normalize() else {
        return Err("旋转轴无效".into());
    };
    let mut geometry = before
        .asset_target
        .editable_geometry()
        .ok_or("当前对象不支持点边面旋转")?;
    let original = geometry.vertices_half_mm;
    if mask == 0 || radians.abs() <= 1e-7 {
        return Ok(before.clone());
    }
    if mask.count_ones() == 1 {
        let local = Vec3::from_array(
            original[mask.trailing_zeros() as usize].map(|value| value as f32 * 0.0005),
        );
        let point = Vec3::from_array(before.translation)
            + Quat::from_rotation_y(before.yaw_degrees.to_radians())
                * (local * Vec3::from_array(before.scale));
        if point.distance_squared(pivot) <= 1e-12 {
            return Ok(before.clone());
        }
    }
    let scale = Vec3::from_array(before.scale);
    if !scale.is_finite() || scale.min_element() <= 0.0 || !before.yaw_degrees.is_finite() {
        return Err("实例旋转或缩放无效".into());
    }
    let pivot = pivot.as_dvec3();
    let rotation = DQuat::from_axis_angle(axis.as_dvec3(), f64::from(radians));
    let root = DQuat::from_rotation_y(f64::from(before.yaw_degrees.to_radians()));
    let scale = scale.as_dvec3();
    let origin = Vec3::from_array(before.translation).as_dvec3();
    if !origin.is_finite() {
        return Err("实例原点无效".into());
    }
    let mut transformed = original;
    for (index, vertex) in transformed
        .iter_mut()
        .take(geometry.vertex_count as usize)
        .enumerate()
    {
        if mask & (1 << index) == 0 {
            continue;
        }
        let local = DVec3::from_array(original[index].map(f64::from)) * 0.0005;
        let world = origin + root * (local * scale);
        let rotated = pivot + rotation * (world - pivot);
        let local = (root.conjugate() * (rotated - origin)) / scale * 2_000.0;
        for coordinate in 0..3 {
            let value = local[coordinate].round();
            if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
                return Err("旋转后的顶点超过可保存范围".into());
            }
            vertex[coordinate] = value as i32;
        }
    }
    if transformed == original {
        return Ok(before.clone());
    }
    let mut after = before.clone();
    geometry.vertices_half_mm = transformed;
    after.asset_target = before.asset_target.with_editable_geometry(geometry)?;
    after.asset_target.validate()?; // 旋转量化后的完整闭壳仍需通过原非退化、非翻转和自交保护。
    Ok(after)
}

/// 世界坐标围绕选区中心缩放，再逆实例非均匀变换；只改变共享点，不改对象scale。
pub(super) fn scaled_record(
    before: &MapPlacementRecord,
    mask: u64,
    pivot: Vec3,
    constraint: MoveConstraint,
    factor: f32,
) -> Result<MapPlacementRecord, String> {
    if !factor.is_finite() || factor <= 0.0 || !pivot.is_finite() {
        return Err("缩放倍率必须是有限正数".into());
    }
    if mask == 0 || (factor - 1.0).abs() < 1e-7 {
        return Ok(before.clone());
    }
    let mut geometry = before
        .asset_target
        .editable_geometry()
        .ok_or("当前对象不支持元素缩放")?;
    let original = geometry;
    let scale = Vec3::from_array(before.scale).as_dvec3();
    let root = DQuat::from_rotation_y(f64::from(before.yaw_degrees.to_radians()));
    let origin = Vec3::from_array(before.translation).as_dvec3();
    if !scale.is_finite() || scale.min_element() <= 0.0 || !root.is_finite() || !origin.is_finite()
    {
        return Err("实例变换无效".into());
    }
    if mask.count_ones() == 1 {
        // 枢轴来自显示/拾取使用的f32世界点；另算f64再转回f32可能差一ULP，
        // 在小实例倍率下会放大成半毫米几何改动。单点门复用同源点，且保留上方非法变换校验。
        if super::geometry::world_vertices(before)
            .and_then(|vertices| vertices.get(mask.trailing_zeros() as usize).copied())
            .is_some_and(|point| point.distance_squared(pivot) <= 1e-12)
        {
            return Ok(before.clone());
        }
    }
    let factors = constraint
        .axis()
        .map_or(DVec3::splat(f64::from(factor)), |axis| {
            DVec3::ONE + axis.as_dvec3() * f64::from(factor - 1.0)
        });
    let pivot = pivot.as_dvec3();
    for (index, point) in geometry
        .vertices_half_mm
        .iter_mut()
        .take(geometry.vertex_count as usize)
        .enumerate()
    {
        if mask & (1u64 << index) == 0 {
            continue;
        }
        let local = DVec3::from_array(point.map(f64::from)) * 0.0005;
        let world = origin + root * (local * scale);
        let transformed = pivot + (world - pivot) * factors;
        let local = root.conjugate() * (transformed - origin) / scale * 2000.0;
        for axis in 0..3 {
            let value = local[axis].round();
            if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
                return Err("缩放后的顶点超过可保存范围".into());
            }
            point[axis] = value as i32;
        }
    }
    if geometry == original {
        return Ok(before.clone());
    }
    let mut after = before.clone();
    after.asset_target = before.asset_target.with_editable_geometry(geometry)?;
    after.asset_target.validate()?;
    Ok(after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        authoring::mesh_edit::geometry::{selection_center, world_vertices},
        resources::{MapEditorState, MapEditorViewportState},
    };
    use feathrise_map_content::MapPlacementAssetTarget;

    fn record() -> MapPlacementRecord {
        serde_json::from_value(serde_json::json!({
            "id":"element-rotate", "asset_path":"", "asset_target":MapPlacementAssetTarget::DEFAULT_CUBOID,
            "translation":[3,2,-4], "yaw_degrees":37, "scale":[1.2,2.1,0.7]
        })).unwrap()
    }

    fn camera() -> MoveCamera {
        MoveCamera::new(
            &MapEditorState {
                orthographic: true,
                ..default()
            },
            &MapEditorViewportState {
                _image: default(),
                camera: Entity::PLACEHOLDER,
                center: Vec3::ZERO,
                scene_center: Vec3::ZERO,
                scene_half_extents: Vec3::splat(4.0),
                extent: 8.0,
                yaw: 0.0,
                pitch: 0.0,
                framed_map_id: None,
                render_size: UVec2::new(800, 600),
            },
            1.0,
        )
        .unwrap()
    }

    #[test]
    fn group_rotation_preserves_unselected_shared_points_and_inverse_root_transform() {
        let before = record();
        let mask = 0b1100_1100; // 同一上表面的四个共享点，而非各面复制的显示顶点。
        let points = world_vertices(&before).unwrap();
        let pivot = selection_center(&points, mask).unwrap();
        let angle = 0.12;
        let after = rotated_record(&before, mask, pivot, Vec3::Y, angle).unwrap();
        let result = world_vertices(&after).unwrap();
        let old_local = before.asset_target.editable_vertices_half_mm().unwrap();
        let new_local = after.asset_target.editable_vertices_half_mm().unwrap();
        for index in 0..8 {
            if mask & (1 << index) == 0 {
                assert_eq!(old_local[index], new_local[index]);
            } else {
                let expected = pivot + Quat::from_rotation_y(angle) * (points[index] - pivot);
                assert!(
                    result[index].distance(expected) < 0.0015,
                    "非均匀缩放的逆算必须保持世界旋转"
                );
            }
        }
        assert_eq!(after.translation, before.translation);
        assert_eq!(after.yaw_degrees, before.yaw_degrees);
        assert_eq!(after.scale, before.scale);
        assert_eq!(after.editor, before.editor);
        assert_eq!(after.id, before.id);
        let mut restored_source = after.clone();
        restored_source.asset_target = before.asset_target;
        assert_eq!(
            restored_source, before,
            "点组旋转只能替换几何来源，不能改写实例姿态或其他作者元数据"
        );
        assert!(after.asset_target.validate().is_ok());
    }

    #[test]
    fn world_axis_scale_preserves_root_and_unselected_points_under_nonuniform_instance() {
        let before = record();
        let points = world_vertices(&before).unwrap();
        let mask = 0b1100_1100;
        let pivot = selection_center(&points, mask).unwrap();
        let after = scaled_record(&before, mask, pivot, MoveConstraint::X, 1.15).unwrap();
        let result = world_vertices(&after).unwrap();
        let old = before.asset_target.editable_geometry().unwrap();
        let new = after.asset_target.editable_geometry().unwrap();
        for index in 0..points.len() {
            if mask & (1 << index) == 0 {
                assert_eq!(old.vertices()[index], new.vertices()[index]);
            } else {
                let expected = pivot + (points[index] - pivot) * Vec3::new(1.15, 1.0, 1.0);
                assert!(result[index].distance(expected) < 0.0015);
            }
        }
        // 世界单轴缩放必须逆算到共享点，实例的姿态与作者元数据仍是原记录。
        let mut restored = after.clone();
        restored.asset_target = before.asset_target;
        assert_eq!(restored, before);
        after.asset_target.validate().unwrap();
    }

    #[test]
    fn scale_noop_preserves_recipe_and_rejects_zero_negative_or_nonfinite_factors() {
        let before = record();
        let point = world_vertices(&before).unwrap()[7];
        assert_eq!(
            scaled_record(&before, 1 << 7, point, MoveConstraint::View, 2.0).unwrap(),
            before
        );
        assert_eq!(
            scaled_record(&before, 255, point, MoveConstraint::X, 1.0).unwrap(),
            before
        );
        for factor in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(scaled_record(&before, 255, point, MoveConstraint::View, factor).is_err());
        }
    }

    #[test]
    fn single_point_scale_uses_the_displayed_pivot_at_large_origin_and_small_instance_scale() {
        let mut before = record();
        before.translation = [100.0, 2.0, -4.0];
        before.yaw_degrees = 0.0;
        before.scale = [0.01, 2.1, 0.7];
        before.asset_target = MapPlacementAssetTarget::Cuboid {
            size_mm: [9_832, 2_000, 2_000], color_srgb: [30, 90, 180],
            roughness_milli: 250, collision: false,
        };
        before.validated_transform(
            &feathrise_map_content::MapId::parse("MAP-single-point-scale").unwrap(),
            Rect::from_corners(Vec2::splat(-10.0), Vec2::splat(10.0)),
        ).unwrap();
        let points = world_vertices(&before).unwrap();
        for index in 0..points.len() {
            let mask = 1 << index;
            // 真实会话从显示/拾取共用的f32世界点求枢轴；不能用另一条f64路径把同一点误判成偏心缩放。
            let pivot = selection_center(&points, mask).unwrap();
            for constraint in [MoveConstraint::View, MoveConstraint::X, MoveConstraint::Y, MoveConstraint::Z] {
                for factor in [0.5, 2.0] {
                    assert_eq!(
                        scaled_record(&before, mask, pivot, constraint, factor).unwrap(),
                        before,
                        "单点{index}绕自身中心按{constraint:?}缩放{factor}倍必须保留完整参数配方",
                    );
                }
            }
        }
    }

    #[test]
    fn single_point_scale_noop_still_rejects_invalid_instance_transform() {
        let before = record();
        let pivot = world_vertices(&before).unwrap()[7];
        for value in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut invalid = before.clone();
            invalid.scale[0] = value;
            assert!(scaled_record(&invalid, 1 << 7, pivot, MoveConstraint::View, 2.0).is_err());
        }
        let mut invalid = before.clone();
        invalid.yaw_degrees = f32::NAN;
        assert!(scaled_record(&invalid, 1 << 7, pivot, MoveConstraint::View, 2.0).is_err());
        let mut invalid = before;
        invalid.translation[0] = f32::INFINITY;
        assert!(scaled_record(&invalid, 1 << 7, pivot, MoveConstraint::View, 2.0).is_err());
    }

    #[test]
    fn scale_handle_pick_and_snapping_use_same_logical_axis() {
        let camera = camera();
        let handles = scale_handles(camera, Vec3::ZERO);
        assert!(handles[2].is_none());
        let handle = handles[0].as_ref().unwrap();
        let tip = handle.screen[1] / (camera.size * camera.inverse_scale);
        assert_eq!(
            pick_transform_handle(MeshEditTool::Scale, camera, Vec3::ZERO, tip),
            Some(MoveConstraint::X)
        );
        let settings = feathrise_editor_core::EditorSnapSettings {
            scale_factor: 0.25,
            ..default()
        };
        let factor = scale_factor(
            camera,
            Vec2::ZERO,
            Vec2::new(0.21, 0.13),
            Vec3::ZERO,
            MoveConstraint::X,
            settings,
            true,
        )
        .unwrap();
        assert!((factor * 4.0 - (factor * 4.0).round()).abs() < 1e-5);
        assert!(
            scale_factor(
                camera,
                Vec2::ZERO,
                Vec2::X,
                Vec3::ZERO,
                MoveConstraint::Z,
                settings,
                false
            )
            .is_err()
        );
    }

    #[test]
    fn single_point_self_rotation_and_zero_angle_preserve_parameter_recipe() {
        let before = record();
        let point = world_vertices(&before).unwrap()[7];
        assert_eq!(
            rotated_record(&before, 1 << 7, point, Vec3::Y, 1.2).unwrap(),
            before
        );
        assert_eq!(
            rotated_record(&before, u64::from(u8::MAX), Vec3::ZERO, Vec3::Z, 0.0).unwrap(),
            before
        );
        assert!(rotated_record(&before, 3, point, Vec3::ZERO, 1.2).is_err());
    }

    #[test]
    fn rotation_direction_and_pick_share_the_drawn_ring_including_edge_on_axes() {
        let camera = camera();
        let rings = rotation_rings(camera, Vec3::ZERO);
        assert!(rings.iter().all(Option::is_some), "侧视旋转环必须保留");
        let ring = rings[2].as_ref().unwrap();
        let cursor = ring.screen[9] / (camera.size * camera.inverse_scale);
        assert_eq!(
            pick_transform_handle(MeshEditTool::Rotate, camera, Vec3::ZERO, cursor),
            Some(MoveConstraint::Z)
        );
        let start = camera.project(Vec3::X).unwrap();
        let end = camera.project(Vec3::Y).unwrap();
        let (axis, radians) =
            rotation_delta(camera, start, end, Vec3::ZERO, MoveConstraint::Z).unwrap();
        assert_eq!(axis, Vec3::Z);
        assert!((radians - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        let (_, reverse) =
            rotation_delta(camera, end, start, Vec3::ZERO, MoveConstraint::Z).unwrap();
        assert!((reverse + radians).abs() < 1e-5);
        assert!(
            rotation_delta(camera, start, end, Vec3::ZERO, MoveConstraint::X)
                .unwrap()
                .1
                .is_finite()
        );
        assert!(rotation_delta(camera, Vec2::ZERO, end, Vec3::ZERO, MoveConstraint::Z).is_err());
    }

    #[test]
    fn edge_on_rings_remain_pickable_and_drag_continuously_across_camera_angles_and_dpi() {
        for perspective in [false, true] {
            for inverse_scale in [1.0, 0.5] {
                for tilt in [-0.03, 0.0, 0.03] {
                    let projection = if perspective {
                        Projection::default()
                    } else {
                        Projection::Orthographic(OrthographicProjection {
                            scaling_mode: bevy::camera::ScalingMode::FixedVertical {
                                viewport_height: 6.0,
                            },
                            ..OrthographicProjection::default_3d()
                        })
                    };
                    let transform = GlobalTransform::from(
                        Transform::from_xyz(0.0, tilt, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
                    );
                    let camera = MoveCamera::from_projection(
                        &projection,
                        &transform,
                        Vec2::new(800., 600.) / inverse_scale,
                        inverse_scale,
                    )
                    .unwrap();
                    let rings = rotation_rings(camera, Vec3::ZERO);
                    let ring = rings[1].as_ref().expect("绿色Y环不能消失");
                    assert!(ring.edge_on);
                    let logical = camera.size * camera.inverse_scale;
                    let start = ring.screen[10] / logical;
                    assert_eq!(
                        pick_transform_handle(MeshEditTool::Rotate, camera, Vec3::ZERO, start),
                        Some(MoveConstraint::Y)
                    );
                    let drag = Vec2::new(36., 0.) / logical;
                    let (axis, radians) =
                        rotation_delta(camera, start, start + drag, Vec3::ZERO, MoveConstraint::Y)
                            .unwrap();
                    assert_eq!(axis, Vec3::Y);
                    assert!((radians - 0.5).abs() < 1e-4);
                    assert!(
                        (rotation_delta(
                            camera,
                            start,
                            start - drag,
                            Vec3::ZERO,
                            MoveConstraint::Y
                        )
                        .unwrap()
                        .1 + radians)
                            .abs()
                            < 1e-4
                    );
                    assert!(
                        rotation_delta(
                            camera,
                            start,
                            Vec2::splat(f32::NAN),
                            Vec3::ZERO,
                            MoveConstraint::Y
                        )
                        .is_err()
                    );
                }
            }
        }
    }

    #[test]
    fn move_handle_pick_matches_arrows_and_center_view_square() {
        let camera = camera();
        assert_eq!(
            pick_transform_handle(MeshEditTool::Move, camera, Vec3::ZERO, Vec2::ZERO),
            Some(MoveConstraint::View)
        );
        let arrows = move_arrows(camera, Vec3::ZERO);
        assert!(arrows[2].is_none());
        let arrow = arrows[0].as_ref().unwrap();
        let cursor =
            arrow.screen[0].lerp(arrow.screen[1], 0.6) / (camera.size * camera.inverse_scale);
        assert_eq!(
            pick_transform_handle(MeshEditTool::Move, camera, Vec3::ZERO, cursor),
            Some(MoveConstraint::X)
        );
        assert_eq!(
            pick_transform_handle(MeshEditTool::Select, camera, Vec3::ZERO, cursor),
            None
        );
    }
}
