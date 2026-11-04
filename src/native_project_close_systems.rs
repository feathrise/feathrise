//! 工程退出推进；每帧只检查原有任务槽，保存和写盘继续由各作者领域负责。

use super::{components::CloseAction, resources::*};
use bevy::prelude::*;
use feathrise_editor_map::standalone::{
    ProjectSaveProgress, SavedPlayFeedback, cancel_project_save, project_close_snapshot,
    project_save_progress, request_save_all,
};

/// 返回true只是作者部分已经允许关闭；宿主仍须确认自己拥有的游戏和构建全部收口。
pub(crate) fn authorize_exit(world: &mut World, requested: bool) -> bool {
    if requested && !world.resource::<NativeProjectCloseState>().active() {
        let previous_failure = match project_save_progress(world) {
            ProjectSaveProgress::Failed { detail } => Some(detail),
            _ => None,
        };
        let mut state = world.resource_mut::<NativeProjectCloseState>();
        state.stage = CloseStage::Inspecting;
        state.failed = previous_failure;
        state.host_blocked = None;
        state.startup_save_pending = false;
        state.intent = None;
        state.focus_cancel = true;
    }
    let (stage, choice) = {
        let mut state = world.resource_mut::<NativeProjectCloseState>();
        (state.stage, state.choice.take())
    };
    if stage == CloseStage::Idle {
        return false;
    }
    if choice == Some(CloseAction::Cancel) {
        cancel_project_save(world);
        let mut state = world.resource_mut::<NativeProjectCloseState>();
        state.stage = CloseStage::Idle;
        state.intent = None;
        state.failed = None;
        state.host_blocked = None;
        state.startup_save_pending = false;
        state.status.clear();
        let operation = state.target().operation();
        world.write_message(SavedPlayFeedback(
            format!("已取消{operation}，作者草稿和历史保留"),
        ));
        return false;
    }
    if world
        .resource::<NativeProjectCloseState>()
        .host_blocked
        .is_some()
    {
        return false;
    }
    // 取消后原写者仍会收口；再次关闭可能先等到Running，随后才收到“已写盘但采用失败”。
    // 每次检查都保留原调度器的失败，不能仅靠首次关闭时的快照或已清除的dirty放行退出。
    if let ProjectSaveProgress::Failed { detail } = project_save_progress(world)
        && world.resource::<NativeProjectCloseState>().intent != Some(CloseAction::Discard)
    {
        world.resource_mut::<NativeProjectCloseState>().failed = Some(detail);
    }
    if stage == CloseStage::Saving {
        match project_save_progress(world) {
            ProjectSaveProgress::Running { current } => {
                let mut state = world.resource_mut::<NativeProjectCloseState>();
                state.status = format!("正在保存：{current}。全部成功后{}，可取消后续保存。", state.target().operation());
                return false;
            }
            ProjectSaveProgress::Failed { detail } => {
                let mut state = world.resource_mut::<NativeProjectCloseState>();
                state.failed = Some(detail);
                state.intent = None;
                state.stage = CloseStage::Confirm;
                state.focus_cancel = true;
            }
            ProjectSaveProgress::Succeeded => {
                world.resource_mut::<NativeProjectCloseState>().stage = CloseStage::Inspecting;
            }
            ProjectSaveProgress::Idle => {
                let mut state = world.resource_mut::<NativeProjectCloseState>();
                state.failed = Some("保存事务未完成，原文档保持打开，请重试或返回编辑".into());
                state.intent = None;
                state.stage = CloseStage::Confirm;
            }
        }
    }
    let snapshot = project_close_snapshot(world);
    let save_progress = project_save_progress(world);
    let mut state = world.resource_mut::<NativeProjectCloseState>();
    state.documents = snapshot.unsaved;
    if let Some(choice @ (CloseAction::Save | CloseAction::Discard)) = choice {
        state.intent = Some(choice);
        state.failed = None;
    }
    if choice == Some(CloseAction::Close) && state.stage == CloseStage::Confirm {
        // 干净关闭独立于“放弃”：点击与采用之间若新增草稿或出现保存失败，必须重新选择。
        state.intent = (!state.needs_save_decision()).then_some(CloseAction::Close);
        if state.intent.is_none() {
            state.focus_cancel = true;
        }
    }
    // 新修改出现时当帧撤回旧授权，不能等原任务完成后因脏位再次清零而恢复这笔意图。
    if state.intent == Some(CloseAction::Close) && state.needs_save_decision() {
        state.intent = None;
        state.focus_cancel = true;
    }
    if let Some(reason) = snapshot.blocked {
        state.stage = CloseStage::Blocked;
        state.status = format!("{reason}。请取消本次操作，回到原控件完成或取消当前操作。");
        return false;
    }
    if let Some(reason) = snapshot.pending {
        state.stage = CloseStage::Waiting;
        state.status = format!("{reason}。完成后继续检查{}，可取消。", state.target().operation());
        return false;
    }
    // 原Tasks可先清除dirty/pending，唯一保存队列到Last才发布正式采用回执。
    // 取消后重关、放弃及宿主收口等待仍须重检，不能在这一个Update窗口退出或drop原写者。
    if let ProjectSaveProgress::Running { current } = save_progress {
        state.stage = CloseStage::Waiting;
        state.status = format!("正在等待原保存队列收口：{current}。收到完整回执后继续{}，可取消。", state.target().operation());
        return false;
    }
    // 弹出确认层只是在请求用户选择；文档为空、后台保存结束都不是关闭授权。
    // 保存关闭必须同时保留本次明确意图与原队列成功回执，不能把旧成功沿用到下一次关闭。
    if state.intent == Some(CloseAction::Discard)
        || state.intent == Some(CloseAction::Close) && !state.needs_save_decision()
        || state.intent == Some(CloseAction::Save)
            && matches!(save_progress, ProjectSaveProgress::Succeeded)
            && state.documents.is_empty()
            && state.failed.is_none()
    {
        state.stage = CloseStage::Draining;
        state.status = format!("正在结束本次工程运行和构建，确认全部结束后{}…", state.target().operation());
        return true;
    }
    if state.intent == Some(CloseAction::Save) {
        drop(state);
        let result = request_save_all(world);
        let mut state = world.resource_mut::<NativeProjectCloseState>();
        match result {
            Ok(()) => {
                state.stage = CloseStage::Saving;
                state.status = "正在按原作者流程保存全部修改…".into();
            }
            Err(error) => {
                state.failed = Some(error);
                state.intent = None;
                state.stage = CloseStage::Confirm;
            }
        }
        return false;
    }
    state.stage = CloseStage::Confirm;
    state.status = state.failed.as_ref().map_or_else(
        || state.target().confirmation(state.documents.len()),
        |error| format!("保存未完成：{error}。当前工程、未保存内容和历史保留。"),
    );
    false
}
