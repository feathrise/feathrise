//! 独占夹具、偏好工程3D和真实Cargo延迟的准备／聚合；所有整树哈希都在App之外执行。

use super::{ENVIRONMENT, OWNER_FILE, PURPOSE, REPORT_FILE, RELEASE_FILE, RETRY_FILE, START_FILE};
use crate::{build_process::{self, BuildControl}, project::Project};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::{Read, Write}, path::{Path, PathBuf}, process::Command, time::{Duration, Instant}};

pub(crate) fn run(target: PathBuf, report: PathBuf) -> Result<(), String> {
    let target = target.canonicalize().map_err(|error|format!("必须复用已存在的构建缓存：{error}"))?;
    let report = if report.is_absolute() { report } else { std::env::current_dir().map_err(|error|error.to_string())?.join(report) };
    if report.exists() { return Err("停止编译验收报告已存在，请使用新的文件名".into()); }
    fs::create_dir_all(report.parent().ok_or("停止编译报告缺少父目录")?).map_err(|error|error.to_string())?;
    let work = report.with_extension("work");
    fs::create_dir(&work).map_err(|error|format!("停止编译工作根不能独占创建：{error}"))?;
    let work = work.canonicalize().map_err(|error|error.to_string())?;
    write_new(&work.join(".feathrise-native-validation-owner.json"), &json!({"schema":1,
        "purpose":"ordinary-author-boundary-validation","work_root":work}))?;
    let started = Instant::now();
    let result = (|| -> Result<Value, String> {
        let template = crate::project_template_root().canonicalize().map_err(|error|error.to_string())?;
        let root = crate::project_template::create_with_control(&work.join("工程3D停止编译"), "工程3D停止编译", &template, &BuildControl::default())?;
        crate::native_validation::prepare_workspace_fixture(&root, "工程3D停止编译")?;
        let project = super::super::orchestration::owner(&root)?;
        let directory = work.join("build-cancel");
        fs::create_dir(&directory).map_err(|error|error.to_string())?;
        let nonce = bevy::asset::uuid::Uuid::new_v4().to_string();
        let source = super::script(&nonce, &project.id);
        // 只有刚创建且再次通过owner/provenance核对的夹具允许注入延迟源码。
        let original_script = read_bounded(&root.join("build.rs"), 32 * 1024)?;
        if original_script != super::TEMPLATE_BUILD_SCRIPT.as_bytes() {
            return Err("自有夹具build.rs不是当前空模板原字节，拒绝覆盖".into());
        }
        write_bytes_new(&directory.join("original-build.rs"), &original_script)?;
        fs::write(root.join("build.rs"), source.as_bytes()).map_err(|error|error.to_string())?;
        write_bytes_new(&root.join(".validation-build-token"), nonce.as_bytes())?;
        let preferences = directory.join("isolated-preferences.json");
        write_new(&preferences.with_extension("viewport.json"), &json!({"format":1,"mode":"embedded_world"}))?;
        write_new(&root.join(OWNER_FILE), &json!({"schema":1,"purpose":PURPOSE,"project_root":project.root,
            "project_id":project.id,"directory":directory,"target":target,"nonce":nonce,
            "build_script_sha256":hash_bytes(source.as_bytes()),"original_build_script_sha256":hash_bytes(&original_script),
            "preferences":preferences}))?;
        owned(&root, &directory)?;
        let before = protected_hashes(&project)?;
        let mut command = Command::new(std::env::current_exe().map_err(|error|error.to_string())?);
        command.args(["--native-editor", "--project"]).arg(&root).arg("--target-dir").arg(&target);
        // 本轮自动操作相互排斥；清除继承的其他验收开关后，再明确传递本场景和实际工程GPU目录。
        for (key, _) in std::env::vars_os() {
            let key_text = key.to_string_lossy();
            if key_text.starts_with("NEW_EDITOR_") || key_text.starts_with("FEATHRISE_EMBEDDED_")
                || key_text.contains("_VALIDATION") || key_text.contains("_SMOKE_")
                || key_text.starts_with("FEATHRISE_CODE_ITERATION_") || key_text.starts_with("FEATHRISE_EXPORT_") {
                command.env_remove(key);
            }
        }
        command.env(ENVIRONMENT, &directory).env("FEATHRISE_PREFERENCES_FILE", &preferences)
            .env("NEW_EDITOR_EMBEDDED_GAME_SMOKE_DIR", directory.join("game"));
        crate::native_validation::isolate_author_command(&mut command, &directory)?;
        let process = build_process::run(&command, &BuildControl::default(), Some(Duration::from_secs(3600)), tail)?;
        write_bytes_new(&directory.join("process.log"), format!("{}\n{}", process.stdout, process.stderr).as_bytes())?;
        let after = protected_hashes(&project)?;
        let child = read_json(&directory.join(REPORT_FILE))?;
        let validation = verify(&child, &project, &directory, &target, &nonce);
        let preserved = before == after;
        let evidence = json!({"process_exit":process.exit_code,"protected_file_count":before.len(),
            "all_author_assets_and_sources_preserved":preserved,"protected_before":before,"protected_after":after,
            "editor":child,"independent_checks":validation.as_ref().ok(),"verification_error":validation.as_ref().err()});
        write_new(&directory.join("aggregate-evidence.json"), &evidence)?;
        if process.exit_code != 0 || !preserved { return Err(format!("真实作者退出或受保护文件保持失败：{evidence}")); }
        validation?;
        Ok(evidence)
    })();
    write_new(&report, &json!({"schema":1,"scope":PURPOSE,"passed":result.is_ok(),"work":work,
        "elapsed_ms":started.elapsed().as_secs_f64()*1000.,"evidence":result.as_ref().ok(),"error":result.as_ref().err(),
        "not_verified":["physical keyboard or mouse feel","normal running/Playing stop","all unsaved author domains","other machines or large-map frame performance"]}))?;
    result.map(|_|())
}

pub(super) fn owned(root: &Path, directory: &Path) -> Result<(Project, Value), String> {
    let project = super::super::orchestration::owner(root)?;
    let owner = read_json(&project.root.join(OWNER_FILE))?;
    if !directory.is_absolute() || !directory.is_dir() || owner["schema"] != 1 || owner["purpose"] != PURPOSE
        || owner["project_id"] != project.id || owner["project_root"] != json!(project.root)
        || owner["directory"] != json!(directory) || directory.parent() != project.root.parent()
        || owner["build_script_sha256"] != hash_bytes(&read_bounded(&project.root.join("build.rs"), 32 * 1024)?) {
        return Err("停止编译验收归属、输出目录或延迟源码身份不一致，拒绝安装自动操作".into());
    }
    let nonce = owner["nonce"].as_str().ok_or("停止编译夹具缺少随机身份")?;
    bevy::asset::uuid::Uuid::parse_str(nonce).map_err(|_|"停止编译夹具随机身份无效")?;
    if read_bounded(&project.root.join(".validation-build-token"), 64)? != nonce.as_bytes()
        || hash_bytes(super::script(nonce, &project.id).as_bytes()) != owner["build_script_sha256"]
        || owner["original_build_script_sha256"] != hash_bytes(super::TEMPLATE_BUILD_SCRIPT.as_bytes())
        || read_bounded(&directory.join("original-build.rs"), 32 * 1024)? != super::TEMPLATE_BUILD_SCRIPT.as_bytes() {
        return Err("停止编译夹具令牌或真实Cargo脚本与本轮身份不符".into());
    }
    Ok((project, owner))
}

fn protected_hashes(project: &Project) -> Result<BTreeMap<PathBuf, String>, String> {
    let mut result = BTreeMap::new();
    for (name, hash) in super::super::orchestration::asset_hashes(&project.root)? { result.insert(Path::new("assets").join(name), hash); }
    let mut pending = vec![project.root.join("src")];
    while let Some(path) = pending.pop() {
        if result.len() >= 1024 { return Err("停止编译夹具源码超过固定文件预算".into()); }
        let metadata = fs::symlink_metadata(&path).map_err(|error|error.to_string())?;
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 { return Err("停止编译夹具源码不能含重解析路径".into()); }
        if metadata.is_dir() {
            for entry in fs::read_dir(&path).map_err(|error|error.to_string())? { pending.push(entry.map_err(|error|error.to_string())?.path()); }
        } else { result.insert(path.strip_prefix(&project.root).map_err(|error|error.to_string())?.into(), hash_bytes(&read_bounded(&path, 8 * 1024 * 1024)?)); }
    }
    for name in ["editor-project.json", "Cargo.toml", "Cargo.lock", "build.rs"] {
        result.insert(name.into(), hash_bytes(&read_bounded(&project.root.join(name), 8 * 1024 * 1024)?));
    }
    Ok(result)
}

pub(super) fn read_json(path: &Path) -> Result<Value, String> {
    serde_json::from_slice(&read_bounded(path, 1024 * 1024)?).map_err(|error|format!("{}：{error}", path.display()))
}

pub(super) fn read_bounded(path: &Path, budget: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    fs::File::open(path).map_err(|error|format!("{}：{error}", path.display()))?.take(budget as u64 + 1)
        .read_to_end(&mut bytes).map_err(|error|error.to_string())?;
    if bytes.len() > budget { return Err(format!("{}超过固定取证预算", path.display())); }
    Ok(bytes)
}

pub(super) fn write_new(path: &Path, value: &Value) -> Result<(), String> {
    write_bytes_new(path, &serde_json::to_vec_pretty(value).map_err(|error|error.to_string())?)
}

pub(super) fn write_bytes_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::OpenOptions::new().write(true).create_new(true).open(path).and_then(|mut file|file.write_all(bytes)).map_err(|error|format!("{}：{error}", path.display()))
}

pub(super) fn hash_bytes(bytes: &[u8]) -> String { Sha256::digest(bytes).iter().map(|byte|format!("{byte:02x}")).collect() }

fn tail(mut reader: fs::File) -> Result<String, String> {
    let mut bytes = Vec::new();
    let mut line = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer).map_err(|error|error.to_string())?;
        if count == 0 { break; }
        bytes.extend_from_slice(&buffer[..count]);
        let excess = bytes.len().saturating_sub(64 * 1024);
        bytes.drain(..excess);
        line.extend_from_slice(&buffer[..count]);
        while let Some(end) = line.iter().position(|byte|*byte == b'\n') {
            if end < 16 * 1024 && line[..end].starts_with(b"FEATHRISE_EMBEDDED_BUILD_CANCEL_") { println!("{}", String::from_utf8_lossy(&line[..end])); }
            line.drain(..=end);
        }
        if line.len() > 16 * 1024 { line.clear(); }
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

const STAGES: &[&str] = &[
    "opened_with_embedded_preference", "unsaved_author_draft_prepared", "real_cargo_build_script_running_and_stop_button_gpu",
    "original_stop_compilation_pressed", "cancelled_cargo_worker_and_process_really_drained", "actual_stopped_ui_gpu",
    "original_editor_3d_selected", "editor_3d_stable_without_automatic_restart", "original_editor_undo_after_stop",
    "original_editor_redo_after_stop", "editor_3d_usable_actual_gpu", "owned_retry_gate_released_after_drain",
    "original_project_3d_retry_pressed", "explicit_retry_started_new_real_slot", "retry_adopted_preserved_author_and_real_gpu",
    "retry_host_ui_gpu", "back_to_editor_3d_and_retry_worker_drained",
];

fn step<'a>(child: &'a Value, stage: &str) -> Result<&'a Value, String> {
    let matches: Vec<_> = child["steps"].as_array().ok_or("停止编译报告缺少阶段数组")?.iter().filter(|step|step["stage"] == stage).collect();
    if matches.len() != 1 { return Err(format!("停止编译阶段必须唯一存在：{stage}")); }
    Ok(matches[0])
}

fn verify(child: &Value, project: &Project, directory: &Path, target: &Path, nonce: &str) -> Result<Value, String> {
    if child["schema"] != 1 || child["passed"] != true || child["scope"] != PURPOSE || child["project_id"] != project.id
        || child["project_root"] != json!(project.root) || child["directory"] != json!(directory) || child["nonce"] != nonce {
        return Err("停止编译实际作者报告未通过或归属不一致".into());
    }
    verify_stage_order(child)?;
    let opened = step(child, STAGES[0])?;
    if opened["selected"] != "embedded_world" || opened["selection_revision"] != 1 || opened["status"]["starts"] != 1
        || opened["status"]["ready"] != false || opened["initial_mode_input_injected"] != false { return Err("没有证明打开工程直接采用隔离工程3D偏好".into()); }
    let started = step(child, STAGES[2])?;
    let stop = step(child, STAGES[3])?;
    let drain = step(child, STAGES[4])?;
    verify_cancel(started, stop, drain)?;
    if started["build"]["marker"] != read_json(&project.root.join(START_FILE))?
        || started["build"]["marker"]["nonce"] != nonce || started["build"]["marker"]["released"] != false
        || started["build"]["marker"]["project_id"] != project.id { return Err("真实Cargo启动标记不属于本轮独占工程".into()); }
    let build_image = PathBuf::from(started["build"]["image"].as_str().ok_or("缺少真实build.rs进程路径")?).canonicalize().map_err(|error|error.to_string())?;
    if !build_image.starts_with(target) || build_image.file_name().is_none_or(|name|name != "build-script-build.exe") { return Err("运行标记未对应本轮缓存中的真实Cargo build.rs可执行文件".into()); }
    let baseline = &step(child, STAGES[1])?["author"];
    for stage in [STAGES[2], STAGES[3], STAGES[4], STAGES[6], STAGES[7]] {
        if &step(child, stage)?["author"] != baseline { return Err(format!("{stage}改变作者草稿、修订或历史")); }
    }
    let stable = step(child, STAGES[7])?;
    if stable["observed_frames"].as_u64().unwrap_or(0) < 60 || stable["observed_ms"].as_f64().unwrap_or(0.) < 3000.
        || stable["status"]["starts"] != 1 || stable["status"]["active"] != false || stable["selected"] != "saved_world" { return Err("停止后编辑器3D没有经过真实持续观察，或出现自动重启".into()); }
    let restored = &step(child, STAGES[9])?["author"];
    verify_history_settlement(&step(child, STAGES[9])?["history_settlement"], &stable["presentation"])?;
    for field in ["placements", "dirty", "save_sequence", "undo_len", "redo_len", "last_added_id"] {
        if restored[field] != baseline[field] { return Err(format!("原Undo/Redo后没有恢复作者{field}")); }
    }
    let retry = step(child, STAGES[13])?;
    if retry["status"]["starts"] != 2 || retry["status"]["completed"] != 1 || retry["status"]["active"] != true
        || retry["session"].as_u64().is_none_or(|session|session == 0) || retry["session"] == stop["session"]
        || retry["control_cancelled"] != false { return Err("显式重试没有获得新的真实工作槽和会话身份".into()); }
    let final_step = step(child, STAGES[16])?;
    if final_step["status"]["starts"] != 2 || final_step["status"]["completed"] != 2 || final_step["status"]["active"] != false
        || final_step["runtime_slot_present"] != false || final_step["selected"] != "saved_world" || &final_step["author"] != restored {
        return Err("显式重试最终未真实排空或丢失恢复后的作者现场".into());
    }
    let retried = read_json(&project.root.join(RETRY_FILE))?;
    if retried["nonce"] != nonce || retried["project_id"] != project.id || retried["released"] != true
        || !project.root.join(RELEASE_FILE).is_file() { return Err("显式重试没有再次经过本轮真实Cargo构建脚本".into()); }
    let guest = step(child, STAGES[14])?;
    if &guest["author"] != restored || guest["guest"]["game"]["source"] != "real-project-world"
        || guest["guest"]["game"]["pid"] != guest["pid"] || guest["guest"]["game"]["world_id"] != guest["world_id"]
        || guest["guest"]["game"]["revision"] != guest["applied_revision"] || guest["guest"]["game"]["mode"] != "editing"
        || guest["guest"]["png"]["rgba_sha256"] != guest["guest"]["game"]["author_tools"]["rgba_sha256"] {
        return Err("重新连接没有保留作者草稿，或实际GPU像素没有绑定当前工程World/修订".into());
    }
    super::super::placement_evidence::verify(&guest["guest"]["game"], restored)?;
    super::super::window_evidence::verify_record(guest, &guest["guest"]["game"], &child["host_pid"])?;
    let mut captures = Vec::new();
    for (stage, name) in [(STAGES[2], "compiling-stop-visible.png"), (STAGES[5], "compilation-stopped.png"),
        (STAGES[10], "editor3d-usable.png"), (STAGES[15], "retry-engine-ready.png")] {
        captures.push(super::evidence::verify_png(&directory.join(name), &step(child, stage)?["capture"])?);
    }
    captures.push(super::evidence::verify_png(&directory.join("retry-game.png"), &guest["guest"]["png"])?);
    if read_json(&directory.join("retry-game.json"))? != guest["guest"]["game"] { return Err("重新连接的不可变游戏GPU记录与当前作者阶段不一致".into()); }
    Ok(json!({"required_stages":STAGES.len(),"captures":captures,"real_cargo_cancel_and_retry":true,
        "asset_hashes_checked_outside_app":true,"original_stop_button_route":true,"no_automatic_restart_ms":stable["observed_ms"]}))
}

fn verify_stage_order(child: &Value) -> Result<(), String> {
    let steps = child["steps"].as_array().ok_or("停止编译报告缺少阶段")?;
    let mut previous = None;
    for name in STAGES {
        step(child, name)?;
        let index = steps.iter().position(|step|step["stage"] == *name).unwrap();
        if previous.is_some_and(|previous|index <= previous) { return Err("停止编译必需阶段顺序错误".into()); }
        previous = Some(index);
    }
    Ok(())
}

fn verify_cancel(started: &Value, stop: &Value, drain: &Value) -> Result<(), String> {
    if started["build"]["image_in_current_target"] != true || started["build"]["process_alive"] != true
        || stop["input"] != "original visible StopCompilation Button + EditorButtonClick"
        || stop["control_cancelled_before"] != false || stop["thread_finished_before"] != false
        || stop["session"].as_u64().is_none_or(|session|session == 0) || stop["build"] != started["build"]
        || drain["control_cancelled"] != true || drain["cleanup_failed"] != false || drain["build_process"]["signaled"] != true
        || drain["build_process"]["pid"] != started["build"]["marker"]["pid"] || drain["runtime_slot_present"] != false
        || drain["status"]["active"] != false || drain["status"]["ready"] != false || drain["status"]["starts"] != 1
        || drain["status"]["completed"] != 1 || drain["status"].get("error") != Some(&Value::Null)
        || drain["compilation_stopped"] != true || drain["pending_play"] != false || drain["restart_requested"] != false
        || drain.get("identity") != Some(&Value::Null) { return Err("缺少真实Cargo运行、原停止入口或线程/进程完整收口证据".into()); }
    Ok(())
}

/// 作者历史回执之后还须等待真实呈现连续稳定；仅同一已删对象的选择清理可成为新基线。
fn verify_history_settlement(proof: &Value, before: &Value) -> Result<(), String> {
    let after = &proof["after_history"];
    let changes = proof["selection_transitions"].as_array().filter(|changes|!changes.is_empty() && changes.len() <= 8)
        .ok_or("原历史后的选择收口缺少有界变化记录")?;
    if proof["ready"] != true || proof["stable_frames"].as_u64().unwrap_or(0) < 8
        || proof["stable_ms"].as_f64().is_none_or(|ms|!ms.is_finite() || ms < 250.)
        || proof["before_history"] != *before || !before["selection"].is_object() || !after["selection"].is_object()
        || changes.last().unwrap()["selection"] != after["selection"] {
        return Err("原Undo/Redo没有取得与历史前基线对应的连续稳定呈现".into());
    }
    for field in ["roots", "headers", "camera", "navigation", "panes", "layout"] {
        if after[field] != before[field] { return Err(format!("原历史收口改变作者{field}现场")); }
    }
    let mut previous = 0.;
    for change in changes {
        let elapsed = change["elapsed_ms"].as_f64().filter(|ms|ms.is_finite() && *ms >= previous)
            .ok_or("原历史选择变化没有单调真实时间")?;
        previous = elapsed;
        for field in ["map", "directory_map", "portal", "view", "orthographic"] {
            if change["selection"][field] != before["selection"][field] {
                return Err(format!("原历史选择清理误改{field}，不能形成恢复基线"));
            }
        }
        if !change["selection"]["placement"].is_null() && change["selection"]["placement"] != before["selection"]["placement"] {
            return Err("原历史收口选择了已删恢复对象之外的其他实例".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_retry_receipt_cannot_replace_missing_duplicate_or_reordered_stop_stages() {
        let valid = json!({"passed":true,"steps":STAGES.iter().map(|stage|json!({"stage":stage})).collect::<Vec<_>>()});
        assert!(verify_stage_order(&valid).is_ok());
        let mut missing = valid.clone(); missing["steps"].as_array_mut().unwrap().remove(4);
        assert!(verify_stage_order(&missing).is_err());
        let mut duplicate = valid.clone(); duplicate["steps"].as_array_mut().unwrap().push(json!({"stage":STAGES[4]}));
        assert!(verify_stage_order(&duplicate).is_err());
        let mut reordered = valid; reordered["steps"].as_array_mut().unwrap().swap(3,4);
        assert!(verify_stage_order(&reordered).is_err());
    }

    #[test]
    fn stop_requires_cancelled_control_drained_slot_and_same_held_process_exit() {
        let started = json!({"build":{"image_in_current_target":true,"process_alive":true,"marker":{"pid":73}}});
        let stop = json!({"input":"original visible StopCompilation Button + EditorButtonClick","session":7,
            "control_cancelled_before":false,"thread_finished_before":false,"build":started["build"]});
        let drain = json!({"control_cancelled":true,"cleanup_failed":false,"build_process":{"signaled":true,"pid":73},
            "runtime_slot_present":false,"status":{"active":false,"ready":false,"starts":1,"completed":1,"error":null},
            "compilation_stopped":true,"pending_play":false,"restart_requested":false,"identity":null});
        assert!(verify_cancel(&started,&stop,&drain).is_ok());
        for path in ["/control_cancelled", "/build_process/signaled", "/runtime_slot_present", "/status/ready", "/status/completed", "/identity"] {
            let mut bad = drain.clone(); *bad.pointer_mut(path).unwrap() = json!("missing or stale");
            assert!(verify_cancel(&started,&stop,&bad).is_err(), "{path}");
        }
        let mut unrelated = drain; unrelated["build_process"]["pid"] = json!(74);
        assert!(verify_cancel(&started,&stop,&unrelated).is_err());
    }

    #[test]
    fn history_settlement_requires_stability_and_preserves_all_unrelated_presentation() {
        let before = json!({"selection":{"map":"M1-SAMPLE","directory_map":"M1-SAMPLE","portal":null,
            "view":"Perspective","orthographic":false,"placement":"cube"},"camera":{"position":[1,2,3]},
            "roots":[],"headers":[],"navigation":{"yaw":1},"panes":{"width":232},"layout":[1309,994,100,200]});
        let mut after = before.clone();
        after["selection"]["placement"] = Value::Null;
        let valid = json!({"ready":true,"stable_frames":8,"stable_ms":250.,"before_history":before,"after_history":after,
            "selection_transitions":[{"elapsed_ms":1.,"selection":before["selection"]},{"elapsed_ms":2.,"selection":after["selection"]}]});
        verify_history_settlement(&valid, &before).unwrap();
        for (path, value) in [("/stable_frames", json!(7)), ("/stable_ms", json!(249.)),
            ("/after_history/camera/position/0", json!(999)), ("/after_history/layout/0", json!(999)),
            ("/selection_transitions/1/selection/placement", json!("other-object"))] {
            let mut bad = valid.clone();
            *bad.pointer_mut(path).unwrap() = value;
            assert!(verify_history_settlement(&bad, &before).is_err(), "{path}");
        }
    }
}
