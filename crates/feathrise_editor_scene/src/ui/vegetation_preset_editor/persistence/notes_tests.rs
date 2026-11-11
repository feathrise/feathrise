//! 备注从旧来源初始化并经过完整目录原子事务往返的隔离回归。

use std::path::PathBuf; // 临时资产根必须以拥有路径传入生产事务。
use std::sync::atomic::{AtomicU64, Ordering}; // 同一测试进程中的目录名称不能碰撞。

use super::*; // 直接验证生产迁移函数、目录修改与原子写入事务。

static NEXT_TEST_ROOT: AtomicU64 = AtomicU64::new(0); // 仅为本进程测试隔离生成后缀。

/// 测试结束时只删除自己在系统临时目录下创建的精确子目录。
struct TestAssetRoot(PathBuf); // RAII避免把测试JSON留在用户工作区或真实assets中。

impl TestAssetRoot {
    // 临时根的创建与写入都保持明确路径边界。
    fn new() -> Self {
        // 每个测试获得一个独立assets等价根。
        let suffix = NEXT_TEST_ROOT.fetch_add(1, Ordering::Relaxed); // 顺序只要求本进程唯一。
        let root = std::env::temp_dir().join(format!(
            // 目标始终是系统临时目录的直接子项。
            "bevy-learn5-vegetation-notes-{}-{suffix}", // 可审计前缀便于异常时辨认。
            std::process::id(),                         // 跨并行测试进程隔离。
        )); // 完成精确临时路径。
        std::fs::create_dir(&root).expect("应能创建隔离备注事务目录"); // 已存在即失败，绝不覆盖未知内容。
        Self(root) // Drop只负责这一个确定目录。
    } // 临时根创建完成。

    fn path(&self) -> &std::path::Path {
        // 生产事务只需要借用根路径。
        &self.0 // 不暴露删除或重命名操作。
    } // 返回精确测试根。
} // 临时根辅助结束。

impl Drop for TestAssetRoot {
    // 测试成功、失败展开或panic都尽力清理。
    fn drop(&mut self) {
        // 只删除new创建的精确直接子目录。
        let _ = std::fs::remove_dir_all(&self.0); // 清理失败不覆盖原测试断言结果。
    } // 本次测试资源释放完成。
} // RAII清理实现完成。

/// 从随包目录读取一份真实完整Mesh预设，避免在测试中伪造运行时结构。
fn legacy_preset() -> VegetationPresetDescriptor {
    // 随包旧数据当前没有notes字段。
    let catalog: VegetationPresetCatalogAsset = serde_json::from_str(include_str!(concat!(
        // 编译期嵌入正式UTF-8目录。
        env!("CARGO_MANIFEST_DIR"), // 从场景编辑器crate定位工作区。
        "/tests/fixtures/vegetation_presets.vegetation_presets.json", // 使用真实数据协议。
    ))) // JSON输入组装完成。
    .expect("随包植被预设目录必须可解析"); // 格式损坏直接阻断备注迁移测试。
    catalog // 从完整目录中选择固定稳定身份。
        .presets // 取得拥有型预设，后续可自由修改备注。
        .into_iter() // 不克隆无关目录项。
        .find(|preset| preset.id == "hn01_tree") // Mesh四级结构已经由公共校验覆盖。
        .expect("随包树木预设必须存在") // 测试夹具缺失时给出明确原因。
} // 返回缺省备注的真实预设。

/// 把基线写成生产事务期望的资产目录位置。
fn write_baseline(root: &std::path::Path, catalog: &VegetationPresetCatalogAsset) {
    // 只写本测试临时根。
    let path = safe_asset_path(root, VEGETATION_PRESET_CATALOG_ASSET_PATH) // 复用生产安全路径约束。
        .expect("正式植被目录路径必须安全"); // 常量非法应立即失败。
    let parent = path.parent().expect("植被目录必须拥有config父目录"); // 生产常量保证该结构。
    std::fs::create_dir_all(parent).expect("应能创建临时config目录"); // 不触碰真实assets。
    let mut bytes = serde_json::to_vec_pretty(catalog).expect("基线目录必须可序列化"); // 与生产漂亮格式一致。
    bytes.push(b'\n'); // 作者JSON保留终止换行。
    std::fs::write(path, bytes).expect("应能写入隔离基线目录"); // 后续生产事务将原子替换此文件。
} // 基线准备完成。

#[test] // 旧来源仅初始化一次，显式清空后永远不能自动复活。
fn legacy_source_initialization_respects_an_explicit_empty_note() {
    // 迁移边界独立于UI焦点。
    let mut preset = legacy_preset(); // 当前随包JSON代表没有notes字段的旧数据。
    assert!(preset.notes.is_none()); // 缺省值由serde兼容读取为None。
    let source = preset.author_asset_path.clone(); // 冻结预期来源文字。
    initialize_notes(&mut preset); // 第一次明确保存采用旧来源作为备注初值。
    assert_eq!(preset.notes.as_deref(), Some(source.as_str())); // 来源完整保留中文与正斜杠。
    preset.notes = Some(String::new()); // 模拟作者明确清空备注并提交。
    initialize_notes(&mut preset); // 后续保存仍会经过同一入口。
    assert_eq!(preset.notes.as_deref(), Some("")); // Some("")必须保持，不再回填来源。
} // 一次性兼容迁移回归完成。

#[test] // 中文多行与空串必须经过真实磁盘基线核对和原子替换完整往返。
fn notes_round_trip_through_the_catalog_transaction() {
    // 不写用户真实目录。
    let root = TestAssetRoot::new(); // 创建精确隔离的临时资产根。
    let original = legacy_preset(); // 使用可通过公共校验的完整运行时定义。
    let baseline = VegetationPresetCatalogAsset {
        // 单项目录让结果身份明确。
        schema_version: VEGETATION_PRESET_SCHEMA_VERSION, // 使用当前协议且不因可选备注升级。
        presets: vec![original.clone()],                  // 保留全部LOD、材质、碰撞与作者来源。
    }; // 初始目录完成。
    write_baseline(root.path(), &baseline); // 生产事务必须先看到完全相同的磁盘基线。
    let mut edited = original.clone(); // 只修改作者备注。
    edited.notes = Some("来源：自制白桦\n用途：远景林带".to_owned()); // 中文、多行与冒号都必须原样保留。
    let first = write_catalog_transaction(
        // 运行完整读取、公共校验、漂亮序列化与原子替换。
        root.path().to_path_buf(), // 事务拥有隔离根路径。
        baseline,                  // 乐观并发基线与磁盘一致。
        CatalogMutation::Upsert {
            // 稳定身份原位编辑。
            original_id: Some(original.id.clone()), // 不更改技术ID。
            descriptor: edited.clone(),             // 完整强类型新值。
        }, // 修改请求完成。
        17,                        // 任意非零注册表修订用于结果身份。
        Some(3),                   // 模拟来源草稿标签。
    ) // 事务参数完成。
    .expect("中文多行备注应通过完整目录事务"); // 任一IO或校验失败都给出生产诊断。
    assert_eq!(first.preset_id, edited.id); // 采用身份仍是原稳定预设。
    let path = safe_asset_path(root.path(), VEGETATION_PRESET_CATALOG_ASSET_PATH) // 定位原子替换结果。
        .expect("正式目录路径必须安全"); // 常量路径仍需复验。
    let bytes = std::fs::read(&path).expect("应能读取第一轮事务结果"); // 文件句柄立即释放。
    let saved: VegetationPresetCatalogAsset = serde_json::from_slice(&bytes) // 按生产数据类型重新解析。
        .expect("第一轮目录结果必须是有效UTF-8 JSON"); // 不接受只在内存成功。
    assert_eq!(saved.presets[0].notes, edited.notes); // 中文和换行完整往返。
    let mut cleared = saved.presets[0].clone(); // 第二次只清空备注。
    cleared.notes = Some(String::new()); // 显式空串必须进入磁盘，不能按None省略。
    write_catalog_transaction(
        // 再次经过真实磁盘基线核对。
        root.path().to_path_buf(), // 使用同一隔离目录。
        saved.clone(),             // 第一轮结果成为精确新基线。
        CatalogMutation::Upsert {
            // 原位保存清空结果。
            original_id: Some(cleared.id.clone()), // 身份保持不变。
            descriptor: cleared.clone(),           // 完整描述只变notes。
        }, // 第二轮修改请求完成。
        18,                        // 模拟热采用后的下一修订。
        Some(3),                   // 仍属于同一草稿标签。
    ) // 第二轮事务参数完成。
    .expect("显式清空备注应通过完整目录事务"); // 空串不是校验错误。
    let final_bytes = std::fs::read(&path).expect("应能读取清空后的事务结果"); // 读取最终作者JSON。
    let final_catalog: VegetationPresetCatalogAsset = serde_json::from_slice(&final_bytes) // 再走serde兼容层。
        .expect("清空后的目录仍必须可解析"); // 空串不能破坏协议。
    assert_eq!(final_catalog.presets[0].notes.as_deref(), Some("")); // 反序列化不得把空串变回None。
    assert!(
        String::from_utf8(final_bytes)
            .expect("目录必须是UTF-8")
            .contains("\"notes\": \"\"")
    ); // 序列化结果必须明确记录作者清空。
} // 完整原子往返回归完成，临时根由Drop清理。
