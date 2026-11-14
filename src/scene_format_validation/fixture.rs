//! 在独占工作根创建生产空模板，协调两次作者进程与一次真实交付包启动。

use super::{
    CHILD, ENV, OWNER, PARENT, PURPOSE, WINDOW_REPORT, create_dir, read_json, write_json_new,
    write_new,
};
use crate::{
    build_process::{self, BuildControl},
    project::Project,
};
use feathrise_scene_content::{
    SCENE_CATALOG_ASSET_PATH, SCENE_MANIFEST_FILE_NAME, SCENE_PLACEMENTS_GLB_FILE_NAME,
    SCENE_SCHEMA_VERSION, SCENE_STARTUP_FILE_NAME, SceneCatalogAsset, SceneCatalogEntry, SceneId,
    SceneManifestAsset, SceneStartup,
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(super) struct ScenePair {
    pub parent: SceneId,
    pub child: SceneId,
}

pub(crate) fn run(target: PathBuf, report: PathBuf) -> Result<(), String> {
    if !report.is_absolute() || report.exists() {
        return Err("场景格式验收报告必须为本轮新的绝对路径".into());
    }
    fs::create_dir_all(report.parent().ok_or("报告缺少父目录")?)
        .map_err(|error| error.to_string())?;
    let work = report.with_extension("work");
    create_dir(&work)?;
    let work = work.canonicalize().map_err(|error| error.to_string())?;
    let mut proof = json!({"schema":1,"purpose":PURPOSE,"work":work,"os_input_verified":false});
    let result = exercise(&work, &target, &mut proof);
    proof["passed"] = json!(result.is_ok());
    proof["error"] = json!(result.as_ref().err());
    write_json_new(&report, &proof)?;
    result
}

fn exercise(work: &Path, target: &Path, proof: &mut Value) -> Result<(), String> {
    let root = crate::project_template::create_with_control(
        &work.join("project"),
        "场景格式验收",
        &crate::project_template_root(),
        &BuildControl::default(),
    )?;
    let project = crate::project::open(&root)?;
    if project.root != work.join("project") {
        return Err("新建工程返回路径不属于本轮独占根".into());
    }
    verify_empty(&project)?;
    enable_runtime_validation(&project)?;
    // 本轮预览与导出使用同一正式 Debug/Cargo 配置及调用方给出的既有缓存。
    let launch = project.context.ensure_data_directory()?.join("launch.json");
    write_json_new(
        &launch,
        &json!({"format":1,"mode":"saved_world","profile":"debug","backend":"cargo"}),
    )?;
    write_json_new(
        &project.root.join(OWNER),
        &json!({"schema":1,"purpose":PURPOSE,"project_id":project.id,"project_root":project.root,"work":work}),
    )?;
    proof["project"] = json!(project.root);
    proof["author"] = run_editor(work, target, &project, "author", 600)?;
    let (scenes, saved) = verify_saved(&project)?;
    if proof["author"]["startup_save_completed"] != true
        || proof["author"]["child_save_sequence"].as_u64().unwrap_or(0) == 0
        || proof["author"]["parent_save_sequence"]
            .as_u64()
            .unwrap_or(0)
            == 0
    {
        return Err("真实作者窗口缺少创建后内容与启动场景的保存回执".into());
    }
    proof["disk"] = saved.clone();
    proof["reopen"] = run_editor(work, target, &project, "reopen", 3300)?;
    if proof["reopen"]["pid"] == proof["author"]["pid"]
        || proof["reopen"]["parent_scene_id"] != json!(scenes.parent)
        || proof["reopen"]["child_scene_id"] != json!(scenes.child)
        || proof["reopen"]["preview"]["scene_id"] != json!(scenes.parent)
        || proof["reopen"]["preview"]["playing"] != true
        || verify_saved(&project)?.1 != saved
    {
        return Err("重开或预览未核对同一份已保存的场景身份".into());
    }
    proof["delivery"] = run_delivery(work, &project, &scenes, &proof["reopen"]["export"])?;
    Ok(())
}

fn enable_runtime_validation(project: &Project) -> Result<(), String> {
    // 仅本轮自建项目附带游戏端截图探针。正常模板和用户项目的 features 无需改动。
    let text = fs::read_to_string(&project.manifest).map_err(|error| error.to_string())?;
    let mut manifest = toml::from_str::<toml::Value>(&text).map_err(|error| error.to_string())?;
    let features = manifest
        .get_mut("dependencies")
        .and_then(|table| table.get_mut("feathrise_runtime"))
        .and_then(|sdk| sdk.get_mut("features"))
        .and_then(toml::Value::as_array_mut)
        .ok_or("正式空模板缺少 SDK 特性表")?;
    if !features
        .iter()
        .any(|feature| feature.as_str() == Some("validation"))
    {
        features.push(toml::Value::String("validation".into()));
    }
    fs::write(
        &project.manifest,
        toml::to_string(&manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

pub(super) fn verify_empty(project: &Project) -> Result<(), String> {
    let assets = project
        .context
        .asset_roots
        .first()
        .ok_or("验收工程缺少资产根")?;
    let catalog: SceneCatalogAsset = read_asset(&assets.join(SCENE_CATALOG_ASSET_PATH))?;
    if catalog.schema_version != SCENE_SCHEMA_VERSION
        || !catalog.scenes.is_empty()
        || project.root.join(SCENE_STARTUP_FILE_NAME).exists()
    {
        return Err("场景格式验收必须从无场景和启动选择的正式空模板开始".into());
    }
    Ok(())
}

fn read_asset<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_value(read_json(path)?)
        .map_err(|error| format!("读取场景文件 {} 失败：{error}", path.display()))
}

fn find_scene<'a>(
    catalog: &'a SceneCatalogAsset,
    name: &str,
) -> Result<&'a SceneCatalogEntry, String> {
    let entry = catalog
        .scenes
        .iter()
        .find(|entry| entry.display_name == name)
        .ok_or_else(|| format!("正式目录缺少{name}"))?;
    if entry.folder != format!("scenes/{}", entry.id) {
        return Err(format!("正式目录将{name}指向了错误的资产目录"));
    }
    Ok(entry)
}

pub(super) fn verify_saved(project: &Project) -> Result<(ScenePair, Value), String> {
    let assets = project
        .context
        .asset_roots
        .first()
        .ok_or("验收工程缺少资产根")?;
    let catalog: SceneCatalogAsset = read_asset(&assets.join(SCENE_CATALOG_ASSET_PATH))?;
    if catalog.schema_version != SCENE_SCHEMA_VERSION || catalog.scenes.len() != 2 {
        return Err("落盘场景目录没有记录本轮创建的两个场景".into());
    }
    let parent = find_scene(&catalog, PARENT)?;
    let child = find_scene(&catalog, CHILD)?;
    if catalog
        .scenes
        .first()
        .is_none_or(|entry| entry.id != child.id || !entry.published)
    {
        return Err("启动验证需要与目录默认首项不同的显式父场景选择".into());
    }
    let parent_doc: SceneManifestAsset =
        read_asset(&assets.join(&parent.folder).join(SCENE_MANIFEST_FILE_NAME))?;
    let child_doc: SceneManifestAsset =
        read_asset(&assets.join(&child.folder).join(SCENE_MANIFEST_FILE_NAME))?;
    let placements_path = child.placements_path();
    let child_objects = feathrise_scene_content::read_scene_placements(
        &fs::read(assets.join(&placements_path)).map_err(|error| error.to_string())?,
        &placements_path,
    )?;
    if parent_doc.id != parent.id
        || child_doc.id != child.id
        || child_objects.scene_id != child.id
        || parent_doc.schema_version != SCENE_SCHEMA_VERSION
        || child_doc.schema_version != SCENE_SCHEMA_VERSION
        || child_objects.schema_version != SCENE_SCHEMA_VERSION
        || !child.placements_glb
        || !parent.placements_glb
        || child_objects.placements.len() != 1
        || parent_doc.subscenes.len() != 1
        || parent_doc.subscenes[0].source_scene_id != child.id
        || !child_doc.subscenes.is_empty()
    {
        return Err("落盘清单、子场景引用或源场景立方体的身份不一致".into());
    }
    let startup: SceneStartup = read_asset(&project.root.join(SCENE_STARTUP_FILE_NAME))?;
    startup.validate(&project.id)?;
    if startup.selected() != Some(parent.id.as_str()) {
        return Err("正式启动设置没有保存本轮选中的父场景".into());
    }
    let pair = ScenePair {
        parent: parent.id.clone(),
        child: child.id.clone(),
    };
    let proof = json!({"catalog":catalog,"parent":parent_doc,"child":child_doc,"child_placements":child_objects,"startup":startup});
    Ok((pair, proof))
}

fn run_editor(
    work: &Path,
    target: &Path,
    project: &Project,
    mode: &str,
    seconds: u64,
) -> Result<Value, String> {
    let directory = work.join(mode);
    create_dir(&directory)?;
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    clear_other_validation(&mut command);
    command
        .args(["--native-editor", "--project"])
        .arg(&project.root)
        .arg("--target-dir")
        .arg(target)
        .env(ENV, mode);
    crate::native_validation::isolate_author_command(&mut command, &directory)?;
    println!(
        "FEATHRISE_SCENE_FORMAT_VALIDATION stage={mode} project={}",
        project.root.display()
    );
    let process = build_process::run(
        &command,
        &BuildControl::default(),
        Some(Duration::from_secs(seconds)),
        tail,
    )?;
    write_new(
        &directory.join("process.log"),
        format!("{}\n{}", process.stdout, process.stderr).as_bytes(),
    )?;
    let window = read_json(&directory.join(WINDOW_REPORT))?;
    if process.exit_code != 0 || window["passed"] != true {
        return Err(format!(
            "真实作者窗口 {mode} 失败，退出码 {}：{}",
            process.exit_code, window["error"]
        ));
    }
    verify_png(&directory.join("scene-tree.png"), &window["screenshot"])?;
    Ok(window)
}

fn run_delivery(
    work: &Path,
    project: &Project,
    scenes: &ScenePair,
    receipt: &Value,
) -> Result<Value, String> {
    let package = work.join("delivery");
    let executable = PathBuf::from(
        receipt["executable"]
            .as_str()
            .ok_or("实际导出回执缺少程序")?,
    )
    .canonicalize()
    .map_err(|error| error.to_string())?;
    if receipt["directory"] != json!(package) || executable.parent() != Some(package.as_path()) {
        return Err("实际导出回执没有指向本轮交付包".into());
    }
    let mut files = vec![
        PathBuf::from(SCENE_STARTUP_FILE_NAME),
        Path::new("assets").join(SCENE_CATALOG_ASSET_PATH),
    ];
    for scene in [&scenes.parent, &scenes.child] {
        let folder = Path::new("assets/scenes").join(scene.as_str());
        files.push(folder.join(SCENE_MANIFEST_FILE_NAME));
        files.push(folder.join(SCENE_PLACEMENTS_GLB_FILE_NAME));
    }
    for relative in &files {
        if fs::read(project.root.join(relative)).map_err(|error| error.to_string())?
            != fs::read(package.join(relative)).map_err(|error| error.to_string())?
        {
            return Err(format!(
                "导出文件{}与场景保存后的原始字节不同",
                relative.display()
            ));
        }
    }
    let output = work.join("delivered-run");
    create_dir(&output)?;
    let mut command = Command::new(&executable);
    clear_other_validation(&mut command);
    command
        .env_remove("EDITOR_SAVED_PLAY")
        .env_remove("EDITOR_SAVED_PLAY_SCENE");
    // 从交付包之外的目录启动；应用需用包内描述与启动文件解析自身场景。
    command
        .current_dir(&output)
        .env("NEW_EDITOR_GAME_SMOKE_DIR", &output)
        .env("NEW_EDITOR_GAME_SMOKE_SECONDS", "180");
    println!(
        "FEATHRISE_SCENE_FORMAT_VALIDATION stage=delivery program={}",
        executable.display()
    );
    let process = build_process::run(
        &command,
        &BuildControl::default(),
        Some(Duration::from_secs(240)),
        tail,
    )?;
    write_new(
        &output.join("process.log"),
        format!("{}\n{}", process.stdout, process.stderr).as_bytes(),
    )?;
    let game = read_json(&output.join("game.json"))?;
    if process.exit_code != 0
        || game["passed"] != true
        || game["scene_id"] != json!(scenes.parent)
        || game["source"]["project_root"] != json!(package)
        || game["runtime"]["phase"] != "Ready"
        || !game["subscene_instances"]
            .as_array()
            .is_some_and(|instances| {
                instances.len() == 1
                    && instances[0]["source_scene_id"] == json!(scenes.child)
                    && instances[0]["active"] == true
            })
    {
        return Err(format!(
            "交付包未从包内启动设置进入父场景：{}",
            game["reason"]
        ));
    }
    crate::native_validation::verify_scene_view(&game)?;
    verify_png(&output.join("game.png"), &game["capture"])?;
    Ok(
        json!({"executable":executable,"bytes_verified":files,"runtime":game,"screenshot":output.join("game.png")}),
    )
}

fn verify_png(path: &Path, capture: &Value) -> Result<(), String> {
    let mut header = [0_u8; 24];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|error| error.to_string())?;
    let width = u32::from_be_bytes(header[16..20].try_into().unwrap());
    let height = u32::from_be_bytes(header[20..24].try_into().unwrap());
    if &header[..8] != b"\x89PNG\r\n\x1a\n"
        || &header[12..16] != b"IHDR"
        || width < 320
        || height < 240
        || capture["source"] != "Bevy ScreenshotCaptured"
        || capture["width"] != width
        || capture["height"] != height
        || capture["sha256"]
            .as_str()
            .is_none_or(|hash| hash.len() != 64)
    {
        return Err(format!("GPU回执与截图 {} 不一致", path.display()));
    }
    Ok(())
}

fn clear_other_validation(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        let text = key.to_string_lossy();
        if text.starts_with("NEW_EDITOR_")
            || text.starts_with("FEATHRISE_EMBEDDED_")
            || text.starts_with("FEATHRISE_WORKSPACE_")
            || text.contains("_VALIDATION")
            || text.contains("_SMOKE_")
            || text.starts_with("FEATHRISE_EXPORT_")
            || text.starts_with("FEATHRISE_CODE_ITERATION_")
        {
            command.env_remove(key);
        }
    }
}

fn tail(mut file: File) -> Result<String, String> {
    let size = file.metadata().map_err(|error| error.to_string())?.len();
    file.seek(SeekFrom::Start(size.saturating_sub(256 * 1024)))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
