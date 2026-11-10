//! 显式导出验收的有限状态与单飞任务槽；不进入普通产品构建。

use super::super::types::ExportReceipt;
use bevy::{prelude::*, tasks::Task};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::{Arc, Mutex}, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase { OpenScene, WaitScene, WaitDraft, OpenMenu, OpenExport, CaptureDialog, Start, Exporting, CaptureSuccess, Custom(CustomPhase), CloseDialog, Report, Closing, Finished }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CustomPhase {
    FillDestination, WaitDestination, CaptureDestination, Start, Exporting, CaptureCreated,
    BeforeCancel, StartUpdate, WaitWorker, Cancelling, AfterCancel,
    ReopenMenu, ReopenExport, CaptureCancelled, StartRetry, Retrying, VerifyRetry, CaptureRetry,
}

/// 增量场景只有一个包核验任务槽，完整文件内容始终留在后台，不向作者帧复制字节。
pub(super) struct CustomState {
    pub destination: PathBuf,
    pub io: Option<Task<Result<PackageCheckpoint, String>>>,
    pub baseline: Option<PackageCheckpoint>,
    pub cancel_control: Option<crate::build_process::BuildControl>,
    pub started: bool,
    pub completed: bool,
}

pub(super) struct PackageCheckpoint {
    pub inventory: Arc<super::package_evidence::Inventory>,
    pub marker: Value,
    pub proof: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scenario { Portable, CustomCancelRetry }

impl Scenario {
    pub(super) fn from_environment() -> Result<Self, String> {
        match std::env::var_os("FEATHRISE_EXPORT_VALIDATION_SCENARIO") {
            None => Ok(Self::Portable),
            Some(value) => Self::parse(&value.to_string_lossy()),
        }
    }
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "portable" => Ok(Self::Portable),
            "custom-cancel-retry" => Ok(Self::CustomCancelRetry),
            _ => Err(format!("未知工程导出验收场景：{value}；仅支持portable或custom-cancel-retry")),
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self { Self::Portable => "portable", Self::CustomCancelRetry => "custom-cancel-retry" }
    }
}

#[derive(Resource)]
pub(super) struct Plan {
    pub(super) project: crate::project::Project,
    pub(super) report: PathBuf,
    pub(super) phase: Phase,
    pub(super) changed: Instant,
    pub(super) frames: u64,
    pub(super) pulse: Option<Entity>,
    pub(super) baseline: Option<Value>,
    pub(super) receipt: Option<ExportReceipt>,
    pub(super) proofs: Vec<Value>,
    pub(super) error: Option<String>,
    pub(super) screenshot: Option<async_channel::Receiver<Result<Value, String>>>,
    pub(super) writer: Option<Task<Result<(), String>>>,
    pub(super) completion: Completion,
    pub(super) scenario: Scenario,
    pub(super) custom: Option<CustomState>,
}

impl Plan {
    pub(super) fn advance(&mut self, phase: Phase) {
        self.phase = phase;
        self.changed = Instant::now();
        self.frames = 0;
        if let Ok(mut state) = self.completion.state.try_lock() {
            state["phase"] = json!(format!("{phase:?}"));
            state["stages"] = json!(self.proofs);
        }
        println!("FEATHRISE_EXPORT_VALIDATION_STAGE {phase:?}");
    }
}

/// 在World之外保留有限阶段证据；窗口提前结束不能因正常AppExit而被当成完整验收。
#[derive(Clone)]
pub(crate) struct Completion {
    project_id: String,
    project_root: PathBuf,
    report: PathBuf,
    pub(super) state: Arc<Mutex<Value>>,
}

impl Completion {
    pub(super) fn new(project: crate::project::Project, report: PathBuf) -> Self {
        Self { project_id: project.id, project_root: project.root, report,
            state: Arc::new(Mutex::new(json!({"phase":"OpenScene","stages":[],"events":[]}))) }
    }

    /// 已离开App::run和帧关键路径，才允许同步核对或独占写入中断报告；绝不补写成功。
    pub(crate) fn finish(&self, exit: &AppExit) -> Result<(), String> {
        let state = self.state.lock().map_err(|_|"导出验收完成状态损坏")?.clone();
        if !self.report.try_exists().map_err(|error|error.to_string())? {
            let reason = format!("工程导出验收提前结束，最后阶段：{}；AppExit={exit:?}；最后进度：{}；退出来源尚未确定",
                state["phase"], state["status"]);
            let interrupted = json!({"schema":1,"scope":"owned-project-folder-export-editor","passed":false,
                "interrupted":true,"error":reason,"project_id":self.project_id,"project_root":self.project_root,
                "receipt":null,"stages":state["stages"],"lifecycle":state,"app_run_returned":true,
                "app_exit":format!("{exit:?}"),"os_input_verified":false});
            super::write_new(&self.report, &serde_json::to_vec_pretty(&interrupted).map_err(|error|error.to_string())?)?;
            return Err(reason);
        }
        let report = super::read_json(&self.report, 1024 * 1024)?;
        if *exit != AppExit::Success || report["passed"] != true || state["phase"] != "Closing" {
            return Err(format!("工程导出验收未完整结束：阶段={}，AppExit={exit:?}，原因={}", state["phase"], report["error"]));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_scenario_never_falls_back_to_portable() {
        assert_eq!(Scenario::parse("portable").unwrap(), Scenario::Portable);
        assert_eq!(Scenario::parse("custom-cancel-retry").unwrap(), Scenario::CustomCancelRetry);
        assert!(Scenario::parse("custom-cancel-rettry").is_err());
    }

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> (Self, Completion) {
            let root = std::env::temp_dir().join(format!("export-completion-{}", bevy::asset::uuid::Uuid::new_v4()));
            std::fs::create_dir(&root).unwrap();
            let completion = Completion { project_id:"owned-test".into(), project_root:root.clone(),
                report:root.join("editor-report.json"), state:Arc::new(Mutex::new(json!({"phase":"Exporting",
                    "status":"正在构建Debug工程…","stages":[{"stage":"original_start_export_pressed"}],"events":[]}))) };
            (Self(root), completion)
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

    #[test]
    fn successful_app_exit_without_finished_driver_records_interruption() {
        let (_fixture, completion) = Fixture::new();
        assert!(completion.finish(&AppExit::Success).unwrap_err().contains("Exporting"));
        let report = super::super::read_json(&completion.report, 1024 * 1024).unwrap();
        assert_eq!(report["passed"], false);
        assert_eq!(report["interrupted"], true);
        assert_eq!(report["lifecycle"]["phase"], "Exporting");
        assert_eq!(report["stages"][0]["stage"], "original_start_export_pressed");
    }

    #[test]
    fn existing_failure_report_is_preserved_after_normal_app_exit() {
        let (_fixture, completion) = Fixture::new();
        let bytes = br#"{"passed":false,"error":"real worker failed"}"#;
        std::fs::write(&completion.report, bytes).unwrap();
        completion.state.lock().unwrap()["phase"] = json!("Closing");
        assert!(completion.finish(&AppExit::Success).unwrap_err().contains("real worker failed"));
        assert_eq!(std::fs::read(&completion.report).unwrap(), bytes);
    }
}
