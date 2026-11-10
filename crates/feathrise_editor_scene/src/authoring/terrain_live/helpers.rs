//! 未保存 GLB 地形接管所需的物理形状身份判断。

use std::sync::Arc;

use avian3d::prelude::Collider;

/// 动态地形无论来自运行时GLB还是F12派生，都必须使用实际拓扑Trimesh。
pub(super) fn collider_contains_trimesh(collider: &Collider) -> bool {
    if collider.shape().as_trimesh().is_some() {
        return true;
    }
    collider.shape().as_compound().is_some_and(|compound| {
        !compound.shapes().is_empty()
            && compound
                .shapes()
                .iter()
                .all(|(_, shape)| shape.as_trimesh().is_some())
    })
}

/// Clone出来的候选共享同一Parry Arc；仅形状类型相同不足以证明修订一致。
pub(super) fn colliders_share_shape(current: &Collider, expected: &Collider) -> bool {
    Arc::ptr_eq(&current.shape().0, &expected.shape().0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::Vec3;

    #[test]
    fn only_the_same_trimesh_candidate_passes_identity_check() {
        let candidate = Collider::trimesh(vec![Vec3::ZERO, Vec3::X, Vec3::Z], vec![[0, 1, 2]]);
        let clone = candidate.clone();
        let rebuilt = Collider::trimesh(vec![Vec3::ZERO, Vec3::X, Vec3::Z], vec![[0, 1, 2]]);
        assert!(collider_contains_trimesh(&candidate));
        assert!(colliders_share_shape(&candidate, &clone));
        assert!(!colliders_share_shape(&candidate, &rebuilt));
        assert!(!collider_contains_trimesh(&Collider::cuboid(1.0, 1.0, 1.0)));
    }
}
