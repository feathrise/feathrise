//! 目录扫描真实文件系统回归；仅在独立临时目录创建夹具，不读取或修改作者 assets。

use std::path::PathBuf; // 临时目录的唯一明确所有权边界。
use std::sync::atomic::{AtomicU64, Ordering}; // 并行测试也不能复用相同夹具目录。
use std::time::{SystemTime, UNIX_EPOCH}; // 临时名称包含进程和时间，避免碰触现有文件。

use super::{AssetScanRequest, scan_asset_folder, scan_asset_request}; // 直接测试后台正式扫描与完整路径搜索。
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
        },
    );
    assert_eq!(completed.request.revision, 31); // 后台没有重新猜测票据。
    let contents = completed.result.expect("合法目录不应被旧 4096 门截断"); // 失败必须让回归明确失败。
    assert_eq!(contents.entries.len(), 4_099); // 4097 模型 + models 目录 + 纹理，隐藏 .trash。
    assert_eq!(contents.entries[0].asset_path, "models"); // 目录优先规则保持原行为。
    assert!(contents.entries[0].is_directory); // 目录不会错误选作场景预览。
    let target = contents.reveal_index.expect("后台应准确定位末段目标"); // 页码来自后台排序之后的绝对索引。
    assert_eq!(target, 4_097); // 前面恰有一个目录与 4096 个模型。
    assert_eq!(contents.entries[target].asset_path, "item-4096.glb"); // 第 4097 个文件仍可定位。
    assert_eq!(contents.entries.last().unwrap().kind, SceneAssetKind::Texture); // 类型和名称排序继续正确。
}

#[test]
fn scan_rejects_parent_escape_and_reports_missing_folders() {
    let fixture = DirectoryFixture::new(); // 根目录拥有独立可验证边界。
    let escaped = scan_asset_folder(fixture.root.clone(), "..".to_owned()); // 防御绕过 UI 词法入口的内部请求。
    assert!(escaped.unwrap_err().contains("越过assets根")); // 禁止扫描系统临时根内容。
    let missing = scan_asset_folder(fixture.root.clone(), "missing".to_owned()); // 普通缺失目录与非法路径错误不同。
    assert!(missing.unwrap_err().contains("无法打开资产目录")); // 返回具体目录 IO 原因而非伪成功空表。
}
