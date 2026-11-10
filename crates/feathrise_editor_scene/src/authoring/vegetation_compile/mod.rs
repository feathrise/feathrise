//! 植被作者图层的有界后台编译、迟到拒收和原子采用。
pub(crate) use compiler::author_layers_at_world; // 选择候选复用正式编译的单点贡献判定。

mod budget; // 正式输出与单组临时整图的显式内存上限。
mod compiler; // 纯CPU组解析、稳定覆盖和不同分辨率合并。
mod resources; // single-flight任务槽与完整完成值。
#[cfg(test)]
mod tests; // 南北规则差异、固定运行时层数与保细节降采样回归。

use bevy::prelude::*; // AsyncComputeTaskPool、ECS资源和日志。
use bevy::tasks::AsyncComputeTaskPool; // 整图多层编译不得阻塞帧主线程。
use bevy::tasks::futures::check_ready; // Update只非阻塞探测任务完成值。

pub(crate) use compiler::compile_vegetation_authoring; // 跨网格事务也在唯一后台槽复用同一编译规则。
pub(crate) use resources::{CompiledDensityImage, CompiledDensityStats, VegetationCompileSource, VegetationCompileState}; // 来源身份供图片准备共用。

use super::resources::{EditorAuthoringSession, VegetationBrushStroke}; // 当前草稿、笔划和作者/编译修订身份。
use crate::resources::SceneEditorState; // 失败、完成和等待状态回显。

/// 作者修订变化时启动最多一个完整编译任务；在途旧任务结束前只合并最新期望身份。
pub(crate) fn start_vegetation_compile(
    session: Res<EditorAuthoringSession>,
    mut state: ResMut<VegetationCompileState>,
) {
    let Some(draft) = session.draft.as_ref() else {
        state.invalidate(); // 没有草稿时旧任务结果不得获得采用资格。
        return;
    };
    if draft.vegetation.is_none() {
        state.invalidate(); // 当前场景没有通用植被作者文件。
        return;
    }
    state.desired_scene_id = Some(draft.scene_id.clone()); // 每帧只写轻量稳定身份。
    state.desired_revision = session.vegetation_revision; // 合并笔划期间连续修订到最新值。
    let Some(source) = VegetationCompileSource::capture(&session) else { state.invalidate(); return; };
    state.desired_source = Some(source.clone()); // 连续刷印也按单飞槽编译，不为每帧新建任务。
    if session.vegetation_compiled_revision == session.vegetation_revision
        || state.task.is_some()
        || (state.failed_scene_id.as_ref() == Some(&draft.scene_id)
            && state.failed_revision == Some(session.vegetation_revision))
    {
        return; // 当前已编译、任务在途或同修订已确定失败都不重复启动。
    }
    let vegetation = draft
        .vegetation
        .as_ref()
        .expect("上方已确认植被存在")
        .clone(); // 描述符很小，可作为任务不可变快照。
    let densities = draft.vegetation_author_densities.clone(); // BTreeMap只克隆键与Arc，不复制R8像素。
    let revision = session.vegetation_revision; // 任务输入修订。
    let world_min = draft.terrain_manifest().world_min;
    let world_max = draft.terrain_manifest().world_max;
    state.task = Some(AsyncComputeTaskPool::get().spawn(async move {
        let result = compile_vegetation_authoring(&vegetation, &densities, world_min, world_max); // 后台只返回纯Vec/Arc数据，不借用World或Assets。
        (source, revision, result) // 场景/网格/结构共同冻结，像素新修订不会饿死完整预览。
    }));
}

/// 同一作者来源的完整结果可连续预览；保存仍等待编译修订追上最后一枚刷印。
pub(crate) fn poll_vegetation_compile(
    mut session: ResMut<EditorAuthoringSession>,
    mut editor: ResMut<SceneEditorState>,
    mut state: ResMut<VegetationCompileState>,
    stroke: Option<Res<VegetationBrushStroke>>,
) {
    let Some(task) = state.task.as_mut() else {
        return; // 稳定帧没有任务时常数成本退出。
    };
    let Some((source, revision, result)) = check_ready(task) else {
        return; // 非阻塞轮询，未完成不等待线程。
    };
    state.task = None; // CPU密集任务已经真正结束，下一帧才允许启动新修订。
    let current = session
        .draft
        .as_ref()
        .is_some_and(|draft| draft.scene_id == source.scene_id)
        && state.desired_scene_id.as_ref() == Some(&source.scene_id)
        && state.desired_source.as_ref() == Some(&source)
        && VegetationCompileSource::capture(&session).as_ref() == Some(&source)
        && (revision == session.vegetation_revision || (revision < session.vegetation_revision
            && stroke.as_ref().is_some_and(|stroke| stroke.active))); // 仅连续刷印可采用中间结果；松笔后撤销等操作仍拒收迟到输出。
    if !current {
        return; // 迟到完整结果无任何草稿、ECS或文件副作用。
    }
    match result {
        Ok(compiled) => {
            let Some(draft) = session.draft.as_mut() else {
                return; // 上方身份判断后的防御性借用门。
            };
            let output_ids = compiled.outputs.keys().cloned().collect::<Vec<_>>(); // 先冻结本次原子正式输出全集。
            draft.vegetation_densities = compiled.outputs; // 一次替换所有正式层，绝不发布半组结果。
            draft.vegetation_nonzero_layers = compiled.nonzero_output_ids; // 后台完成的非零身份与R8同次采用，预览无需在主线程重扫像素。
            draft.vegetation_density_stats = compiled.output_stats; // F4只读取同修订常数统计，不在显示帧重新遍历R8。
            draft.vegetation_conflicts = compiled.conflicts; // 冲突预览与正式输出保持同修订。
            draft.vegetation_dirty_layers.extend(output_ids); // 保存时派生全部受当前作者图影响的正式R8/KTX2。
            session.vegetation_compiled_revision = revision; // 实际模型与Blade Image只观察完成修订。
            session.vegetation_compiled_structure_revision = source.structure_revision;
            state.failed_scene_id = None; // 新修订成功解除旧错误门。
            state.failed_revision = None; // 后续作者变化可继续编译。
            editor.status = format!(
                "植被作者图层修订{revision}已编译为正式运行时层；四级LOD、风场与贴地继续复用现有系统"
            ); // 明确说明没有为作者层新建低配运行时。
        }
        Err(error) => {
            state.failed_scene_id = Some(source.scene_id.clone()); // 同一坏修订不再每帧重试。
            state.failed_revision = Some(revision); // 新笔划或参数变化自然解除门。
            editor.status = format!("植被作者图层编译失败：{error}"); // 保留上一完整正式输出和画面。
            error!("{}", editor.status); // 每个失败修订只记录一次日志。
        }
    }
}
