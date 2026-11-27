//! 不同World首版的自有工程窗口验收；只在显式--native-report时运行，不操作作者工程或桌面输入。
#[path = "native_author_tools_validation.rs"]
mod author_tools;
#[cfg(windows)]
#[path = "native_animation_workspaces.rs"]
pub(crate) mod animation_workspaces;
#[path = "native_boundary_validation.rs"]
mod boundary;
#[path = "native_validation_fixture.rs"]
mod fixture;
#[path = "native_validation_model_surfaces.rs"]
mod model_surfaces;
#[path = "native_validation_geometry.rs"]
mod geometry;
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

pub(crate) const CLOSE_SCENE_MANIFEST_PATH: &str = "scenes/M1-SAMPLE/scene.scene_manifest.json";
pub(crate) const SCENE_BEFORE_CLOSE_FILENAME: &str = "map-before-close.json";
const SCENE_TREE_STARTUP: &str = "FEATHRISE_SCENE_TREE_STARTUP_VALIDATION";
const SCENE_TREE_REOPEN: &str = "FEATHRISE_SCENE_TREE_REOPEN_VALIDATION";

fn scene_tree_validation_mode() -> Option<String> {
    std::env::var(SCENE_TREE_STARTUP).ok().filter(|mode| matches!(mode.as_str(), "1" | "lights" | "glb" | "environment"))
}

fn uses_scene_tree_fixture() -> bool {
    scene_tree_validation_mode().is_some()
}

pub(crate) fn expects_saved_scene_reopened() -> bool {
    matches!(std::env::var(SCENE_TREE_STARTUP).as_deref(), Ok("glb" | "environment"))
        && std::env::var(SCENE_TREE_REOPEN).as_deref() == Ok("1")
}
pub(crate) const ANIMATION_EDIT_VALIDATION: &str = "FEATHRISE_ANIMATION_EDIT_VALIDATION";

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
    let scene_tree_mode = scene_tree_validation_mode();
    let scene_tree_startup = scene_tree_mode.is_some();
    let scene_tree_glb = scene_tree_mode.as_deref() == Some("glb");
    let animation_edit = std::env::var(ANIMATION_EDIT_VALIDATION).as_deref() == Ok("1");
    let animation_workspaces = std::env::var(animation_workspaces::ENABLE).as_deref() == Ok("1");
    let motion_triggers = animation_workspaces && std::env::var("FEATHRISE_MOTION_TRIGGER_VALIDATION").as_deref() == Ok("1");
    let wall_line = animation_workspaces && std::env::var("FEATHRISE_WALL_LINE_VALIDATION").as_deref() == Ok("1");
    let export_lifecycle = std::env::var_os("FEATHRISE_NATIVE_EXPORT").as_deref() == Some(std::ffi::OsStr::new("1"));
    let blender_mesh = std::env::var("FEATHRISE_BLENDER_MESH_VALIDATION").as_deref() == Ok("1");
    let mut steps = Vec::new();
    let result = (|| -> Result<(), String> {
        if animation_workspaces {
            return animation_workspaces::run(&work, &repository, &target, &control, &mut steps);
        }
        if animation_edit {
            return run_animation_edit(&work, &repository, &target, &control, &mut steps);
        }
        if let Some(mode) = scene_tree_mode.as_deref() {
            return run_scene_tree_startup(&work, &repository, &target, &control, &mut steps, mode);
        }
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
            // A/B来源由真实工程根、资产根、场景和保存后的实例证明，不再制造原RPG开局文件。
            let dependencies = boundary::verify(root, &control)?;
            steps.push(json!({"stage":"production_template_dependency_boundary","index":index,"passed":true,"report":dependencies}));
        }
        let recovery_project = project::open(&roots[0])?;
        author_tools::prepare(&roots[0])?;
        write_author_fixture_owner(&recovery_project)?;
        let mut saved_edit = None;
        let mut saved_presets = None;
        // A真实编辑保存/工程运行，B独立来源，再重开A核对保存后的新对象仍由原作者视口还原。
        for (index, root) in roots.iter().chain(std::iter::once(&roots[0])).enumerate() {
            let output = work.join(format!("editor-{}", index + 1));
            let mut command = author_window_command(root, &target, &output, false)?;
            if index == 0 {
                command
                    .env("NEW_EDITOR_NATIVE_SMOKE_PLAY", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_SAVE", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_AUTHORING", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_RECOVERY", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_CLOSE", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_SCENE_TOOLS", "1")
                    .env("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS", "1")
                    .env("NEW_EDITOR_GAME_SMOKE_DIR", work.join("game-f12"));
            }
            let (project, observed) = run_author_window(&command, &control, root, &output)?;
            if index == 0 {
                let presets = &observed["preset_authoring"];
                verify_preset_edits(presets)?;
                let recovery = &observed["saved_play_recovery"]["Ok"];
                if recovery["schema"] != 2 || recovery["scope"] != "ordinary_surface_session_and_existing_scene_history_metadata" {
                    return Err("失败恢复报告不是当前普通作者范围，旧领域报告不能计为通过".into());
                }
                for field in [
                    "complete",
                    "failure_observed",
                    "descriptor_restored",
                    "same_world_marker",
                    "original_save_proof_preserved",
                    "scene_history_metadata_preserved",
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
                let manifest = project.context.asset_roots[0].join(CLOSE_SCENE_MANIFEST_PATH);
                let saved: feathrise_scene_content::SceneManifestAsset = serde_json::from_slice(
                    &fs::read(&manifest).map_err(|error| error.to_string())?,
                ).map_err(|error| error.to_string())?;
                if saved.environment.skybox_yaw_degrees != feathrise_editor_scene::standalone::validation_fixture::CLOSE_SCENE_SKYBOX_YAW
                    || close["scene_save_sequence_after"].as_u64().zip(close["scene_save_sequence_before"].as_u64())
                        .is_none_or(|(after, before)| after != before.wrapping_add(1)) {
                    return Err("退出保存未完整写入界面标记已清零的场景环境草稿".into());
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
                    || proof["final_save_sequence_before"].as_u64().is_none()
                    || proof["successful_save_sequence"] == proof["final_save_sequence_before"]
                {
                    return Err(format!("原作者真实编辑保存缺少完整成功回执：{proof}"));
                }
                verify_subdivision(&output, proof)?;
                verify_model_lod_preview(&output, proof)?;
                let game = verify_game_with_prior_manifest(
                    root,
                    &work.join("game-f12"),
                    Some(&output.join(SCENE_BEFORE_CLOSE_FILENAME)),
                )?;
                if game["return_input"] != "official KeyboardInput F12 through exit_saved_play" {
                    return Err("工程运行未通过原F12返回处理器完成回程".into());
                }
                verify_placement(&game, proof)?;
                verify_game_presets(&game, presets)?;
                verify_scene_tools(root, &game, &proof["scene_tools"])?;
                author_tools::verify(root, &game, &proof["author_tools"])?;
                model_surfaces::verify_initial(root,&output,&game,&proof["model_surfaces"])?;
                geometry::verify_saved(root,&output,&game,&proof["geometry_unification"])?;
                saved_edit = Some(proof.clone());
                steps.push(
                    json!({"stage":"saved_game_in_separate_world","passed":true,"report":game}),
                );
            } else if index == 2 {
                if observed["current_author_document"]["skybox_yaw_degrees"]
                    != json!(feathrise_editor_scene::standalone::validation_fixture::CLOSE_SCENE_SKYBOX_YAW) {
                    return Err("重开原场景没有采用退出时保存的环境参数".into());
                }
                author_tools::verify_reopen(
                    root,
                    &observed["current_author_document"],
                    &saved_edit.as_ref().ok_or("缺少A保存证据")?["author_tools"],
                    true,
                )?;
                verify_placement(&observed, saved_edit.as_ref().ok_or("缺少A保存证据")?)?;
                model_surfaces::verify_reopen(root,&observed["current_author_document"],true)?;
                geometry::verify_reopen(root,&observed["current_author_document"],&saved_edit.as_ref().ok_or("缺少A保存证据")?["geometry_unification"],true)?;
                verify_preset_reopen(
                    &observed["preset_authoring"],
                    saved_presets.as_ref().ok_or("缺少A预设保存证据")?,
                    true,
                )?;
            } else {
                model_surfaces::verify_reopen(root,&observed["current_author_document"],false)?;
                geometry::verify_reopen(root,&observed["current_author_document"],&saved_edit.as_ref().ok_or("缺少A保存证据")?["geometry_unification"],false)?;
                author_tools::verify_reopen(
                    root,
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
            steps.push(json!({"stage":if index == 2 { "saved_scene_reopened_in_original_editor" } else { "original_editor_world" },"index":index,"passed":true,"report":observed}));
        }
        // 往返专项已核对原场景保存、F12 和新进程重开。常驻视口及额外发行分支由原整套验收负责。
        if blender_mesh { return Ok(()); }
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
            .env_remove("EDITOR_SAVED_PLAY_SCENE")
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_DIR")
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_PLAY");
        command
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_AUTHORING")
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_RECOVERY");
        command.env_remove("NEW_EDITOR_NATIVE_SMOKE_SAVE");
        command
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_CLOSE")
            .env_remove("NEW_EDITOR_NATIVE_SMOKE_SCENE_TOOLS")
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
        verify_scene_tools(&roots[0], &ordinary, &proof["scene_tools"])?;
        author_tools::verify(&roots[0], &ordinary, &proof["author_tools"])?;
        model_surfaces::verify_initial(&roots[0], &work.join("editor-1"), &ordinary, &proof["model_surfaces"])?;
        steps.push(json!({"stage":"edited_author_content_in_ordinary_cargo_run","passed":true,"report":ordinary}));
        // 发布后连续链选择保存后F12与同工程导出；额外常驻/原生交接仍由原无开关专项独立证明。
        if export_lifecycle {
            let exported = crate::project_export::validation::run_owned_project(&target, &work, &roots[0], &control)?;
            let game = &exported["independent"]["game"];
            verify_placement(game, proof)?;
            verify_scene_tools(&roots[0], game, &proof["scene_tools"])?;
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
    let scope = if wall_line { "wall_line_authoring_and_reopen" }
        else if motion_triggers { "scene_motion_trigger_authoring_and_reopen" }
        else if animation_workspaces { "animation_workspaces_edit_reopen_and_gameplay" }
        else if animation_edit { "native_editor_animation_edit_mode" }
        else if scene_tree_glb { "scene_tree_choose_existing_glb_save_and_reopen" }
        else if scene_tree_startup { "native_editor_scene_tree_startup" }
        else if blender_mesh { "blender_mesh_roundtrip_saved_play_and_reopened_editor" }
        else if export_lifecycle { "saved_project_lifecycle_with_same_project_export" }
        else { "dual_world_modes_neutral_scene_and_ordinary_authoring" };
    fs::write(&report, serde_json::to_vec_pretty(&json!({
        "schema":4,"scope":scope,"passed":passed,
        "resident_mode_comparison_included":!animation_workspaces && !animation_edit && !scene_tree_startup && !blender_mesh && !export_lifecycle,
        "excluded_scope":if wall_line || motion_triggers { "independent game, packaged export, physical input" }
            else if animation_workspaces { "packaged export, physical keyboard and mouse, private RPG project" }
            else if scene_tree_startup { "saved play, independent game and export" }
            else if animation_edit { "saved play, independent game, project reopening and export" }
            else if blender_mesh { "independent cargo runs, resident viewport comparison, export and gameplay" }
            else { "player initialization, movement, inventory, skills and combat" },
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
    let label = if wall_line { "沿线围墙铺设、撤销和保存重开验收完成" }
        else if motion_triggers { "动画触发区域绘制、保存与重开验收完成" }
        else if animation_workspaces { "双动画工作区保存、重开与游戏验收完成" }
        else if animation_edit { "动画编辑模式窗口验收完成" }
        else if scene_tree_glb { "场景树添加已有 GLB、保存与重开验收完成" }
        else if scene_tree_startup { "首次打开场景树窗口验收完成" }
        else if blender_mesh { "Blender 网格往返窗口验收完成" } else { "双模式窗口验收完成" };
    println!("{label}：{}", report.display());
    Ok(())
}

#[cfg(windows)]
fn author_window_command(root: &Path, target: &Path, output: &Path, startup_only: bool) -> Result<Command, String> {
    fs::create_dir(output).map_err(|error| format!("作者验收输出不能独占创建：{error}"))?;
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    for name in [
        "NEW_EDITOR_NATIVE_SMOKE_PLAY", "NEW_EDITOR_NATIVE_SMOKE_SAVE", "NEW_EDITOR_NATIVE_SMOKE_AUTHORING",
        "NEW_EDITOR_NATIVE_SMOKE_RECOVERY", "NEW_EDITOR_NATIVE_SMOKE_CLOSE", "NEW_EDITOR_NATIVE_SMOKE_SCENE_TOOLS",
        "NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS", "NEW_EDITOR_GAME_SMOKE_DIR",
    ] {
        command.env_remove(name);
    }
    if startup_only {
        // 首次打开专项不继承外层其他验收计划，作者偏好与插件另由同一隔离入口设置。
        for (key, _) in std::env::vars_os() {
            let name = key.to_string_lossy();
            if name.starts_with("NEW_EDITOR_") || name.starts_with("FEATHRISE_WORKSPACE_")
                || name.starts_with("FEATHRISE_EMBEDDED_") || name.contains("_VALIDATION")
                || name.contains("_SMOKE_") || name.starts_with("FEATHRISE_EXPORT_")
                || name.starts_with("FEATHRISE_CODE_ITERATION_") || name.starts_with("FEATHRISE_FILE_MANAGER_")
                || name.starts_with("FEATHRISE_PROJECT_BROWSER_")
            {
                command.env_remove(key);
            }
        }
    }
    isolate_author_command(&mut command, output)?;
    command.arg("--native-editor").arg("--project").arg(root).arg("--target-dir").arg(target)
        .env("NEW_EDITOR_NATIVE_SMOKE_DIR", output);
    Ok(command)
}

#[cfg(windows)]
fn run_author_window(command: &Command, control: &BuildControl, root: &Path, output: &Path) -> Result<(project::Project, Value), String> {
    let run = build_process::run(command, control, Some(Duration::from_secs(1900)), tail)?;
    fs::write(output.join("process.log"), format!("{}\n{}", run.stdout, run.stderr))
        .map_err(|error| error.to_string())?;
    if run.exit_code != 0 {
        return Err(format!("原生编辑器验收退出{}：{}\n{}", run.exit_code, run.stdout, run.stderr));
    }
    let observed: Value = serde_json::from_slice(&fs::read(output.join("native-editor.json"))
        .map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    if observed["passed"] != true {
        return Err(format!("原生编辑器验收未通过：{observed}"));
    }
    let project = project::open(root)?;
    if observed["schema"] != 2 || observed["project_root"] != json!(project.root)
        || observed["asset_root"] != json!(project.context.asset_roots[0])
        || observed["camera_3d_count"].as_u64().unwrap_or(0) == 0
    {
        return Err("编辑器没有确认当前工程来源及实际3D视图".into());
    }
    Ok((project, observed))
}

#[cfg(windows)]
fn write_author_fixture_owner(project: &project::Project) -> Result<(), String> {
    use std::io::Write;

    // 这份租约只写入父验收独占创建的工程，允许动画夹具核对后写入自有资产。
    let bytes = serde_json::to_vec_pretty(&json!({
        "schema":1,"purpose":"native-play-recovery",
        "project_id":project.id,"project_root":project.root,
    })).map_err(|error| error.to_string())?;
    fs::OpenOptions::new().write(true).create_new(true)
        .open(project.root.join(".new-editor-recovery-owner.json"))
        .and_then(|mut file| file.write_all(&bytes)).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn run_animation_edit(work: &Path, repository: &Path, target: &Path, control: &BuildControl, steps: &mut Vec<Value>) -> Result<(), String> {
    let label = "独立工程 A";
    let root = project_template::create_with_control(&work.join(label), label, repository, control)?;
    fixture::prepare(&root, label)?;
    write_author_fixture_owner(&project::open(&root)?)?;
    let output = work.join("editor-1");
    let mut command = author_window_command(&root, target, &output, true)?;
    command.env("NEW_EDITOR_NATIVE_SMOKE_SAVE", "1").env("NEW_EDITOR_NATIVE_SMOKE_AUTHOR_TOOLS", "1")
        .env(ANIMATION_EDIT_VALIDATION, "1");
    for option in ["FEATHRISE_CURVE_VALIDATION", "FEATHRISE_GLB_COMPOSITION_VALIDATION", "FEATHRISE_MATERIAL_SLOT_VALIDATION"] {
        if std::env::var(option).as_deref() == Ok("1") {
            command.env(option, "1");
        }
    }
    let (project, observed) = run_author_window(&command, control, &root, &output)?;
    let workflow = &observed["animation_edit"];
    let animation = &workflow["animation_assets"]["animation"];
    let gate = &animation["edit_gate"];
    let toolbar = &animation["pose_toolbar"];
    if workflow["complete"] != true || workflow["animation_assets"]["passed"] != true
        || workflow["animation_assets"]["scene_preview_restored"] != true
        || workflow["animation_assets"]["source_asset_root"] != json!(project.context.asset_roots[0])
        || ["opened_read_only", "selected_read_only", "read_only_insert_rejected", "entered_edit_mode", "inserted_key"]
            .into_iter().any(|key| gate[key] != true)
        || ["hidden_in_preview", "rotate_selected", "move_selected", "history_unchanged", "hidden_after_exit"]
            .into_iter().any(|key| toolbar[key] != true)
        || animation["saved"]["save_receipt"] != json!({"Ok": null})
        || animation["saved"]["dirty"] != false || animation["saved"]["key"] != animation["disk_key"]
    {
        return Err(format!("动画模式缺少原窗口、历史或磁盘回执：{workflow}"));
    }
    let path = project.context.asset_roots[0].join("scene/animation.glb");
    let bytes = fs::read(&path).map_err(|error| error.to_string())?;
    if animation["sha256_after"] != digest(&bytes) || animation["sha256_before"] == animation["sha256_after"] {
        return Err("作者保存回执未对应本轮写入的动画 GLB".into());
    }
    let created = &workflow["animation_assets"]["created_clip"];
    let model = fs::read(project.context.asset_roots[0].join("scene/probe.glb")).map_err(|error| error.to_string())?;
    if created["asset_path"] != "scene/probe.glb" || created["disk_clip_count_before"] != 0
        || created["disk_clip_count_after"] != 2 || created["created_read_only"] != true
        || created["duplicate_via_toolbar"] != true || created["disk_clip_names"] != json!(["动画 1", "动画 1 副本"])
        || created["entered_edit_mode"] != true || created["save_receipt"] != json!({"Ok": null})
        || created["sha256_after"] != digest(&model) || created["sha256_before"] == created["sha256_after"]
    {
        return Err(format!("零动画 GLB 缺少新建、显式编辑或磁盘回执：{created}"));
    }
    let mut screenshots = ["animation-read-only.png", "animation-edit-mode.png"].into_iter()
        .map(|filename| read_author_screenshot(&output.join(filename)))
        .collect::<Result<Vec<_>, _>>()?;
    if std::env::var("FEATHRISE_CURVE_VALIDATION").as_deref() == Ok("1") {
        let curve = &workflow["animation_assets"]["curve_tangent"];
        if curve["key"].is_null() || curve["key"] != animation["disk_key"] {
            return Err("曲线手柄的重做结果与独立从磁盘读取的关键帧不一致".into());
        }
        screenshots.push(read_author_screenshot(&output.join("curve-tangents.png"))?);
    }
    if std::env::var_os("FEATHRISE_GLB_COMPOSITION_VALIDATION").is_some_and(|value| value == "1") {
        let composition = &created["composition"];
        if composition["published_clip_times"].as_array().is_none_or(|times| times.len() != 2 || times[0] == times[1]) {
            return Err("导入的角色动作缺少两个不同已发布姿态的时间回执".into());
        }
        for (file, key) in [("actor.glb", "saved_clips"), ("actor_所选.glb", "exported_clips")] {
            let bytes = fs::read(project.context.asset_roots[0].join("scene").join(file)).map_err(|e| e.to_string())?;
            let document = bevy::gltf::gltf::Gltf::from_slice(&bytes).map_err(|e| e.to_string())?;
            let names: Vec<_> = document.animations().map(|a| a.name().unwrap_or_default()).collect();
            if composition[key] != json!(names) || composition["source_count"] != 2 {
                return Err(format!("跨 GLB 磁盘内容与编辑器回执不符：{file}"));
            }
        }
        screenshots.push(read_author_screenshot(&output.join("glb-content-composition.png"))?);
    }
    if std::env::var("FEATHRISE_MATERIAL_SLOT_VALIDATION").as_deref() == Ok("1") {
        let slots = &created["material_slots"];
        if slots["preview"] != json!([1,0,0,0]) || slots["undo"] != json!([0,0,0,0]) ||
            slots["redo"] != slots["preview"] || slots["reopened"] != slots["preview"] || slots["saved_clips"] != 2 || slots["meshes"] != 2 {
            return Err(format!("原窗口材质槽替换、撤销、共享实例或独立重开结果缺失：{slots}"));
        }
        screenshots.push(read_author_screenshot(&output.join("glb-material-draft.png"))?);
        screenshots.push(read_author_screenshot(&output.join("glb-material-slots.png"))?);
    }
    steps.push(json!({"stage":"animation_edit_mode_in_original_editor","passed":true,
        "report":observed,"screenshots":screenshots}));
    Ok(())
}

#[cfg(windows)]
fn run_scene_tree_startup(work: &Path, repository: &Path, target: &Path, control: &BuildControl, steps: &mut Vec<Value>, mode: &str) -> Result<(), String> {
    let label = "独立工程 A";
    let root = project_template::create_with_control(&work.join(label), label, repository, control)?;
    fixture::prepare(&root, label)?;
    steps.push(json!({"stage":"create_owned_scene_tree_startup_fixture","passed":true,"root":root}));
    if mode == "glb" {
        return run_scene_tree_glb(&root, work, target, control, steps);
    }
    let output = work.join("editor-1");
    let (project, observed) = save_scene_tree(&root, target, &output, control, mode)?;
    let proof = &observed["author_save"];
    if mode == "environment" {
        let environment = &proof["scene_tree"]["startup"]["environment"];
        let id = environment["profile_id"].as_str().ok_or("环境验收缺少新建方案身份")?;
        let name = environment["profile_name"].as_str().ok_or("环境验收缺少新建方案名称")?;
        let simulation: feathrise_scene_content::EnvironmentSimulation =
            serde_json::from_value(environment["simulation"].clone()).map_err(|error| error.to_string())?;
        let local_simulation: feathrise_scene_content::EnvironmentSimulation =
            serde_json::from_value(environment["local_simulation"].clone()).map_err(|error| error.to_string())?;
        if (simulation.hour - 6.0).abs() > 0.03 || simulation.month != 9 || !simulation.paused
            || simulation.weather_mode_index != 3 || (simulation.weather_transition - 0.5).abs() > 0.01
            || (simulation.grass_wind_tip_strength - 0.35).abs() > 0.001
            || (local_simulation.hour - 18.0).abs() > 0.03
            || local_simulation.grass_wind_tip_strength.abs() > 0.001
            || environment["inherited_default_verified"] != true
            || environment["shared_controls_hidden"] != true
            || environment["runtime_adopted"] != true
            || environment["local_zero_wind_adopted"] != true
            || environment["undo_redo_verified"] != true
            || environment["local_restored"] != true
        { return Err("原环境控件未完成全局配置、独立副本和继承只读验收".into()); }
        // 原 Save 完成后，宿主独立读回两个正式文件，不能用窗口草稿冒充落盘。
        let assets = &project.context.asset_roots[0];
        let library_path = assets.join(feathrise_scene_content::WORLD_ENVIRONMENT_ASSET_PATH);
        let manifest_path = assets.join(CLOSE_SCENE_MANIFEST_PATH);
        let library_bytes = fs::read(&library_path).map_err(|error| error.to_string())?;
        let manifest_bytes = fs::read(&manifest_path).map_err(|error| error.to_string())?;
        let library: feathrise_scene_content::WorldEnvironmentAsset =
            serde_json::from_slice(&library_bytes).map_err(|error| error.to_string())?;
        let manifest: feathrise_scene_content::SceneManifestAsset =
            serde_json::from_slice(&manifest_bytes).map_err(|error| error.to_string())?;
        library.validate()?;
        manifest.environment.validate()?;
        let resolved = feathrise_scene_content::resolve_scene_environment(
            &manifest.environment, &library.celestial_lighting, &library.library,
        );
        if library.library.default_profile_id != id
            || library.library.profiles.get(id).is_none_or(|profile| profile.name != name)
            || manifest.environment.source != feathrise_scene_content::SceneEnvironmentSource::Shared(id.to_owned())
            || resolved.profile_id.as_deref() != Some(id) || resolved.missing_profile.is_some()
            || library.library.profiles.get(id).is_none_or(|profile| profile.appearance.simulation != simulation)
            || resolved.environment.simulation != Some(simulation)
            || manifest.environment.simulation != Some(local_simulation)
        {
            return Err("工程默认、场景引用和保存后的生效环境必须同属界面新建的方案".into());
        }
        let screenshot = read_author_screenshot(&output.join("scene-tree-environment.png"))?;
        let local_screenshot = read_author_screenshot(&output.join("scene-tree-environment-local.png"))?;
        let (id, name) = (id.to_owned(), name.to_owned());
        steps.push(json!({"stage":"create_share_and_save_environment","passed":true,
            "profile_id":id,"profile_name":name,"simulation":simulation,"local_simulation":local_simulation,
            "screenshot":screenshot,"local_screenshot":local_screenshot,"report":observed}));
        let output = work.join("editor-2");
        let reopened = reopen_tree(&root, target, &output, control, Some(mode))?;
        let document = &reopened["current_author_document"];
        let environment = &document["reopened_source_node"]["reopened_environment"];
        if reopened["author_save_requested"] != false || document["scene_id"] != "M1-SAMPLE"
            || environment["profile_id"] != id || environment["profile_name"] != name
            || environment["scene_source"] != "shared"
            || environment["visible_source"] != format!("环境来源  共享方案 · {name}")
            || environment["simulation"] != serde_json::to_value(simulation).map_err(|error| error.to_string())?
            || environment["local_simulation"] != serde_json::to_value(local_simulation).map_err(|error| error.to_string())?
            || environment["shared_controls_hidden"] != true
            || environment["runtime_adopted"] != true
            || fs::read(&library_path).map_err(|error| error.to_string())? != library_bytes
            || fs::read(&manifest_path).map_err(|error| error.to_string())? != manifest_bytes
        {
            return Err("重开必须只读取已保存的共享方案，并在场景环境页显示同一方案".into());
        }
        let screenshot = read_author_screenshot(&output.join("native-editor.png"))?;
        steps.push(json!({"stage":"reopen_saved_environment","passed":true,
            "profile_id":id,"profile_name":name,"screenshot":screenshot,"report":reopened}));
        return Ok(());
    }
    let placement = proof["new_placement_id"].as_str().ok_or("保存报告缺少本轮新增摆放 ID")?;
    let placement_path = fixture::MODEL_SURFACE_SOURCE; // 场景树种子和保存均使用 M1-SAMPLE 私有 scene.glb。
    let saved = fs::read(project.context.asset_roots[0].join(placement_path)).map_err(|error| error.to_string())?;
    let folder_glb = bevy::gltf::gltf::Glb::from_slice(&saved).map_err(|error| error.to_string())?;
    let folder_document: Value = serde_json::from_slice(&folder_glb.json).map_err(|error| error.to_string())?;
    let saved = feathrise_scene_content::read_scene_placements(&saved, placement_path)?;
    if saved.scene_id.as_str() != "M1-SAMPLE" || !saved.placements.iter().any(|record| record.id == placement) {
        return Err("本轮立方体未随原保存写入该场景的 GLB".into());
    }
    let startup = &proof["scene_tree"]["startup"];
    let mut saved_light_ids = Value::Null;
    let mut saved_light_optics = Value::Null;
    if startup["glb_menu"]["local_lights_supported"] == true {
        let definitions = folder_document["extensions"]["KHR_lights_punctual"]["lights"].as_array().ok_or("主 GLB 缺少标准灯光表")?;
        let nodes = folder_document["nodes"].as_array().ok_or("主 GLB 缺少节点")?;
        let id = |kind: &str, cd: f64, range: f64| nodes.iter().find_map(|node| {
            let index = node["extensions"]["KHR_lights_punctual"]["light"].as_u64()? as usize;
            let light = definitions.get(index)?;
            (light["type"] == kind && light["intensity"].as_f64() == Some(cd) && light["range"].as_f64().is_some_and(|r| (r - range).abs() < 0.08))
                .then(|| Some((node["extras"]["feathrise_id"].as_str()?, light))).flatten()
        });
        let (point_id, point) = id("point",650.0,9.0).ok_or("已保存点光源缺少编辑后的光强或范围")?;
        let (spot_id, spot) = id("spot",850.0,14.0).ok_or("已保存聚光灯缺少编辑后的光强或范围")?;
        let cone = &spot["spot"];
        let inner = cone["innerConeAngle"].as_f64().ok_or("保存的聚光灯缺少内锥角")?.to_degrees();
        let outer = cone["outerConeAngle"].as_f64().ok_or("保存的聚光灯缺少外锥角")?.to_degrees();
        if (inner - 15.0).abs() >= 0.08 || (outer - 32.0).abs() >= 0.08 { return Err("落盘的聚光半角与视口拖动不一致".into()); }
        saved_light_ids = json!({"point":point_id, "spot":spot_id});
        saved_light_optics = json!({"point_range_m":point["range"], "spot_range_m":spot["range"], "spot_inner_deg":inner, "spot_outer_deg":outer});
    }
    if mode == "lights" {
        if saved_light_ids.is_null() || startup["dragged_local_lights"] != saved_light_ids { return Err("已保存的灯具身份与实拖不符".into()); }
        let point = read_author_screenshot(&output.join("scene-tree-point-light-range.png"))?;
        let spot = read_author_screenshot(&output.join("scene-tree-local-lights.png"))?;
        steps.push(json!({"stage":"drag_local_light_handles_and_save","passed":true,"optics":saved_light_optics,
            "point_screenshot":point,"spot_screenshot":spot,"report":observed}));
        let output = work.join("editor-2");
        let reopened = reopen_tree(&root, target, &output, control, Some(mode))?;
        if reopened["author_save_requested"] != false || reopened["current_author_document"]["reopened_source_node"]["reopened_local_lights"] != saved_light_ids {
            return Err("重开的实际灯光组件、姿态或身份与已保存节点不符".into());
        }
        steps.push(json!({"stage":"reopen_saved_local_light_handles","passed":true,"saved_local_lights":saved_light_ids,"report":reopened}));
        return Ok(());
    }
    let parents = &folder_document["extras"]["feathrise_object_folder_parents"];
    if parents["验收文件夹"] != "验收目标" || parents["验收子文件夹"] != "验收文件夹" {
        return Err("场景 GLB 未保存新建子文件夹或整夹移动的父级关系".into());
    }
    if ["main_glb_icons_indented_14_px", "terrain_and_object_icons_aligned", "height_terrain_has_one_caption",
        "source_mesh_indented_14_px", "initial_glb_arrows_visible"]
        .into_iter().any(|key| startup["layout"][key] != true)
        || startup["glb_menu"]["creation_submenu_beside_unchanged_main"] != true
    {
        return Err("场景树层级、小箭头或侧开新建子菜单缺少实际布局回执".into());
    }
    let [water_target, water_neighbor] = fixture::SCENE_TREE_WATER_IDS;
    let mut expected_water = json!({
        "target":{"id":water_target,"enabled":false,"hidden":true,"locked":true},
        "neighbor":{"id":water_neighbor,"enabled":true,"hidden":false,"locked":false},
    });
    let manifest: Value = serde_json::from_slice(&fs::read(project.context.asset_roots[0].join(CLOSE_SCENE_MANIFEST_PATH))
        .map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    let stored_water = &manifest["scene_layers"]["still_water"]["root"];
    if startup["still_water"] != expected_water
        || stored_water != &json!({"disabled":[water_target],"locked":[water_target]})
    {
        return Err("静水子行未分别改变目标状态，或工程清单未只保存目标的参与与锁定".into());
    }
    let new_folder = startup["root_folder_from_blank"].as_str().ok_or("缺少空白处新建目录的回执")?;
    let deleted = startup["glb_menu"]["source_staged_for_permanent_deletion"].as_str().ok_or("缺少永久删除 GLB 的菜单回执")?;
    let deleted_path = format!("scenes/M1-SAMPLE/models/{deleted}.glb");
    if project.context.asset_roots[0].join(&deleted_path).try_exists().map_err(|error| error.to_string())?
        || saved.placements.iter().any(|record| record.asset_path == deleted_path)
    {
        return Err("永久删除后，源 GLB 与摆放记录应同时离开已保存的场景".into());
    }
    if !saved.placements.iter().any(|record|
        matches!(&record.asset_target, feathrise_scene_content::ScenePlacementAssetTarget::EditableMesh { geometry, .. } if geometry.is_open()))
    {
        return Err("从主 GLB 新建的平面未经过原保存链写入".into());
    }
    if startup["glb_menu"]["created_main_cuboid_at_local_origin"] != true
        || startup["glb_menu"]["created_source_cuboid_at_local_origin"] != true
        || !saved.placements.iter().any(|record| record.id != placement
            && matches!(&record.asset_target, feathrise_scene_content::ScenePlacementAssetTarget::Cuboid { .. })
            && record.translation == [0.0; 3] && record.terrain_offset == 0.0
            && record.height_mode == feathrise_scene_content::ScenePlacementHeightMode::FixedHeight)
    {
        return Err("右键新建立方体缺少当前 GLB 本地原点或主场景固定高度的落盘证据".into());
    }
    if !saved.placements.iter().any(|record| record.asset_path == "scene/probe.glb" && record.editor.group == new_folder) {
        return Err("从活动选择窗口添加的 GLB 没有保存在右键指向的文件夹".into());
    }
    for (index, group) in ["", new_folder].into_iter().enumerate() {
        let name = startup["empty_glbs"][index].as_str().ok_or("缺少空 GLB 创建回执")?;
        let relative = format!("scenes/M1-SAMPLE/models/{name}.glb");
        if !saved.placements.iter().any(|record| record.asset_path == relative && record.editor.group == group) {
            return Err(format!("新建空 GLB 未按场景树归属保存：{relative}"));
        }
        let bytes = fs::read(project.context.asset_roots[0].join(&relative)).map_err(|error| error.to_string())?;
        let model = bevy::gltf::gltf::Gltf::from_slice(&bytes).map_err(|error| error.to_string())?;
        if model.default_scene().is_none_or(|scene| scene.nodes().next().is_some()) || model.meshes().next().is_some() {
            return Err(format!("新建 GLB 没有保存为可独立读取的空场景：{relative}"));
        }
    }
    for (id, x) in [("scene-tree-group-0", 24.0), ("scene-tree-group-1", 36.0)] {
        if !saved.placements.iter().any(|record| record.id == id && record.editor.group == "验收文件夹"
            && record.asset_path == "scene/probe.glb" && record.translation == [x, 2.0, 12.0]) {
            return Err("改名后的文件夹未与原成员一起写入场景 GLB".into());
        }
    }
    let source = fs::read(project.context.asset_roots[0].join("scene/probe.glb")).map_err(|error| error.to_string())?;
    let source = bevy::gltf::gltf::Glb::from_slice(&source).map_err(|error| error.to_string())?;
    let source: Value = serde_json::from_slice(&source.json).map_err(|error| error.to_string())?;
    let local_x = source["nodes"][0]["translation"][0].as_f64();
    // 世界 7.5 米扣除整件 5 米和父节点 1.25 米，只有网格的源局部位置应改变。
    if local_x != Some(1.25) || source["nodes"][0]["name"] != "验收网格"
        || source["nodes"][1]["translation"][0].as_f64() != Some(1.25)
        || !saved.placements.iter().any(|record| record.id == "sample-model" && record.translation[0] == 5.0)
    {
        return Err("保存的源 GLB 节点名称或局部 X 未正确写回".into());
    }
    let baseline = bevy::gltf::gltf::Glb::from_slice(include_bytes!("../tests/fixtures/scene/probe.glb"))
        .map_err(|error| error.to_string())?;
    let baseline: Value = serde_json::from_slice(&baseline.json).map_err(|error| error.to_string())?;
    let position = |doc: &Value| doc["meshes"][0]["primitives"][0]["attributes"]["POSITION"].as_u64().map(|index| index as usize);
    let original = position(&baseline).ok_or("原夹具位置访问器缺失")?;
    let applied = position(&source).ok_or("保存的网格位置访问器缺失")?;
    if original == applied || source["nodes"][0]["scale"][0].as_f64() != Some(1.0)
        || source["accessors"][applied]["max"][0].as_f64() != baseline["accessors"][original]["max"][0].as_f64().map(|x| x * 2.0)
    { return Err("应用缩放后没有归一姿态并写入二倍的真实源网格几何".into()); }
    let screenshot = read_author_screenshot(&output.join("scene-tree-startup.png"))?;
    let coordinates = read_author_screenshot(&output.join("scene-tree-glb-coordinates.png"))?;
    let cube_properties = read_author_screenshot(&output.join("scene-tree-cube-properties.png"))?;
    let drag_preview = read_author_screenshot(&output.join("scene-tree-drag-preview.png"))?;
    let nested_folder = read_author_screenshot(&output.join("scene-tree-nested-folder.png"))?;
    let creation_submenu = read_author_screenshot(&output.join("scene-tree-new-glb-submenu.png"))?;
    let still_water = read_author_screenshot(&output.join("scene-tree-still-water-toggles.png"))?;
    let local_lights = (!saved_light_ids.is_null()).then(|| read_author_screenshot(&output.join("scene-tree-local-lights.png"))).transpose()?;
    let point_range = (!saved_light_ids.is_null()).then(|| read_author_screenshot(&output.join("scene-tree-point-light-range.png"))).transpose()?;
    steps.push(json!({"stage":"scene_tree_startup_in_original_editor","passed":true,"report":observed,"saved_placement_id":placement,
        "startup_screenshot":screenshot,"source_coordinates_screenshot":coordinates,"mesh_properties_screenshot":cube_properties,
        "drag_preview_screenshot":drag_preview,"nested_folder_screenshot":nested_folder,"creation_submenu_screenshot":creation_submenu,
        "still_water_screenshot":still_water,"saved_still_water":stored_water,
        "local_lights_screenshot":local_lights,"point_range_screenshot":point_range,"saved_local_lights":saved_light_ids,"saved_light_optics":saved_light_optics,
        "renamed_folder":"验收文件夹",
        "saved_source_local_x":local_x,"saved_source_name":"验收网格","applied_scale_x":1.0}));

    let output = work.join("editor-2");
    let reopened = reopen_tree(&root, target, &output, control, None)?;
    let node = &reopened["current_author_document"]["reopened_source_node"];
    if node["reopened_local_lights"] != saved_light_ids {
        return Err(format!("重开后的真实灯光组件或变换与已保存节点不符：{}", node["reopened_local_lights"]));
    }
    expected_water["target"]["hidden"] = json!(false); // 眼睛只作用于作者进程，重开恢复可见偏好。
    if node["reopened_still_water"] != expected_water {
        return Err(format!("重开工程后静水子行参与、锁定或临时显隐不符：{}", node["reopened_still_water"]));
    }
    if reopened["author_save_requested"] != false || node["placement"] != "sample-model"
        || node["local_x"].as_f64() != Some(1.25) || node["world_x"].as_f64() != Some(7.5)
        || node["reopened_folder_parent"] != "验收目标" || node["reopened_folder_members"] != 2
        || node["reopened_created_child_parent"] != "验收文件夹"
        || node["reopened_empty_glbs"] != true || node["reopened_picked_glb"] != true
    {
        return Err(format!("重开工程后未采用已保存的源节点变换：{node}"));
    }
    let screenshot = read_author_screenshot(&output.join("native-editor.png"))?;
    steps.push(json!({"stage":"saved_source_node_reopened_in_original_editor","passed":true,"report":reopened,"screenshot":screenshot}));
    Ok(())
}

#[cfg(windows)]
fn run_scene_tree_glb(
    root: &Path,
    work: &Path,
    target: &Path,
    control: &BuildControl,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    const SOURCE: &str = "scene/probe.glb";
    const GROUP: &str = "验收根目录";
    let fixture = project::open(root)?;
    let asset_root = &fixture.context.asset_roots[0];
    let source_path = asset_root.join(SOURCE);
    let scene_path = asset_root.join(fixture::MODEL_SURFACE_SOURCE);
    let source_before = fs::read(&source_path).map_err(|error| error.to_string())?;
    let before = feathrise_scene_content::read_scene_placements(
        &fs::read(&scene_path).map_err(|error| error.to_string())?,
        fixture::MODEL_SURFACE_SOURCE,
    )?;
    // 夹具已有同源摆放，必须通过新增稳定 ID 区分这次选择和原场景里的引用。
    let existing: Vec<_> = before.placements.iter().filter(|record| record.asset_path == SOURCE).collect();
    if existing.is_empty() || existing.iter().any(|record| record.editor.group == GROUP) {
        return Err("场景树夹具缺少原同源引用，或目标文件夹已包含源模型".into());
    }

    let output = work.join("editor-1");
    let (_, observed) = save_scene_tree(root, target, &output, control, "glb")?;
    let proof = &observed["author_save"];
    if proof["original_save_sequence"].as_u64().and_then(|sequence| sequence.checked_add(1))
        != proof["successful_save_sequence"].as_u64()
    {
        return Err("添加已有 GLB 专项应由标准保存按钮产生且只产生一次成功保存回执".into());
    }
    let startup = &proof["scene_tree"]["startup"];
    let chosen = &startup["chosen_glb"];
    let id = chosen["id"].as_str().filter(|id| !id.is_empty()).ok_or("缺少文件窗口添加的 GLB ID")?;
    let identity = json!({"id":id,"asset_path":SOURCE,"group":GROUP});
    if chosen != &identity || proof["new_placement_id"] != id
        || startup["visible_tree_row"] != true || startup["rendered_instance"] != true
        || before.placements.iter().any(|record| record.id == id)
    {
        return Err(format!("首窗未将新 GLB 放到目标树行：{chosen}"));
    }

    // 编辑器已退出，宿主独立读取正式 scene.glb 和源 GLB，不能用窗口草稿冒充落盘。
    let saved_bytes = fs::read(&scene_path).map_err(|error| error.to_string())?;
    let saved = feathrise_scene_content::read_scene_placements(&saved_bytes, fixture::MODEL_SURFACE_SOURCE)?;
    let added = saved.placements.iter().find(|record| record.id == id).ok_or("新增 GLB 未写入场景")?;
    let glb = bevy::gltf::gltf::Glb::from_slice(&saved_bytes).map_err(|error| error.to_string())?;
    let document: Value = serde_json::from_slice(&glb.json).map_err(|error| error.to_string())?;
    let extras = &document["extras"];
    if before.scene_id.as_str() != "M1-SAMPLE" || saved.scene_id != before.scene_id
        || saved.placements.len() != before.placements.len() + 1
        || added.asset_path != SOURCE || added.editor.group != GROUP || !added.asset_target.is_scene0()
        || saved.placements.iter().filter(|record| record.asset_path == SOURCE).count() != existing.len() + 1
        || !existing.iter().all(|old| saved.placements.iter().any(|record| record.id == old.id
            && record.asset_path == old.asset_path && record.asset_target == old.asset_target
            && record.editor.group == old.editor.group))
        || !extras["feathrise_object_folders"].as_array().is_some_and(|folders| folders.iter().any(|folder| folder == GROUP))
        || extras["feathrise_object_folder_parents"].get(GROUP).is_some()
        || fs::read(&source_path).map_err(|error| error.to_string())? != source_before
    {
        return Err("磁盘上的完整 GLB 引用、根级文件夹或源文件不符合预期".into());
    }
    let screenshot = read_author_screenshot(&output.join("native-editor.png"))?;
    steps.push(json!({"stage":"choose_existing_glb_and_save","passed":true,
        "placement":identity,"scene_glb":scene_path,"scene_glb_sha256":digest(&saved_bytes),
        "source_glb_sha256":digest(&source_before),"existing_source_references":existing.len(),
        "screenshot":screenshot,"report":observed}));

    let output = work.join("editor-2");
    let reopened = reopen_tree(root, target, &output, control, Some("glb"))?;
    let document = &reopened["current_author_document"];
    let tree = &document["reopened_source_node"];
    if reopened["author_save_requested"] != false || document["scene_id"] != "M1-SAMPLE"
        || tree["chosen_glb"] != identity || tree["visible_tree_row"] != true || tree["rendered_instance"] != true
        || fs::read(&scene_path).map_err(|error| error.to_string())? != saved_bytes
        || fs::read(&source_path).map_err(|error| error.to_string())? != source_before
    {
        return Err(format!("独立窗口没有从已保存的场景回读同一 GLB、树行和渲染实例：{tree}"));
    }
    let screenshot = read_author_screenshot(&output.join("native-editor.png"))?;
    steps.push(json!({"stage":"reopen_chosen_glb_in_scene_tree","passed":true,
        "placement":identity,"screenshot":screenshot,"report":reopened}));
    Ok(())
}

#[cfg(windows)]
fn save_scene_tree(root: &Path, target: &Path, output: &Path, control: &BuildControl, mode: &str) -> Result<(project::Project, Value), String> {
    let mut command = author_window_command(root, target, output, true)?;
    command.env("NEW_EDITOR_NATIVE_SMOKE_SAVE", "1")
        .env("FEATHRISE_SCENE_TREE_VALIDATION", "1")
        .env(SCENE_TREE_STARTUP, mode);
    let (project, observed) = run_author_window(&command, control, root, output)?;
    let proof = &observed["author_save"];
    if observed["author_save_requested"] != true
        || proof["complete"] != true || proof["draft_clean"] != true || proof["handoff_complete"] != true
        || !proof["error"].is_null() || proof["source_project_root"] != json!(project.root)
        || proof["source_asset_root"] != json!(project.context.asset_roots[0])
        || proof["successful_save_sequence"].as_u64().zip(proof["original_save_sequence"].as_u64())
            .is_none_or(|(after, before)| after <= before)
        || !proof["scene_tree"]["startup"].is_object()
    {
        return Err(format!("首次打开场景树或原保存缺少真实回执：{proof}"));
    }
    Ok((project, observed))
}

#[cfg(windows)]
fn reopen_tree(root: &Path, target: &Path, output: &Path, control: &BuildControl, mode: Option<&str>) -> Result<Value, String> {
    let mut command = author_window_command(root, target, output, true)?;
    command.env(SCENE_TREE_REOPEN, "1");
    if let Some(mode) = mode { command.env(SCENE_TREE_STARTUP, mode); }
    run_author_window(&command, control, root, output).map(|(_, observed)| observed)
}

#[cfg(windows)]
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(windows)]
fn read_author_screenshot(path: &Path) -> Result<Value, String> {
    use bevy::{
        asset::RenderAssetUsages,
        image::{CompressedImageFormats, ImageSampler, ImageType},
        prelude::Image,
    };

    let bytes = fs::read(path).map_err(|error| format!("无法读取GPU截图：{error}"))?;
    let image = Image::from_buffer(&bytes, ImageType::Extension("png"), CompressedImageFormats::NONE,
        true, ImageSampler::nearest(), RenderAssetUsages::default())
        .map_err(|error| format!("GPU截图无法解码：{error}"))?;
    Ok(json!({"path":path,"width":image.width(),"height":image.height(),"sha256":digest(&bytes)}))
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

fn verify_scene_tools(root: &Path, game: &Value, proof: &Value) -> Result<(), String> {
    let terrains = game["terrain_objects"]
        .as_array()
        .ok_or("缺少活动地形实例证据")?;
    let matching = terrains
        .iter()
        .filter(|terrain| {
            terrain["terrain_id"] == proof["terrain_id"] && terrain["source_scene_id"] == game["scene_id"]
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
        || vegetation["scene_id"] != game["scene_id"]
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

fn verify_subdivision(output: &Path, proof: &Value) -> Result<(), String> {
    if std::env::var("FEATHRISE_SUBDIVIDE_VALIDATION").as_deref() != Ok("1") {
        return Ok(());
    }
    let subdivision = &proof["chunk_lod"]["subdivision"];
    for (key, expected) in [
        ("source", json!([8, 12, 6])),
        ("one_cut", json!([26, 48, 24])),
        ("two_cuts", json!([56, 108, 54])),
        ("three_cuts", json!([98, 192, 96])),
        ("object_mode_visible", json!(true)),
        ("preview_history_unchanged", json!(true)),
        ("recomputed_exact", json!(true)),
        ("confirmed_one_undo", json!(true)),
    ] {
        if subdivision[key] != expected {
            return Err(format!("细分正式参数控件缺少有效证据：{key}，{subdivision}"));
        }
    }
    let name = "mesh-subdivide-two-cuts.png";
    if subdivision["screenshot"] != name
        || !fs::metadata(output.join(name)).is_ok_and(|file| file.is_file() && file.len() > 0)
    {
        return Err("细分两刀实时预览缺少本轮独占窗口截图".into());
    }
    let name = "mesh-subdivide-object-mode.png";
    if subdivision["object_screenshot"] != name
        || !fs::metadata(output.join(name)).is_ok_and(|file| file.is_file() && file.len() > 0)
    {
        return Err("细分后的对象模式缺少本轮独占窗口截图".into());
    }
    Ok(())
}

fn verify_model_lod_preview(output: &Path, proof: &Value) -> Result<(), String> {
    if std::env::var("FEATHRISE_MODEL_LOD_PREVIEW_VALIDATION").as_deref() != Ok("1") {
        return Ok(());
    }
    let preview = &proof["chunk_lod"]["model_lod_preview"];
    if preview["complete"] != true || preview["source_triangles"] != 122_412
        || !preview["lod_triangles"].as_u64().is_some_and(|count| count > 0 && count < 122_412)
    {
        return Err(format!("模型LOD预览缺少同一真实密集来源的减面回执：{preview}"));
    }
    for phase in ["original", "lod", "original_repeat"] {
        if preview[phase]["frames"] != 90 || preview[phase]["continuous_no_vsync"] != true
            || !preview[phase]["median_ms"].as_f64().is_some_and(|value| value > 0.0) {
            return Err(format!("模型LOD对照缺少有效实际帧样本：{phase}"));
        }
    }
    for name in ["model-lod-simplified.png", "model-lod-original.png"] {
        if !fs::metadata(output.join(name)).is_ok_and(|file| file.is_file() && file.len() > 0) {
            return Err(format!("缺少本轮窗口三角线截图：{name}"));
        }
    }
    Ok(())
}

fn verify_placement(observed: &Value, proof: &Value) -> Result<(), String> {
    if std::env::var("FEATHRISE_BLENDER_MESH_VALIDATION").as_deref() == Ok("1") {
        verify_blender_mesh(observed, proof)?;
    }
    if std::env::var("FEATHRISE_PLANE_VALIDATION").as_deref() == Ok("1") && proof["plane"].is_null() {
        return Err("平面专项缺少原创建与建模证据".into());
    }
    if !proof["plane"].is_null() {
        let plane = &proof["plane"];
        if plane["complete"] != true || plane["undo_exact"] != true || plane["redo_exact"] != true {
            return Err("平面专项未完成真实建模和历史恢复".into());
        }
        let assets = PathBuf::from(proof["source_asset_root"].as_str().ok_or("平面缺少自有资产根")?)
            .canonicalize().map_err(|e| e.to_string())?;
        let relative = plane["placements_path"].as_str().ok_or("平面没有冻结摆放源")?;
        let path = assets.join(relative).canonicalize().map_err(|e| e.to_string())?;
        if !path.starts_with(&assets) { return Err("平面摆放源越出自有工程".into()); }
        let saved = feathrise_scene_content::read_scene_placements(&fs::read(path).map_err(|e| e.to_string())?, relative)?;
        let expected: feathrise_scene_content::ScenePlacementRecord = serde_json::from_value(plane["saved_record"].clone()).map_err(|e| e.to_string())?;
        if !expected.asset_target.editable_geometry().is_some_and(|g| g.is_open() && (g.vertex_count(), g.face_count()) == (9, 4))
            || saved.placements.iter().find(|record| record.id == expected.id) != Some(&expected) {
            return Err("原保存未保留完整细分平面".into());
        }
        if let Some(records) = observed["current_author_document"]["placements"].as_array()
            && !records.iter().any(|record| record == &plane["saved_record"]) {
            return Err("新进程作者重开未恢复细分平面".into());
        }
    }
    if !proof["chunk_lod"].is_null() {
        let chunks = &proof["chunk_lod"];
        if chunks["complete"] != true || chunks["undo_exact"] != true || chunks["redo_exact"] != true
            || chunks["lod_properties_ui"] != true || chunks["lod_undo_exact"] != true
            || chunks["lod_redo_exact"] != true || chunks["lod_original_restored"] != true
            || chunks["lod_fields_visible"] != true
            || !chunks["mixed_levels"].as_array().is_some_and(|levels|
                levels.iter().any(|level| level == 0) && levels.iter().any(|level| level.as_u64().is_some_and(|level| level > 0))) {
            return Err(format!("分块LOD缺少真实混合显示或原建模历史证据：{chunks}"));
        }
        let assets = PathBuf::from(proof["source_asset_root"].as_str().ok_or("分块LOD缺少自有资产根")?)
            .canonicalize().map_err(|error| error.to_string())?;
        let relative = chunks["placements_path"].as_str().ok_or("分块LOD没有冻结摆放源")?;
        let path = assets.join(relative).canonicalize().map_err(|error| error.to_string())?;
        if !path.starts_with(&assets) { return Err("分块LOD摆放源越出自有工程".into()); }
        let saved = feathrise_scene_content::read_scene_placements(&fs::read(path).map_err(|error| error.to_string())?, relative)?;
        let expected: feathrise_scene_content::ScenePlacementRecord = serde_json::from_value(chunks["saved_record"].clone())
            .map_err(|error| error.to_string())?;
        if saved.placements.iter().find(|record| record.id == expected.id) != Some(&expected) {
            return Err("原保存未保留完整细分作者网格或LOD策略".into());
        }
        if let Some(records) = observed["current_author_document"]["placements"].as_array()
            && !records.iter().any(|record| record == &chunks["saved_record"]) {
            return Err("新进程作者重开未恢复完整细分网格".into());
        }
    }
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

fn verify_blender_mesh(observed: &Value, proof: &Value) -> Result<(), String> {
    let roundtrip = &proof["blender_mesh"];
    if roundtrip["complete"] != true || roundtrip["undo_exact"] != true || roundtrip["redo_exact"] != true
        || roundtrip["peer_unchanged"] != true || roundtrip["preview_step_shown"] != true
        || roundtrip["repeat_save_exact"] != true || roundtrip["blender"]["second"]["step_restored"] != true
        || roundtrip["collision_inherited"] != true || roundtrip["collision_toggled_via_properties"] != true
    {
        return Err(format!("缺少真实 Blender 导出、原作者往返或历史回执：{roundtrip}"));
    }
    let assets = PathBuf::from(proof["source_asset_root"].as_str().ok_or("缺少 Blender 自有资产根")?)
        .canonicalize().map_err(|error| error.to_string())?;
    let relative = roundtrip["placements_path"].as_str().ok_or("缺少 Blender 正式摆放路径")?;
    let path = assets.join(relative).canonicalize().map_err(|error| error.to_string())?;
    if !path.starts_with(&assets)
        || path.file_name().and_then(|name| name.to_str()) != Some(feathrise_scene_content::SCENE_PLACEMENTS_FILE_NAME)
    {
        return Err("单件 Blender 验收只读取自有场景的旧式 JSON 摆放".into());
    }
    let saved = feathrise_scene_content::read_scene_placements(&fs::read(path).map_err(|error| error.to_string())?, relative)?;
    if saved.placements.len() <= 8_192 || roundtrip["saved_collision"]["additional_collision_hits"] != 130
        || (observed["entry"] != "production-SceneRuntimePlugin"
            && (observed["placement_instances"].as_array().map(Vec::len) != Some(saved.placements.len())
                || observed["current_author_document"]["blender_mesh_collision"]["additional_collision_hits"] != 130))
    {
        return Err("真实保存或重开未完整覆盖大场景摆放和碰撞".into());
    }
    for key in ["saved_record", "peer_record"] {
        let expected: feathrise_scene_content::ScenePlacementRecord = serde_json::from_value(roundtrip[key].clone())
            .map_err(|error| error.to_string())?;
        if saved.placements.iter().find(|record| record.id == expected.id) != Some(&expected) {
            return Err(format!("正式摆放文件未保存完整对象记录 {key}"));
        }
        if observed["entry"] != "production-SceneRuntimePlugin"
            && !observed["current_author_document"]["placements"].as_array()
                .is_some_and(|records| records.iter().any(|record| record == &roundtrip[key]))
        {
            return Err(format!("新进程重开后的作者草稿未恢复对象记录 {key}"));
        }
        if !observed["placement_instances"].as_array().is_some_and(|instances| {
            instances.iter().any(|instance| instance["id"] == expected.id
                && instance["asset_target"] == json!(expected.asset_target)
                && instance["position"].as_array().is_some_and(|position| position.len() == 3
                    && position.iter().zip(expected.translation).all(|(actual, expected)|
                        actual.as_f64().is_some_and(|actual| (actual - f64::from(expected)).abs() < 0.001))))
        }) {
            return Err(format!("相同场景的正式运行实例未加载对象 {key}"));
        }
        if key == "saved_record" {
            if saved.placements.iter().position(|record| record.id == expected.id).is_none_or(|index| index < 8_192) {
                return Err("Blender 目标未覆盖旧场景树索引上限以后的摆放".into());
            }
            let collision_matches = |proof: &Value| proof["placement_id"] == expected.id
                && proof["lower_step_distances"].as_array().is_some_and(|values| values.len() == 2
                    && values.iter().zip([0.125, 0.09375]).all(|(value, expected)|
                        value.as_f64().is_some_and(|actual| (actual - expected).abs() < 0.001)));
            if roundtrip[key]["asset_target"]["collision"] != true || !collision_matches(&roundtrip["saved_collision"])
                || (observed["entry"] != "production-SceneRuntimePlugin"
                    && !collision_matches(&observed["current_author_document"]["blender_mesh_collision"]))
            {
                return Err("保存或重开时没有采用 Blender 模型的真实台阶碰撞".into());
            }
            // 摆放文件持久化模型外链。核对模型字节仍与 Blender 实际出件相同，且位于场景自有资产目录。
            let model = assets.join(&expected.asset_path).canonicalize().map_err(|error| error.to_string())?;
            let models = assets.join(relative).parent().ok_or("缺少场景自有目录")?.join("models");
            if !expected.asset_target.is_scene0() || model.parent() != Some(models.as_path()) {
                return Err("保存的 Blender GLB 不在场景自有 models 目录".into());
            }
            let bytes = fs::read(model).map_err(|error| error.to_string())?;
            if !bytes.starts_with(b"glTF") || roundtrip["blender"]["first"]["glb_sha256"]
                != feathrise_scene_content::authoring_io::bytes_hash(&bytes)
            {
                return Err("保存后正式 GLB 与 Blender 出件摘要不一致".into());
            }
        }
    }
    for phase in ["first", "second"] {
        let session = roundtrip["blender"][phase]["session"].as_str().ok_or("缺少 Blender 会话身份")?;
        if Path::new(session).exists() {
            return Err(format!("正式保存且 Blender 退出后，会话缓存仍存在：{session}"));
        }
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
    verify_game_with_prior_manifest(root, output, None)
}

fn verify_game_with_prior_manifest(
    root: &Path,
    output: &Path,
    manifest_before_close: Option<&Path>,
) -> Result<Value, String> {
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
    if manifest_before_close.is_some()
        && !files.iter().any(|file| file["path"] == CLOSE_SCENE_MANIFEST_PATH)
    {
        return Err("退出前的工程运行缺少样例场景清单指纹".into());
    }
    for file in files {
        let relative = Path::new(file["path"].as_str().ok_or("工程源文件路径缺失")?);
        if relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        {
            return Err("运行报告源文件路径无效".into());
        }
        // F12 已经返回，关闭验收随后又保存了一次场景。样例清单用关闭前的真实磁盘字节核对。
        let source = match manifest_before_close {
            Some(before) if relative == Path::new(CLOSE_SCENE_MANIFEST_PATH) => before.to_path_buf(),
            _ => project.context.asset_roots[0].join(relative),
        };
        let bytes = fs::read(source).map_err(|e| e.to_string())?;
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
