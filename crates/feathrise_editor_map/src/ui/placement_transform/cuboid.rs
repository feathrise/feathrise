//! 内置立方体属性只改变几何配方；三轴变换保持独立，历史沿用完整摆放记录。

use bevy::prelude::*;
use feathrise_map_content::{MapPlacementAssetTarget, MapPlacementRecord};

use super::{
    PlacementTransformField, PlacementTransformTarget, actions::sync_placement_record_world,
};
use crate::{
    authoring::{EditorAuthoringSession, EditorCommand},
    resources::MapEditorState,
    ui::placement_water::PlacementInstanceIndex,
};

pub(super) fn field_value(
    field: PlacementTransformField,
    record: &MapPlacementRecord,
) -> Option<f32> {
    let (size_mm, color_srgb, roughness_milli) = match record.asset_target {
        MapPlacementAssetTarget::Cuboid { size_mm, color_srgb, roughness_milli, .. } =>
            (Some(size_mm), color_srgb, roughness_milli),
        MapPlacementAssetTarget::EditableCuboid { color_srgb, roughness_milli, .. } | MapPlacementAssetTarget::EditableMesh { color_srgb, roughness_milli, .. } =>
            (None, color_srgb, roughness_milli),
        _ => return None,
    };
    Some(match field {
        PlacementTransformField::CuboidSizeX => size_mm?[0] as f32 / 1000.0,
        PlacementTransformField::CuboidSizeY => size_mm?[1] as f32 / 1000.0,
        PlacementTransformField::CuboidSizeZ => size_mm?[2] as f32 / 1000.0,
        PlacementTransformField::CuboidColorR => f32::from(color_srgb[0]),
        PlacementTransformField::CuboidColorG => f32::from(color_srgb[1]),
        PlacementTransformField::CuboidColorB => f32::from(color_srgb[2]),
        PlacementTransformField::CuboidRoughness => f32::from(roughness_milli) / 1000.0,
        _ => return None,
    })
}

/// 只写入整数化配方；不会把米制尺寸写到 Transform.scale，避免两种编辑语义互相覆盖。
pub(super) fn set_field_value(
    field: PlacementTransformField,
    record: &mut MapPlacementRecord,
    value: f32,
) -> bool {
    if !value.is_finite() {
        return false;
    }
    let before = record.asset_target;
    // 编辑网格保留公共材质能力，尺寸配方只用于仍为参数化盒体的记录。
    let (color_srgb, roughness_milli) = match &mut record.asset_target {
        MapPlacementAssetTarget::Cuboid { size_mm, color_srgb, roughness_milli, .. } => {
            let axis = match field {
                PlacementTransformField::CuboidSizeX => Some(0),
                PlacementTransformField::CuboidSizeY => Some(1),
                PlacementTransformField::CuboidSizeZ => Some(2),
                _ => None,
            };
            if let Some(axis) = axis {
                size_mm[axis] = (value * 1000.0).round() as u32;
                return before != record.asset_target;
            }
            (color_srgb, roughness_milli)
        }
        MapPlacementAssetTarget::EditableCuboid { color_srgb, roughness_milli, .. } | MapPlacementAssetTarget::EditableMesh { color_srgb, roughness_milli, .. } =>
            (color_srgb, roughness_milli),
        _ => return false,
    };
    match field {
        PlacementTransformField::CuboidColorR => {
            color_srgb[0] = value.round().clamp(0.0, 255.0) as u8
        }
        PlacementTransformField::CuboidColorG => {
            color_srgb[1] = value.round().clamp(0.0, 255.0) as u8
        }
        PlacementTransformField::CuboidColorB => {
            color_srgb[2] = value.round().clamp(0.0, 255.0) as u8
        }
        PlacementTransformField::CuboidRoughness => {
            *roughness_milli = (value * 1000.0).round().clamp(0.0, 1000.0) as u16
        }
        _ => return false,
    }
    before != record.asset_target
}

#[allow(clippy::too_many_arguments)]
pub(super) fn toggle_collision(
    target: &PlacementTransformTarget,
    before: MapPlacementRecord,
    commands: &mut Commands,
    server: &AssetServer,
    session: &mut EditorAuthoringSession,
    index: &PlacementInstanceIndex,
    roots: &Query<(Entity, &feathrise_scene::MapPlacementInstance)>,
    editor: &mut MapEditorState,
) -> Result<(), String> {
    let mut after = before.clone();
    let collision = match &mut after.asset_target {
        MapPlacementAssetTarget::Cuboid { collision, .. } | MapPlacementAssetTarget::EditableCuboid { collision, .. } | MapPlacementAssetTarget::EditableMesh { collision, .. } => collision,
        _ => return Err("碰撞开关仅用于内置网格".into()),
    };
    *collision = !*collision;
    let enabled = *collision;
    let draft = session.draft.as_ref().ok_or("地图作者草稿尚未就绪")?;
    let bounds = draft.manifest.world_bounds().ok_or("地图范围无效")?;
    after.validated_transform(&draft.map_id, bounds)?;
    let record = session
        .draft
        .as_mut()
        .and_then(|draft| draft.placements.placements.get_mut(target.record_index))
        .filter(|record| record.id == target.placement_id)
        .ok_or("立方体选择已经失效")?;
    *record = after.clone();
    if let Err(error) = sync_placement_record_world(
        commands, server, session, index, roots, &after, bounds, true, true,
    ) {
        if let Some(record) = session
            .draft
            .as_mut()
            .and_then(|draft| draft.placements.placements.get_mut(target.record_index))
            .filter(|record| record.id == target.placement_id)
        {
            *record = before;
        }
        return Err(error);
    }
    session.push_bounded_water_history(EditorCommand::PlacementChanged {
        record_index: target.record_index,
        before,
        after,
    });
    session.mark_changed(editor);
    editor.status = format!(
        "内置网格碰撞已{}；可撤销并随地图保存",
        if enabled { "开启" } else { "关闭" }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editable_mesh_material_keeps_shared_vertices_and_rejects_size_recipe() {
        let mut record = record();
        record.asset_target = record.asset_target.into_editable().unwrap();
        let vertices = record.asset_target.editable_vertices_half_mm();
        assert!(set_field_value(PlacementTransformField::CuboidColorG, &mut record, 77.0));
        assert!(set_field_value(PlacementTransformField::CuboidRoughness, &mut record, 0.271));
        assert_eq!(record.asset_target.editable_vertices_half_mm(), vertices);
        assert_eq!(field_value(PlacementTransformField::CuboidColorG, &record), Some(77.0));
        assert_eq!(field_value(PlacementTransformField::CuboidSizeX, &record), None);
        let before = record.clone();
        assert!(!set_field_value(PlacementTransformField::CuboidSizeX, &mut record, 5.0));
        assert!(!PlacementTransformField::CuboidSizeX.supported_by(&record));
        assert!(PlacementTransformField::CuboidRoughness.supported_by(&record));
        assert_eq!(record, before);
    }

    fn record() -> MapPlacementRecord {
        let mut record: MapPlacementRecord = serde_json::from_str(
            r#"{"id":"cube","asset_path":"","translation":[0,1,0],"yaw_degrees":0,"scale":[2,3,4]}"#,
        ).unwrap();
        record.asset_target = MapPlacementAssetTarget::DEFAULT_CUBOID;
        record
    }

    #[test]
    fn cuboid_meter_dimensions_preserve_independent_transform_and_material() {
        let mut record = record();
        let before = record.clone();
        assert!(set_field_value(
            PlacementTransformField::CuboidSizeY,
            &mut record,
            3.125
        ));
        assert_eq!(
            field_value(PlacementTransformField::CuboidSizeY, &record),
            Some(3.125)
        );
        assert_eq!(record.scale, before.scale);
        assert_eq!(record.translation, before.translation);
        let MapPlacementAssetTarget::Cuboid {
            size_mm,
            color_srgb,
            roughness_milli,
            collision,
        } = record.asset_target
        else {
            panic!("应保持立方体来源")
        };
        assert_eq!(size_mm, [2000, 3125, 2000]);
        let MapPlacementAssetTarget::Cuboid {
            color_srgb: original_color,
            roughness_milli: original_roughness,
            collision: original_collision,
            ..
        } = before.asset_target
        else {
            unreachable!()
        };
        assert_eq!(
            (color_srgb, roughness_milli, collision),
            (original_color, original_roughness, original_collision)
        );
        assert!(!set_field_value(
            PlacementTransformField::CuboidSizeY,
            &mut record,
            3.125
        ));
    }

    #[test]
    fn cuboid_material_roundtrips_without_changing_geometry() {
        let mut record = record();
        assert!(set_field_value(
            PlacementTransformField::CuboidColorG,
            &mut record,
            42.0
        ));
        assert!(set_field_value(
            PlacementTransformField::CuboidRoughness,
            &mut record,
            0.375
        ));
        let restored: MapPlacementRecord =
            serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
        assert_eq!(restored, record);
        assert_eq!(
            field_value(PlacementTransformField::CuboidColorG, &restored),
            Some(42.0)
        );
        assert_eq!(
            field_value(PlacementTransformField::CuboidRoughness, &restored),
            Some(0.375)
        );
        assert_eq!(
            field_value(PlacementTransformField::CuboidSizeX, &restored),
            Some(2.0)
        );
    }
}
