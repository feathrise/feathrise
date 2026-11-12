//! 受检 Blender 子进程。用独占目录传参、捕获有界诊断，并跟随当前绑骨请求取消。

use crate::files::{self, Stage};
use feathrise_editor_glb_model::rigging::BlenderLocator;
use serde::{Serialize, de::DeserializeOwned};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const WORKER: &str = include_str!("自动绑骨.py");
const PLUGIN: &str = "Blender保存时自动导出GLB.py";
const TIMEOUT: Duration = Duration::from_secs(180);
const MAX_REPORT: u64 = 64 * 1024;
const LOG_TAIL: u64 = 4096;

pub(super) fn export_plugin() -> Result<PathBuf, String> {
    let root = std::env::var_os("FEATHRISE_RIG_BLENDER_TOOLS_DIR")
        .or_else(|| std::env::var_os("FEATHRISE_BLENDER_TOOLS_DIR"))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("请将 FEATHRISE_BLENDER_TOOLS_DIR 设为包含 Blender 保存导出插件的绝对目录")?;
    let path = root.join(PLUGIN);
    if !files::ordinary(&path).is_ok_and(|metadata| metadata.is_file() && metadata.len() > 0) {
        return Err(format!("指定的工具目录缺少 {PLUGIN}"));
    }
    Ok(path)
}

pub(super) fn execute<T: DeserializeOwned>(
    stage: &Stage,
    request: &impl Serialize,
    report: &Path,
    exporting: bool,
    locate_blender: BlenderLocator,
    cancel: &AtomicBool,
) -> Result<T, String> {
    files::active(cancel)?;
    let blender = locate_blender()?
        .canonicalize()
        .map_err(|error| format!("无法定位 Blender：{error}"))?;
    let script = stage.path("worker.py");
    let request_path = stage.path("request.json");
    let stdout = stage.path("stdout.log");
    let stderr = stage.path("stderr.log");
    fs::write(&script, WORKER).map_err(|error| format!("暂存绑骨脚本失败：{error}"))?;
    let request_bytes = serde_json::to_vec(request).map_err(|error| error.to_string())?;
    fs::write(&request_path, request_bytes)
        .map_err(|error| format!("暂存绑骨请求失败：{error}"))?;
    let mut command = Command::new(blender);
    // 工程资产只作为数据导入，用户启动文件、Python 自动执行与全局 Blender 设置不参与。
    command
        .args([
            "--background",
            "--factory-startup",
            "--disable-autoexec",
            "--offline-mode",
            "--threads",
            "4",
        ])
        .args(["--python-exit-code", "13", "--python"])
        .arg(&script)
        .arg("--")
        .arg(&request_path)
        .current_dir(stage.root())
        .env("BLENDER_USER_RESOURCES", stage.path("blender-user"))
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(create_log(&stdout)?))
        .stderr(Stdio::from(create_log(&stderr)?));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW，作者留在现有编辑器窗口。
    }
    files::active(cancel)?;
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动 Blender 绑骨失败：{error}"))?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                return Err(terminate(
                    &mut child,
                    &format!("读取 Blender 状态失败：{error}"),
                ));
            }
        }
        if cancel.load(Ordering::Acquire) {
            return Err(terminate(&mut child, "已取消人形绑骨任务"));
        }
        if started.elapsed() >= TIMEOUT {
            return Err(terminate(&mut child, "Blender 绑骨超过 180 秒"));
        }
        std::thread::sleep(Duration::from_millis(40));
    };
    files::active(cancel)?;
    if !status.success() {
        return Err(format!(
            "Blender 人形绑骨失败（{status}）\n{}\n{}",
            log_tail(&stderr),
            log_tail(&stdout)
        ));
    }
    if exporting && !log_tail(&stdout).contains("[Bevy GLB 自动导出] 已更新") {
        return Err(format!(
            "Blender 未确认通过保存插件导出 GLB\n{}",
            log_tail(&stdout)
        ));
    }
    if !files::ordinary(report)
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_REPORT)
    {
        return Err("Blender 没有返回有效的绑骨回执".into());
    }
    let mut bytes = Vec::new();
    File::open(report)
        .and_then(|file| file.take(MAX_REPORT + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("读取 Blender 回执失败：{error}"))?;
    if bytes.len() as u64 > MAX_REPORT {
        return Err("Blender 回执超出读取预算".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("解析 Blender 回执失败：{error}"))
}

fn create_log(path: &Path) -> Result<File, String> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("创建绑骨日志失败：{error}"))
}

fn log_tail(path: &Path) -> String {
    let read = || {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        file.seek(SeekFrom::Start(length.saturating_sub(LOG_TAIL)))?;
        let mut bytes = Vec::new();
        file.take(LOG_TAIL).read_to_end(&mut bytes)?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&bytes).into_owned())
    };
    read().unwrap_or_default()
}

fn terminate(child: &mut Child, reason: &str) -> String {
    // 只终止和回收本任务拥有的子进程，并且一直占用当前工作槽。
    if let Err(error) = child.kill()
        && !matches!(child.try_wait(), Ok(Some(_)))
    {
        return format!("{reason}，终止 Blender 失败：{error}");
    }
    if let Err(error) = child.wait() {
        return format!("{reason}，回收 Blender 失败：{error}");
    }
    reason.to_owned()
}
