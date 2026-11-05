//! 不同World首版的自有工程窗口验收；只在显式--native-report时运行，不操作作者工程或桌面输入。
#[path = "native_author_tools_validation.rs"]
mod author_tools;
#[path = "native_boundary_validation.rs"]
mod boundary;
#[path = "native_validation_fixture.rs"]
mod fixture;
#[path = "native_validation_environment.rs"]
mod environment;
pub(crate) use environment::isolate_author_command;
use crate::{
    build_process::{self, BuildControl},
    project, project_template,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

/// 同窗口验收复用相同中性资产，父级独占标记和新空骨架仍由夹具内部严格核验。
pub(crate) fn prepare_workspace_fixture(root: &Path, label: &str) -> Result<(), String> {
    fixture::prepare(root, label)
}

// 此历史专项包含Win32工程子窗口对比，Unix实现不能据此冒充已通过双视口验收。
#[cfg(windows)]
pub(crate) fn run(target: PathBuf, report: PathBuf) -> Result<(), String> {
    let report = if report.is_absolute() {
        report
    } else {
        std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(report)
    };
    if report.exists() {
        return Err("验收报告已存在，请为新一轮选择不同文件名；历史证据不覆盖".into());
    }
    let parent = report.parent().ok_or("验收报告缺少父目录")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let work = report.with_extension("work");
    fs::create_dir(&work).map_err(|e| format!("验收自有目录不可创建（不会覆盖既有工程）：{e}"))?;
    let work = work.canonicalize().map_err(|e| e.to_string())?;
    // 夹具只改本轮create_dir独占的工程；显式租约不能由正常新建工程或作者目录隐式获得。
    {
        use std::io::Write;
        let mut owner = fs::OpenOptions::new().write(true).create_new(true)
            .open(work.join(".feathrise-native-validation-owner.json"))
            .map_err(|error| format!("无法独占标记验收目录：{error}"))?;
        owner.write_all(&serde_json::to_vec_pretty(&json!({
            "schema":1,"purpose":"ordinary-author-boundary-validation","work_root":work
        })).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    }
    let repository = crate::project_template_root()
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let control = BuildControl::default();
    let before = protected_files(&repository)?;
    let started = Instant::now();
    let export_lifecycle = std::env::var_os("FEATHRISE_NATIVE_EXPORT").as_deref() == Some(std::ffi::OsStr::new("1"));
    let mut steps = Vec::new();
    let result = (|| -> Result<(), String> {
        // 旧分离管理器已随原型归档。本轮只验证当前作者实例和保存后普通游戏，
        // 不再以历史管理器的截图冒充正式同窗口工程管理验收。
        let mut roots = Vec::new();
        for label in ["独立工程 A", "独立工程 B"] {
            let root = project_template::create_with_control(
                &work.join(label),
                label,
                &repository,
                &control,
            )?;
            fixture::prepare(&root, label)?;
            roots.push(root);
        }
        steps.push(json!({"stage":"create_empty_projects_and_install_owned_neutral_fixtures","passed":true,"roots":roots,"default_template_contains_sample_content":false}));
        for (index, root) in roots.iter().enumerate() {
            // A/B来源由真实工程根、资产根、地图和保存后的实例证明，不再制造原RPG开局文件。
            let dependencies = boundary::verify(root, &control)?;
            steps.push(json!({"stage":"production_template_dependency_boundary","index":index,"passed":true,"report":dependencies}));
        }
        let recovery_project = project::open(&roots[0])?;
        author_tools::prepare(&roots[0])?;
        // 仅本轮create_dir独占创建的工程带故障注入租约，普通作者目录不能启用验收改写。
        fs::write(
            recovery_project
                .root
                .join(".new-editor-recovery-owner.json"),
            serde_json::to_vec_pretty(&json!({
                "schema":1,"purpose":"native-play-recovery",
                "project_id":recovery_project.id,"project_root":recovery_project.root
            }))
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let mut saved_edit = None;
        let mut saved_presets = None;
        // A真实编辑保存/工程运行，B独立来源，再重开A核对保存后的新对象仍由原作者视口还原。
        for (index, root) in roots.iter().chain(std::iter::once(&roots[0])).enumerate() {
            let output = work.join(format!("editor-{}", index + 1));
            fs::create_dir(&output).map_err(|error| format!("作者验收输出不能独占创建：{error}"))?;
            let mut command = Command::new(std::env::current_exe().map_err(|e| e.to_string())?);
            isolate_author_command(&mut command, &output)?;
            command
                .arg("--native-editor")
                .arg("--project")
                .arg(root)
                .arg("--target-dir")
                .arg(&target)
                .env("NEW_EDITOR_NATIVE_SMOKE_DIR", &output);
            if index == 0 {
                command
                    .env("NEW_EDITOR_NATIVE_SMOKE_PLAY", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_SAVE", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_AUTHORING", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_RECOVERY", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_CLOSE", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_MAP_TOOLS", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS", "1")
                    .env("NEW_EDITOR_GAME_SMOKE_DIR", work.join("game-f12"));
            } else {
                command
                    .env_remove("NEW_EDITOR_NATIVE_SMOKE_PLAY")
                    .env_remove("NEW_EDITOR_NATIVE_SMOKE_SAVE")
                    .env_remove("NEW_EDITOR_NATIVE_SMOKE_AUTHORING")
                    .env_remove("NEW_EDITOR_NATIVE_SMOKE_RECOVERY")
                    .env_remove("NEW_EDITOR_NATIVE_SMOKE_CLOSE")
                    .env_remove("NEW_EDITOR_NATIVE_SMOKE_MAP_TOOLS")
                    .env_remove("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS")
                    .env_remove("NEW_EDITOR_GAME_SMOKE_DIR");
            }
            let run =
                build_process::run(&command, &control, Some(Duration::from_secs(1900)), tail)?;
            fs::create_dir_all(&output).map_err(|e| e.to_string())?;
            fs::write(
                output.join("process.log"),
                format!("{}\n{}", run.stdout, run.stderr),
            )
            .map_err(|e| e.to_string())?;
            if run.exit_code != 0 {
                return Err(format!(
                    "原完整编辑器验收{index}退出{}：{}\n{}",
                    run.exit_code, run.stdout, run.stderr
                ));
            }
            let observed: Value = serde_json::from_slice(
                &fs::read(output.join("native-editor.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if observed["passed"] != true {
                return Err(format!("原完整编辑器验收{index}未通过：{observed}"));
            }
            let project = project::open(root)?;
            if observed["schema"] != 2
                || observed["project_root"] != json!(project.root)
                || observed["asset_root"] != json!(project.context.asset_roots[0])
                || observed["camera_3d_count"].as_u64().unwrap_or(0) == 0
            {
                return Err("编辑器没有确认当前工程来源及实际3D视图，旧RPG报告不能计入本轮".into());
            }
            if index == 0 {
                let presets = &observed["preset_authoring"];
                verify_preset_edits(presets)?;
                let recovery = &observed["saved_play_recovery"]["Ok"];
                if recovery["schema"] != 2 || recovery["scope"] != "ordinary_surface_session_and_existing_map_history_metadata" {
                    return Err("失败恢复报告不是当前普通作者范围，旧领域报告不能计为通过".into());
                }
                for field in [
                    "complete",
                    "failure_observed",
                    "descriptor_restored",
                    "same_world_marker",
                    "original_save_proof_preserved",
                    "map_history_metadata_preserved",
                    "surface_session_preserved",
                    "owner_marker_checked",
                ] {
                    if recovery[field] != true {
                        return Err(format!("F12失败恢复缺少真实回执：{field}，{recovery}"));
                    }
                }
                let close: Value = serde_json::from_slice(
                    &fs::read(output.join("project-close.json"))
                        .map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                if close["schema"] != 3 || close["scope"] != "ordinary_document_close_cancel_conflict_retry" {
                    return Err("关闭报告未验证当前普通文本的真实冲突和重试".into());
                }
                for field in [
                    "passed",
                    "cancel_kept_same_world_and_dirty_draft",
                    "clean_prompt_waited_without_choice_and_cancel_kept_same_world",
                    "injected_save_failure_kept_draft",
                    "retry_used_original_document_writer",
                    "save_completed_before_app_exit",
                    "app_run_returned",
                ] {
                    if close[field] != true {
                        return Err(format!("工程关闭流程缺少真实回执：{field}，{close}"));
                    }
                }
                steps.push(json!({"stage":"project_close_cancel_failure_retry_and_save","passed":true,"report":close}));
                saved_presets = Some(presets.clone());
                let proof = &observed["author_save"];
                let project = project::open(root)?;
                if proof["complete"] != true
                    || proof["draft_clean"] != true
                    || proof["handoff_complete"] != true
                    || !proof["error"].is_null()
                    || proof["source_project_root"] != json!(project.root)
                    || proof["source_asset_root"] != json!(project.context.asset_roots[0])
                    || proof["successful_save_sequence"] == proof["original_save_sequence"]
                {
                    return Err(format!("原作者真实编辑保存缺少完整成功回执：{proof}"));
                }
                let game = verify_game(root, &work.join("game-f12"))?;
                if game["return_input"] != "official KeyboardInput F12 through exit_saved_play" {
                    return Err("工程运行未通过原F12返回处理器完成回程".into());
                }
                verify_placement(&game, proof)?;
                verify_game_presets(&game, presets)?;
                verify_map_tools(root, &game, &proof["map_tools"])?;
                author_tools::verify(root, &game, &proof["author_tools"])?;
                saved_edit = Some(proof.clone());
                steps.push(
                    json!({"stage":"saved_game_in_separate_world","passed":true,"report":game}),
                );
            } else if index == 2 {
                author_tools::verify_reopen(
                    &observed["current_author_document"],
                    &saved_edit.as_ref().ok_or("缺少A保存证据")?["author_tools"],
                    true,
                )?;
                verify_placement(&observed, saved_edit.as_ref().ok_or("缺少A保存证据")?)?;
                verify_preset_reopen(
                    &observed["preset_authoring"],
                    saved_presets.as_ref().ok_or("缺少A预设保存证据")?,
                    true,
                )?;
            } else {
                author_tools::verify_reopen(
                    &observed["current_author_document"],
                    &saved_edit.as_ref().ok_or("缺少A保存证据")?["author_tools"],
                    false,
                )?;
                verify_preset_reopen(
                    &observed["preset_authoring"],
                    saved_presets.as_ref().ok_or("缺少A预设保存证据")?,
                    false,
                )?;
            }
            steps.push(json!({"stage":if index == 2 { "saved_map_reopened_in_original_editor" } else { "original_editor_world" },"index":index,"passed":true,"report":observed}));
        }
        // 独立cargo run从工程cwd启动相同正式game；不设置EDITOR_SAVED_PLAY，避免仅验证开发专用入口。
        let other = project::open(&roots[1])?;
        let mut command = Command::new(
            std::env::var_os("EDITOR_HOST_CARGO")
                .or_else(|| std::env::var_os("CARGO"))
                .unwrap_or_else(|| "cargo".into()),
        );
        command
            .args(["run", "--offline", "--locked", "--manifest-path"])
            .arg(&other.manifest)
            .current_dir(&other.root)
            .env("CARGO_TARGET_DIR", &target)
            .env("NEW_EDITOR_GAME_SMOKE_DIR", work.join("game-independent"))
            .env_remove("EDITOR_SAVED_PLAY")
            .env_remove("EDITOR_SAVED_PLAY_MAP")
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_DIR")
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_PLAY");
        command
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_AUTHORING")
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_RECOVERY");
        command.env_remove("NEW_EDITOR_NATIVE_SMOKE_SAVE");
        command
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_CLOSE")
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_MAP_TOOLS")
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS");
        let game = build_process::run(&command, &control, Some(Duration::from_secs(1900)), tail)?;
        fs::write(
            work.join("independent-game.log"),
            format!("{}\n{}", game.stdout, game.stderr),
        )
        .map_err(|e| e.to_string())?;
        if game.exit_code != 0 {
            return Err(format!(
                "工程普通cargo run失败{}：{}\n{}",
                game.exit_code, game.stdout, game.stderr
            ));
        }
        let observed = verify_game(&other.root, &work.join("game-independent"))?;
        steps.push(json!({"stage":"independent_cargo_run","passed":true,"root":other.root,"report":observed}));
        let ordinary = author_tools::independent(
            &roots[0],
            &target,
            &work.join("game-independent-a"),
            &control,
        )?;
        let proof = saved_edit.as_ref().ok_or("缺少A原作者保存证据")?;
        verify_placement(&ordinary, proof)?;
        verify_map_tools(&roots[0], &ordinary, &proof["map_tools"])?;
        author_tools::verify(&roots[0], &ordinary, &proof["author_tools"])?;
        steps.push(json!({"stage":"edited_author_content_in_ordinary_cargo_run","passed":true,"report":ordinary}));
        // 发布后连续链选择保存后F12与同工程导出；额外常驻/原生交接仍由原无开关专项独立证明。
        if export_lifecycle {
            let exported = crate::project_export::validation::run_owned_project(&target, &work, &roots[0], &control)?;
            let game = &exported["independent"]["game"];
            verify_placement(game, proof)?;
            verify_map_tools(&roots[0], game, &proof["map_tools"])?;
            author_tools::verify(&roots[0], game, &proof["author_tools"])?;
            steps.push(json!({"stage":"saved_reopened_project_exported_and_run_without_source","passed":true,"report":exported}));
        } else {
            let embedded = crate::embedded_viewport::validation::run_owned_project(
                &roots[0], &target, &work.join("embedded-comparison"),
            )?;
            steps.push(json!({"stage":"embedded_resident_world_comparison","passed":true,"report":embedded}));
        }
        Ok(())
    })();
    let after = protected_files(&repository)?;
    let unchanged = before == after;
    let passed = result.is_ok() && unchanged && !control.cleanup_failed();
    fs::write(&report, serde_json::to_vec_pretty(&json!({
        "schema":4,"scope":if export_lifecycle { "saved_project_lifecycle_with_same_project_export" } else { "dual_world_modes_neutral_scene_and_ordinary_authoring" }, "passed":passed,
        "resident_mode_comparison_included":!export_lifecycle,
        "excluded_scope":"player initialization, movement, inventory, skills and combat",
        "elapsed_seconds":started.elapsed().as_secs_f64(),"work_directory":work,
        // 移动程序验收必须记录实际绑定的资源根，不能只凭窗口成功推定使用了安装资源。
        "editor_resource_root":repository,"executable":std::env::current_exe().ok(),"build_target":target,
        "steps":steps,"error":result.as_ref().err(),
        "protected_unchanged":unchanged,"protected_before":before,"protected_after":after,
        "cleanup_confirmed":!control.cleanup_failed(),
        "not_verified":["same-window project-manager UI flow", "physical keyboard and system folder dialog", "game-private RPG author extensions", "every original professional author tool"]
    })).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    if !unchanged {
        return Err("原资产/设置/Cargo保护文件发生变化，请检查报告".into());
    }
    result?;
    if control.cleanup_failed() {
        return Err("验收进程收口未确认".into());
    }
    println!("双模式窗口验收完成：{}", report.display());
    Ok(())
}

fn verify_preset_edits(report: &Value) -> Result<(), String> {
    let presets = report["presets"].as_array().ok_or("缺少普通地表编辑报告")?;
    if report["schema"] != 2
        || report["scope"] != "ordinary_surface_text_edit_save_close_reopen"
        || report["complete"] != true
        || !report["error"].is_null()
        || presets.len() != 1
        || presets[0]["domain"] != "surface"
    {
        return Err(format!(
            "普通地表作者流程未完成，历史RPG报告不能代替当前验收：{report}"
        ));
    }
    for preset in presets {
        if preset["original_text_field_used"] != true
            || preset["original_save_button_used"] != true
            || preset["write_confirmed"] != true
            || preset["tab_closed"] != true
            || preset["name_after"] != preset["expected_name"]
            || preset["catalog_hash_before"] == preset["catalog_hash_after"]
        {
            return Err(format!("预设缺少原控件、真实写入与目录采用证据：{preset}"));
        }
    }
    Ok(())
}

fn verify_preset_reopen(observed: &Value, saved: &Value, same_project: bool) -> Result<(), String> {
    if observed["schema"] != 2
        || saved["schema"] != 2
        || observed["complete"] != true
        || !observed["error"].is_null()
    {
        return Err("普通地表重开报告缺少当前协议或成功回执".into());
    }
    let observed = observed["presets"].as_array().ok_or("重开缺少预设目录")?;
    let expected = saved["presets"].as_array().ok_or("缺少已保存预设目录")?;
    for preset in expected {
        let current = observed
            .iter()
            .find(|item| item["domain"] == preset["domain"])
            .ok_or("重开预设域不完整")?;
        if current["stable_id"] != preset["stable_id"]
            || (current["name_after"] == preset["expected_name"]) != same_project
            || same_project && current["catalog_hash_after"] != preset["catalog_hash_after"]
        {
            return Err(format!("预设保存重开或跨工程隔离不一致：{current}"));
        }
    }
    Ok(())
}

fn verify_game_presets(game: &Value, saved: &Value) -> Result<(), String> {
    for preset in saved["presets"].as_array().ok_or("没有预设保存证据")? {
        let domain = preset["domain"].as_str().ok_or("预设域无效")?;
        if domain != "surface" {
            return Err("本轮仅验收普通地表，不复用历史RPG预设通过结论".into());
        }
        let actual = &game["preset_catalogs"][domain];
        if actual["first"]["stable_id"] != preset["stable_id"]
            || actual["first"]["name"] != preset["expected_name"]
            || actual["first"]["descriptor_sha256"] != preset["catalog_hash_after"]
            || actual["revision"].as_u64().unwrap_or(0) == 0
            || !actual["error"].is_null()
        {
            return Err(format!("工程运行没有采用刚保存的{domain}预设：{actual}"));
        }
    }
    Ok(())
}

fn verify_map_tools(root: &Path, game: &Value, proof: &Value) -> Result<(), String> {
    let terrains = game["terrain_objects"]
        .as_array()
        .ok_or("缺少活动地形实例证据")?;
    let matching = terrains
        .iter()
        .filter(|terrain| {
            terrain["terrain_id"] == proof["terrain_id"] && terrain["source_map_id"] == game["map"]
        })
        .collect::<Vec<_>>();
    let terrain = matching
        .first()
        .copied()
        .filter(|_| matching.len() == 1)
        .ok_or("笔划地形没有唯一活动来源实例，拒绝借用根或候选状态")?;
    let vegetation = &terrain["vegetation"];
    if proof["complete"] != true
        || !proof["error"].is_null()
        || vegetation["phase"] != "Ready"
        || vegetation["map"] != game["map"]
        || vegetation["layers"].as_u64().unwrap_or(0) == 0
        || vegetation["mesh_instances"].as_u64().unwrap_or(0) == 0
        || vegetation["mesh_batches"].as_u64().unwrap_or(0) == 0
        || vegetation["mesh_render_nodes"].as_u64().unwrap_or(0) == 0
        || game["water"]["triangles"].as_u64().unwrap_or(0) == 0
    {
        return Err(format!(
            "植被/水域原轨迹未保存并被运行实例采用：{proof}；活动地形：{terrain}"
        ));
    }
    if !game["source"]["files"].as_array().is_some_and(|files| {
        files
            .iter()
            .any(|file| file["path"] == terrain["vegetation_path"])
    }) {
        return Err("原笔划地形的正式植被源未通过工程文件指纹核对".into());
    }
    let assets = root
        .join("assets")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    for domain in ["vegetation", "water"] {
        let item = &proof[domain];
        let relative = item["source_path"].as_str().ok_or("笔划缺少正式源路径")?;
        let path = assets
            .join(relative)
            .canonicalize()
            .map_err(|error| error.to_string())?;
        if !path.starts_with(&assets) {
            return Err("笔划源文件越出本验收工程".into());
        }
        let bytes = fs::read(path).map_err(|error| error.to_string())?;
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if item["undo_records"] != 1
            || item["source_nonzero"].as_u64().unwrap_or(0) == 0
            || item["source_sha256"] != hash
            || item["source_bytes"].as_u64() != Some(bytes.len() as u64)
        {
            return Err(format!(
                "{domain}原笔划、唯一撤销与正式源字节不一致：{item}"
            ));
        }
    }
    Ok(())
}

fn verify_placement(observed: &Value, proof: &Value) -> Result<(), String> {
    let id = proof["new_placement_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("原编辑保存没有新增对象稳定ID")?;
    let expected = proof["translation"]
        .as_array()
        .filter(|value| value.len() == 3)
        .ok_or("原编辑保存没有新增对象位置")?;
    let present = observed["placement_instances"]
        .as_array()
        .is_some_and(|instances| {
            instances.iter().any(|instance| {
                instance["id"] == id
                    && instance["position"].as_array().is_some_and(|position| {
                        position.len() == 3
                            && position.iter().zip(expected).all(|(left, right)| {
                                left.as_f64()
                                    .zip(right.as_f64())
                                    .is_some_and(|(left, right)| (left - right).abs() < 0.001)
                            })
                    })
            })
        });
    if !present {
        return Err(format!("新保存对象{id}未在真实World按相同位置还原"));
    }
    Ok(())
}

fn tail(mut file: File) -> Result<String, String> {
    let mut tail = Vec::new();
    let mut bytes = [0u8; 8192];
    loop {
        let count = file.read(&mut bytes).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        let excess = (tail.len() + count).saturating_sub(16 * 1024);
        if excess > 0 {
            tail.drain(..excess);
        }
        tail.extend_from_slice(&bytes[..count]);
    }
    Ok(String::from_utf8_lossy(&tail).into_owned())
}

/// 不以退出码代替内容证据；核对真实生产门、GPU回读和游戏读到的当前工程文件字节。
fn verify_game(root: &Path, output: &Path) -> Result<Value, String> {
    let report: Value = serde_json::from_slice(
        &fs::read(output.join("game.json")).map_err(|e| format!("运行就绪报告缺失：{e}"))?,
    )
    .map_err(|e| e.to_string())?;
    verify_scene_view(&report)?;
    let project = project::open(root)?;
    if report["passed"] != true
        || report["schema"] != 2
        || report["entry"] != "production-SceneRuntimePlugin"
        || report["primary_scene_view_count"] != 1
        || report["primary_scene_views"][0]["active"] != true
        || report["mesh_entities"].as_u64().unwrap_or(0) == 0
        || report["source"]["project_root"] != json!(project.root)
        || report["source"]["project_id"] != project.id
        || report["capture"]["bytes"].as_u64().unwrap_or_default() == 0
        || !output.join("game.png").is_file()
    {
        return Err(format!(
            "运行报告未通过完整来源/生产门/GPU回读检查：{report}"
        ));
    }
    let files = report["source"]["files"]
        .as_array()
        .ok_or("运行报告没有源文件指纹")?;
    if files.is_empty() || files.len() > 64 {
        return Err("工程源文件指纹集合为空或超出验收上限".into());
    }
    for file in files {
        let relative = Path::new(file["path"].as_str().ok_or("工程源文件路径缺失")?);
        if relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err("运行报告源文件路径无效".into());
        }
        let bytes =
            fs::read(project.context.asset_roots[0].join(relative)).map_err(|e| e.to_string())?;
        let hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if file["sha256"] != hash || file["bytes"] != bytes.len() {
            return Err(format!(
                "运行实例读到的已保存文件不是本工程最新字节：{}",
                relative.display()
            ));
        }
    }
    Ok(report)
}

/// 新入口必须给出实际相机与几何证据；旧玩家报告或只有passed布尔值不能冒充新边界验收。
pub(crate) fn verify_scene_view(report: &Value) -> Result<(), String> {
    let view = &report["primary_scene_views"][0];
    let position_valid = view["position"].as_array().is_some_and(|value| value.len() == 3
        && value.iter().all(|component| component.as_f64().is_some_and(f64::is_finite)));
    let projection = &view["projection"];
    let clip_valid = projection["near"].as_f64().zip(projection["far"].as_f64())
        .is_some_and(|(near, far)| near.is_finite() && far.is_finite() && far > near);
    let projection_valid = match projection["kind"].as_str() {
        Some("perspective") => projection["fov"].as_f64().is_some_and(|fov| fov.is_finite() && fov > 0.)
            && projection["near"].as_f64().is_some_and(|near| near > 0.),
        Some("orthographic") => projection["scale"].as_f64().is_some_and(|scale| scale.is_finite() && scale > 0.),
        _ => false,
    };
    if report["schema"] != 2 || report["entry"] != "production-SceneRuntimePlugin"
        || report["primary_scene_view_count"] != 1
        || !report["primary_scene_views"].as_array().is_some_and(|views| views.len() == 1)
        || view["active"] != true || view["window_target"] != true
        || !position_valid || !clip_valid || !projection_valid
        || report["mesh_entities"].as_u64().unwrap_or(0) == 0 {
        return Err("场景报告缺少唯一真实窗口视图、有效投影/姿态或已采用几何".into());
    }
    Ok(())
}

fn protected_files(repository: &Path) -> Result<Value, String> {
    let mut observed = Vec::new();
    // 同时适用于源码开发和仅含运行资源的组包目录；校验真实资源清单，不能要求包带 host 源码。
    // 读取只发生在显式验收前后，不进入作者或游戏的帧循环。
    let mut files = crate::resource_paths::required_resource_files()?;
    for optional in ["sdk-source.toml", "build/runtime_resources.json", "Cargo.toml", ".cargo/config.toml"] {
        if repository.join(optional).try_exists().map_err(|error| error.to_string())? {
            files.push(optional.into());
        }
    }
    for relative in files {
        let path = repository.join(&relative);
        let bytes = fs::read(&path).map_err(|e| format!("验收保护文件{}不可读：{e}", path.display()))?;
        let sha256: String = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        observed.push(json!({"path":relative,"sha256":sha256}));
    }
    Ok(json!(observed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordinary_surface_report() -> Value {
        json!({
            "schema":2,"scope":"ordinary_surface_text_edit_save_close_reopen","complete":true,"error":null,
            "presets":[{"domain":"surface","stable_id":"surface","name_before":"旧","name_after":"新","expected_name":"新",
                "catalog_hash_before":"old","catalog_hash_after":"new","original_text_field_used":true,
                "original_save_button_used":true,"write_confirmed":true,"tab_closed":true}]
        })
    }

    #[test]
    fn current_surface_acceptance_cannot_reuse_historical_rpg_success() {
        let report = ordinary_surface_report();
        assert!(verify_preset_edits(&report).is_ok());
        let mut old = report.clone();
        old["schema"] = 1.into();
        old["presets"][0]["domain"] = "equipment".into();
        assert!(verify_preset_edits(&old).is_err());
        let mut missing_write = report;
        missing_write["presets"][0]["write_confirmed"] = false.into();
        assert!(verify_preset_edits(&missing_write).is_err());
    }

    #[test]
    fn saved_surface_requires_actual_runtime_adoption_and_matching_descriptor() {
        let report = ordinary_surface_report();
        let mut game = json!({"preset_catalogs":{"surface":{"revision":1,"error":null,
            "first":{"stable_id":"surface","name":"新","descriptor_sha256":"new"}}}});
        assert!(verify_game_presets(&game, &report).is_ok());
        game["preset_catalogs"]["surface"]["first"]["descriptor_sha256"] = "old".into();
        assert!(verify_game_presets(&game, &report).is_err());
        game["preset_catalogs"]["surface"] = Value::Null;
        assert!(verify_game_presets(&game, &report).is_err());
    }

    #[test]
    fn neutral_scene_acceptance_rejects_old_player_report_and_missing_real_geometry() {
        let mut report = json!({"schema":2,"entry":"production-SceneRuntimePlugin","mesh_entities":1,
            "primary_scene_view_count":1,"primary_scene_views":[{"active":true,"window_target":true,
                "position":[0,4,8],"projection":{"kind":"perspective","fov":0.8,"near":0.1,"far":1000.}}]});
        assert!(verify_scene_view(&report).is_ok());
        let mut old = report.clone();
        old["entry"] = "production-GamePlugin".into();
        old["actual_player"] = json!({"name":"旧玩家"});
        assert!(verify_scene_view(&old).is_err());
        report["mesh_entities"] = 0.into();
        assert!(verify_scene_view(&report).is_err());
        report["mesh_entities"] = 1.into();
        report["primary_scene_views"][0]["projection"]["near"] = 0.into();
        assert!(verify_scene_view(&report).is_err());
    }
}
