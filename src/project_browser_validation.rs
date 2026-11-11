//! 工程目录真实窗口专项入口。父进程独占新工程与证据目录，子进程只在通过归属核验后安装驱动。

mod fixture;

use crate::{build_process::{self, BuildControl}, project::Project};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const ENV: &str = "FEATHRISE_PROJECT_BROWSER_VALIDATION";
const OWNER: &str = ".project-browser-owner.json";
const PURPOSE: &str = "owned-project-browser-validation";

pub(crate) fn run(target: PathBuf, report: PathBuf) -> Result<(), String> {
    if !report.is_absolute() || report.exists() {
        return Err("工程目录验收报告须使用本轮新的绝对路径".into());
    }
    fs::create_dir_all(report.parent().ok_or("验收报告缺少父目录")?)
        .map_err(|error| error.to_string())?;
    let work = report.with_extension("work");
    fs::create_dir(&work).map_err(|error| format!("验收工作目录必须为本轮独占：{error}"))?;
    let work = work.canonicalize().map_err(|error| error.to_string())?;
    let mut proof = json!({"schema":1,"purpose":PURPOSE,"work":work,"os_input_verified":false});
    let result = exercise(&target, &work, &mut proof);
    proof["passed"] = json!(result.is_ok());
    proof["error"] = json!(result.as_ref().err());
    write_new(&report, &serde_json::to_vec_pretty(&proof).map_err(|error| error.to_string())?)?;
    result
}

fn exercise(target: &Path, work: &Path, proof: &mut Value) -> Result<(), String> {
    let root = crate::project_template::create_with_control(
        &work.join("project"), "工程目录验收", &crate::project_template_root(), &BuildControl::default(),
    )?;
    let project = crate::project::open(&root)?;
    if project.root != work.join("project") || project.context.asset_roots != [root.join("assets")] {
        return Err("正式空工程路径或资产根与本轮夹具不一致".into());
    }
    let assets = root.join("assets");
    let original = fixture::prepare(&assets)?;
    write_new(&work.join(OWNER), &serde_json::to_vec(&owner(&project, work)).map_err(|error| error.to_string())?)?;
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    // 只清除相互排斥的验证入口，保留当前设备、工具链、SDK 与缓存配置。
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.contains("_VALIDATION") || name.contains("_SMOKE_") || name.starts_with("FEATHRISE_WORKSPACE_") {
            command.env_remove(key);
        }
    }
    command.args(["--native-editor", "--project"]).arg(&root)
        .arg("--target-dir").arg(target).env(ENV, work);
    crate::native_validation::isolate_author_command(&mut command, work)?;
    let child = build_process::run(&command, &BuildControl::default(), Some(Duration::from_secs(150)), tail)?;
    write_new(&work.join("process.log"), format!("{}\n{}", child.stdout, child.stderr).as_bytes())?;
    proof["exit_code"] = json!(child.exit_code);
    let window = read_json(&work.join("window.json"))?;
    proof["window"] = window;
    if child.exit_code != 0 || proof["window"]["passed"] != true {
        return Err(format!("真实编辑器退出码 {}：{}", child.exit_code, proof["window"]["error"]));
    }
    let final_assets = fixture::fingerprints(&assets)?;
    for (name, initial) in &original {
        let expected = if name == fixture::MOTIONS { &original[fixture::STATIC] } else { initial };
        if final_assets.get(name) != Some(expected) {
            return Err(format!("夹具资产 {name} 的最终摘要不符"));
        }
    }
    proof["before_sha256"] = json!(original);
    proof["after_sha256"] = json!(final_assets);
    for name in ["skeleton.png", "narrow-menu.png", "compact-browser.png", "clip-menu.png", "second-clip.png", "replaced.png"] {
        let capture = &proof["window"]["screenshots"][name];
        let bytes = fs::read(work.join(name)).map_err(|error| error.to_string())?;
        let header = bytes.get(..24).ok_or("GPU 截图缺少 PNG 文件头")?;
        if &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR"
            || capture["width"] != u32::from_be_bytes(header[16..20].try_into().unwrap())
            || capture["height"] != u32::from_be_bytes(header[20..24].try_into().unwrap())
            || capture["sha256"] != sha256(&bytes)
        {
            return Err(format!("GPU 截图 {name} 与窗口回执不一致"));
        }
    }
    Ok(())
}

pub(crate) fn preflight(project: &Project) -> Result<Option<PathBuf>, String> {
    let Some(work) = std::env::var_os(ENV).map(PathBuf::from) else { return Ok(None); };
    if !work.is_absolute()
        || work.canonicalize().ok().as_ref() != Some(&work)
        || project.root != work.join("project")
        || project.context.asset_roots != [project.root.join("assets")]
        || read_json(&work.join(OWNER))? != owner(project, &work)
        || work.join("window.json").exists()
        || std::env::var_os("FEATHRISE_PREFERENCES_FILE").as_deref()
            != Some(work.join("isolated-preferences.json").as_os_str())
        || std::env::var_os("FEATHRISE_VALIDATION_GLOBAL_PLUGINS").as_deref()
            != Some(work.join("isolated-global-plugins").as_os_str())
        || fs::read(work.join("isolated-global-plugins/.feathrise-plugin-fixture-owner")).ok().as_deref()
            != Some(b"feathrise-single-binary-validation-v1")
    {
        return Err("工程目录验收工程、偏好与插件隔离未通过归属校验".into());
    }
    Ok(Some(work))
}

fn owner(project: &Project, work: &Path) -> Value {
    json!({"schema":1,"purpose":PURPOSE,"work":work,"project_id":project.id,"project":project.root})
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_json(path: &Path) -> Result<Value, String> {
    let mut bytes = Vec::new();
    File::open(path).and_then(|file| file.take(128 * 1024).read_to_end(&mut bytes))
        .map_err(|error| format!("无法读取验收证据 {}：{error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    OpenOptions::new().write(true).create_new(true).open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| format!("无法独占写入 {}：{error}", path.display()))
}

fn tail(mut file: File) -> Result<String, String> {
    let size = file.metadata().map_err(|error| error.to_string())?.len();
    file.seek(SeekFrom::Start(size.saturating_sub(128 * 1024))).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
