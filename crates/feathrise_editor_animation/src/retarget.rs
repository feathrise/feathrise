//! 自动绑骨后的外部动画映射：基于真实静止关节轴向与同名层级，保留目标比例并原地播放。

use super::{
    data::{AnimationDocument, Node},
    preview::world_matrices,
    types::SkeletonMapping,
};
use feathrise_editor_animation_model::{media::AssetPreviewModelBounds, retarget::{JointCorrection, RigRetarget}};
use bevy::prelude::*;
use std::{collections::HashMap, sync::Arc};

const PREFIX: &str = "mixamorig:";
const CORE_JOINTS: &[&str] = &[
    "Hips",
    "Spine",
    "Head",
    "LeftArm",
    "LeftForeArm",
    "LeftHand",
    "RightArm",
    "RightForeArm",
    "RightHand",
    "LeftUpLeg",
    "LeftLeg",
    "LeftFoot",
    "RightUpLeg",
    "RightLeg",
    "RightFoot",
];

struct RestFrame {
    local: Vec<Transform>,
    world: Vec<Mat4>,
}

impl RestFrame {
    fn new(nodes: &[Node]) -> Self {
        let local: Vec<_> = nodes
            .iter()
            .map(|node| Transform {
                translation: Vec3::from_array(node.translation),
                rotation: Quat::from_array(node.rotation),
                scale: Vec3::from_array(node.scale),
            })
            .collect();
        let world = world_matrices(nodes, &local);
        Self { local, world }
    }

    fn rotation(&self, index: usize) -> Result<Quat, String> {
        let matrix = self.world[index];
        // 只有正比例且无剪切的骨架能用四元数转换局部轴。拒绝镜像或退化变换，避免产生反向蒙皮。
        let axes = [
            matrix.x_axis.truncate(),
            matrix.y_axis.truncate(),
            matrix.z_axis.truncate(),
        ];
        let lengths = Vec3::new(axes[0].length(), axes[1].length(), axes[2].length());
        let max = lengths.max_element();
        if !lengths.is_finite()
            || lengths.min_element() <= 1e-8
            || max - lengths.min_element() > max * 0.001
            || matrix.determinant() <= 0.0
        {
            return Err("骨架的镜像、非等比缩放或退化变换需要先应用到源模型".into());
        }
        let basis = Mat3::from_cols(
            axes[0] / lengths.x,
            axes[1] / lengths.y,
            axes[2] / lengths.z,
        );
        if basis
            .x_axis
            .dot(basis.y_axis)
            .abs()
            .max(basis.y_axis.dot(basis.z_axis).abs())
            .max(basis.z_axis.dot(basis.x_axis).abs())
            > 0.001
        {
            return Err("骨架含剪切变换，无法安全重定向".into());
        }
        Ok(Quat::from_mat3(&basis).normalize())
    }

    fn position(&self, index: usize) -> Vec3 {
        self.world[index].w_axis.truncate()
    }
}

fn named_bones<'a>(
    nodes: &'a [Node],
    include: impl Fn(usize) -> bool,
) -> Result<HashMap<&'a str, usize>, String> {
    let mut names = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        if let Some(name) = node.name.strip_prefix(PREFIX)
            && include(index)
            && names.insert(name, index).is_some()
        {
            return Err(format!("骨架内有重名关节 {}", node.name));
        }
    }
    for name in CORE_JOINTS {
        if !names.contains_key(name) {
            return Err(format!("当前人形骨架缺少 {PREFIX}{name}"));
        }
    }
    Ok(names)
}

fn facing(
    source: &RestFrame,
    target: &RestFrame,
    source_names: &HashMap<&str, usize>,
    target_names: &HashMap<&str, usize>,
) -> Result<Quat, String> {
    let lateral = |rest: &RestFrame, names: &HashMap<&str, usize>| {
        let delta = rest.position(names["LeftArm"]) - rest.position(names["RightArm"]);
        Vec3::new(delta.x, 0.0, delta.z).try_normalize()
    };
    let a = lateral(source, source_names).ok_or("源骨架的两肩无法确定朝向")?;
    let b = lateral(target, target_names).ok_or("角色骨架的两肩无法确定朝向")?;
    // 180° 时仍限定绕 glTF 的竖直轴，不让通用最短弧把角色倒转。
    Ok(Quat::from_rotation_y(a.cross(b).y.atan2(a.dot(b))))
}

pub(super) fn mapping(
    source: &AnimationDocument,
    target: &AnimationDocument,
) -> Result<SkeletonMapping, String> {
    if source.nodes.len().max(target.nodes.len()) > 8192 {
        return Err("人形预览的单个 GLB 超过 8192 个节点".into());
    }
    let (target_joints, target_bounds) = target.scene_skin();
    let source_names = named_bones(&source.nodes, |_| true)?;
    let target_names = named_bones(&target.nodes, |index| target_joints[index])?;
    let s = RestFrame::new(&source.nodes);
    let t = RestFrame::new(&target.nodes);
    let facing = facing(&s, &t, &source_names, &target_names)?;
    let mut offsets = vec![None; source.nodes.len()];
    let mut names = HashMap::new();
    let mut primary = vec![None; source.nodes.len()];
    for (&name, &index) in &source_names {
        if !target_names.contains_key(name) {
            continue; // 不同细节等级的手指/末端关节可以留在目标自身的静止姿势。
        }
        if let Some(parent) = source.nodes[index].parent {
            // 关节向量默认取唯一子骨。分叉处明确选脊柱、颈或中指，避免由文件节点顺序决定扭转。
            let preferred = matches!(
                name,
                "Spine" | "Neck" | "LeftHandMiddle1" | "RightHandMiddle1"
            );
            if primary[parent].is_none_or(|(previous, _, was_preferred)| {
                preferred > was_preferred || (preferred == was_preferred && index < previous)
            }) {
                primary[parent] = Some((index, target_names[name], preferred));
            }
        }
    }
    for (&name, &index) in &source_names {
        let Some(&target_index) = target_names.get(name) else {
            continue;
        };
        let source_parent = source.nodes[index].parent;
        let target_parent = target.nodes[target_index].parent;
        let source_parent_name =
            source_parent.and_then(|p| source.nodes[p].name.strip_prefix(PREFIX));
        let target_parent_name =
            target_parent.and_then(|p| target.nodes[p].name.strip_prefix(PREFIX));
        if source_parent_name != target_parent_name
            || source_parent_name.is_some_and(|p| !target_names.contains_key(p))
        {
            return Err(format!("关节 {name} 的父级与动作骨架不兼容"));
        }
        let source_rotation = s.rotation(index)?;
        let target_rotation = t.rotation(target_index)?;
        let swing = primary[index]
            .filter(|(_, child, _)| target.nodes[*child].parent == Some(target_index))
            .and_then(|(source_child, target_child, _)| {
                let a = (t.position(target_child) - t.position(target_index)).try_normalize()?;
                let b = (s.position(source_child) - s.position(index)).try_normalize()?;
                Some(Quat::from_rotation_arc(a, facing * b))
            })
            .unwrap_or(Quat::IDENTITY);
        // 目标世界转动 = 朝向 × 源世界转动 × 静止偏移。静止关节线校正让 T 姿势
        // 的手臂不把 A 姿势的下垂角重复叠加；骨骼长度仍来自目标的本地位移。
        offsets[index] =
            Some(source_rotation.inverse() * facing.inverse() * swing * target_rotation);
        names.insert(target.nodes[target_index].name.clone(), index);
    }
    let mut corrections = vec![None; source.nodes.len()];
    for (&name, &index) in &source_names {
        let Some(after) = offsets[index] else {
            continue;
        };
        let target_index = target_names[name];
        let source_parent = source.nodes[index].parent;
        let before = if let Some(parent_offset) = source_parent.and_then(|p| offsets[p]) {
            parent_offset.inverse()
        } else {
            let source_parent = source_parent.map_or(Ok(Quat::IDENTITY), |p| s.rotation(p))?;
            let target_parent = target.nodes[target_index]
                .parent
                .map_or(Ok(Quat::IDENTITY), |p| t.rotation(p))?;
            // 包装根只参与坐标基线，忽略其动画位移与缩放。父骨偏移抵消后采样只需两个四元数。
            target_parent.inverse() * facing * source_parent
        };
        corrections[index] = Some(JointCorrection {
            rest: t.local[target_index],
            before,
            after,
        });
    }
    let clips = source
        .clips
        .iter()
        .map(|clip| {
            clip.channels
                .iter()
                .any(|channel| channel.path == "rotation" && corrections[channel.node].is_some())
        })
        .collect::<Vec<_>>();
    if !clips.iter().any(|supports| *supports) {
        return Err("动作文件没有可重定向的人形旋转通道".into());
    }

    let mut minimum = Vec3::splat(f32::INFINITY);
    let mut maximum = Vec3::splat(f32::NEG_INFINITY);
    let mut include = |point: Vec3| {
        if point.is_finite() {
            minimum = minimum.min(point);
            maximum = maximum.max(point);
        }
    };
    for &index in target_names.values() {
        include(t.position(index));
    }
    for (index, lower, upper) in target_bounds {
        for x in [lower.x, upper.x] {
            for y in [lower.y, upper.y] {
                for z in [lower.z, upper.z] {
                    include(t.world[index].transform_point3(Vec3::new(x, y, z)));
                }
            }
        }
    }
    let margin = Vec3::splat(((maximum - minimum).max_element() * 0.06).max(0.005));
    let rig = RigRetarget {
        corrections,
        clips,
        bounds: AssetPreviewModelBounds {
            minimum: minimum - margin,
            maximum: maximum + margin,
        },
    };
    Ok(SkeletonMapping {
        names,
        retarget: Some(Arc::new(rig)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn relative_rotation_preserves_target_length_scale_and_calibrates_arm_angle() {
        let rest = Transform::from_translation(Vec3::new(0.65, -0.32, 0.0))
            .with_rotation(Quat::from_rotation_z(-0.45))
            .with_scale(Vec3::splat(1.2));
        let correction = JointCorrection {
            rest,
            before: Quat::IDENTITY,
            // 目标 A 姿势静止轴转到源 T 姿势后，源下垂角只会应用一次。
            after: Quat::from_rotation_z(0.45) * rest.rotation,
        };
        let rig = RigRetarget {
            corrections: vec![Some(correction)],
            clips: vec![true, false],
            bounds: AssetPreviewModelBounds {
                minimum: Vec3::ZERO,
                maximum: Vec3::ONE,
            },
        };
        let source = Transform::from_translation(Vec3::X * 3.0)
            .with_rotation(Quat::from_rotation_z(-0.9))
            .with_scale(Vec3::splat(0.01));
        let pose = rig.sample(&[source])[0].unwrap();
        assert_eq!(pose.translation, rest.translation);
        assert_eq!(pose.scale, rest.scale);
        assert!(pose.rotation.abs_diff_eq(source.rotation, 1e-5));
        assert!(rig.supports(0));
        assert!(!rig.supports(1));
    }

    fn test_rig(scale: f32, yaw: f32, arm_angle: f32) -> AnimationDocument {
        let joint = |name, children: &[usize], at: Vec3, rotation: Quat| {
            json!({
                "name": format!("{PREFIX}{name}"),
                "children": children,
                "translation": (at / scale).to_array(),
                "rotation": rotation.to_array(),
            })
        };
        let identity = Quat::IDENTITY;
        let nodes = vec![
            json!({"name":"Wrapper", "children":[1], "scale":[scale,scale,scale], "rotation":Quat::from_rotation_y(yaw).to_array()}),
            joint("Hips", &[2, 10, 13], Vec3::Y, identity),
            joint("Spine", &[3, 4, 7], Vec3::Y * 0.4, identity),
            joint("Head", &[], Vec3::Y * 0.4, identity),
            joint(
                "LeftArm",
                &[5],
                Vec3::X * 0.2,
                Quat::from_rotation_z(arm_angle),
            ),
            joint("LeftForeArm", &[6], Vec3::X * 0.34, identity),
            joint("LeftHand", &[], Vec3::X * 0.28, identity),
            joint(
                "RightArm",
                &[8],
                Vec3::NEG_X * 0.2,
                Quat::from_rotation_z(-arm_angle),
            ),
            joint("RightForeArm", &[9], Vec3::NEG_X * 0.34, identity),
            joint("RightHand", &[], Vec3::NEG_X * 0.28, identity),
            joint("LeftUpLeg", &[11], Vec3::X * 0.12, identity),
            joint("LeftLeg", &[12], Vec3::NEG_Y * 0.42, identity),
            joint("LeftFoot", &[], Vec3::NEG_Y * 0.42, identity),
            joint("RightUpLeg", &[14], Vec3::NEG_X * 0.12, identity),
            joint("RightLeg", &[15], Vec3::NEG_Y * 0.42, identity),
            joint("RightFoot", &[], Vec3::NEG_Y * 0.42, identity),
            json!({"name":"Body", "mesh":0, "skin":0}),
        ];
        super::super::data::tests::test_scene(&json!({
            "asset": {"version": "2.0"},
            "nodes": nodes,
            "scenes": [{"nodes":[0,16]}],
            "skins": [{"joints": (1..16).collect::<Vec<_>>()}],
            "meshes": [{"primitives":[{"attributes":{"POSITION":0}}]}],
            "accessors": [{"min":[-1.0,0.0,-0.5],"max":[1.0,2.0,0.5]}],
        }))
    }

    #[test]
    fn fitted_a_pose_uses_source_t_pose_motion_without_copying_root_motion_or_bone_length() {
        let mut source = test_rig(0.01, std::f32::consts::PI, 0.0);
        let mut target = test_rig(1.0, 0.0, -0.45);
        target.nodes[5].translation[0] = 0.26;
        source.clips = super::super::data::tests::test_document().clips;
        source.clips[0].channels.clear(); // 保留原 GLB 序号，跳过仅含不支持通道的片段。
        let rotation = &mut source.clips[1].channels[0];
        rotation.node = 4;
        rotation.path = "rotation".into();
        let lower_arm = Quat::from_rotation_z(-0.9);
        for key in &mut rotation.keys {
            key.value = lower_arm.to_array().to_vec();
        }
        let mapped = mapping(&source, &target).unwrap();
        let retarget = mapped.retarget.unwrap();
        assert!(!retarget.supports(0) && retarget.supports(1));
        assert_eq!(mapped.names.len(), 15);

        let mut sample = RestFrame::new(&source.nodes).local;
        sample[1].translation.x += 80.0;
        sample[4].rotation = lower_arm;
        sample[5].translation = Vec3::X * 90.0;
        let corrected = retarget.sample(&sample);
        assert_eq!(corrected[1].unwrap().translation, Vec3::Y);
        let mut local = RestFrame::new(&target.nodes).local;
        for (local, corrected) in local.iter_mut().zip(corrected) {
            if let Some(corrected) = corrected {
                *local = corrected;
            }
        }
        let world = world_matrices(&target.nodes, &local);
        let arm = world[5].w_axis.truncate() - world[4].w_axis.truncate();
        assert!(arm.abs_diff_eq(lower_arm * Vec3::X * 0.26, 1e-5));
        assert!(retarget.bounds.maximum.y > 2.0);
        target.nodes[5].parent = Some(2);
        assert!(mapping(&source, &target).is_err());
    }
}
