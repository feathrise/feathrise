//! 独立交付包的真实进程验收；隐藏本轮源码、清除开发覆盖后核对生产场景与GPU截图。

use super::{package_evidence as package, read_json, write_new};
use crate::{build_process::{self, BuildControl}, project::Project};
use serde_json::{Value, json};
use std::{fs, io::Read, path::{Path, PathBuf}, process::Command, time::Duration};

/// Result错误在正常出口恢复，Rust展开异常也尽力恢复；强制终止时另有独占路径记录可恢复现场。
struct SourceRetirement { original: PathBuf, retired: PathBuf, active: bool }

impl Drop for SourceRetirement {
    fn drop(&mut self) {
        if self.active {
            if let Err(error) = fs::rename(&self.retired, &self.original) {
                eprintln!("FEATHRISE_EXPORT_VALIDATION_SOURCE_RESTORE_FAILED {error}；源码保留于{}", self.retired.display());
            }
        }
    }
}

fn isolated_environment(command: &mut Command) -> Result<Value, String> {
    let mut removed = Vec::new();
    for (name, _) in std::env::vars_os() {
        let upper = name.to_string_lossy().to_ascii_uppercase();
        if ["CARGO", "RUST", "EDITOR_", "FEATHRISE_", "NEW_EDITOR_", "BEVY_"].iter().any(|prefix| upper.starts_with(prefix)) {
            command.env_remove(&name);
            removed.push(name.to_string_lossy().into_owned());
        }
    }
    // 保留Windows自身与驱动所需环境；运行时搜索路径只提供系统目录，不继承工具链/Cargo的PATH。
    let path = if cfg!(windows) {
        let system = PathBuf::from(std::env::var_os("SystemRoot").ok_or("Windows缺少SystemRoot")?);
        std::env::join_paths([system.join("System32"), system]).map_err(|error| error.to_string())?
    } else { "/usr/bin:/bin".into() };
    command.env("PATH", &path);
    removed.sort();
    Ok(json!({"removed":removed,"path":path.to_string_lossy(),"toolchain_on_path":false}))
}

pub(super) fn run(work: &Path, delivered: &Path, marker: &Value, project: &Project, saved_author_tools: bool, control: &BuildControl) -> Result<Value, String> {
    // 重命名前再次证明路径和来源：这里只能使本轮独占夹具暂不可达，不接受普通工程。
    if crate::embedded_viewport::validation::code_iteration_owner(&project.root)? != *project
        || project.root.parent() != Some(work) || delivered.parent() != Some(work) || delivered.starts_with(&project.root) {
        return Err("独立运行验收的原工程或交付包不属于本轮工作根".into());
    }
    let retired = work.join("source-retired");
    if retired.try_exists().map_err(|error| error.to_string())? { return Err("本轮源码退休位置已存在，拒绝覆盖".into()); }
    let cwd = work.join("unrelated-cwd");
    fs::create_dir(&cwd).map_err(|error| error.to_string())?;
    let output = work.join("evidence/independent-game");
    fs::create_dir(&output).map_err(|error| error.to_string())?;
    let executable = PathBuf::from(marker["executable"].as_str().ok_or("标记缺少运行程序")?);
    package::relative(&executable)?;
    if executable.components().count() != 1 { return Err("独立运行入口必须位于包根".into()); }
    let mut command = Command::new(delivered.join(&executable));
    command.current_dir(&cwd);
    let environment = isolated_environment(&mut command)?;
    command.env("NEW_EDITOR_GAME_SMOKE_DIR", &output).env("NEW_EDITOR_GAME_SMOKE_SECONDS", "180");
    // 连续流程要求运行端读取刚保存的专业作者结果；原独立导出夹具未编辑这些域，保持原范围。
    if saved_author_tools { command.env("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS", "1"); }
    write_new(&output.join("source-retirement.json"), &serde_json::to_vec_pretty(&json!({
        "schema":1,"purpose":"owned-export-source-temporarily-unavailable","original":project.root,"retired":retired
    })).map_err(|error|error.to_string())?)?;
    fs::rename(&project.root, &retired).map_err(|error| format!("本轮源码暂时移走失败：{error}"))?;
    let mut retirement = SourceRetirement { original: project.root.clone(), retired: retired.clone(), active: true };
    // 无论真实进程成功或失败都先恢复自有源码，失败报告保留退休位置，绝不删除现场。
    let result = (|| -> Result<Value, String> {
        if project.root.try_exists().map_err(|error| error.to_string())? { return Err("原工程路径仍然可达，独立证据无效".into()); }
        let process = build_process::run(&command, control, Some(Duration::from_secs(240)), super::fixture::tail)?;
        write_new(&output.join("process.log"), format!("{}\n{}", process.stdout, process.stderr).as_bytes())?;
        if process.exit_code != 0 { return Err(format!("独立交付程序退出{}：{}", process.exit_code, process.stderr)); }
        let game = verify_game(delivered, &output, project)?;
        Ok(json!({"process_exit":process.exit_code,"executable":delivered.join(&executable),"cwd":cwd,
            "original_project_unreachable_during_process":true,"environment":environment,"game":game,
            "screenshot":output.join("game.png"),"os_input_verified":false,"toolchain_uninstalled_machine_verified":false}))
    })();
    let restored = fs::rename(&retired, &project.root);
    retirement.active = false;
    match (result, restored) {
        (Ok(mut proof), Ok(())) => { proof["original_project_restored"] = json!(true); Ok(proof) }
        (Err(error), Ok(())) => Err(error),
        (result, Err(error)) => Err(format!("独立运行结果：{}；本轮源码恢复失败：{error}；源码保留于{}",
            result.as_ref().err().map(String::as_str).unwrap_or("运行核对已通过"), retired.display())),
    }
}

fn verify_game(delivered: &Path, output: &Path, project: &Project) -> Result<Value, String> {
    let game = read_json(&output.join("game.json"), 1024 * 1024)?;
    crate::native_validation::verify_scene_view(&game)?;
    let roots = project.context.asset_roots.iter().map(|root| {
        let relative = root.strip_prefix(&project.root).map_err(|error| error.to_string())?;
        delivered.join(relative).canonicalize().map_err(|error| error.to_string())
    }).collect::<Result<Vec<_>, _>>()?;
    if game["passed"] != true || game["scene_id"] != "M1-SAMPLE" || game["runtime"]["phase"] != "Ready"
        || game["placements"]["phase"] != "Ready" || game["source"]["project_id"] != project.id
        || game["source"]["project_root"] != json!(delivered) || game["source"]["asset_roots"] != json!(roots)
        || roots.iter().any(|root| !root.starts_with(delivered)) {
        return Err(format!("独立进程未采用包内完整生产场景与资产根：{}", game["reason"]));
    }
    let files = game["source"]["files"].as_array().filter(|files| !files.is_empty() && files.len() <= 64)
        .ok_or("独立运行未提供有限完整的源文件指纹")?;
    for file in files {
        let relative = Path::new(file["path"].as_str().ok_or("独立源文件缺少路径")?);
        package::relative(relative)?;
        let actual = package::hash(&roots.first().ok_or("独立包未登记资产根")?.join(relative))?;
        if file["bytes"] != actual.bytes || file["sha256"] != actual.sha256 {
            return Err(format!("独立游戏源文件不是交付包字节：{}", relative.display()));
        }
    }
    let capture = &game["capture"];
    let width = capture["width"].as_u64().ok_or("GPU回执缺少宽度")?;
    let height = capture["height"].as_u64().ok_or("GPU回执缺少高度")?;
    let mut header = [0_u8; 24];
    fs::File::open(output.join("game.png")).and_then(|mut file|file.read_exact(&mut header)).map_err(|error|error.to_string())?;
    if capture["source"] != "Bevy ScreenshotCaptured" || width < 320 || height < 240
        || capture["bytes"].as_u64().unwrap_or(0) < width.saturating_mul(height).saturating_mul(3)
        || capture["sha256"].as_str().is_none_or(|hash|hash.len() != 64)
        || &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR"
        || u32::from_be_bytes(header[16..20].try_into().map_err(|_|"PNG宽度无效")?) as u64 != width
        || u32::from_be_bytes(header[20..24].try_into().map_err(|_|"PNG高度无效")?) as u64 != height {
        return Err("独立运行缺少与真实GPU回执匹配的PNG".into());
    }
    let file_count = files.len();
    let mut verified = game;
    verified["independently_verified_source_files"] = json!(file_count);
    verified["png_file_identity"] = serde_json::to_value(package::hash(&output.join("game.png"))?).map_err(|error|error.to_string())?;
    Ok(verified)
}
