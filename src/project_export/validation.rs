//! 工程导出专项验收：只在显式validation和本轮独占夹具中驱动原菜单、真实后台组包与独立运行。

mod driver;
mod evidence;
mod fixture;
mod package_evidence;
mod resources;
mod standalone;
mod custom;

pub(crate) use fixture::{run, run_owned_project};

use self::resources::{Phase, Plan, Scenario};
pub(crate) use self::resources::Completion;
use bevy::prelude::*;
use serde_json::{Value, json};
use std::{io::{Read, Write}, path::{Path, PathBuf}, time::Instant};

const OWNER_FILE: &str = ".export-validation-owner.json";
const PURPOSE: &str = "owned-project-folder-export";

pub(crate) fn install(app: &mut App, project: &crate::project::Project, _target: &Path) -> Result<Option<Completion>, String> {
    let Some(report) = std::env::var_os("FEATHRISE_EXPORT_VALIDATION_REPORT").map(PathBuf::from) else { return Ok(None); };
    if !report.is_absolute() || report.exists() { return Err("工程导出验收要求本轮未存在的绝对报告路径".into()); }
    if crate::embedded_viewport::validation::code_iteration_owner(&project.root)? != *project {
        return Err("工程导出验收的中性工程来源不一致".into());
    }
    let owner = read_json(&project.root.join(OWNER_FILE), 16 * 1024)?;
    let scenario = Scenario::from_environment()?;
    if owner["schema"] != 1 || owner["purpose"] != PURPOSE || owner["project_id"] != project.id
        || owner["project_root"] != json!(project.root) || owner["report"] != json!(report)
        || owner["scenario"] != scenario.label()
    { return Err("导出验收归属与当前工程或报告不一致，未安装自动操作".into()); }
    // 仅显式自建工程允许内部按钮驱动；保持真实窗口连续更新，不要求OS恰好把焦点交给它。
    app.insert_resource(bevy::winit::WinitSettings::continuous());
    feathrise_editor_map::standalone::embedded_validation::install(app);
    let completion = Completion::new(project.clone(), report.clone());
    app.insert_resource(Plan {
        project: project.clone(), report, phase: Phase::OpenMap, changed: Instant::now(), frames: 0,
        pulse: None, baseline: None, receipt: None, proofs: Vec::new(), error: None,
        screenshot: None, writer: None, completion: completion.clone(), scenario,
        custom: (scenario == Scenario::CustomCancelRetry).then(|| custom::State::new(&project.root)),
    }).add_systems(Last, (driver::drive, driver::observe_lifecycle).chain());
    Ok(Some(completion))
}

fn read_json(path: &Path, limit: usize) -> Result<Value, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path).map_err(|error| format!("验收证据不可读{}：{error}", path.display()))?
        .take(limit as u64 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > limit { return Err(format!("验收JSON超过{limit}字节预算")); }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| format!("本轮证据不能独占写入{}：{error}", path.display()))
}
