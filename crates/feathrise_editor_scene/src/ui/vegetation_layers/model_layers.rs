//! 模型稀疏样本的作者层复制与删除；只由单飞CPU任务调用，主线程只交换完整Arc。

use super::operations::PreparedVegetationMutation;
use crate::types::EditorAction;
use std::sync::Arc;

pub(super) fn prepare(
    mut prepared: PreparedVegetationMutation,
    action: EditorAction,
    source_layer: &str,
) -> Result<PreparedVegetationMutation, String> {
    let Some(change) = prepared.change.as_mut() else { return Ok(prepared); };
    let Some(models) = change.model_vegetation.as_mut() else { return Ok(prepared); };
    let duplicate = if action == EditorAction::VegetationDuplicateLayer {
        Some(change.after.layers.iter().find(|layer|
            !change.before.layers.iter().any(|old| old.id == layer.id))
            .ok_or("复制模型植被作者层缺少新身份")?.id.clone())
    } else if action == EditorAction::VegetationDeleteSelected {
        None
    } else {
        return Err("模型样本结构任务只接受复制或删除作者层".into());
    };
    let mut after = Vec::with_capacity(models.before.len());
    let mut bytes = 0usize;
    let mut history_bytes = 0usize;
    for original in models.before.iter() {
        history_bytes = history_bytes.saturating_add(
            crate::authoring::model_vegetation::sample_heap_bytes(original));
        // 不涉及本层的目标继续共享；只有真正受影响的目标才在后台复制样本。
        let changed = original.samples.iter().any(|sample|
            sample.layers.iter().any(|(id, _)| id == source_layer));
        let data = if changed {
            let mut data = original.as_ref().clone();
            for sample in &mut data.samples {
                if let Some(new_id) = &duplicate {
                    if let Some(density) = sample.layers.iter()
                        .find_map(|(id, density)| (id == source_layer).then_some(*density)) {
                        sample.layers.push((new_id.clone(), density));
                    }
                } else {
                    sample.layers.retain(|(id, _)| id != source_layer);
                }
            }
            data.samples.retain(|sample| !sample.layers.is_empty());
            data.sample_blocks.clear(); // 正式保存为新完整样本版本生成私有内容摘要块。
            data.validate()?;
            Arc::new(data)
        } else {
            original.clone()
        };
        bytes = bytes.saturating_add(crate::authoring::model_vegetation::sample_heap_bytes(&data));
        if bytes > 64 * 1024 * 1024 {
            return Err("作者层操作超过模型植被64MiB样本预算，结构与样本均未修改".into());
        }
        after.push(data);
    }
    change.model_heap_bytes = history_bytes.saturating_add(bytes);
    models.after = Arc::new(after);
    Ok(prepared)
}
