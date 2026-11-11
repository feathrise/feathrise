//! 地表层删除与独立写侧准备的输入冻结、single-flight调度和完整采用。

use super::super::resources::{EditorAuthoringSession, EditorCommand}; // 删除复用统一撤销与地表脏修订。
use super::helpers::{
    // 引入地表作者层事务所需类型和纯助手。
    definitions_match,
    install_snapshot,
    materialize_snapshot,
    remove_layer,
    snapshot,          // 完成当前地表作者事务的受检参数或值转换。
    validate_snapshot, // 完成当前地表作者事务的受检参数或值转换。
}; // 所有整图操作留在纯助手后台调用中。
use super::resources::SurfaceLayerEditTask; // 删除和物化共用唯一在途槽。
use super::types::{SurfaceAuthoringChange, SurfaceEditKind, SurfaceEditResult}; // 引入地表作者层事务所需类型和纯助手。
use crate::resources::SceneEditorState; // 状态、场景选择与F12生命周期共同约束采用。
use bevy::prelude::ResMut; // 主世界系统独占当前作者会话和任务资源。
use bevy::tasks::{AsyncComputeTaskPool, futures::check_ready}; // CPU任务只异步调度，主线程不阻塞等待。
use std::sync::Arc; // 大型输入只浅复制，采用前用指针验证同一个权威像素版本。 // 完整输入身份与结果同任务传递。

pub(crate) fn start_surface_layer_delete(
    // 地表作者操作在受检输入下执行当前步骤。
    editor: &mut SceneEditorState, // 当前F12场景选择、保存关闭意图与状态。
    session: &EditorAuthoringSession, // 完整作者修订共同防止迟到结果回写。
    task: &mut SurfaceLayerEditTask, // 地表删除和写侧准备共用single-flight任务槽。
    layer_index: usize,          // 冻结场景局部层位置，防止删除或绘制其他作者层。
    layer_id: &str,              // 稳定作者层和预设引用保持真实对应关系。
) -> Result<(), String> {
    // 当前受检分支或参数组结束。
    // UI必须提交其可见行的稳定层ID与原局部位置。
    if !editor.open || session.saving {
        // 依据完整作者条件选择有限处理分支。
        // 关闭状态和保存中的草稿不接收新删除。
        return Err("请在场景编辑器打开且保存完成后删除地表作者层".to_owned()); // 不建立后台任务或改写草稿。
    } // 编辑生命周期前置检查结束。
    if task.is_pending() {
        // 依据完整作者条件选择有限处理分支。
        // 不能通过再次点击制造第二份整图副本。
        return Err("上一项地表删除或画布准备仍在后台处理，请等待完成".to_owned()); // 继续持有旧槽直到真实结束。
    } // single-flight背压结束。
    let draft = session // 只操作已经完整加载的当前场景草稿。
        .draft // 只操作已经完整加载的当前场景草稿。
        .as_ref() // 只读借用当前完整作者值。
        .ok_or_else(|| "当前场景作者数据尚未就绪".to_owned())?; // 只操作已完整加载草稿。
    if editor.selected_scene_id.as_ref() != Some(&draft.scene_id) {
        // 稳定场景身份必须与当前草稿一致。
        // UI场景与草稿场景必须一致。
        return Err("当前地表作者层所属场景已变化，请重新选择".to_owned()); // 旧行点击不能跨场景删除。
    } // 场景身份校验结束。
    let source = snapshot(draft)?; // 大型像素只冻结Arc句柄。
    validate_snapshot(&source)?; // 不完整作者图不会排入后台删除。
    let layer =
        source // 保持当前地表操作的数据和原子采用边界。
            .surface // 地表局部层表是唯一作者索引依据。
            .layers // 地表局部层表是唯一作者索引依据。
            .get(layer_index) // 冻结场景局部层位置，防止删除或绘制其他作者层。
            .filter(|layer| layer.id == layer_id) // 稳定作者层和预设引用保持真实对应关系。
            .ok_or_else(|| "待删除的地表作者层位置或身份已变化，请重新选择".to_owned())?; // 同名层按稳定ID区分。
    let label = layer.label.clone(); // 任务状态使用真实作者层名称。
    task.ticket = task.ticket.wrapping_add(1); // 单任务槽每次启动获得独立票据。
    let ticket = task.ticket; // 后台结果必须回报相同票据。
    let revision = session.revision; // 任一场景或全局作者动作都可取消旧删除采用。
    let surface_revision = session.surface_revision; // 额外冻结地表专用身份。
    task.deleting = true; // 保存与关闭开始等待该完整作者事务。
    task.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        // 完成当前地表作者事务的受检参数或值转换。
        // 一次删除仅占一个CPU worker。
        let result = remove_layer(&source, layer_index); // 整图复制、重映射与归一化全部后台完成。
        SurfaceEditResult {
            // 保持当前地表操作的数据和原子采用边界。
            ticket,           // 任务票据必须属于当前唯一后台槽。
            revision,         // 完整作者修订共同防止迟到结果回写。
            surface_revision, // 地表专用修订对应当前定义和两张图。
            before: source,   // 只读输入保留操作前的完整作者版本。
            kind: SurfaceEditKind::Delete { index: layer_index }, // 冻结场景局部层位置，防止删除或绘制其他作者层。
            result, // 完整输出成功后才允许作为单一版本采用。
        } // 原输入和完整结果一起交还主世界。
    })); // 调度后立即返回，当前画面继续使用上一完整地表。
    editor.status = format!("正在后台删除地表作者层“{label}”并重新分配覆盖；完成后可撤销"); // 明确显示尚未采用状态。
    Ok(()) // 成功仅表示任务已经排入，dirty在完整采用时才变化。
} // 删层请求结束。

pub(crate) fn start_surface_materialization_if_shared(
    // 地表作者操作在受检输入下执行当前步骤。
    session: &EditorAuthoringSession, // 完整作者修订共同防止迟到结果回写。
    task: &mut SurfaceLayerEditTask,  // 地表删除和写侧准备共用single-flight任务槽。
) -> Result<bool, String> {
    // 当前受检分支或参数组结束。
    // 笔刷或局部撤销需要写侧时调用。
    if task.is_pending() {
        // 依据完整作者条件选择有限处理分支。
        // 删除和复制共同占用单槽，不能并行堆积整图任务。
        return Ok(true); // 调用方保留完整输入意图，下一帧继续。
    } // 在途任务检查结束。
    let draft = session // 只操作已经完整加载的当前场景草稿。
        .draft // 只操作已经完整加载的当前场景草稿。
        .as_ref() // 只读借用当前完整作者值。
        .ok_or_else(|| "当前场景作者数据尚未就绪".to_owned())?; // 缺草稿无法准备任何写侧。
    if let Some((scene_id, revision, surface_revision, detail)) = task.materialize_failure.take() // 一次取走错误交给上层取消当前完整待补意图。
        && draft.scene_id == scene_id && session.revision == revision && session.surface_revision == surface_revision
    // 完整作者修订共同防止迟到结果回写。
    // 旧图或旧修订失败不得阻断新作者内容。
    {
        // 满足上述完整条件后才进入当前分支。
        // 只有同输入的真实失败才终止本次请求。
        return Err(detail); // 待补笔划会收口、撤销会清请求；下一次明确操作才允许人工重试。
    } // 同输入失败闩锁消费结束。
    let weights = draft // 保持当前地表操作的数据和原子采用边界。
        .surface_weights // 读取或传递同版本RGBA8权重作者源。
        .as_ref() // 只读借用当前完整作者值。
        .ok_or_else(|| "当前场景缺少地表权重作者源".to_owned())?; // 权重必须存在。
    let indices = draft // 保持当前地表操作的数据和原子采用边界。
        .surface_indices // 读取或传递同版本小端RGBA32局部索引源。
        .as_ref() // 只读借用当前完整作者值。
        .ok_or_else(|| "当前场景缺少地表索引作者源".to_owned())?; // 索引必须与权重配对。
    if Arc::strong_count(weights) == 1 && Arc::strong_count(indices) == 1 {
        // 依据完整作者条件选择有限处理分支。
        // 唯一拥有者可直接局部写入。
        return Ok(false); // 没有整图复制，也无需任何任务调度。
    } // 独占所有权检查结束。
    let source = snapshot(draft)?; // 历史或预览仍持有旧像素时只浅克隆输入。
    task.ticket = task.ticket.wrapping_add(1); // 写侧任务同样需要票据，防止旧结果误用。
    let ticket = task.ticket; // 冻结当前任务身份。
    let revision = session.revision; // 完整作者修订换代时旧副本拒收。
    let surface_revision = session.surface_revision; // 地表修订额外保护当前图版本。
    task.deleting = false; // 复制不改变语义，不增加保存或关闭等待责任。
    task.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        // 完成当前地表作者事务的受检参数或值转换。
        // 两张图共享一个后台CPU任务。
        let result = materialize_snapshot(&source); // 分配与逐字节复制仅在后台执行。
        SurfaceEditResult {
            // 保持当前地表操作的数据和原子采用边界。
            ticket,                             // 任务票据必须属于当前唯一后台槽。
            revision,                           // 完整作者修订共同防止迟到结果回写。
            surface_revision,                   // 地表专用修订对应当前定义和两张图。
            before: source,                     // 只读输入保留操作前的完整作者版本。
            kind: SurfaceEditKind::Materialize, // 完成当前地表作者事务的受检参数或值转换。
            result,                             // 完整输出成功后才允许作为单一版本采用。
        } // 整套候选成功后才能安装。
    })); // 输入帧立即返回，不能等待未来完成值。
    Ok(true) // 上层冻结笔刷样本或撤销请求，避免共享等待漏掉单击。
} // 后台写侧请求结束。

pub(super) fn adopt_result(
    // 地表作者操作在受检输入下执行当前步骤。
    editor: &mut SceneEditorState, // 当前F12场景选择、保存关闭意图与状态。
    session: &mut EditorAuthoringSession, // 完整作者修订共同防止迟到结果回写。
    ticket: u64,                 // 任务票据必须属于当前唯一后台槽。
    output: SurfaceEditResult,   // 完成当前地表作者事务的受检参数或值转换。
) -> Result<bool, String> {
    // 当前受检分支或参数组结束。
    // 测试与ECS轮询共享同一原子采用边界。
    let current = editor.open // 保持当前地表操作的数据和原子采用边界。
        && !session.saving // 依据完整作者条件选择有限处理分支。
        && ticket == output.ticket // 任务票据必须属于当前唯一后台槽。
        && session.revision == output.revision // 完整作者修订共同防止迟到结果回写。
        && session.surface_revision == output.surface_revision // 地表专用修订对应当前定义和两张图。
        && editor.selected_scene_id.as_ref() == Some(&output.before.surface.scene_id) // 稳定场景身份必须与当前草稿一致。
        && session.draft.as_ref().is_some_and(|draft| { // 只读借用当前完整作者值。
            // 先比较固定身份，再复核完整元数据和两个共享输入指针。
            draft.scene_id == output.before.surface.scene_id // 稳定场景身份必须与当前草稿一致。
                && draft // 依据完整作者条件选择有限处理分支。
                    .surface // 地表局部层表是唯一作者索引依据。
                    .as_ref() // 只读借用当前完整作者值。
                    .is_some_and(|surface| definitions_match(surface, &output.before.surface)) // 只继续处理存在且身份仍匹配的作者值。
                && draft // 依据完整作者条件选择有限处理分支。
                    .surface_weights // 读取或传递同版本RGBA8权重作者源。
                    .as_ref() // 只读借用当前完整作者值。
                    .is_some_and(|weights| Arc::ptr_eq(weights, &output.before.weights)) // 只继续处理存在且身份仍匹配的作者值。
                && draft // 依据完整作者条件选择有限处理分支。
                    .surface_indices // 读取或传递同版本小端RGBA32局部索引源。
                    .as_ref() // 只读借用当前完整作者值。
                    .is_some_and(|indices| Arc::ptr_eq(indices, &output.before.indices)) // 避免同图重载或未推进修订的源替换冒充原输入。
        }); // 完整输入身份检查结束。
    if !current {
        // 依据完整作者条件选择有限处理分支。
        // 切换场景、关闭、作者修改或旧票据都使候选失效。
        return Ok(false); // 即使结果包含错误，也不能污染新会话状态。
    } // 迟到完成值过滤结束。
    let after = output.result?; // 只有当前任务的真实失败才向用户报告。
    validate_snapshot(&after)?; // 两图齐全之前不得修改草稿任一字段。
    let draft = session.draft.as_mut().expect("刚验证当前草稿身份"); // 当前独占系统内身份不会中途变化。
    match output.kind {
        // 依据完整作者条件选择有限处理分支。
        // 作者删除与无语义物化拥有不同历史责任。
        SurfaceEditKind::Delete { index } => {
            // 保持当前地表操作的数据和原子采用边界。
            // 全部重映射完成才建立唯一删除命令。
            let removed_label = output.before.surface.layers[index].label.clone(); // 使用被冻结的作者层名称显示结果。
            install_snapshot(draft, &after); // 定义和两张图作为一个完整版本安装。
            let selected = index.min(after.surface.layers.len().saturating_sub(1)); // 最后一层删除后使用零哨兵，避免空表减法下溢。
            editor.selected_surface_layer = selected; // 非空时选后继，空层表由预设None表示无选择。
            editor.surface_selected_preset_id = after
                .surface
                .layers
                .get(selected)
                .map(|layer| layer.preset_id.clone()); // 空层不再保留旧预设，防止下一笔意外重新加入被删层。
            session // 完成当前地表作者事务的受检参数或值转换。
                .undo // 完整作者动作进入唯一线性撤销历史。
                .push(EditorCommand::SurfaceAuthoringChanged(Box::new(
                    // 完整作者动作进入唯一线性撤销历史。
                    SurfaceAuthoringChange {
                        // 保持当前地表操作的数据和原子采用边界。
                        removed_index: index, // 冻结场景局部层位置，防止删除或绘制其他作者层。
                        before: output.before, // 只读输入保留操作前的完整作者版本。
                        after,                // 完整输出成功后才允许作为单一版本采用。
                    }, // 当前受检分支或参数组结束。
                ))); // 一条命令精确保留原字节和新字节。
            session.redo.clear(); // 新作者操作按统一线性历史丢弃旧重做分支。
            session.mark_surface_changed(editor); // 统一推动dirty、作者修订、地表预览和保存。
            editor.status = // 完成当前地表作者事务的受检参数或值转换。
                format!("已删除地表作者层“{removed_label}”及其覆盖，可撤销并保存"); // 最后一层清空与多层重分配均适用，不宣称存在剩余层。
        } // 删除完整采用结束。
        SurfaceEditKind::Materialize => {
            // 保持当前地表操作的数据和原子采用边界。
            // 同值写侧准备不推进作者或预览修订。
            draft.surface_weights = Some(after.weights); // 移入唯一权重Arc，让下一帧局部写入无需复制。
            draft.surface_indices = Some(after.indices); // 同次移动索引写侧，拒绝跨版本半套图。
        } // 独立写侧采用结束。
    } // 操作种类处理结束。
    Ok(true) // 所有权威字段已经原子采用。
} // 完整结果采用结束。

pub(super) fn finish_result(
    // 地表作者操作在受检输入下执行当前步骤。
    editor: &mut SceneEditorState, // 当前F12场景选择、保存关闭意图与状态。
    session: &mut EditorAuthoringSession, // 完整作者修订共同防止迟到结果回写。
    task: &mut SurfaceLayerEditTask, // 地表删除和写侧准备共用single-flight任务槽。
    output: SurfaceEditResult,   // 完成当前地表作者事务的受检参数或值转换。
) {
    // 当前受检分支或参数组结束。
    // 统一处理完成、取消、失败以及等待中的保存关闭意图。
    let deleting = task.deleting; // 删除完成值过期需要给用户明确反馈。
    let same_open_scene = editor.open // 只有原任务所属的仍打开场景才拥有其等待保存/关闭请求。
        && editor.selected_scene_id.as_ref() == Some(&output.before.surface.scene_id) // 旧场景不能清理新图UI上的生命周期意图。
        && session.draft.as_ref().is_some_and(|draft| draft.scene_id == output.before.surface.scene_id); // 草稿和UI场景都必须仍属于原任务。
    let failure_identity = (
        // 保持当前地表操作的数据和原子采用边界。
        output.before.surface.scene_id.clone(), // 稳定场景身份必须与当前草稿一致。
        output.revision,                      // 完整作者修订共同防止迟到结果回写。
        output.surface_revision,              // 地表专用修订对应当前定义和两张图。
    ); // 错误返回后仍能绑定同一等待中的作者意图。
    task.deleting = false; // 下一次保存或关闭不再等待已经完成的后台工作。
    match adopt_result(editor, session, task.ticket, output) {
        // 依据完整作者条件选择有限处理分支。
        // 所有副作用集中在经过身份检查的采用边界。
        Ok(true) => {} // 当前完整结果已经采用，状态由作者删除路径发布。
        Ok(false) if deleting && same_open_scene => {
            // 同图换代可以提示取消，跨场景迟到结果必须静默丢弃。
            // 旧删除未完成时不能把保存旧地表冒充用户等待的结果。
            editor.save_requested = false; // 原草稿和dirty保留，修复选择后由作者明确重试保存。
            editor.cancel_close_request(
                // 完成当前地表作者事务的受检参数或值转换。
                "地表删除期间场景或作者内容已变化，已取消旧删除，请重新选择作者层", // 回归检查当前作者定义、像素或生命周期不变量。
            ); // 保持F12打开以说明取消原因。
        } // 过期作者删除的生命周期请求清理结束。
        Ok(false) => {} // 失效写侧会在下一次真实写入意图时重新准备。
        Err(error) => {
            // 完成当前地表作者事务的受检参数或值转换。
            // 当前任务真实失败必须停止自动重试，不让保存和关闭永久等待。
            if !deleting {
                // 依据完整作者条件选择有限处理分支。
                // 删除已完整结束；只有共享写侧存在待补笔划或撤销请求需要消费错误。
                task.materialize_failure = Some((
                    // 完成当前地表作者事务的受检参数或值转换。
                    failure_identity.0, // 完成当前地表作者事务的受检参数或值转换。
                    failure_identity.1, // 完成当前地表作者事务的受检参数或值转换。
                    failure_identity.2, // 完成当前地表作者事务的受检参数或值转换。
                    error.clone(),      // 小型定义或Arc句柄复制不遍历整图像素。
                )); // 下一次准备调用返回该错误并取消当前意图。
            } // 写侧失败闩锁登记结束。
            editor.save_requested = false; // 本次作者操作没有完成，不能继续保存旧图并掩盖失败状态。
            editor.cancel_close_request(format!("地表作者操作失败，已保留原地表：{error}")); // 失败时两张旧图、定义和历史全部保留，等待关闭取消以便作者看到错误。
        } // 真实失败处理结束。
    } // 当前完成值只被消费一次。
} // 完整结果生命周期处理结束。

pub(crate) fn poll_surface_layer_edit(
    // 地表作者操作在受检输入下执行当前步骤。
    mut editor: ResMut<SceneEditorState>, // 当前F12场景选择、保存关闭意图与状态。
    mut session: ResMut<EditorAuthoringSession>, // 完整作者修订共同防止迟到结果回写。
    mut task: ResMut<SurfaceLayerEditTask>, // 地表删除和写侧准备共用single-flight任务槽。
) {
    // 当前受检分支或参数组结束。
    // 插件每帧最多轮询一个任务一次。
    let Some(pending) = task.task.as_mut() else {
        // 独占借用当前作者字段，尚未写入任何像素。
        return; // 本帧不再继续消费或修改作者数据。
    }; // 没有在途任务时常数时间返回。
    let Some(output) = check_ready(pending) else {
        // 保持当前地表操作的数据和原子采用边界。
        return; // 本帧不再继续消费或修改作者数据。
    }; // 尚未完成时立即返回，禁止等待或忙轮询。
    task.task = None; // 任务真正结束后才释放single-flight槽。
    finish_result(&mut editor, &mut session, &mut task, output); // 成功或失败一次性推进作者和等待请求状态。
} // 非阻塞轮询结束。
