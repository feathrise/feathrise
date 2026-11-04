//! 独占创建本轮中性模板并安装实际相机组件取证；不接收或改写普通作者工程。

use super::{OWNER_FILE, PURPOSE};
use crate::{build_process::{self, BuildControl}, embedded_viewport::validation as embedded};
use serde_json::{Value, json};
use std::{fs, io::{Read, Write}, path::{Path, PathBuf}, process::Command, time::{Duration, Instant}};

const CONTROLLER_PROBE: &str = r#"

// 仅本轮独占夹具安装：读取真实FreeCamera组件与已采用World身份，不用源码文本替代运行证据。
fn code_iteration_runtime_probe(
    cameras: Query<(Entity, &FreeCamera), With<PrimarySceneView>>,
    state: Option<Res<EmbeddedProjectState>>,
    mut previous: Local<Option<(u64, u64, u64)>>,
) {
    let (Ok((entity, controls)), Some(state)) = (cameras.single(), state) else { return; };
    let key = (state.world_id, state.revision, entity.to_bits());
    if *previous == Some(key) { return; }
    *previous = Some(key);
    let pid = std::process::id();
    let proof = format!("{{\"source\":\"actual-primary-scene-FreeCamera-component\",\"pid\":{},\"world_id\":{},\"revision\":{},\"entity\":{},\"walk_speed\":{},\"run_speed\":{}}}",
        pid, state.world_id, state.revision, entity.to_bits(), controls.walk_speed, controls.run_speed);
    println!("FEATHRISE_CODE_ITERATION_CAMERA {proof}");
    let Some(directory) = std::env::var_os("FEATHRISE_CODE_ITERATION_RUNTIME_DIR") else { return; };
    let path = std::path::PathBuf::from(directory).join(format!("{}-{}-{}.json", pid, state.world_id, state.revision));
    std::thread::spawn(move || {
        use std::io::Write;
        match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(mut file) => { if let Err(error) = file.write_all(proof.as_bytes()) { eprintln!("相机组件取证失败：{error}"); } }
            Err(error) => eprintln!("相机组件取证不能独占创建：{error}"),
        }
    });
}
"#;

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    fs::OpenOptions::new().write(true).create_new(true).open(path)
        .and_then(|mut file| file.write_all(bytes)).map_err(|error| format!("本轮证据不能独占写入{}：{error}", path.display()))
}

fn replace_once(source: String, before: &str, after: &str) -> Result<String, String> {
    if source.matches(before).count() != 1 { return Err(format!("正式模板取证锚点不是唯一：{before}")); }
    Ok(source.replacen(before, after, 1))
}

fn install_probe(root: &Path) -> Result<(), String> {
    embedded::code_iteration_owner(root)?;
    let path = root.join("src/controller.rs");
    let source = fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let source = replace_once(source, "pub struct ProjectScenePlugin", "const CODE_ITERATION_WALK_SPEED: f32 = 12.0;\n\npub struct ProjectScenePlugin")?;
    let source = replace_once(source, ".add_systems(Update, synchronize_observer.in_set(ProjectGameplay));",
        ".add_systems(Update, synchronize_observer.in_set(ProjectGameplay))\n            .add_systems(Update, code_iteration_runtime_probe.after(initialize_view));")?;
    let source = if source.contains("walk_speed: walk_speed()") {
        let source = replace_once(source, "walk_speed: walk_speed()", "walk_speed: CODE_ITERATION_WALK_SPEED")?;
        let source = replace_once(source, "fn walk_speed()", "fn original_walk_speed()")?;
        format!("{source}\nfn walk_speed() -> f32 {{ CODE_ITERATION_WALK_SPEED }}\n")
    } else {
        replace_once(source, "FreeCamera::default(), controls",
            "FreeCamera { walk_speed: CODE_ITERATION_WALK_SPEED, ..default() }, controls")?
    };
    fs::write(&path, format!("{source}{CONTROLLER_PROBE}")).map_err(|error| error.to_string())?;
    write_new(&root.join("src/author_notes.rs"), "// 作者文本草稿：基线\n".as_bytes())
}

fn configure_function_probe(root: &Path, enabled: bool) -> Result<(), String> {
    // 只修改刚创建且已核验归属的夹具，不替普通作者开启功能或重写其清单。
    embedded::code_iteration_owner(root)?;
    let path = root.join("Cargo.toml");
    let mut manifest: toml::Value = toml::from_str(&fs::read_to_string(&path).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let mut current = &mut manifest;
    for key in ["package", "metadata", "feathrise", "hotpatch"] {
        current = current.as_table_mut().ok_or("自有夹具清单层级不是表")?
            .entry(key).or_insert_with(|| toml::Value::Table(Default::default()));
    }
    let settings = current.as_table_mut().ok_or("自有夹具热更新合同不是表")?;
    settings.insert("enabled".into(), toml::Value::Boolean(enabled));
    if enabled {
        settings.insert("functions".into(), toml::Value::Array(vec![toml::Value::String("src/controller.rs::live_step".into())]));
        let features = manifest.as_table_mut().unwrap().entry("features")
            .or_insert_with(|| toml::Value::Table(Default::default())).as_table_mut().ok_or("夹具features不是表")?;
        features.entry("function_hotpatch").or_insert_with(|| toml::Value::Array(vec![toml::Value::String("feathrise_runtime/function_hotpatch".into())]));
        let defaults = features.entry("default").or_insert_with(|| toml::Value::Array(Vec::new()))
            .as_array_mut().ok_or("夹具默认feature不是数组")?;
        if !defaults.iter().any(|value| value.as_str() == Some("function_hotpatch")) {
            defaults.push(toml::Value::String("function_hotpatch".into()));
        }
        let controller = root.join("src/controller.rs");
        let source = fs::read_to_string(&controller).map_err(|error| error.to_string())?;
        let source = replace_once(source, ".add_systems(Update, code_iteration_runtime_probe.after(initialize_view));",
            ".add_systems(Update, code_iteration_runtime_probe.after(initialize_view))\n            .add_systems(Startup, initialize_live_probe)\n            .add_systems(First, verify_live_probe_world)\n            .add_systems(Update, advance_live_probe.in_set(ProjectGameplay))\n            .add_systems(Update, present_live_probe.after(advance_live_probe).after(initialize_view))\n            .add_systems(Update, report_live_probe.after(present_live_probe));")?;
        fs::write(controller, format!("{source}{}", super::function_hotpatch::PROBE_SOURCE)).map_err(|error| error.to_string())?;
    }
    fs::write(path, toml::to_string(&manifest).map_err(|error| error.to_string())?).map_err(|error| error.to_string())
}

/// 外层不运行Bevy帧；创建、全资产摘要、子进程等待和落盘核对均在专用协调入口完成。
pub(crate) fn run(target: PathBuf, report: PathBuf) -> Result<(), String> {
    let report = if report.is_absolute() { report } else {
        std::env::current_dir().map_err(|error| error.to_string())?.join(report)
    };
    if report.exists() { return Err("代码迭代报告已存在，请指定本轮新的文件名".into()); }
    fs::create_dir_all(report.parent().ok_or("代码迭代报告缺少父目录")?).map_err(|error| error.to_string())?;
    let work = report.with_extension("work");
    fs::create_dir(&work).map_err(|error| format!("不能独占创建代码迭代工作目录：{error}"))?;
    let work = work.canonicalize().map_err(|error| error.to_string())?;
    write_new(&work.join(".feathrise-native-validation-owner.json"), &serde_json::to_vec_pretty(&json!({
        "schema":1,"purpose":"ordinary-author-boundary-validation","work_root":work
    })).map_err(|error| error.to_string())?)?;
    let started = Instant::now();
    let result = (|| -> Result<Value, String> {
        let repository = crate::project_template_root().canonicalize().map_err(|error| error.to_string())?;
        let root = crate::project_template::create_with_control(&work.join("代码迭代自有工程"), "代码迭代自有工程", &repository, &BuildControl::default())?;
        crate::native_validation::prepare_workspace_fixture(&root, "代码迭代自有工程")?;
        install_probe(&root)?;
        let function_hotpatch = std::env::var_os("FEATHRISE_CODE_ITERATION_FUNCTION_HOTPATCH").is_some_and(|value| value == "1");
        configure_function_probe(&root, function_hotpatch)?;
        let project = embedded::code_iteration_owner(&root)?;
        let output = work.join("evidence");
        fs::create_dir(&output).map_err(|error| error.to_string())?;
        fs::create_dir(output.join("game")).map_err(|error| error.to_string())?;
        fs::create_dir(output.join("runtime")).map_err(|error| error.to_string())?;
        let child_report = output.join("editor-report.json");
        write_new(&root.join(OWNER_FILE), &serde_json::to_vec_pretty(&json!({
            "schema":1,"purpose":PURPOSE,"project_id":project.id,"project_root":root,"report":child_report,
            "fixture_source":"formal-empty-template+owned-neutral-author-fixture","source_probe":"actual FreeCamera component"
        })).map_err(|error| error.to_string())?)?;
        let before = embedded::code_iteration_asset_hashes(&root)?;
        let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
        command.args(["--native-editor", "--project"]).arg(&root).arg("--target-dir").arg(&target)
            .env("FEATHRISE_CODE_ITERATION_REPORT", &child_report)
            .env("FEATHRISE_CODE_ITERATION_RUNTIME_DIR", output.join("runtime"))
            .env("NEW_EDITOR_EMBEDDED_GAME_SMOKE_DIR", output.join("game"))
            .env("FEATHRISE_PREFERENCES_FILE", output.join("isolated-preferences.json"))
            .env_remove("FEATHRISE_EMBEDDED_SMOKE_DIR")
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_DIR")
            .env_remove("NEW_EDITOR_GAME_SMOKE_DIR")
            .env_remove("FEATHRISE_WORKSPACE_SMOKE_DIR");
        crate::native_validation::isolate_author_command(&mut command, &output)?;
        let process = build_process::run(&command, &BuildControl::default(), Some(Duration::from_secs(2400)), tail)?;
        write_new(&output.join("process.log"), format!("{}\n{}", process.stdout, process.stderr).as_bytes())?;
        let mut child: Value = serde_json::from_slice(&fs::read(&child_report)
            .map_err(|error| format!("未生成代码迭代子报告：{error}；退出{}；{}", process.exit_code, process.stderr))?)
            .map_err(|error| error.to_string())?;
        let after = embedded::code_iteration_asset_hashes(&root)?;
        let controller = fs::read_to_string(root.join("src/controller.rs")).map_err(|error| error.to_string())?;
        let note = fs::read_to_string(root.join("src/author_notes.rs")).map_err(|error| error.to_string())?;
        child["process_exit"] = json!(process.exit_code);
        child["all_author_asset_bytes_preserved"] = json!(before == after);
        child["asset_files_checked"] = json!(before.len());
        child["saved_source_is_36"] = json!(controller.contains("const CODE_ITERATION_WALK_SPEED: f32 = 36.0;"));
        child["function_hotpatch_selected"] = json!(function_hotpatch);
        child["saved_live_step_is_3"] = json!(controller.contains("fn live_step() -> f32 { 3.0 }"));
        child["unsaved_note_disk_unchanged"] = json!(note == "// 作者文本草稿：基线\n");
        if process.exit_code != 0 || before != after
            || (if function_hotpatch { child["saved_live_step_is_3"] != true } else { child["saved_source_is_36"] != true })
            || child["unsaved_note_disk_unchanged"] != true { child["passed"] = json!(false); }
        write_new(&output.join("verified-editor-report.json"), &serde_json::to_vec_pretty(&child).map_err(|error| error.to_string())?)?;
        if child["passed"] != true {
            // 完整阶段和源码探针已独占落盘；命令行只给失败原因，避免把整份报告再次塞进日志尾部。
            return Err(format!("代码迭代窗口验收未通过：{}；完整证据：{}",
                child["error"].as_str().unwrap_or("进程退出、资产字节或源码落盘核对失败"),
                output.join("verified-editor-report.json").display()));
        }
        Ok(child)
    })();
    write_new(&report, &serde_json::to_vec_pretty(&json!({"schema":1,"scope":if std::env::var_os("FEATHRISE_CODE_ITERATION_FUNCTION_HOTPATCH").is_some_and(|value| value == "1") {"owned-function-hotpatch-formal-preview"} else {"owned-code-iteration-first-stage"},
        "passed":result.is_ok(),"elapsed_ms":started.elapsed().as_secs_f64()*1000.,"work":work,
        "result":result.as_ref().ok(),"error":result.as_ref().err()})).map_err(|error| error.to_string())?)?;
    result.map(|_| ())
}

fn tail(mut input: fs::File) -> Result<String, String> {
    let mut retained = Vec::new();
    let mut bytes = [0_u8; 8192];
    loop {
        let count = input.read(&mut bytes).map_err(|error| error.to_string())?;
        if count == 0 { break; }
        // 专项协调进程实时转发自有子窗口的阶段日志，避免长编译或阶段失败只能等退出后才看见。
        std::io::stdout().write_all(&bytes[..count]).map_err(|error| error.to_string())?;
        retained.extend_from_slice(&bytes[..count]);
        if retained.len() > 128 * 1024 { retained.drain(..retained.len() - 128 * 1024); }
    }
    Ok(String::from_utf8_lossy(&retained).into_owned())
}
