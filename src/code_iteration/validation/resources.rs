//! 代码迭代专项的有限阶段与证据句柄；任务单飞，作者数据仍由原生产资源持有。

use bevy::{ecs::message::MessageCursor, prelude::*, tasks::Task};
use serde_json::Value;
use std::{path::PathBuf, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Open, Local, Embedded, FirstAdded, SecondAdded, Undo, NoteOpen, NoteEdit,
    CloseConfirm, CloseDrain, CloseCancelled, CloseRecovered, CloseReconnectLocal, CloseMapCapture, CloseReturnNote,
    FunctionHotpatch,
    ControllerOpen, InvalidBuffer, SaveInvalid, CompileFailed,
    Diagnostics, Located, FixedBuffer, SaveFixed, CandidateReady, ReturnNote, ReturnMap,
    BeforeCapture, Applying, AfterCapture, Report, Closing, Finished,
}

#[derive(Resource)]
pub(super) struct Plan {
    pub project: crate::project::Project,
    pub report: PathBuf,
    pub target: PathBuf,
    pub directory: PathBuf,
    pub started: Instant,
    pub changed: Instant,
    pub source_wait: Instant,
    pub phase: Phase,
    pub frames: u64,
    pub pulse: Option<Entity>,
    pub steps: Vec<Value>,
    pub error: Option<String>,
    pub initial: Option<Value>,
    pub baseline: Option<Value>,
    pub baseline_stable_frames: u8,
    pub note_baseline: Option<Value>,
    pub close_cancel_evidence: Option<Value>,
    pub function_hotpatch: bool,
    pub hotpatch_check: Option<Box<super::function_hotpatch::Check>>,
    pub function_hotpatch_evidence: Option<Value>,
    pub source_target: Option<String>,
    pub first_id: Option<String>,
    pub old_pid: u32,
    pub old_world: u64,
    pub old_frame: u64,
    pub invalid_generation: u64,
    pub apply_generation: u64,
    pub apply_requests: MessageCursor<crate::code_iteration::RequestApplyCode>,
    pub apply_request_generation: Option<u64>,
    pub apply_button_evidence: Option<Value>,
    pub apply_accepted: bool,
    pub diagnostic_index: Option<usize>,
    pub diagnostic_location: Option<Value>,
    pub evidence_task: Option<Task<Result<Option<Value>, String>>>,
    pub evidence_key: Option<(u32, u64, u64)>,
    pub evidence: Option<Value>,
    pub retry: Instant,
    pub screenshot: Option<async_channel::Receiver<Result<(), String>>>,
    pub screenshot_name: Option<String>,
    pub completion: Option<Task<Result<(), String>>>,
}

impl Plan {
    pub fn new(project: crate::project::Project, report: PathBuf, target: PathBuf) -> Self {
        Self { directory: report.parent().unwrap().to_path_buf(), project, report, target,
            started: Instant::now(), changed: Instant::now(), source_wait: Instant::now(), phase: Phase::Open, frames: 0,
            pulse: None, steps: Vec::new(), error: None, initial: None, baseline: None, baseline_stable_frames: 0,
            note_baseline: None, close_cancel_evidence: None,
            function_hotpatch: std::env::var_os("FEATHRISE_CODE_ITERATION_FUNCTION_HOTPATCH").is_some_and(|value| value == "1"),
            hotpatch_check: None, function_hotpatch_evidence: None,
            source_target: None, first_id: None, old_pid: 0, old_world: 0,
            old_frame: 0, invalid_generation: 0, apply_generation: 0,
            apply_requests: MessageCursor::default(), apply_request_generation: None, apply_button_evidence: None,
            apply_accepted: false, diagnostic_index: None,
            diagnostic_location: None, evidence_task: None, evidence_key: None, evidence: None,
            retry: Instant::now(), screenshot: None, screenshot_name: None, completion: None }
    }

    pub fn advance(&mut self, phase: Phase) {
        self.phase = phase;
        self.changed = Instant::now();
        self.source_wait = self.changed;
        println!("FEATHRISE_CODE_ITERATION_VALIDATION_PHASE {:?}", phase);
    }
}
