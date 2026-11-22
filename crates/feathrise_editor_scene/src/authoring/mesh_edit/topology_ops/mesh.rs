//! 操作共享的世界米制工作网格、半毫米回写及多边形三角化；不接触ECS、磁盘或作者会话。

use super::super::selection::MeshSelection;
use crate::authoring::placement_pose::placement_rotation;
use bevy::math::{DVec2, DVec3};
use feathrise_scene_content::{
    MAX_EDITABLE_MESH_FACE_VERTICES, MAX_EDITABLE_MESH_FACES,
    MAX_EDITABLE_MESH_VERTICES, ScenePlacementAssetTarget, ScenePlacementRecord, MeshFace,
};

pub(super) const EPSILON: f64 = 1e-8;

pub(super) struct MeshWork {
    pub points: Vec<DVec3>,
    pub faces: Vec<Vec<usize>>,
    pub edges: Vec<[usize; 2]>,
}

pub(super) fn edge(a: usize, b: usize) -> [usize; 2] {
    if a < b { [a, b] } else { [b, a] }
}

pub(super) fn chosen(selected: &MeshSelection, count: usize) -> Result<Vec<usize>, String> {
    let indices: Vec<_> = selected.iter().collect();
    if indices.last().is_some_and(|&index| index >= count) {
        return Err("元素选择已经超出当前拓扑，请重新选择".into());
    }
    Ok(indices)
}

pub(super) fn positive_amount(amount: f32, name: &str) -> Result<f64, String> {
    if !amount.is_finite() || amount < 0.0 {
        return Err(format!("{name}必须是有限的非负数"));
    }
    Ok(f64::from(amount))
}

impl MeshWork {
    pub fn from_record(before: &ScenePlacementRecord) -> Result<Self, String> {
        Self::with_coordinates(before, false)
    }

    pub(super) fn from_local_record(before: &ScenePlacementRecord) -> Result<Self, String> {
        Self::with_coordinates(before, true)
    }

    fn with_coordinates(before: &ScenePlacementRecord, local_half_mm: bool) -> Result<Self, String> {
        before.asset_target.validate()?; // 正式校验先于按面索引取点，坏存档不能令纯操作越界。
        let geometry = before
            .asset_target
            .editable_geometry()
            .ok_or("当前来源不支持拓扑编辑")?;
        let scale = DVec3::from_array(before.scale.map(f64::from));
        let origin = DVec3::from_array(before.translation.map(f64::from));
        let rotation = placement_rotation(before).as_dquat();
        if !scale.is_finite()
            || scale.min_element() <= 0.0
            || !origin.is_finite()
            || !rotation.is_finite()
        {
            return Err("对象变换无效，不能编辑拓扑".into());
        }
        let rotation = rotation.normalize();
        Ok(Self {
            points: geometry
                .vertices()
                .iter()
                .map(|point| DVec3::from_array(point.map(f64::from)))
                .map(|point| if local_half_mm { point } else { origin + rotation * (point * 0.0005 * scale) })
                .collect(),
            faces: geometry
                .faces()
                .iter()
                .map(|face| face.indices().iter().map(|&v| v as usize).collect())
                .collect(),
            edges: geometry.edges().to_vec(),
        })
    }

    pub fn push_point(&mut self, point: DVec3) -> Result<usize, String> {
        // 候选构造沿用作者网格的内存边界，在分配下一点之前拒绝超预算的操作。
        if !point.is_finite() {
            return Err("操作产生了无效坐标".into());
        }
        if self.points.len() >= MAX_EDITABLE_MESH_VERTICES {
            return Err("操作结果超过作者网格内存预算，请缩小选择范围".into());
        }
        let index = self.points.len();
        self.points.push(point);
        Ok(index)
    }

    pub fn finish(self, before: &ScenePlacementRecord) -> Result<ScenePlacementRecord, String> {
        let scale = DVec3::from_array(before.scale.map(f64::from));
        let origin = DVec3::from_array(before.translation.map(f64::from));
        let inverse = placement_rotation(before).as_dquat().normalize().inverse();
        self.finish_record(before, |point| inverse * (point - origin) / scale * 2_000.0)
    }

    /// 线性细分直接写本地半毫米坐标，避免合法实例变换改变同一条边的切点舍入。
    pub(super) fn finish_local(self, before: &ScenePlacementRecord) -> Result<ScenePlacementRecord, String> {
        self.finish_record(before, |point| point)
    }

    fn finish_record(
        self,
        before: &ScenePlacementRecord,
        local_half_mm: impl Fn(DVec3) -> DVec3,
    ) -> Result<ScenePlacementRecord, String> {
        if self.faces.len() > MAX_EDITABLE_MESH_FACES {
            return Err("操作结果超过作者网格内存预算，请缩小选择范围".into());
        }
        // 点 ID 在候选内连续，按原索引顺序压实可保留稳定选择且避免每个角点查树。
        let mut used = vec![false; self.points.len()];
        for &index in self.faces.iter().flatten() {
            *used.get_mut(index).ok_or("操作产生了不存在的顶点引用")? = true;
        }
        let mut remap = vec![0; self.points.len()];
        let mut vertices = Vec::with_capacity(self.points.len());
        for (index, point) in self.points.into_iter().enumerate() {
            if !used[index] {
                continue;
            }
            let quantized = local_half_mm(point);
            if !quantized.is_finite() || quantized.abs().max_element() > 20_000_000.0 {
                return Err("拓扑结果超过本地各轴±10000米范围".into());
            }
            remap[index] = u32::try_from(vertices.len()).map_err(|_| "顶点索引超过可保存范围")?;
            vertices.push(quantized.to_array().map(|value| value.round() as i32));
        }
        let mut faces = Vec::with_capacity(self.faces.len());
        for face in self.faces {
            if face.len() > MAX_EDITABLE_MESH_FACE_VERTICES {
                return Err("单个面超过16个顶点，请先细分邻面".into());
            }
            faces.push(MeshFace::new(
                &face.iter().map(|&index| remap[index]).collect::<Vec<_>>(),
            )?);
        }
        // 量化结果交给保存与运行时共用的完整校验，采用成功时直接转移所有权。
        let geometry = before.asset_target.editable_geometry().ok_or("当前来源没有可编辑网格")?
            .with_parts(vertices, faces)?;
        let (color_srgb, roughness_milli, collision) = match before.asset_target {
            ScenePlacementAssetTarget::Cuboid {
                color_srgb,
                roughness_milli,
                collision,
                ..
            }
            | ScenePlacementAssetTarget::EditableCuboid {
                color_srgb,
                roughness_milli,
                collision,
                ..
            }
            | ScenePlacementAssetTarget::EditableMesh {
                color_srgb,
                roughness_milli,
                collision,
                ..
            } => (color_srgb, roughness_milli, collision),
            _ => return Err("当前来源没有可编辑网格材质".into()),
        };
        let mut after = before.clone();
        after.asset_target = ScenePlacementAssetTarget::EditableMesh {
            geometry,
            color_srgb,
            roughness_milli,
            collision,
        };
        Ok(after)
    }
}

pub(super) fn face_normal(face: &[usize], points: &[DVec3]) -> Result<DVec3, String> {
    let origin = points[face[0]];
    let normal = (0..face.len()).fold(DVec3::ZERO, |sum, i| {
        // 先移到面内局部原点，远离世界原点的小面也不承受大坐标叉积的抵消。
        sum + (points[face[i]] - origin).cross(points[face[(i + 1) % face.len()]] - origin)
    });
    normal
        .try_normalize()
        .ok_or_else(|| "面法线退化，不能执行拓扑操作".into())
}

pub(super) fn planar_normal(face: &[usize], points: &[DVec3]) -> Result<DVec3, String> {
    let normal = face_normal(face, points)?;
    let origin = points[face[0]];
    let extent = face
        .iter()
        .map(|&v| points[v].distance(origin))
        .fold(1.0_f64, f64::max);
    if face
        .iter()
        .any(|&v| normal.dot(points[v] - origin).abs() > extent * 1e-6)
    {
        return Err("此操作需要平面面片，请先细分非平面四边面".into());
    }
    Ok(normal)
}

/// 保留3/4边面的正式对角线；复杂平面面片用不跨越边界的耳切，不能用扇形制造凹面外三角。
pub(super) fn triangulate(face: &[usize], points: &[DVec3]) -> Result<Vec<[usize; 3]>, String> {
    if face.len() == 3 {
        return Ok(vec![[face[0], face[1], face[2]]]);
    }
    if face.len() == 4
        && (points[face[1]] - points[face[0]])
            .cross(points[face[2]] - points[face[0]])
            .length_squared()
            > EPSILON * EPSILON
        && (points[face[2]] - points[face[0]])
            .cross(points[face[3]] - points[face[0]])
            .length_squared()
            > EPSILON * EPSILON
    {
        return Ok(vec![
            [face[0], face[1], face[2]],
            [face[0], face[2], face[3]],
        ]);
    }
    let normal = planar_normal(face, points)?;
    let axis = normal.abs().max_element();
    let project = |v: usize| {
        let p = points[v];
        if normal.x.abs() == axis {
            DVec2::new(p.y, p.z)
        } else if normal.y.abs() == axis {
            DVec2::new(p.z, p.x)
        } else {
            DVec2::new(p.x, p.y)
        }
    };
    let sign = if normal.x.abs() == axis {
        normal.x.signum()
    } else if normal.y.abs() == axis {
        normal.y.signum()
    } else {
        normal.z.signum()
    };
    let cross = |a, b, c| (project(b) - project(a)).perp_dot(project(c) - project(a)) * sign;
    let mut remaining = face.to_vec();
    let mut triangles = Vec::with_capacity(face.len() - 2);
    while remaining.len() > 3 {
        let mut ear = None;
        for i in 0..remaining.len() {
            let a = remaining[(i + remaining.len() - 1) % remaining.len()];
            let b = remaining[i];
            let c = remaining[(i + 1) % remaining.len()];
            if cross(a, b, c) <= EPSILON {
                continue;
            }
            if remaining.iter().copied().any(|p| {
                p != a
                    && p != b
                    && p != c
                    && cross(a, b, p) >= -EPSILON
                    && cross(b, c, p) >= -EPSILON
                    && cross(c, a, p) >= -EPSILON
            }) {
                continue;
            }
            ear = Some((i, [a, b, c]));
            break;
        }
        let Some((index, triangle)) = ear else {
            return Err("面片无法形成非退化三角，请减小操作量".into());
        };
        triangles.push(triangle);
        remaining.remove(index);
    }
    if cross(remaining[0], remaining[1], remaining[2]) <= EPSILON {
        return Err("面片末端三角退化，请减小操作量".into());
    }
    triangles.push([remaining[0], remaining[1], remaining[2]]);
    Ok(triangles)
}
