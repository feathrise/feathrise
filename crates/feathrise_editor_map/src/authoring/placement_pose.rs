//! 摆放记录的源局部与世界姿态边界；三维地形保留父级非等比缩放形成的完整仿射组合。

use bevy::prelude::*;
use feathrise_map_content::TerrainObjectTransform;

/// 自动体积的记录、接缝与驻留根由专用雕刻/拓扑链共同拥有，普通选择、变换和批删不得接管。
/// 两个标志独立检查以失败关闭；普通外部GLB三维地形仍沿用完整源坐标编辑能力。
pub(crate) fn is_automatic_volume(record: &feathrise_map_content::MapPlacementRecord) -> bool {
    record.terrain_patch.as_ref().is_some_and(|patch| patch.sculpted_volume || patch.streamed_volume)
}

/// 普通摆放保存世界姿态；独立三维地形保存所属启用Clipmap的源局部姿态。
pub(crate) fn placement_record_frame(
    draft: &super::resources::EditorMapDraft,
    record: &feathrise_map_content::MapPlacementRecord,
) -> Result<TerrainObjectTransform, String> {
    let Some(patch) = &record.terrain_patch else { return Ok(Default::default()); };
    let id = patch.terrain_id.clone().unwrap_or_else(feathrise_map_content::TerrainId::legacy);
    if let Some(objects) = &draft.manifest.terrains {
        let source = objects.iter().find(|object| object.id == id
            && object.enabled && object.terrain.kind == feathrise_map_content::MapTerrainKind::SparseClipmap)
            .ok_or_else(|| format!("三维地形{}关联的Clipmap不存在或已禁用", record.id))?;
        source.transform.validate()?;
        return Ok(source.transform);
    }
    if id != feathrise_map_content::TerrainId::legacy()
        || draft.manifest.terrain.kind != feathrise_map_content::MapTerrainKind::SparseClipmap {
        return Err(format!("三维地形{}未关联有效的旧主Clipmap", record.id));
    }
    Ok(Default::default()) // 旧主地形的作者与世界坐标一致，不借当前画布重复叠加变换。
}

/// 框选、手柄与多选缓存统一读取实际世界原点；缺失源不能降级为看似有效的局部点。
pub(crate) fn placement_record_world_position(
    draft: &super::resources::EditorMapDraft,
    record: &feathrise_map_content::MapPlacementRecord,
) -> Result<Vec3, String> {
    let frame = placement_record_frame(draft, record)?;
    let point = frame.as_transform().transform_point(Vec3::from_array(record.translation));
    if !point.is_finite() { return Err(format!("资产{}的世界位置无效", record.id)); }
    Ok(point)
}

// 只容忍f32矩阵乘法和求逆的舍入残差；按单位基向量比较，不能因世界坐标很大而放宽倾斜/剪切。
const BASIS_TOLERANCE: f32 = 32.0 * f32::EPSILON;

/// 将所属地形的源局部记录转为完整世界矩阵；不能先拆TRS，否则会丢失合法父子组合的剪切。
pub(crate) fn source_to_world(
    pose: TerrainObjectTransform,
    source: TerrainObjectTransform,
) -> Result<Mat4, String> {
    let local = checked_pose(pose, "三维地形源局部姿态")?;
    if pose.pitch_degrees != 0.0 || pose.roll_degrees != 0.0 {
        return Err("三维地形记录只支持绕Y轴旋转，不能携带俯仰或翻滚".into());
    }
    let source = checked_pose(source, "三维地形所属源姿态")?;
    let world = source * local;
    checked_matrix(world, "三维地形世界姿态")?;
    Ok(world)
}

/// 确认时转入真实目标地形的源局部空间，仅接受可由现有yaw及正缩放格式重建的结果。
pub(crate) fn world_to_source(
    world: Mat4,
    target: TerrainObjectTransform,
) -> Result<TerrainObjectTransform, String> {
    checked_matrix(world, "三维地形世界姿态")?;
    let target = checked_pose(target, "三维地形目标源姿态")?;
    let local = target.inverse() * world;
    checked_matrix(local, "三维地形目标局部姿态")?;

    let axes = [local.x_axis.truncate(), local.y_axis.truncate(), local.z_axis.truncate()];
    let scale = Vec3::new(axes[0].length(), axes[1].length(), axes[2].length());
    if !scale.is_finite() || scale.cmple(Vec3::ZERO).any() {
        return Err("三维地形目标局部缩放必须有限且大于零".into());
    }
    let basis = [axes[0] / scale.x, axes[1] / scale.y, axes[2] / scale.z];
    if basis[0].dot(basis[1]).abs() > BASIS_TOLERANCE
        || basis[0].dot(basis[2]).abs() > BASIS_TOLERANCE
        || basis[1].dot(basis[2]).abs() > BASIS_TOLERANCE
    {
        return Err("三维地形在目标源局部空间产生剪切，现有姿态格式无法准确保存".into());
    }
    // yaw正缩放的本地Y轴必须朝上，X/Z轴必须水平；拒绝用丢弃pitch/roll的分量近似改变模型。
    if !basis[1].abs_diff_eq(Vec3::Y, BASIS_TOLERANCE)
        || basis[0].y.abs() > BASIS_TOLERANCE
        || basis[2].y.abs() > BASIS_TOLERANCE
    {
        return Err("三维地形在目标源局部空间包含俯仰或翻滚，现有记录只支持绕Y轴旋转".into());
    }
    let pose = TerrainObjectTransform {
        translation: local.w_axis.truncate().to_array(),
        yaw_degrees: (-basis[0].z).atan2(basis[0].x).to_degrees().rem_euclid(360.0),
        scale: scale.to_array(),
        ..Default::default()
    };
    let rebuilt = checked_pose(pose, "三维地形目标局部姿态")?;
    let rebuilt_axes = [rebuilt.x_axis.truncate(), rebuilt.y_axis.truncate(), rebuilt.z_axis.truncate()];
    if axes.into_iter().zip(rebuilt_axes).zip(scale.to_array()).any(|((actual, expected), size)|
        !(actual / size).abs_diff_eq(expected / size, BASIS_TOLERANCE))
    {
        return Err("三维地形目标局部姿态无法由绕Y轴旋转和正缩放准确重建".into());
    }
    Ok(pose) // 洞口直角旋转及所属源范围仍由原terrain_cutouts统一校验，不在此另设合同。
}

fn checked_pose(pose: TerrainObjectTransform, label: &str) -> Result<Mat4, String> {
    pose.validate().map_err(|error| format!("{label}无效：{error}"))?;
    let matrix = pose.as_transform().to_matrix();
    checked_matrix(matrix, label)?;
    Ok(matrix)
}

fn checked_matrix(matrix: Mat4, label: &str) -> Result<(), String> {
    let determinant = matrix.determinant();
    if !matrix.is_finite() || !determinant.is_finite() || determinant <= 0.0
        || !matrix.inverse().is_finite()
    {
        return Err(format!("{label}必须有限、可逆且使用正缩放"));
    }
    if matrix.x_axis.w.abs() > BASIS_TOLERANCE || matrix.y_axis.w.abs() > BASIS_TOLERANCE
        || matrix.z_axis.w.abs() > BASIS_TOLERANCE || (matrix.w_axis.w - 1.0).abs() > BASIS_TOLERANCE
    {
        return Err(format!("{label}必须是仿射矩阵，不能包含投影分量"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_pose() -> TerrainObjectTransform {
        TerrainObjectTransform {
            translation: [-120.0, 8.0, 73.0],
            yaw_degrees: 31.0,
            pitch_degrees: 17.0,
            roll_degrees: -12.0,
            scale: [2.0, 3.0, 5.0],
        }
    }

    fn local_pose() -> TerrainObjectTransform {
        TerrainObjectTransform {
            translation: [4.0, -1.0, 6.0],
            yaw_degrees: 37.0,
            scale: [0.75, 1.25, 2.5],
            ..Default::default()
        }
    }

    fn assert_same_geometry(actual: Mat4, expected: Mat4) {
        // 比较原点与三根独立轴上的实际点，避免只比较欧拉角而漏过剪切或缩放错误。
        for point in [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z, Vec3::new(-2.0, 3.0, 5.0)] {
            assert!(actual.transform_point3(point).abs_diff_eq(expected.transform_point3(point), 0.0002),
                "point={point:?}, actual={actual:?}, expected={expected:?}");
        }
    }

    #[test]
    fn same_source_round_trip_preserves_rotated_nonuniform_parent_and_world_shear() {
        let source = source_pose();
        let local = local_pose();
        let world = source_to_world(local, source).unwrap();
        let x = world.x_axis.truncate().normalize();
        let z = world.z_axis.truncate().normalize();
        assert!(x.dot(z).abs() > 0.1, "夹具必须包含真实世界剪切，不能退化为普通TRS");
        assert_same_geometry(world, source.as_transform().to_matrix() * local.as_transform().to_matrix());
        let restored = world_to_source(world, source).unwrap();
        assert_eq!(restored.pitch_degrees, 0.0);
        assert_eq!(restored.roll_degrees, 0.0);
        assert_same_geometry(restored.as_transform().to_matrix(), local.as_transform().to_matrix());
    }

    #[test]
    fn cross_source_axis_swap_and_world_translation_preserve_complete_geometry() {
        let source = source_pose();
        // 目标在同一倾斜平面内换轴并交换XZ尺度；相对变换可准确写成yaw，必须允许这种跨源复制。
        let target = TerrainObjectTransform {
            translation: [-116.0, 11.0, 69.0],
            scale: [source.scale[2], source.scale[1], source.scale[0]],
            ..source
        }.with_rotation(source.rotation() * Quat::from_rotation_y(90.0_f32.to_radians()));
        let world = Mat4::from_translation(Vec3::new(3.0, -2.0, 5.0))
            * source_to_world(local_pose(), source).unwrap();
        let restored = world_to_source(world, target).unwrap();
        assert_same_geometry(target.as_transform().to_matrix() * restored.as_transform().to_matrix(), world);
    }

    #[test]
    fn incompatible_nonuniform_target_rejects_shear_instead_of_approximating_scale() {
        let world = source_to_world(local_pose(), TerrainObjectTransform::default()).unwrap();
        let target = TerrainObjectTransform { scale: [2.0, 1.0, 1.0], ..Default::default() };
        assert!(world_to_source(world, target).unwrap_err().contains("剪切"));
    }

    #[test]
    fn incompatible_tilted_source_rejects_pitch_and_roll_in_target_local_space() {
        for source in [
            TerrainObjectTransform { pitch_degrees: 20.0, ..Default::default() },
            TerrainObjectTransform { roll_degrees: -30.0, ..Default::default() },
            TerrainObjectTransform { pitch_degrees: 180.0, ..Default::default() },
        ] {
            let world = source_to_world(TerrainObjectTransform::default(), source).unwrap();
            assert!(world_to_source(world, TerrainObjectTransform::default()).unwrap_err().contains("俯仰或翻滚"));
        }
    }

    #[test]
    fn large_translation_does_not_hide_small_but_real_shear_or_tilt() {
        let translation = Mat4::from_translation(Vec3::splat(1_000_000.0));
        let mut shear = Mat4::IDENTITY;
        shear.z_axis.x = 0.0001;
        assert!(world_to_source(translation * shear, TerrainObjectTransform::default()).unwrap_err().contains("剪切"));
        let tilted = translation * Mat4::from_rotation_x(0.01_f32.to_radians());
        assert!(world_to_source(tilted, TerrainObjectTransform::default()).unwrap_err().contains("俯仰或翻滚"));
    }

    #[test]
    fn nonpositive_nonfinite_singular_and_projective_inputs_fail_closed() {
        for scale in [[0.0, 1.0, 1.0], [-1.0, 1.0, 1.0], [f32::INFINITY, 1.0, 1.0]] {
            let invalid = TerrainObjectTransform { scale, ..Default::default() };
            assert!(source_to_world(invalid, TerrainObjectTransform::default()).is_err());
            assert!(source_to_world(TerrainObjectTransform::default(), invalid).is_err());
            assert!(world_to_source(Mat4::IDENTITY, invalid).is_err());
        }
        let tilted_local = TerrainObjectTransform { pitch_degrees: 1.0, ..Default::default() };
        assert!(source_to_world(tilted_local, TerrainObjectTransform::default()).is_err());
        for world in [Mat4::ZERO, Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)),
            Mat4::from_translation(Vec3::new(f32::NAN, 0.0, 0.0)), bevy::math::proj::perspective(1.0, 1.0, 0.1, 100.0)]
        {
            assert!(world_to_source(world, TerrainObjectTransform::default()).is_err());
        }
    }
}
