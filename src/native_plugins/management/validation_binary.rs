//! 管理器升级验收的真实双版本C ABI协议探针；不冒充装备业务，也不改正式插件版本。
use feathrise_plugin_api::{
    PluginMetadata, Snapshot, encode_plugin_metadata, read_plugin_metadata,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// 只在调用方已创建并确认归属的绝对夹具目录内生成源码、日志和两个独立动态库。
pub(crate) fn build_versions(output: &Path) -> Result<Vec<PathBuf>, String> {
    if !output.is_absolute() || !output.is_dir() {
        return Err("协议探针输出必须是已存在的owner夹具绝对目录".into());
    }
    let info = fs::symlink_metadata(output).map_err(|error| error.to_string())?;
    if info.file_type().is_symlink() {
        return Err("协议探针目录不能是符号链接".into());
    }
    let suffix = match std::env::consts::OS {
        "windows" => "dll",
        "linux" => "so",
        "macos" => "dylib",
        _ => return Err("当前平台不支持协议探针".into()),
    };
    let mut binaries = Vec::new();
    for version in ["1.0.0", "1.1.0"] {
        let directory = output.join(format!("protocol-probe-{version}"));
        // 新目录避免把上轮库或报告当成本轮真正编译出的版本。
        fs::create_dir(&directory).map_err(|error| format!("协议探针目录必须全新：{error}"))?;
        let metadata = PluginMetadata {
            format: 1,
            id: "example.protocol_probe".into(),
            tool: "协议探针".into(),
            version: version.into(),
            target_os: std::env::consts::OS.into(),
            target_arch: std::env::consts::ARCH.into(),
            abi_major: feathrise_plugin_api::ABI_MAJOR,
            runtime_dependencies: vec!["example_runtime".into()],
            runtime: None,
        };
        let snapshot = Snapshot {
            plugin_id: metadata.id.clone(),
            tool: metadata.tool.clone(),
            status: format!("管理器C ABI协议探针 {version}；不提供装备业务"),
            pages: 1,
            ..Default::default()
        };
        snapshot.validate(&metadata.id)?;
        let metadata_bytes = encode_plugin_metadata(&metadata)?;
        let snapshot_bytes = serde_json::to_vec(&snapshot).map_err(|error| error.to_string())?;
        let list = |bytes: &[u8]| {
            bytes
                .iter()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        let source = SOURCE
            .replace("@METADATA_LENGTH@", &metadata_bytes.len().to_string())
            .replace("@METADATA@", &list(&metadata_bytes))
            .replace("@SNAPSHOT@", &list(&snapshot_bytes))
            .replace("@ABI_MAJOR@", &feathrise_plugin_api::ABI_MAJOR.to_string())
            .replace("@ABI_MINOR@", &feathrise_plugin_api::ABI_MINOR.to_string());
        let source_path = directory.join("protocol_probe.rs");
        fs::write(&source_path, source).map_err(|error| error.to_string())?;
        let binary = directory.join(format!("example.protocol_probe.{suffix}"));
        let mut command = Command::new("rustc");
        command
            .current_dir(&directory)
            .args([
                "--edition=2024",
                "--crate-name",
                "feathrise_management_protocol_probe",
                "--crate-type",
                "cdylib",
            ])
            .arg(&source_path)
            .arg("-o")
            .arg(&binary);
        if cfg!(target_os = "windows") {
            command.args(["-C", "linker=rust-lld"]);
        }
        let result = command
            .output()
            .map_err(|error| format!("协议探针rustc启动失败：{error}"))?;
        fs::write(directory.join("rustc.stdout.txt"), &result.stdout)
            .map_err(|error| error.to_string())?;
        fs::write(directory.join("rustc.stderr.txt"), &result.stderr)
            .map_err(|error| error.to_string())?;
        if !result.status.success() {
            return Err(format!(
                "协议探针{version}真实编译失败：{}",
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        if read_plugin_metadata(&binary)? != metadata {
            return Err(format!("协议探针{version}最终产物静态元数据不符"));
        }
        binaries.push(binary);
    }
    Ok(binaries)
}

// 直接rustc构建的自包含C ABI探针；宿主先用正式SDK序列化并验证Snapshot，避免手写JSON漂移。
const SOURCE: &str = r#"//! 管理器验收协议探针，不含装备业务或工程资产写入。
use std::ffi::c_void;
type Writer = unsafe extern "C" fn(*mut c_void, *const u8, usize) -> i32;
#[repr(C)]
struct Api {
    abi_major: u32, abi_minor: u32, struct_size: u32, reserved: u32,
    create: unsafe extern "C" fn(*const u8, usize, *mut *mut c_void, *mut c_void, Writer) -> i32,
    invoke: unsafe extern "C" fn(*mut c_void, *const u8, usize, *mut c_void, Writer) -> i32,
    destroy: unsafe extern "C" fn(*mut c_void),
}
#[used]
#[cfg_attr(target_os="macos", unsafe(link_section="__DATA,__frmeta"))]
#[cfg_attr(not(target_os="macos"), unsafe(link_section=".frmeta"))]
static METADATA: [u8; @METADATA_LENGTH@] = [@METADATA@];
const SNAPSHOT: &[u8] = &[@SNAPSHOT@];
unsafe extern "C" fn create(ptr: *const u8, len: usize, out: *mut *mut c_void, context: *mut c_void, writer: Writer) -> i32 {
    if out.is_null() { return -1; }
    unsafe { *out = std::ptr::null_mut(); }
    if ptr.is_null() || len == 0 || len > 524288 { return -1; }
    let instance = Box::into_raw(Box::new(1_u8));
    unsafe { *out = instance.cast(); }
    let result = unsafe { writer(context, SNAPSHOT.as_ptr(), SNAPSHOT.len()) };
    if result != 0 { unsafe { drop(Box::from_raw(instance)); *out = std::ptr::null_mut(); } }
    result
}
unsafe extern "C" fn invoke(instance: *mut c_void, ptr: *const u8, len: usize, context: *mut c_void, writer: Writer) -> i32 {
    if instance.is_null() || ptr.is_null() || len == 0 || len > 524288 { return -1; }
    let request = unsafe { std::slice::from_raw_parts(ptr, len) };
    if request != b"{\"kind\":\"open\"}" { return -1; }
    unsafe { writer(context, SNAPSHOT.as_ptr(), SNAPSHOT.len()) }
}
unsafe extern "C" fn destroy(instance: *mut c_void) {
    if !instance.is_null() { unsafe { drop(Box::from_raw(instance.cast::<u8>())); } }
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn feathrise_plugin_metadata_v1(context: *mut c_void, writer: Writer) -> i32 {
    unsafe { writer(context, METADATA.as_ptr(), METADATA.len()) }
}
#[unsafe(no_mangle)]
pub extern "C" fn feathrise_plugin_api_v1() -> *const Api {
    static API: Api = Api { abi_major: @ABI_MAJOR@, abi_minor: @ABI_MINOR@,
        struct_size: std::mem::size_of::<Api>() as u32, reserved: 0, create, invoke, destroy };
    &API
}
"#;
