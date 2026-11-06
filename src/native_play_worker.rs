//! 阻塞型普通游戏构建/启动只在唯一后台槽执行；Cargo日志与游戏日志持续有界排空。

#[path = "native_play_dependency.rs"]
mod dependency;
#[path = "native_play_features.rs"]
mod features;
#[path = "native_play_hotpatch.rs"]
#[cfg(windows)]
mod hotpatch;
use feathrise_runtime_manifest::streamline as streamline_files;
#[cfg(test)]
#[path = "native_play_worker_map_tests.rs"]
mod map_tests;

use crate::{
    build_process::{self, BuildControl},
    project::{self, Project},
};
use feathrise_editor_map::standalone::RunSavedProject;
use feathrise_editor_core::ProjectBuildProfile;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, MutexGuard, OnceLock, mpsc::SyncSender},
    time::Duration,
};

const LINE_LIMIT: usize = 1024 * 1024;
const LOG_LIMIT: usize = 16 * 1024;

pub(super) fn run(
    project: Project,
    target: PathBuf,
    request: RunSavedProject,
    control: &BuildControl,
    progress: &SyncSender<String>,
    started: Option<SyncSender<()>>,
) -> Result<u32, String> {
    let prepared = prepare(&project, &target, request.map_id.as_deref(), request.profile, control)?;
    run_prepared(project, Arc::new(prepared), request, control, progress, started)
}

/// 显式采用后台已完成的不可变版本；直接运行副本，避免 Windows 占用 Cargo 的下一次链接目标。
pub(super) fn run_prepared(
    project: Project,
    prepared: Arc<PreparedGame>,
    request: RunSavedProject,
    control: &BuildControl,
    progress: &SyncSender<String>,
    started: Option<SyncSender<()>>,
) -> Result<u32, String> {
    if !prepared.matches_project(&project, request.profile) { return Err("运行版本不属于当前工程或构建配置".into()); }
    if project::open(&project.root)? != project { return Err("工程清单已变化，拒绝运行旧工程版本".into()); }
    if let Some(map_id) = request.map_id.as_deref() { validate_requested_map(&project, map_id, control)?; }
    let warning = if request.has_unsaved_changes {
        "；当前未保存修改仍只在编辑器"
    } else {
        ""
    };
    let _ = progress.try_send(format!(
        "正在运行已保存的工程；关闭工程窗口可返回编辑器{warning}"
    ));
    let mut game = prepared.embedded_command(&project, control)?;
    game.env("EDITOR_SAVED_PLAY", "1");
    if let Some(map_id) = request.map_id {
        game.env("EDITOR_SAVED_PLAY_MAP", map_id);
    } else {
        game.env_remove("EDITOR_SAVED_PLAY_MAP");
    }
    let log = control.log_sink();
    let result = build_process::run_with_started(&game, control, None, started, move |output| {
        read_log(crate::project_log::LogReader::new(output, log, crate::project_log::LogSource::Stdout))
    })?;
    if result.exit_code != 0 {
        return Err(crate::environment_diagnostics::cargo_exit_failure(
            crate::environment_diagnostics::CargoStage::Play,
            &game, result.exit_code, &result.stdout, &result.stderr,
        ));
    }
    Ok(result.exit_code)
}

/// 两种呈现方式共用实际工程构建；结果只在该次工程身份未变时交给启动层。
#[derive(Clone, Debug)]
pub(crate) struct PreparedGame {
    pub executable: PathBuf,
    pub target: PathBuf,
    linked_paths: Vec<PathBuf>,
    dlls: Vec<crate::build_artifacts::CargoDll>,
    build_outputs: Vec<(String, PathBuf)>,
    project_id: String,
    project_root: PathBuf,
    profile: ProjectBuildProfile,
    bundle: Option<crate::build_artifacts::PublishedBundle>,
    pub(crate) hotpatch: Option<Arc<crate::code_iteration::hotpatch::FatBaseline>>,
}

pub(crate) type DiagnosticCallback = Arc<dyn Fn(crate::compiler_diagnostics::CompilerDiagnostic) + Send + Sync>;

pub(crate) fn build_feature_identity() -> String { features::identity() }

#[cfg(test)]
pub(crate) fn prepared_for_test(project: &Project, profile: ProjectBuildProfile) -> PreparedGame {
    PreparedGame { executable: project.root.join("test-version/game.exe"), target: project.root.join("unused-target"),
        linked_paths: Vec::new(), dlls: Vec::new(), build_outputs: Vec::new(),
        project_id: project.id.clone(), project_root: project.root.clone(), profile, bundle: None, hotpatch: None }
}

// 普通启动与自动编译共用一个取消感知后台门；不能各自再启动 Cargo 等待同一缓存锁。
static BUILD_SLOT: OnceLock<Mutex<bool>> = OnceLock::new();

struct BuildSlotGuard<'slot, 'control> {
    failed: MutexGuard<'slot, bool>,
    control: &'control BuildControl,
}

impl Drop for BuildSlotGuard<'_, '_> {
    fn drop(&mut self) {
        // 共享槽保留跨控制牌的永久屏障，不能换一个 BuildControl 绕过尚未确认退出的旧 Job。
        if self.control.cleanup_failed() { *self.failed = true; }
    }
}

fn check_build_slot(failed: bool, control: &BuildControl) -> Result<(), String> {
    if failed {
        control.mark_cleanup_failed();
        return Err("先前构建进程树未确认收口，本次宿主禁止启动后续构建；作者现场保留".into());
    }
    control.check()
}

fn acquire_build_slot(control: &BuildControl) -> Result<BuildSlotGuard<'static, '_>, String> {
    let slot = BUILD_SLOT.get_or_init(|| Mutex::new(false));
    loop {
        control.check()?;
        match slot.try_lock() {
            Ok(failed) => {
                check_build_slot(*failed, control)?;
                return Ok(BuildSlotGuard { failed, control });
            }
            Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(10)),
            Err(std::sync::TryLockError::Poisoned(_)) => return Err("后台构建槽异常，拒绝并行启动第二次 Cargo".into()),
        }
    }
}

pub(crate) fn prepare(
    project: &Project,
    target: &Path,
    map_id: Option<&str>,
    profile: ProjectBuildProfile,
    control: &BuildControl,
) -> Result<PreparedGame, String> {
    prepare_with_diagnostics(project, target, map_id, profile, control, None)
}

pub(crate) fn prepare_export(
    project: &Project,
    target: &Path,
    profile: ProjectBuildProfile,
    control: &BuildControl,
) -> Result<PreparedGame, String> {
    // 显式厂商原生组合有独立的运行库前提，基础 Windows 导出才要求整个依赖闭包免装 VC Redist。
    let no_vc_redist = cfg!(all(windows, target_env = "msvc"))
        && !cfg!(any(feature = "rtx_rendering", feature = "dlss_frame_generation", feature = "fsr3_frame_generation"));
    prepare_build(project, target, None, profile, control, None, no_vc_redist)
}

pub(crate) fn prepare_with_diagnostics(
    project: &Project,
    target: &Path,
    map_id: Option<&str>,
    profile: ProjectBuildProfile,
    control: &BuildControl,
    diagnostics: Option<DiagnosticCallback>,
) -> Result<PreparedGame, String> {
    prepare_build(project, target, map_id, profile, control, diagnostics, false)
}

fn prepare_build(
    project: &Project,
    target: &Path,
    map_id: Option<&str>,
    profile: ProjectBuildProfile,
    control: &BuildControl,
    diagnostics: Option<DiagnosticCallback>,
    no_vc_redist: bool,
) -> Result<PreparedGame, String> {
    let _guard = acquire_build_slot(control)?;
    control.check()?;
    if project::open(&project.root)? != *project {
        return Err("工程清单已变化，请重新打开工程".into());
    }
    verify_game_contract(&project, control)?;
    if let Some(id) = map_id {
        validate_requested_map(&project, id, control)?;
    }
    fs::create_dir_all(&target).map_err(|error| crate::environment_diagnostics::cache_failure(&target, "准备构建缓存", &error))?;
    let target = target.canonicalize().map_err(|error| crate::environment_diagnostics::cache_failure(&target, "确认构建缓存目录", &error))?;
    // 独立交付只运行已保存工程，无需加载 F12 的 fat 热补丁基线或其动态 CRT。
    #[cfg(windows)]
    if !no_vc_redist && let Some(prepared) = hotpatch::prepare(project, &target, profile, control, diagnostics.clone())? {
        return Ok(prepared);
    }
    let cargo = std::env::var_os("EDITOR_HOST_CARGO")
        .or_else(|| std::env::var_os("CARGO"))
        .unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(&cargo);
    // 启动普通game而非DLL或作者宿主；Cargo自动判断新源码，未改源码时直接复用缓存。
    command.arg("build");
    configure_project_cargo(&mut command, project, &target, profile);
    if no_vc_redist {
        configure_export_crt(&mut command);
    }
    command.args(["--bins", "--message-format=json"]);
    // 只依据该工程自己的依赖决定是否允许取源，不把编辑器的新建配置套到旧工程上。
    crate::project_template::apply_project_network_policy(&project.manifest, &mut command)?;
    features::apply(&mut command);
    let manifest = project.manifest.clone();
    let project_root = project.root.clone();
    let log = control.log_sink();
    if log.is_some() {
        // 日志旁路重定向后Cargo默认隐藏进度和颜色；只为本次可见构建恢复终端输出。
        command.env("CARGO_TERM_PROGRESS_WHEN", "always").env("CARGO_TERM_PROGRESS_WIDTH", "100")
            .env("CARGO_TERM_COLOR", "always");
    }
    let build = build_process::run_cargo(&command, control, None, move |output| {
        read_artifacts_with_diagnostics(output, &manifest, &project_root, log, diagnostics)
    })?;
    if build.exit_code != 0 {
        if let Some(log) = control.log_sink() { log.task_message(&format!("构建失败，退出码{}", build.exit_code)); }
        return Err(crate::environment_diagnostics::cargo_exit_failure(
            crate::environment_diagnostics::CargoStage::Build,
            &command, build.exit_code, &build.stdout.diagnostics, &build.stderr,
        ));
    }
    let executable = select_executable(build.stdout.executables, &target)?;
    control.check()?;
    if project::open(&project.root)? != *project {
        return Err("工程清单在构建期间变化，本次工程运行已取消".into());
    }
    verify_game_contract(&project, control)?;
    if let Some(id) = map_id {
        // 构建期间作者可能删除或改写目标；只复核当前目标，不让无关坏图挡住本次工程运行。
        validate_requested_map(&project, id, control)?;
    }
    if let Some(log) = control.log_sink() { log.task_message("工程构建完成"); }
    PreparedGame { executable, target, linked_paths: build.stdout.linked_paths,
        dlls: build.stdout.dlls, build_outputs: build.stdout.build_outputs,
        project_id: project.id.clone(), project_root: project.root.clone(), profile, bundle: None, hotpatch: None,
    }.snapshot(project, control, no_vc_redist)
}

impl PreparedGame {
    /// 只向导出器借出已校验完整版本；租约由 PreparedGame 保持，不能从缓存猜测文件来源。
    pub(crate) fn distribution_bundle(&self, control: &BuildControl) -> Result<&crate::build_artifacts::PublishedBundle, String> {
        let bundle = self.bundle.as_ref().ok_or("工程尚未发布完整运行版本")?;
        crate::build_artifacts::verify(bundle, control)?;
        Ok(bundle)
    }
    /// 主线程只做固定字段检查；源码/工具链与产物 SHA 的完整检查已经在后台完成。
    pub(crate) fn matches_project(&self, project: &Project, profile: ProjectBuildProfile) -> bool {
        self.project_id == project.id && self.project_root == project.root && self.profile == profile
    }

    fn snapshot(&self, project: &Project, control: &BuildControl, no_vc_redist: bool) -> Result<Self, String> {
        if !self.matches_project(project, self.profile) { return Err("运行副本的工程身份不匹配".into()); }
        if let Some(bundle) = &self.bundle { crate::build_artifacts::verify(bundle, control)?; return Ok(self.clone()); }
        let mut search_paths = self.linked_paths.clone();
        let profile = self.executable.parent().ok_or("工程产物缺少 profile 目录")?;
        search_paths.extend([profile.join("deps"), profile.to_path_buf(), rust_runtime_directory(project, control)?]);
        let mut files = crate::build_artifacts::runtime_files(&self.executable, &self.target, &self.dlls, &search_paths, no_vc_redist, control)?;
        append_vendor_runtime(&self.build_outputs, &self.target, &mut files)?;
        let bundle = crate::build_artifacts::publish(&self.target, &project.context.storage_key, &self.executable, &files,
            serde_json::json!({"project_id":project.id,"project_root":project.root,"profile":self.profile.cargo_profile()}), control)?;
        let mut prepared = self.clone();
        prepared.executable = bundle.executable.clone();
        prepared.linked_paths = vec![bundle.directory.clone()];
        prepared.bundle = Some(bundle);
        Ok(prepared)
    }

    pub(crate) fn bind_build_metadata(&mut self, metadata: serde_json::Value, control: &BuildControl) -> Result<(), String> {
        let Some(baseline) = self.hotpatch.as_mut() else {
            let bundle = self.bundle.as_mut().ok_or("运行版本尚未形成完整副本")?;
            return crate::build_artifacts::bind_metadata(bundle, metadata, control);
        };
        // fat版本在本对象内部持有两份相同租约；对外共享过的基线仍然禁止修改，不能靠克隆绕过。
        let baseline = Arc::get_mut(baseline).ok_or("已交付热更新基线不能修改构建身份")?;
        let bundle = self.bundle.as_ref().ok_or("运行版本尚未形成完整副本")?;
        if bundle.directory != baseline.bundle.directory || bundle.executable != baseline.bundle.executable
            || bundle.manifest_sha256 != baseline.bundle.manifest_sha256 {
            return Err("运行版本与热更新基线身份不一致，拒绝修改构建身份".into());
        }
        // 暂撤本对象自己的重复持有，仍由原绑定器的唯一租约门拒绝任何外部运行包持有者。
        drop(self.bundle.take());
        let result = crate::build_artifacts::bind_metadata(&mut baseline.bundle, metadata, control);
        // 包含末尾取消在内的失败也必须恢复两份当前摘要，不能遗失副本或留下旧基线清单。
        self.bundle = Some(baseline.bundle.clone());
        result
    }
    /// 游戏直接作为Job根进程接管专用句柄；不能隔着cargo run猜孙进程的句柄继承。
    /// DLL来源取本次Cargo回执、实际profile与当前rustc sysroot，不搜索其他项目缓存。
    pub(crate) fn embedded_command(&self, project: &Project, control: &BuildControl) -> Result<Command, String> {
        if !self.matches_project(project, self.profile) { return Err("运行版本不属于当前工程".into()); }
        if let Some(bundle) = &self.bundle { crate::build_artifacts::verify(bundle, control)?; }
        let profile = self.executable.parent().ok_or("工程产物缺少profile目录")?;
        let mut paths = self.linked_paths.clone();
        paths.extend([profile.to_path_buf()]);
        #[cfg(windows)]
        paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()));
        let mut command = Command::new(&self.executable);
        command.current_dir(&project.root);
        let library_paths = std::env::join_paths(paths).map_err(|error| format!("运行库路径无效：{error}"))?;
        #[cfg(windows)]
        command.env("PATH", &library_paths);
        #[cfg(target_os = "linux")]
        command.env("LD_LIBRARY_PATH", &library_paths);
        #[cfg(target_os = "macos")]
        command.env("DYLD_LIBRARY_PATH", &library_paths);
        command
            .env_remove("EDITOR_SAVED_PLAY")
            .env_remove("EDITOR_SAVED_PLAY_MAP")
            .env_remove("FEATHRISE_WORKSPACE_PARENT_HWND")
            .env_remove("FEATHRISE_WORKSPACE_PARENT_PID")
            .env_remove("FEATHRISE_WORKSPACE_TOKEN");
        if let Some(baseline) = &self.hotpatch {
            command.env("FEATHRISE_CODE_PATCH_ROOT", &baseline.patch_root);
        } else {
            command.env_remove("FEATHRISE_CODE_PATCH_ROOT");
        }
        Ok(command)
    }
}

/// 补丁构建与普通构建持有相同的唯一后台门；旧任务真实结束前不能并行占用链接缓存。
pub(crate) fn prepare_function_patch(
    project: &Project, target: &Path,
    context: &crate::code_iteration::hotpatch::PatchContext, generation: u64,
    control: &BuildControl, diagnostics: Option<DiagnosticCallback>,
) -> Result<crate::code_iteration::hotpatch::PreparedPatch, String> {
    let _guard = acquire_build_slot(control)?;
    #[cfg(windows)]
    { crate::code_iteration::hotpatch::artifacts::prepare(project, target, context, generation, control, diagnostics) }
    #[cfg(not(windows))]
    {
        // Unix不会发布Windows fat基线，正常源码保存沿上面的完整构建／重启路径执行。
        let _ = (project, target, context, generation, diagnostics);
        control.check()?;
        Err("当前平台使用完整构建与重启，不采用Windows函数热补丁".into())
    }
}

fn rust_runtime_directory(project: &Project, control: &BuildControl) -> Result<PathBuf, String> {
    let mut rustc = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    rustc.args(["--print", "target-libdir"]).current_dir(&project.root);
    let result = build_process::run(&rustc, control, Some(Duration::from_secs(30)), read_log)?;
    if result.exit_code != 0 { return Err(format!("Rust 运行库定位失败：{}", result.stderr)); }
    let text = result.stdout.trim();
    if text.is_empty() || text.contains(['\r', '\n']) { return Err("rustc 未返回单条运行库目录".into()); }
    let directory = PathBuf::from(text).canonicalize().map_err(|error| error.to_string())?;
    if !directory.is_dir() { return Err("rustc 运行库目录无效".into()); }
    Ok(directory)
}

fn append_vendor_runtime(outputs: &[(String, PathBuf)], target: &Path, files: &mut Vec<crate::build_artifacts::RuntimeFile>) -> Result<(), String> {
    // SDK 的动态 LoadLibrary 组件不会出现在 PE 导入表；按同次 build.rs 回执复制其既有完整白名单。
    let groups: &[(&str, bool, &str, &[&str])] = &[
        ("dlss_wgpu", cfg!(feature = "rtx_rendering"), "dlss_runtime", &["nvngx_dlss.dll", "nvngx_dlssd.dll"]),
        ("feathrise_fsr3_frame_generation", cfg!(feature = "fsr3_frame_generation"), "fsr3-frame-generation-runtime", &["amd_fidelityfx_vk.dll", "manifest.json", "LICENSE-FidelityFX-SDK.txt", "v1.1.4-vulkan-lifetime.patch", "source-manifest.json", "README.md"]),
    ];
    for (package, enabled, directory, names) in groups {
        if !enabled { continue; }
        let mut matching = outputs.iter().filter(|(name, _)| name == package);
        let (_, root) = matching.next().ok_or_else(|| format!("本次 Cargo 缺少 {package} 的运行库回执"))?;
        if matching.next().is_some() { return Err(format!("本次 Cargo 的 {package} 运行库来源不唯一")); }
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        if !root.starts_with(target) { return Err("厂商运行库回执逃出本次构建缓存".into()); }
        for name in *names {
            let relative = if *package == "dlss_wgpu" { PathBuf::from(name) } else { PathBuf::from(directory).join(name) };
            files.push(crate::build_artifacts::RuntimeFile { source: root.join(directory).join(name), relative });
        }
    }
    if cfg!(feature = "dlss_frame_generation") {
        let mut matching = outputs.iter().filter(|(name, _)| name == "feathrise_frame_generation");
        let (_, root) = matching.next().ok_or("本次 Cargo 缺少 Streamline 运行库回执")?;
        if matching.next().is_some() { return Err("本次 Cargo 的 Streamline 来源不唯一".into()); }
        let root = root.canonicalize().map_err(|error| error.to_string())?;
        if !root.starts_with(target) { return Err("Streamline 回执逃出本次构建缓存".into()); }
        for (name, _) in streamline_files::RUNTIME_FILES {
            files.push(crate::build_artifacts::RuntimeFile { source: root.join("streamline-runtime").join(name), relative: PathBuf::from("streamline-runtime").join(name) });
        }
    }
    Ok(())
}

/// build和随后cargo run使用同一冻结配置与缓存，Release失败由原调用链报告，绝不回退Debug。
fn configure_project_cargo(command: &mut Command, project: &Project, target: &Path, profile: ProjectBuildProfile) {
    command.args(["--locked", "--manifest-path"])
        .arg(&project.manifest)
        .args(["-p", &project.package, "--profile", profile.cargo_profile()])
        .current_dir(&project.root)
        .env("CARGO_TARGET_DIR", target);
}

fn configure_export_crt(command: &mut Command) {
    const STATIC_CRT: &str = "-Ctarget-feature=+crt-static";
    // Cargo 优先采用这两种环境变量。向已生效的值追加标志，避免丢失作者显式传入的参数。
    if let Some(mut flags) = std::env::var_os("CARGO_ENCODED_RUSTFLAGS") {
        if !flags.is_empty() { flags.push("\x1f"); }
        flags.push(STATIC_CRT);
        command.env("CARGO_ENCODED_RUSTFLAGS", flags);
    } else if let Some(mut flags) = std::env::var_os("RUSTFLAGS") {
        flags.push(" ");
        flags.push(STATIC_CRT);
        command.env("RUSTFLAGS", flags);
    } else {
        // 使用独立的 cfg 键与工程的具体 target 配置合并，避免改写工程的 Cargo 文件或目标选择。
        command.args(["--config", r#"target.'cfg(all(windows, target_env = "msvc"))'.rustflags=['-Ctarget-feature=+crt-static']"#]);
    }
}

fn validate_requested_map(
    project: &Project,
    map_id: &str,
    control: &BuildControl,
) -> Result<(), String> {
    control.check()?;
    let context = feathrise_runtime::ProjectContext {
        root: project.root.clone(),
        id: project.id.clone(),
        name: project.name.clone(),
        asset_roots: project.context.asset_roots.clone(),
    };
    feathrise_runtime::validate_saved_map_with_check(&context, map_id, || control.check())
}

#[derive(Default)]
struct CargoArtifacts {
    executables: Vec<PathBuf>,
    linked_paths: Vec<PathBuf>,
    diagnostics: String,
    dlls: Vec<crate::build_artifacts::CargoDll>,
    build_outputs: Vec<(String, PathBuf)>,
}

/// 普通打开不覆写旧工程源码；只有工程明确声明本版完整游戏/F12返回合同才启动。
fn verify_game_contract(project: &Project, control: &BuildControl) -> Result<(), String> {
    control.check()?;
    let descriptor = project
        .root
        .join("editor-project.json")
        .canonicalize()
        .map_err(|error| format!("工程入口清单不可读：{error}"))?;
    if !descriptor.is_file() || !descriptor.starts_with(&project.root) {
        return Err("工程入口清单必须属于当前工程".into());
    }
    let mut bytes = Vec::new();
    File::open(descriptor)
        .map_err(|error| error.to_string())?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    validate_game_contract(&bytes)?;
    dependency::verify(project, control)
}

fn validate_game_contract(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > 64 * 1024 {
        return Err("工程入口清单超过64KiB预算".into());
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| format!("工程入口清单无效：{error}"))?;
    if manifest["game_entry"]["kind"] != "saved-world"
        || manifest["game_entry"]["version"].as_u64() != Some(1)
    {
        return Err(
            "该工程使用旧原型或未登记的工程入口；需显式迁移入口或新建工程，编辑器不会覆写工程源码"
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
fn read_artifacts(output: impl Read, manifest: &Path, log: Option<crate::project_log::ProjectLogSink>) -> Result<CargoArtifacts, String> {
    read_artifacts_with_diagnostics(output, manifest, manifest.parent().unwrap_or(Path::new(".")), log, None)
}

fn read_artifacts_with_diagnostics(output: impl Read, manifest: &Path, project_root: &Path, log: Option<crate::project_log::ProjectLogSink>, diagnostics: Option<DiagnosticCallback>) -> Result<CargoArtifacts, String> {
    let mut reader = BufReader::new(output);
    let mut log = crate::project_log::LogDecoder::new(log, crate::project_log::LogSource::Build);
    let mut artifacts = CargoArtifacts::default();
    let mut line = Vec::new();
    loop {
        line.clear();
        let mut end = false;
        while !end {
            let available = reader
                .fill_buf()
                .map_err(|error| format!("Cargo输出读取失败：{error}"))?;
            if available.is_empty() {
                break;
            }
            let count = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |at| at + 1);
            if line.len() + count > LINE_LIMIT {
                return Err("Cargo单条输出超过1MiB，本次构建已取消".into());
            }
            line.extend_from_slice(&available[..count]);
            end = available[count - 1] == b'\n';
            reader.consume(count);
        }
        if line.is_empty() {
            break;
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&line) else {
            append_tail(&mut artifacts.diagnostics, &String::from_utf8_lossy(&line));
            log.write(&line);
            continue;
        };
        if let Some(callback) = &diagnostics {
            if let Some(diagnostic) = crate::compiler_diagnostics::from_cargo(&value, project_root) { callback(diagnostic); }
        }
        if value["reason"] == "compiler-artifact" {
            for path in value["filenames"].as_array().into_iter().flatten().filter_map(serde_json::Value::as_str).map(PathBuf::from)
                .filter(|path| path.extension().is_some_and(|extension| ["dll", "so", "dylib"].iter().any(|kind| extension.eq_ignore_ascii_case(kind)))) {
                if artifacts.dlls.iter().any(|record| record.path == path) { continue; }
                if artifacts.dlls.len() == 256 { return Err("Cargo DLL 回执超过 256 项".into()); }
                artifacts.dlls.push(crate::build_artifacts::CargoDll { path,
                    package_id: value["package_id"].as_str().unwrap_or_default().into(),
                    target_name: value["target"]["name"].as_str().unwrap_or_default().into(),
                    dylib: value["target"]["kind"].as_array().is_some_and(|kinds| kinds.iter().any(|kind| kind == "dylib")),
                });
            }
        }
        if value["reason"] == "compiler-artifact"
            && value["manifest_path"].as_str().is_some_and(|path| {
                Path::new(path).canonicalize().ok().as_deref() == Some(manifest)
            })
            && value["target"]["kind"]
                .as_array()
                .is_some_and(|kinds| kinds.iter().any(|kind| kind == "bin"))
        {
            if let Some(executable) = value["executable"].as_str() {
                let path = PathBuf::from(executable);
                if artifacts.executables.len() < 2 && !artifacts.executables.contains(&path) {
                    artifacts.executables.push(path);
                }
            }
        }
        if let Some(message) = value["message"]["rendered"].as_str() {
            append_tail(&mut artifacts.diagnostics, message);
            log.write(message.as_bytes());
            log.write(b"\n");
        }
        if value["reason"] == "build-script-executed" {
            if let (Some(package), Some(out_dir)) = (value["package_id"].as_str(), value["out_dir"].as_str()) {
                for name in ["dlss_wgpu", "feathrise_frame_generation", "feathrise_fsr3_frame_generation"] {
                    if package.rsplit_once('#').is_some_and(|(_, package)| package.starts_with(&format!("{name}@"))) {
                        let record = (name.into(), PathBuf::from(out_dir));
                        if !artifacts.build_outputs.contains(&record) { artifacts.build_outputs.push(record); }
                    }
                }
            }
            for value in value["linked_paths"].as_array().into_iter().flatten() {
                if let Some(raw) = value.as_str() {
                    // Cargo 允许 kind=path；仅移除已知类别，不能截断目录名本身的等号。
                    let path = PathBuf::from(link_search_path(raw));
                    if path.is_absolute() && !artifacts.linked_paths.contains(&path) {
                        if artifacts.linked_paths.len() >= 256 { return Err("构建运行库搜索目录超过256项".into()); }
                        artifacts.linked_paths.push(path);
                    }
                }
            }
        }
    }
    log.finish();
    Ok(artifacts)
}

fn link_search_path(raw: &str) -> &str {
    match raw.split_once('=') {
        Some(("native" | "dependency" | "crate" | "framework" | "all", path)) => path,
        _ => raw,
    }
}

fn select_executable(mut paths: Vec<PathBuf>, target: &Path) -> Result<PathBuf, String> {
    if paths.len() != 1 {
        return Err(format!(
            "工程需恰有一个可执行入口，Cargo返回{}个可执行文件",
            paths.len()
        ));
    }
    let path = paths
        .pop()
        .expect("数量已经检查")
        .canonicalize()
        .map_err(|error| format!("工程产物不存在：{error}"))?;
    if !path.is_file() || !path.starts_with(target) {
        return Err("Cargo工程产物不属于本次构建缓存".into());
    }
    Ok(path)
}

fn read_log(mut file: impl Read) -> Result<String, String> {
    let mut tail = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let count = file
            .read(&mut chunk)
            .map_err(|error| format!("运行输出读取失败：{error}"))?;
        if count == 0 {
            break;
        }
        let excess = (tail.len() + count).saturating_sub(LOG_LIMIT);
        if excess > 0 {
            tail.drain(..excess);
        }
        tail.extend_from_slice(&chunk[..count]);
    }
    Ok(String::from_utf8_lossy(&tail).into_owned())
}

fn append_tail(tail: &mut String, text: &str) {
    tail.push_str(text);
    if tail.len() > LOG_LIMIT {
        let mut cut = tail.len() - LOG_LIMIT;
        while !tail.is_char_boundary(cut) {
            cut += 1;
        }
        tail.drain(..cut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn unconfirmed_cleanup_latches_build_slot_across_new_controls() {
        let slot = Mutex::new(false);
        let first = BuildControl::default();
        {
            let _guard = BuildSlotGuard { failed: slot.lock().unwrap(), control: &first };
            first.mark_cleanup_failed();
        }
        let next = BuildControl::default();
        let error = check_build_slot(*slot.lock().unwrap(), &next).unwrap_err();
        assert!(error.contains("先前构建进程树未确认收口"));
        assert!(next.cleanup_failed());
        // 纯自有局部槽验证永久门，不污染测试进程实际 Cargo 全局槽。
    }

    #[test]
    fn runtime_search_path_preserves_equals_inside_directory_names() {
        let directory = "D:/工程/runtime=preview/lib";
        assert_eq!(link_search_path(directory), directory);
        assert_eq!(link_search_path("native=D:/工程/runtime=preview/lib"), directory);
        assert_eq!(link_search_path("all=D:/工程/runtime=preview/lib"), directory);
    }

    #[test]
    fn cargo_game_selection_uses_exact_manifest_and_bin_kind() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("Cargo.toml")
            .canonicalize()
            .unwrap();
        let artifact = |manifest_path: &Path, kind: &str, executable: &str| {
            json!({
                "reason":"compiler-artifact", "manifest_path":manifest_path,
                "target":{"kind":[kind]}, "executable":executable,
            })
            .to_string()
        };
        let lines = [
            artifact(
                &manifest.with_file_name("Cargo.other.toml"),
                "bin",
                "wrong-project.exe",
            ),
            artifact(&manifest, "cdylib", "wrong-library.dll"),
            artifact(&manifest, "bin", "game-project-a.exe"),
        ]
        .join("\n");
        let output = read_artifacts(lines.as_bytes(), &manifest, None).unwrap();
        assert_eq!(
            output.executables,
            vec![PathBuf::from("game-project-a.exe")]
        );
    }

    #[test]
    fn cargo_output_is_bounded_and_multiple_game_entries_are_rejected() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("Cargo.toml")
            .canonicalize()
            .unwrap();
        let oversized = vec![b'x'; LINE_LIMIT + 1];
        assert!(read_artifacts(oversized.as_slice(), &manifest, None).is_err());
        assert!(
            select_executable(
                vec!["a.exe".into(), "b.exe".into()],
                manifest.parent().unwrap()
            )
            .is_err()
        );
        let mut tail = String::new();
        append_tail(&mut tail, &"中文诊断".repeat(LOG_LIMIT));
        assert!(tail.len() <= LOG_LIMIT);
        assert!(tail.is_char_boundary(0));
    }

    #[test]
    fn saved_game_contract_is_explicit_and_old_project_code_is_not_auto_upgraded() {
        assert!(
            validate_game_contract(br#"{"game_entry":{"kind":"saved-world","version":1}}"#).is_ok()
        );
        for old in [
            br#"{}"#.as_slice(),
            br#"{"game_entry":{"kind":"preview","version":1}}"#,
            br#"{"game_entry":{"kind":"saved-world","version":2}}"#,
        ] {
            assert!(
                validate_game_contract(old)
                    .unwrap_err()
                    .contains("不会覆写工程源码")
            );
        }
    }
}
