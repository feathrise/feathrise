//! Windows 专属子进程边界：工程运行与构建工具均挂起创建、显式句柄白名单、入独占Job后恢复。
//!
//! 控制协议使用独立双管道，标准流保留普通日志；兼容入口仍提供三标准流。
//! 此模块不按进程名或 PID 扫描、结束其他程序。调用者在后台排空 stdout/stderr，
//! 停止后轮询 Job 计数归零；关闭句柄只是最后一道回收保障，不能冒充已确认清理。

use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::File,
    mem::size_of,
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    process::Command,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{HANDLE, HANDLE_FLAG_INHERIT, SetHandleInformation, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Security::SECURITY_ATTRIBUTES,
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
            QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        },
        Pipes::CreatePipe,
        Threading::{
            CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
            EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, InitializeProcThreadAttributeList,
            LPPROC_THREAD_ATTRIBUTE_LIST, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROCESS_INFORMATION,
            ResumeThread, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
            UpdateProcThreadAttribute, WaitForSingleObject,
        },
    },
};

/// 只持有本次启动创建的句柄；Job 不可继承，最后一个宿主句柄关闭会回收整个子树。
pub struct JobProcess {
    // 字段按声明顺序析构，先关 Job 发出兜底回收，再关闭根进程句柄；Drop 不阻塞 UI。
    job: OwnedHandle,
    process: OwnedHandle,
    pid: u32,
}

struct SpawnedProcess {
    process: JobProcess,
    input: File,
    output: File,
    errors: File,
    control: Option<(File, File)>,
}

/// 启动前的一般错误可以重试；只有已经创建进程却未确认收口时才必须挡住后续工作。
#[derive(Debug)]
pub(crate) struct SpawnFailure {
    pub(crate) message: String,
    pub(crate) cleanup_unconfirmed: bool,
}

impl From<String> for SpawnFailure {
    fn from(message: String) -> Self { Self { message, cleanup_unconfirmed: false } }
}

impl From<&str> for SpawnFailure {
    fn from(message: &str) -> Self { message.to_string().into() }
}

impl JobProcess {
    /// 仅在后台调用。返回的三个 File 依次为父端 stdin、stdout、stderr。
    #[cfg(test)] // 生产入口使用带明确工作目录及环境的 Command；此便捷入口只供进程回归夹具。
    pub fn spawn(exe: &Path, args: &[OsString]) -> Result<(Self, File, File, File), String> {
        let spawned = Self::spawn_inner(exe, args, false, None).map_err(|failure| failure.message)?;
        Ok((
            spawned.process,
            spawned.input,
            spawned.output,
            spawned.errors,
        ))
    }

    /// 后台构建工具沿用同一挂起创建／Job边界，并显式传递工作目录和环境覆盖。
    /// 只接受原生EXE，不经shell；调用者不得使用Command::env_clear（本入口保留宿主环境）。
    pub(crate) fn spawn_command(command: &Command) -> Result<(Self, File, File, File), SpawnFailure> {
        let executable = command_executable(command)?;
        let args: Vec<_> = command.get_args().map(OsStr::to_owned).collect();
        let spawned = Self::spawn_inner(&executable, &args, false, Some(command))?;
        Ok((spawned.process, spawned.input, spawned.output, spawned.errors))
    }

    /// 专用控制管道与标准流互不混用；返回父控制写、父控制读、stdout、stderr。
    /// 子端句柄通过保留的两个 CLI 参数传递，并与三标准流共同加入继承白名单。
    #[cfg(test)]
    pub fn spawn_runtime(
        exe: &Path,
        args: &[OsString],
    ) -> Result<(Self, File, File, File, File), String> {
        if args
            .iter()
            .any(|argument| argument == "--ipc-read" || argument == "--ipc-write")
        {
            return Err("专用管道句柄参数只能由宿主启动边界生成".into());
        }
        let spawned = Self::spawn_inner(exe, args, true, None).map_err(|failure| failure.message)?;
        let (control_write, control_read) = spawned
            .control
            .expect("spawn_inner 专用通道请求必须返回两个父端");
        // 游戏不从 stdin 接收控制或输入。关闭唯一父写端，让正常 stdin 明确读到 EOF。
        drop(spawned.input);
        Ok((
            spawned.process,
            control_write,
            control_read,
            spawned.output,
            spawned.errors,
        ))
    }

    /// 与普通工具共用受控创建边界，并给真实游戏EXE提供明确cwd、DLL环境及独立协议管道。
    pub(crate) fn spawn_runtime_command(command: &Command) -> Result<(Self, File, File, File, File), SpawnFailure> {
        let executable = command_executable(command)?;
        let args: Vec<_> = command.get_args().map(OsStr::to_owned).collect();
        if args.iter().any(|arg| arg == "--ipc-read" || arg == "--ipc-write") {
            return Err("协议管道句柄只能由受控进程边界生成".into());
        }
        let spawned = Self::spawn_inner(&executable, &args, true, Some(command))?;
        let (write, read) = spawned.control.expect("专用管道已经请求");
        drop(spawned.input);
        Ok((spawned.process, write, read, spawned.output, spawned.errors))
    }

    fn spawn_inner(
        exe: &Path,
        args: &[OsString],
        dedicated_control: bool,
        command_options: Option<&Command>,
    ) -> Result<SpawnedProcess, SpawnFailure> {
        if !exe.is_absolute() {
            return Err("子进程启动必须提供可执行文件的绝对路径".into());
        }
        let executable = exe
            .canonicalize()
            .map_err(|error| format!("子进程可执行文件不存在或不可访问：{error}"))?;
        if !executable.is_file() {
            return Err("子进程可执行文件必须是普通文件".into());
        }
        let executable = process_image_path(&executable);
        let application = nul_terminated(executable.as_os_str())?;
        let working_directory = command_options.and_then(Command::get_current_dir)
            .map(|directory| directory.canonicalize().map_err(|e| format!("工具工作目录无效：{e}")))
            .transpose()?.map(|path| nul_terminated(process_image_path(&path).as_os_str())).transpose()?;
        let environment = command_options.map(command_environment).transpose()?;

        let job = unsafe { CreateJobObjectW(null(), null()) };
        if job.is_null() {
            return Err(win_error("CreateJobObjectW").into());
        }
        // SAFETY：创建成功后立即取得唯一所有权，所有错误分支均由 RAII 归还句柄。
        let job = unsafe { OwnedHandle::from_raw_handle(job) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(win_error("SetInformationJobObject").into());
        }

        let (child_input, parent_input) = pipe(false)?;
        let (child_output, parent_output) = pipe(true)?;
        let (child_error, parent_error) = pipe(true)?;
        let control = if dedicated_control {
            Some((pipe(false)?, pipe(true)?))
        } else {
            None
        };
        let mut inherited = vec![
            child_input.as_raw_handle(),
            child_output.as_raw_handle(),
            child_error.as_raw_handle(),
        ];
        let mut child_args = args.to_vec();
        if let Some(((control_read, _), (control_write, _))) = &control {
            // 只传本次刚创建的子端；父端已清除继承位，不让其他进程维持控制管道存活。
            inherited.extend([control_read.as_raw_handle(), control_write.as_raw_handle()]);
            child_args.extend([
                "--ipc-read".into(),
                (control_read.as_raw_handle() as usize).to_string().into(),
                "--ipc-write".into(),
                (control_write.as_raw_handle() as usize).to_string().into(),
            ]);
        }
        let mut command = command_line(executable.as_os_str(), &child_args)?;
        let mut attributes = AttributeList::new()?;
        if unsafe {
            UpdateProcThreadAttribute(
                attributes.pointer(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                inherited.as_ptr().cast(),
                inherited.len() * size_of::<HANDLE>(),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(win_error("UpdateProcThreadAttribute(HANDLE_LIST)").into());
        }
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = inherited[0];
        startup.StartupInfo.hStdOutput = inherited[1];
        startup.StartupInfo.hStdError = inherited[2];
        startup.lpAttributeList = attributes.pointer();
        let mut information = PROCESS_INFORMATION::default();
        // 显式 application 路径与 CRT 参数编码避免 shell 解析；NO_WINDOW 只禁止控制台，
        // 不隐藏工程随后在父视口内创建的图形窗口。挂起期间不可能派生逃逸子进程。
        if unsafe {
            CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_SUSPENDED | CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT
                    | if environment.is_some() { CREATE_UNICODE_ENVIRONMENT } else { 0 },
                environment.as_ref().map_or(null(), |value| value.as_ptr().cast()),
                working_directory.as_ref().map_or(null(), |value| value.as_ptr()),
                &startup.StartupInfo,
                &mut information,
            )
        } == 0
        {
            return Err(win_error("CreateProcessW").into());
        }
        let process = unsafe { OwnedHandle::from_raw_handle(information.hProcess) };
        let thread = unsafe { OwnedHandle::from_raw_handle(information.hThread) };
        if unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) } == 0 {
            let error = win_error("AssignProcessToJobObject");
            return Err(abort_start(&job, &process, false, error));
        }
        if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
            let error = win_error("ResumeThread");
            return Err(abort_start(&job, &process, true, error));
        }
        // 创建线程、白名单缓冲及子端在返回时关闭。父端不可继承，其他子进程不会
        // 延长本次管道/Job 的生命周期；返回后唯一 worker 拥有本次完整进程边界。
        Ok(SpawnedProcess {
            process: Self {
                job,
                process,
                pid: information.dwProcessId,
            },
            input: File::from(parent_input),
            output: File::from(parent_output),
            errors: File::from(parent_error),
            control: control.map(|((_, parent_write), (_, parent_read))| {
                (File::from(parent_write), File::from(parent_read))
            }),
        })
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// 先检查进程句柄是否 signaled；退出码 259 本身也可能是合法的最终退出码。
    pub fn try_exit(&self) -> Result<Option<u32>, String> {
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
                    Err(win_error("GetExitCodeProcess"))
                } else {
                    Ok(Some(code))
                }
            }
            _ => Err(win_error("WaitForSingleObject")),
        }
    }

    /// 只发出终止请求，不等待；调用者必须另行轮询 active_processes 直到归零。
    pub fn terminate(&self) -> Result<(), String> {
        if self.active_processes()? == 0 {
            return Ok(());
        }
        if unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) } == 0 {
            Err(win_error("TerminateJobObject"))
        } else {
            Ok(())
        }
    }

    pub fn active_processes(&self) -> Result<u32, String> {
        let mut information = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        if unsafe {
            QueryInformationJobObject(
                self.job.as_raw_handle(),
                JobObjectBasicAccountingInformation,
                (&mut information as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        } == 0
        {
            Err(win_error("QueryInformationJobObject"))
        } else {
            Ok(information.ActiveProcesses)
        }
    }
}

/// 接管 CLI 传来的专用子端，返回控制读、控制写；取得所有权后立即禁止再次继承。
///
/// # Safety
/// 两个值必须是本次 `spawn_runtime` 传入、仍然打开且不同的匿名管道子端，
/// 当前进程必须拥有它们的唯一关闭责任。每对句柄只能调用一次；错误返回也会关闭
/// 已接管的两个句柄。不得把其他打开文件、标准流或借用的句柄传给本函数。
#[cfg(test)] // 仅测试夹具把宿主自身作为子端启动；真实工程子端由 SDK 接管管道。
pub unsafe fn child_channels(read: usize, write: usize) -> Result<(File, File), String> {
    if read == 0 || write == 0 || read == usize::MAX || write == usize::MAX || read == write {
        return Err("专用控制管道需要两个有效且不同的继承句柄".into());
    }
    // SAFETY：唯一所有权由调用者保证；从此处起所有路径均以 RAII 归还句柄。
    let read = unsafe { OwnedHandle::from_raw_handle(read as HANDLE) };
    let write = unsafe { OwnedHandle::from_raw_handle(write as HANDLE) };
    for handle in [&read, &write] {
        if unsafe { SetHandleInformation(handle.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(win_error("SetHandleInformation(child control)"));
        }
    }
    Ok((File::from(read), File::from(write)))
}

/// true 为子端写/父端读，false 为子端读/父端写；只保留子端的可继承标记。
fn pipe(child_writes: bool) -> Result<(OwnedHandle, OwnedHandle), String> {
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    let mut read = null_mut();
    let mut write = null_mut();
    if unsafe { CreatePipe(&mut read, &mut write, &attributes, 64 * 1024) } == 0 {
        return Err(win_error("CreatePipe"));
    }
    let read = unsafe { OwnedHandle::from_raw_handle(read) };
    let write = unsafe { OwnedHandle::from_raw_handle(write) };
    let (child, parent) = if child_writes {
        (write, read)
    } else {
        (read, write)
    };
    if unsafe { SetHandleInformation(parent.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(win_error("SetHandleInformation"));
    }
    Ok((child, parent))
}

/// Win32 要求属性列表连续且对齐；usize 存储同时满足指针对齐和可变长度。
struct AttributeList {
    storage: Vec<usize>,
    initialized: bool,
}

impl AttributeList {
    fn new() -> Result<Self, String> {
        let mut bytes = 0;
        unsafe { InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut bytes) };
        if bytes == 0 {
            return Err(win_error("InitializeProcThreadAttributeList(size)"));
        }
        let mut value = Self {
            storage: vec![0; bytes.div_ceil(size_of::<usize>())],
            initialized: false,
        };
        if unsafe { InitializeProcThreadAttributeList(value.pointer(), 1, 0, &mut bytes) } == 0 {
            // 初始化失败时不可调用 Delete；此处只让原始存储自然释放。
            return Err(win_error("InitializeProcThreadAttributeList"));
        }
        value.initialized = true;
        Ok(value)
    }

    fn pointer(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for AttributeList {
    fn drop(&mut self) {
        if self.initialized {
            unsafe { DeleteProcThreadAttributeList(self.pointer()) };
        }
    }
}

fn abort_start(
    job: &OwnedHandle,
    process: &OwnedHandle,
    assigned: bool,
    mut error: String,
) -> SpawnFailure {
    // Assign 失败时仍是本次 CreateProcess 返回的挂起根，只对该精确句柄终止。
    // 入 Job 后则经 Job 收口。此函数只在后台 spawn 失败分支有限等待。
    let terminated = unsafe {
        if assigned {
            TerminateJobObject(job.as_raw_handle(), 1)
        } else {
            TerminateProcess(process.as_raw_handle(), 1)
        }
    };
    if terminated == 0 {
        error.push_str(&format!(
            "；启动失败收口请求失败：{}",
            std::io::Error::last_os_error()
        ));
    }
    let mut cleanup_unconfirmed = false;
    match unsafe { WaitForSingleObject(process.as_raw_handle(), 10_000) } {
        WAIT_OBJECT_0 => {}
        WAIT_TIMEOUT => {
            cleanup_unconfirmed = true;
            error.push_str("；启动失败收口超时，未确认根进程退出");
        }
        _ => {
            cleanup_unconfirmed = true;
            error.push_str(&format!("；{}", win_error("等待启动失败根进程")));
        }
    }
    if assigned {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        if unsafe { QueryInformationJobObject(job.as_raw_handle(), JobObjectBasicAccountingInformation,
            (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32, null_mut()) } == 0
        {
            cleanup_unconfirmed = true;
            error.push_str(&format!("；{}", win_error("确认启动失败Job收口")));
        } else if accounting.ActiveProcesses != 0 {
            cleanup_unconfirmed = true;
            error.push_str(&format!("；启动失败Job仍有{}个进程，未确认收口", accounting.ActiveProcesses));
        }
    }
    SpawnFailure { message: error, cleanup_unconfirmed }
}

fn nul_terminated(value: &OsStr) -> Result<Vec<u16>, String> {
    let mut value: Vec<_> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err("运行进程路径或参数不能包含 NUL".into());
    }
    value.push(0);
    Ok(value)
}

/// 编码 Microsoft CRT argv 规则：引号前反斜杠翻倍，末尾反斜杠也必须翻倍。
/// canonical身份已核验；短驱动器路径以普通Win32形式启动，避免SDK继承扩展前缀。
fn process_image_path(path: &Path) -> std::path::PathBuf {
    let wide: Vec<_> = path.as_os_str().encode_wide().collect();
    if wide.len() < 248
        && wide.starts_with(&[92, 92, 63, 92])
        && wide
            .get(4)
            .is_some_and(|c| matches!(*c, 65..=90 | 97..=122))
        && wide.get(5) == Some(&58)
        && wide.get(6) == Some(&92)
    {
        let ordinary = std::path::PathBuf::from(OsString::from_wide(&wide[4..]));
        // 普通Win32会裁剪尾点/空格；不能把已核验的A悄悄换成另一个可执行文件B。
        let aliases_component = ordinary.components().any(|part| {
            matches!(part, std::path::Component::Normal(name)
                if matches!(name.encode_wide().last(), Some(32 | 46)))
        });
        // 留在短路径范围，复验使用与CreateProcess一致的普通Win32解析，不扩大长路径承诺。
        if !aliases_component && ordinary.canonicalize().is_ok_and(|actual| actual == path) {
            ordinary
        } else {
            path.to_owned()
        }
    } else {
        path.to_owned()
    }
}

fn command_line(executable: &OsStr, args: &[OsString]) -> Result<Vec<u16>, String> {
    let mut result = Vec::new();
    for argument in std::iter::once(executable).chain(args.iter().map(OsString::as_os_str)) {
        if !result.is_empty() {
            result.push(b' ' as u16);
        }
        result.push(b'"' as u16);
        let mut slashes = 0;
        for word in argument.encode_wide() {
            match word {
                0 => return Err("运行进程路径或参数不能包含 NUL".into()),
                92 => slashes += 1,
                34 => {
                    result.extend(std::iter::repeat_n(92, slashes * 2 + 1));
                    result.push(34);
                    slashes = 0;
                }
                _ => {
                    result.extend(std::iter::repeat_n(92, slashes));
                    result.push(word);
                    slashes = 0;
                }
            }
            if result.len() + slashes + 2 > 32_767 {
                return Err("运行进程命令行超过 Win32 的 32767 UTF-16 单元上限".into());
            }
        }
        result.extend(std::iter::repeat_n(92, slashes * 2));
        result.push(b'"' as u16);
        if result.len() + 1 > 32_767 {
            return Err("运行进程命令行超过 Win32 的 32767 UTF-16 单元上限".into());
        }
    }
    result.push(0);
    Ok(result)
}

/// bare工具名只从本次环境的PATH查找，不让工程工作目录中的同名文件抢占Cargo/rustc。
pub(crate) fn command_executable(command: &Command) -> Result<PathBuf, String> {
    let program = Path::new(command.get_program());
    let mut recovered_directories = Vec::new();
    let candidates = if program.is_absolute() || program.components().count() > 1 {
        vec![if program.is_absolute() { program.to_owned() } else {
            command.get_current_dir().map(Path::to_owned)
                .unwrap_or(std::env::current_dir().map_err(|e| e.to_string())?).join(program)
        }]
    } else {
        let overridden_path = command.get_envs().find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case("PATH"));
        let path = match overridden_path {
            Some((_, value)) => value.map(OsStr::to_owned),
            None => std::env::var_os("PATH"),
        }.ok_or("工具环境缺少PATH，必须提供EXE绝对路径")?;
        recovered_directories = recover_unclosed_path_tail(&path);
        std::env::split_paths(&path).filter(|directory| directory.is_absolute())
            .map(|directory| directory.join(program)).collect()
    };
    // 标准解析的全部候选优先；仅标准未命中时才尝试坏引号之后的完整PATH项。
    for mut candidate in candidates.into_iter().chain(recovered_directories.into_iter().map(|directory|directory.join(program))) {
        if candidate.extension().is_none() { candidate.set_extension("exe"); }
        if candidate.extension().is_none_or(|extension| !extension.to_string_lossy().eq_ignore_ascii_case("exe")) { continue; }
        if let Ok(path) = candidate.canonicalize() {
            if path.is_file() { return Ok(path); }
        }
    }
    Err(format!("找不到原生工具EXE：{}", program.display()))
}

/// 本机PATH的第三方目录项含有未配对引号，会使std::split_paths吞掉其后所有分号。
/// 不改全局环境，也不拆合法的带分号引号目录；只恢复最后一个坏引号之后的完整绝对项。
fn recover_unclosed_path_tail(path: &OsStr) -> Vec<PathBuf> {
    let units: Vec<_> = path.encode_wide().collect();
    if units.iter().filter(|&&unit|unit == 34).count() % 2 == 0 { return Vec::new(); }
    let last_quote = units.iter().rposition(|&unit|unit == 34).expect("奇数引号必有末项");
    units[last_quote + 1..].split(|&unit|unit == 59)
        // 引号所属项已损坏，不能将残片变成新搜索根；从下一完整分号项开始恢复。
        .skip(1).filter(|part|!part.is_empty()).map(OsString::from_wide).map(PathBuf::from)
        .filter(|directory|directory.is_absolute()).collect()
}

fn command_environment(command: &Command) -> Result<Vec<u16>, String> {
    let mut environment = BTreeMap::new();
    for (name, value) in std::env::vars_os() {
        environment.insert(name.to_string_lossy().to_uppercase(), (name, value));
    }
    for (name, value) in command.get_envs() {
        let key = name.to_string_lossy().to_uppercase();
        if let Some(value) = value { environment.insert(key, (name.to_owned(), value.to_owned())); }
        else { environment.remove(&key); }
    }
    let mut block = Vec::new();
    for (_, (name, value)) in environment {
        let mut entry = name;
        entry.push("="); entry.push(value);
        block.extend(nul_terminated(&entry)?);
    }
    // Win32环境块以双NUL结束；空块也必须保留两个单元。
    if block.is_empty() { block.push(0); }
    block.push(0);
    Ok(block)
}

fn win_error(operation: &str) -> String {
    format!("{operation} 失败：{}", std::io::Error::last_os_error())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_recovery_preserves_valid_quotes_and_only_recovers_absolute_tail_entries() {
        let valid = OsStr::new(r#""C:\semi;literal";D:\normal"#);
        assert!(recover_unclosed_path_tail(valid).is_empty());
        assert_eq!(std::env::split_paths(valid).collect::<Vec<_>>(),
            vec![PathBuf::from(r"C:\semi;literal"), PathBuf::from(r"D:\normal")]);
        let unclosed = OsStr::new(r#"C:\normal;"D:\semi;literal";F:\broken";relative;\rooted;G:\cargo;;H:\rust"#);
        assert_eq!(recover_unclosed_path_tail(unclosed), vec![PathBuf::from(r"G:\cargo"),PathBuf::from(r"H:\rust")]);
        assert!(recover_unclosed_path_tail(OsStr::new(r#"C:\normal;D:\broken"tail-without-next-item"#)).is_empty());
    }

    #[test]
    fn bare_cargo_and_rustc_resolve_after_unclosed_quote_with_standard_candidates_first() {
        let root = std::env::temp_dir().join(format!("editor-tool-path-{}-{}", std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let primary = root.join("primary");let recovery = root.join("recovery");
        std::fs::create_dir(&primary).unwrap();std::fs::create_dir(&recovery).unwrap();
        for tool in ["cargo.exe", "rustc.exe"] {
            // 只验证定位，不执行这些假EXE，也不依赖机器实际Rust路径或修改全局环境。
            std::fs::write(primary.join(tool), b"resolver-only").unwrap();
            std::fs::write(recovery.join(tool), b"resolver-only").unwrap();
        }
        for tool in ["cargo", "rustc"] {
            let mut command = Command::new(tool);
            command.env("PATH", format!(r#"{};C:\broken";{}"#, primary.display(), recovery.display()));
            assert_eq!(command_executable(&command).unwrap(), primary.join(format!("{tool}.exe")).canonicalize().unwrap());
            command.env("PATH", format!(r#"C:\broken";{}"#, recovery.display()));
            assert_eq!(command_executable(&command).unwrap(), recovery.join(format!("{tool}.exe")).canonicalize().unwrap());
        }
        for directory in [&primary, &recovery] {
            std::fs::remove_file(directory.join("cargo.exe")).unwrap();
            std::fs::remove_file(directory.join("rustc.exe")).unwrap();
            std::fs::remove_dir(directory).unwrap();
        }
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn sdk_image_path_normalizes_short_drive_paths_and_preserves_long_or_unc_paths() {
        let image = std::env::current_exe().unwrap().canonicalize().unwrap();
        let ordinary = process_image_path(&image);
        assert_eq!(ordinary.canonicalize().unwrap(), image);
        assert!(
            !ordinary
                .as_os_str()
                .encode_wide()
                .collect::<Vec<_>>()
                .starts_with(&[92, 92, 63, 92])
        );
        let unc = Path::new(r"\\?\UNC\server\share\feathrise.exe");
        assert_eq!(process_image_path(unc), unc);
        let long = format!(r"\\?\D:\{}\feathrise.exe", "a".repeat(260));
        assert_eq!(process_image_path(Path::new(&long)), Path::new(&long));
    }

    #[test]
    fn sdk_image_path_preserves_identity_when_win32_would_trim_names() {
        let image = std::env::current_exe().unwrap().canonicalize().unwrap();
        for suffix in [".", " "] {
            let mut alias = image.as_os_str().to_owned();
            alias.push(suffix);
            let alias = std::path::PathBuf::from(alias);
            assert_eq!(process_image_path(&alias), alias);
        }
        let missing = Path::new(r"\\?\D:\missing-editor-host-fixture-7acb\feathrise.exe");
        assert_eq!(process_image_path(missing), missing);
    }
    use std::{
        io::{BufRead, Read, Write},
        process::{Command, Stdio},
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };

    const CHILD_MARKER: &str = "editor-job-child-only-051b";

    fn helper_args(name: &str) -> Vec<OsString> {
        [
            "--ignored",
            "--exact",
            name,
            "--nocapture",
            "--test-threads=1",
            "--skip",
            CHILD_MARKER,
        ]
        .into_iter()
        .map(OsString::from)
        .collect()
    }

    fn is_child() -> bool {
        std::env::args_os().any(|value| value == CHILD_MARKER)
    }

    fn wait_until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition() {
            assert!(
                Instant::now() < deadline,
                "专属测试 Job 在 5 秒内未达到预期状态"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn ready_reader(
        reader: File,
        marker: &'static str,
    ) -> (mpsc::Receiver<u32>, thread::JoinHandle<()>) {
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            for line in std::io::BufReader::new(reader).lines() {
                let line = line.unwrap();
                if let Some(pid) = line
                    .strip_prefix(marker)
                    .and_then(|value| value.parse().ok())
                {
                    let _ = sender.try_send(pid);
                }
            }
        });
        (receiver, worker)
    }

    fn job_ids(process: &JobProcess) -> Vec<u32> {
        use windows_sys::Win32::System::JobObjects::JobObjectBasicProcessIdList;
        #[repr(C)]
        struct Pids {
            assigned: u32,
            listed: u32,
            ids: [usize; 16],
        }
        let mut pids = Pids {
            assigned: 0,
            listed: 0,
            ids: [0; 16],
        };
        assert_ne!(
            unsafe {
                QueryInformationJobObject(
                    process.job.as_raw_handle(),
                    JobObjectBasicProcessIdList,
                    (&mut pids as *mut Pids).cast(),
                    size_of::<Pids>() as u32,
                    null_mut(),
                )
            },
            0
        );
        let mut result: Vec<_> = pids.ids[..pids.listed as usize]
            .iter()
            .map(|&pid| pid as u32)
            .collect();
        result.sort_unstable();
        result
    }

    fn assert_member(process_handle: &OwnedHandle, job: &JobProcess, expected: bool) {
        use windows_sys::Win32::System::JobObjects::IsProcessInJob;
        let mut member = 0;
        assert_ne!(
            unsafe {
                IsProcessInJob(
                    process_handle.as_raw_handle(),
                    job.job.as_raw_handle(),
                    &mut member,
                )
            },
            0
        );
        assert_eq!(member != 0, expected);
    }

    #[test]
    fn quotes_empty_unicode_quotes_and_trailing_backslashes() {
        let args: Vec<OsString> = ["", "中文 路径", "a\"b", "tail\\", "slash\\\"quote"]
            .into_iter()
            .map(OsString::from)
            .collect();
        let encoded = command_line(OsStr::new("C:\\game.exe"), &args).unwrap();
        let actual = String::from_utf16(&encoded[..encoded.len() - 1]).unwrap();
        assert_eq!(
            actual,
            "\"C:\\game.exe\" \"\" \"中文 路径\" \"a\\\"b\" \"tail\\\\\" \"slash\\\\\\\"quote\""
        );
        assert!(command_line(OsStr::new("game.exe"), &["x\0y".into()]).is_err());
        assert!(command_line(OsStr::new("game.exe"), &["x".repeat(32_768).into()]).is_err());
        assert!(
            JobProcess::spawn(Path::new("relative.exe"), &[])
                .err()
                .unwrap()
                .contains("绝对路径")
        );
    }

    #[test]
    fn suspended_job_stdio_roundtrip_and_real_exit_code() {
        let mut args = helper_args("native_process::windows::tests::stdio_child_helper");
        for value in ["", "中文 带空格", "a\"b", "tail\\"] {
            // --exact 使这些额外过滤值不匹配其他测试；空 --skip 会排除所有测试，
            // 因此用 libtest 支持的并列精确过滤值验证真实 argv 编解码。
            args.push(value.into());
        }
        let (process, mut input, mut output, mut errors) =
            JobProcess::spawn(&std::env::current_exe().unwrap(), &args).unwrap();
        assert_ne!(process.pid(), std::process::id());
        let output_reader = thread::spawn(move || {
            let mut bytes = String::new();
            output.read_to_string(&mut bytes).unwrap();
            bytes
        });
        let error_reader = thread::spawn(move || {
            let mut bytes = String::new();
            errors.read_to_string(&mut bytes).unwrap();
            bytes
        });
        writeln!(input, "中文标准输入").unwrap();
        drop(input);
        wait_until(|| process.try_exit().unwrap().is_some());
        assert_eq!(process.try_exit().unwrap(), Some(259));
        wait_until(|| process.active_processes().unwrap() == 0);
        assert!(
            output_reader
                .join()
                .unwrap()
                .contains("PIPE_OUT:中文标准输入")
        );
        assert!(error_reader.join().unwrap().contains("PIPE_ERROR"));
    }

    #[test]
    fn dedicated_control_survives_arbitrary_stdout_without_touching_other_job() {
        let mut args = helper_args("native_process::windows::tests::dedicated_control_child_helper");
        // libtest 的 -- 之后都是过滤值，让测试子入口能够读取真正的 IPC CLI 参数。
        args.push("--".into());
        let (process, mut control_write, mut control_read, mut output, mut errors) =
            JobProcess::spawn_runtime(&std::env::current_exe().unwrap(), &args).unwrap();
        let output_reader = thread::spawn(move || {
            let mut text = String::new();
            output.read_to_string(&mut text).unwrap();
            text
        });
        let error_reader = thread::spawn(move || {
            let mut text = String::new();
            errors.read_to_string(&mut text).unwrap();
            text
        });
        let control_reader = thread::spawn(move || {
            let mut text = String::new();
            control_read.read_to_string(&mut text).unwrap();
            text
        });
        let sibling_args = helper_args("native_process::windows::tests::sleeping_child_helper");
        let (sibling, _sibling_input, sibling_output, mut sibling_errors) =
            JobProcess::spawn(&std::env::current_exe().unwrap(), &sibling_args).unwrap();
        let (sibling_ready, sibling_output_reader) = ready_reader(sibling_output, "JOB_ROOT:");
        let sibling_error_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            sibling_errors.read_to_end(&mut bytes).unwrap();
        });
        assert_eq!(
            sibling_ready.recv_timeout(Duration::from_secs(5)).unwrap(),
            sibling.pid()
        );
        let sibling_ids = job_ids(&sibling);
        assert_member(&process.process, &process, true);
        assert_member(&process.process, &sibling, false);
        assert_member(&sibling.process, &process, false);
        writeln!(control_write, "独立控制:会话-729").unwrap();
        drop(control_write);
        wait_until(|| process.try_exit().unwrap().is_some());
        assert_eq!(process.try_exit().unwrap(), Some(73));
        wait_until(|| process.active_processes().unwrap() == 0);
        assert_eq!(
            control_reader.join().unwrap(),
            "CONTROL_REPLY:独立控制:会话-729\n"
        );
        let output = output_reader.join().unwrap();
        // 超过现有 IPC 单行上限的普通日志及伪 JSON 均只出现在日志流。
        assert!(output.contains(&"x".repeat(40_000)));
        assert!(output.contains("{\"kind\":\"ordinary-log-not-control\"}"));
        assert!(error_reader.join().unwrap().contains("DEDICATED_STDERR"));
        process.terminate().unwrap();
        assert_eq!(sibling.try_exit().unwrap(), None);
        assert_eq!(job_ids(&sibling), sibling_ids);
        sibling.terminate().unwrap();
        wait_until(|| sibling.active_processes().unwrap() == 0);
        wait_until(|| sibling.try_exit().unwrap().is_some());
        sibling_output_reader.join().unwrap();
        sibling_error_reader.join().unwrap();
    }

    #[test]
    fn terminate_reclaims_only_own_job_including_grandchild() {
        let args = helper_args("native_process::windows::tests::tree_child_helper");
        let (process, input, output, errors) =
            JobProcess::spawn(&std::env::current_exe().unwrap(), &args).unwrap();
        let sibling_args = helper_args("native_process::windows::tests::sleeping_child_helper");
        let (sibling, _sibling_input, sibling_output, mut sibling_errors) =
            JobProcess::spawn(&std::env::current_exe().unwrap(), &sibling_args).unwrap();
        drop(input);
        let (tree_ready, output_reader) = ready_reader(output, "JOB_GRANDCHILD:");
        let (sibling_ready, sibling_output_reader) = ready_reader(sibling_output, "JOB_ROOT:");
        let sibling_error_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            sibling_errors.read_to_end(&mut bytes).unwrap();
        });
        let error_reader = thread::spawn(move || {
            let mut bytes = Vec::new();
            std::io::BufReader::new(errors)
                .read_to_end(&mut bytes)
                .unwrap();
        });
        // Windows 可能把无窗口 conhost.exe 计入 Job；成员数 >=2 不能证明显式孙进程
        // 已启动。由受控 helper 回报精确 PID，再以已持有句柄核验两个 Job 互不包含。
        let grandchild_pid = tree_ready.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            sibling_ready.recv_timeout(Duration::from_secs(5)).unwrap(),
            sibling.pid()
        );
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let grandchild =
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, grandchild_pid) };
        assert!(!grandchild.is_null());
        let grandchild = unsafe { OwnedHandle::from_raw_handle(grandchild) };
        assert_member(&process.process, &process, true);
        assert_member(&grandchild, &process, true);
        assert_member(&sibling.process, &sibling, true);
        assert_member(&process.process, &sibling, false);
        assert_member(&grandchild, &sibling, false);
        assert_member(&sibling.process, &process, false);
        let target_ids = job_ids(&process);
        assert!(target_ids.contains(&process.pid()));
        assert!(target_ids.contains(&grandchild_pid));
        assert!(!target_ids.contains(&sibling.pid()));
        let sibling_ids = job_ids(&sibling);
        assert!(sibling_ids.contains(&sibling.pid()));
        assert!(!sibling_ids.contains(&process.pid()));
        assert!(!sibling_ids.contains(&grandchild_pid));
        assert_eq!(process.try_exit().unwrap(), None);
        process.terminate().unwrap();
        wait_until(|| process.active_processes().unwrap() == 0);
        wait_until(|| process.try_exit().unwrap().is_some());
        process.terminate().unwrap();
        // 对照进程属于另一独占 Job：收口被测父子树不能影响它。
        assert_eq!(sibling.try_exit().unwrap(), None);
        assert_eq!(job_ids(&sibling), sibling_ids);
        assert_eq!(
            sibling.active_processes().unwrap() as usize,
            sibling_ids.len()
        );
        sibling.terminate().unwrap();
        wait_until(|| sibling.active_processes().unwrap() == 0);
        wait_until(|| sibling.try_exit().unwrap().is_some());
        output_reader.join().unwrap();
        error_reader.join().unwrap();
        sibling_output_reader.join().unwrap();
        sibling_error_reader.join().unwrap();
    }

    #[test]
    #[ignore = "仅供本模块通过专属 Job 创建的子测试进程调用"]
    fn stdio_child_helper() {
        if !is_child() {
            return;
        }
        assert!(unsafe { windows_sys::Win32::System::Console::GetConsoleWindow() }.is_null());
        let args: Vec<_> = std::env::args_os().collect();
        for expected in ["", "中文 带空格", "a\"b", "tail\\"] {
            assert!(args.iter().any(|actual| actual == expected));
        }
        let mut line = String::new();
        std::io::stdin().lock().read_line(&mut line).unwrap();
        print!("PIPE_OUT:{line}");
        std::io::stdout().flush().unwrap();
        eprintln!("PIPE_ERROR");
        // STILL_ACTIVE 的数值也可被程序作为真实退出码，必须用 signaled 句柄判定。
        std::process::exit(259);
    }

    #[test]
    #[ignore = "仅供本模块通过专属 Job 创建的子测试进程调用"]
    fn dedicated_control_child_helper() {
        if !is_child() {
            return;
        }
        let arguments: Vec<_> = std::env::args().collect();
        let handle = |name: &str| {
            arguments
                .iter()
                .position(|argument| argument == name)
                .and_then(|index| arguments.get(index + 1))
                .unwrap()
                .parse::<usize>()
                .unwrap()
        };
        // SAFETY：此入口只由上方 spawn_runtime 启动，各继承管道从 CLI 唯一接管。
        let (control_read, mut control_write) =
            unsafe { child_channels(handle("--ipc-read"), handle("--ipc-write")).unwrap() };
        for stream in [&control_read, &control_write] {
            let mut flags = 0;
            assert_ne!(
                unsafe {
                    windows_sys::Win32::Foundation::GetHandleInformation(
                        stream.as_raw_handle(),
                        &mut flags,
                    )
                },
                0
            );
            assert_eq!(flags & HANDLE_FLAG_INHERIT, 0);
        }
        let mut standard_input = String::new();
        std::io::stdin()
            .read_to_string(&mut standard_input)
            .unwrap();
        assert!(standard_input.is_empty(), "控制输入不能占用标准输入");
        println!("guest println!:{}", "x".repeat(40_000));
        println!("{{\"kind\":\"ordinary-log-not-control\"}}");
        std::io::stdout().flush().unwrap();
        eprintln!("DEDICATED_STDERR");
        let mut control = std::io::BufReader::new(control_read);
        let mut line = String::new();
        control.read_line(&mut line).unwrap();
        assert_eq!(line, "独立控制:会话-729\n");
        let mut remainder = String::new();
        control.read_to_string(&mut remainder).unwrap();
        assert!(remainder.is_empty(), "父写端关闭后专用读端应立即获得 EOF");
        write!(control_write, "CONTROL_REPLY:{line}").unwrap();
        control_write.flush().unwrap();
        // 显式关闭后退出，父方独立控制读端无需等待无关 Job 的生命周期。
        drop(control_write);
        std::process::exit(73);
    }

    #[test]
    #[ignore = "仅供本模块通过专属 Job 创建的子测试进程调用"]
    fn tree_child_helper() {
        if !is_child() {
            return;
        }
        use std::os::windows::process::CommandExt;
        let grandchild = Command::new(std::env::current_exe().unwrap())
            .args(helper_args("native_process::windows::tests::sleeping_child_helper"))
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        println!("\nJOB_GRANDCHILD:{}", grandchild.id());
        std::io::stdout().flush().unwrap();
        thread::sleep(Duration::from_secs(30));
    }

    #[test]
    #[ignore = "仅供本模块通过专属 Job 创建的子测试进程调用"]
    fn sleeping_child_helper() {
        if is_child() {
            println!("\nJOB_ROOT:{}", std::process::id());
            std::io::stdout().flush().unwrap();
            thread::sleep(Duration::from_secs(30));
        }
    }
}
