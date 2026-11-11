//! 全局世界环境单文件保存入口与完成采用；严格绕过全部场景派生预检。

use bevy::prelude::*; // AssetServer 与 ECS 资源用于启动、轮询和立即发布。
use bevy::tasks::{IoTaskPool, futures::check_ready}; // 小型 JSON 仍在 IO 线程执行原子事务，主线程只非阻塞轮询。
use feathrise_scene_content::{WORLD_ENVIRONMENT_ASSET_PATH, WorldEnvironmentRegistry}; // 保存成功后立即更新正式动态场景配置并显式请求热重载。

use super::super::resources::{
    DraftSaveTask, EditorAuthoringSession, GlobalEnvironmentAuthoringState,
    GlobalEnvironmentSaveTask,
}; // 全局单文件任务与联合场景任务共用同一保存互斥位。
use super::super::{SurfaceBrushStroke, SurfaceLayerEditTask}; // 全局保存也必须等待尚未dirty的地表删除或冻结单击。
use super::save_global_environment; // 文件写入仍经过统一 PreparedWrite 回滚事务。
use crate::resources::{SceneDeletionTask, SceneEditorState}; // 回收互斥、顶部保存请求、脏状态与关闭反馈。

/// 只有全局配置脏且场景无修改时，直接启动唯一配置文件原子保存。
pub(crate) fn start_global_environment_save(
    mut editor: ResMut<SceneEditorState>, // 消费顶部统一保存请求并发布状态。
    mut session: ResMut<EditorAuthoringSession>, // 复用 saving 与联合作者修订身份。
    global: Res<GlobalEnvironmentAuthoringState>, // 冻结完整有效全局草稿。
    mut task: ResMut<GlobalEnvironmentSaveTask>, // 同时最多一个全局单文件任务。
    scene_save: Res<DraftSaveTask>,       // 场景联合事务存在时不能并发发布同一全局目标。
    deletion: Res<SceneDeletionTask>,     // 回收事务拥有 catalog 与私有目录期间暂停保存。
    surface_task: Option<Res<SurfaceLayerEditTask>>, // 全局配置的独立测试场景允许没有场景删层资源。
    surface_stroke: Option<Res<SurfaceBrushStroke>>, // 若场景正在补画，保留顶部请求等待形成完整联合脏状态。
) {
    if editor.save_requested // 依据完整作者条件选择有限处理分支。
        && (surface_task // 依据完整作者条件选择有限处理分支。
            .as_deref() // 完成当前地表作者事务的受检参数或值转换。
            .is_some_and(SurfaceLayerEditTask::is_deleting) // 地表删除和写侧准备共用single-flight任务槽。
            || surface_stroke // 依据完整作者条件选择有限处理分支。
                .as_deref() // 完成当前地表作者事务的受检参数或值转换。
                .is_some_and(|stroke| stroke.active))
    // 只继续处理存在且身份仍匹配的作者值。
    {
        // 满足上述完整条件后才进入当前分支。
        // 不允许仅全局保存抢先冻结尚未收口的地表作者事务。
        editor.status = "当前地表作者操作正在后台完成；收口后会自动继续保存".to_owned(); // 删除成功后请求会自然进入场景联合保存。
        return; // 保留save_requested，且不启动任何磁盘IO。
    } // 地表原子作者事务保存门结束。
    if !editor.save_requested || !global.dirty || session.has_scene_changes() {
        return; // 场景也脏时保留请求，交给后续联合事务完整处理。
    }
    editor.save_requested = false; // 从此处起本系统完整拥有这次仅全局请求。
    if deletion.busy() {
        editor.cancel_global_close_with_error("场景回收事务完成前不能保存全局世界环境"); // 不建立任何 IO 任务。
        return;
    }
    if task.task.is_some() || scene_save.task.is_some() || session.saving {
        editor.status = "世界环境保存任务正在执行".to_owned(); // 防止重复点击建立第二写侧。
        return;
    }
    if let Err(error) = global.draft.validate() {
        editor.cancel_global_close_with_error(format!("全局世界环境无效，保存已停止：{error}")); // 非法值在首个 stage 前失败关闭。
        return;
    }
    let snapshot = global.draft.clone(); // 六组小型值与 schema 作为同一不可变任务输入。
    let session_revision = session.revision; // 与联合撤销栈身份绑定。
    let global_revision = global.revision; // 迟到结果不能清理新草稿。
    task.ticket = task.ticket.wrapping_add(1); // 每次启动使用不可复用票据。
    let ticket = task.ticket;
    session.saving = true; // 保存期间所有作者输入与撤销统一暂停。
    task.task = Some(IoTaskPool::get().spawn(async move {
        let result = save_global_environment(&snapshot); // 不触碰任何场景文件或派生构建器。
        (ticket, session_revision, global_revision, snapshot, result)
    }));
    editor.status = "正在后台原子保存全局太阳/月光基础设置".to_owned(); // 明确本次没有场景预检。
}

/// 非阻塞采用全局单文件事务结果；失败保留草稿、历史和脏状态。
pub(crate) fn poll_global_environment_save(
    mut task: ResMut<GlobalEnvironmentSaveTask>, // 只轮询当前唯一任务槽。
    mut session: ResMut<EditorAuthoringSession>, // 成功后清理已落盘全局历史，失败保留。
    mut global: ResMut<GlobalEnvironmentAuthoringState>, // 按修订更新保存基线与脏状态。
    mut editor: ResMut<SceneEditorState>,          // 发布用户可见结果并推进关闭门。
    mut registry: Option<ResMut<WorldEnvironmentRegistry>>, // 正式运行时立即采用同一成功快照。
    asset_server: Res<AssetServer>,              // 成功或回滚后显式请求唯一配置资产重载。
) {
    let Some(active_task) = task.task.as_mut() else {
        return; // 稳定帧没有任务时保持零工作。
    };
    let Some((ticket, session_revision, global_revision, snapshot, result)) =
        check_ready(active_task)
    else {
        return; // IO 未完成时不阻塞 Update。
    };
    task.task = None; // 完成值只消费一次。
    session.saving = false; // 成功、失败或过期都归还作者输入。
    if ticket != task.ticket
        || session_revision != session.revision
        || global_revision != global.revision
    {
        editor.cancel_close_request(
            "全局世界环境保存结果已经过期；草稿和撤销记录均已保留，请重新保存",
        ); // 迟到结果不能清理较新的作者责任。
        editor.record_global_status_result(crate::types::EditorStatusKind::Warning);
        return;
    }
    match result {
        Ok(()) => {
            global.saved = snapshot.clone(); // 成功磁盘内容成为新的撤销脏判断基线。
            global.draft = snapshot.clone(); // 保存期间输入冻结，仍显式保持同一权威快照。
            global.dirty = false; // 单文件事务完整成功后才解除全局责任。
            global.load_error = None; // 成功保存可修复启动时的缺失或无效文件。
            session
                .undo
                .retain(|command| !command.is_global_environment()); // 已落盘全局操作不再跨越保存基线撤销。
            session
                .redo
                .retain(|command| !command.is_global_environment()); // 重做分支同样不能越过新基线。
            editor.dirty = session.has_scene_changes(); // 仅全局路径通常归零，但仍防御并发场景责任。
            session.successful_save_sequence = session.successful_save_sequence.wrapping_add(1); // 仅全局保存也向冻结切换场景请求提供成功回执。
            if let Some(registry) = registry.as_deref_mut() {
                registry.celestial_lighting = snapshot.celestial_lighting.clone().normalized(); // 同帧运行时使用成功落盘值。
                registry.last_error = None; // F12 全局加载错误立即清除。
                registry.revision = registry.revision.wrapping_add(1); // 动态场景重新解析场景覆盖。
                global.source_revision = registry.revision; // 后续热重载同值不会误判为未观察来源。
            }
            asset_server.reload(WORLD_ENVIRONMENT_ASSET_PATH); // 文件监听关闭或合并事件时仍显式交还 AssetServer 权威。
            editor.status = "全局太阳/月光基础设置已原子保存".to_owned();
            editor.record_global_status_result(crate::types::EditorStatusKind::Success);
        }
        Err(error) => {
            for path in error.reload_asset_paths() {
                asset_server.reload(path.clone()); // 事务曾发布又回滚时纠正可能读到中间值的缓存。
            }
            editor.cancel_global_close_with_error(format!(
                "全局世界环境保存失败：{error}；草稿、撤销记录和脏状态已保留"
            )); // 任一失败都允许作者直接重试。
        }
    }
}
