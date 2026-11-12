//! 在 glTF 世界坐标内估计人形关节点，并用编辑后的点位拟合参考骨架。
//!
//! 只读取后台任务已经冻结的几何和参考姿态。保持骨骼身份、层级和参考局部轴约定，
//! 让输出 GLB 的静止姿态可由现有的跨 GLB 动画预览重新定向。

use super::{Geometry, Joint, Landmarks};
use bevy::prelude::{Quat, Vec2, Vec3};
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    sync::atomic::{AtomicBool, Ordering},
};

#[cfg(test)]
#[path = "rigging_native_skeleton_tests.rs"]
mod tests;

const SIDES: [(&str, &str, f32); 2] = [("left", "Left", 1.0), ("right", "Right", -1.0)];
const LIMB_MARKERS: [(&str, &str); 7] = [
    ("shoulder", "Arm"),
    ("elbow", "ForeArm"),
    ("wrist", "Hand"),
    ("hip", "UpLeg"),
    ("knee", "Leg"),
    ("ankle", "Foot"),
    ("toe", "ToeBase"),
];
const SPINE: [(&str, f32); 3] = [("Spine", 0.24), ("Spine1", 0.53), ("Spine2", 0.78)];
const MAIN_CHAINS: [&[&str]; 5] = [
    &[
        "Hips",
        "Spine",
        "Spine1",
        "Spine2",
        "Neck",
        "Head",
        "HeadTop_End",
    ],
    &[
        "Spine2",
        "LeftShoulder",
        "LeftArm",
        "LeftForeArm",
        "LeftHand",
        "LeftHandMiddle1",
    ],
    &[
        "Spine2",
        "RightShoulder",
        "RightArm",
        "RightForeArm",
        "RightHand",
        "RightHandMiddle1",
    ],
    &[
        "Hips",
        "LeftUpLeg",
        "LeftLeg",
        "LeftFoot",
        "LeftToeBase",
        "LeftToe_End",
    ],
    &[
        "Hips",
        "RightUpLeg",
        "RightLeg",
        "RightFoot",
        "RightToeBase",
        "RightToe_End",
    ],
];
const CHUNK_SIZE: usize = 4096;

#[derive(Clone, Copy)]
struct Bounds {
    min: Vec3,
    max: Vec3,
}

impl Bounds {
    fn from_points(points: &[Vec3], cancel: &AtomicBool) -> Result<Self, String> {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for chunk in points.chunks(CHUNK_SIZE) {
            active(cancel)?;
            for &point in chunk {
                if !point.is_finite() {
                    return Err("模型坐标无效，请检查 GLB 数据".into());
                }
                min = min.min(point);
                max = max.max(point);
            }
        }
        if points.is_empty() || !(max - min).is_finite() {
            return Err("模型没有可用的人形网格".into());
        }
        Ok(Self { min, max })
    }

    fn height(self) -> Result<f32, String> {
        let height = self.max.y - self.min.y;
        if !(0.001..=1000.0).contains(&height) {
            return Err("模型高度无效，请检查站立方向和 GLB 的单位".into());
        }
        Ok(height)
    }

    fn contains(self, point: Vec3, margin: f32) -> bool {
        let padding = Vec3::splat(margin);
        point.cmpge(self.min - padding).all() && point.cmple(self.max + padding).all()
    }
}

struct Body<'a> {
    points: Cow<'a, [Vec3]>,
    bounds: Bounds,
    estimated: Option<(Landmarks, Vec<String>)>,
}

fn active(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        return Err("已取消人形绑骨任务".into());
    }
    Ok(())
}

fn horizontal(point: Vec3) -> Vec2 {
    Vec2::new(point.x, point.z)
}

fn body<'a>(geometry: &'a Geometry, cancel: &AtomicBool) -> Result<Body<'a>, String> {
    active(cancel)?;
    let vertices = &geometry.vertices;
    if vertices.len() > 500_000 || geometry.parts.len() > 32 {
        return Err("模型超过 50 万顶点或 32 个网格对象".into());
    }
    let bounds = Bounds::from_points(vertices, cancel)?;
    let all = Body {
        points: Cow::Borrowed(vertices),
        bounds,
        estimated: None,
    };
    if geometry.parts.len() <= 1 {
        if geometry
            .parts
            .first()
            .is_some_and(|range| *range != (0..vertices.len()))
        {
            return Err("模型网格部件的顶点范围无效".into());
        }
        return Ok(all);
    }

    let mut next = 0;
    let mut parts = Vec::with_capacity(geometry.parts.len());
    for range in &geometry.parts {
        if range.start != next || range.is_empty() || range.end > vertices.len() {
            return Err("模型网格部件的顶点范围无效".into());
        }
        let points = &vertices[range.clone()];
        parts.push((points, Bounds::from_points(points, cancel)?));
        next = range.end;
    }
    if next != vertices.len() {
        return Err("模型网格部件的顶点范围不完整".into());
    }
    let &(dominant, dominant_bounds) = parts.iter().max_by_key(|(part, _)| part.len()).unwrap();
    let dominant_height = dominant_bounds.max.y - dominant_bounds.min.y;
    if dominant.len() * 2 < vertices.len() || dominant_height < 0.001 {
        return Ok(all);
    }
    // 独立长武器可能改变全模型的高度和主轴。只在主体仍能识别为完整人形时排除越界部件，
    // 保留原网格供后续蒙皮，披风等紧邻主体的部分继续参与估计。
    let margin = 0.16 * dominant_height;
    parts.retain(|(_, bounds)| {
        dominant_bounds.contains(bounds.min, margin) && dominant_bounds.contains(bounds.max, margin)
    });
    if parts.len() == geometry.parts.len() {
        return Ok(all);
    }
    let mut points = Vec::new();
    for (part, _) in parts {
        for chunk in part.chunks(CHUNK_SIZE) {
            active(cancel)?;
            points.extend_from_slice(chunk);
        }
    }
    let bounds = Bounds::from_points(&points, cancel)?;
    let estimated = estimate_points(&points, bounds, cancel);
    active(cancel)?;
    let Ok((landmarks, mut warnings)) = estimated else {
        return Ok(all);
    };
    warnings.insert(0, "超出主体轮廓的独立部件未参与关节估计".into());
    Ok(Body {
        points: Cow::Owned(points),
        bounds,
        estimated: Some((landmarks, warnings)),
    })
}

pub(super) fn estimate(
    geometry: &Geometry,
    cancel: &AtomicBool,
) -> Result<(Landmarks, Vec<String>), String> {
    let body = body(geometry, cancel)?;
    match body.estimated {
        Some(estimate) => Ok(estimate),
        None => estimate_points(&body.points, body.bounds, cancel),
    }
}

/// 线性选取分位数，避免为横截面估计而排序全部顶点。
fn quantile(values: &mut [f32], fraction: f32) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    let rank = (values.len() - 1) as f32 * fraction;
    let (_, pivot, higher) = values.select_nth_unstable_by(rank.floor() as usize, f32::total_cmp);
    let next = higher
        .iter()
        .copied()
        .min_by(f32::total_cmp)
        .unwrap_or(*pivot);
    Some(*pivot + (next - *pivot) * rank.fract())
}

fn samples(
    points: &[Vec3],
    cancel: &AtomicBool,
    mut project: impl FnMut(Vec3) -> Option<f32>,
) -> Result<Vec<f32>, String> {
    let mut values = Vec::new();
    for chunk in points.chunks(CHUNK_SIZE) {
        active(cancel)?;
        values.extend(chunk.iter().copied().filter_map(&mut project));
    }
    Ok(values)
}

fn estimate_points(
    points: &[Vec3],
    bounds: Bounds,
    cancel: &AtomicBool,
) -> Result<(Landmarks, Vec<String>), String> {
    let height = bounds.height()?;
    let extent = bounds.max - bounds.min;
    if extent.x.max(extent.z) > 8.0 * height {
        return Err("模型轮廓过宽，请使用完整站立的人形模型".into());
    }
    let origin = bounds.min + Vec3::new(extent.x, 0.0, extent.z) * 0.5;
    let mut profile = Vec::with_capacity(points.len());
    let (mut sum, mut xx, mut zz, mut xz) = ([0.0_f64; 2], 0.0_f64, 0.0_f64, 0.0_f64);
    let mut upper_x = Vec::new();
    let mut upper_z = Vec::new();
    for chunk in points.chunks(CHUNK_SIZE) {
        active(cancel)?;
        for &point in chunk {
            let point = (point - origin) / height;
            profile.push(point);
            if point.y > 0.45 && point.y < 0.88 {
                upper_x.push(point.x);
                upper_z.push(point.z);
                let (x, z) = (f64::from(point.x), f64::from(point.z));
                sum[0] += x;
                sum[1] += z;
                xx += x * x;
                zz += z * z;
                xz += x * z;
            }
        }
    }
    if upper_x.len() < 4 {
        return Err("无法辨识站立的人形上身，请使用 T 姿态或 A 姿态".into());
    }
    // 水平协方差的最大特征向量给出臂展方向，再用脚尖相对脚踝的伸出方向确定正面。
    let count = upper_x.len() as f64;
    xx -= sum[0] * sum[0] / count;
    zz -= sum[1] * sum[1] / count;
    xz -= sum[0] * sum[1] / count;
    let angle = (0.5 * (2.0 * xz).atan2(xx - zz)) as f32;
    let mut across = Vec2::new(angle.cos(), angle.sin());
    if across.x < 0.0 {
        across = -across;
    }
    let mut forward = Vec2::new(-across.y, across.x);
    let sole = quantile(
        &mut samples(&profile, cancel, |p| {
            (p.y < 0.035).then(|| horizontal(p).dot(forward))
        })?,
        0.5,
    );
    let heel = quantile(
        &mut samples(&profile, cancel, |p| {
            (p.y > 0.045 && p.y < 0.105).then(|| horizontal(p).dot(forward))
        })?,
        0.5,
    );
    let facing = sole.zip(heel).map_or(0.0, |(a, b)| a - b);
    let spread = (xx - zz).hypot(2.0 * xz);
    let mut warnings = Vec::new();
    if facing.abs() < 0.006 || xx + zz + spread < 1.4 * (xx + zz - spread) {
        warnings.push("正面朝向依模型坐标估算，请核对左右标记".into());
    }
    if facing < -0.006 {
        across = -across;
        forward = -forward;
    }

    let median = Vec2::new(
        quantile(&mut upper_x, 0.5).unwrap(),
        quantile(&mut upper_z, 0.5).unwrap(),
    );
    for chunk in profile.chunks_mut(CHUNK_SIZE) {
        active(cancel)?;
        for point in chunk {
            let horizontal = horizontal(*point) - median;
            point.x = horizontal.dot(across);
            point.z = horizontal.dot(forward);
        }
    }
    // 头部横截面提供中线，避免不对称手臂改变左右分界。
    let mut crown = samples(&profile, cancel, |p| (p.y > 0.88).then_some(p.x))?;
    let center = (quantile(&mut crown, 0.14).unwrap_or(0.0)
        + quantile(&mut crown, 0.86).unwrap_or(0.0))
        * 0.5;
    for chunk in profile.chunks_mut(CHUNK_SIZE) {
        active(cancel)?;
        for point in chunk {
            point.x -= center;
        }
    }
    let planar_center = median + across * center;
    let to_world = |x: f32, y: f32, z: f32| {
        let planar = planar_center + across * x + forward * z;
        (origin + Vec3::new(planar.x, y, planar.y) * height).to_array()
    };
    let depth = |level: f32| -> Result<f32, String> {
        let mut values = samples(&profile, cancel, |p| {
            ((p.y - level).abs() < 0.038 && p.x.abs() < 0.11).then_some(p.z)
        })?;
        Ok(quantile(&mut values, 0.5).unwrap_or(0.0))
    };
    let mut markers = Landmarks::new();
    let hip_depth = depth(0.60)?;
    markers.insert("head".into(), to_world(0.0, 0.86, depth(0.86)?));
    markers.insert("neck".into(), to_world(0.0, 0.81, depth(0.81)?));
    markers.insert("hips".into(), to_world(0.0, 0.60, hip_depth));
    let shoulder_depth = depth(0.80)?;
    let arm_depth = depth(0.73)?;
    let ankle_depth = depth(0.065)?;
    let arms = |p: Vec3| p.y > 0.42 && p.y < 0.90;
    let mut spans = [0.0_f32; 2];
    for (i, &(_, _, sign)) in SIDES.iter().enumerate() {
        let mut wing = samples(&profile, cancel, |p| {
            (arms(p) && p.x * sign > 0.08).then_some(p.x * sign)
        })?;
        if wing.len() < 4 {
            return Err("未检测到两侧肢体，请确认人物双臂张开且网格完整".into());
        }
        spans[i] = quantile(&mut wing, 0.99).unwrap();
    }
    let smaller = spans[0].min(spans[1]);
    if smaller < 0.16 || smaller < 0.52 * spans[0].max(spans[1]) || spans[0] + spans[1] < 0.35 {
        return Err("两侧臂展差异过大或一侧缺少手臂，请检查模型是否为完整 T/A 姿态".into());
    }
    if spans[0] + spans[1] > 1.9 {
        return Err("模型轮廓过宽，请使用完整站立的人形模型".into());
    }
    for ((side, _, sign), span) in SIDES.into_iter().zip(spans) {
        let shoulder = (span * 0.21).clamp(0.055, 0.13);
        let wrist = span * 0.84;
        let elbow = shoulder + (wrist - shoulder) * 0.53;
        let mut add = |name, x, y, z| {
            markers.insert(format!("{side}_{name}"), to_world(x * sign, y, z));
        };
        add("shoulder", shoulder, 0.80, shoulder_depth);
        for (name, at) in [("elbow", elbow), ("wrist", wrist)] {
            let section = |p: Vec3| arms(p) && (p.x * sign - at).abs() < (span * 0.055).max(0.020);
            let y = quantile(
                &mut samples(&profile, cancel, |p| section(p).then_some(p.y))?,
                0.5,
            );
            let z = quantile(
                &mut samples(&profile, cancel, |p| section(p).then_some(p.z))?,
                0.5,
            );
            add(name, at, y.unwrap_or(0.73), z.unwrap_or(arm_depth));
        }
        let ankle_section = |p: Vec3| p.x * sign > 0.0 && p.y > 0.035 && p.y < 0.082;
        let ankle_x = quantile(
            &mut samples(&profile, cancel, |p| ankle_section(p).then_some(p.x * sign))?,
            0.5,
        )
        .unwrap_or(0.053)
        .clamp(0.025, 0.25);
        let ankle_z = quantile(
            &mut samples(&profile, cancel, |p| ankle_section(p).then_some(p.z))?,
            0.5,
        )
        .unwrap_or(ankle_depth);
        let hip_x = 0.048;
        let leg_axis = (hip_x + ankle_x) * 0.5;
        let knee_section =
            |p: Vec3| (p.x * sign - leg_axis).abs() < 0.045 && (p.y - 0.31).abs() < 0.028;
        let knee_x = quantile(
            &mut samples(&profile, cancel, |p| knee_section(p).then_some(p.x * sign))?,
            0.5,
        )
        .unwrap_or(leg_axis);
        let knee_z = quantile(
            &mut samples(&profile, cancel, |p| knee_section(p).then_some(p.z))?,
            0.5,
        )
        .unwrap_or((hip_depth + ankle_z) * 0.5);
        let toe_z = quantile(
            &mut samples(&profile, cancel, |p| {
                ((p.x * sign - ankle_x).abs() < 0.065 && p.y < 0.055).then_some(p.z)
            })?,
            0.8,
        )
        .unwrap_or(ankle_z + 0.045)
        .clamp(ankle_z + 0.012, ankle_z + 0.085);
        add("hip", hip_x, 0.56, hip_depth);
        add("knee", knee_x, 0.31, knee_z);
        add("ankle", ankle_x, 0.055, ankle_z);
        add("toe", ankle_x, 0.028, toe_z);
    }
    active(cancel)?;
    Ok((markers, warnings))
}

fn marker(points: &Landmarks, name: &str) -> Vec3 {
    Vec3::from_array(points[name])
}

fn validate_markers(points: &Landmarks, bounds: Bounds, height: f32) -> Result<(), String> {
    let mut names = vec!["head".to_owned(), "neck".to_owned(), "hips".to_owned()];
    for (side, _, _) in SIDES {
        names.extend(LIMB_MARKERS.map(|(name, _)| format!("{side}_{name}")));
    }
    if points.len() != names.len() || names.iter().any(|name| !points.contains_key(name)) {
        return Err("请确认全部 17 个人形关节点".into());
    }
    for name in &names {
        let position = marker(points, name);
        if !position.is_finite() || !bounds.contains(position, height * 0.15) {
            return Err(format!("关节点 {name} 超出模型附近，请检查标记位置"));
        }
    }
    let check_chain = |chain: &[String]| -> Result<(), String> {
        for pair in chain.windows(2) {
            if marker(points, &pair[0]).distance(marker(points, &pair[1])) < height * 0.009 {
                return Err(format!(
                    "关节点 {} 和 {} 太接近，请拉开距离",
                    pair[0], pair[1]
                ));
            }
        }
        Ok(())
    };
    check_chain(&names[..3])?;
    if !(marker(points, "head").y > marker(points, "neck").y
        && marker(points, "neck").y > marker(points, "hips").y)
    {
        return Err("请从上至下标记头部、颈部和胯部".into());
    }
    for (side, _, _) in SIDES {
        let arm = ["shoulder", "elbow", "wrist"].map(|name| format!("{side}_{name}"));
        let leg = ["hip", "knee", "ankle", "toe"].map(|name| format!("{side}_{name}"));
        check_chain(&arm)?;
        check_chain(&leg)?;
        if !(marker(points, &leg[0]).y > marker(points, &leg[1]).y
            && marker(points, &leg[1]).y > marker(points, &leg[2]).y)
        {
            return Err("请从上至下标记髋、膝和脚踝".into());
        }
    }
    for name in ["shoulder", "hip", "ankle"] {
        check_chain(&[format!("left_{name}"), format!("right_{name}")])?;
    }
    Ok(())
}

struct Reference<'a> {
    joints: &'a [Joint],
    names: HashMap<&'a str, usize>,
    children: Vec<Vec<usize>>,
    ordered: Vec<usize>,
}

impl<'a> Reference<'a> {
    fn new(joints: &'a [Joint]) -> Result<Self, String> {
        if joints.is_empty() || joints.len() > 128 {
            return Err("参考骨架关节数量无效".into());
        }
        let mut names = HashMap::new();
        let mut unique = HashSet::new();
        let mut children = vec![Vec::new(); joints.len()];
        let mut pending = Vec::new();
        for (i, joint) in joints.iter().enumerate() {
            if joint.name.is_empty()
                || !unique.insert(joint.name.as_str())
                || !joint.position.is_finite()
                || !joint.rotation.is_finite()
                || !(1e-8..=1e8).contains(&joint.rotation.length_squared())
            {
                return Err("参考骨架包含重名或姿态无效的关节".into());
            }
            if let Some(name) = joint.name.strip_prefix("mixamorig:") {
                names.insert(name, i);
            }
            if let Some(parent) = joint.parent {
                children
                    .get_mut(parent)
                    .ok_or("参考骨架父级索引无效")?
                    .push(i);
            } else {
                pending.push(i);
            }
        }
        let mut ordered = Vec::with_capacity(joints.len());
        while let Some(i) = pending.pop() {
            ordered.push(i);
            pending.extend(children[i].iter().copied());
        }
        if ordered.len() != joints.len() {
            return Err("参考骨架的父级存在循环".into());
        }
        Ok(Self {
            joints,
            names,
            children,
            ordered,
        })
    }

    fn required(&self, name: &str) -> Result<usize, String> {
        self.names
            .get(name)
            .copied()
            .ok_or_else(|| format!("参考动作骨架缺少关节 mixamorig:{name}"))
    }

    fn validate_chains(&self) -> Result<(), String> {
        for chain in MAIN_CHAINS {
            for pair in chain.windows(2) {
                let (Some(&bone), Some(&child)) =
                    (self.names.get(pair[0]), self.names.get(pair[1]))
                else {
                    continue;
                };
                let mut parent = self.joints[child].parent;
                while let Some(index) = parent {
                    if index == bone {
                        break;
                    }
                    parent = self.joints[index].parent;
                }
                if parent.is_none() {
                    return Err(format!(
                        "参考骨架层级不兼容，{} 应位于 {} 下",
                        pair[1], pair[0]
                    ));
                }
            }
        }
        Ok(())
    }
}

pub(super) fn fit(
    geometry: &Geometry,
    reference: &[Joint],
    markers: &Landmarks,
    cancel: &AtomicBool,
) -> Result<Vec<Joint>, String> {
    let body = body(geometry, cancel)?;
    let height = body.bounds.height()?;
    validate_markers(markers, body.bounds, height)?;
    let reference = Reference::new(reference)?;
    reference.validate_chains()?;
    let joints = reference.joints;
    let mut fitted = vec![None; joints.len()];
    let hips = reference.required("Hips")?;
    let neck = reference.required("Neck")?;
    let head = reference.required("Head")?;
    let hip_position = marker(markers, "hips");
    let neck_position = marker(markers, "neck");
    let head_position = marker(markers, "head");
    fitted[hips] = Some(hip_position);
    fitted[neck] = Some(neck_position);
    fitted[head] = Some(head_position);
    let old_torso = joints[neck].position.distance(joints[hips].position);
    if !old_torso.is_finite() || old_torso < 1e-6 {
        return Err("参考骨架的躯干长度无效".into());
    }
    let ratio = neck_position.distance(hip_position) / old_torso;
    if !ratio.is_finite() {
        return Err("标记的躯干长度无效".into());
    }
    for (name, fraction) in SPINE {
        fitted[reference.required(name)?] = Some(hip_position.lerp(neck_position, fraction));
    }
    if let Some(&end) = reference.names.get("HeadTop_End") {
        let radius = height * 0.11;
        let crown = samples(&body.points, cancel, |p| {
            (horizontal(p - head_position).length_squared() < radius * radius).then_some(p.y)
        })?
        .into_iter()
        .max_by(f32::total_cmp)
        .unwrap_or(body.bounds.max.y);
        fitted[end] = Some(Vec3::new(
            head_position.x,
            (crown - 0.015 * height).max(head_position.y + 0.04 * height),
            head_position.z,
        ));
    }

    let left_arm = reference.required("LeftArm")?;
    let right_arm = reference.required("RightArm")?;
    let old_side = joints[left_arm].position - joints[right_arm].position;
    let new_side = marker(markers, "left_shoulder") - marker(markers, "right_shoulder");
    let flat = |p: Vec3| Vec3::new(p.x, 0.0, p.z);
    let (Some(old_side), Some(new_side)) = (
        flat(old_side).try_normalize(),
        flat(new_side).try_normalize(),
    ) else {
        return Err("参考骨架或当前标记没有可用的左右肩方向".into());
    };
    let facing = Quat::from_rotation_y(old_side.cross(new_side).y.atan2(old_side.dot(new_side)));

    for (side, prefix, _) in SIDES {
        active(cancel)?;
        for (point, name) in LIMB_MARKERS {
            fitted[reference.required(&format!("{prefix}{name}"))?] =
                Some(marker(markers, &format!("{side}_{point}")));
        }
        let shoulder = marker(markers, &format!("{side}_shoulder"));
        fitted[reference.required(&format!("{prefix}Shoulder"))?] =
            Some(neck_position.lerp(shoulder, 0.45));
        let hand = reference.required(&format!("{prefix}Hand"))?;
        let forearm = reference.required(&format!("{prefix}ForeArm"))?;
        fit_hand(
            &reference,
            hand,
            forearm,
            &mut fitted,
            &body.points,
            facing,
            ratio,
            height,
            cancel,
        )?;
        if let Some(&end) = reference.names.get(format!("{prefix}Toe_End").as_str()) {
            let toe = marker(markers, &format!("{side}_toe"));
            let ankle = marker(markers, &format!("{side}_ankle"));
            let direction = flat(toe - ankle)
                .try_normalize()
                .unwrap_or(new_side.cross(Vec3::Y));
            fitted[end] = Some(toe + direction * (height * 0.026));
        }
    }

    for &index in &reference.ordered {
        if fitted[index].is_none() {
            let anchor = joints[index].parent.unwrap_or(hips);
            let anchor_position = fitted[anchor].ok_or("参考骨架无法沿父级拟合")?;
            fitted[index] = Some(
                anchor_position
                    + facing * (joints[index].position - joints[anchor].position) * ratio,
            );
        }
    }
    let positions: Vec<_> = fitted.into_iter().map(Option::unwrap).collect();
    let mut primary = vec![None; joints.len()];
    for pair in MAIN_CHAINS.into_iter().flat_map(|chain| chain.windows(2)) {
        if let (Some(&bone), Some(&child)) =
            (reference.names.get(pair[0]), reference.names.get(pair[1]))
            && joints[child].parent == Some(bone)
            && primary[bone].is_none()
        {
            primary[bone] = Some(child);
        }
    }
    let mut basis_delta = vec![facing; joints.len()];
    for &index in &reference.ordered {
        let children = &reference.children[index];
        let child = primary[index].or_else(|| (children.len() == 1).then(|| children[0]));
        // 沿参考主子骨方向旋转整个静止坐标系，保留源骨骼本来的轴和滚转角。
        // 没有方向的末端或辅助节点沿用父节点的坐标系变化。
        let inherited = joints[index]
            .parent
            .map_or(facing, |parent| basis_delta[parent]);
        basis_delta[index] = child
            .and_then(|child| {
                let before =
                    (facing * (joints[child].position - joints[index].position)).try_normalize()?;
                let after = (positions[child] - positions[index]).try_normalize()?;
                Some(Quat::from_rotation_arc(before, after) * facing)
            })
            .unwrap_or(inherited);
    }
    active(cancel)?;
    joints
        .iter()
        .enumerate()
        .map(|(i, joint)| {
            let rotation = (basis_delta[i] * joint.rotation.normalize()).normalize();
            if !positions[i].is_finite() || !rotation.is_finite() {
                return Err("参考骨架拟合得到无效的关节姿态".into());
            }
            Ok(Joint {
                name: joint.name.clone(),
                parent: joint.parent,
                position: positions[i],
                rotation,
            })
        })
        .collect()
}

fn fit_hand(
    reference: &Reference<'_>,
    hand: usize,
    forearm: usize,
    fitted: &mut [Option<Vec3>],
    points: &[Vec3],
    facing: Quat,
    ratio: f32,
    height: f32,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let joints = reference.joints;
    let mut pending = reference.children[hand].clone();
    if pending.is_empty() {
        return Ok(());
    }
    let mut descendants = Vec::new();
    while let Some(index) = pending.pop() {
        descendants.push(index);
        pending.extend(reference.children[index].iter().copied());
    }
    let wrist = fitted[hand].unwrap();
    let previous = (facing * (joints[hand].position - joints[forearm].position))
        .try_normalize()
        .ok_or("参考骨架的前臂长度无效")?;
    let direction = (wrist - fitted[forearm].unwrap())
        .try_normalize()
        .ok_or("标记的前臂长度无效")?;
    let hand_rotation = Quat::from_rotation_arc(previous, direction) * facing;
    let hand_length = descendants
        .iter()
        .map(|&index| joints[index].position.distance(joints[hand].position))
        .fold(0.0_f32, f32::max);
    let radius = 0.055 * height;
    let mut forward = samples(points, cancel, |point| {
        let offset = point - wrist;
        let distance = offset.dot(direction);
        (distance > 0.0
            && distance < 0.22 * height
            && (offset - direction * distance).length_squared() < radius * radius)
            .then_some(distance)
    })?;
    // 手指保持参考手部的相对形状，手长以腕端附近的网格为准。
    let scale = if hand_length > 1e-6 && forward.len() >= 4 {
        (quantile(&mut forward, 0.92).unwrap() / hand_length).clamp(0.4 * ratio, 2.0 * ratio)
    } else {
        ratio
    };
    for index in descendants {
        fitted[index] =
            Some(wrist + hand_rotation * (joints[index].position - joints[hand].position) * scale);
    }
    Ok(())
}
