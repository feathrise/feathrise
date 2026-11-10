//! 验证真实三角连通关系、左右肢体隔离、原顶点映射和无种子回退。

use super::*;
use bevy::math::Quat;

fn joint(name: &str, parent: Option<usize>, x: f32, y: f32) -> Joint {
    Joint {
        name: format!("mixamorig:{name}"),
        parent,
        position: Vec3::new(x, y, 0.0),
        rotation: Quat::IDENTITY,
    }
}

fn geometry() -> Geometry {
    Geometry {
        vertices: Vec::new(),
        triangles: Vec::new(),
        parts: Vec::new(),
    }
}

fn strip(geometry: &mut Geometry, xs: &[f32], y: f32, z: f32) -> usize {
    let start = geometry.vertices.len();
    for &x in xs {
        geometry
            .vertices
            .extend([Vec3::new(x, y - 0.015, z), Vec3::new(x, y + 0.015, z)]);
    }
    for column in 0..xs.len() - 1 {
        let first = (start + 2 * column) as u32;
        geometry.triangles.extend([
            [first, first + 2, first + 1],
            [first + 1, first + 2, first + 3],
        ]);
    }
    start
}

fn body_height(geometry: &mut Geometry) {
    let first = geometry.vertices.len();
    geometry.vertices.extend([
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.08),
        Vec3::new(0.0, 2.0, 0.0),
    ]);
    geometry
        .triangles
        .push([first as u32, first as u32 + 1, first as u32 + 2]);
    geometry.parts.push(first..geometry.vertices.len());
}

fn arms() -> Vec<Joint> {
    // 前臂先于父节点出现，输出仍使用原骨架数组的关节索引。
    vec![
        joint("LeftForeArm", Some(2), 0.55, 1.45),
        joint("LeftHand", Some(0), 0.85, 1.45),
        joint("LeftArm", Some(4), 0.25, 1.45),
        joint("Hips", None, 0.0, 0.9),
        joint("Spine", Some(3), 0.0, 1.28),
        joint("RightArm", Some(4), -0.25, 1.45),
        joint("RightForeArm", Some(5), -0.55, 1.45),
        joint("RightHand", Some(6), -0.85, 1.45),
    ]
}

fn influence(skin: &VertexSkin, joint: u16) -> f32 {
    skin.joints
        .iter()
        .zip(skin.weights)
        .filter(|(index, _)| **index == joint)
        .map(|(_, weight)| weight)
        .sum()
}

fn assert_normalized(skins: &[VertexSkin], joint_count: usize) {
    for skin in skins {
        assert!((skin.weights.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(
            skin.weights
                .iter()
                .all(|weight| weight.is_finite() && (0.0..=1.0).contains(weight))
        );
        assert!(skin.weights.windows(2).all(|pair| pair[0] >= pair[1]));
        assert!(
            skin.joints
                .iter()
                .all(|joint| (*joint as usize) < joint_count)
        );
    }
}

#[test]
fn uv_seam_vertices_share_the_surface_blend_between_upper_arm_and_forearm() {
    let mut geometry = geometry();
    let upper = strip(&mut geometry, &[0.35, 0.45, 0.55], 1.45, 0.06);
    let lower = strip(&mut geometry, &[0.55, 0.65, 0.75], 1.45, 0.06);
    // 模拟不同 primitive 读入后略有浮点差异的接缝副本。
    geometry.vertices[lower].x += 0.000002;
    geometry.parts.push(0..geometry.vertices.len());
    body_height(&mut geometry);
    let joints = arms();
    let (skins, _) = solve(&geometry, &joints, &AtomicBool::new(false)).unwrap();
    assert_eq!(skins.len(), geometry.vertices.len());
    assert_normalized(&skins, joints.len());
    for row in 0..2 {
        let a = &skins[upper + 4 + row];
        let b = &skins[lower + row];
        assert_eq!(a.joints, b.joints);
        assert_eq!(a.weights, b.weights);
        assert!(influence(a, 0) > 0.35);
        assert!(influence(a, 2) > 0.35);
    }
    assert!(influence(&skins[upper], 2) > 0.95);
    assert!(influence(&skins[lower + 4], 0) > 0.95);
}

#[test]
fn connected_garment_does_not_bleed_weights_to_the_opposite_leg() {
    let mut geometry = geometry();
    let garment = strip(&mut geometry, &[-0.09, -0.03, 0.03, 0.09], 0.4, 0.025);
    geometry.parts.push(0..geometry.vertices.len());
    body_height(&mut geometry);
    let joints = vec![
        joint("Hips", None, 0.0, 1.0),
        joint("Spine", Some(0), 0.0, 1.3),
        joint("LeftUpLeg", Some(0), 0.075, 0.95),
        joint("LeftLeg", Some(2), 0.075, 0.5),
        joint("LeftFoot", Some(3), 0.075, 0.1),
        joint("RightUpLeg", Some(0), -0.075, 0.95),
        joint("RightLeg", Some(5), -0.075, 0.5),
        joint("RightFoot", Some(6), -0.075, 0.1),
    ];
    let (skins, _) = solve(&geometry, &joints, &AtomicBool::new(false)).unwrap();
    assert_normalized(&skins, joints.len());
    let right = &skins[garment];
    let left = &skins[garment + 6];
    assert!(influence(right, 6) > 0.9);
    assert!(influence(left, 3) > 0.9);
    assert_eq!(influence(right, 3), 0.0);
    assert_eq!(influence(left, 6), 0.0);
}

#[test]
fn disconnected_or_unreferenced_vertices_receive_explicit_fallback() {
    let mut geometry = geometry();
    let attached = strip(&mut geometry, &[0.35, 0.45, 0.55], 1.45, 0.06);
    geometry.parts.push(0..geometry.vertices.len());
    let separate = geometry.vertices.len();
    // 与可种子的主网格顶点重合，但不同部件的独立点不获得虚假的表面连接。
    geometry.vertices.push(geometry.vertices[attached]);
    geometry.parts.push(separate..geometry.vertices.len());
    body_height(&mut geometry);
    let joints = arms();
    let (skins, warnings) = solve(&geometry, &joints, &AtomicBool::new(false)).unwrap();
    assert_normalized(&skins, joints.len());
    assert!(warnings.iter().any(|warning| warning.contains("骨段距离")));
    assert!(influence(&skins[separate], 2) > 0.95);
}

#[test]
fn invalid_topology_and_cancel_fail_before_publishing_skin() {
    let mut geometry = geometry();
    body_height(&mut geometry);
    geometry.triangles[0][2] = 4;
    let joints = arms();
    assert!(
        solve(&geometry, &joints, &AtomicBool::new(false))
            .err()
            .unwrap()
            .contains("越界")
    );
    assert!(
        solve(&geometry, &joints, &AtomicBool::new(true))
            .err()
            .unwrap()
            .contains("取消")
    );
}
