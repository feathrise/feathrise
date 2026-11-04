//! 同窗口验收的有界磁盘工作：自有目录、完整原生报告轮询、设置重读与最终报告发布。

use super::resources::{IoResult, NativeReport, Paths, same_path};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const REPORT_LIMIT: u64 = 8 * 1024 * 1024;

pub(super) fn prepare(directory: PathBuf) -> Result<IoResult, String> {
    let parent = directory.join("Projects");
    // 只采用本轮独占创建的空父目录；不重用用户已有工程或旧失败现场，也不删除任何目录。
    fs::create_dir(&parent).map_err(|error| format!("无法独占创建验收工程父目录：{error}"))?;
    let parent = parent.canonicalize().map_err(|error| error.to_string())?;
    // 现代空模板不再带示例地图。只为本次新建的Projects授予原生夹具租约，不扩大到普通工程。
    let mut owner = OpenOptions::new().write(true).create_new(true)
        .open(parent.join(".feathrise-native-validation-owner.json"))
        .map_err(|error| format!("无法独占标记同窗口夹具根：{error}"))?;
    owner.write_all(&serde_json::to_vec(&json!({
        "schema":1,"purpose":"ordinary-author-boundary-validation","work_root":parent
    })).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
    Ok(IoResult::Prepared(Paths {
        a: parent.join("中文 工程A"),
        b: parent.join("中文 工程B"),
        parent,
        missing: directory.join("不存在的 工程"),
        default: directory.join("默认 工程目录"),
    }))
}

/// 原创建事务完成后、候选窗口启动前执行；先确认当前进程持有同窗口验收根，再复用空骨架校验。
pub(super) fn prepare_created(directory: &Path, root: &Path, label: &str) -> Result<(), String> {
    let directory = directory.canonicalize().map_err(|error| error.to_string())?;
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    if root.parent() != Some(directory.join("Projects").as_path()) {
        return Err("同窗口夹具拒绝非本轮Projects直接子目录".into());
    }
    let bytes = read_optional(&directory.join(".feathrise-workspace-smoke-owner.json"))?
        .ok_or("同窗口夹具缺少当前进程的独占验收标记")?;
    let owner: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if owner["pid"] != std::process::id()
        || owner["directory"] != json!(directory)
    {
        return Err("同窗口夹具标记不属于当前进程和验收根".into());
    }
    crate::native_validation::prepare_workspace_fixture(&root, label)
}

pub(super) fn native_report(
    directory: PathBuf,
    index: usize,
    expected: PathBuf,
    first_a: Option<Arc<Value>>,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
) -> Result<IoResult, String> {
    let file = directory
        .join(format!("editor-{index}"))
        .join("native-editor.json");
    let mut previous: Option<Vec<u8>> = None;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("同窗口验收报告轮询已取消".into());
        }
        if Instant::now() >= deadline {
            return Err(format!("等待原作者报告超时：{}", file.display()));
        }
        let bytes = match read_optional(&file)? {
            Some(bytes) => bytes,
            None => {
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        // 原夹具先保存GPU图片再写JSON。连续两次完整原字节一致后解析，不把半次write误报为坏档。
        if previous.as_ref() != Some(&bytes) {
            previous = Some(bytes);
            thread::sleep(Duration::from_millis(100));
            continue;
        }
        let report: Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("原作者报告不是完整JSON：{}：{error}", file.display()))?;
        verify_native(&report, &expected, index == 1, first_a.as_deref())?;
        let game_proof = if index == 1 {
            let game_file = directory.join("game-f12/game.json");
            let bytes = read_optional(&game_file)?.ok_or("已返回编辑器但缺少实际运行报告")?;
            let game: Value = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            crate::native_validation::verify_scene_view(&game)?;
            let root = game["source"]["project_root"]
                .as_str()
                .ok_or("运行实例缺少工程根身份")?;
            if game["passed"] != true
                || game["schema"] != 2
                || game["entry"] != "production-SceneRuntimePlugin"
                || game["primary_scene_view_count"] != 1
                || game["primary_scene_views"][0]["active"] != true
                || game["mesh_entities"].as_u64().unwrap_or(0) == 0
                || !same_path(Path::new(root), &expected)
            {
                return Err("工程运行没有确认当前工程的完整生产入口与GPU输出".into());
            }
            verify_saved_placement(&game, &report)?;
            json!({"path":game_file,"sha256":hash(&bytes),"saved_placement_in_real_world":true})
        } else {
            Value::Null
        };
        let png = file.with_file_name("native-editor.png");
        let mut header = [0u8; 24];
        File::open(&png)
            .and_then(|mut file| file.read_exact(&mut header))
            .map_err(|error| format!("原作者GPU截图缺失或不完整：{error}"))?;
        if &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
            return Err("原作者GPU截图不是PNG输出".into());
        }
        let width = u32::from_be_bytes(header[16..20].try_into().unwrap());
        let height = u32::from_be_bytes(header[20..24].try_into().unwrap());
        if width < 100 || height < 100 {
            return Err("原作者GPU截图尺寸不足，不能作为完整工作台输出".into());
        }
        let document = serde_json::to_vec(&report["current_author_document"])
            .map_err(|error| error.to_string())?;
        let layout =
            serde_json::to_vec(&scene_layout(&report)).map_err(|error| error.to_string())?;
        let proof = json!({
            "index":index,"path":file,"sha256":hash(&bytes),"passed":true,
            "project_root":report["project_root"],"asset_root":report["asset_root"],
            "original_header_layout_ready":report["original_header_layout_ready"],
            "editor_3d_ready":report["editor_3d_ready"],"font_loaded":report["font_loaded"],
            "author_save_requested":report["author_save_requested"],
            "author_save_complete":report["author_save"]["complete"],
            "saved_play_requested":report["saved_play_requested"],
            "saved_play_result":report["saved_play_result"],
            "saved_play_active_after_return":report["saved_play_active_after_return"],
            "scripted_input_ownership":report["scripted_input_ownership"],
            "current_author_document_sha256":hash(&document),
            "saved_scene_layout_sha256":hash(&layout),
            "reopened_saved_scene_layout_equal":first_a.is_some(),
            "saved_game_proof":game_proof,
            "gpu_image":{"path":png,"width":width,"height":height}
        });
        return Ok(IoResult::Native(NativeReport {
            index,
            source: Arc::new(report),
            proof,
        }));
    }
}

fn verify_native(
    report: &Value,
    expected: &Path,
    saved_play: bool,
    first_a: Option<&Value>,
) -> Result<(), String> {
    if report["passed"] != true || report["schema"] != 2 {
        let reason = report["author_save"]["error"]
            .as_str()
            .or_else(|| report["author_save_error"].as_str())
            .or_else(|| report["map_request_error"].as_str())
            .or_else(|| report["saved_play_result"]["Err"].as_str())
            .unwrap_or("原界面、文档或GPU输出未完整就绪");
        return Err(format!("原作者有限验收未通过：{reason}"));
    }
    let actual = report["project_root"]
        .as_str()
        .map(Path::new)
        .ok_or("原作者报告缺少工程根身份")?;
    if !same_path(actual, expected) {
        return Err(format!("原作者报告工程身份不符：{}", actual.display()));
    }
    for field in [
        "original_header_layout_ready",
        "editor_3d_ready",
        "font_loaded",
        "project_catalog_ready",
    ] {
        if report[field] != true {
            return Err(format!("原作者报告缺少真实就绪条件：{field}"));
        }
    }
    if !report["current_author_document"].is_object() {
        return Err("原作者报告缺少实际文档快照".into());
    }
    if saved_play
        && (report["author_save_requested"] != true
            || report["author_save"]["complete"] != true
            || report["saved_play_requested"] != true
            || report["saved_play_result"].get("Ok").is_none()
            || report["saved_play_active_after_return"] != false)
    {
        return Err("A工程没有完整确认原编辑、保存、工程运行及返回回执".into());
    }
    if first_a.is_some_and(|first| scene_layout(first) != scene_layout(report)) {
        return Err("A工程重开后的已保存地图、网格对象或放置布局不同".into());
    }
    if let Some(first) = first_a {
        verify_saved_placement(report, first)?;
    }
    Ok(())
}

fn verify_saved_placement(observed: &Value, first_author: &Value) -> Result<(), String> {
    let saved = &first_author["author_save"];
    let id = saved["new_placement_id"]
        .as_str()
        .ok_or("保存没有确认新对象稳定ID")?;
    let position = saved["translation"]
        .as_array()
        .filter(|value| value.len() == 3)
        .ok_or("保存没有确认对象三维位置")?;
    let present = observed["placement_instances"]
        .as_array()
        .is_some_and(|instances| {
            instances.iter().any(|instance| {
                instance["id"] == id
                    && instance["position"].as_array().is_some_and(|actual| {
                        actual.len() == 3
                            && actual.iter().zip(position).all(|(left, right)| {
                                left.as_f64()
                                    .zip(right.as_f64())
                                    .is_some_and(|(left, right)| (left - right).abs() < 0.001)
                            })
                    })
            })
        });
    if !present {
        return Err(format!("保存对象{id}没有在实际World按最新位置还原"));
    }
    Ok(())
}

fn scene_layout(report: &Value) -> Value {
    let document = &report["current_author_document"];
    // 本轮编辑的是地图放置布局。重开夹具会聚焦首个私有网格，而首轮保存停在地图总览；
    // 该焦点及其投影地表/植被并非同一作者文档，不能拿未持久化的视图状态冒充布局差异。
    json!({"map_id":document["map_id"],"terrains":document["terrains"],"placements":document["placements"]})
}

pub(super) fn preferences(file: PathBuf, home: PathBuf, paths: Paths) -> Result<IoResult, String> {
    let preferences = crate::user_preferences::Preferences::load_at(&file, &home)?;
    if !same_path(&preferences.default_project_directory, &paths.default) {
        return Err("重新读取个人设置后默认工程目录不一致".into());
    }
    for path in [&paths.a, &paths.b] {
        if preferences
            .recent_projects
            .iter()
            .filter(|entry| same_path(&entry.path, path))
            .count()
            != 1
        {
            return Err("最近工程未正确持久化或同一工程重复记录".into());
        }
    }
    if !preferences
        .recent_projects
        .first()
        .is_some_and(|entry| same_path(&entry.path, &paths.a))
    {
        return Err("最后重开的A工程没有成为最近记录首项".into());
    }
    Ok(IoResult::Preferences(json!({
        "file":file,"default_directory":preferences.default_project_directory,
        "reloaded_from_disk":true,"a_b_records_unique":true,"a_is_most_recent":true,
        "recent_count":preferences.recent_projects.len()
    })))
}

pub(super) fn publish(directory: PathBuf, report: Value) -> Result<IoResult, String> {
    let path = directory.join("unified-workspace.json");
    let temporary = directory.join(".unified-workspace.pending");
    let bytes = serde_json::to_vec_pretty(&report).map_err(|error| error.to_string())?;
    if bytes.len() > 256 * 1024 {
        return Err("同窗口汇总报告超过256KiB，未发布".into());
    }
    if path.exists() {
        return Err("同窗口汇总报告已经存在，拒绝覆盖".into());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| format!("无法独占准备同窗口汇总报告：{error}"))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("无法完整写入同窗口汇总报告：{error}"))?;
    drop(file);
    fs::rename(&temporary, &path).map_err(|error| format!("无法发布同窗口汇总报告：{error}"))?;
    Ok(IoResult::Written(path))
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法读取原作者报告：{error}")),
    };
    let mut bytes = Vec::new();
    file.take(REPORT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("原作者报告读取失败：{error}"))?;
    if bytes.len() as u64 > REPORT_LIMIT {
        return Err("原作者报告超过8MiB有界读取预算".into());
    }
    Ok(Some(bytes))
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod fixture_owner_tests {
    use super::*;

    struct OwnedRoot(PathBuf);
    impl Drop for OwnedRoot {
        fn drop(&mut self) {
            let temporary = std::env::temp_dir().canonicalize().unwrap();
            assert_eq!(self.0.parent(), Some(temporary.as_path()));
            assert_eq!(self.0.canonicalize().unwrap(), self.0);
            assert!(self.0.file_name().unwrap().to_string_lossy().starts_with("feathrise-workspace-owner-"));
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn only_fresh_projects_under_current_workspace_owner_receive_fixtures() {
        let directory = std::env::temp_dir().join(format!("feathrise-workspace-owner-{}-{}",
            std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        fs::create_dir(&directory).unwrap();
        let owned = OwnedRoot(directory.canonicalize().unwrap());
        let IoResult::Prepared(paths) = prepare(owned.0.clone()).unwrap() else { panic!("应准备独占目录") };
        let project = paths.parent.join("owned");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("Cargo.toml"), "[package]\nname='feathrise_game_template'\nversion='0.1.0'\n[dependencies.feathrise_runtime]\nversion='0.1.0'\nfeatures=[]\n").unwrap();
        fs::write(project.join("editor-project.json"), serde_json::to_vec(&json!({
            "format":2,"id":"c2ec3247-0bd1-422d-945e-5ddf94d6cad0","name":"owned",
            "package":"feathrise_game_template","cargo_manifest":"Cargo.toml",
            "asset_roots":["assets"],"editor_data":".editor"
        })).unwrap()).unwrap();
        feathrise_project::seed::create_empty(&project.join("assets")).unwrap();
        let catalog = project.join("assets/maps/catalog.map_catalog.json");
        let original = fs::read(&catalog).unwrap();
        assert!(prepare_created(&owned.0, &project, "owned").is_err());
        let marker = owned.0.join(".feathrise-workspace-smoke-owner.json");
        fs::write(&marker, serde_json::to_vec(&json!({"pid":0,"directory":owned.0})).unwrap()).unwrap();
        assert!(prepare_created(&owned.0, &project, "owned").is_err());
        assert_eq!(fs::read(&catalog).unwrap(), original);
        fs::write(&marker, serde_json::to_vec(&json!({"pid":std::process::id(),"directory":owned.0})).unwrap()).unwrap();
        assert!(prepare_created(&owned.0, &owned.0, "outside").is_err());
        prepare_created(&owned.0, &project, "owned").unwrap();
        let installed = fs::read(&catalog).unwrap();
        assert_ne!(installed, original);
        assert!(project.join("assets/scene/probe.glb").is_file());
        // 重复调用不能覆盖已安装的夹具，更不能覆盖作者后来产生的内容。
        assert!(prepare_created(&owned.0, &project, "owned").is_err());
        assert_eq!(fs::read(&catalog).unwrap(), installed);
    }
}
