//! 静水整笔历史按完整前后Arc原子恢复，拒绝缺失或已经换拓扑的作者源。

use crate::authoring::{commands::WaterStrokeSnapshot, resources::EditorSceneDraft}; // 完整笔划命令与当前权威草稿。

pub(crate) fn apply(
    draft: &mut EditorSceneDraft,
    change: &WaterStrokeSnapshot,
    undoing: bool,
) -> Result<(), String> {
    let current = draft
        .water_masks
        .get(&change.layer_id)
        .ok_or_else(|| "静水作者层已经失效".to_owned())?; // 不误写另一水层。
    if current.len() != change.before_mask.len() || current.len() != change.after_mask.len() {
        return Err("静水画布规格已经变化，无法恢复原笔划".to_owned());
    } // 两个方向都先验证完整规格。
    if let Some((before, after)) = &change.terrain {
        // 先验证河床依赖，再修改任何一侧。
        let mesh = draft
            .terrain_mesh
            .as_ref()
            .ok_or_else(|| "静水笔划所需地形已经失效".to_owned())?; // 外部GLB丢失时历史保持原样。
        if mesh.positions.len() != before.positions.len()
            || mesh.positions.len() != after.positions.len()
            || mesh.indices.len() != before.indices.len()
            || mesh.indices.len() != after.indices.len()
        {
            return Err("静水笔划的地形拓扑已经变化".to_owned());
        } // 完整拓扑热更还会清理相关历史，常数规格门防御陈旧输入。
    }
    *draft
        .water_masks
        .get_mut(&change.layer_id)
        .expect("上方已验证水层") = if undoing {
        change.before_mask.clone()
    } else {
        change.after_mask.clone()
    }; // 整笔全部R8一次替换。
    if let Some((before, after)) = &change.terrain {
        draft.terrain_mesh = Some(if undoing {
            before.clone()
        } else {
            after.clone()
        });
    } // 同次系统原子恢复河床，纯擦除不影响地形。
    Ok(()) // 完整采用后调用方才转移历史栈并推进修订。
}
