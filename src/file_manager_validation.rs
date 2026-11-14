//! 桌面文件管理真窗口验收；协调器只创建并操作本轮独占的中性工程和夹具文件。

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

const ENV: &str = "FEATHRISE_FILE_MANAGER_VALIDATION";
const OWNER: &str = ".file-manager-owner.json";
const PURPOSE: &str = "owned-file-manager-validation";
const COPY_BYTES: &[u8] = b"feathrise-file-manager-copy-v1\n";
const MOVE_BYTES: &[u8] = b"feathrise-file-manager-move-v1\n";
const NESTED_BYTES: &[u8] = b"feathrise-file-manager-nested-v1\n";

pub(crate) fn run(target: PathBuf, report: PathBuf) -> Result<(), String> {
    if !report.is_absolute() || report.exists() {
        return Err("文件管理验收报告须为本轮新的绝对路径".into());
    }
    fs::create_dir_all(report.parent().ok_or("文件管理报告缺少父目录")?)
        .map_err(|error| error.to_string())?;
    let work = report.with_extension("work");
    fs::create_dir(&work).map_err(|error| format!("文件管理工作目录必须独占新建：{error}"))?;
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
        &work.join("project"), "文件桌面验收", &crate::project_template_root(), &BuildControl::default(),
    )?;
    let project = crate::project::open(&root)?;
    if project.root != work.join("project") || project.context.asset_roots != [root.join("assets")] {
        return Err("文件管理夹具工程路径与资产根不符".into());
    }
    let source = root.join("assets/整理来源");
    let destination = root.join("assets/整理目标");
    fs::create_dir(&source).and_then(|_| fs::create_dir(&destination))
        .and_then(|_| fs::create_dir(source.join("嵌套"))).map_err(|error| error.to_string())?;
    for (name, bytes) in [
        ("copy-me.txt", COPY_BYTES), ("move-me.txt", MOVE_BYTES),
        ("remove-me.txt", b"feathrise-file-manager-recycle-v1\n".as_slice()),
        ("嵌套/inside.txt", NESTED_BYTES),
    ] {
        write_new(&source.join(name), bytes)?;
    }
    let browser_assets = root.join("assets/浏览一致性");
    fs::create_dir(&browser_assets).map_err(|error| error.to_string())?;
    let browser_original = crate::project_browser_validation::fixture::prepare(&browser_assets)?;
    write_new(&browser_assets.join("notes.md"), b"FILE_MANAGER_SEARCH_TOKEN\n")?;
    // 明确的缺失模型引用只放在本轮自建工程的原检查范围内，供唯一搜索 worker 回报真实命中。
    let config = root.join("assets/config");
    fs::create_dir_all(&config).map_err(|error| error.to_string())?;
    let missing_fixture = config.join("文件管理验收缺失.json");
    let missing_bytes = b"{\"model_path\":\"file-manager-validation/absent.glb\"}\n";
    write_new(&missing_fixture, missing_bytes)?;
    proof["asset_fixtures_before_sha256"] = json!(browser_original);
    write_new(&work.join(OWNER), &serde_json::to_vec(&owner(&project, work)).map_err(|error| error.to_string())?)?;
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    // 每轮自动操作只接受独占夹具；旧验收变量不可意外启动另一条作者自动流程。
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.contains("_VALIDATION") || name.contains("_SMOKE_") || name.starts_with("FEATHRISE_WORKSPACE_") {
            command.env_remove(key);
        }
    }
    command.args(["--native-editor", "--project"]).arg(&root)
        .arg("--target-dir").arg(target).env(ENV, work);
    crate::native_validation::isolate_author_command(&mut command, work)?;
    let child = build_process::run(&command, &BuildControl::default(), Some(Duration::from_secs(270)), tail)?;
    write_new(&work.join("process.log"), format!("{}\n{}", child.stdout, child.stderr).as_bytes())?;
    proof["exit_code"] = json!(child.exit_code);
    proof["window"] = read_json(&work.join("window.json"))?;
    if child.exit_code != 0 || proof["window"]["passed"] != true {
        return Err(format!("文件桌面窗口退出码 {}：{}", child.exit_code, proof["window"]["error"]));
    }
    if proof["window"]["context_menu_verified"] != true || proof["window"]["new_file_types_verified"] != true
        || proof["window"]["menu_width_verified"] != true
        || proof["window"]["steps"]["fixed_sidebar_toolbar_removed"] != true
        || proof["window"]["steps"]["fixed_blank_cancel_keeps_empty_selection"].is_null()
    {
        return Err("右键、新建子菜单或固定栏空白选择没有实际窗口证据".into());
    }
    // 从已退出的真实作者进程外重新读取磁盘；内存回执本身不能替代文件操作完成。
    for (path, expected) in [
        (source.join("copy-me.txt"), COPY_BYTES),
        (source.join("嵌套/inside.txt"), NESTED_BYTES),
        (destination.join("renamed.txt"), COPY_BYTES),
        (destination.join("move-me.txt"), MOVE_BYTES),
        (destination.join("嵌套/inside.txt"), NESTED_BYTES),
    ] {
        let bytes = fs::read(&path).map_err(|error| format!("文件管理磁盘回读 {}：{error}", path.display()))?;
        if bytes != expected { return Err(format!("文件管理磁盘字节不符：{}", path.display())); }
    }
    if source.join("move-me.txt").exists() || source.join("remove-me.txt").exists()
        || destination.join("copy-me.txt").exists()
        || !destination.join("新建整理目录").is_dir()
    {
        return Err("移动、回收删除或新建目录的最终磁盘结构不符".into());
    }
    proof["disk_reread_verified"] = json!(true);
    let mut files = serde_json::Map::new();
    for name in ["中文笔记.txt", "中文说明.md", "中文空场景.glb"] {
        let path = destination.join(name);
        let bytes = fs::read(&path).map_err(|error| format!("宿主回读新建文件 {}：{error}", path.display()))?;
        let digest = sha256(&bytes);
        if proof["window"]["steps"]["new_files_reread"]["files"][name]["sha256"] != digest {
            return Err(format!("新建文件 {name} 的退出后字节与作者后台回读不符"));
        }
        let mut file = json!({"bytes":bytes.len(),"sha256":digest});
        if name.ends_with(".glb") {
            file["empty_scene"] = verify_empty_glb(&bytes)?;
        } else {
            std::str::from_utf8(&bytes).map_err(|error| format!("新建文档不是UTF-8：{error}"))?;
            if !bytes.is_empty() { return Err(format!("新建文档 {name} 不为空")); }
            file["utf8"] = json!(true);
        }
        files.insert(name.into(), file);
    }
    proof["new_file_types_reread_verified"] = json!(true);
    proof["new_files"] = Value::Object(files);
    let browser_after = crate::project_browser_validation::fixture::fingerprints(&browser_assets)?;
    if browser_after != browser_original
        || fs::read(browser_assets.join("notes.md")).map_err(|error| error.to_string())? != b"FILE_MANAGER_SEARCH_TOKEN\n"
        || fs::read(missing_fixture).map_err(|error| error.to_string())? != missing_bytes
    {
        return Err("浮窗资产浏览验收改变了中性素材的磁盘字节".into());
    }
    proof["asset_fixtures_after_sha256"] = json!(browser_after);
    proof["asset_fixture_bytes_preserved"] = json!(true);
    if proof["window"]["steps"]["floating_thumbnail_grid"]["verified"] != true {
        return Err("活动窗缩略图的实窗尺寸、滚轮和目录标签证据缺失".into());
    }
    let isolation = &proof["window"]["steps"]["floating_toolbar_sidebar_isolation"];
    if isolation["fixed_sidebar_hidden"] != true || isolation["returned_search_closed"] != true {
        return Err("浮窗工具点击与固定栏隔离的实窗证据缺失".into());
    }
    for name in ["files-context-menu.png", "files-new-submenu.png", "files-desktop.png", "files-restored.png",
        "files-assets-thumbnails.png", "files-assets-clips.png", "files-assets-services.png",
        "files-assets-grid-compact.png", "files-assets-grid-maximized.png"] {
        if proof["window"]["screenshots"][name].is_null() { return Err(format!("缺少本轮文件桌面截图 {name}")); }
    }
    let captures = proof["window"]["screenshots"].as_object().ok_or("文件窗口截图回执不是对象")?;
    for (name, capture) in captures {
        if Path::new(name).components().count() != 1 || !name.ends_with(".png") { return Err("文件窗口截图名称越出本轮工作目录".into()); }
        let bytes = fs::read(work.join(name)).map_err(|error| error.to_string())?;
        let header = bytes.get(..24).ok_or("文件窗口截图缺少 PNG 文件头")?;
        if &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR"
            || capture["width"] != u32::from_be_bytes(header[16..20].try_into().unwrap())
            || capture["height"] != u32::from_be_bytes(header[20..24].try_into().unwrap())
            || capture["sha256"] != sha256(&bytes)
        {
            return Err(format!("文件窗口截图 {name} 与 GPU 回执不符"));
        }
    }
    proof["context_menu_verified"] = json!(true);
    Ok(())
}

fn verify_empty_glb(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() < 20 || bytes.len() > 4096 || &bytes[..4] != b"glTF"
        || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != 2
        || u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize != bytes.len()
        || &bytes[16..20] != b"JSON"
    {
        return Err("退出后新建GLB的二进制头、版本或总长度无效".into());
    }
    let length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    if !length.is_multiple_of(4) || length.checked_add(20) != Some(bytes.len()) { return Err("新建空GLB的JSON块长度无效".into()); }
    let document: Value = serde_json::from_slice(&bytes[20..]).map_err(|error| format!("退出后解析新建GLB JSON失败：{error}"))?;
    let scenes = document["scenes"].as_array().ok_or("新建GLB缺少场景数组")?;
    if document["asset"]["version"] != "2.0" || document["scene"] != 0 || scenes.len() != 1
        || scenes[0].get("nodes").is_some_and(|value| value.as_array().is_none_or(|nodes| !nodes.is_empty()))
        || ["nodes", "meshes", "skins", "animations"].into_iter().any(|key| document.get(key)
            .is_some_and(|value| value.as_array().is_none_or(|items| !items.is_empty())))
    {
        return Err("退出后新建GLB不是单个默认空场景".into());
    }
    Ok(json!({"version":2,"default_scene":0,"scenes":1,"nodes":0,"meshes":0,"skins":0,"animations":0}))
}

pub(crate) fn preflight(project: &Project) -> Result<Option<PathBuf>, String> {
    let Some(work) = std::env::var_os(ENV).map(PathBuf::from) else { return Ok(None); };
    if !work.is_absolute() || work.canonicalize().ok().as_ref() != Some(&work)
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
        return Err("文件管理自动验收未通过工程、偏好与插件的夹具归属校验".into());
    }
    for name in ["整理来源", "整理目标"] {
        let path = project.root.join("assets").join(name);
        if path.canonicalize().ok().as_ref() != Some(&path) {
            return Err("文件管理自动验收目录缺失或含重定向".into());
        }
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
    File::open(path).and_then(|file| file.take(256 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("无法读取文件管理证据 {}：{error}", path.display()))?;
    if bytes.len() > 256 * 1024 { return Err("文件管理证据超过预算".into()); }
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    OpenOptions::new().write(true).create_new(true).open(path)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|error| format!("无法独占写入文件管理证据 {}：{error}", path.display()))
}

fn tail(mut file: File) -> Result<String, String> {
    let size = file.metadata().map_err(|error| error.to_string())?.len();
    file.seek(SeekFrom::Start(size.saturating_sub(128 * 1024))).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
