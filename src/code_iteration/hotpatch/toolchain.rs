//! DX 完整构建、fat 基线与薄补丁的后台调用，冻结链接上下文并由宿主采用产物。

use super::compiler_output::{self, FatBuildOutput, HotpatchOutput, MAX_COMPILER_OUTPUT_BYTES};
use crate::{build_process::{self, BuildControl}, compiler_diagnostics::CompilerDiagnostic, project::{self, Project}};
use feathrise_editor_core::ProjectBuildProfile;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};

#[path = "captured_client.rs"]
mod captured_client;
#[path = "empty_archive.rs"]
mod empty_archive;
#[path = "toolchain_log.rs"]
mod output_log;
#[path = "tool_cache.rs"]
mod tool_cache;
#[path = "bundled_dx.rs"]
mod bundled_dx;
pub(crate) use output_log::OutputLog as DxOutputLog;
#[cfg(test)]
use output_log::{cargo_verbose_process_line, compiler_diagnostic};

type Diagnostics = Option<Arc<dyn Fn(CompilerDiagnostic) + Send + Sync>>;
const CONTEXT_ENV: &str = "FEATHRISE_FUNCTION_HOTPATCH_CONTEXT";
const CONTEXT_HASH_ENV: &str = "FEATHRISE_FUNCTION_HOTPATCH_CONTEXT_SHA256";
const CONTEXT_LIMIT: usize = 256 * 1024;
const LINK_CONTEXT_ENV: &str = "FEATHRISE_HOTPATCH_LINK_CONTEXT";
const MSVC_TARGET: &str = "x86_64-pc-windows-msvc";
const ENV_INPUTS: &[&str] = &["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "RUSTC", "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_TARGET", "CARGO_BUILD_RUSTFLAGS", "RUSTUP_TOOLCHAIN",
    "RUSTUP_HOME", "CARGO_HOME", "CARGO_INCREMENTAL"];

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    format: u32,
    project: ProjectIdentity,
    target: PathBuf,
    features: String,
    bin: String,
    dx: PathBuf,
    dx_sha256: String,
    rustc: PathBuf,
    rustc_sha256: String,
    rustc_version: String,
    cargo: PathBuf,
    cargo_sha256: String,
    proxy: PathBuf,
    proxy_sha256: String,
    raw_linker: PathBuf,
    raw_sha256: String,
    sysroot_lib: PathBuf,
    session_cache: PathBuf,
    link_manifest: PathBuf,
    link_manifest_sha256: String,
    imports_file: PathBuf,
    imports: Vec<(PathBuf, String)>,
    environment: BTreeMap<String, Option<String>>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProjectIdentity { id: String, root: PathBuf, manifest: PathBuf, package: String }

impl From<&Project> for ProjectIdentity {
    fn from(project: &Project) -> Self {
        Self { id: project.id.clone(), root: project.root.clone(), manifest: project.manifest.clone(), package: project.package.clone() }
    }
}

/// 两条 DX 路径共用同一次入口和工具确认，避免旧的外部路径探测跳过内嵌工具。
pub(crate) fn project_tool(project: &Project, target: &Path, control: &BuildControl) -> Result<Option<(PathBuf, String)>, String> {
    let bin = match select_bin(project) {
        Ok(bin) => bin,
        Err(reason) => {
            if let Some(log) = control.log_sink() { log.task_message(&format!("已选择 DX，当前工程入口使用原生 Cargo：{reason}")); }
            return Ok(None);
        }
    };
    let resources = crate::project_template_root();
    let repository = resources.parent().ok_or("编辑器资源目录没有工具仓库父目录")?;
    let Some(dx) = bundled_dx::resolve(target, &repository.join(".local/hotpatch/tools/dx-0.7.9/dx.exe"), control)? else {
        if let Some(log) = control.log_sink() { log.task_message("已选择 DX，但未找到 DX 0.7.9，回退到原生 Cargo"); }
        return Ok(None);
    };
    let mut version = Command::new(&dx);
    version.arg("--version").current_dir(&project.root);
    if !small_output(&version, control)?.split_whitespace().any(|part| part == "0.7.9") {
        return Err("工程构建仅支持已验证的 DX 0.7.9".into());
    }
    Ok(Some((dx, bin)))
}

/// 普通工程也从 DX 构建，返回同次 Cargo 产物回执供宿主发布运行副本。
pub(crate) fn full_build_command(project: &Project, target: &Path, profile: ProjectBuildProfile,
    dx: &Path, bin: &str, control: &BuildControl) -> Result<Command, String> {
    let directory = project.manifest.parent().ok_or("工程 Cargo 入口没有父目录")?;
    let cargo = rustup_which("cargo", project, control)?;
    let tools = target.join("hotpatch-tools");
    tool_cache::create_directory(&tools)?;
    let dx_home = tools.join("dx-build-home");
    tool_cache::create_directory(&dx_home)?;
    let mut command = Command::new(dx);
    command.current_dir(directory)
        .args(["build", "--windows", "--windows-subsystem", "CONSOLE", "--skip-assets", "--locked",
            "--package", &project.package, "--bin", bin, "--profile", profile.cargo_profile(),
            "--raw-json-diagnostics", "--json-output"])
        .env("CARGO", &cargo).env("PATH", toolchain_path(&cargo)?)
        .env("CARGO_TARGET_DIR", target).env("DX_HOME", dx_home);
    let config = cargo_config2::Config::load_with_cwd(directory).map_err(|error| error.to_string())?;
    let linker = config.linker(MSVC_TARGET).map_err(|error| error.to_string())?;
    if linker.as_deref().is_some_and(|path| path == Path::new("rust-lld") || path == Path::new("rust-lld.exe")) {
        // DX 直接调用配置的 linker，而 Cargo 的 build.rs 仍可能传入 -flavor link。
        // 复用原始 LLD 的 lld-link 缓存别名，让驱动同时接受两种调用，并提供 MSVC 库路径。
        let rustc = rustup_which("rustc", project, control)?;
        let sysroot = rustc_sysroot(&rustc, project, control)?;
        let raw = sysroot.join("lib/rustlib").join(MSVC_TARGET).join("bin/rust-lld.exe");
        let linker = tool_cache::raw(&tools, &raw, &hash_file(&raw, control)?, control)?;
        command.env("CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER", linker)
            .env("LIB", discover_lib(project, control)?);
    }
    Ok(command)
}

/// 返回的client包含本次实际完整链接记录及只属于该基线的上下文；EXE/PDB发布由原运行版本层负责。
pub(crate) fn fat(project: &Project, target: &Path, dx: &Path, control: &BuildControl, features: &str, diagnostics: Diagnostics) -> Result<FatBuildOutput, String> {
    validate_project(project, control)?;
    let target = existing_target(target)?;
    let mut context = prepare_context(project, &target, dx, features, control)?;
    let path = context.link_manifest.parent().unwrap().join("context.json");
    let mut command = context.command(project)?;
    command.args(["build", "--fat-binary"]);
    context.arguments(&mut command);
    empty_archive::prepare(&context, project, control)?;
    let output = execute(&command, None, project, control, diagnostics)?;
    let mut result = compiler_output::parse_fat_output(&output)?;
    validate_artifacts(&result.artifacts, project, &target)?;
    let mut client: Value = serde_json::from_slice(&result.client_json).map_err(|error| error.to_string())?;
    captured_client::normalize(&mut client, &target, &context.bin, &project.manifest, &project.package, &context.link_manifest)?;
    empty_archive::verify(&context, &client, control)?;
    control.check()?;
    context.imports = collect_imports(&client, &target, &context.sysroot_lib, control)?;
    write_imports(&context.imports_file, &context.imports)?;
    let bytes = serde_json::to_vec(&context).map_err(|error| error.to_string())?;
    if bytes.len() > CONTEXT_LIMIT { return Err("热更新工具上下文超过256KiB".into()); }
    write_new(&path, &bytes)?;
    let mut client = client;
    bind_client_environment(&mut client, CONTEXT_ENV, path_text(&path)?)?;
    bind_client_environment(&mut client, CONTEXT_HASH_ENV, sha(&bytes))?;
    let bytes = serde_json::to_vec(&client).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_COMPILER_OUTPUT_BYTES { return Err("热更新基线client超过8MiB".into()); }
    result.client_json = Arc::new(bytes);
    validate_project(project, control)?;
    Ok(result)
}

/// 原运行依赖发布器使用同一次dx完整链接回执，不以Cargo无hash副本或目录最新时间替代。
pub(crate) fn runtime_dlls(fat: &FatBuildOutput, target: &Path, control: &BuildControl) -> Result<Vec<crate::build_artifacts::CargoDll>, String> {
    let target = existing_target(target)?;
    if fat.client_json.len() > MAX_COMPILER_OUTPUT_BYTES { return Err("fat动态导入回执超过8MiB".into()); }
    let client: Value = serde_json::from_slice(&fat.client_json).map_err(|error| error.to_string())?;
    let arguments = client["link_args"].as_array().ok_or("fat client缺少link_args")?;
    let receipt = format!("dx-fat-linker-receipt:{}", sha(&fat.client_json));
    let mut dlls = BTreeMap::<String, crate::build_artifacts::CargoDll>::new();
    for argument in arguments {
        control.check()?;
        let argument = argument.as_str().ok_or("fat link_args不是字符串")?.trim_matches('"');
        if !argument.to_ascii_lowercase().ends_with(".dll.lib") { continue; }
        let import = Path::new(argument);
        if !import.is_absolute() { return Err("fat动态导入库必须是实际绝对路径".into()); }
        let import = import.canonicalize().map_err(|error| error.to_string())?;
        if !import.starts_with(&target) { continue; } // sysroot的std DLL继续走原受检搜索路径。
        if read_bounded_prefix(&import, 8)? != b"!<arch>\n" { return Err("fat动态导入库不是COFF归档".into()); }
        let dll = import.with_extension("").canonicalize().map_err(|error| format!("fat导入库缺少配对DLL：{error}"))?;
        if !dll.starts_with(&target) || !dll.is_file() || dll.extension().is_none_or(|extension| !extension.eq_ignore_ascii_case("dll")) {
            return Err("fat导入库配对DLL逃出target或类型不匹配".into());
        }
        let name = dll.file_name().and_then(|value| value.to_str()).ok_or("fat DLL名称不是UTF-8")?.to_ascii_lowercase();
        let record = crate::build_artifacts::CargoDll { target_name: dll.file_stem().and_then(|value| value.to_str())
            .ok_or("fat DLL目标名称不是UTF-8")?.to_owned(), path: dll.clone(), package_id: receipt.clone(), dylib: true };
        if let Some(previous) = dlls.insert(name, record) && previous.path != dll {
            return Err("fat链接回执包含同名不同来源DLL".into());
        }
        if dlls.len() > 128 { return Err("fat链接回执的动态DLL超过128项".into()); }
    }
    Ok(dlls.into_values().collect())
}

/// 只根据原fat client与其冻结环境生成补丁；不连接devserver、不执行或替换运行实例。
pub(crate) fn patch(project: &Project, target: &Path, frozen_client_json: Arc<Vec<u8>>, aslr_ref: u64,
    control: &BuildControl, features: &str, diagnostics: Diagnostics) -> Result<HotpatchOutput, String> {
    validate_project(project, control)?;
    if aslr_ref == 0 || frozen_client_json.len() > MAX_COMPILER_OUTPUT_BYTES {
        return Err("薄补丁缺少有效运行ASLR或基线client超过8MiB".into());
    }
    let target = existing_target(target)?;
    let client: Value = serde_json::from_slice(&frozen_client_json).map_err(|error| error.to_string())?;
    let path = PathBuf::from(client_environment(&client, CONTEXT_ENV)?).canonicalize().map_err(|error| error.to_string())?;
    if !path.starts_with(target.join("hotpatch-tools/function-sessions")) || path.file_name().is_none_or(|name| name != "context.json") {
        return Err("薄补丁上下文不属于当前target的独占会话".into());
    }
    let bytes = read_bounded(&path, CONTEXT_LIMIT)?;
    if sha(&bytes) != client_environment(&client, CONTEXT_HASH_ENV)? { return Err("原fat工具上下文已改变".into()); }
    let context: Context = serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
    if context.format != 1 || context.project != ProjectIdentity::from(project) || context.target != target
        || context.features != normalized_features(features)? || context.bin != select_bin(project)? {
        return Err("薄补丁工程、入口、target或特性与原fat基线不同".into());
    }
    let mut locks = Vec::new();
    for (path, expected) in [(&context.dx, &context.dx_sha256), (&context.rustc, &context.rustc_sha256),
        (&context.cargo, &context.cargo_sha256),
        (&context.proxy, &context.proxy_sha256), (&context.raw_linker, &context.raw_sha256)] {
        let file = read_lock(path)?;
        if hash_file(path, control)? != *expected { return Err("原fat工具文件已改变，未重建基线不允许继续补丁".into()); }
        locks.push(file);
    }
    let mut rustc = context.command_for(&context.rustc, project)?;
    rustc.arg("-Vv");
    if small_output(&rustc, control)?.trim() != context.rustc_version { return Err("原fat Rust工具链版本已改变".into()); }
    for (path, expected) in &context.imports {
        locks.push(read_lock(path)?);
        if hash_file(path, control)? != *expected { return Err("原fat动态导入库已改变".into()); }
    }
    locks.push(read_lock(&context.imports_file)?);
    locks.push(read_lock(&context.link_manifest)?);
    if sha(&read_bounded(&context.link_manifest, 64 * 1024)?) != context.link_manifest_sha256 {
        return Err("原fat链接上下文已改变".into());
    }
    verify_imports_file(&context.imports_file, &context.imports)?;
    let mut command = context.command(project)?;
    command.args(["tools", "hotpatch", "--aslr-reference", &aslr_ref.to_string()]);
    context.arguments(&mut command);
    empty_archive::prepare(&context, project, control)?;
    let output = execute(&command, Some((*frozen_client_json).clone()), project, control, diagnostics)?;
    let mut result = compiler_output::parse_hotpatch_output(&output)?;
    // 冻结 client.exe 是初始跳表的输入；thin 回执中的 exe 则是本次 rustc 在 deps 内的临时目标。
    // 二者分别核验，薄产物不能被提升成新的完整基线。
    let executable = Path::new(client["exe"].as_str().ok_or("薄补丁 client 缺少冻结 EXE")?);
    validate_patch_artifact_paths(&result.artifacts, &target, &context.bin, &project.context.storage_key, executable)?;
    result.normalize_windows_sources(aslr_ref)?;
    let library = result.library.canonicalize().map_err(|error| error.to_string())?;
    let pdb = result.pdb.canonicalize().map_err(|error| error.to_string())?;
    let root = result.artifacts.root.canonicalize().map_err(|error| error.to_string())?;
    let prefix = format!("lib{}-patch-", context.bin);
    let patch_stamp = library.file_name().and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(prefix.as_str())).and_then(|name| name.strip_suffix(".dll"));
    if library.parent() != Some(root.as_path()) || pdb != library.with_extension("pdb") || result.aslr_reference != aslr_ref
        || !patch_stamp.is_some_and(|stamp| !stamp.is_empty() && stamp.len() <= 20 && stamp.bytes().all(|byte| byte.is_ascii_digit())) {
        return Err(format!("薄补丁DLL/PDB或运行引用身份不匹配：DLL={}；PDB={}；运行main={aslr_ref}", library.display(), pdb.display()));
    }
    validate_project(project, control)?;
    drop(locks);
    Ok(result)
}

impl Context {
    fn command(&self, project: &Project) -> Result<Command, String> { self.command_for(&self.dx, project) }
    fn command_for(&self, executable: &Path, project: &Project) -> Result<Command, String> {
        let mut command = Command::new(executable);
        command.current_dir(project.manifest.parent().ok_or("工程Cargo入口没有父目录")?);
        for (name, value) in &self.environment {
            if let Some(value) = value { command.env(name, value); } else { command.env_remove(name); }
        }
        Ok(command)
    }
    fn arguments(&self, command: &mut Command) {
        command.args(["--package", &self.project.package, "--bin", &self.bin,
            "--profile", "render-dev", "--no-default-features", "--offline", "--locked",
            "--raw-json-diagnostics", "--json-output", "--session-cache-dir"])
            .arg(&self.session_cache);
        for feature in self.features.split(',') { command.args(["--features", feature]); }
    }
}

fn prepare_context(project: &Project, target: &Path, dx: &Path, features: &str, control: &BuildControl) -> Result<Context, String> {
    let resources = crate::project_template_root();
    let rustc = rustup_which("rustc", project, control)?;
    let cargo = rustup_which("cargo", project, control)?;
    let mut command = Command::new(&rustc); command.arg("-Vv").current_dir(&project.root);
    let rustc_version = small_output(&command, control)?.trim().to_owned();
    let sysroot = rustc_sysroot(&rustc, project, control)?;
    let sysroot_lib = sysroot.join("lib/rustlib/x86_64-pc-windows-msvc/lib").canonicalize().map_err(|error| error.to_string())?;
    let raw = sysroot.join("lib/rustlib/x86_64-pc-windows-msvc/bin/rust-lld.exe");
    let raw_sha256 = hash_file(&raw, control)?;
    let tools = target.join("hotpatch-tools");
    fs::create_dir_all(&tools).map_err(|error| error.to_string())?;
    let raw_linker = tool_cache::raw(&tools, &raw, &raw_sha256, control)?;
    let lib = discover_lib(project, control)?;
    let source = resources.join("assets/hotpatch/原生热补丁链接代理.rs");
    let identity = sha(format!("{}|{}|{}", hash_file(&source, control)?, rustc_version, raw_sha256).as_bytes());
    let proxy = tool_cache::proxy(&tools, &identity, control, |proxy| {
        let mut command = Command::new(&rustc);
        command.args(["--edition=2024", "--crate-name", "feathrise_hotpatch_linker", "-C"])
            .arg(format!("linker={}", raw_linker.display())).arg(&source).arg("-o").arg(&proxy)
            .env("LIB", &lib).current_dir(&project.root);
        small_output(&command, control)?;
        Ok(())
    })?;
    let directory = tools.join("function-sessions").join(bevy::asset::uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let directory = directory.canonicalize().map_err(|error| error.to_string())?;
    let session_cache = directory.join("session-cache");
    let dx_home = directory.join("dx-home");
    fs::create_dir(&session_cache).map_err(|error| error.to_string())?;
    fs::create_dir(&dx_home).map_err(|error| error.to_string())?;
    let bin = select_bin(project)?;
    let mut environment = BTreeMap::new();
    for name in ENV_INPUTS { environment.insert((*name).into(), std::env::var(name).ok()); }
    // profile环境也会改变代码生成；仅冻结Cargo专用键，不把终端中的其他私密环境写入上下文。
    for (name, value) in std::env::vars().filter(|(name, _)| name.starts_with("CARGO_PROFILE_")) {
        environment.insert(name, Some(value));
    }
    for (name, value) in [
        ("PATH", toolchain_path(&rustc)?.into_string().map_err(|_| "PATH不是UTF-8")?),
        ("CARGO", path_text(&cargo)?), ("LIB", lib), ("CARGO_TARGET_DIR", path_text(target)?),
        ("CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER", path_text(&proxy)?),
        ("HOTPATCH_PROBE_REAL_LINKER", path_text(&raw_linker)?), ("DX_HOME", path_text(&dx_home)?),
        (LINK_CONTEXT_ENV, path_text(&directory.join("link-context.txt"))?),
    ] { environment.insert(name.into(), Some(value)); }
    environment.insert("HOTPATCH_PROBE_IMPORT_MANIFEST".into(), None);
    environment.insert("HOTPATCH_PROBE_SYSROOT_LIB".into(), None);
    // 独立 thin rustc 不属于已退出的 Cargo jobserver；临时调度句柄不能跨构建会话继承。
    environment.insert("CARGO_MAKEFLAGS".into(), None);
    environment.insert("MAKEFLAGS".into(), None);
    let mut context = Context { format: 1, project: project.into(), target: target.to_owned(), features: normalized_features(features)?,
        bin, dx_sha256: hash_file(dx, control)?, dx: dx.to_owned(), rustc_sha256: hash_file(&rustc, control)?, rustc, rustc_version,
        cargo_sha256: hash_file(&cargo, control)?, cargo,
        proxy_sha256: hash_file(&proxy, control)?, proxy, raw_linker, raw_sha256, sysroot_lib,
        session_cache, link_manifest: directory.join("link-context.txt"), imports_file: directory.join("imports.txt"),
        link_manifest_sha256: String::new(),
        imports: Vec::new(), environment };
    write_link_context(&context)?;
    context.link_manifest_sha256 = sha(&read_bounded(&context.link_manifest, 64 * 1024)?);
    Ok(context)
}

fn write_link_context(context: &Context) -> Result<(), String> {
    let text = format!("FEATHRISE_FUNCTION_LINK_V1\nproject\t{}\nroot\t{}\nmanifest\t{}\npackage\t{}\nbin\t{}\ncrate\t{}\ntarget\t{}\nsysroot\t{}\nimports\t{}\n",
        context.project.id, context.project.root.display(), context.project.manifest.display(), context.project.package,
        context.bin, context.bin.replace('-', "_"), context.target.display(), context.sysroot_lib.display(), context.imports_file.display());
    write_new(&context.link_manifest, text.as_bytes())
}

fn collect_imports(client: &Value, target: &Path, sysroot: &Path, control: &BuildControl) -> Result<Vec<(PathBuf, String)>, String> {
    let arguments = client["link_args"].as_array().ok_or("fat client缺少link_args")?;
    let mut imports = BTreeSet::new();
    for argument in arguments {
        let value = argument.as_str().ok_or("fat link_args不是字符串")?.trim_matches('"');
        if !value.to_ascii_lowercase().ends_with(".dll.lib") { continue; }
        if !Path::new(value).is_absolute() { return Err("fat动态导入库必须是实际绝对路径".into()); }
        let path = Path::new(value).canonicalize().map_err(|error| format!("fat导入库不存在：{error}"))?;
        if !path.starts_with(target) && path.parent() != Some(sysroot) { return Err("fat动态导入库逃出当前target/sysroot".into()); }
        if read_bounded_prefix(&path, 8)? != b"!<arch>\n" { return Err("fat导入库不是COFF归档".into()); }
        imports.insert(path);
        if imports.len() > 128 { return Err("fat动态导入库超过128项".into()); }
    }
    if !imports.iter().any(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("bevy_dylib-"))) {
        return Err("fat原始link_args未给出实际Bevy动态导入库；未扫描缓存猜测其他版本".into());
    }
    imports.into_iter().map(|path| hash_file(&path, control).map(|hash| (path, hash))).collect()
}

fn write_imports(path: &Path, imports: &[(PathBuf, String)]) -> Result<(), String> {
    let text = imports.iter().map(|(path, _)| path_text(path)).collect::<Result<Vec<_>, _>>()?.join("\n") + "\n";
    write_new(path, text.as_bytes())
}

fn verify_imports_file(path: &Path, imports: &[(PathBuf, String)]) -> Result<(), String> {
    let expected = imports.iter().map(|(path, _)| path_text(path)).collect::<Result<Vec<_>, _>>()?.join("\n") + "\n";
    if read_bounded(path, 64 * 1024)? != expected.as_bytes() { return Err("fat导入清单已改变".into()); }
    Ok(())
}

fn execute(command: &Command, input: Option<Vec<u8>>, project: &Project, control: &BuildControl, diagnostics: Diagnostics) -> Result<Vec<u8>, String> {
    build_process::log_build_command(command, control);
    let root = project.root.clone(); let log = control.log_sink();
    let reader = move |output| read_output(output, &root, log, diagnostics);
    let result = if let Some(input) = input { build_process::run_with_input(command, control, None, input, reader) }
        else { build_process::run(command, control, None, reader) }?;
    if result.exit_code != 0 { return Err(format!("dx构建失败，退出码{}；{}", result.exit_code, result.stderr)); }
    control.check()?;
    Ok(result.stdout)
}

/// 仅测试把真实展示适配器接到生产日志UI；不在普通程序暴露或安装自动输出。
#[cfg(test)]
pub(crate) fn with_fixture_output_log(sink: crate::project_log::ProjectLogSink, consume: impl FnOnce(&mut dyn FnMut(&[u8]))) {
    let root = std::env::current_dir().unwrap();
    let mut log = output_log::OutputLog::new(&root, Some(sink), None);
    consume(&mut |line| log.emit_line(line));
    log.finish();
}

fn read_output(mut output: File, project: &Path, log: Option<crate::project_log::ProjectLogSink>, diagnostics: Diagnostics) -> Result<Vec<u8>, String> {
    let mut tail = VecDeque::new(); let mut line = Vec::new(); let mut oversized = false; let mut truncated = false;
    let mut log = output_log::OutputLog::new(project, log, diagnostics);
    let mut buffer = [0; 8192];
    loop {
        let count = output.read(&mut buffer).map_err(|error| error.to_string())?;
        if count == 0 { break; }
        tail.extend(&buffer[..count]);
        let excess = tail.len().saturating_sub(MAX_COMPILER_OUTPUT_BYTES);
        if excess != 0 { tail.drain(..excess); truncated = true; }
        for &byte in &buffer[..count] {
            if !oversized { line.push(byte); if line.len() > MAX_COMPILER_OUTPUT_BYTES { line.clear(); oversized = true; } }
            if byte == b'\n' {
                if !oversized { log.emit_line(&line); }
                line.clear(); oversized = false;
            }
        }
    }
    if !line.is_empty() { log.emit_line(&line); }
    log.finish();
    let mut bytes: Vec<u8> = tail.into_iter().collect();
    if truncated { let skip = bytes.iter().take(3).take_while(|byte| (**byte & 0xc0) == 0x80).count(); bytes.drain(..skip); }
    Ok(bytes)
}

fn select_bin(project: &Project) -> Result<String, String> {
    if project.manifest.file_name().is_none_or(|name| name != "Cargo.toml") { return Err("热更新Cargo入口必须是显式Cargo.toml".into()); }
    let text = String::from_utf8(read_bounded(&project.manifest, CONTEXT_LIMIT)?).map_err(|error| error.to_string())?;
    let manifest: toml::Value = toml::from_str(&text).map_err(|error| error.to_string())?;
    // 当前fat上下文只确认显式叶包清单；虚拟workspace入口不能猜成员，也不能用索引panic。
    if manifest.get("package").and_then(|package| package.get("name")).and_then(toml::Value::as_str) != Some(project.package.as_str()) {
        return Err("热更新入口清单必须直接声明当前工程包，未猜测其他workspace包".into());
    }
    let mut bins = BTreeSet::new(); let mut explicit_main = false;
    if let Some(entries) = manifest.get("bin").and_then(toml::Value::as_array) {
        for entry in entries {
            let name = entry.get("name").and_then(toml::Value::as_str).ok_or("bin缺少名称")?;
            valid_bin(name)?; bins.insert(name.to_owned());
            explicit_main |= entry.get("path").and_then(toml::Value::as_str) == Some("src/main.rs") || name == project.package;
        }
    }
    let package = &manifest["package"];
    if package.get("autobins").and_then(toml::Value::as_bool) != Some(false) {
        let root = project.manifest.parent().unwrap();
        if root.join("src/main.rs").is_file() && !explicit_main { bins.insert(project.package.clone()); }
        if root.join("src/bin").is_dir() {
            for (index, entry) in fs::read_dir(root.join("src/bin")).map_err(|error| error.to_string())?.enumerate() {
                if index >= 128 { return Err("bin自动发现超过128目录项".into()); }
                let path = entry.map_err(|error| error.to_string())?.path();
                if path.extension().is_some_and(|extension| extension == "rs") || path.join("main.rs").is_file() {
                    let name = path.file_stem().and_then(|name| name.to_str()).ok_or("bin路径不是UTF-8")?;
                    valid_bin(name)?; bins.insert(name.to_owned());
                }
            }
        }
    }
    if let Some(selected) = package.get("default-run").and_then(toml::Value::as_str) {
        valid_bin(selected)?;
        if bins.contains(selected) { return Ok(selected.into()); }
        return Err("default-run不是当前清单可确认的bin".into());
    }
    if bins.len() != 1 { return Err("热更新需要Cargo default-run或唯一bin，未任取首个程序".into()); }
    Ok(bins.into_iter().next().unwrap())
}

fn valid_bin(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 128 || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')) {
        return Err("热更新bin名称超出已验证的128字节ASCII范围".into());
    }
    Ok(())
}

fn normalized_features(features: &str) -> Result<String, String> {
    let values = features.split(',').map(str::trim).filter(|value| !value.is_empty()).collect::<BTreeSet<_>>();
    if values.is_empty() || values.iter().any(|value| value.len() > 256 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'/'))) {
        return Err("热更新特性集合为空或包含不支持的名称".into());
    }
    Ok(values.into_iter().collect::<Vec<_>>().join(","))
}

fn bind_client_environment(client: &mut Value, name: &str, value: String) -> Result<(), String> {
    let envs = client["rustc_envs"].as_array_mut().ok_or("fat client缺少rustc_envs")?;
    envs.retain(|entry| entry[0].as_str() != Some(name));
    envs.push(serde_json::json!([name, value])); Ok(())
}

fn client_environment<'a>(client: &'a Value, name: &str) -> Result<&'a str, String> {
    let mut matching = client["rustc_envs"].as_array().ok_or("fat client缺少rustc_envs")?.iter()
        .filter(|entry| entry[0].as_str() == Some(name));
    let value = matching.next().and_then(|entry| entry[1].as_str()).ok_or("fat client缺少冻结工具上下文")?;
    if matching.next().is_some() { return Err("fat client工具上下文重复".into()); }
    Ok(value)
}

fn validate_project(project: &Project, control: &BuildControl) -> Result<(), String> {
    control.check()?;
    if project::open(&project.root)? != *project { return Err("工程身份已改变，热更新任务未继续".into()); }
    Ok(())
}

fn validate_artifacts(artifacts: &compiler_output::CompilerArtifacts, project: &Project, target: &Path) -> Result<(), String> {
    validate_artifact_paths(artifacts, target, &select_bin(project)?)
}

fn validate_artifact_paths(artifacts: &compiler_output::CompilerArtifacts, target: &Path, bin: &str) -> Result<(), String> {
    let expected_executable = target.join("x86_64-pc-windows-msvc/render-dev").join(format!("{bin}.exe"));
    validate_output_paths(artifacts, target, bin, &expected_executable)
}

fn validate_patch_artifact_paths(artifacts: &compiler_output::CompilerArtifacts, target: &Path, bin: &str,
    storage_key: &str, frozen_executable: &Path) -> Result<(), String> {
    let frozen = frozen_executable.canonicalize().map_err(|error| error.to_string())?;
    let group = target.join("feathrise-runtime-versions").join(storage_key);
    if frozen.parent().and_then(Path::parent) != Some(group.as_path())
        || frozen.file_name().is_none_or(|name| name != std::ffi::OsStr::new(&format!("{bin}.exe")))
        || !frozen.is_file() || !frozen.with_extension("pdb").is_file() {
        return Err("薄补丁冻结 EXE 不属于当前工程独占运行版本".into());
    }
    let staging = target.join("x86_64-pc-windows-msvc/render-dev/deps").join(format!("{}.exe", bin.replace('-', "_")));
    validate_output_paths(artifacts, target, bin, &staging)
}

fn validate_output_paths(artifacts: &compiler_output::CompilerArtifacts, target: &Path, bin: &str,
    expected_executable: &Path) -> Result<(), String> {
    let root = artifacts.root.canonicalize().map_err(|error| error.to_string())?;
    let executable = artifacts.executable.canonicalize().map_err(|error| error.to_string())?;
    let pdb = artifacts.pdb.canonicalize().map_err(|error| error.to_string())?;
    // dx 0.7.9 的 path 是 root_dir（打包输出）；EXE 则区分完整 Cargo 产物与 thin rustc 的 deps 临时目标。
    // 输出根和明确期待的 EXE/PDB 必须同时匹配，不能放宽成任意 target 内文件。
    let expected_root = target.join("dx").join(bin).join("debug/windows/app");
    if root != expected_root || executable != expected_executable || pdb != expected_executable.with_extension("pdb")
        || !root.is_dir() || !executable.is_file() || !pdb.is_file() {
        return Err(format!("dx实际输出目录或EXE/PDB身份不符：输出={}（期待{}）；EXE={}（期待{}）；PDB={}",
            root.display(), expected_root.display(), executable.display(), expected_executable.display(), pdb.display()));
    }
    Ok(())
}

fn existing_target(target: &Path) -> Result<PathBuf, String> {
    if !target.is_absolute() || !target.is_dir() { return Err("热更新必须复用已存在的绝对target目录".into()); }
    target.canonicalize().map_err(|error| error.to_string())
}

fn rustup_which(name: &str, project: &Project, control: &BuildControl) -> Result<PathBuf, String> {
    let mut command = Command::new("rustup"); command.args(["which", name]).current_dir(&project.root);
    PathBuf::from(small_output(&command, control)?.trim()).canonicalize().map_err(|error| error.to_string())
}

fn rustc_sysroot(rustc: &Path, project: &Project, control: &BuildControl) -> Result<PathBuf, String> {
    let mut command = Command::new(rustc);
    command.args(["--print", "sysroot"]).current_dir(&project.root);
    PathBuf::from(small_output(&command, control)?.trim()).canonicalize().map_err(|error| error.to_string())
}

fn toolchain_path(tool: &Path) -> Result<std::ffi::OsString, String> {
    let directory = tool.parent().ok_or("Rust 工具缺少所在目录")?;
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(directory.to_owned()).chain(std::env::split_paths(&inherited)))
        .map_err(|error| error.to_string())
}

fn small_output(command: &Command, control: &BuildControl) -> Result<String, String> {
    let result = build_process::run(command, control, Some(Duration::from_secs(30)), |mut file| {
        let mut bytes = VecDeque::new(); let mut chunk = [0; 4096];
        loop { let count = file.read(&mut chunk).map_err(|error| error.to_string())?; if count == 0 { break; }
            bytes.extend(&chunk[..count]); let excess = bytes.len().saturating_sub(16 * 1024); if excess != 0 { bytes.drain(..excess); } }
        String::from_utf8(bytes.into_iter().collect()).map_err(|error| error.to_string())
    })?;
    if result.exit_code != 0 { return Err(format!("工具准备失败（{}）：{}", result.exit_code, result.stderr)); }
    Ok(result.stdout)
}

fn discover_lib(project: &Project, control: &BuildControl) -> Result<String, String> {
    if let Some(lib) = std::env::var("LIB").ok().filter(|value| !value.trim().is_empty()) { return Ok(lib); }
    let program_files = PathBuf::from(std::env::var_os("ProgramFiles(x86)").ok_or("缺少ProgramFiles(x86)，无法定位MSVC/SDK")?);
    let mut command = Command::new(program_files.join("Microsoft Visual Studio/Installer/vswhere.exe"));
    command.args(["-latest", "-products", "*", "-property", "installationPath"]).current_dir(&project.root);
    let installation = PathBuf::from(small_output(&command, control)?.trim());
    let version = String::from_utf8(read_bounded(&installation.join("VC/Auxiliary/Build/Microsoft.VCToolsVersion.default.txt"), 256)?)
        .map_err(|error| error.to_string())?;
    let msvc = installation.join("VC/Tools/MSVC").join(version.trim()).join("lib/x64");
    if !msvc.join("libcmt.lib").is_file() { return Err("MSVC x64库不完整".into()); }
    let roots = program_files.join("Windows Kits/10/Lib"); let mut candidates = BTreeMap::new();
    for (index, entry) in fs::read_dir(&roots).map_err(|error| error.to_string())?.enumerate() {
        if index >= 128 { return Err("Windows SDK候选超过128项".into()); }
        let path = entry.map_err(|error| error.to_string())?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else { continue; };
        let Ok(parts) = name.split('.').map(str::parse::<u32>).collect::<Result<Vec<_>, _>>() else { continue; };
        if parts.len() == 4 && path.join("ucrt/x64/ucrt.lib").is_file() && path.join("um/x64/kernel32.lib").is_file() {
            candidates.insert(parts, path);
        }
    }
    let (_, sdk) = candidates.pop_last().ok_or("未发现完整Windows SDK x64库")?;
    Ok(format!("{};{};{}", msvc.display(), sdk.join("ucrt/x64").display(), sdk.join("um/x64").display()))
}

fn path_text(path: &Path) -> Result<String, String> {
    let value = path.to_str().ok_or("工具路径不是UTF-8")?;
    if value.chars().any(char::is_control) { return Err("工具路径包含控制字符".into()); }
    Ok(value.to_owned())
}
fn sha(bytes: &[u8]) -> String { Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect() }
fn hash_file(path: &Path, control: &BuildControl) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?; let mut hash = Sha256::new(); let mut buffer = [0; 64 * 1024];
    loop { control.check()?; let count = file.read(&mut buffer).map_err(|error| error.to_string())?; if count == 0 { break; } hash.update(&buffer[..count]); }
    Ok(hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}
fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new(); File::open(path).map_err(|error| error.to_string())?.take(maximum as u64 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > maximum { return Err(format!("工具输入{}超过{}字节", path.display(), maximum)); } Ok(bytes)
}
fn read_bounded_prefix(path: &Path, bytes: usize) -> Result<Vec<u8>, String> {
    let mut data = vec![0; bytes]; File::open(path).map_err(|error| error.to_string())?.read_exact(&mut data).map_err(|error| error.to_string())?; Ok(data)
}
fn read_lock(path: &Path) -> Result<File, String> { OpenOptions::new().read(true).share_mode(1).open(path).map_err(|error| error.to_string()) }
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> { OpenOptions::new().write(true).create_new(true).open(path).and_then(|mut file| file.write_all(bytes)).map_err(|error| error.to_string()) }

#[cfg(test)]
mod artifact_path_tests {
    use super::*;

    struct Fixture { directory: PathBuf, target: PathBuf, artifacts: compiler_output::CompilerArtifacts }
    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!("feathrise-dx-path-{}", bevy::asset::uuid::Uuid::new_v4()));
            fs::create_dir_all(directory.join("target")).unwrap();
            let directory = directory.canonicalize().unwrap();
            let target = directory.join("target");
            let root = target.join("dx/selected-bin/debug/windows/app");
            let executable = target.join("x86_64-pc-windows-msvc/render-dev/selected-bin.exe");
            fs::create_dir_all(&root).unwrap();
            fs::create_dir_all(executable.parent().unwrap()).unwrap();
            fs::write(&executable, b"fixture-exe-not-executed").unwrap();
            let pdb = executable.with_extension("pdb");
            fs::write(&pdb, b"fixture-pdb").unwrap();
            Self { directory, target, artifacts: compiler_output::CompilerArtifacts { root, executable, pdb } }
        }
    }
    impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.directory); } }

    #[test]
    fn nested_leaf_entry_keeps_full_project_identity_and_does_not_guess_virtual_workspace_members() {
        let fixture = Fixture::new(); let root = fixture.directory.join("project"); let leaf = root.join("game");
        fs::create_dir_all(leaf.join("src")).unwrap();
        fs::write(root.join("Cargo.toml"),"[workspace]\nmembers=['game']\n").unwrap();
        fs::write(leaf.join("Cargo.toml"),"[package]\nname='selected-package'\nversion='0.1.0'\n").unwrap();
        fs::write(leaf.join("src/main.rs"),"fn main() {}\n").unwrap();
        let write_descriptor = |manifest: &str| fs::write(root.join("editor-project.json"),serde_json::to_vec(&serde_json::json!({
            "format":1,"id":"nested-manifest-fixture","name":"嵌套清单夹具","cargo_manifest":manifest,"package":"selected-package"
        })).unwrap()).unwrap();
        write_descriptor("game/Cargo.toml");
        let selected = project::open(&root).unwrap();
        assert_eq!(select_bin(&selected).unwrap(),"selected-package");
        let identity = ProjectIdentity::from(&selected);
        assert_eq!(identity.root,root); assert_eq!(identity.manifest,leaf.join("Cargo.toml"));
        assert_eq!(identity.id,"nested-manifest-fixture"); assert_eq!(identity.package,"selected-package");
        write_descriptor("Cargo.toml");
        let virtual_entry = project::open(&root).unwrap();
        assert!(select_bin(&virtual_entry).unwrap_err().contains("未猜测其他workspace包"),"虚拟workspace必须明确拒绝，不能panic或猜第一个成员");
    }

    #[test]
    fn only_cargo_verbose_process_lines_are_hidden_in_plain_and_sgr_output() {
        for line in [
            "       Fresh bevy_app v0.19.1\n",
            "       Dirty avian_derive v0.2.3: the dependency `proc-macro-error3` was rebuilt\r\n",
            r"     Running `rustc --crate-name avian_derive --edition=2024 C:\Users\wcz\.cargo\registry\src\index.crates.io-1949cf8c6b5b557f\avian_derive-0.2.3\src\lib.rs --out-dir \\?\D:\bevy-project\bevy_learn5_3d\feathrise\target\render-dev\deps --cap-lints allow`",
            "\u{1b}[1m\u{1b}[32m       Fresh\u{1b}[0m bevy_app v0.19.1\n",
            "\u{1b}[33m       Dirty\u{1b}[0m avian_derive v0.2.3: the dependency `proc-macro-error3` was rebuilt\n",
            "\u{1b}[1;32m     Running\u{1b}[0m `CARGO=path rustc --crate-name rpg`\n",
            "\u{1b}[K       Fresh bevy_app v0.19.1\n",
        ] { assert!(cargo_verbose_process_line(line.as_bytes()), "{line:?}"); }
        for line in [
            "   Compiling avian_derive v0.2.3\n", "    Finished `render-dev` profile in 1.0s\n",
            "warning: Dirty rpg v0.1.0 is a user diagnostic\n", "error[E0308]: mismatched types\n",
            "\u{1b}[1;31merror\u{1b}[0m: Running `rustc` failed\n",
            "ordinary dx output\n", "Freshness matters\n", "Fresh fruit v1\n", "Dirty: cache failed\n",
            "Running a custom build task\n", "Running `unterminated\n", "Dirty package version\n",
            "       Dirty rpg v0.1.0: changed\rerror: actual failure\n",
            "\u{1b}[32\n",
        ] { assert!(!cargo_verbose_process_line(line.as_bytes()), "{line:?}"); }
    }

    #[test]
    fn dx_path_is_bundle_output_while_executable_remains_in_cargo_profile() {
        let fixture = Fixture::new();
        assert!(validate_artifact_paths(&fixture.artifacts, &fixture.target, "selected-bin").is_ok());
        // 作者源码根与输出根不同是官方合同；旧判断会把这类有效输出全部拒绝。
        let mut wrong = fixture.artifacts.clone(); wrong.root = fixture.directory.clone();
        assert!(validate_artifact_paths(&wrong, &fixture.target, "selected-bin").is_err());
    }

    #[test]
    fn dx_output_identity_rejects_other_bin_executable_and_pdb() {
        let fixture = Fixture::new();
        assert!(validate_artifact_paths(&fixture.artifacts, &fixture.target, "another-bin").is_err());
        let mut wrong = fixture.artifacts.clone(); wrong.executable = fixture.artifacts.pdb.clone();
        assert!(validate_artifact_paths(&wrong, &fixture.target, "selected-bin").is_err());
        let mut wrong = fixture.artifacts.clone(); wrong.pdb = fixture.artifacts.executable.clone();
        assert!(validate_artifact_paths(&wrong, &fixture.target, "selected-bin").is_err());
    }

    #[test]
    fn thin_receipt_and_frozen_baseline_keep_their_distinct_exact_paths() {
        let fixture = Fixture::new();
        let directory = fixture.target.join("feathrise-runtime-versions/current-project/owned-version");
        fs::create_dir_all(&directory).unwrap();
        let executable = directory.join("selected-bin.exe");
        fs::write(&executable, b"frozen-exe-not-executed").unwrap();
        let pdb = executable.with_extension("pdb"); fs::write(&pdb, b"frozen-pdb").unwrap();
        let frozen = compiler_output::CompilerArtifacts { root: fixture.artifacts.root.clone(), executable: executable.clone(), pdb };
        assert!(validate_artifact_paths(&frozen, &fixture.target, "selected-bin").is_err());
        assert!(validate_patch_artifact_paths(&frozen, &fixture.target, "selected-bin", "current-project", &executable).is_err());
        let staging = fixture.target.join("x86_64-pc-windows-msvc/render-dev/deps/selected_bin.exe");
        fs::create_dir_all(staging.parent().unwrap()).unwrap(); fs::write(&staging, b"thin-staging-not-executed").unwrap();
        let pdb = staging.with_extension("pdb"); fs::write(&pdb, b"thin-staging-pdb").unwrap();
        let thin = compiler_output::CompilerArtifacts { root: fixture.artifacts.root.clone(), executable: staging, pdb };
        assert!(validate_patch_artifact_paths(&thin, &fixture.target, "selected-bin", "current-project", &executable).is_ok());
        assert!(validate_artifact_paths(&thin, &fixture.target, "selected-bin").is_err());
        assert!(validate_patch_artifact_paths(&fixture.artifacts, &fixture.target, "selected-bin", "current-project", &executable).is_err());
        assert!(validate_patch_artifact_paths(&thin, &fixture.target, "selected-bin", "other-project", &executable).is_err());
        let mut wrong = thin.clone(); wrong.pdb = fixture.artifacts.pdb.clone();
        assert!(validate_patch_artifact_paths(&wrong, &fixture.target, "selected-bin", "current-project", &executable).is_err());
        let second = directory.join("another-version/selected-bin.exe");
        fs::create_dir_all(second.parent().unwrap()).unwrap(); fs::write(&second, b"not-the-base").unwrap();
        assert!(validate_patch_artifact_paths(&thin, &fixture.target, "selected-bin", "current-project", &second).is_err());
    }

    #[test]
    fn raw_thin_rustc_error_reaches_original_source_location_without_log_mirror_duplicates() {
        let fixture = Fixture::new();
        fs::create_dir(fixture.directory.join("src")).unwrap();
        fs::write(fixture.directory.join("src/controller.rs"), "fn live_step() -> f32 { 1_u32 }\n").unwrap();
        let diagnostic = serde_json::json!({"$message_type":"diagnostic","message":"mismatched types",
            "code":{"code":"E0308"},"level":"error","spans":[{"file_name":"src\\controller.rs",
                "line_start":149,"column_start":25,"is_primary":true}]});
        let mirror = serde_json::json!({"timestamp":"1.64s","level":"INFO",
            "json":serde_json::json!({"RustcOutput":{"message":diagnostic}}).to_string()});
        let cargo_warning = serde_json::json!({"reason":"compiler-message","message":{
            "message":"unused variable: retained","level":"warning","code":{"code":"unused_variables"},
            "spans":[],"rendered":"warning: unused variable: retained\n"}});
        let path = fixture.directory.join("thin-output.txt");
        // 让Running行的SGR跨8192字节读取边界，仍由原read_output完整拼行。
        let prefix = "x".repeat(8187) + "\n";
        let text = format!("{prefix}\u{1b}[32m     Running\u{1b}[0m `rustc --crate-name rpg`\n\
            \x20      Fresh bevy_app v0.19.1\n\
            \x20      Dirty rpg v0.1.0: the dependency was rebuilt\n\
            warning: retained warning\n{diagnostic}\n{mirror}\n{cargo_warning}\nerror: retained failure\n");
        fs::write(&path, &text).unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let callback: Arc<dyn Fn(CompilerDiagnostic) + Send + Sync> = Arc::new(move |diagnostic| { sender.send(diagnostic).unwrap(); });
        let (sink, probe) = crate::project_log::fixture_log_probe("dx-thin-visible-diagnostics");
        let raw = read_output(File::open(path).unwrap(), &fixture.directory, Some(sink), Some(callback)).unwrap();
        assert_eq!(raw, text.as_bytes(), "展示过滤不得删除/改写原始工具输出tail");
        let events = probe.drain();
        let visible = events.iter().filter(|event| event.kind == "line").map(|event| event.text.as_str()).collect::<Vec<_>>().join("\n");
        assert!(visible.contains("error[E0308]: mismatched types"));
        assert!(visible.contains("warning: unused variable: retained") && visible.contains("error: retained failure"));
        assert!(!visible.contains("Running") && !visible.contains("Fresh") && !visible.contains("Dirty")
            && !visible.contains("RustcOutput") && !visible.contains("compiler-message"));
        assert_eq!(probe.dropped(), 0);
        let diagnostics = receiver.into_iter().collect::<Vec<_>>();
        assert_eq!(diagnostics.len(), 2, "两条原诊断各发布一次，RustcOutput镜像不重复发布");
        assert_eq!(diagnostics[0].code.as_deref(), Some("E0308"));
        let location = diagnostics[0].location.as_ref().unwrap();
        assert_eq!(location.path, PathBuf::from("src/controller.rs"));
        assert_eq!((location.line, location.column), (149, 25));
        assert_eq!(diagnostics[1].level, crate::compiler_diagnostics::DiagnosticLevel::Warning);
        assert_eq!(diagnostics[1].code.as_deref(), Some("unused_variables"));
        assert_eq!(diagnostics[1].message, "unused variable: retained");
        let cargo = serde_json::json!({"reason":"compiler-message","message":diagnostic});
        assert_eq!(compiler_diagnostic(&cargo, &fixture.directory), Some(diagnostics[0].clone()));
    }

    #[test]
    fn verbose_display_filter_keeps_raw_fat_and_hotpatch_receipts_parseable() {
        let fixture = Fixture::new();
        let artifacts = serde_json::json!({"path":fixture.artifacts.root,"exe":fixture.artifacts.executable,
            "rustc_args":["rustc","--crate-name","selected_bin"],"rustc_envs":[],"link_args":[],"assets":[]});
        let fat = serde_json::json!({"BuildsFinished":{"client":artifacts,"server":null}});
        let patch = serde_json::json!({"Hotpatch":{"artifacts":artifacts,"jump_table":{
            "lib":fixture.artifacts.root.join("patch.dll"),"map":{"4096":8192},
            "aslr_reference":4096,"new_base_address":8192,"ifunc_count":0}}});
        for (name, receipt) in [("fat", fat), ("patch", patch)] {
            let wrapped = serde_json::json!({"timestamp":"1s","level":"INFO","json":receipt.to_string()});
            let noisy = "       Fresh bevy_app v0.19.1\n     Running `rustc --crate-name selected_bin`\n".repeat(1024);
            let text = format!("{noisy}    Finished `render-dev` profile in 1.0s\n{wrapped}\n");
            let path = fixture.directory.join(format!("{name}-verbose-output.txt"));
            fs::write(&path, &text).unwrap();
            let (sink, probe) = crate::project_log::fixture_log_probe("dx-fat-visible-output");
            let raw = read_output(File::open(path).unwrap(), &fixture.directory, Some(sink), None).unwrap();
            assert_eq!(raw, text.as_bytes());
            let events = probe.drain();
            let lines: Vec<_> = events.iter().filter(|event| event.kind == "line").map(|event| event.text.as_str()).collect();
            assert_eq!(lines, ["    Finished `render-dev` profile in 1.0s"]);
            assert_eq!(probe.dropped(), 0, "2048条过程输出不能占用真实日志邮箱");
            if name == "fat" {
                let parsed = compiler_output::parse_fat_output(&raw).unwrap();
                assert_eq!(parsed.artifacts.executable, fixture.artifacts.executable);
                assert_eq!(serde_json::from_slice::<Value>(&parsed.client_json).unwrap(), artifacts);
            } else {
                let parsed = compiler_output::parse_hotpatch_output(&raw).unwrap();
                assert_eq!((parsed.aslr_reference, parsed.entry_count), (4096, 1));
                assert_eq!(parsed.library, fixture.artifacts.root.join("patch.dll"));
            }
        }
    }
}
