//! 地表局部差分、懒加入层与整层删除的统一原子历史应用。

use crate::authoring::resources::{EditorCommand, EditorSceneDraft}; // 统一历史枚举与当前完整场景草稿。
use crate::authoring::surface_layer_edit::apply_surface_authoring_change; // 删层快照共享同一严格应用入口。
use crate::resources::SceneEditorState; // 引入地表作者层事务所需类型和纯助手。
use std::sync::Arc; // 局部撤销只允许独占写侧，整图历史只换Arc句柄。 // 结构历史恢复后同步真实局部层与预设选择。

pub(super) fn apply_surface_history(
    // 地表作者操作在受检输入下执行当前步骤。
    draft: &mut EditorSceneDraft, // 完成当前地表作者事务的受检参数或值转换。
    editor: &mut SceneEditorState, // 当前F12场景选择、保存关闭意图与状态。
    command: &EditorCommand,    // 完成当前地表作者事务的受检参数或值转换。
    undoing: bool,              // 完成当前地表作者事务的受检参数或值转换。
) -> Result<(), String> {
    // 当前受检分支或参数组结束。
    // 上层仅在整个命令成功后转移历史栈。
    match command {
        // 依据完整作者条件选择有限处理分支。
        // 全部地表命令在一个边界保持层表和像素一致。
        EditorCommand::SurfaceChanged {
            // 完成当前地表作者事务的受检参数或值转换。
            weight_changes, // 完成当前地表作者事务的受检参数或值转换。
            index_changes,  // 完成当前地表作者事务的受检参数或值转换。
        } => {
            // 当前受检分支或参数组结束。
            // 普通笔刷保持局部差分，不复制整图历史。
            let weights = draft // 保持当前地表操作的数据和原子采用边界。
                .surface_weights // 读取或传递同版本RGBA8权重作者源。
                .as_mut() // 独占借用当前作者字段，尚未写入任何像素。
                .ok_or_else(|| "当前草稿没有地表权重作者源".to_owned())?; // 配对源必须同时存在。
            let indices = draft // 保持当前地表操作的数据和原子采用边界。
                .surface_indices // 读取或传递同版本小端RGBA32局部索引源。
                .as_mut() // 独占借用当前作者字段，尚未写入任何像素。
                .ok_or_else(|| "当前草稿没有地表索引作者源".to_owned())?; // 索引与权重共同预检。
            if !weight_changes // 依据完整作者条件选择有限处理分支。
                .iter() // 完成当前地表作者事务的受检参数或值转换。
                .all(|change| change.index < weights.len()) // 完成当前地表作者事务的受检参数或值转换。
                || !index_changes // 依据完整作者条件选择有限处理分支。
                    .iter() // 完成当前地表作者事务的受检参数或值转换。
                    .all(|change| change.index < indices.len())
            // 完成当前地表作者事务的受检参数或值转换。
            {
                // 满足上述完整条件后才进入当前分支。
                // 先检查全部差分下标，之后才允许写入。
                return Err("地表索引或权重差分下标已失效".to_owned()); // 任一侧失败，两图都保持原样。
            } // 局部差分范围校验结束。
            let weights = // 保持当前地表操作的数据和原子采用边界。
                Arc::get_mut(weights).ok_or_else(|| "地表权重独立写侧尚未准备完成".to_owned())?; // 上层已后台物化，此处绝不Arc::make_mut复制整图。
            let indices = // 保持当前地表操作的数据和原子采用边界。
                Arc::get_mut(indices).ok_or_else(|| "地表索引独立写侧尚未准备完成".to_owned())?; // 两边均成功获得写侧后才执行首个字节变化。
            for change in weight_changes {
                // 依据完整作者条件选择有限处理分支。
                // 只访问命令覆盖的局部RGBA8字节。
                weights[change.index] = if undoing { change.before } else { change.after }; // 同一撤销方向恢复完整权重差分。
            } // 权重局部恢复结束。
            for change in index_changes {
                // 依据完整作者条件选择有限处理分支。
                // 只访问命令覆盖的局部RGBA32字节。
                indices[change.index] = if undoing { change.before } else { change.after }; // 索引和权重按同方向恢复。
            } // 局部索引恢复结束。
        } // 普通地表笔刷历史结束。
        EditorCommand::SurfaceLayerAdded { index, layer } => {
            // 完成当前地表作者事务的受检参数或值转换。
            // 懒引用依旧只追加末项，不扫描像素。
            let surface = draft // 保持当前地表操作的数据和原子采用边界。
                .surface // 地表局部层表是唯一作者索引依据。
                .as_mut() // 独占借用当前作者字段，尚未写入任何像素。
                .ok_or_else(|| "当前草稿没有地表层定义".to_owned())?; // 完整局部层表必须存在。
            if undoing {
                // 依据完整作者条件选择有限处理分支。
                // 逆序历史保证该层之前的笔刷已经撤销。
                if index.checked_add(1) != Some(surface.layers.len()) // 依据完整作者条件选择有限处理分支。
                    || surface.layers.get(*index) != Some(layer)
                // 按已冻结下标或独占所有权取得受检目标。
                {
                    // 满足上述完整条件后才进入当前分支。
                    // 删除末项前复核精确位置及完整层身份。
                    return Err("待撤销的地表局部层已换代，拒绝删除其他层".to_owned()); // 不冒险删除其他作者层。
                } // 懒新增撤销身份校验结束。
                surface.layers.pop(); // 与原懒新增行为相同，常数时间移除末项。
                editor.selected_surface_layer =
                    editor // 完成当前地表作者事务的受检参数或值转换。
                        .selected_surface_layer // 完成当前地表作者事务的受检参数或值转换。
                        .min(surface.layers.len().saturating_sub(1)); // 保持当前选择有效。
            } else {
                // 当前受检分支或参数组结束。
                // 重做只接受原追加位置仍然有效的层表。
                if surface.layers.len() != *index // 依据完整作者条件选择有限处理分支。
                    || surface // 依据完整作者条件选择有限处理分支。
                        .layers // 地表局部层表是唯一作者索引依据。
                        .iter() // 完成当前地表作者事务的受检参数或值转换。
                        .any(|candidate| candidate.id == layer.id)
                // 完成当前地表作者事务的受检参数或值转换。
                {
                    // 满足上述完整条件后才进入当前分支。
                    // 精确复核原数组末尾和唯一ID。
                    return Err("待重做的地表局部层位置或身份已失效".to_owned()); // 错误历史不制造重名引用。
                } // 懒新增重做身份校验结束。
                surface.layers.push(layer.clone()); // 恢复原层完整ID、标签和预设引用。
                editor.selected_surface_layer = *index; // 恢复新增时的局部选择。
            } // 懒引用历史分支结束。
            editor.surface_selected_preset_id = surface
                .layers
                .get(editor.selected_surface_layer) // 新建撤销和重做后同步实际层的预设。
                .map(|selected| selected.preset_id.clone()); // 空表清空选择，重做不沿用用户后来选的其他预设。
        } // 懒加入层历史结束。
        EditorCommand::SurfaceAuthoringChanged(change) => {
            // 完成当前地表作者事务的受检参数或值转换。
            // 删除定义和两图作为唯一命令恢复。
            apply_surface_authoring_change(draft, change, undoing)?; // 全部校验通过后才原子交换快照。
            let surface = draft.surface.as_ref().expect("完整地表快照刚成功安装"); // 空表仍保留可继续绘制的作者画布。
            editor.selected_surface_layer = change
                .removed_index
                .min(surface.layers.len().saturating_sub(1)); // 空表安全归零，撤销选回原层。
            editor.surface_selected_preset_id = surface
                .layers
                .get(editor.selected_surface_layer) // 空表没有可选层，不进行越界访问。
                .map(|layer| layer.preset_id.clone()); // 清除已删除预设选择，等待用户选择新绘制目标。
        } // 整层删除历史结束。
        _ => return Err("请求应用的历史命令不属于地表编辑".to_owned()), // 未来调用方不能误把其他作者域命令当作成功。
    } // 地表历史原子分支结束。
    draft.surface_dirty = true; // 普通差分、懒层和删除快照均复用配对保存链路。
    Ok(()) // 统一历史系统随后推进一次地表与全局作者修订。
} // 地表历史应用结束。
