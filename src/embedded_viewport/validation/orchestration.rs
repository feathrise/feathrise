//! 常驻验收的磁盘归属、独占输出和真实子App调度；全部由启动前或后台协调器调用。

use crate::{
    build_process::{self, BuildControl},
    project,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

pub(super) fn is_empty_fixture(root: &Path) -> Result<bool, String> {
    super::empty_fixture::selected(root)
}

pub(super) fn owner(root: &Path) -> Result<project::Project, String> {
    let project = project::open(root)?;
    let work = project.root.parent().ok_or("验收工程缺少本轮父目录")?;
    let owner: Value = serde_json::from_slice(
        &fs::read(work.join(".feathrise-native-validation-owner.json"))
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let provenance: Value = serde_json::from_slice(
        &fs::read(project.root.join(".native-fixture-provenance.json"))
            .map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if owner["schema"] != 1
        || owner["purpose"] != "ordinary-author-boundary-validation"
        || owner["work_root"] != json!(work)
        || provenance["project_root"] != json!(project.root)
        || provenance["project_id"] != project.id
        || provenance["not_in_normal_template"] != true
    {
        return Err("常驻验收要求本轮独占owner和中性夹具来源，拒绝操作普通作者工程".into());
    }
    Ok(project)
}

/// 对固定小夹具完整计算作者资产身份；不在帧内运行，不把只校验一个地图JSON当全体未落盘。
pub(super) fn asset_hashes(root: &Path) -> Result<BTreeMap<PathBuf, String>, String> {
    let assets = root
        .join("assets")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let mut pending = vec![assets.clone()];
    let mut result = BTreeMap::new();
    let mut total = 0_u64;
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let metadata = entry.metadata().map_err(|error| error.to_string())?;
            let path = entry.path();
            use std::os::windows::fs::MetadataExt;
            if entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_symlink()
                || metadata.file_attributes() & 0x400 != 0
            {
                return Err("自有夹具的资产树不能含符号链接或重解析点".into());
            }
            if metadata.is_dir() {
                pending.push(path);
                continue;
            }
            total = total.saturating_add(metadata.len());
            if result.len() >= 1024 || total > 128 * 1024 * 1024 {
                return Err("常驻验收夹具超过固定资产规模".into());
            }
            let bytes = fs::read(&path).map_err(|error| error.to_string())?;
            let hash: String = Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect();
            result.insert(
                path.strip_prefix(&assets)
                    .map_err(|error| error.to_string())?
                    .to_owned(),
                hash,
            );
        }
    }
    Ok(result)
}

/// 由 --native-report 新增的独立阶段调用；不会修改原10阶段的通过口径。
pub(crate) fn run_owned_project(
    root: &Path,
    target: &Path,
    output: &Path,
) -> Result<Value, String> {
    let project = owner(root)?;
    if !output.is_absolute() || output.exists() {
        return Err("常驻验收输出必须为本轮未存在的绝对路径".into());
    }
    fs::create_dir(output).map_err(|error| error.to_string())?;
    let preview_chrome = std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref() == Ok("preview-chrome");
    let native_preview = std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref() == Ok("native-preview");
    let build_backend = matches!(std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref(), Ok("build-backend" | "bundled-dx"));
    // 私有RPG装配仅属于这一显式自有夹具；准备完全部源码/字体后才建立运行期资产字节基线。
    if native_preview { prepare_native_preview(&project.root, output)?; }
    let before = asset_hashes(&project.root)?;
    if preview_chrome && std::env::var("FEATHRISE_PREVIEW_CHROME_NATIVE_INPUT").as_deref()!=Ok("1") {
        return Err("预览三键专项必须由真实NC输入观察器启动（FEATHRISE_PREVIEW_CHROME_NATIVE_INPUT=1）".into());
    }
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    // 原常驻报告继续直开作者；显式三键专项才通过正常主入口得到root→author→game。
    if !preview_chrome && !native_preview { command.arg("--native-editor"); }
    command
        .arg("--project")
        .arg(&project.root)
        .arg("--target-dir")
        .arg(target)
        // 每轮从独立默认偏好启动；中止的上一轮模式或用户当前选择不能污染比较顺序。
        .env(
            "FEATHRISE_PREFERENCES_FILE",
            output.join("isolated-preferences.json"),
        )
        .env("FEATHRISE_EMBEDDED_SMOKE_DIR", output)
        .env("NEW_EDITOR_EMBEDDED_GAME_SMOKE_DIR", output.join("game"))
        .env_remove("NEW_EDITOR_NATIVE_SMOKE_DIR")
        .env_remove("NEW_EDITOR_GAME_SMOKE_DIR")
        .env_remove("FEATHRISE_WORKSPACE_SMOKE_DIR");
    crate::native_validation::isolate_author_command(&mut command, output)?;
    let run = build_process::run(
        &command,
        &BuildControl::default(),
        Some(Duration::from_secs(2400)),
        tail,
    )?;
    fs::write(
        output.join("process.log"),
        format!("{}\n{}", run.stdout, run.stderr),
    )
    .map_err(|error| error.to_string())?;
    let after = asset_hashes(&project.root)?;
    let report_path = output.join(if build_backend { "build-backend.json" } else if native_preview { "native-preview.json" } else if preview_chrome { "preview-chrome.json" } else { "embedded-editor.json" });
    let mut report: Value = serde_json::from_slice(&fs::read(&report_path).map_err(|error| {
        format!(
            "常驻验收未生成回执：{error}；退出{}；{}",
            run.exit_code, run.stderr
        )
    })?)
    .map_err(|error| error.to_string())?;
    report["asset_files_checked"] = json!(before.len());
    report["all_author_asset_bytes_preserved"] = json!(before == after);
    report["process_exit"] = json!(run.exit_code);
    if before != after || run.exit_code != 0 {
        report["passed"] = json!(false);
    }
    let gpu = if build_backend { super::build_backend::verify(output, &report) }
        else if native_preview { super::native_preview_validation::verify(output, &report) }
        else if preview_chrome { verify_preview_chrome(output, &report) } else { verify_guest_evidence(output, &report) };
    match &gpu {
        Ok(evidence) => report["game_gpu_evidence"] = json!(evidence),
        Err(error) => {
            report["passed"] = json!(false);
            report["game_gpu_error"] = json!(error);
        }
    }
    fs::write(
        &report_path,
        serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if report["passed"] != true {
        return Err(format!("常驻双模式验收未通过：{report}"));
    }
    Ok(report)
}

/// 专用 CLI 每次独占新工作根，复用与原 --native-report 完全相同的中性夹具入口。
pub(crate) fn run(target: PathBuf, report: PathBuf) -> Result<(), String> {
    let report = if report.is_absolute() {
        report
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(report)
    };
    if report.exists() {
        return Err("常驻报告已存在，请使用本轮新的文件名".into());
    }
    fs::create_dir_all(report.parent().ok_or("报告缺少父目录")?)
        .map_err(|error| error.to_string())?;
    let work = report.with_extension("work");
    fs::create_dir(&work).map_err(|error| format!("常驻验收工作根不能独占创建：{error}"))?;
    let work = work.canonicalize().map_err(|error| error.to_string())?;
    use std::io::Write;
    fs::OpenOptions::new().write(true).create_new(true).open(work.join(".feathrise-native-validation-owner.json"))
        .and_then(|mut file|file.write_all(&serde_json::to_vec_pretty(&json!({"schema":1,"purpose":"ordinary-author-boundary-validation","work_root":work})).expect("固定owner可编码")))
        .map_err(|error|error.to_string())?;
    let started = Instant::now();
    let result = (|| -> Result<Value, String> {
        let repository = crate::project_template_root()
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let root = crate::project_template::create_with_control(
            &work.join("常驻工程对比"),
            "常驻工程对比",
            &repository,
            &BuildControl::default(),
        )?;
        crate::native_validation::prepare_workspace_fixture(&root, "常驻工程对比")?;
        // 仅此显式validation入口、刚刚独占创建的夹具可切空图；普通产品不消费此开关。
        if std::env::var("FEATHRISE_EMBEDDED_EMPTY_FIXTURE").as_deref() == Ok("1") {
            super::empty_fixture::prepare(&root)?;
        }
        if std::env::var("FEATHRISE_EMBEDDED_LIVE_PREVIEW").as_deref() == Ok("1") {
            super::live_fixture::prepare(&root)?;
        }
        run_owned_project(&root, &target, &work.join("embedded-comparison"))
    })();
    let scope=match std::env::var("FEATHRISE_EMBEDDED_SCENARIO").as_deref() {
        Ok("build-backend") => "project_build_backend_owned_window_validation",
        Ok("bundled-dx") => "bundled_dx_owned_window_validation",
        Ok("native-preview") => "native_preview_owned_window_validation",
        Ok("preview-chrome") => "preview_chrome_owned_three_level_window_validation",
        _ => "resident_dual_mode_owned_window_validation",
    };
    let summary = json!({"schema":1,"scope":scope,"passed":result.is_ok(),
        "elapsed_ms":started.elapsed().as_secs_f64()*1000.0,"work":work,"report":result.as_ref().ok(),"error":result.as_ref().err()});
    fs::write(
        report,
        serde_json::to_vec_pretty(&summary).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    result.map(|_| ())
}

/// 公共编辑器只运行本轮显式指定的夹具准备器，不依赖私有RPG源码或向普通模板安装它。
fn prepare_native_preview(root: &Path, output: &Path) -> Result<(), String> {
    owner(root)?;
    let argument = |name: &str| std::env::var_os(name).map(PathBuf::from).ok_or_else(|| format!("原生专项缺少{name}"));
    let preparer = argument("FEATHRISE_NATIVE_PREVIEW_FIXTURE_PREPARER")?;
    let python = argument("FEATHRISE_VALIDATION_PYTHON")?;
    let repository = argument("FEATHRISE_NATIVE_PREVIEW_REPO_ROOT")?;
    let token = std::env::var("FEATHRISE_NATIVE_PREVIEW_TOKEN").map_err(|_| "原生专项缺少token")?;
    if !(16..=128).contains(&token.len()) || !token.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte)) {
        return Err("原生专项token格式无效".into());
    }
    for path in [&preparer, &python, &repository] {
        if !path.is_absolute() || super::native_preview_validation::ordinary_path(path).is_err() {
            return Err("夹具准备器、解释器和仓库必须是实际绝对普通路径".into());
        }
    }
    let mut command = Command::new(python);
    command.args(["-B", "-X", "utf8"]).arg(preparer).arg("--project").arg(root)
        .arg("--repo-root").arg(repository).arg("--output").arg(output.join("runtime")).arg("--token").arg(token);
    let result = build_process::run(&command, &BuildControl::default(), Some(Duration::from_secs(60)), tail)?;
    fs::write(output.join("fixture-preparation.log"), format!("{}\n{}", result.stdout, result.stderr)).map_err(|error| error.to_string())?;
    if result.exit_code != 0 { return Err(format!("私有RPG夹具准备失败：{}", result.stderr)); }
    Ok(())
}

fn verify_preview_chrome(output:&Path,report:&Value)->Result<Vec<Value>,String>{
    if report["scope"]!="preview-original-caption-owned-three-level-window"||report["owner_checked"]!=true {
        return Err("预览三键专项缺少本轮三层窗口归属".into());
    }
    let steps=report["steps"].as_array().ok_or("预览三键缺少阶段")?;
    for name in ["author-ready","embedded-editing","playing-original-captions","returned-editing","playing-before-close","close-confirm-restored-editor"] {
        if steps.iter().filter(|step|step["stage"]==name).count()!=1{return Err(format!("预览三键阶段不完整：{name}"));}
    }
    if report["native_input"]!=true{return Err("三键必须经过实际原生NC输入".into());}
    for stage in ["native-controls","native-close"] {
        let mut gates=steps.iter().filter(|step|step["native_input_gate"]==stage);
        let ack=&gates.next().ok_or_else(||format!("缺少实际原生三键门回执：{stage}"))?["external_real_input_ack"];
        if gates.next().is_some()||ack["passed"]!=true{return Err(format!("原生三键门重复或失败：{stage}"));}
        let actions=ack["actions"].as_array().ok_or("原生门缺少实际按住/松开采样")?;
        let expected:&[&str]=if stage=="native-controls"{&["maximize","restore","minimize"]}else{&["close"]};
        if actions.len()!=expected.len(){return Err("原生门动作数量不符".into());}
        for action in expected{
            let samples:Vec<_>=actions.iter().filter(|sample|sample["action"]==*action).collect();
            let Some(sample)=samples.first().filter(|_|samples.len()==1)else{return Err("原生三键缺少唯一动作".into());};
            if sample["held_ms"].as_f64().unwrap_or(0.)<120.||sample["action_before_release"]!=false
                ||sample["held_capture_author"]!=true||sample["held_samples"].as_array().is_none_or(|values|values.len()<2)
                ||sample["down_ns"].as_u64().zip(sample["up_ns"].as_u64()).is_none_or(|(down,up)|up<=down)
                ||(*action=="close"&&sample["hit"]!=20){return Err(format!("{action}缺少实际NC按住/松开/作者捕获证明"));}
            if *action!="close"{
                let key=if *action=="minimize"{"iconic"}else{"zoomed"};let after=*action!="restore";
                if sample["state_transitions"]!=1||sample["single_action_observed"]!=true
                    ||sample["before"][key]!=!after||sample["after"][key]!=after{return Err(format!("{action}没有且仅有一次实际系统状态切换"));}
            }
        }
        if stage=="native-controls"&&(ack["system_restore"]["message"]!=274||ack["system_restore"]["command"]!=61728
            ||ack["system_restore"]["before"]["iconic"]!=true||ack["system_restore"]["after"]["iconic"]!=false
            ||ack["system_restore"]["state_transitions"]!=1){return Err("最小化后缺少真实SC_RESTORE恢复证明".into());}
    }
    let mut evidence=Vec::new();
    for step in steps.iter().filter(|step|!step["game"].is_null()) {
        let game=&step["game"];let path=PathBuf::from(game["snapshot_png"].as_str().ok_or("阶段游戏PNG未冻结")?);
        let canonical=path.canonicalize().map_err(|error|error.to_string())?;
        if !canonical.starts_with(output.canonicalize().map_err(|error|error.to_string())?){return Err("阶段截图超出本轮输出".into());}
        let bytes=fs::read(&canonical).map_err(|error|error.to_string())?;
        let hash:String=Sha256::digest(&bytes).iter().map(|byte|format!("{byte:02x}")).collect();
        let pixels=crate::window_chrome::preview_validation::snapshot_pixel_identity(&bytes,&game["game"])?
            .ok_or("阶段实际游戏PNG与原GPU/RGBA回执不符")?;
        if game["snapshot_pixels"]!=pixels||pixels["png_file_sha256"]!=hash{return Err("阶段游戏PNG文件或像素被后续覆盖".into());}
        evidence.push(json!({"stage":step["stage"],"path":canonical,"pixel_identity":pixels}));
    }
    Ok(evidence)
}

pub(crate) fn read_guest_capture(
    directory: &Path,
    pid: u32,
    revision: u64,
    playing: bool,
    min_frame: u64,
) -> Result<Option<Value>, String> {
    let mode = if playing { "playing" } else { "editing" };
    let stem = format!("{pid}-{revision}-{mode}");
    let path = directory.join(format!("{stem}.json"));
    let mut file = match fs::File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    file.by_ref()
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 128 * 1024 {
        return Err("运行截图证据超过固定预算".into());
    }
    // 同文件重写窗口可能读到尚未完整的JSON；下一次有界重试，不把临时缺写当GPU成功。
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return Ok(None);
    };
    if value["pid"] != pid
        || value["revision"] != revision
        || value["mode"] != mode
        || value["source"] != "real-project-world"
    {
        return Err("运行截图的会话/修订/模式身份不符".into());
    }
    if value["frame"].as_u64().unwrap_or(0) < min_frame {
        return Ok(None);
    }
    // 作者参数可能比同修订首张截图稍后到达；SDK按显示身份重拍后才能作为该阶段证据。
    if super::view_controls::verify_mode(&value, playing).is_err() {
        return Ok(None);
    }
    if value["capture"]["source"] != "Bevy ScreenshotCaptured"
        || value["capture"]["width"].as_u64().unwrap_or(0) == 0
        || value["capture"]["height"].as_u64().unwrap_or(0) == 0
        || value["capture"]["sha256"]
            .as_str()
            .is_none_or(|hash| hash.len() != 64)
    {
        return Err("运行实例证据没有真实GPU像素回读".into());
    }
    let png = directory.join(format!("{stem}.png"));
    let mut signature = [0_u8; 8];
    fs::File::open(&png)
        .and_then(|mut file| file.read_exact(&mut signature))
        .map_err(|error| error.to_string())?;
    if signature != *b"\x89PNG\r\n\x1a\n" {
        return Err("运行实例实际截图不是完整PNG文件".into());
    }
    Ok(Some(value))
}

fn verify_guest_evidence(output: &Path, report: &Value) -> Result<Vec<Value>, String> {
    if report["game_preview_return_input"]
        != "owned HWND mouse -> actual Bevy toolbar button -> RequestMode -> ModeApplied"
    {
        return Err("缺少本轮实际工程预览工具条返回链验收".into());
    }
    let steps = report["steps"].as_array().ok_or("宿主回执缺少阶段数组")?;
    super::presentation_evidence::verify_records(steps, report["empty_fixture"] == true)?;
    super::tool_previews::verify(&output.join("game"), &report["semantic_tools"])?;
    if report["empty_fixture"]!=true {
        super::handle_hover::verify(&output.join("game"),&report["semantic_tools"]["terrain_hover"])?;
    }
    super::preview_toolbar::verify(&output.join("game"), report)?;
    let new_id = steps
        .iter()
        .find(|step| step["stage"] == "unsaved_cuboid_applied")
        .and_then(|step| step["author"]["last_added_id"].as_str())
        .ok_or("未保存创建没有真实稳定ID")?;
    if report["live_fixture"] == true {
        super::live_preview::verify(&output.join("game"), report, new_id)?;
    }
    let mut evidence = Vec::new();
    let mut stages = vec![
        "resident_game_3d_ready",
        "unsaved_cuboid_applied",
        "original_undo_applied",
        "original_redo_applied",
        "f12_play_same_world",
        "f12_edit_same_world_and_paused_ticks",
        "restart_adopted_same_unsaved_author",
        "recovered_world_returned_to_edit",
    ];
    if report["empty_fixture"] == true {
        stages.extend(["empty_map_f12_play_same_world", "empty_map_f12_returned"]);
    }
    for stage in stages {
        let step = steps
            .iter()
            .find(|step| step["stage"] == stage)
            .ok_or_else(|| format!("缺少常驻阶段：{stage}"))?;
        let pid = step["pid"].as_u64().ok_or("阶段缺少PID")? as u32;
        let revision = step["applied_revision"]
            .as_u64()
            .ok_or("阶段缺少实际修订")?;
        let playing = step["mode"] == "playing";
        let capture = read_guest_capture(&output.join("game"), pid, revision, playing, 0)?
            .ok_or_else(|| format!("阶段{stage}缺少对应真实运行实例GPU截图"))?;
        if capture["world_id"] != step["world_id"] {
            return Err(format!("{stage}截图来自另一World"));
        }
        super::placement_evidence::verify(&capture, &step["author"])
            .map_err(|error| format!("{stage}实际实例未通过：{error}"))?;
        super::window_evidence::verify_record(step, &capture, &report["host_pid"])
            .map_err(|error| format!("{stage}原生窗口区域未通过：{error}"))?;
        let placements = capture["placements"]
            .as_array()
            .ok_or("运行截图未记录真实实例")?;
        let actual = placements
            .iter()
            .find(|placement| placement["id"] == new_id);
        let expected = step["author"]["placements"]
            .as_array()
            .ok_or("作者证据缺少实例")?
            .iter()
            .find(|placement| placement["id"] == new_id);
        if actual.is_some() != expected.is_some() {
            return Err(format!("{stage}运行实例实际实例与未保存作者/撤销状态不一致"));
        }
        if let (Some(actual), Some(expected)) = (actual, expected) {
            if actual["position"] != expected["translation"] || actual["map"] != "M1-SAMPLE" {
                return Err(format!("{stage}运行实例实际实例位置或地图不符"));
            }
        }
        evidence.push(json!({"stage":stage,"passed":true,"capture":capture}));
    }
    Ok(evidence)
}

fn tail(mut reader: fs::File) -> Result<String, String> {
    let mut tail = Vec::new();
    let mut progress = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        let excess = (tail.len() + count).saturating_sub(64 * 1024);
        if excess != 0 {
            tail.drain(..excess);
        }
        tail.extend_from_slice(&buffer[..count]);
        // 本轮自有验收的阶段即时可见，不能把数分钟构建和失败原因藏到整个子进程退出后。
        // 只转发有限阶段行；完整诊断仍由原64KiB尾部与最终报告负责。
        progress.extend_from_slice(&buffer[..count]);
        while let Some(end) = progress.iter().position(|byte| *byte == b'\n') {
            let line = &progress[..end];
            if line.len() <= 16 * 1024 && line.starts_with(b"FEATHRISE_EMBEDDED_") {
                println!("{}", String::from_utf8_lossy(line));
            }
            progress.drain(..=end);
        }
        if progress.len() > 16 * 1024 {
            progress.clear();
        }
    }
    Ok(String::from_utf8_lossy(&tail).into_owned())
}
