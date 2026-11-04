//! 院落成员与完整摆放记录之间的局部、世界变换换算。
//!
//! 【功能设计】院落预设保存各成员相对共同锚点的变换；落印后仍保存独立摆放记录。
//! 作者调整一个成员时只改该记录，整体复制或旋转再从当前记录反推局部差值，
//! 因而不会用旧模板覆盖该副本的局部修改。

use bevy::prelude::*; // 与正式摆放共同使用世界Y轴和米制三维坐标。
use feathrise_map_content::{
    CourtyardMemberDescriptor, CourtyardPresetDescriptor, MapPlacementCourtyardMembership,
    MapPlacementRecord,
}; // 预设、地图成员与普通摆放共享同一来源格式。

/// 盖印冻结模板和朝向，移动或复制冻结目标副本成员；这里只公开固定大小用途身份。
pub(crate) fn clipmap_cursor_target(
    ui: &crate::ui::courtyard::CourtyardUiState,
    catalogs: [u64; 2],
) -> Option<crate::viewport::vegetation_ground::GroundCursorTarget> {
    use crate::ui::courtyard::CourtyardViewportMode;
    let (preset_id, selected_id, quarter_turns) = match ui.mode {
        CourtyardViewportMode::Stamp => {
            (Some(ui.selected_preset_id.clone()?), None, ui.quarter_turns)
        }
        CourtyardViewportMode::MoveGroup | CourtyardViewportMode::CopyGroup => {
            (None, Some(ui.selected_placement_ids.first()?.clone()), 0)
        }
        _ => return None,
    };
    Some(
        crate::viewport::vegetation_ground::GroundCursorTarget::Courtyard {
            mode: ui.mode as u8,
            preset_id,
            selected_id,
            quarter_turns,
            catalogs,
        },
    )
}

/// 围绕世界Y轴旋转局部位移，正角度把局部+Z转向世界+X。
fn rotate_y(value: Vec3, yaw_degrees: f32) -> Vec3 {
    Quat::from_rotation_y(yaw_degrees.to_radians()) * value // 与MapPlacementRecord正式Transform一致。
}

/// 由一组现有摆放生成可跨地图复用的院落模板。
pub(super) fn preset_from_placements(
    id: String,                     // 全局目录稳定预设ID。
    label: String,                  // 作者输入的中文名称。
    records: &[MapPlacementRecord], // 选中成员的完整当前作者值。
) -> Result<(CourtyardPresetDescriptor, Vec3, f32), String> {
    if records.is_empty() {
        return Err("请先选择至少一个已摆放资产".to_owned()); // 空模板没有可见用途。
    }
    if records
        .iter()
        .any(|record| !record.water_surfaces.is_empty())
    {
        return Err("院落组合暂不接受带整网格水语义的资产".to_owned()); // 水体另有独立引用和保存预算。
    }
    let shared = records.first().and_then(|record| record.courtyard.as_ref()); // 现有同组可复用原锚点。
    let same_group = shared.is_some_and(|first| {
        records.iter().all(|record| {
            record.courtyard.as_ref().is_some_and(|member| {
                member.instance_id == first.instance_id && member.preset_id == first.preset_id
            })
        })
    }); // 仅整组成员同源时沿用旧锚点，混合选择建立新锚点。
    let (origin, yaw_degrees) = if same_group {
        let group = shared.expect("上方已确认同组成员"); // 首项提供完整一致的锚点。
        (Vec3::from_array(group.origin), group.yaw_degrees) // 重新保存当前局部改动，不回退模板。
    } else {
        let (minimum, maximum) = records.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(minimum, maximum), record| {
                let position = Vec3::from_array(record.translation); // 只在显式保存边沿遍历受限成员。
                (minimum.min(position), maximum.max(position)) // X/Z中心、最低Y构造直观枢轴。
            },
        );
        (
            Vec3::new(
                (minimum.x + maximum.x) * 0.5,
                minimum.y,
                (minimum.z + maximum.z) * 0.5,
            ),
            0.0, // 新组合以当前世界朝向作为局部零度。
        )
    };
    let members = records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            let local = rotate_y(Vec3::from_array(record.translation) - origin, -yaw_degrees); // 保存完整当前局部改动。
            CourtyardMemberDescriptor {
                mesh_lod: record.mesh_lod,
                id: format!("member-{}", index + 1), // 目录内身份与当前地图摆放ID分离。
                asset_path: record.asset_path.clone(), // GLB来源可独立于建筑目录加载。
                asset_target: record.asset_target,   // Scene0和静态primitive保留精确目标。
                building_preset_id: record.building_preset_id.clone(), // 建筑规则仍引用规范预设。
                local_translation: local.to_array(), // 世界原点换成院落局部米制偏移。
                local_yaw_degrees: (record.yaw_degrees - yaw_degrees).rem_euclid(360.0), // 成员独立朝向不被整体旋转吃掉。
                scale: record.scale, // 普通装饰保留三轴缩放；建筑仍受单位缩放规则。
                height_mode: record.height_mode, // 普通摆放沿用原高度模式。
                terrain_offset: record.terrain_offset, // 原地形相对偏移在新地块重新采样。
            }
        })
        .collect();
    Ok((
        CourtyardPresetDescriptor { id, label, members },
        origin,
        yaw_degrees,
    )) // 目录及首个实例共享同一次冻结。
}

/// 将模板成员落成独立的地图摆放记录，建筑最终高度由后台五点地基校验再写入。
pub(super) fn instantiate_preset(
    preset: &CourtyardPresetDescriptor, // 已经完整校验的院落目录条目。
    placement_ids: &[String],           // 每个成员一个全新的当前地图稳定ID。
    instance_id: &str,                  // 这一印章副本的共同稳定身份。
    origin: Vec3,                       // 当前地块的世界锚点。
    yaw_degrees: f32,                   // 院落整体朝向。
) -> Result<Vec<MapPlacementRecord>, String> {
    if preset.members.len() != placement_ids.len() {
        return Err("院落成员与新摆放ID数量不一致".to_owned()); // 不生成半套印章。
    }
    Ok(preset
        .members
        .iter()
        .zip(placement_ids)
        .map(|(member, placement_id)| {
            let translation =
                origin + rotate_y(Vec3::from_array(member.local_translation), yaw_degrees); // 局部米制位置随整体朝向旋转。
            MapPlacementRecord {
                mesh_lod: member.mesh_lod,
                terrain_binding: None,
                editor: Default::default(),
                terrain_patch: None,
                id: placement_id.clone(), // 每处院落都产生独立实例ID。
                asset_path: member.asset_path.clone(), // 不依赖目录索引位置猜来源。
                asset_target: member.asset_target, // 原GLB目标保持完整。
                building_preset_id: member.building_preset_id.clone(), // 建筑规则只作用于建筑成员。
                courtyard: Some(MapPlacementCourtyardMembership {
                    instance_id: instance_id.to_owned(), // 全组共享当前副本身份。
                    preset_id: preset.id.clone(),        // 预设更新不会自动覆盖已落印成员。
                    member_id: member.id.clone(),        // 更新和诊断不靠成员数组下标。
                    origin: origin.to_array(),           // 个别成员编辑后仍保留整体枢轴。
                    yaw_degrees,                         // 整组旋转时可反推当前成员局部偏移。
                }),
                translation: translation.to_array(), // 建筑Y稍后由真实地形解析。
                height_mode: member.height_mode,     // 普通装饰保留所选模式。
                terrain_offset: member.terrain_offset, // 跟随型成员在新地形重新采样。
                yaw_degrees: (member.local_yaw_degrees + yaw_degrees).rem_euclid(360.0), // 预设局部角度加整体角度。
                scale: member.scale,        // 与模板可见模型保持一致。
                water_surfaces: Vec::new(), // 院落模板当前不承载整网格水绑定。
            }
        })
        .collect()) // 只有全部成员完成后才交给后台校验和作者事务。
}

/// 用当前副本的实际成员变换进行整体移动、旋转或复制，保留已发生的单成员修改。
pub(super) fn transform_existing_member(
    record: &MapPlacementRecord,    // 当前作者记录而非旧预设模板。
    new_instance_id: Option<&str>,  // 复制时换组身份；移动和旋转保持原身份。
    new_origin: Vec3,               // 新共同世界枢轴。
    new_yaw_degrees: f32,           // 新整体朝向。
    new_placement_id: Option<&str>, // 复制时为每个成员生成新摆放ID。
) -> Result<MapPlacementRecord, String> {
    let group = record
        .courtyard
        .as_ref()
        .ok_or_else(|| "所选资产不属于院落副本".to_owned())?; // 独立GLB不得被整体操作误收。
    let old_origin = Vec3::from_array(group.origin); // 旧锚点不因个别成员编辑而漂移。
    let local = rotate_y(
        Vec3::from_array(record.translation) - old_origin,
        -group.yaw_degrees,
    ); // 当前副本局部值包含手工修改。
    let mut changed = record.clone(); // 保留模型、水语义与单成员作者参数。
    changed.translation = (new_origin + rotate_y(local, new_yaw_degrees)).to_array(); // 整组变换只改位置。
    changed.yaw_degrees =
        (record.yaw_degrees + new_yaw_degrees - group.yaw_degrees).rem_euclid(360.0); // 成员自身旋转保持相对差值。
    if let Some(id) = new_placement_id {
        changed.id = id.to_owned(); // 复制不能复用现有根ID。
    }
    let membership = changed.courtyard.as_mut().expect("上方已确认院落成员"); // 与记录同时更新分组元数据。
    membership.instance_id = new_instance_id.unwrap_or(&group.instance_id).to_owned(); // 原地编辑与复制分开。
    membership.origin = new_origin.to_array(); // 保存重载后仍可整体选取。
    membership.yaw_degrees = new_yaw_degrees; // 后续操作继续基于新锚点。
    Ok(changed) // 地形、碰撞和边界由同批后台校验决定是否采用。
}

#[cfg(test)]
mod tests {
    use super::*;
    use feathrise_map_content::{
        CourtyardPresetCatalogAsset, MapPlacementAssetTarget, MapPlacementHeightMode,
    }; // 随包样板与普通摆放使用同一正式格式。

    /// 两栋独立资产足以验证模板局部坐标与地图记录分离。
    fn member(id: &str, x: f32, z: f32) -> MapPlacementRecord {
        MapPlacementRecord {
            mesh_lod: None,
            terrain_binding: None,
            editor: Default::default(),
            terrain_patch: None,
            id: id.to_owned(),
            asset_path: "gltf/建筑/院落测试民居.glb".to_owned(),
            asset_target: MapPlacementAssetTarget::default(),
            building_preset_id: None,
            courtyard: None,
            translation: [x, 0.0, z],
            height_mode: MapPlacementHeightMode::FixedHeight,
            terrain_offset: 0.0,
            yaw_degrees: 0.0,
            scale: [1.0; 3],
            water_surfaces: Vec::new(),
        }
    }

    #[test]
    fn stamp_creates_independent_member_ids_and_preserves_local_edit_on_group_copy() {
        let originals = vec![member("house-a", -3.0, 0.0), member("house-b", 3.0, 0.0)];
        let (preset, origin, yaw) = preset_from_placements(
            "courtyard-test".to_owned(),
            "测试院落".to_owned(),
            &originals,
        )
        .unwrap();
        assert_eq!(origin, Vec3::ZERO); // 新模板以当前选择的 XZ 中心为共同锚点。
        assert_eq!(yaw, 0.0);
        let ids = vec!["copy-a".to_owned(), "copy-b".to_owned()];
        let mut stamped = instantiate_preset(
            &preset,
            &ids,
            "instance-a",
            Vec3::new(20.0, 0.0, 20.0),
            90.0,
        )
        .unwrap();
        assert_eq!(stamped[0].id, "copy-a");
        assert_eq!(stamped[1].id, "copy-b");
        assert!((stamped[0].translation[2] - 23.0).abs() < 0.001);
        stamped[0].translation[0] += 2.0; // 作者在这一处副本中单独向右移动厢房。
        let copied = transform_existing_member(
            &stamped[0],
            Some("instance-b"),
            Vec3::new(50.0, 0.0, 50.0),
            180.0,
            Some("second-a"),
        )
        .unwrap();
        let untouched = transform_existing_member(
            &stamped[1],
            Some("instance-b"),
            Vec3::new(50.0, 0.0, 50.0),
            180.0,
            Some("second-b"),
        )
        .unwrap();
        assert_eq!(copied.id, "second-a");
        assert_eq!(copied.courtyard.as_ref().unwrap().instance_id, "instance-b");
        assert_ne!(copied.translation, untouched.translation); // 复制依据当前成员位置，未回退旧模板。
        assert_eq!(
            stamped[0].courtyard.as_ref().unwrap().instance_id,
            "instance-a"
        ); // 原副本不变。
    }

    #[test]
    fn mesh_lod_policy_survives_template_save_reload_stamp_and_group_move_or_copy() {
        let mut original = member("house-a", 0.0, 0.0);
        let policy = feathrise_map_content::MeshLodPolicy {
            display: feathrise_map_content::MeshLodMode::Override(feathrise_map_content::MeshDisplayLodSettings {
                max_error_pixels: 0.5, ..Default::default()
            }),
            collision: feathrise_map_content::MeshLodMode::Disabled,
        };
        original.mesh_lod = Some(policy);
        let (preset, _, _) = preset_from_placements("yard-lod".into(), "策略院落".into(), &[original]).unwrap();
        assert_eq!(preset.members[0].mesh_lod, Some(policy));
        let saved = serde_json::to_string(&preset).unwrap();
        let restored: CourtyardPresetDescriptor = serde_json::from_str(&saved).unwrap();
        let stamped = instantiate_preset(&restored, &["stamp-a".into()], "yard-stamp", Vec3::ZERO, 0.0).unwrap();
        assert_eq!(stamped[0].mesh_lod, Some(policy));
        let moved = transform_existing_member(&stamped[0], None, Vec3::new(5.0, 1.0, 7.0), 90.0, None).unwrap();
        assert_eq!(moved.mesh_lod, Some(policy));
        let copied = transform_existing_member(&moved, Some("yard-copy"), Vec3::new(10.0, 2.0, 8.0), 180.0, Some("copy-a")).unwrap();
        assert_eq!(copied.mesh_lod, Some(policy));
        assert_eq!(stamped[0].id, "stamp-a");
        assert_eq!(copied.id, "copy-a");
    }

    #[test]
    fn neutral_fixture_stamps_twice_and_keeps_member_edit_after_group_move_and_reload() {
        let catalog: CourtyardPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/courtyard_presets.courtyard_presets.json",
        )))
        .expect("随包院落目录应可加载"); // 使用仓库自有的中性24成员几何夹具。
        let preset = catalog
            .presets
            .iter()
            .find(|item| item.id == "fixture-yard")
            .expect("随包小院样板须存在"); // 保留多成员覆盖，夹具不引用原游戏样例。
        let first_ids: Vec<String> = (0..preset.members.len())
            .map(|index| format!("first-{index}"))
            .collect(); // 第一印章每成员有独立地图身份。
        let second_ids: Vec<String> = (0..preset.members.len())
            .map(|index| format!("second-{index}"))
            .collect(); // 第二印章不能复用第一处的记录 ID。
        let first = instantiate_preset(preset, &first_ids, "yard-first", Vec3::ZERO, 0.0)
            .expect("第一处院落应能形成完整候选"); // 空地原点形成第一份独立布局。
        let mut second = instantiate_preset(
            preset,
            &second_ids,
            "yard-second",
            Vec3::new(60.0, 0.0, 0.0),
            0.0,
        )
        .expect("第二处院落应能形成完整候选"); // 第二份从同一预设盖印。
        assert_eq!(first.len(), 24); // 实际样板不能在实例化时漏掉墙段、铺地或装饰。
        assert_eq!(second.len(), first.len()); // 两处副本成员数量一致。
        assert_ne!(first[1].id, second[1].id); // 同名厢房在两处地图实例中具有独立记录。
        second[1].translation[0] += 0.25; // 模拟作者只调整第二处左厢房，不改变模板或第一处。
        let moved: Vec<MapPlacementRecord> = second
            .iter()
            .map(|record| {
                transform_existing_member(record, None, Vec3::new(60.0, 0.0, 50.0), 90.0, None)
                    .expect("完整组移动和旋转应保留各成员当前值")
            })
            .collect(); // 依据当前实例而非旧模板换算新布局。
        assert!((moved[1].translation[2] - 56.75).abs() < 0.001); // 厢房额外移动的0.25米随整体旋转仍被保留。
        assert_eq!(
            moved[1].courtyard.as_ref().unwrap().instance_id,
            "yard-second"
        ); // 移动保持第二处副本身份。
        assert_eq!(moved[1].courtyard.as_ref().unwrap().yaw_degrees, 90.0); // 保存的新锚点朝向与整体操作一致。
        assert_eq!(first[1].translation, [-7.0, 0.0, 0.0]); // 第一处院落不受第二处局部编辑影响。
        let saved = serde_json::to_string(&moved).expect("院落成员应能写入普通摆放格式"); // 模拟地图 placements 的序列化边界。
        let reloaded: Vec<MapPlacementRecord> =
            serde_json::from_str(&saved).expect("保存后的院落成员应能重载"); // 使用正式记录反序列化格式。
        assert_eq!(reloaded, moved); // 成员模型、建筑引用、组身份和局部编辑全都保留。
    }
}
