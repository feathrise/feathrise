//! 目录扫描真实文件系统回归；仅在独立临时目录创建夹具，不读取或修改作者 assets。

use std::path::PathBuf; // 临时目录的唯一明确所有权边界。
use std::sync::atomic::{AtomicU64, Ordering}; // 并行测试也不能复用相同夹具目录。
use std::time::{SystemTime, UNIX_EPOCH}; // 临时名称包含进程和时间，避免碰触现有文件。

use super::{
    AssetScanRequest, AssetTypeFilter, DirectoryFilters, DirectoryQuery, DirectorySnapshot, DirectoryWorker,
    FolderScope, scan_asset_folder, scan_asset_request, scan_directory,
}; // 直接测试后台正式扫描与完整路径搜索。
use feathrise_scene_content::SceneAssetKind; // 输出仍使用正式类型识别结果。

/// 私有临时目录在创建时冻结规范根，只回收本次测试亲自创建的明确子目录。
struct DirectoryFixture {
    root: PathBuf,   // 经 canonicalize 的本次测试唯一目录。
    parent: PathBuf, // 经 canonicalize 的系统临时目录，仅用作回收边界验证。
}

impl DirectoryFixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0); // 同一纳秒和同一进程内仍保证名字唯一。
        let parent = std::env::temp_dir().canonicalize().unwrap(); // 测试不在作者 assets 或工作区根创建文件。
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(); // 无需外部随机依赖。
        let root = parent.join(format!(
            "bevy-map-editor-asset-scan-{}-{nonce}-{}", // 固定专用前缀和完整唯一身份。
            std::process::id(),                         // 并行 Cargo 测试进程各自隔离。
            NEXT.fetch_add(1, Ordering::Relaxed)        // 同进程夹具彼此隔离。
        ));
        std::fs::create_dir(&root).unwrap(); // 必须亲自新建成功，已有目录绝不复用或覆盖。
        Self {
            root: root.canonicalize().unwrap(), // 后续目标都相对这一个规范目录。
            parent,                             // 析构前再次验证路径仍是直接受控子目录。
        }
    }

    fn add_file(&self, name: &str) {
        std::fs::OpenOptions::new()
            .create_new(true) // 夹具不能覆盖原有文件，即使误复用名称也立刻失败。
            .write(true) // 零字节场景文件已足够测试扫描，完全不加载 glTF。
            .open(self.root.join(name)) // 所有文件必须位于当前唯一临时根。
            .unwrap(); // 句柄此处立即释放，不积累数千打开文件。
    }
}

impl Drop for DirectoryFixture {
    fn drop(&mut self) {
        let canonical = self.root.canonicalize().unwrap(); // 回收前重新解析，防止目标变成其他路径。
        assert_eq!(canonical.parent(), Some(self.parent.as_path())); // 绝不能递归回收临时根或作者目录。
        assert!(
            canonical
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("bevy-map-editor-asset-scan-")
        ); // 仅允许本夹具保留前缀。
        std::fs::remove_dir_all(&canonical).unwrap(); // 只回收这次测试新建的受检目录和空文件。
    }
}

fn write_glb(path: &std::path::Path, json: serde_json::Value) {
    let mut json = serde_json::to_vec(&json).unwrap();
    json.resize(json.len().next_multiple_of(4), b' ');
    let mut bytes = b"glTF".to_vec();
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&((20 + json.len()) as u32).to_le_bytes());
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(b"JSON");
    bytes.extend_from_slice(&json);
    std::fs::write(path, bytes).unwrap();
}

fn listed_paths(snapshot: &DirectorySnapshot) -> Vec<&str> {
    snapshot.entries.iter().map(|entry| entry.asset_path.as_str()).collect()
}

#[test]
fn model_filter_reads_small_json_even_when_the_glb_bin_exceeds_the_rigging_limit() {
    use std::io::Write;

    let fixture = DirectoryFixture::new();
    let bin_size = 256 * 1024 * 1024 + 4u32;
    let mut json = serde_json::to_vec(&serde_json::json!({
        "asset":{"version":"2.0"},"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],
        "accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[0,0,0]}],
        "bufferViews":[{"buffer":0,"byteLength":36}], "buffers":[{"byteLength":bin_size}],
        "nodes":[{"mesh":0}], "scenes":[{"nodes":[0]}], "scene":0
    })).unwrap();
    json.resize(json.len().next_multiple_of(4), b' ');
    let total_size = 28 + json.len() as u32 + bin_size;
    let mut file = std::fs::File::create(fixture.root.join("large.glb")).unwrap();
    file.write_all(b"glTF").unwrap();
    file.write_all(&2u32.to_le_bytes()).unwrap();
    file.write_all(&total_size.to_le_bytes()).unwrap();
    file.write_all(&(json.len() as u32).to_le_bytes()).unwrap();
    file.write_all(b"JSON").unwrap();
    file.write_all(&json).unwrap();
    file.write_all(&bin_size.to_le_bytes()).unwrap();
    file.write_all(b"BIN\0").unwrap();
    // 由文件系统扩展 BIN，夹具和目录读取都无需在内存中分配整份二进制。
    file.set_len(u64::from(total_size)).unwrap();
    drop(file);
    let snapshot = scan_directory(fixture.root.clone(), String::new(), DirectoryQuery {
        context: 1, scope: FolderScope::Assets, asset_root: None,
        filters: &DirectoryFilters { kind: AssetTypeFilter::Model, ..Default::default() }, reveal_file: None,
    }, &DirectoryWorker::default()).unwrap();
    assert_eq!(listed_paths(&snapshot), ["large.glb"]);
}

#[test]
fn type_filters_keep_registered_asset_paths_and_source_clip_indices() {
    use serde_json::json;
    use AssetTypeFilter::*;

    let fixture = DirectoryFixture::new();
    for folder in ["content/library/roles/rig", "content/library/roles/empty/deeper", "content/library/.trash", "assets", "target"] {
        std::fs::create_dir_all(fixture.root.join(folder)).unwrap();
    }
    let asset_root = fixture.root.join("content/library");
    let mesh = json!({"asset":{"version":"2.0"},"meshes":[{"primitives":[{}]}]});
    let skeleton = json!({"asset":{"version":"2.0"},"meshes":[{"primitives":[{}]}],"nodes":[{}],"skins":[{"joints":[0]}]});
    write_glb(&asset_root.join("roles/rig/character.glb"), skeleton.clone());
    write_glb(&asset_root.join(".trash/deleted.glb"), skeleton.clone());
    write_glb(&fixture.root.join("target/private.glb"), skeleton);
    write_glb(&asset_root.join("prop.glb"), mesh);
    write_glb(&asset_root.join("motion.GLB"), json!({"asset":{"version":"2.0"},
        "animations":[{"name":"Walk"},{"name":"Walk"},{},{"name":"Run"}]}));
    for file in ["content/library/picture.PNG", "content/library/sound.WAV", "content/library/clip.AVI", "assets/readme.txt", "README.md"] {
        fixture.add_file(file);
    }

    let project_worker = DirectoryWorker::default();
    let project = |kind| scan_directory(fixture.root.clone(), String::new(), DirectoryQuery {
        context: 1, scope: FolderScope::ProjectFiles, asset_root: Some(&asset_root),
        filters: &DirectoryFilters { kind, ..Default::default() }, reveal_file: None,
    }, &project_worker).unwrap();
    assert_eq!(listed_paths(&project(Asset)), ["content"], "登记根不是硬编码的 assets");
    assert_eq!(listed_paths(&project(Skeleton)), ["content"], "构建和回收目录不参与后代匹配");
    assert_eq!(listed_paths(&project(All)), ["assets", "content", "README.md"]);

    let assets_worker = DirectoryWorker::default();
    let assets = |folder: &str, kind, filename: &str| scan_directory(asset_root.clone(), folder.to_owned(), DirectoryQuery {
        context: 1, scope: FolderScope::Assets, asset_root: None,
        filters: &DirectoryFilters { kind, filename: filename.to_owned() }, reveal_file: None,
    }, &assets_worker).unwrap();
    assert_eq!(listed_paths(&assets("", Skeleton, "")), ["roles"]);
    assert_eq!(listed_paths(&assets("roles", Skeleton, "")), ["roles/rig"]);
    assert_eq!(listed_paths(&assets("roles/rig", Skeleton, "")), ["roles/rig/character.glb"]);
    assert_eq!(listed_paths(&assets("roles", All, "")), ["roles/empty", "roles/rig"]);
    assert_eq!(listed_paths(&assets("", Model, "")), ["roles", "prop.glb"]);
    assert_eq!(listed_paths(&assets("", Image, "")), ["picture.PNG"]);
    assert_eq!(listed_paths(&assets("", Audio, "")), ["sound.WAV"]);
    assert_eq!(listed_paths(&assets("", Video, "")), ["clip.AVI"]);
    let motion = assets("", Animation, "MoTiOn");
    assert_eq!(listed_paths(&motion), ["motion.GLB"]);
    let clips = &motion.animations["motion.GLB"];
    assert_eq!(clips.iter().map(|clip| (clip.index, clip.label.as_str())).collect::<Vec<_>>(),
        [(0, "#1 · Walk"), (1, "#2 · Walk"), (2, "#3"), (3, "Run")]);
    let same_file = assets("", Animation, "");
    assert!(std::sync::Arc::ptr_eq(clips, &same_file.animations["motion.GLB"]), "未改动 GLB 重用一次 JSON 解析结果");
}

#[cfg(windows)]
#[test]
fn native_file_events_refresh_added_replaced_and_deleted_glb_matches() {
    use serde_json::json;
    use std::time::{Duration, Instant};

    let fixture = DirectoryFixture::new();
    let worker = DirectoryWorker::default();
    let filters = DirectoryFilters { kind: AssetTypeFilter::Animation, ..Default::default() };
    let scan = || scan_directory(fixture.root.clone(), String::new(), DirectoryQuery {
        context: 2, scope: FolderScope::Assets, asset_root: None, filters: &filters, reveal_file: None,
    }, &worker).unwrap();
    let initial = scan();
    assert!(initial.entries.is_empty());
    assert!(worker.is_watching());
    let wait_for = |ready: &dyn Fn(&DirectorySnapshot) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if worker.take_changed() {
                let snapshot = scan();
                if ready(&snapshot) { break; }
            }
            assert!(Instant::now() < deadline, "原生目录事件未刷新 GLB 快照");
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    std::fs::create_dir(fixture.root.join("nested")).unwrap();
    let file = fixture.root.join("nested/motion.glb");
    write_glb(&file, json!({"asset":{"version":"2.0"}, "animations":[{}]}));
    wait_for(&|snapshot| listed_paths(snapshot) == ["nested"]);
    write_glb(&file, json!({"asset":{"version":"2.0"}, "meshes":[{}]}));
    wait_for(&|snapshot| snapshot.entries.is_empty());
    write_glb(&file, json!({"asset":{"version":"2.0"}, "animations":[{},{}]}));
    wait_for(&|snapshot| listed_paths(snapshot) == ["nested"]);
    std::fs::remove_file(file).unwrap();
    wait_for(&|snapshot| snapshot.entries.is_empty());
}

#[test]
fn complete_directory_scan_and_reveal_have_no_former_4096_entry_cutoff() {
    let fixture = DirectoryFixture::new(); // 规模测试不使用任何真实资产文件。
    for index in 0..4_097 {
        fixture.add_file(&format!("item-{index:04}.glb")); // 构造刚好越过旧硬门的合法文件集合。
    }
    std::fs::create_dir(fixture.root.join("models")).unwrap(); // 目录应始终排序到全部文件前面。
    std::fs::create_dir(fixture.root.join(".trash")).unwrap(); // 回收区仍必须隐藏，不因去掉上限而暴露。
    fixture.add_file("texture.PNG"); // 类型识别继续接受大小写扩展名。
    let completed = scan_asset_request(
        fixture.root.clone(), // 生产扫描只接收一份明确资产根。
        AssetScanRequest {
            revision: 31,                                  // 完成值必须原样保留票据。
            folder: String::new(), // 扫描当前一级根目录，不递归 models 或 .trash。
            reveal_file: Some("item-4096.glb".to_owned()), // 目标位于旧容量门之后。
            ..Default::default()
        },
        &DirectoryWorker::default(),
    );
    assert_eq!(completed.request.revision, 31); // 后台没有重新猜测票据。
    let contents = completed.result.expect("合法目录不应被旧 4096 门截断"); // 失败必须让回归明确失败。
    let entries = contents.snapshot.entries;
    assert_eq!(entries.len(), 4_099); // 4097 模型 + models 目录 + 纹理，隐藏 .trash。
    assert_eq!(entries[0].asset_path, "models"); // 目录优先规则保持原行为。
    assert!(entries[0].is_directory); // 目录不会错误选作场景预览。
    let target = contents.reveal_index.expect("后台应准确定位末段目标"); // 页码来自后台排序之后的绝对索引。
    assert_eq!(target, 4_097); // 前面恰有一个目录与 4096 个模型。
    assert_eq!(entries[target].asset_path, "item-4096.glb"); // 第 4097 个文件仍可定位。
    assert_eq!(entries.last().unwrap().kind, SceneAssetKind::Texture); // 类型和名称排序继续正确。
}

#[test]
fn scan_rejects_parent_escape_and_reports_missing_folders() {
    let fixture = DirectoryFixture::new(); // 根目录拥有独立可验证边界。
    let escaped = scan_asset_folder(fixture.root.clone(), "..".to_owned()); // 防御绕过 UI 词法入口的内部请求。
    assert!(escaped.unwrap_err().contains("越过assets根")); // 禁止扫描系统临时根内容。
    let missing = scan_asset_folder(fixture.root.clone(), "missing".to_owned()); // 普通缺失目录与非法路径错误不同。
    assert!(missing.unwrap_err().contains("无法打开资产目录")); // 返回具体目录 IO 原因而非伪成功空表。
}
