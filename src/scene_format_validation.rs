//! 新场景格式实窗专项。只在独占空模板工程内安装原作者操作的驱动。

mod driver;
mod fixture;

pub(crate) use fixture::run;

use bevy::prelude::*;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const ENV: &str = "FEATHRISE_SCENE_FORMAT_VALIDATION";
const OWNER: &str = ".scene-format-validation-owner.json";
const PURPOSE: &str = "owned-new-scene-format";
const PARENT: &str = "验收父场景";
const CHILD: &str = "验收子场景";
const WINDOW_REPORT: &str = "window.json";
const NATIVE_TIMEOUT: &str = "native-handoff-timeout.json";

pub(crate) struct OwnedFixture {
    directory: PathBuf,
    saved: Option<fixture::ScenePair>,
}

/// 必须早于作者偏好与插件库读取，避免错误环境把验收操作带入个人状态。
pub(crate) fn preflight(project: &crate::project::Project) -> Result<Option<OwnedFixture>, String> {
    let Some(mode) = std::env::var_os(ENV) else {
        return Ok(None);
    };
    let mode = mode.to_str().ok_or("场景格式验收阶段无效")?;
    let work = project.root.parent().ok_or("验收工程缺少独占工作根")?;
    let owner = read_json(&project.root.join(OWNER))?;
    if owner
        != json!({"schema":1,"purpose":PURPOSE,"project_id":project.id,"project_root":project.root,"work":work})
        || project.root != work.join("project")
    {
        return Err("场景格式验收工程归属与本轮空模板夹具不一致".into());
    }
    let directory = match mode {
        "author" | "reopen" => work.join(mode),
        _ => return Err("场景格式验收阶段无效".into()),
    };
    if directory
        .canonicalize()
        .map_err(|error| error.to_string())?
        != directory
        || directory.join(WINDOW_REPORT).exists()
    {
        return Err("场景格式验收输出目录已使用或离开本轮夹具".into());
    }
    for (variable, child) in [
        ("FEATHRISE_PREFERENCES_FILE", "isolated-preferences.json"),
        (
            "FEATHRISE_VALIDATION_GLOBAL_PLUGINS",
            "isolated-global-plugins",
        ),
    ] {
        if std::env::var_os(variable).as_deref() != Some(directory.join(child).as_os_str()) {
            return Err(format!("场景格式验收缺少本轮目录内的隔离状态 {variable}"));
        }
    }
    let saved = if mode == "reopen" {
        Some(fixture::verify_saved(project)?.0)
    } else {
        fixture::verify_empty(project)?;
        None
    };
    Ok(Some(OwnedFixture { directory, saved }))
}

impl OwnedFixture {
    pub(crate) fn install(self, app: &mut App) {
        app.insert_resource(bevy::winit::WinitSettings::continuous());
        app.world_mut()
            .resource_mut::<feathrise_editor_core::EditorSettings>()
            .ui_locale = feathrise_editor_core::EditorLocalePreference::Chinese;
        feathrise_editor_scene::standalone::scene_format_validation::install(app);
        driver::install(app, self.directory, self.saved);
    }
}

/// 原作者入口已通过 preflight 归属校验；只为本轮重开过程旁录 SDK 自己的超时回读。
pub(crate) fn preview_handoff_report(project: &crate::project::Project) -> Option<PathBuf> {
    if std::env::var(ENV).as_deref() != Ok("reopen") {
        return None;
    }
    Some(project.root.parent()?.join("reopen").join(NATIVE_TIMEOUT))
}

/// 后台 stderr 读取保留完整有界事件，避免终端每行截断隐藏原生交接阶段。
pub(crate) fn record_preview_handoff(path: &Path, stderr_tail: &[u8], pid: u32) -> bool {
    let report = stderr_tail.rsplit(|byte| *byte == b'\n').find_map(|line| {
        let payload = line.strip_prefix(b"FEATHRISE_NATIVE_HANDOFF_TIMEOUT ")?;
        let report: Value = serde_json::from_slice(payload).ok()?;
        (report["source"] == "native-handoff-timeout-readback"
            && report["not_success"] == true
            && report["pid"] == pid
            && report["stage"].is_string())
        .then_some(report)
    });
    let Some(report) = report else { return false };
    if let Err(error) = write_json_new(path, &report) {
        eprintln!("保存原生窗口交接诊断失败：{error}");
    }
    true
}

fn read_json(path: &Path) -> Result<Value, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("读取{}失败：{error}", path.display()))?;
    if bytes.len() > 1024 * 1024 {
        return Err(format!("验收JSON超出有界输入：{}", path.display()));
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("解析{}失败：{error}", path.display()))
}

fn write_json_new(path: &Path, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    write_new(path, &bytes)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| format!("无法独占写入验收证据{}：{error}", path.display()))
}

fn create_dir(path: &Path) -> Result<(), String> {
    fs::create_dir(path).map_err(|error| format!("无法独占创建验收目录{}：{error}", path.display()))
}
