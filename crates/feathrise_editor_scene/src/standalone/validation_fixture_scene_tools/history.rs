//! 验收只接纳当前网格/作者层的一条真实非空植被历史；局部差分与跨网格原子快照使用各自正式合同。

use crate::authoring::{
    EditorAuthoringSession, EditorCommand, VegetationBrushStroke, WaterBrushStroke,
};
use bevy::prelude::*;
use feathrise_scene_content::TerrainId;
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

/// 超时记录固定大小元数据，不遍历场景像素或完整历史；后续失败能定位输入、原任务或历史合同。
pub(super) fn diagnostic(world: &World, undo_before: usize) -> serde_json::Value {
    let session = world.resource::<EditorAuthoringSession>();
    let vegetation = world.resource::<VegetationBrushStroke>();
    let water = world.resource::<WaterBrushStroke>();
    let mouse = world.resource::<ButtonInput<MouseButton>>();
    let editor = world.resource::<crate::resources::SceneEditorState>();
    let scope = world.get_resource::<crate::authoring::vegetation_paint_scope::VegetationPaintSettings>()
        .map(|settings| match settings.scope {
            crate::authoring::vegetation_paint_scope::PaintScope::Current => "current",
            crate::authoring::vegetation_paint_scope::PaintScope::Cross => "cross",
        });
    let author_ground = session.draft.as_ref().and_then(|draft| {
        let ground = draft.terrain_ground.as_ref()?;
        let target = editor.terrain_canvas_target.as_ref()?;
        let query = ground.object_query(target)?;
        let page_revision = match query {
            feathrise_scene::TerrainObjectRaySnapshot::Clipmap(snapshot, _) => snapshot.authoritative_page_revision(),
            feathrise_scene::TerrainObjectRaySnapshot::Mesh(..) => None,
        };
        Some(serde_json::json!({"generation":ground.generation,"revision":ground.revision,"author_page_revision":page_revision}))
    });
    serde_json::json!({
        "status":editor.status,"undo_before":undo_before,"undo_now":session.undo.len(),
        "undo_last":session.undo.last().map(label),"revision":session.revision,
        "mouse_pressed":mouse.pressed(MouseButton::Left),"mouse_press_edge":mouse.just_pressed(MouseButton::Left),
        "mouse_released_edge":mouse.just_released(MouseButton::Left),
        "vegetation_active":vegetation.active,"vegetation_pending_ground":vegetation.pending_ground.is_some(),
        "vegetation_before_bytes":vegetation.before.len(),"vegetation_last_world":vegetation.last_world.map(|point| point.to_array()),
        "vegetation_scope":scope,
        "vegetation_revision":session.vegetation_revision,
        "vegetation_compiled_revision":session.vegetation_compiled_revision,
        "vegetation_structure_revision":session.vegetation_structure_revision,
        "vegetation_compiled_structure_revision":session.vegetation_compiled_structure_revision,
        "vegetation_scope_busy":world.get_resource::<crate::authoring::vegetation_paint_scope::VegetationPaintState>().is_some_and(|scope|scope.busy()),
        // 只读冻结实际分流门；失败报告必须能区分旧光标、当前作者画布和模型选择残留。
        "canvas_writable":crate::authoring::terrain_documents::canvas_writable(session, editor),
        "canvas_target":editor.terrain_canvas_target.as_ref().map(|id|id.as_str()),
        "active_canvas":session.draft.as_ref().and_then(|draft|draft.terrain_documents.active.as_ref()).map(|active|active.definition.id.as_str()),
        "model_selection":world.get_resource::<crate::ui::gltf_browser::EditorGltfBrowserState>().and_then(|browser|browser.selected_placement_id.as_deref()),
        "legacy_volume_ray":world.get_resource::<crate::viewport::VegetationGroundCursor>().is_some_and(|cursor|cursor.volume_paint_ray().is_some()),
        "source_authoring_available":world.get_resource::<crate::viewport::VegetationGroundCursor>()
            .is_none_or(|cursor|cursor.source_authoring_available()),
        "author_ground":author_ground,
        "object_cursor":crate::authoring::terrain_ground::cursor_validation_snapshot(world),
        "native_volume_active":editor.terrain_volume_field_active,
        "water_active":water.active,"water_pending_sample":water.pending_sample.is_some(),
        "water_queued_sample":water.queued_sample.is_some(),"water_release_requested":water.release_requested,
    })
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
