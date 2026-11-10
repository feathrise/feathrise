//! 停止编译验收的有限阶段、单飞IO槽与离开App后仍可核对的完成状态。

use super::{REPORT_FILE, drag::CargoDrag, types::BuildWitness};
use bevy::{prelude::*, tasks::Task};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::{Arc, Mutex}, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Open, Scene, Draft, Build, CaptureBuild, Stop, Drain, CaptureStopped, StableStopped, Local, WaitLocal,
    StableLocal, Undo, Redo, RedoSettle, CaptureLocal, Release, Retry, StartingRetry, Guest, CaptureRetry,
    ReturnLocal, DrainRetry, FailureDrain, Write, Closing, Finished,
}

#[derive(Resource)]
pub(super) struct Plan {
    pub project: crate::project::Project,
    pub directory: PathBuf,
    pub target: PathBuf,
    pub nonce: String,
    pub phase: Phase,
    pub changed: Instant,
    pub started: Instant,
    pub frames: u64,
    pub pulse: Option<Entity>,
    pub initial: Option<Value>,
    pub author: Option<Value>,
    pub presentation: Option<Value>,
    /// 历史先采用作者记录，场景树签名随后清理选择；只在完整呈现连续稳定后冻结新基线。
    pub history_settlement: Option<HistorySettlement>,
    pub original_session: Option<u64>,
    pub control: Option<crate::build_process::BuildControl>,
    pub witness: Option<BuildWitness>,
    pub cargo_drag: Option<CargoDrag>,
    pub cargo_drag_proof: Option<Value>,
    /// 只取消本轮取证等待，不触碰生产构建控制牌；失败与提前退出不遗留长等待任务。
    pub witness_control: crate::build_process::BuildControl,
    pub build_io: Option<Task<Result<BuildWitness, String>>>,
    pub io: Option<Task<Result<Option<Value>, String>>>,
    pub screenshot: Option<async_channel::Receiver<Result<Value, String>>>,
    pub writer: Option<Task<Result<(), String>>>,
    pub steps: Vec<Value>,
    pub error: Option<String>,
    pub completion: Completion,
}

pub(super) struct HistorySettlement {
    pub presentation: Value,
    pub stable_since: Instant,
    pub stable_frames: u64,
    pub transitions: Vec<Value>,
}

impl Drop for Plan {
    fn drop(&mut self) { self.witness_control.cancel(); }
}

impl Plan {
    pub fn advance(&mut self, phase: Phase) {
        self.phase = phase;
        self.changed = Instant::now();
        self.frames = 0;
        if let Ok(mut state) = self.completion.state.try_lock() {
            *state = json!({"phase":format!("{phase:?}"),"steps":self.steps,"error":self.error});
        }
        println!("FEATHRISE_EMBEDDED_BUILD_CANCEL_PHASE {phase:?}");
    }
}

#[derive(Clone)]
pub(crate) struct Completion {
    pub(super) directory: PathBuf,
    pub(super) state: Arc<Mutex<Value>>,
}

impl Completion {
    pub(super) fn new(directory: PathBuf) -> Self {
        Self { directory, state:Arc::new(Mutex::new(json!({"phase":"Open","steps":[]}))) }
    }

    /// App::run提前返回只能记录失败；不得因正常退出码而补造停止/重试成功证据。
    pub(crate) fn finish(&self, exit: &AppExit) -> Result<(), String> {
        let state = self.state.lock().map_err(|_|"停止编译验收完成状态损坏")?.clone();
        let path = self.directory.join(REPORT_FILE);
        if !path.try_exists().map_err(|error|error.to_string())? {
            let error = format!("停止编译验收提前退出：phase={}，AppExit={exit:?}", state["phase"]);
            super::fixture::write_new(&path, &json!({"schema":1,"scope":super::PURPOSE,
                "passed":false,"interrupted":true,"error":error,"lifecycle":state}))?;
            return Err(error);
        }
        let report = super::fixture::read_json(&path)?;
        if *exit != AppExit::Success || report["passed"] != true || state["phase"] != "Closing" {
            return Err(format!("停止编译验收未完整结束：phase={}，AppExit={exit:?}，error={}", state["phase"], report["error"]));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> (Self, Completion) {
            let root = std::env::temp_dir().join(format!("embedded-build-cancel-completion-{}", bevy::asset::uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            let completion = Completion::new(root.clone());
            (Self(root), completion)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0.join(REPORT_FILE));
            let _ = std::fs::remove_dir(&self.0);
        }
    }

    #[test]
    fn successful_app_exit_before_drain_and_retry_finished_records_only_failure() {
        let (_fixture, completion) = Fixture::new();
        *completion.state.lock().unwrap() = json!({"phase":"Drain","steps":[{"stage":"original_stop_compilation_pressed"}]});
        assert!(completion.finish(&AppExit::Success).unwrap_err().contains("Drain"));
        let report = super::super::fixture::read_json(&completion.directory.join(REPORT_FILE)).unwrap();
        assert_eq!(report["passed"], false);
        assert_eq!(report["interrupted"], true);
        assert_eq!(report["lifecycle"]["steps"][0]["stage"], "original_stop_compilation_pressed");
    }

    #[test]
    fn report_pass_flag_requires_driver_closing_and_real_successful_exit() {
        let (_fixture, completion) = Fixture::new();
        super::super::fixture::write_new(&completion.directory.join(REPORT_FILE), &json!({"passed":true})).unwrap();
        assert!(completion.finish(&AppExit::Success).is_err());
        completion.state.lock().unwrap()["phase"] = json!("Closing");
        assert!(completion.finish(&AppExit::error()).is_err());
        assert!(completion.finish(&AppExit::Success).is_ok());
    }
}
