//! 工程导出的外层自有夹具协调器；完整扫描、进程等待和源目录暂退均不进入Bevy帧。

use super::{OWNER_FILE, PURPOSE, custom, package_evidence as package, read_json, resources::Scenario, standalone, write_new};
use crate::{build_process::{self, BuildControl}, embedded_viewport::validation as embedded};
use serde_json::{Value, json};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, process::Command, time::{Duration, Instant}};

pub(crate) fn run(target: PathBuf, report: PathBuf) -> Result<(), String> {
    let selected = Scenario::from_environment()?;
    if !report.is_absolute() || report.exists() { return Err("导出报告必须为本轮新的绝对路径".into()); }
    fs::create_dir_all(report.parent().ok_or("导出报告缺少父目录")?).map_err(|error|error.to_string())?;
    let work = report.with_extension("work");
    fs::create_dir(&work).map_err(|error|format!("不能独占创建导出验收工作根：{error}"))?;
    let work = work.canonicalize().map_err(|error|error.to_string())?;
    write_new(&work.join(".feathrise-native-validation-owner.json"), &serde_json::to_vec_pretty(&json!({
        "schema":1,"purpose":"ordinary-author-boundary-validation","work_root":work
    })).map_err(|error|error.to_string())?)?;
    let started = Instant::now();
    let mut evidence = json!({"work":work,"scenario":selected.label()});
    let result = scenario(&target, &work, selected, &mut evidence);
    // 失败同样留下已完成阶段和本轮工作根；既有报告不会被覆盖，也不删除失败现场。
    write_new(&report, &serde_json::to_vec_pretty(&json!({"schema":1,"scope":"owned-project-folder-export",
        "passed":result.is_ok(),"scenario":selected.label(),"elapsed_ms":started.elapsed().as_secs_f64()*1000.,"evidence":evidence,
        "error":result.as_ref().err(),"os_input_verified":false,
        "not_verified":["physical mouse and OS folder picker","machine with Rust/Cargo uninstalled","other GPU and Windows versions",
            "real-window cancellation during file copy or atomic publish"]
    })).map_err(|error|error.to_string())?)?;
    result
}

fn scenario(target: &Path, work: &Path, selected: Scenario, evidence: &mut Value) -> Result<(), String> {
    let repository = crate::project_template_root().canonicalize().map_err(|error|error.to_string())?;
    let label = "导出自有工程";
    let root = crate::project_template::create_with_control(&work.join(label), label, &repository, &BuildControl::default())?;
    evidence["project_root"] = json!(root);
    crate::native_validation::prepare_workspace_fixture(&root, label)?;
    let project = embedded::code_iteration_owner(&root)?;
    // 任何源码和启动设置都在打开作者窗口前准备；窗口中的源码监听不应因验收自身改动撤销导出。
    let startup = root.join(crate::startup_document::FILE_NAME);
    let mut document = if startup.try_exists().map_err(|error|error.to_string())? {
        crate::startup_document::parse(&fs::read(&startup).map_err(|error|error.to_string())?, &project.id)?
    } else { crate::startup_document::Document::empty(&project.id) };
    document.select_map("M1-SAMPLE")?;
    let startup_bytes = crate::startup_document::to_bytes(&document, &project.id)?;
    fs::write(&startup, &startup_bytes).map_err(|error|error.to_string())?;
    let descriptor_bytes = fs::read(root.join("editor-project.json")).map_err(|error|error.to_string())?;
    let output = work.join("evidence");
    fs::create_dir(&output).map_err(|error|error.to_string())?;
    let child_report = output.join("editor-report.json");
    write_new(&root.join(OWNER_FILE), &serde_json::to_vec_pretty(&json!({
        "schema":1,"purpose":PURPOSE,"project_id":project.id,"project_root":root,"report":child_report,
        "fixture_source":"formal-empty-template+owned-neutral-author-fixture","scenario":selected.label()
    })).map_err(|error|error.to_string())?)?;
    let before = embedded::code_iteration_asset_hashes(&root)?;
    evidence["asset_files_before"] = json!(before.len());
    let mut command = Command::new(std::env::current_exe().map_err(|error|error.to_string())?);
    clear_other_validation(&mut command);
    command.args(["--native-editor", "--project"]).arg(&root).arg("--target-dir").arg(target)
        .env("FEATHRISE_EXPORT_VALIDATION_REPORT", &child_report)
        .env("FEATHRISE_EXPORT_VALIDATION_SCENARIO", selected.label())
        .env("FEATHRISE_PREFERENCES_FILE", output.join("isolated-preferences.json"));
    crate::native_validation::isolate_author_command(&mut command, &output)?;
    println!("FEATHRISE_EXPORT_VALIDATION_COORDINATOR editor {}", root.display());
    let budget = if selected == Scenario::CustomCancelRetry { 5700 } else { 2100 };
    let process = build_process::run(&command, &BuildControl::default(), Some(Duration::from_secs(budget)), tail)?;
    write_new(&output.join("editor-process.log"), format!("{}\n{}", process.stdout, process.stderr).as_bytes())?;
    evidence["editor_process_exit"] = json!(process.exit_code);
    let after = embedded::code_iteration_asset_hashes(&root)?;
    evidence["all_author_asset_bytes_preserved"] = json!(before == after);
    evidence["startup_bytes_preserved"] = json!(fs::read(&startup).map_err(|error|error.to_string())? == startup_bytes);
    // 提前退出或报告损坏也保留独立资产保护核对，不能让“缺报告”掩盖已经可确认的事实。
    let child = read_json(&child_report, 1024 * 1024)
        .map_err(|error|format!("作者验收进程退出{}，未取得有效完成报告：{error}；查看{}", process.exit_code, output.join("editor-process.log").display()))?;
    evidence["editor"] = child.clone();
    if process.exit_code != 0 || before != after || evidence["startup_bytes_preserved"] != true {
        return Err("原作者导出进程退出、资产字节或启动设置保护检查失败".into());
    }
    verify_editor(&child, &root, &project.id, &output, selected)?;
    let exported = root.join(if selected == Scenario::CustomCancelRetry { custom::DIRECTORY } else { "output" });
    let (inventory, marker) = package::verify(&exported, &project.id, &project.package)?;
    evidence["default_output"] = json!(root.join("output"));
    evidence["exported_directory"] = json!(exported);
    evidence["package"] = package::summary(&inventory);
    if selected == Scenario::CustomCancelRetry {
        evidence["custom_checkpoints"] = verify_custom_checkpoints(&child, &root, &project.id, &output, &inventory, &marker)?;
        let (default_inventory, _) = package::verify(&root.join("output"), &project.id, &project.package)?;
        evidence["default_package_still_valid"] = package::summary(&default_inventory);
    }
    if fs::read(exported.join("editor-project.json")).map_err(|error|error.to_string())? != descriptor_bytes
        || fs::read(exported.join(crate::startup_document::FILE_NAME)).map_err(|error|error.to_string())? != startup_bytes {
        return Err("导出描述或启动设置没有保留原字节".into());
    }
    let delivered = work.join("delivered 独立包");
    package::copy_to(&exported, &delivered, &inventory)?;
    let delivered = delivered.canonicalize().map_err(|error|error.to_string())?;
    let (copied, _) = package::verify(&delivered, &project.id, &project.package)?;
    if copied != inventory { return Err("搬移后的交付包字节身份变化".into()); }
    evidence["delivered"] = json!(delivered);
    evidence["copied_outside_project_and_all_bytes_verified"] = json!(true);
    println!("FEATHRISE_EXPORT_VALIDATION_COORDINATOR standalone {}", delivered.display());
    let independent = standalone::run(work, &delivered, &marker, &project)?;
    // GPU中出现的实例只允许来自已保存地图，不能把窗口中未保存立方体偷偷打进包。
    let unsaved = child["stages"].as_array().and_then(|stages|stages.iter().find(|step|step["stage"] == "unsaved_author_draft_prepared"))
        .ok_or("原窗口缺少未保存草稿证据")?;
    let id = unsaved["author"]["last_added_id"].as_str().ok_or("未保存立方体缺少真实ID")?;
    if independent["game"]["placement_instances"].as_array().is_none_or(|instances|instances.iter().any(|instance|instance["id"] == id)) {
        return Err("独立场景缺少实例证据或包含作者未保存对象".into());
    }
    evidence["independent"] = independent;
    if embedded::code_iteration_owner(&root)? != project { return Err("独立运行后原工程身份未恢复".into()); }
    Ok(())
}

fn clear_other_validation(command: &mut Command) {
    for (name, _) in std::env::vars_os() {
        let upper = name.to_string_lossy().to_ascii_uppercase();
        if upper.starts_with("NEW_EDITOR_") || upper.starts_with("EDITOR_") || upper.starts_with("FEATHRISE_WORKSPACE_")
            || (upper.starts_with("FEATHRISE_") && ["SMOKE", "VALIDATION", "REPORT", "CODE_ITERATION"].iter().any(|part|upper.contains(part))) {
            command.env_remove(name);
        }
    }
}

fn verify_editor(child: &Value, root: &Path, id: &str, output: &Path, selected: Scenario) -> Result<(), String> {
    let expected = root.join(if selected == Scenario::CustomCancelRetry { custom::DIRECTORY } else { "output" });
    if child["schema"] != 1 || child["scope"] != "owned-project-folder-export-editor" || child["passed"] != true
        || child["project_id"] != id || child["project_root"] != json!(root) || child["scenario"] != selected.label()
        || child["receipt"]["directory"] != json!(expected) {
        return Err(format!("原作者导出验收未通过：{}", child["error"]));
    }
    let stages = child["stages"].as_array().ok_or("原作者导出缺少阶段证据")?;
    for required in ["unsaved_author_draft_prepared", "original_project_menu_pressed", "original_export_menu_pressed",
        "default_output_and_actual_dialog_gpu", "original_start_export_pressed", "real_worker_completed_and_author_preserved", "actual_success_dialog_gpu"] {
        if !stages.iter().any(|stage|stage["stage"] == required) { return Err(format!("导出验收缺少实际阶段：{required}")); }
    }
    let mut screenshots = vec!["export-dialog.png", "export-completed.png"];
    if selected == Scenario::CustomCancelRetry {
        verify_custom_stages(child, root)?;
        screenshots.extend(["custom-export-dialog.png", "custom-export-created.png", "custom-export-cancelled.png", "custom-export-completed.png"]);
    }
    for name in screenshots {
        let path = output.join(name);
        if !path.is_file() || package::hash(&path)?.bytes < 1000 { return Err(format!("实际导出窗口截图缺失：{name}")); }
    }
    Ok(())
}

fn step<'a>(stages: &'a [Value], required: &str) -> Result<&'a Value, String> {
    let mut found = stages.iter().filter(|stage|stage["stage"] == required);
    let step = found.next().ok_or_else(||format!("自选取消重试验收缺少实际阶段：{required}"))?;
    if found.next().is_some() { return Err(format!("自选取消重试阶段重复：{required}")); }
    Ok(step)
}

/// 场景选择是验收合同：不能只完成默认导出就用旧回执宣称自选、取消及重试已经通过。
fn verify_custom_stages(child: &Value, root: &Path) -> Result<(), String> {
    let stages = child["stages"].as_array().filter(|stages|stages.len() <= 64).ok_or("附加阶段缺失或超过64项预算")?;
    let expected = root.join(custom::DIRECTORY);
    let mut previous = None;
    for required in custom::REQUIRED_STAGES {
        step(stages, required)?;
        let index = stages.iter().position(|stage|stage["stage"] == *required).ok_or("附加阶段索引缺失")?;
        if previous.is_some_and(|previous|index <= previous) { return Err("自选、取消和重试的实际阶段顺序不完整".into()); }
        previous = Some(index);
    }
    let queued = step(stages, "original_custom_destination_edits_queued")?;
    if queued["input"] != "original EditableText SelectAll + Insert" || queued["destination"] != crate::project::display_path(&expected) {
        return Err("自选路径没有经过本轮原EditableText编辑队列".into());
    }
    let cancellation = step(stages, "original_cancel_export_pressed_while_real_worker_running")?;
    let drained = step(stages, "cancelled_worker_really_drained_and_author_preserved")?;
    if cancellation["active_slot"] != true || cancellation["handle_finished"] != false
        || cancellation["control_cancelled_before"] != false || cancellation["status"].as_str().is_none_or(|status|!status.starts_with("正在构建"))
        || drained["control_cancelled"] != true || drained["cleanup_failed"] != false || drained["active_slot"] != false || drained["working"] != false
        || drained.get("receipt") != Some(&Value::Null) || drained.get("failed") != Some(&Value::Null)
        || drained["status"].as_str().is_none_or(|status|!status.starts_with("导出已取消")) {
        return Err("取消没有提供真实活动控制牌、线程归槽和无成功回执的完整证据".into());
    }
    let baseline = &step(stages, "unsaved_author_draft_prepared")?["author"];
    let first = step(stages, "custom_export_completed_and_author_preserved")?;
    let retry = step(stages, "retry_export_completed_and_author_preserved")?;
    if baseline.is_null() || first["author"] != *baseline || drained["author"] != *baseline || retry["author"] != *baseline
        || first["receipt"]["directory"] != json!(expected) || retry["receipt"]["directory"] != json!(expected)
        || retry["receipt"] != child["receipt"] || step(stages, "original_retry_export_start_after_drain")?["active_slot_before"] != false {
        return Err("自选或重试未保持作者现场，或最终回执没有绑定取消收口后的新任务".into());
    }
    let before = &step(stages, "complete_old_package_captured_before_cancel")?["package"]["export_id"];
    let after = step(stages, "complete_old_package_unchanged_after_cancel")?;
    let fresh = step(stages, "complete_retry_package_verified_with_new_export_id")?;
    if after["entire_inventory_equal"] != true || after["export_id_unchanged"] != true || after["package"]["export_id"] != *before
        || fresh["previous_export_id"] != *before || fresh["new_export_id"] == *before || fresh["new_export_id"] != fresh["package"]["export_id"] {
        return Err("取消未保留完整旧包，或再次导出沿用了旧发布身份".into());
    }
    for id in [before, &fresh["new_export_id"]] {
        bevy::asset::uuid::Uuid::parse_str(id.as_str().ok_or("发布身份缺失")?).map_err(|_|"发布身份不是UUID")?;
    }
    Ok(())
}

fn verify_custom_checkpoints(
    child: &Value, root: &Path, id: &str, output: &Path, inventory: &package::Inventory, marker: &Value,
) -> Result<Value, String> {
    let stages = child["stages"].as_array().ok_or("缺少自选阶段证据")?;
    let mut checkpoints = Vec::new();
    for (name, stage) in [("before-cancel", "complete_old_package_captured_before_cancel"),
        ("after-cancel", "complete_old_package_unchanged_after_cancel"), ("after-retry", "complete_retry_package_verified_with_new_export_id")] {
        let path = output.join(format!("custom-{name}-package.json"));
        let checkpoint = read_json(&path, 32 * 1024 * 1024)?;
        let proof = &step(stages, stage)?["package"];
        if checkpoint["schema"] != 1 || checkpoint["checkpoint"] != name || checkpoint["project_id"] != id
            || checkpoint["directory"] != json!(root.join(custom::DIRECTORY)) || proof["path"] != json!(path)
            || proof["file_identity"] != serde_json::to_value(package::hash(&path)?).map_err(|error|error.to_string())?
            || proof["export_id"] != checkpoint["marker"]["export_id"] {
            return Err(format!("自选包独立检查点未匹配真实报告与文件身份：{name}"));
        }
        checkpoints.push(checkpoint);
    }
    if checkpoints[0]["package"] != checkpoints[1]["package"] || checkpoints[0]["marker"] != checkpoints[1]["marker"]
        || checkpoints[2]["package"] != package::summary(inventory) || checkpoints[2]["marker"] != *marker
        || checkpoints[0]["marker"]["export_id"] == checkpoints[2]["marker"]["export_id"] {
        return Err("外层核对发现取消前后完整旧包不一致，或重试检查点不是最终新发布包".into());
    }
    Ok(json!({"all_checkpoint_files_verified":true,"entire_old_inventory_unchanged_after_cancel":true,
        "old_export_id":checkpoints[0]["marker"]["export_id"],"retry_export_id":checkpoints[2]["marker"]["export_id"],
        "retry_checkpoint_matches_actual_final_package":true,"copy_or_publish_cancellation_verified":false}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> (PathBuf, Value) {
        let root = PathBuf::from("D:/owned-export-contract");
        let mut stages = vec![json!({"stage":"unsaved_author_draft_prepared","author":{"dirty":true,"revision":2,"undo_len":1}})];
        stages.extend(custom::REQUIRED_STAGES.iter().map(|stage|json!({"stage":stage})));
        let mut child = json!({"stages":stages,"receipt":{"directory":root.join(custom::DIRECTORY)}});
        let author = child["stages"][0]["author"].clone();
        for stage in child["stages"].as_array_mut().unwrap() {
            match stage["stage"].as_str().unwrap() {
                "original_custom_destination_edits_queued" => {
                    stage["input"] = json!("original EditableText SelectAll + Insert");
                    stage["destination"] = json!(crate::project::display_path(&root.join(custom::DIRECTORY)));
                }
                "original_cancel_export_pressed_while_real_worker_running" => {
                    stage["active_slot"] = json!(true); stage["handle_finished"] = json!(false);
                    stage["control_cancelled_before"] = json!(false); stage["status"] = json!("正在构建Debug工程…");
                }
                "cancelled_worker_really_drained_and_author_preserved" => {
                    stage["control_cancelled"] = json!(true); stage["cleanup_failed"] = json!(false); stage["active_slot"] = json!(false);
                    stage["working"] = json!(false); stage["receipt"] = Value::Null; stage["failed"] = Value::Null;
                    stage["status"] = json!("导出已取消，原输出与作者草稿保留"); stage["author"] = author.clone();
                }
                "custom_export_completed_and_author_preserved" | "retry_export_completed_and_author_preserved" => {
                    stage["receipt"] = json!({"directory":root.join(custom::DIRECTORY)}); stage["author"] = author.clone();
                }
                "original_retry_export_start_after_drain" => stage["active_slot_before"] = json!(false),
                "complete_old_package_captured_before_cancel" | "complete_old_package_unchanged_after_cancel" => {
                    stage["package"] = json!({"export_id":"00000000-0000-4000-8000-000000000001"});
                    stage["entire_inventory_equal"] = json!(true); stage["export_id_unchanged"] = json!(true);
                }
                "complete_retry_package_verified_with_new_export_id" => {
                    stage["previous_export_id"] = json!("00000000-0000-4000-8000-000000000001");
                    stage["new_export_id"] = json!("00000000-0000-4000-8000-000000000002");
                    stage["package"] = json!({"export_id":"00000000-0000-4000-8000-000000000002"});
                }
                _ => (),
            }
        }
        (root, child)
    }

    #[test]
    fn custom_contract_rejects_missing_cancel_stage_even_with_final_receipt() {
        let (root, mut child) = contract();
        verify_custom_stages(&child, &root).unwrap();
        child["stages"].as_array_mut().unwrap().retain(|stage|stage["stage"] != "original_cancel_export_pressed_while_real_worker_running");
        assert!(verify_custom_stages(&child, &root).unwrap_err().contains("original_cancel_export_pressed"));
    }

    #[test]
    fn custom_contract_rejects_old_export_id_and_cancel_success_receipt() {
        let (root, mut child) = contract();
        let fresh = child["stages"].as_array_mut().unwrap().iter_mut().find(|stage|stage["stage"] == "complete_retry_package_verified_with_new_export_id").unwrap();
        fresh["new_export_id"] = fresh["previous_export_id"].clone();
        assert!(verify_custom_stages(&child, &root).unwrap_err().contains("旧发布身份"));
        let (root, mut child) = contract();
        let drained = child["stages"].as_array_mut().unwrap().iter_mut().find(|stage|stage["stage"] == "cancelled_worker_really_drained_and_author_preserved").unwrap();
        drained["receipt"] = json!({"directory":root.join(custom::DIRECTORY)});
        assert!(verify_custom_stages(&child, &root).unwrap_err().contains("无成功回执"));
    }
}

pub(super) fn tail(mut input: fs::File) -> Result<String, String> {
    let mut retained = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = input.read(&mut buffer).map_err(|error|error.to_string())?;
        if count == 0 { break; }
        std::io::stdout().write_all(&buffer[..count]).map_err(|error|error.to_string())?;
        retained.extend_from_slice(&buffer[..count]);
        if retained.len() > 128 * 1024 { retained.drain(..retained.len() - 128 * 1024); }
    }
    Ok(String::from_utf8_lossy(&retained).into_owned())
}
