//! 验收只接纳当前网格/作者层的一条真实非空植被历史；局部差分与跨网格原子快照使用各自正式合同。

use crate::authoring::{
    EditorAuthoringSession, EditorCommand, VegetationBrushStroke, WaterBrushStroke,
};
use bevy::prelude::*;
use feathrise_map_content::TerrainId;
use std::sync::Arc;

pub(super) fn validate_vegetation(
    mut command: &EditorCommand,
    terrain: &TerrainId,
    layer: &str,
    current: &Arc<Vec<u8>>,
) -> Result<&'static str, String> {
    for _ in 0..8 {
        match command {
            EditorCommand::TerrainCanvas { id, command: inner } if id == terrain => command = inner,
            EditorCommand::TerrainCanvas { .. } => return Err("原植被笔划历史属于另一网格".into()),
            EditorCommand::VegetationChanged { layer_id, changes } => {
                let Some(first) = changes.first() else {
                    return Err("原植被笔划历史为空差分".into());
                };
                if layer_id != layer
                    || first.before == first.after
                    || current.get(first.index) != Some(&first.after)
                {
                    return Err("原植被差分与按下时层身份或当前源不一致".into());
                }
                return Ok("VegetationChanged");
            }
            EditorCommand::VegetationPaintChanged(change) => {
                if change.canvases.len() != 1 {
                    return Err("有限单网格植被轨迹的历史包含其它网格".into());
                }
                let (before, after) = change
                    .canvases
                    .get(terrain)
                    .ok_or("原植被原子历史不含目标网格")?;
                let before = before
                    .authors
                    .get(layer)
                    .ok_or("原植被原子历史缺少原作者层")?;
                let after = after
                    .authors
                    .get(layer)
                    .ok_or("原植被原子历史缺少完成作者层")?;
                if Arc::ptr_eq(before, after) || !Arc::ptr_eq(after, current) {
                    return Err("原植被原子历史未换代或不对应当前完整作者源".into());
                }
                return Ok("VegetationPaintChanged");
            }
            _ => return Err(format!("原植被释放后不是新笔划历史：{}", label(command))),
        }
    }
    Err("原植被历史包装深度异常".into())
}

fn label(command: &EditorCommand) -> String {
    match command {
        EditorCommand::TerrainCanvas { id, command } => {
            format!("TerrainCanvas({},{})", id.as_str(), label(command))
        }
        EditorCommand::RootWater(command) => format!("RootWater({})", label(command)),
        EditorCommand::VegetationChanged { .. } => "VegetationChanged".into(),
        EditorCommand::VegetationPaintChanged(_) => "VegetationPaintChanged".into(),
        EditorCommand::VegetationAuthoringChanged(_) => "VegetationAuthoringChanged".into(),
        EditorCommand::TerrainObjectsChanged { .. } => "TerrainObjectsChanged".into(),
        EditorCommand::PlacementAdded(_) => "PlacementAdded".into(),
        EditorCommand::WaterStrokeSnapshot(_) => "WaterStrokeSnapshot".into(),
        EditorCommand::WaterLevelChanged { .. } => "WaterLevelChanged".into(),
        EditorCommand::WaterLayersChanged(_) => "WaterLayersChanged".into(),
        other => format!("其它原命令{:?}", std::mem::discriminant(other)),
    }
}

/// 超时记录固定大小元数据，不遍历地图像素或完整历史；后续失败能定位输入、原任务或历史合同。
pub(super) fn diagnostic(world: &World, undo_before: usize) -> String {
    let session = world.resource::<EditorAuthoringSession>();
    let vegetation = world.resource::<VegetationBrushStroke>();
    let water = world.resource::<WaterBrushStroke>();
    let mouse = world.resource::<ButtonInput<MouseButton>>();
    serde_json::json!({
        "undo_before":undo_before,"undo_now":session.undo.len(),
        "undo_last":session.undo.last().map(label),"revision":session.revision,
        "mouse_pressed":mouse.pressed(MouseButton::Left),"mouse_released_edge":mouse.just_released(MouseButton::Left),
        "vegetation_active":vegetation.active,"vegetation_pending_ground":vegetation.pending_ground.is_some(),
        "vegetation_before_bytes":vegetation.before.len(),"vegetation_last_world":vegetation.last_world.map(|point| point.to_array()),
        "vegetation_scope_busy":world.get_resource::<crate::authoring::vegetation_paint_scope::VegetationPaintState>().is_some_and(|scope|scope.busy()),
        "water_active":water.active,"water_pending_sample":water.pending_sample.is_some(),
        "water_queued_sample":water.queued_sample.is_some(),"water_release_requested":water.release_requested,
    }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authoring::{ByteChange, vegetation_paint_scope::VegetationPaintChange};

    #[test]
    fn local_paint_proof_requires_matching_canvas_layer_and_current_changed_byte() {
        let terrain = TerrainId::parse("sample").unwrap();
        let pixels = Arc::new(vec![17]);
        let local = EditorCommand::VegetationChanged {
            layer_id: "grass".into(),
            changes: vec![ByteChange {
                index: 0,
                before: 0,
                after: 17,
            }],
        };
        assert_eq!(
            validate_vegetation(&local, &terrain, "grass", &pixels).unwrap(),
            "VegetationChanged"
        );
        assert!(validate_vegetation(&local, &terrain, "other", &pixels).is_err());
        let wrong_canvas = EditorCommand::TerrainCanvas {
            id: TerrainId::parse("other").unwrap(),
            command: Box::new(local),
        };
        assert!(validate_vegetation(&wrong_canvas, &terrain, "grass", &pixels).is_err());
        assert!(
            validate_vegetation(
                &EditorCommand::TerrainChanged(vec![]),
                &terrain,
                "grass",
                &pixels
            )
            .is_err()
        );
    }

    #[test]
    fn atomic_paint_proof_requires_one_matching_changed_canvas_and_adopted_source() {
        let terrain = TerrainId::parse("sample").unwrap();
        let pixels = Arc::new(vec![17]);
        let mut change = VegetationPaintChange {
            before_groups: Arc::default(),
            after_groups: Arc::default(),
            canvases: Default::default(),
        };
        let (before, after) = change.canvases.entry(terrain.clone()).or_default();
        before.authors.insert("grass".into(), Arc::new(vec![0]));
        after.authors.insert("grass".into(), pixels.clone());
        let command = EditorCommand::VegetationPaintChanged(Box::new(change.clone()));
        assert_eq!(
            validate_vegetation(&command, &terrain, "grass", &pixels).unwrap(),
            "VegetationPaintChanged"
        );
        assert!(validate_vegetation(&command, &terrain, "grass", &Arc::new(vec![17])).is_err());
        change
            .canvases
            .entry(TerrainId::parse("other").unwrap())
            .or_default();
        assert!(
            validate_vegetation(
                &EditorCommand::VegetationPaintChanged(Box::new(change)),
                &terrain,
                "grass",
                &pixels
            )
            .is_err()
        );
    }
}
