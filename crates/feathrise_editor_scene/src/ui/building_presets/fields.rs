//! 建筑预设中央表单字段与正式强类型描述之间的唯一读写映射。

use feathrise_scene_content::{BuildingPresetDescriptor, valid_building_preset_id}; // 表单直接编辑公共持久格式，不维护第二份影子数据。

use super::types::BuildingPresetField; // 强类型字段避免用字符串路径误写相邻几何参数。

const MAX_LOCAL_DISTANCE: f32 = 100_000.0; // 与公共建筑目录校验的局部距离上限保持一致。
const MAX_LABEL_CHARS: usize = 128; // 与公共目录文字上限保持一致。

/// 把一个正式字段格式化为当前输入缓冲的初始文本。
pub(super) fn field_value(
    descriptor: &BuildingPresetDescriptor,
    field: BuildingPresetField,
) -> String {
    let number = match field {
        BuildingPresetField::Label => return descriptor.label.clone(), // 中文名称保留原 UTF-8 内容。
        BuildingPresetField::ConnectionId(index) => {
            return descriptor
                .connections
                .get(index)
                .map(|connection| connection.id.clone())
                .unwrap_or_default();
        }
        BuildingPresetField::ConnectionKind(index) => {
            return descriptor
                .connections
                .get(index)
                .map(|connection| connection.kind.clone())
                .unwrap_or_default();
        }
        BuildingPresetField::ConnectionX(index) => descriptor
            .connections
            .get(index)
            .map_or(0.0, |connection| connection.position[0]),
        BuildingPresetField::ConnectionZ(index) => descriptor
            .connections
            .get(index)
            .map_or(0.0, |connection| connection.position[1]),
        BuildingPresetField::ConnectionYaw(index) => descriptor
            .connections
            .get(index)
            .map_or(0.0, |connection| connection.outward_yaw_degrees),
        BuildingPresetField::FootprintCenterX => descriptor.footprint.center[0],
        BuildingPresetField::FootprintCenterZ => descriptor.footprint.center[1],
        BuildingPresetField::FootprintHalfX => descriptor.footprint.half_extents[0],
        BuildingPresetField::FootprintHalfZ => descriptor.footprint.half_extents[1],
        BuildingPresetField::EntranceX => descriptor.entrance.position[0],
        BuildingPresetField::EntranceY => descriptor.entrance.position[1],
        BuildingPresetField::EntranceZ => descriptor.entrance.position[2],
        BuildingPresetField::EntranceYaw => descriptor.entrance.outward_yaw_degrees,
        BuildingPresetField::EntranceHalfWidth => descriptor.entrance.clearance_half_extents[0],
        BuildingPresetField::EntranceHalfDepth => descriptor.entrance.clearance_half_extents[1],
        BuildingPresetField::FoundationBaseY => descriptor.foundation.model_base_y,
        BuildingPresetField::FoundationMaxDelta => descriptor.foundation.max_ground_height_delta,
        BuildingPresetField::VegetationCenterX => descriptor.vegetation_clearance.center[0],
        BuildingPresetField::VegetationCenterZ => descriptor.vegetation_clearance.center[1],
        BuildingPresetField::VegetationHalfX => descriptor.vegetation_clearance.half_extents[0],
        BuildingPresetField::VegetationHalfZ => descriptor.vegetation_clearance.half_extents[1],
    };
    number.to_string() // Rust 的有限浮点格式可无损回读当前 f32 值。
}

/// 校验并把一个已确认输入写入当前建筑草稿。
pub(super) fn commit_field(
    descriptor: &mut BuildingPresetDescriptor,
    field: BuildingPresetField,
    value: &str,
) -> Result<(), String> {
    if field == BuildingPresetField::Label {
        let label = value.trim(); // 名称首尾空白不作为作者内容持久化。
        if label.is_empty() {
            return Err("建筑显示名称不能为空".to_owned());
        }
        if label.chars().count() > MAX_LABEL_CHARS {
            return Err(format!("建筑显示名称不能超过{MAX_LABEL_CHARS}个字符"));
        }
        descriptor.label = label.to_owned(); // 稳定 ID 与显示名保持独立。
        return Ok(());
    }
    if let BuildingPresetField::ConnectionId(index) = field {
        let id = value.trim();
        if !valid_building_preset_id(id) {
            return Err("连接点ID只能使用1至128个ASCII字母、数字、下划线或短横线".to_owned());
        }
        if descriptor
            .connections
            .iter()
            .enumerate()
            .any(|(other_index, connection)| other_index != index && connection.id == id)
        {
            return Err(format!("连接点ID“{id}”在当前预设内重复"));
        }
        let connection = descriptor
            .connections
            .get_mut(index)
            .ok_or_else(|| "目标连接点已经不存在".to_owned())?;
        connection.id = id.to_owned(); // 修改本点语义时稳定ID由作者显式负责。
        return Ok(());
    }
    if let BuildingPresetField::ConnectionKind(index) = field {
        let kind = value.trim();
        if !valid_building_preset_id(kind) {
            return Err("连接兼容类型只能使用1至128个ASCII字母、数字、下划线或短横线".to_owned());
        }
        let connection = descriptor
            .connections
            .get_mut(index)
            .ok_or_else(|| "目标连接点已经不存在".to_owned())?;
        connection.kind = kind.to_owned(); // 首期只按完全相同字符串兼容。
        return Ok(());
    }
    let number = value
        .trim()
        .parse::<f32>()
        .map_err(|error| format!("建筑参数数值格式无效：{error}"))?;
    if !number.is_finite() {
        return Err("建筑参数必须是有限数值".to_owned()); // NaN 和无穷不能进入几何索引。
    }
    let signed = number.abs() <= MAX_LOCAL_DISTANCE; // 中心、门槛、基准和角度使用有界有符号值。
    let positive = number > 0.0 && number <= MAX_LOCAL_DISTANCE; // 矩形半尺寸必须严格为正。
    let non_negative = (0.0..=MAX_LOCAL_DISTANCE).contains(&number); // 坡差允许零但不能为负。
    match field {
        BuildingPresetField::FootprintCenterX if signed => descriptor.footprint.center[0] = number,
        BuildingPresetField::FootprintCenterZ if signed => descriptor.footprint.center[1] = number,
        BuildingPresetField::FootprintHalfX if positive => {
            descriptor.footprint.half_extents[0] = number
        }
        BuildingPresetField::FootprintHalfZ if positive => {
            descriptor.footprint.half_extents[1] = number
        }
        BuildingPresetField::EntranceX if signed => descriptor.entrance.position[0] = number,
        BuildingPresetField::EntranceY if signed => descriptor.entrance.position[1] = number,
        BuildingPresetField::EntranceZ if signed => descriptor.entrance.position[2] = number,
        BuildingPresetField::EntranceYaw if signed => {
            descriptor.entrance.outward_yaw_degrees = number.rem_euclid(360.0)
        }
        BuildingPresetField::EntranceHalfWidth if positive => {
            descriptor.entrance.clearance_half_extents[0] = number
        }
        BuildingPresetField::EntranceHalfDepth if positive => {
            descriptor.entrance.clearance_half_extents[1] = number
        }
        BuildingPresetField::FoundationBaseY if signed => {
            descriptor.foundation.model_base_y = number
        }
        BuildingPresetField::FoundationMaxDelta if non_negative => {
            descriptor.foundation.max_ground_height_delta = number
        }
        BuildingPresetField::VegetationCenterX if signed => {
            descriptor.vegetation_clearance.center[0] = number
        }
        BuildingPresetField::VegetationCenterZ if signed => {
            descriptor.vegetation_clearance.center[1] = number
        }
        BuildingPresetField::VegetationHalfX if positive => {
            descriptor.vegetation_clearance.half_extents[0] = number
        }
        BuildingPresetField::VegetationHalfZ if positive => {
            descriptor.vegetation_clearance.half_extents[1] = number
        }
        BuildingPresetField::ConnectionX(index) if signed => {
            descriptor
                .connections
                .get_mut(index)
                .ok_or_else(|| "目标连接点已经不存在".to_owned())?
                .position[0] = number
        }
        BuildingPresetField::ConnectionZ(index) if signed => {
            descriptor
                .connections
                .get_mut(index)
                .ok_or_else(|| "目标连接点已经不存在".to_owned())?
                .position[1] = number
        }
        BuildingPresetField::ConnectionYaw(index) if signed => {
            let normalized = number.rem_euclid(360.0);
            let quarter_turn = (normalized / 90.0).round() * 90.0;
            if (normalized - quarter_turn).abs() > 0.001 {
                return Err("连接点朝外角度必须是90°的整数倍".to_owned());
            }
            descriptor
                .connections
                .get_mut(index)
                .ok_or_else(|| "目标连接点已经不存在".to_owned())?
                .outward_yaw_degrees = quarter_turn.rem_euclid(360.0)
        }
        BuildingPresetField::Label
        | BuildingPresetField::ConnectionId(_)
        | BuildingPresetField::ConnectionKind(_) => {
            unreachable!("文字字段已经在数值解析前处理")
        }
        _ => {
            return Err(
                "数值超出允许范围：半尺寸必须大于0，坡差不能为负，其余局部参数需在±100000米内"
                    .to_owned(),
            );
        }
    }
    Ok(()) // 公共目录保存前仍会执行整份跨字段校验。
}

#[cfg(test)]
mod tests {
    use super::*; // 纯字段回归不创建窗口或文件任务。
    use feathrise_scene_content::{BuildingConnectionPointDescriptor, ScenePlacementAssetTarget}; // 构造合法公共建筑描述。

    #[test]
    fn numeric_fields_reject_degenerate_geometry_and_normalize_yaw() {
        let mut descriptor = BuildingPresetDescriptor::from_asset(
            "test-house".to_owned(),              // 稳定测试身份。
            "测试房屋".to_owned(),                // 合法中文名称。
            "gltf/buildings/test.glb".to_owned(), // 合法模型路径。
            ScenePlacementAssetTarget::Scene0,      // 使用完整场景目标。
        );
        assert!(commit_field(&mut descriptor, BuildingPresetField::FootprintHalfX, "0").is_err()); // 退化占地失败关闭。
        commit_field(&mut descriptor, BuildingPresetField::EntranceYaw, "-90")
            .expect("有限角度应能规范化");
        assert_eq!(descriptor.entrance.outward_yaw_degrees, 270.0); // 保存值保持统一角度区间。
    }

    #[test]
    fn connection_fields_require_stable_unique_ids_and_quarter_turns() {
        let mut descriptor = BuildingPresetDescriptor::from_asset(
            "connection-fields".to_owned(),
            "连接字段测试".to_owned(),
            "gltf/buildings/test.glb".to_owned(),
            ScenePlacementAssetTarget::Scene0,
        );
        descriptor.connections = vec![
            BuildingConnectionPointDescriptor::default(),
            BuildingConnectionPointDescriptor {
                id: "connection-2".to_owned(),
                ..Default::default()
            },
        ];
        assert!(
            commit_field(
                &mut descriptor,
                BuildingPresetField::ConnectionId(1),
                "connection-1",
            )
            .is_err()
        ); // 点ID不能在同一预设内重复。
        assert!(
            commit_field(
                &mut descriptor,
                BuildingPresetField::ConnectionKind(0),
                "墙端",
            )
            .is_err()
        ); // 兼容类型使用跨平台稳定技术字符集。
        assert!(
            commit_field(&mut descriptor, BuildingPresetField::ConnectionYaw(0), "45",).is_err()
        ); // 首期不接受任意角度连接。
        commit_field(
            &mut descriptor,
            BuildingPresetField::ConnectionYaw(0),
            "-90",
        )
        .expect("负九十度应规范化");
        assert_eq!(descriptor.connections[0].outward_yaw_degrees, 270.0);
    }
}
