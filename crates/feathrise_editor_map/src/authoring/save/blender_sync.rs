//! 可选Blender工具把动态地形GLB暂存文件转换为同名Blend镜像。
//!
//! 【功能设计】本模块只在既有IoTaskPool保存任务内启动一个隐藏Blender子进程；输入与输出
//! 都使用事务私有临时路径。脚本必须通过项目唯一save_post插件完成一次隔离GLB导出证明，
//! 然后事务才会把GLB与Blend共同原子发布，任何失败都不会覆盖正式文件。

use std::ffi::OsString; // 构造不经字符串转义的同目录唯一临时文件名。
use std::fs::{File, OpenOptions}; // GLB硬链接失败时受检复制，并为子进程创建独占日志文件。
use std::io::{Read, Seek, SeekFrom}; // 只读取Blender日志尾部固定字节，避免异常输出放大内存。
use std::path::{Path, PathBuf}; // 约束GLB、Blend、工具脚本和Blender可执行文件路径。
use std::process::{Command, ExitStatus, Stdio}; // 以参数数组启动隐藏后台Blender，不经过Shell拼接。
use std::time::{Duration, Instant}; // 子进程轮询与固定真实时间超时。

const BLEND_GENERATOR_SCRIPT: &str = "从GLB生成同名Blend.py"; // 唯一GLB到Blend后台入口。
const AUTO_EXPORT_PLUGIN_SCRIPT: &str = "Blender保存时自动导出GLB.py"; // 所有Blender生成器必须复用的save_post实现。
const BLENDER_TOOLS_ENVIRONMENT: &str = "FEATHRISE_BLENDER_TOOLS_DIR"; // 脚本由使用者显式提供，不依赖私有主仓或用户工程。
const BLENDER_PATH_ENVIRONMENT: &str = "BEVY_LEARN5_BLENDER_PATH"; // 可选显式Blender可执行文件覆盖。
const BLENDER_PROCESS_TIMEOUT: Duration = Duration::from_secs(600); // 超大合法地形最多占用一个保存槽十分钟，之后终止并保持旧批次。
const BLENDER_POLL_INTERVAL: Duration = Duration::from_millis(50); // 只阻塞后台IO worker，不在Bevy主线程忙等。
const MAX_BLENDER_LOG_BYTES: u64 = 16 * 1024; // 每个stdout/stderr诊断最多保留尾部16KiB。
const MAX_PATH_DIRECTORIES: usize = 256; // PATH候选扫描具有明确数量上限。
const MAX_BLENDER_INSTALLATIONS: usize = 64; // Blender Foundation目录候选同样有固定上限。
const SCRIPT_SUCCESS_MARKER: &str = "[F12 BLEND 同步] 已生成"; // 防止Blender零退出码掩盖Python脚本异常。
const PLUGIN_SUCCESS_MARKER: &str = "[Bevy GLB 自动导出] 已更新"; // 证明统一save_post插件确实导出验证GLB。

/// 一次保存共用同一组受检工具路径；判断和查找只发生在后台保存任务中。
#[derive(Clone)]
pub(super) struct BlenderSync {
    executable: PathBuf,
    generator_script: PathBuf,
    auto_export_plugin: PathBuf,
}

/// 工具目录、脚本或Blender不可用时不计划Blend写项，地形GLB仍可独立保存。
pub(super) fn optional_blender_sync() -> Option<BlenderSync> {
    let directory = std::env::var_os(BLENDER_TOOLS_ENVIRONMENT);
    resolve_blender_sync(directory.as_deref().map(Path::new), find_blender_executable)
}

/// 显式输入与工具发现分开，脚本缺席时连Blender扫描也不执行。
pub(super) fn resolve_blender_sync(
    directory: Option<&Path>,
    find_blender: impl FnOnce() -> Result<PathBuf, String>,
) -> Option<BlenderSync> {
    let (generator_script, auto_export_plugin) = resolve_tool_scripts(directory)?;
    let executable = find_blender().ok()?;
    Some(BlenderSync { executable, generator_script, auto_export_plugin })
}

/// 从正式或暂存terrain.glb路径确定同目录、同文件名的terrain.blend目标。
pub(super) fn blend_path_for_glb(glb_path: &Path) -> Result<PathBuf, String> {
    let extension = glb_path
        .extension() // 读取实际末级扩展名，不接受名称中间出现“.glb”。
        .and_then(|value| value.to_str()) // 地图协议路径必须能形成Unicode诊断。
        .ok_or_else(|| format!("动态地形GLB路径缺少有效扩展名：{}", glb_path.display()))?; // 无扩展名不能安全推导覆盖目标。
    if !extension.eq_ignore_ascii_case("glb") {
        return Err(format!(
            "动态地形路径必须以.glb结尾才能生成Blend：{}",
            glb_path.display()
        )); // 禁止把任意地图文件直接改扩展名后覆盖。
    }
    Ok(glb_path.with_extension("blend")) // 同目录同stem符合用户要求的直接覆盖位置。
}

/// 从已经完整stage的GLB生成Blend stage；调用方随后把两者纳入同一回滚事务。
pub(super) fn generate_blend_from_staged_glb(
    blender: &BlenderSync,
    source_glb_stage: &Path,   // 上一PreparedWrite已同步完成但尚未正式发布的GLB。
    target_blend_stage: &Path, // 当前PreparedWrite唯一拥有且尚不存在的Blend stage。
) -> Result<(), String> {
    ensure_nonempty_regular_file(source_glb_stage, "动态地形GLB暂存输入")?; // Blender启动前先拒绝空文件、目录或符号链接。
    ensure_path_absent(target_blend_stage, "Blend事务暂存输出")?; // 外部进程不能覆盖未知内部文件。
    let temporary = BlenderTemporaryPaths::new(target_blend_stage)?; // 所有子进程副产物绑定本次事务唯一stage名。
    snapshot_glb(source_glb_stage, &temporary.input_glb)?; // 保留.glb扩展名供Blender导入器识别，同时避免读取正式目标。
    let report = run_blender(
        &blender.executable,        // 使用本次保存开始时选定的本机可执行文件。
        &blender.generator_script,  // 显式工具目录中的GLB到Blend脚本。
        &blender.auto_export_plugin, // 同一工具目录中的save_post导出插件。
        &temporary.input_glb,       // 当前事务GLB不可变快照。
        &temporary.generated_blend, // Blender只写独占临时Blend。
        &temporary.scratch_glb,     // 插件只写隔离验证GLB。
        &temporary.stdout_log,      // stdout写盘避免管道填满阻塞子进程。
        &temporary.stderr_log,      // stderr独立保留异常栈。
    )?; // 超时、非零状态或启动失败都由同一诊断返回。
    ensure_nonempty_regular_file(&temporary.generated_blend, "Blender生成结果")?; // 不接受空Blend或目录替身。
    ensure_nonempty_regular_file(&temporary.scratch_glb, "save_post验证GLB")?; // 插件导出失败即使Blender保存成功也整批失败。
    if !report.stdout.contains(SCRIPT_SUCCESS_MARKER)
        || !report.stdout.contains(PLUGIN_SUCCESS_MARKER)
    {
        return Err(format!(
            "Blender已退出但缺少地形同步脚本或 save_post 的成功记录{}",
            report.diagnostic_suffix()
        )); // 输出文件存在也不能绕过项目统一插件契约。
    }
    std::fs::rename(&temporary.generated_blend, target_blend_stage).map_err(|error| {
        format!(
            "把Blender完成文件移入事务stage {}失败：{error}",
            target_blend_stage.display()
        )
    })?; // 同目录移动不会暴露正式路径，最终原子替换仍由Rust事务执行。
    ensure_nonempty_regular_file(target_blend_stage, "Blend事务暂存结果")?; // 交给事务前再次证明移动完成值。
    Ok(()) // Drop会清理输入快照、验证GLB和受限日志，Blend stage归事务所有。
}

/// 只检查显式绝对目录；缺少任一脚本就跳过，不向编辑器根、父仓或用户工程回退查找。
fn resolve_tool_scripts(directory: Option<&Path>) -> Option<(PathBuf, PathBuf)> {
    let directory = directory.filter(|directory| directory.is_absolute())?.canonicalize().ok()?;
    let generator = directory.join(BLEND_GENERATOR_SCRIPT);
    let plugin = directory.join(AUTO_EXPORT_PLUGIN_SCRIPT);
    (is_nonempty_regular_file(&generator) && is_nonempty_regular_file(&plugin))
        .then_some((generator, plugin))
}

/// 找到显式配置、PATH或Windows标准安装目录中的Blender可执行文件。
pub(crate) fn find_blender_executable() -> Result<PathBuf, String> {
    if let Some(configured) = std::env::var_os(BLENDER_PATH_ENVIRONMENT) {
        if configured.is_empty() {
            return Err(format!(
                "环境变量{BLENDER_PATH_ENVIRONMENT}为空；请删除它或填写blender.exe绝对路径"
            )); // 显式配置错误不得静默换成另一版本。
        }
        let path = PathBuf::from(configured); // OsString保留Windows非ASCII安装路径。
        ensure_nonempty_regular_file(&path, "显式配置的Blender可执行文件")?; // 用户指定值具有最高优先级。
        return Ok(path); // 不再扫描PATH或其他安装版本。
    }
    let executable_name = if cfg!(windows) {
        "blender.exe" // Windows安装文件名。
    } else {
        "blender" // Linux与macOS命令行文件名。
    };
    if let Some(path_value) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path_value).take(MAX_PATH_DIRECTORIES) {
            let candidate = directory.join(executable_name); // PATH顺序就是用户明确选择的版本优先级。
            if is_nonempty_regular_file(&candidate) {
                return Ok(candidate); // 找到首个可执行普通文件后停止有界扫描。
            }
        }
    }
    #[cfg(windows)]
    if let Some(candidate) = newest_windows_blender_installation() {
        return Ok(candidate); // PATH缺失时采用Program Files下版本号最高的安装。
    }
    Err(format!(
        "未找到Blender；请安装Blender，或设置环境变量{BLENDER_PATH_ENVIRONMENT}指向Blender可执行文件"
    )) // F12保留dirty和旧正式文件，作者配置完成后可直接重试。
}

/// 在Windows标准Blender Foundation根下选择版本号最高的受检blender.exe。
#[cfg(windows)]
fn newest_windows_blender_installation() -> Option<PathBuf> {
    let mut roots = Vec::new(); // ProgramFiles与ProgramW6432可能指向同一目录，候选最终按路径去重。
    for variable in ["ProgramFiles", "ProgramW6432"] {
        if let Some(value) = std::env::var_os(variable) {
            let root = PathBuf::from(value).join("Blender Foundation"); // 只扫描Blender官方安装根，不遍历整个Program Files。
            if !roots.contains(&root) {
                roots.push(root); // 最多两个固定环境来源。
            }
        }
    }
    let mut candidates = Vec::new(); // 每个官方根最多读取固定六十四个直接子目录。
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue; // 未安装到某一标准根属于正常候选缺席。
        };
        for entry in entries.take(MAX_BLENDER_INSTALLATIONS).flatten() {
            let candidate = entry.path().join("blender.exe"); // 官方目录结构固定为Blender x.y/blender.exe。
            if is_nonempty_regular_file(&candidate) {
                candidates.push(candidate); // 版本选择在完整受界候选集合上确定执行。
            }
        }
    }
    candidates.sort_by_key(|path| blender_version_rank(path)); // 稳定选择数值版本而非字符串字典序。
    candidates.pop() // 无候选返回None，由上层产生统一安装提示。
}

/// 从“Blender 主.次[.补丁]”父目录名提取可排序版本；未知格式排在已识别版本之前。
fn blender_version_rank(path: &Path) -> (u32, u32, u32, String) {
    let name = path
        .parent() // blender.exe的直接父目录包含官方版本名。
        .and_then(Path::file_name) // 根路径异常时退回空名称。
        .and_then(|value| value.to_str()) // 非Unicode名称仍可作为最低版本候选。
        .unwrap_or_default();
    let version = name.strip_prefix("Blender ").unwrap_or_default(); // 只解析官方稳定前缀。
    let mut parts = version
        .split('.')
        .map(|part| part.parse::<u32>().unwrap_or(0)); // 非数值段按零处理但保留路径稳定排序。
    (
        parts.next().unwrap_or(0),           // 主版本。
        parts.next().unwrap_or(0),           // 次版本。
        parts.next().unwrap_or(0),           // 补丁版本。
        path.to_string_lossy().into_owned(), // 同版本安装使用完整路径作确定性末级排序。
    )
}

/// 本次Blender进程的全部独占副产物；离开作用域时只删除已验证不存在后使用的精确路径。
struct BlenderTemporaryPaths {
    input_glb: PathBuf,       // GLB stage的同目录硬链接或受检副本，保留.glb扩展名。
    generated_blend: PathBuf, // Blender真正写入的.blend，成功后移动为事务stage。
    scratch_glb: PathBuf,     // save_post插件导出的隔离证明。
    stdout_log: PathBuf,      // 避免管道阻塞的stdout临时文件。
    stderr_log: PathBuf,      // Python异常和Blender错误临时文件。
}

impl BlenderTemporaryPaths {
    /// 从事务唯一Blend stage派生五个同目录唯一子文件，并在创建前拒绝任何既有路径。
    fn new(blend_stage: &Path) -> Result<Self, String> {
        let paths = Self {
            input_glb: sibling_with_suffix(blend_stage, ".blender-input.glb")?, // 导入器格式身份。
            generated_blend: sibling_with_suffix(blend_stage, ".blender-output.blend")?, // 主文件保存身份。
            scratch_glb: sibling_with_suffix(blend_stage, ".save-post-check.glb")?, // 插件验证身份。
            stdout_log: sibling_with_suffix(blend_stage, ".blender-stdout.log")?, // 标准输出身份。
            stderr_log: sibling_with_suffix(blend_stage, ".blender-stderr.log")?, // 标准错误身份。
        };
        for path in paths.all() {
            ensure_path_absent(path, "Blender事务临时文件")?; // 崩溃遗留或外部同名文件不能被本次任务删除覆盖。
        }
        Ok(paths) // 后续所有创建都属于本作用域并由Drop精确清理。
    }

    /// 返回固定五个内部路径，供统一预检与Drop清理。
    fn all(&self) -> [&Path; 5] {
        [
            &self.input_glb,       // GLB输入快照。
            &self.generated_blend, // 尚未移交事务的Blend结果。
            &self.scratch_glb,     // save_post验证输出。
            &self.stdout_log,      // stdout日志。
            &self.stderr_log,      // stderr日志。
        ]
    }
}

impl Drop for BlenderTemporaryPaths {
    fn drop(&mut self) {
        for path in self.all() {
            let _ = std::fs::remove_file(path); // 只清理从唯一事务stage派生且预先证明缺席的精确普通文件名。
        }
    }
}

/// 在同一目录保留完整原文件名并追加用途后缀，避免字符串Shell转义和跨目录移动。
fn sibling_with_suffix(path: &Path, suffix: &str) -> Result<PathBuf, String> {
    let parent = path
        .parent() // 所有子文件必须与事务stage同目录。
        .ok_or_else(|| format!("Blend事务stage缺少父目录：{}", path.display()))?;
    let file_name = path
        .file_name() // 事务stage已经具有进程号和票据唯一名。
        .ok_or_else(|| format!("Blend事务stage缺少文件名：{}", path.display()))?;
    let mut temporary_name = OsString::from(file_name); // 保留非ASCII文件名的原生平台编码。
    temporary_name.push(suffix); // 后缀最终提供Blender要求的.glb或.blend扩展名。
    Ok(parent.join(temporary_name)) // 不解析用户字符串，也不会越过已受检目标目录。
}

/// 优先用同卷硬链接冻结GLB stage，文件系统不支持时再执行流式受检复制。
fn snapshot_glb(source: &Path, destination: &Path) -> Result<(), String> {
    if std::fs::hard_link(source, destination).is_ok() {
        return Ok(()); // 不复制大型地形且正式替换前源stage不会被修改。
    }
    let mut source_file = File::open(source)
        .map_err(|error| format!("打开GLB暂存输入{}失败：{error}", source.display()))?; // 回退只读源文件。
    let mut destination_file = OpenOptions::new()
        .write(true) // 只创建本次输入快照。
        .create_new(true) // 绝不覆盖崩溃遗留或外部路径。
        .open(destination)
        .map_err(|error| {
            format!(
                "创建Blender GLB输入快照{}失败：{error}",
                destination.display()
            )
        })?;
    let copy_result = std::io::copy(&mut source_file, &mut destination_file)
        .and_then(|_| destination_file.sync_all()); // 完整复制并刷新后才允许外部进程读取。
    drop(destination_file); // Windows子进程启动前释放写句柄。
    if let Err(error) = copy_result {
        let _ = std::fs::remove_file(destination); // 失败部分文件属于本调用，不能留给后续重试。
        return Err(format!(
            "复制Blender GLB输入快照{}失败：{error}",
            destination.display()
        ));
    }
    Ok(()) // 完整输入快照已经落盘。
}

/// 子进程完成状态和受限日志；成功文件仍由调用者执行独立校验。
struct BlenderProcessReport {
    status: ExitStatus, // 非零状态始终使保存事务失败。
    stdout: String,     // 只保留尾部固定字节用于成功标记和诊断。
    stderr: String,     // 只保留尾部固定字节用于异常栈。
}

impl BlenderProcessReport {
    /// 把非空日志压缩为一段用户可见后缀，避免空分隔符和无限输出。
    fn diagnostic_suffix(&self) -> String {
        let mut parts = Vec::new(); // stdout与stderr各最多16KiB，固定两项。
        if !self.stdout.trim().is_empty() {
            parts.push(format!("stdout：{}", self.stdout.trim())); // 保留插件和脚本成功/失败上下文。
        }
        if !self.stderr.trim().is_empty() {
            parts.push(format!("stderr：{}", self.stderr.trim())); // Python栈通常位于stderr。
        }
        if parts.is_empty() {
            String::new() // 没有日志时不增加多余标点。
        } else {
            format!("；{}", parts.join("；")) // 两段固定上限诊断合并返回F12状态。
        }
    }
}

/// 启动隐藏Blender并在后台IO线程以固定间隔等待，十分钟后强制终止唯一子进程。
#[allow(clippy::too_many_arguments)]
fn run_blender(
    blender: &Path,            // 受检Blender可执行文件。
    generator_script: &Path,   // GLB到Blend脚本。
    auto_export_plugin: &Path, // save_post插件。
    input_glb: &Path,          // 事务输入快照。
    output_blend: &Path,       // Blender独占输出。
    scratch_glb: &Path,        // save_post隔离输出。
    stdout_log: &Path,         // stdout文件。
    stderr_log: &Path,         // stderr文件。
) -> Result<BlenderProcessReport, String> {
    let stdout_file = OpenOptions::new()
        .write(true) // 子进程独占写入stdout。
        .create_new(true) // 事务临时路径预检后仍禁止竞态覆盖。
        .open(stdout_log)
        .map_err(|error| {
            format!(
                "创建Blender stdout日志{}失败：{error}",
                stdout_log.display()
            )
        })?;
    let stderr_file = OpenOptions::new()
        .write(true) // 子进程独占写入stderr。
        .create_new(true) // 事务临时路径预检后仍禁止竞态覆盖。
        .open(stderr_log)
        .map_err(|error| {
            format!(
                "创建Blender stderr日志{}失败：{error}",
                stderr_log.display()
            )
        })?;
    let mut command = Command::new(blender); // 参数逐项传递，不经过cmd.exe或PowerShell。
    command
        .arg("--background") // 不创建可见Blender窗口。
        .arg("--factory-startup") // 不读取用户启动文件或已安装场景。
        .arg("--disable-autoexec") // 禁止任何资产自动执行脚本。
        .arg("--python") // 由下一参数指定项目生成脚本。
        .arg(generator_script) // 使用真实Path保留中文工具名。
        .arg("--") // 后续参数只交给项目脚本解析。
        .arg("--input-glb") // 输入键。
        .arg(input_glb) // 事务GLB快照。
        .arg("--output-blend") // 输出键。
        .arg(output_blend) // 临时Blend结果。
        .arg("--auto-export-plugin") // 插件键。
        .arg(auto_export_plugin) // 项目唯一save_post源码。
        .arg("--scratch-glb") // 验证输出键。
        .arg(scratch_glb) // 隔离插件GLB。
        .stdin(Stdio::null()) // 后台任务绝不等待交互输入。
        .stdout(Stdio::from(stdout_file)) // 大日志直接写盘避免pipe死锁。
        .stderr(Stdio::from(stderr_file)); // 异常栈同样直接写盘。
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt; // Windows为后台Blender抑制控制台窗口。
        const CREATE_NO_WINDOW: u32 = 0x0800_0000; // Win32标准CREATE_NO_WINDOW标志。
        command.creation_flags(CREATE_NO_WINDOW); // F12保存不得闪出额外终端窗口。
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("启动Blender后台转换{}失败：{error}", blender.display()))?; // 进程未启动时正式文件完全未变。
    let started = Instant::now(); // 使用真实单调时间限制唯一外部转换任务。
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("轮询Blender后台转换失败：{error}"))?
        {
            break status; // 子进程已完成，不执行阻塞wait。
        }
        if started.elapsed() >= BLENDER_PROCESS_TIMEOUT {
            let kill_error = child.kill().err(); // 超时后只终止本任务精确持有的子进程。
            let wait_error = child.wait().err(); // 回收系统进程句柄，避免僵尸进程。
            return Err(format!(
                "Blender生成Blend超过{}秒并已终止{}{}",
                BLENDER_PROCESS_TIMEOUT.as_secs(),
                kill_error.map_or_else(String::new, |error| format!("；终止失败：{error}")),
                wait_error.map_or_else(String::new, |error| format!("；回收失败：{error}")),
            )); // 事务Drop清理所有部分输出，正式GLB和Blend继续保持旧版本。
        }
        std::thread::sleep(BLENDER_POLL_INTERVAL); // 只占用当前IoTaskPool worker，不影响Bevy帧循环。
    };
    let stdout = read_log_tail(stdout_log)?; // 子进程退出后文件句柄已释放，可安全读取固定尾部。
    let stderr = read_log_tail(stderr_log)?; // Python异常栈同样只取固定尾部。
    let report = BlenderProcessReport {
        status, // 保存真实退出码供失败诊断。
        stdout, // 保存受限stdout。
        stderr, // 保存受限stderr。
    };
    if !report.status.success() {
        return Err(format!(
            "Blender生成Blend失败，退出状态{}{}",
            report.status,
            report.diagnostic_suffix()
        )); // 非零退出时不相信任何部分输出文件。
    }
    Ok(report) // 调用者继续核对文件和两类成功标记。
}

/// 读取日志尾部固定字节；超大Blender输出不会随地形规模进入保存结果内存。
fn read_log_tail(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("读取Blender日志{}失败：{error}", path.display()))?; // 子进程已经退出，日志必须存在。
    let length = file
        .metadata()
        .map_err(|error| format!("读取Blender日志元数据{}失败：{error}", path.display()))?
        .len(); // 只用文件长度确定尾部偏移。
    let start = length.saturating_sub(MAX_BLENDER_LOG_BYTES); // 小日志从零读取，大日志只取末尾。
    file.seek(SeekFrom::Start(start))
        .map_err(|error| format!("定位Blender日志{}失败：{error}", path.display()))?; // 不扫描被截断前缀。
    let mut bytes = Vec::with_capacity((length - start) as usize); // 最大容量固定为16KiB。
    file.read_to_end(&mut bytes)
        .map_err(|error| format!("读取Blender日志尾部{}失败：{error}", path.display()))?; // 文件已完成，不需要增量轮询。
    Ok(String::from_utf8_lossy(&bytes).into_owned()) // Windows控制台编码异常使用替代字符但保留ASCII成功标记。
}

/// 路径必须尚不存在；目录、符号链接和普通文件都视为冲突而不删除。
fn ensure_path_absent(path: &Path, label: &str) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()), // 唯一允许的初始状态。
        Ok(_) => Err(format!("{label}已存在，拒绝覆盖：{}", path.display())), // 不接管未知遗留文件。
        Err(error) => Err(format!("检查{label}{}失败：{error}", path.display())), // 权限或IO错误必须显式停止。
    }
}

/// 校验真实普通非空文件，禁止符号链接把Blender输入或脚本引到作者边界外。
fn ensure_nonempty_regular_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|error| format!("读取{label}{}失败：{error}", path.display()))?; // 保留原始路径诊断。
    if !metadata.file_type().is_file() || metadata.len() == 0 {
        return Err(format!("{label}必须是非空普通文件：{}", path.display())); // 目录、链接和零字节文件一律拒绝。
    }
    Ok(()) // 文件可作为受检输入或完成结果。
}

/// 无错误分配版本仅供候选扫描，正式采用仍由上方详细校验产生诊断。
fn is_nonempty_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.file_type().is_file() && metadata.len() > 0) // 单次元数据读取且不跟随符号链接。
}

#[cfg(test)]
mod tests {
    use super::*; // 路径推导和版本排序保持纯函数回归。

    /// 本机Blender集成测试只回收本次创建的两个精确路径，不读取原游戏地图。
    struct GeneratedBlendTestFile {
        source: PathBuf,
        output: PathBuf, // 不使用递归目录删除，避免扩大测试清理范围。
    }

    impl Drop for GeneratedBlendTestFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.source);
            let _ = std::fs::remove_file(&self.output); // 只删除本次唯一的输入与输出。
        }
    }

    #[test]
    fn terrain_glb_maps_to_same_name_blend() {
        let glb = Path::new("assets/maps/MAP-test/terrain/terrain.glb"); // 使用正式动态地图目录形状。
        assert_eq!(
            blend_path_for_glb(glb).expect("合法GLB应能推导Blend"), // 执行生产路径推导。
            Path::new("assets/maps/MAP-test/terrain/terrain.blend")  // 只替换末级扩展名。
        );
        assert!(blend_path_for_glb(Path::new("terrain.ktx2")).is_err()); // 非GLB不能覆盖同名Blend。
    }

    #[test]
    fn numeric_blender_versions_sort_newest_first_candidate_last() {
        let older = Path::new("C:/Program Files/Blender Foundation/Blender 4.3/blender.exe"); // 旧稳定版本夹具。
        let newer = Path::new("C:/Program Files/Blender Foundation/Blender 5.1/blender.exe"); // 当前较新版本夹具。
        assert!(blender_version_rank(newer) > blender_version_rank(older)); // 数值主版本不得按路径字符串误排序。
    }

    #[test]
    fn absent_or_incomplete_scripts_skip_blender_discovery() {
        let temporary = super::super::tests::SaveTransactionTestDirectory::new();
        for directory in [None, Some(Path::new("relative")), Some(temporary.path.as_path())] {
            assert!(resolve_blender_sync(directory, || panic!("未配置完整脚本时不能查找或启动Blender")).is_none());
        }
        let generator = temporary.path.join(BLEND_GENERATOR_SCRIPT);
        let plugin = temporary.path.join(AUTO_EXPORT_PLUGIN_SCRIPT);
        std::fs::write(&generator, b"# generator fixture").unwrap();
        assert!(resolve_blender_sync(Some(&temporary.path), || panic!("缺插件时必须跳过")).is_none());
        std::fs::write(&plugin, []).unwrap();
        assert!(resolve_blender_sync(Some(&temporary.path), || panic!("空插件也不可用")).is_none());
        std::fs::remove_file(&plugin).unwrap();
        std::fs::create_dir(&plugin).unwrap();
        assert!(resolve_blender_sync(Some(&temporary.path), || panic!("同名目录不能当作插件")).is_none());
    }

    #[test]
    fn complete_scripts_require_an_available_blender_and_freeze_the_selected_paths() {
        let temporary = super::super::tests::SaveTransactionTestDirectory::new();
        for name in [BLEND_GENERATOR_SCRIPT, AUTO_EXPORT_PLUGIN_SCRIPT] {
            std::fs::write(temporary.path.join(name), b"# optional tool fixture").unwrap();
        }
        assert!(resolve_blender_sync(Some(&temporary.path), || Err("未安装Blender".into())).is_none());
        let executable = temporary.path.join("blender.exe");
        let selected = resolve_blender_sync(Some(&temporary.path), || Ok(executable.clone())).unwrap();
        let directory = temporary.path.canonicalize().unwrap();
        assert_eq!(selected.executable, executable);
        assert_eq!(selected.generator_script, directory.join(BLEND_GENERATOR_SCRIPT));
        assert_eq!(selected.auto_export_plugin, directory.join(AUTO_EXPORT_PLUGIN_SCRIPT));
    }

    #[test]
    #[ignore = "需要本机安装Blender，只在工具链验收时显式运行"]
    fn installed_blender_converts_real_dynamic_terrain_stage() {
        use std::io::Write;
        use feathrise_map_terrain::{TerrainSampleSpec, build_flat_terrain_glb};

        // 用正式地形编码器生成完整中性GLB，独立检出无需原地图或另行下载素材。
        let bytes = build_flat_terrain_glb(TerrainSampleSpec {
            resolution: [4, 4], world_min: [-2.0, -2.0], world_max: [2.0, 2.0],
            base_height: 0.0, height_scale: 4.0,
        }, 0.0).expect("程序地形应可编码");
        let unique = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
            .unwrap().as_nanos();
        let source = std::env::temp_dir().join(format!(
            "feathrise-blend-sync-{}-{unique}.glb", std::process::id()
        ));
        let output = source.with_extension("blend");
        ensure_path_absent(&output, "测试Blend输出").unwrap();
        let mut input = OpenOptions::new().write(true).create_new(true).open(&source).unwrap();
        let cleanup = GeneratedBlendTestFile { source: source.clone(), output: output.clone() };
        input.write_all(&bytes).unwrap();
        input.sync_all().unwrap();
        drop(input); // Windows子进程启动前关闭输入写句柄。

        let blender = optional_blender_sync().expect("集成测试须显式配置完整Blender工具目录并安装Blender");
        generate_blend_from_staged_glb(&blender, &source, &output)
            .expect("已安装Blender应完成真实GLB到Blend事务stage转换"); // 覆盖可执行文件发现、参数、超时、双成功标记与临时清理。
        ensure_nonempty_regular_file(&output, "本机Blender集成测试结果")
            .expect("转换结果必须是非空普通Blend文件"); // 与正式事务采用门使用同一文件条件。
        drop(cleanup); // 测试成功后立即删除唯一临时Blend，不等进程退出。
    }
}
