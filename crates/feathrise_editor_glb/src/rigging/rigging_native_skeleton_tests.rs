//! 以合成的中性人形验证关节点估计、朝向与骨架映射，不依赖私有角色资源。

use super::*;

fn person(lowered_arms: bool) -> Geometry {
    let mut vertices = Vec::new();
    let mut segment = |center: Vec3, radius: Vec3, angle: f32| {
        let orientation = Quat::from_rotation_z(angle);
        for band in 0..=12 {
            let latitude = band as f32 * std::f32::consts::PI / 12.0;
            for step in 0..24 {
                let azimuth = step as f32 * std::f32::consts::TAU / 24.0;
                let surface = Vec3::new(
                    latitude.sin() * azimuth.cos(),
                    latitude.cos(),
                    latitude.sin() * azimuth.sin(),
                );
                vertices.push(center + orientation * (surface * radius));
            }
        }
    };
    segment(Vec3::new(0.0, 1.24, 0.0), Vec3::new(0.19, 0.32, 0.11), 0.0);
    segment(Vec3::new(0.0, 1.71, 0.0), Vec3::new(0.12, 0.15, 0.10), 0.0);
    segment(Vec3::new(0.0, 1.0, 0.0), Vec3::new(0.17, 0.12, 0.11), 0.0);
    for sign in [-1.0, 1.0] {
        segment(
            Vec3::new(sign * 0.105, 0.75, 0.0),
            Vec3::new(0.085, 0.26, 0.079),
            0.0,
        );
        segment(
            Vec3::new(sign * 0.11, 0.31, 0.0),
            Vec3::new(0.071, 0.24, 0.066),
            0.0,
        );
        segment(
            Vec3::new(sign * 0.11, 0.07, 0.075),
            Vec3::new(0.069, 0.07, 0.13),
            0.0,
        );
        let (angle, arm_y, forearm_y, hand_y) = if lowered_arms {
            (-sign * 0.53, 1.39, 1.25, 1.15)
        } else {
            (0.0, 1.46, 1.46, 1.46)
        };
        segment(
            Vec3::new(sign * 0.29, arm_y, 0.0),
            Vec3::new(0.19, 0.069, 0.067),
            angle,
        );
        segment(
            Vec3::new(sign * 0.55, forearm_y, 0.0),
            Vec3::new(0.17, 0.059, 0.058),
            angle,
        );
        segment(
            Vec3::new(sign * 0.71, hand_y, 0.0),
            Vec3::new(0.085, 0.048, 0.05),
            angle,
        );
    }
    // 明确的前脚掌用于核对转身时的角色自身左右。
    for sign in [-1.0, 1.0] {
        for i in 0..64 {
            let x = sign * (0.09 + (i % 8) as f32 * 0.004);
            vertices.push(Vec3::new(x, 0.02, 0.20));
            vertices.push(Vec3::new(x, 0.12, 0.03));
        }
    }
    let count = vertices.len();
    Geometry {
        vertices,
        triangles: Vec::new(),
        parts: vec![0..count],
    }
}

#[test]
fn estimates_t_and_a_poses_with_anatomical_sides_after_turning() {
    let cancel = AtomicBool::new(false);
    for lowered_arms in [false, true] {
        let mut model = person(lowered_arms);
        let (points, _) = estimate(&model, &cancel).unwrap();
        assert_eq!(points.len(), 17);
        assert!(points["left_wrist"][0] > 0.45);
        assert!(points["right_wrist"][0] < -0.45);
        if lowered_arms {
            assert!(points["left_shoulder"][1] - points["left_wrist"][1] > 0.12);
        }
        for point in &mut model.vertices {
            *point = Vec3::new(-point.x, point.y, -point.z);
        }
        let (turned, _) = estimate(&model, &cancel).unwrap();
        assert!(turned["left_wrist"][0] < -0.45);
        assert!(turned["right_wrist"][0] > 0.45);
    }
}

#[test]
fn only_separate_outlier_is_excluded_from_detection() {
    let cancel = AtomicBool::new(false);
    let mut model = person(false);
    let (before, _) = estimate(&model, &cancel).unwrap();
    let start = model.vertices.len();
    model.vertices.extend([
        Vec3::new(0.6, 0.4, 0.1),
        Vec3::new(0.6, 2.8, 0.1),
        Vec3::new(0.65, 2.8, 0.1),
    ]);
    model.parts.push(start..model.vertices.len());
    let (after, warnings) = estimate(&model, &cancel).unwrap();
    assert_eq!(before, after);
    assert!(warnings.iter().any(|message| message.contains("独立部件")));

    let mut incomplete = person(false);
    incomplete.vertices.retain(|p| !(p.x < -0.21 && p.y > 1.2));
    incomplete.parts[0].end = incomplete.vertices.len();
    assert!(estimate(&incomplete, &cancel).unwrap_err().contains("手臂"));
}

fn reference(points: &Landmarks) -> Vec<Joint> {
    let mut joints: Vec<Joint> = Vec::new();
    let mut add = |name: &str, parent: Option<&str>, position: Vec3| {
        let parent = parent.map(|parent| {
            let name = format!("mixamorig:{parent}");
            joints.iter().position(|joint| joint.name == name).unwrap()
        });
        joints.push(Joint {
            name: format!("mixamorig:{name}"),
            parent,
            position,
            rotation: Quat::from_rotation_x(0.37),
        });
    };
    let hips = marker(points, "hips");
    let neck = marker(points, "neck");
    add("Hips", None, hips);
    let mut parent = "Hips";
    for (spine, fraction) in SPINE {
        add(spine, Some(parent), hips.lerp(neck, fraction));
        parent = spine;
    }
    add("Neck", Some("Spine2"), neck);
    add("Head", Some("Neck"), marker(points, "head"));
    for (side, prefix, _) in SIDES {
        let shoulder = format!("{prefix}Shoulder");
        add(
            &shoulder,
            Some("Spine2"),
            neck.lerp(marker(points, &format!("{side}_shoulder")), 0.45),
        );
        let mut parent = shoulder;
        for (i, (point, name)) in LIMB_MARKERS.into_iter().enumerate() {
            if i == 3 {
                parent = "Hips".into();
            }
            let name = format!("{prefix}{name}");
            add(
                &name,
                Some(&parent),
                marker(points, &format!("{side}_{point}")),
            );
            parent = name;
        }
        let wrist = marker(points, &format!("{side}_wrist"));
        let elbow = marker(points, &format!("{side}_elbow"));
        add(
            &format!("{prefix}HandMiddle1"),
            Some(&format!("{prefix}Hand")),
            wrist + (wrist - elbow).normalize() * 0.04,
        );
    }
    // GLB skin.joints 的排列允许子节点先于父节点。
    let last = joints.len() - 1;
    joints.reverse();
    for joint in &mut joints {
        joint.parent = joint.parent.map(|parent| last - parent);
    }
    joints
}

#[test]
fn fitted_joints_keep_identity_and_align_reference_axes() {
    let cancel = AtomicBool::new(false);
    let (source, _) = estimate(&person(false), &cancel).unwrap();
    let reference = reference(&source);
    let model = person(true);
    let (markers, _) = estimate(&model, &cancel).unwrap();
    let fitted = fit(&model, &reference, &markers, &cancel).unwrap();
    assert_eq!(fitted.len(), reference.len());
    for (before, after) in reference.iter().zip(&fitted) {
        assert_eq!(before.name, after.name);
        assert_eq!(before.parent, after.parent);
    }
    let index = |name: &str| {
        reference
            .iter()
            .position(|joint| joint.name == name)
            .unwrap()
    };
    let wrist = index("mixamorig:LeftHand");
    let elbow = index("mixamorig:LeftForeArm");
    assert_eq!(fitted[wrist].position, marker(&markers, "left_wrist"));
    let direction = (reference[wrist].position - reference[elbow].position).normalize();
    let local = reference[elbow].rotation.inverse() * direction;
    let expected = (fitted[wrist].position - fitted[elbow].position).normalize();
    assert!((fitted[elbow].rotation * local).distance(expected) < 1e-5);
    let finger = index("mixamorig:LeftHandMiddle1");
    assert!(fitted[finger].position.distance(fitted[wrist].position) > 0.01);

    cancel.store(true, Ordering::Release);
    assert!(
        matches!(fit(&model, &reference, &markers, &cancel), Err(error) if error.contains("取消"))
    );
}
