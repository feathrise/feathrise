//! 角色姿势组与场景电梯的独占工程编排。作者、重开和游戏均使用独立进程。

use super::{author_window_command, fixture, read_author_screenshot, tail};
use crate::{
    build_process::{self, BuildControl},
    project, project_template,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(crate) const ENABLE: &str = "FEATHRISE_ANIMATION_WORKSPACES_VALIDATION";
const OUTPUT: &str = "FEATHRISE_ANIMATION_WORKSPACES_PROBE_DIR";
const GAME_SDK: &str = "FEATHRISE_ANIMATION_WORKSPACES_SDK";
const GAME_DIR: &str = "FEATHRISE_ANIMATION_WORKSPACES_GAME_DIR";
const OWNER: &str = ".animation-workspaces-owner.json";
const SCENE: &str = "M1-SAMPLE";
const HERO: &str = "roles/nested/hero.glb";
const ELEVATOR: &str = "validation-elevator";

pub(crate) fn install(
    app: &mut bevy::prelude::App,
    project: &project::Project,
) -> Result<(), String> {
    let Some(output) = std::env::var_os(OUTPUT).map(PathBuf::from) else {
        return Ok(());
    };
    if !output.is_absolute() {
        return Err("动画工作区验收输出须是本轮的绝对路径".into());
    }
    let output = output.canonicalize().map_err(|error| error.to_string())?;
    let parent = output.parent().ok_or("验收输出缺少独占父目录")?;
    if read_json(&parent.join(OWNER))? != owner(project) || project.root.parent() != Some(parent) {
        return Err("动画工作区验收未确认本轮工程归属".into());
    }
    app.insert_resource(bevy::winit::WinitSettings::continuous());
    use feathrise_editor_scene::standalone::{
        install_pose_group_validation, install_scene_motion_validation,
    };
    match output.file_name().and_then(|name| name.to_str()) {
        Some("pose-author") => install_pose_group_validation(
            app,
            output,
            project.context.asset_roots[0].clone(),
            false,
        ),
        Some("pose-reopen") => {
            install_pose_group_validation(app, output, project.context.asset_roots[0].clone(), true)
        }
        Some("scene-author") => install_scene_motion_validation(app, output, false),
        Some("scene-reopen") => install_scene_motion_validation(app, output, true),
        _ => return Err("未知的动画工作区验收阶段".into()),
    }
    Ok(())
}

fn owner(project: &project::Project) -> Value {
    json!({"schema":1,"purpose":"owned-animation-workspaces","project_root":project.root,"project_id":project.id})
}

pub(super) fn run(
    work: &Path,
    repository: &Path,
    target: &Path,
    control: &BuildControl,
    steps: &mut Vec<Value>,
) -> Result<(), String> {
    let label = "独立工程 A";
    let root =
        project_template::create_with_control(&work.join(label), label, repository, control)?;
    fixture::prepare(&root, label)?;
    let project = project::open(&root)?;
    let assets = &project.context.asset_roots[0];
    crate::project_browser_validation::fixture::prepare(assets)?;
    seed_elevator(assets)?;
    write_json(&work.join(OWNER), &owner(&project))?;

    let pose = editor_stage(
        &project,
        work,
        target,
        control,
        "pose-author",
        "pose-group",
        steps,
    )?;
    let current = editor_stage(
        &project,
        work,
        target,
        control,
        "pose-reopen",
        "pose-group",
        steps,
    )?;
    if pose["asset"] != HERO
        || pose["bones"] != 2
        || pose["history_cursor"] != 2
        || current["reopened"] != true
        || ["asset", "bones", "time", "hips_translation", "arm_rotation"]
            .into_iter()
            .any(|field| pose[field] != current[field])
    {
        return Err("同帧姿势保存前后缺少两根骨骼及单次历史证据".into());
    }
    let motion = editor_stage(
        &project,
        work,
        target,
        control,
        "scene-author",
        "scene-motion",
        steps,
    )?;
    let current = editor_stage(
        &project,
        work,
        target,
        control,
        "scene-reopen",
        "scene-motion",
        steps,
    )?;
    let glb = assets.join("scenes").join(SCENE).join("scene.glb");
    let channels = feathrise_editor_scene::standalone::scene_motion_validation::inspect_saved_glb(&glb, ELEVATOR)?;
    if motion["placement_id"] != ELEVATOR
        || current["placement_id"] != ELEVATOR
        || current["reopened"] != true
        || channels["position_keys"] != 3 || channels["rotation_keys"] != 3
        || glb.with_file_name("motion.scene_motion.json").exists()
    {
        return Err("主场景 GLB 的位置或旋转通道缺少保存和重开证据".into());
    }
    steps.push(json!({"stage":"main_scene_glb_animation","passed":true,"path":glb,"channels":channels}));
    let game = play(&project, work, target, control, &pose, &motion)?;
    steps.push(json!({"stage":"independent_game","passed":true,"report":game}));
    Ok(())
}

fn seed_elevator(assets: &Path) -> Result<(), String> {
    let path = assets
        .join("scenes")
        .join(SCENE)
        .join("placements.scene_placements.json");
    let mut document = read_json(&path)?;
    let records = document["placements"]
        .as_array_mut()
        .ok_or("缺少独占场景的摆放列表")?;
    if records.len() != 1 || records[0]["id"] != "sample-model" {
        return Err("独占场景的原有几何与已确认夹具不一致".into());
    }
    // 中性模型留给镜头辨识，被测电梯和骑乘角色使用无其他摆放的原点。
    records[0]["translation"][0] = json!(8.0);
    records.push(json!({
        "id":ELEVATOR, "asset_path":"",
        "asset_target":{"kind":"cuboid","size_mm":[3000,500,3000],"color_srgb":[100,146,187],
            "roughness_milli":700,"collision":true},
        "translation":[0.0,0.25,0.0],"yaw_degrees":0.0,"scale":[1.0,1.0,1.0],
    }));
    let placements = serde_json::from_value(document).map_err(|error| error.to_string())?;
    fixture::write_sample_scene_glb(assets, &placements, None)
}

fn editor_stage(
    project: &project::Project,
    work: &Path,
    target: &Path,
    control: &BuildControl,
    stage: &str,
    evidence: &str,
    steps: &mut Vec<Value>,
) -> Result<Value, String> {
    let directory = work.join(stage);
    let mut command = author_window_command(&project.root, target, &directory, true)?;
    command
        .env_remove("NEW_EDITOR_NATIVE_SMOKE_DIR")
        .env(OUTPUT, &directory);
    let child = build_process::run(&command, control, Some(Duration::from_secs(170)), tail)?;
    fs::write(
        directory.join("process.log"),
        format!("{}\n{}", child.stdout, child.stderr),
    )
    .map_err(|error| error.to_string())?;
    let report = read_json(&directory.join(format!("{evidence}.json")));
    if child.exit_code != 0 || report.is_err() {
        return Err(format!(
            "{stage} 退出 {}：{}\n{}",
            child.exit_code,
            report.map_or_else(|error| error, |value| value.to_string()),
            child.stderr
        ));
    }
    let report = report?;
    if report["passed"] != true {
        return Err(format!("{stage} 缺少成功回执：{report}"));
    }
    let screenshot = read_author_screenshot(&directory.join(format!("{evidence}.png")))?;
    steps.push(json!({"stage":stage,"passed":true,"report":report,"screenshot":screenshot}));
    Ok(report)
}

fn play(
    project: &project::Project,
    work: &Path,
    target: &Path,
    control: &BuildControl,
    pose: &Value,
    motion: &Value,
) -> Result<Value, String> {
    let sdk = std::env::var_os(GAME_SDK)
        .map(PathBuf::from)
        .ok_or("缺少本轮实现的本地 SDK 路径")?;
    let sdk = sdk.canonicalize().map_err(|error| error.to_string())?;
    let output = work.join("game");
    fs::create_dir(&output).map_err(|error| error.to_string())?;
    write_json(
        &output.join("expected.json"),
        &json!({"pose":pose,"motion":motion}),
    )?;
    prepare_game(project, &sdk)?;
    let mut lock = Command::new("cargo");
    lock.arg("generate-lockfile").current_dir(&project.root);
    let locked = build_process::run(&lock, control, Some(Duration::from_secs(120)), tail)?;
    if locked.exit_code != 0 {
        return Err(format!("独立游戏夹具锁定失败：{}", locked.stderr));
    }
    let mut command = Command::new("cargo");
    command
        .args([
            "run",
            "--locked",
            "--no-default-features",
            "--features",
            "validation",
        ])
        .current_dir(&project.root)
        .env("CARGO_TARGET_DIR", target)
        .env("EDITOR_SAVED_PLAY", "1")
        .env("EDITOR_SAVED_PLAY_SCENE", SCENE)
        .env_remove("NEW_EDITOR_GAME_SMOKE_DIR")
        .env(GAME_DIR, &output);
    let game = build_process::run(&command, control, Some(Duration::from_secs(1900)), tail)?;
    fs::write(
        output.join("process.log"),
        format!("{}\n{}", game.stdout, game.stderr),
    )
    .map_err(|error| error.to_string())?;
    let report = read_json(&output.join("game-motion.json"));
    if game.exit_code != 0 {
        return Err(format!(
            "独立游戏验收退出 {}：{}\n{}",
            game.exit_code,
            report.map_or_else(|error| error, |value| value.to_string()),
            game.stderr
        ));
    }
    let mut report = report?;
    if report["passed"] != true
        || report["animation"]["sampled_both_bones"] != true
        || report["platform"]["id"] != ELEVATOR
        || report["platform"]["up_and_down_with_rider"] != true
        || report["platform"]["jumped_off"] != true
        || report["platform"]["rotation_reached"] != true
    {
        return Err(format!("游戏未完整采用动画与可载人电梯：{report}"));
    }
    report["screenshot"] = read_author_screenshot(&output.join("game-motion.png"))?;
    Ok(report)
}

fn prepare_game(project: &project::Project, sdk: &Path) -> Result<(), String> {
    if !sdk.join("crates/feathrise_scene/Cargo.toml").is_file() || !sdk.join("Cargo.toml").is_file()
    {
        return Err("动画工作区验收须指向实现本轮电梯的 SDK 源码".into());
    }
    // 只有本轮生成的游戏增加观测插件与本地 SDK 绑定，公开模板及其锁不参与改写。
    let source = fs::read_to_string(&project.manifest).map_err(|error| error.to_string())?;
    let mut manifest: toml::Value = toml::from_str(&source).map_err(|error| error.to_string())?;
    let deps = manifest
        .get_mut("dependencies")
        .and_then(toml::Value::as_table_mut)
        .ok_or("本轮生成的游戏缺少依赖表")?;
    let runtime = deps
        .get_mut("feathrise_runtime")
        .and_then(toml::Value::as_table_mut)
        .ok_or("本轮生成的游戏缺少 SDK 依赖")?;
    for key in ["git", "rev", "tag", "branch", "version"] {
        runtime.remove(key);
    }
    runtime.insert("path".into(), sdk.to_string_lossy().into_owned().into());
    for (name, version) in [
        ("avian3d", "=0.8.0"),
        ("bevy-tnua", "=0.33.2"),
        ("bevy-tnua-avian3d", "=0.13.1"),
        ("serde_json", "1"),
    ] {
        deps.insert(name.into(), version.into());
    }
    let mut scene = toml::map::Map::new();
    scene.insert(
        "path".into(),
        sdk.join("crates/feathrise_scene")
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    deps.insert("feathrise_scene".into(), toml::Value::Table(scene));
    fs::write(
        &project.manifest,
        toml::to_string_pretty(&manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    let entry = project.root.join("src/main.rs");
    let generated = fs::read_to_string(&entry).map_err(|error| error.to_string())?;
    let original = "controller::ProjectScenePlugin { view },";
    if generated.matches(original).count() != 1 {
        return Err("本轮模板入口变更，无法安全安装游戏验收插件".into());
    }
    let generated = generated.replacen(
        original,
        "animation_workspaces_probe::ProbePlugin { view },",
        1,
    );
    fs::write(
        &entry,
        format!("{generated}\nmod animation_workspaces_probe;\n"),
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        project.root.join("src/animation_workspaces_probe.rs"),
        include_str!("../tests/fixtures/animation_workspaces_game.rs"),
    )
    .map_err(|error| error.to_string())
}

fn read_json(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}
