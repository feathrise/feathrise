//! 主线程只合并意图和非阻塞采用回执；路径检查及进程启动全部在 IO 线程执行。

use super::{discovery, launcher, messages::OpenExternalFile, resources::{ExternalEditors, ExternalFileState}, types::ExternalTaskReply};
use crate::resources::MapEditorState;
use bevy::{prelude::*, tasks::{IoTaskPool, futures::check_ready}};

pub(super) fn drive(
    mut requests: MessageReader<OpenExternalFile>,
    mut project_requests: MessageReader<super::messages::OpenExternalProjectFile>,
    mut state: ResMut<ExternalFileState>,
    mut editor: ResMut<MapEditorState>,
    mut available: ResMut<ExternalEditors>,
) {
    if state.was_open != editor.open {
        state.scan_requested |= editor.open; // 首次启动和重新打开F12各合并一次；稳定帧只读缓存。
        state.was_open = editor.open;
    }
    if let Some(request) = requests.read().last() {
        state.pending = Some(request.clone());
        state.pending_project = None;
    }
    if let Some(request) = project_requests.read().last() {
        state.pending_project = Some(request.clone());
        state.pending = None;
    }
    if let Some(result) = state.task.as_mut().and_then(check_ready) {
        state.task = None;
        match result {
            ExternalTaskReply::Detected(detected) => {
                if available.detected != detected { available.detected = detected; }
            }
            ExternalTaskReply::Opened { result, detected } => {
                if available.detected != detected { available.detected = detected; }
                editor.status = result.unwrap_or_else(|error| error);
            }
        }
    }
    if state.task.is_none() && state.scan_requested {
        state.scan_requested = false;
        state.task = Some(IoTaskPool::get().spawn(async { ExternalTaskReply::Detected(discovery::discover()) }));
    } else if state.task.is_none() && let Some(request) = state.pending_project.take() {
        let mut launchers = available.detected.clone();
        editor.status = "正在用外部IDE打开工程文件…".into();
        state.task = Some(IoTaskPool::get().spawn(async move {
            let result = launcher::open_project_file(&request.root, &request.request, &mut launchers);
            ExternalTaskReply::Opened { result, detected: launchers }
        }));
    } else if state.task.is_none() && let Some(request) = state.pending.take() {
        let root = state.root.clone();
        let mut launchers = available.detected.clone(); // 固定三份短路径快照；检测与启动共用原单槽。
        editor.status = "正在打开外部文件…".into();
        state.task = Some(IoTaskPool::get().spawn(async move {
            let result = launcher::open(&root, &request, &mut launchers);
            ExternalTaskReply::Opened { result, detected: launchers }
        }));
    }
}
